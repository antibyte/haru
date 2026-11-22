//! Worker child process (Task 0.3).
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::{ErrorCode, HaruError};
use crate::messages::MsgKey;
use crate::worker::protocol::{check_ready, parse_line, worker_error, ClientMsg, Ready, WorkerMsg};

pub const VOICE_NAME: &str = "user";
pub const READY_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub exe: PathBuf,
    pub model: PathBuf,
    pub device: String,
    pub voice: Option<PathBuf>,
    pub log_path: PathBuf,
}

pub fn worker_args(cfg: &WorkerConfig) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec![
        "worker".into(),
        cfg.model.clone().into_os_string(),
        "--device".into(),
        cfg.device.clone().into(),
    ];
    if let Some(v) = &cfg.voice {
        a.push("--add-voice".into());
        let mut kv = OsString::from(format!("{VOICE_NAME}="));
        kv.push(v);
        a.push(kv);
    }
    a
}

#[derive(Debug)]
pub enum WorkerEvent {
    Message(WorkerMsg),
    Malformed { line: String },
    Exited { code: Option<i32> },
}

/// Maximum number of queued, not yet written messages. A healthy worker drains it instantly; if
/// it is full the worker stopped reading and `send` fails instead of blocking.
pub const SEND_QUEUE: usize = 512;

/// Writes happen on a dedicated thread fed by a bounded queue, so `send` never blocks on the pipe
/// and no lock is held across a blocking write.
#[derive(Clone)]
pub struct WorkerSender {
    tx: Arc<Mutex<Option<SyncSender<String>>>>,
}

