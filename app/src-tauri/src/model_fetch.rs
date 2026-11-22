//! Resumable, verified model download (Task 1.2).
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{ErrorCode, HaruError};
use crate::messages::MsgKey;
use crate::ipc::DownloadEvent;
use crate::paths::partial_path;

pub struct FetchConfig {
    pub url: String,
    pub dest: PathBuf,
    pub size: u64,
    pub sha256: String,
}

pub type DownloadSink = Arc<dyn Fn(DownloadEvent) + Send + Sync>;
pub type FreeSpace = fn(&Path) -> std::io::Result<u64>;

pub const FREE_MARGIN: u64 = 64 * 1024 * 1024;
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
pub const STALL_TIMEOUT: Duration = Duration::from_secs(30);
pub const RETRY_DELAYS: [Duration; 3] = [Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(4)];

const RATE_WINDOW: Duration = Duration::from_millis(500);
const RATE_ALPHA: f64 = 0.3;
const HASH_CHUNK: usize = 1024 * 1024;

/// True when `dest` is a file of exactly `cfg.size` bytes.
pub fn is_present(cfg: &FetchConfig) -> bool {
    std::fs::metadata(&cfg.dest).map(|m| m.is_file() && m.len() == cfg.size).unwrap_or(false)
}

fn real_free_space(p: &Path) -> std::io::Result<u64> {
    fs2::available_space(p)
}

fn default_client() -> Result<reqwest::Client, HaruError> {
    reqwest::Client::builder()
        .user_agent("haru-app/0.1")
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| HaruError::new(ErrorCode::Download, MsgKey::DownloadStartFailed).with_detail(e.to_string()))
}

pub async fn ensure_model(cfg: &FetchConfig, sink: DownloadSink) -> Result<PathBuf, HaruError> {
    let client = default_client()?;
    ensure_model_with(cfg, sink, real_free_space, &client, &RETRY_DELAYS).await
}

enum Failure {
    /// Network or stream problem: resume from the current `.part` after a delay.
    Retry(String),
    Fatal(HaruError),
}

impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        Failure::Fatal(e.into())
    }
}

struct Throttle {
    last: Option<Instant>,
}