impl WorkerSender {
    /// Starts the writer thread over `w`. The thread ends when the queue is closed or a write fails.
    pub(crate) fn with_writer<W: Write + Send + 'static>(mut w: W, capacity: usize) -> WorkerSender {
        let (tx, rx) = mpsc::sync_channel::<String>(capacity);
        std::thread::Builder::new()
            .name("worker-writer".into())
            .spawn(move || {
                for line in rx {
                    if w.write_all(line.as_bytes()).and_then(|_| w.flush()).is_err() {
                        return; // dropping `rx` makes later sends fail
                    }
                }
            })
            .expect("spawn writer thread");
        WorkerSender { tx: Arc::new(Mutex::new(Some(tx))) }
    }

    /// Queues one line. Never blocks: a closed pipe or a full queue is a Worker error with
    /// `backend_failure`.
    pub fn send(&self, m: &ClientMsg) -> Result<(), HaruError> {
        let broken = |d: &str| {
            HaruError::new(ErrorCode::Worker, MsgKey::WorkerConnectionLost)
                .with_detail(d.to_string())
                .backend_failure()
        };
        let g = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        let tx = g.as_ref().ok_or_else(|| broken("worker stdin closed"))?;
        tx.try_send(m.to_line()).map_err(|e| match e {
            TrySendError::Full(_) => broken("worker stopped reading (send queue full)"),
            TrySendError::Disconnected(_) => broken("worker stdin write failed"),
        })
    }

    /// Closes the queue; the writer thread drops stdin once it has written what is queued.
    fn close(&self) {
        *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

pub struct WorkerProcess {
    child: Child,
    sender: WorkerSender,
    rx: Option<Receiver<WorkerEvent>>,
    log_path: PathBuf,
}

impl WorkerProcess {
    pub fn spawn(cfg: &WorkerConfig) -> Result<WorkerProcess, HaruError> {
        if let Some(dir) = cfg.log_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let log = File::create(&cfg.log_path)?;
        let mut cmd = Command::new(&cfg.exe);
        cmd.args(worker_args(cfg))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log))
            .env("SPEECH_NO_UPDATE_CHECK", "1");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        #[cfg(target_os = "linux")]
        if let Some(dir) = cfg.exe.parent() {
            let mut paths = vec![dir.to_path_buf()];
            if let Some(old) = std::env::var_os("LD_LIBRARY_PATH") {
                paths.extend(std::env::split_paths(&old));
            }
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env("LD_LIBRARY_PATH", joined);
            }
        }
        let child = cmd.spawn().map_err(|e| {
            HaruError::new(ErrorCode::Worker, MsgKey::WorkerStartFailed)
                .with_detail(format!("{}: {e}", cfg.exe.display()))
                .backend_failure()
        })?;
        #[cfg(windows)]
        crate::worker::job::assign(&child);
        Ok(Self::from_child(child, cfg.log_path.clone()))
    }

    /// Wires reader and writer threads to a spawned child with piped stdin/stdout.
    fn from_child(mut child: Child, log_path: PathBuf) -> WorkerProcess {
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("worker-reader".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if line.trim().is_empty() {
                        continue;
                    }
                    let ev = match parse_line(&line) {
                        Ok(m) => WorkerEvent::Message(m),
                        Err(_) => WorkerEvent::Malformed { line },
                    };
                    if tx.send(ev).is_err() {
                        return;
                    }
                }
                let _ = tx.send(WorkerEvent::Exited { code: None });
            })
            .expect("spawn reader thread");
        WorkerProcess { child, sender: WorkerSender::with_writer(stdin, SEND_QUEUE), rx: Some(rx), log_path }
    }

    /// Wait for the `ready` message (validated with `check_ready`).
    pub fn wait_ready(&mut self, timeout: Duration) -> Result<Ready, HaruError> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let rx = self.rx.as_ref().expect("worker events already taken");
            match rx.recv_timeout(left) {
                Ok(WorkerEvent::Message(WorkerMsg::Ready(r))) => {
                    check_ready(&r)?;
                    return Ok(r);
                }
                Ok(WorkerEvent::Message(WorkerMsg::Fatal { error })) => {
                    let e = worker_error(&error).backend_failure();
                    let d = e.detail.clone().unwrap_or_default();
                    return Err(e.with_detail(format!("{d} | stderr: {}", self.stderr_tail(2000))));
                }
                Ok(WorkerEvent::Message(_)) | Ok(WorkerEvent::Malformed { .. }) => continue,
                Ok(WorkerEvent::Exited { .. }) | Err(RecvTimeoutError::Disconnected) => {
                    let code = self.child.wait().ok().and_then(|s| s.code());
                    return Err(HaruError::new(ErrorCode::Worker, MsgKey::WorkerExited)
                        .with_detail(format!("exit code {code:?}; stderr: {}", self.stderr_tail(2000)))
                        .backend_failure());
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(HaruError::new(ErrorCode::Worker, MsgKey::WorkerNotReady)
                        .with_detail(format!("timeout after {timeout:?}; stderr: {}", self.stderr_tail(2000)))
                        .backend_failure());
                }
            }
        }
    }

    pub fn sender(&self) -> WorkerSender {
        self.sender.clone()
    }

    /// Once; panics if taken twice.
    pub fn take_events(&mut self) -> Receiver<WorkerEvent> {
        self.rx.take().expect("worker events already taken")
    }

    pub fn stderr_tail(&self, max_bytes: usize) -> String {
        let Ok(mut f) = File::open(&self.log_path) else { return String::new() };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if f.seek(SeekFrom::Start(len.saturating_sub(max_bytes as u64))).is_err() {
            return String::new();
        }
        let mut buf = Vec::new();
        let _ = f.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// Close stdin (the worker ends the conversation and exits, F11), wait up to `grace`, then kill.
    /// Killing the child also fails a write the writer thread may be blocked in; no lock that a
    /// blocked writer could hold is taken here.
    pub fn shutdown(&mut self, grace: Duration) {
        self.sender.close();
        let deadline = Instant::now() + grace;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                _ => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for WorkerProcess {
    fn drop(&mut self) {
        self.shutdown(Duration::from_secs(2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn cfg(voice: Option<&str>) -> WorkerConfig {
        WorkerConfig {
            exe: PathBuf::from("Z:/nope/speech.exe"),
            model: PathBuf::from("C:/m/h.gguf"),
            device: "Vulkan1".into(),
            voice: voice.map(PathBuf::from),
            log_path: std::env::temp_dir().join("haru-test-worker.log"),
        }
    }
    fn strs(v: Vec<OsString>) -> Vec<String> {
        v.into_iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn worker_args_without_voice() {
        assert_eq!(strs(worker_args(&cfg(None))), ["worker", "C:/m/h.gguf", "--device", "Vulkan1"]);
    }

    #[test]
    fn worker_args_with_voice() {
        assert_eq!(
            strs(worker_args(&cfg(Some("C:/v/me.wav")))),
            ["worker", "C:/m/h.gguf", "--device", "Vulkan1", "--add-voice", "user=C:/v/me.wav"]
        );
    }

    #[test]
    fn spawn_missing_exe_is_worker_error() {
        let e = WorkerProcess::spawn(&cfg(None)).err().expect("must fail");
        assert_eq!(e.code, ErrorCode::Worker);
    }

    struct BlockedWriter(Arc<(Mutex<bool>, std::sync::Condvar)>);
    impl Write for BlockedWriter {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            let (m, cv) = &*self.0;
            let mut open = m.lock().unwrap();
            while !*open {
                open = cv.wait(open).unwrap();
            }
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn msg() -> ClientMsg {
        ClientMsg::Cancel { id: "c1".into() }
    }

    #[test]
    fn send_does_not_block_when_writer_is_stuck() {
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let s = WorkerSender::with_writer(BlockedWriter(gate.clone()), 4);
        let t0 = Instant::now();
        let mut failed = None;
        for i in 0..50 {
            if let Err(e) = s.send(&msg()) {
                failed = Some((i, e));
                break;
            }
        }
        assert!(t0.elapsed() < Duration::from_secs(2), "send blocked");
        let (i, e) = failed.expect("queue must fill up");
        assert!(i <= 6, "failed after {i}");
        assert_eq!(e.code, ErrorCode::Worker);
        assert!(e.backend_failure);
        // closing must not wait for the stuck writer either
        s.close();
        assert!(s.send(&msg()).is_err());
        let (m, cv) = &*gate;
        *m.lock().unwrap() = true;
        cv.notify_all();
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn shutdown_completes_with_worker_that_does_not_read() {
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("ping");
            c.args(["-n", "60", "127.0.0.1"]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg("60");
            c
        };
        let child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().expect("spawn");
        let mut w = WorkerProcess::from_child(child, std::env::temp_dir().join("haru-test-worker2.log"));
        let s = w.sender();
        let big = ClientMsg::Chunk { id: "c".into(), seq: 0, pcm: "A".repeat(64 * 1024) };
        let t0 = Instant::now();
        let mut failed = false;
        for _ in 0..SEND_QUEUE + 8 {
            if s.send(&big).is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed, "queue never filled");
        assert!(t0.elapsed() < Duration::from_secs(5), "send blocked");
        let t1 = Instant::now();
        w.shutdown(Duration::from_millis(300));
        assert!(t1.elapsed() < Duration::from_secs(3), "shutdown took {:?}", t1.elapsed());
        assert!(w.child.try_wait().unwrap().is_some(), "child still running");
    }
}