impl Throttle {
    fn new() -> Self {
        Throttle { last: None }
    }
    fn ready(&mut self) -> bool {
        let now = Instant::now();
        match self.last {
            Some(t) if now.duration_since(t) < PROGRESS_INTERVAL => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn ensure_model_with(
    cfg: &FetchConfig,
    sink: DownloadSink,
    free: FreeSpace,
    client: &reqwest::Client,
    retry_delays: &[Duration],
) -> Result<PathBuf, HaruError> {
    // Step 1: a file of the exact size is trusted (only a verified rename creates it).
    if is_present(cfg) {
        sink(DownloadEvent::Done { path: cfg.dest.to_string_lossy().into_owned() });
        return Ok(cfg.dest.clone());
    }
    let fail = |e: HaruError| -> HaruError {
        sink(DownloadEvent::Error(e.text()));
        e
    };
    if cfg.dest.exists() {
        if let Err(e) = tokio::fs::remove_file(&cfg.dest).await {
            return Err(fail(e.into()));
        }
    }
    if let Some(parent) = cfg.dest.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return Err(fail(e.into()));
        }
    }

    let mut attempt = 0usize;
    loop {
        match attempt_download(cfg, &sink, free, client).await {
            Ok(path) => return Ok(path),
            Err(Failure::Fatal(e)) => return Err(fail(e)),
            Err(Failure::Retry(detail)) => {
                if attempt >= retry_delays.len() {
                    return Err(fail(
                        HaruError::new(ErrorCode::Download, MsgKey::DownloadFailed)
                            .with_detail(detail),
                    ));
                }
                tokio::time::sleep(retry_delays[attempt]).await;
                attempt += 1;
            }
        }
    }
}

async fn attempt_download(
    cfg: &FetchConfig,
    sink: &DownloadSink,
    free: FreeSpace,
    client: &reqwest::Client,
) -> Result<PathBuf, Failure> {
    let part = partial_path(&cfg.dest);
    let parent = cfg.dest.parent().unwrap_or_else(|| Path::new("."));

    // Step 2: resume offset.
    let mut offset = match tokio::fs::metadata(&part).await {
        Ok(m) => m.len(),
        Err(_) => 0,
    };
    if offset > cfg.size {
        tokio::fs::remove_file(&part).await?;
        offset = 0;
    }

    // Step 3: disk space.
    let need = cfg.size - offset + FREE_MARGIN;
    let available = free(parent).map_err(|e| Failure::Fatal(e.into()))?;
    if available < need {
        return Err(Failure::Fatal(
            HaruError::new(ErrorCode::DiskSpace, MsgKey::DiskSpace)
                .with_param("need", need as f64 / 1e9)
                .with_param("free", available as f64 / 1e9)
            .with_detail(format!("need {need} bytes, available {available} bytes at {}", parent.display())),
        ));
    }

    // Step 4: hash the existing part.
    let mut hasher = Sha256::new();
    if offset > 0 {
        let mut f = tokio::fs::File::open(&part).await?;
        let mut buf = vec![0u8; HASH_CHUNK];
        let mut done = 0u64;
        let mut throttle = Throttle::new();
        while done < offset {
            let want = ((offset - done) as usize).min(buf.len());
            let n = f.read(&mut buf[..want]).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            done += n as u64;
            if throttle.ready() {
                sink(DownloadEvent::Verifying { done, total: offset });
            }
        }
        if done != offset {
            // The part shrank under us; start over.
            tokio::fs::remove_file(&part).await?;
            return Err(Failure::Retry("partial file changed while hashing".into()));
        }
        sink(DownloadEvent::Verifying { done, total: offset });
    }

    // A full-size part needs no request (a Range request past the end would answer 416).
    if offset == cfg.size {
        return finish(cfg, sink, &part, hasher, offset).await;
    }

    // Step 5: request.
    let mut req = client.get(&cfg.url);
    if offset > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let resp = req.send().await.map_err(|e| Failure::Retry(format!("request failed: {e}")))?;
    let status = resp.status();
    let mut file = if status.as_u16() == 206 && offset > 0 {
        tokio::fs::OpenOptions::new().append(true).open(&part).await?
    } else if status.as_u16() == 200 {
        hasher = Sha256::new();
        offset = 0;
        tokio::fs::OpenOptions::new().create(true).write(true).truncate(true).open(&part).await?
    } else {
        return Err(Failure::Fatal(
            HaruError::new(ErrorCode::Download, MsgKey::DownloadRejected)
                .with_detail(format!("HTTP {status}")),
        ));
    };

    // Step 6: stream.
    let mut downloaded = offset;
    let mut stream = resp.bytes_stream();
    let mut throttle = Throttle::new();
    let mut window_start = Instant::now();
    let mut window_bytes = 0u64;
    let mut ema: Option<f64> = None;
    loop {
        let next = match tokio::time::timeout(STALL_TIMEOUT, stream.next()).await {
            Ok(n) => n,
            Err(_) => {
                let _ = file.flush().await;
                return Err(Failure::Retry("download stalled".into()));
            }
        };
        match next {
            None => break,
            Some(Err(e)) => {
                let _ = file.flush().await;
                return Err(Failure::Retry(format!("stream error: {e}")));
            }
            Some(Ok(chunk)) => {
                file.write_all(&chunk).await?;
                hasher.update(&chunk);
                downloaded += chunk.len() as u64;
                window_bytes += chunk.len() as u64;
                let el = window_start.elapsed();
                if el >= RATE_WINDOW {
                    let rate = window_bytes as f64 / el.as_secs_f64();
                    ema = Some(match ema {
                        Some(prev) => RATE_ALPHA * rate + (1.0 - RATE_ALPHA) * prev,
                        None => rate,
                    });
                    window_start = Instant::now();
                    window_bytes = 0;
                }
                if throttle.ready() {
                    sink(DownloadEvent::Progress { downloaded, total: cfg.size, bytes_per_sec: ema.unwrap_or(0.0) });
                }
            }
        }
    }
    file.flush().await?;
    drop(file);
    sink(DownloadEvent::Progress { downloaded, total: cfg.size, bytes_per_sec: ema.unwrap_or(0.0) });

    // Step 8: size check, hash check, rename.
    if downloaded < cfg.size {
        return Err(Failure::Retry(format!("connection ended at {downloaded} of {} bytes", cfg.size)));
    }
    finish(cfg, sink, &part, hasher, downloaded).await
}

/// Step 8b: hash check and rename of a completely downloaded part.
async fn finish(cfg: &FetchConfig, sink: &DownloadSink, part: &Path, hasher: Sha256, downloaded: u64) -> Result<PathBuf, Failure> {
    sink(DownloadEvent::Verifying { done: cfg.size, total: cfg.size });
    let actual = hex(&hasher.finalize());
    if downloaded != cfg.size || !actual.eq_ignore_ascii_case(&cfg.sha256) {
        let _ = tokio::fs::remove_file(part).await;
        return Err(Failure::Fatal(
            HaruError::new(ErrorCode::HashMismatch, MsgKey::DownloadCorrupt)
                .with_detail(format!("expected sha256 {}, got {actual} ({downloaded} bytes)", cfg.sha256)),
        ));
    }
    tokio::fs::rename(part, &cfg.dest).await?;
    sink(DownloadEvent::Done { path: cfg.dest.to_string_lossy().into_owned() });
    Ok(cfg.dest.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};
    use crate::messages::ParamValue;
    use std::sync::Mutex;

    const MIB: usize = 1024 * 1024;

    #[derive(Clone, Copy)]
    #[allow(dead_code)]
    enum Mode {
        Normal,
        IgnoreRange,
        Corrupt,
        DropFirstAfter(usize),
    }

    type Ranges = Arc<Mutex<Vec<Option<String>>>>;

    /// Reader that yields `limit` bytes of `data`, then fails (aborts the connection).
    struct Dropping {
        inner: Cursor<Vec<u8>>,
        left: usize,
    }
    impl Read for Dropping {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.left == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, "drop"));
            }
            let want = buf.len().min(self.left);
            let n = Read::read(&mut self.inner, &mut buf[..want])?;
            self.left -= n;
            Ok(n)
        }
    }

    fn lcg_blob(len: usize) -> Vec<u8> {
        let mut x: u32 = 12345;
        (0..len)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 24) as u8
            })
            .collect()
    }

    fn sha_hex(data: &[u8]) -> String {
        hex(&Sha256::digest(data))
    }

    fn serve(blob: Arc<Vec<u8>>, mode: Mode) -> (String, Ranges) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.server_addr().to_ip().unwrap());
        let ranges: Ranges = Arc::new(Mutex::new(Vec::new()));
        let seen = ranges.clone();
        std::thread::spawn(move || {
            let mut first = true;
            for req in server.incoming_requests() {
                let range = req
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Range"))
                    .map(|h| h.value.as_str().to_string());
                seen.lock().unwrap().push(range.clone());
                let start = match (mode, &range) {
                    (Mode::IgnoreRange, _) => None,
                    (_, Some(r)) => r.strip_prefix("bytes=").and_then(|r| r.strip_suffix('-')).and_then(|n| n.parse::<usize>().ok()),
                    _ => None,
                };
                let mut body = blob.to_vec();
                if matches!(mode, Mode::Corrupt) {
                    body[1000] ^= 0xFF;
                }
                let total = body.len();
                let from = start.unwrap_or(0);
                let slice = body[from..].to_vec();
                let len = slice.len();
                let mut headers = vec![tiny_http::Header::from_bytes(&b"Connection"[..], &b"close"[..]).unwrap()];
                let code = if start.is_some() {
                    headers.push(
                        tiny_http::Header::from_bytes(
                            &b"Content-Range"[..],
                            format!("bytes {from}-{}/{total}", total - 1).as_bytes(),
                        )
                        .unwrap(),
                    );
                    206
                } else {
                    200
                };
                let drop_after = match mode {
                    Mode::DropFirstAfter(n) if first => Some(n),
                    _ => None,
                };
                first = false;
                let reader: Box<dyn Read + Send> = match drop_after {
                    Some(n) => Box::new(Dropping { inner: Cursor::new(slice), left: n }),
                    None => Box::new(Cursor::new(slice)),
                };
                let resp = tiny_http::Response::new(tiny_http::StatusCode(code), headers, reader, Some(len), None);
                let _ = req.respond(resp);
            }
        });
        (base, ranges)
    }

    fn plenty(_: &Path) -> std::io::Result<u64> {
        Ok(u64::MAX / 2)
    }
    fn tiny(_: &Path) -> std::io::Result<u64> {
        Ok(1000)
    }

    fn collector() -> (DownloadSink, Arc<Mutex<Vec<DownloadEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let e2 = events.clone();
        (Arc::new(move |e| e2.lock().unwrap().push(e)), events)
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        cfg: FetchConfig,
        blob: Arc<Vec<u8>>,
    }

    fn fixture(base: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let blob = Arc::new(lcg_blob(MIB));
        let cfg = FetchConfig {
            url: format!("{base}/blob"),
            dest: dir.path().join("m").join("model.gguf"),
            size: blob.len() as u64,
            sha256: sha_hex(&blob),
        };
        Fixture { _dir: dir, cfg, blob }
    }

    /// Starts a server for the blob that `fixture` will produce (same LCG, same size).
    fn setup(mode: Mode) -> (Fixture, Ranges) {
        let blob = Arc::new(lcg_blob(MIB));
        let (base, ranges) = serve(blob, mode);
        (fixture(&base), ranges)
    }

    const DELAYS: [Duration; 3] = [Duration::from_millis(10); 3];

    async fn run(f: &Fixture, free: FreeSpace) -> (Result<PathBuf, HaruError>, Vec<DownloadEvent>) {
        let (sink, events) = collector();
        let client = reqwest::Client::new();
        let r = ensure_model_with(&f.cfg, sink, free, &client, &DELAYS).await;
        let ev = events.lock().unwrap().clone();
        (r, ev)
    }

    fn write_part(f: &Fixture, bytes: &[u8]) {
        std::fs::create_dir_all(f.cfg.dest.parent().unwrap()).unwrap();
        std::fs::write(partial_path(&f.cfg.dest), bytes).unwrap();
    }

    #[tokio::test]
    async fn fresh_download_writes_verified_file() {
        let (f, ranges) = setup(Mode::Normal);
        let (r, ev) = run(&f, plenty).await;
        assert_eq!(r.unwrap(), f.cfg.dest);
        assert_eq!(std::fs::read(&f.cfg.dest).unwrap(), *f.blob);
        assert!(!partial_path(&f.cfg.dest).exists());
        assert_eq!(*ranges.lock().unwrap(), vec![None]);
        let last_progress = ev.iter().rev().find_map(|e| match e {
            DownloadEvent::Progress { downloaded, total, .. } => Some((*downloaded, *total)),
            _ => None,
        });
        assert_eq!(last_progress, Some((MIB as u64, MIB as u64)));
        assert!(ev.iter().any(|e| matches!(e, DownloadEvent::Verifying { .. })));
        assert!(matches!(ev.last(), Some(DownloadEvent::Done { .. })));
    }

    #[tokio::test]
    async fn resume_with_206_appends() {
        let (f, ranges) = setup(Mode::Normal);
        write_part(&f, &f.blob[..300_000]);
        let (r, ev) = run(&f, plenty).await;
        r.unwrap();
        assert_eq!(*ranges.lock().unwrap(), vec![Some("bytes=300000-".to_string())]);
        assert_eq!(std::fs::read(&f.cfg.dest).unwrap(), *f.blob);
        let first_verifying = ev.iter().find_map(|e| match e {
            DownloadEvent::Verifying { total, .. } => Some(*total),
            _ => None,
        });
        assert_eq!(first_verifying, Some(300_000));
    }

    #[tokio::test]
    async fn server_ignoring_range_restarts_with_200() {
        let (f, _) = setup(Mode::IgnoreRange);
        write_part(&f, &f.blob[..300_000]);
        let (r, _) = run(&f, plenty).await;
        r.unwrap();
        let data = std::fs::read(&f.cfg.dest).unwrap();
        assert_eq!(data.len(), MIB);
        assert_eq!(data, *f.blob);
    }

    #[tokio::test]
    async fn hash_mismatch_deletes_part() {
        let (f, _) = setup(Mode::Corrupt);
        let (r, ev) = run(&f, plenty).await;
        assert_eq!(r.unwrap_err().code, ErrorCode::HashMismatch);
        assert!(!partial_path(&f.cfg.dest).exists());
        assert!(!f.cfg.dest.exists());
        match ev.last() {
            Some(DownloadEvent::Error(t)) => assert_eq!(t.key, MsgKey::DownloadCorrupt),
            other => panic!("expected an error event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn existing_file_with_exact_size_is_used_without_request() {
        let (f, ranges) = setup(Mode::Normal);
        std::fs::create_dir_all(f.cfg.dest.parent().unwrap()).unwrap();
        std::fs::write(&f.cfg.dest, vec![0u8; MIB]).unwrap();
        let (r, ev) = run(&f, plenty).await;
        assert_eq!(r.unwrap(), f.cfg.dest);
        assert_eq!(ranges.lock().unwrap().len(), 0);
        assert!(matches!(ev.last(), Some(DownloadEvent::Done { .. })));
        assert!(is_present(&f.cfg));
    }

    #[tokio::test]
    async fn full_size_part_is_verified_without_request() {
        let (f, ranges) = setup(Mode::Normal);
        write_part(&f, &f.blob);
        let (r, ev) = run(&f, plenty).await;
        assert_eq!(r.unwrap(), f.cfg.dest);
        assert_eq!(ranges.lock().unwrap().len(), 0);
        assert_eq!(std::fs::read(&f.cfg.dest).unwrap(), *f.blob);
        assert!(!partial_path(&f.cfg.dest).exists());
        assert!(matches!(ev.last(), Some(DownloadEvent::Done { .. })));
    }

    #[tokio::test]
    async fn full_size_part_with_wrong_content_is_hash_mismatch() {
        let (f, ranges) = setup(Mode::Normal);
        let mut bad = f.blob.to_vec();
        bad[500] ^= 0xFF;
        write_part(&f, &bad);
        let (r, _) = run(&f, plenty).await;
        assert_eq!(r.unwrap_err().code, ErrorCode::HashMismatch);
        assert_eq!(ranges.lock().unwrap().len(), 0);
        assert!(!partial_path(&f.cfg.dest).exists());
        assert!(!f.cfg.dest.exists());
    }

    #[tokio::test]
    async fn insufficient_disk_space_fails_before_request() {
        let (f, ranges) = setup(Mode::Normal);
        let (r, _) = run(&f, tiny).await;
        let e = r.unwrap_err();
        assert_eq!(e.code, ErrorCode::DiskSpace);
        assert_eq!(e.key, MsgKey::DiskSpace);
        assert!(matches!(e.params.get("need"), Some(ParamValue::Number(n)) if *n > 0.0));
        assert!(matches!(e.params.get("free"), Some(ParamValue::Number(_))));
        assert_eq!(ranges.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn oversized_part_is_discarded() {
        let (f, ranges) = setup(Mode::Normal);
        write_part(&f, &vec![7u8; 2 * MIB]);
        let (r, _) = run(&f, plenty).await;
        r.unwrap();
        assert_eq!(ranges.lock().unwrap()[0], None);
        assert_eq!(std::fs::read(&f.cfg.dest).unwrap(), *f.blob);
    }

    #[tokio::test]
    async fn dropped_connection_resumes_automatically() {
        let (f, ranges) = setup(Mode::DropFirstAfter(400_000));
        let (r, _) = run(&f, plenty).await;
        r.unwrap();
        assert_eq!(*ranges.lock().unwrap(), vec![None, Some("bytes=400000-".to_string())]);
        assert_eq!(std::fs::read(&f.cfg.dest).unwrap(), *f.blob);
    }
}
