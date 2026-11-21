//! M0 spike: drives the real `speech worker` with a WAV question and measures per-frame compute time.
//!
//! fast mode:     lockstep, chunk k is sent only after frame k arrived (pure compute time per frame).
//! realtime mode: chunk k is sent at t0 + k * 133.333 ms, like a live microphone.
//!
//! Exit codes: 0 PASS (p50 < 100 ms), 2 MARGINAL (100 <= p50 < 133), 1 FAIL (p50 >= 133 or protocol error).
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use haru_lib::worker::pcm::{decode_i16, encode_i16};
use haru_lib::worker::process::{WorkerConfig, WorkerEvent, WorkerProcess, READY_TIMEOUT};
use haru_lib::worker::protocol::{ClientMsg, DialogueOptions, WorkerMsg};

const FRAME: usize = 3200;
const FRAME_SECS: f64 = 3200.0 / 24000.0;
const ID: &str = "smoke";

struct Args {
    speech: PathBuf,
    model: PathBuf,
    wav: PathBuf,
    device: String,
    realtime: bool,
    tail_secs: f64,
    language: String,
    name: String,
    speak_first: bool,
    out: Option<PathBuf>,
    log: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        speech: PathBuf::new(),
        model: PathBuf::new(),
        wav: PathBuf::new(),
        device: "auto".into(),
        realtime: false,
        tail_secs: 6.0,
        language: "de".into(),
        name: "Haru".into(),
        speak_first: false,
        out: None,
        log: PathBuf::from("build/test/worker.log"),
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut val = |k: &str| it.next().ok_or_else(|| format!("missing value for {k}"));
        match k.as_str() {
            "--speech" => a.speech = val(&k)?.into(),
            "--model" => a.model = val(&k)?.into(),
            "--wav" => a.wav = val(&k)?.into(),
            "--device" => a.device = val(&k)?,
            "--mode" => {
                a.realtime = match val(&k)?.as_str() {
                    "fast" => false,
                    "realtime" => true,
                    m => return Err(format!("unknown mode {m} (fast|realtime)")),
                }
            }
            "--tail-secs" => a.tail_secs = val(&k)?.parse().map_err(|e| format!("--tail-secs: {e}"))?,
            "--language" => a.language = val(&k)?,
            "--name" => a.name = val(&k)?,
            "--speak-first" => a.speak_first = true,
            "--out" => a.out = Some(val(&k)?.into()),
            "--log" => a.log = val(&k)?.into(),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if a.speech.as_os_str().is_empty() || a.model.as_os_str().is_empty() || a.wav.as_os_str().is_empty() {
        return Err("required: --speech <exe> --model <gguf> --wav <24k mono i16>".into());
    }
    Ok(a)
}

fn read_wav(path: &PathBuf, tail_secs: f64) -> Result<Vec<i16>, String> {
    let mut r = hound::WavReader::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let s = r.spec();
    if s.sample_rate != 24000 || s.channels != 1 || s.bits_per_sample != 16 || s.sample_format != hound::SampleFormat::Int {
        return Err(format!(
            "{} is {} Hz / {} ch / {} bit; convert it to 24000 Hz mono 16-bit first (scripts/make-test-wav.ps1 or ffmpeg -ar 24000 -ac 1 -sample_fmt s16)",
            path.display(), s.sample_rate, s.channels, s.bits_per_sample
        ));
    }
    let mut v: Vec<i16> = r.samples::<i16>().collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    v.extend(std::iter::repeat(0).take((tail_secs * 24000.0) as usize));
    while v.len() % FRAME != 0 {
        v.push(0);
    }
    Ok(v)
}

/// Nearest-rank percentile over an ascending-sorted slice.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

struct Frames {
    audio: Vec<i16>,
    text: String,
    received: usize,
}

/// Receive until a chunk with `seq` arrives; records every chunk on the way. Returns the receive time.
fn recv_until(rx: &Receiver<WorkerEvent>, seq: u64, f: &mut Frames, on_frame: &mut dyn FnMut(u64, Instant, &str)) -> Result<Instant, String> {
    loop {
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(WorkerEvent::Message(WorkerMsg::Chunk { seq: s, pcm, text, .. })) => {
                let now = Instant::now();
                let samples = decode_i16(&pcm).map_err(|e| e.to_string())?;
                if samples.len() != FRAME {
                    return Err(format!("frame {s} has {} samples, expected {FRAME}", samples.len()));
                }
                if s as usize != f.received {
                    return Err(format!("frame seq {s} out of order, expected {}", f.received));
                }
                f.audio.extend(samples);
                let t = text.unwrap_or_default();
                f.text.push_str(&t);
                f.received += 1;
                on_frame(s, now, &t);
                if s >= seq {
                    return Ok(now);
                }
            }
            Ok(WorkerEvent::Message(WorkerMsg::Error { error, .. })) => return Err(format!("worker error {}: {}", error.code, error.message)),
            Ok(WorkerEvent::Message(WorkerMsg::End { .. })) => return Err("conversation ended unexpectedly".into()),
            Ok(WorkerEvent::Message(_)) => {}
            Ok(WorkerEvent::Malformed { line }) => return Err(format!("malformed line: {line}")),
            Ok(WorkerEvent::Exited { code }) => return Err(format!("worker exited (code {code:?})")),
            Err(RecvTimeoutError::Timeout) => return Err(format!("timeout waiting for frame {seq}")),
            Err(RecvTimeoutError::Disconnected) => return Err("worker event channel closed".into()),
        }
    }
}

fn run(a: &Args) -> Result<ExitCode, String> {
    let pcm = read_wav(&a.wav, a.tail_secs)?;
    let n_chunks = pcm.len() / FRAME;
    println!(
        "input {:.2} s ({} chunks of {FRAME}), mode {}, device {}",
        pcm.len() as f64 / 24000.0,
        n_chunks,
        if a.realtime { "realtime" } else { "fast" },
        a.device
    );

    let cfg = WorkerConfig { exe: a.speech.clone(), model: a.model.clone(), device: a.device.clone(), voice: None, log_path: a.log.clone() };
    let t_spawn = Instant::now();
    let mut w = WorkerProcess::spawn(&cfg).map_err(|e| format!("{e} {:?}", e.detail))?;
    let ready = w.wait_ready(READY_TIMEOUT).map_err(|e| format!("{e} {:?}", e.detail))?;
    let load_s = t_spawn.elapsed().as_secs_f64();
    println!("ready: model {} device {:?} weights {:?}", ready.model.name, ready.model.device, ready.model.weight_type);
    let rx = w.take_events();
    let tx = w.sender();

    let options = DialogueOptions {
        language: Some(a.language.clone()),
        name: Some(a.name.clone()),
        speak_first: if a.speak_first { Some(true) } else { None },
        ..Default::default()
    };
    let t_dialogue = Instant::now();
    tx.send(&ClientMsg::Dialogue { id: ID.into(), sample_rate: 24000, options }).map_err(|e| e.to_string())?;

    let mut frames = Frames { audio: Vec::new(), text: String::new(), received: 0 };
    let t_first = recv_until(&rx, 0, &mut frames, &mut |_, _, _| {})?;
    let first_frame_s = t_first.duration_since(t_dialogue).as_secs_f64();
    println!("first frame after {first_frame_s:.3} s");

    let mut compute_ms: Vec<f64> = Vec::with_capacity(n_chunks);
    let enc = |k: usize| encode_i16(&pcm[k * FRAME..(k + 1) * FRAME]);

    if !a.realtime {
        for k in 0..n_chunks {
            let t_send = Instant::now();
            tx.send(&ClientMsg::Chunk { id: ID.into(), seq: k as u64, pcm: enc(k) }).map_err(|e| e.to_string())?;
            let mut text_of = String::new();
            let t_recv = recv_until(&rx, (k + 1) as u64, &mut frames, &mut |_, _, t| text_of = t.to_string())?;
            let ms = t_recv.duration_since(t_send).as_secs_f64() * 1000.0;
            compute_ms.push(ms);
            println!("frame {:>4}  compute {ms:>6.1} ms  lag {:.2} s  text {text_of:?}", k + 1, 0.0);
        }
    } else {
        let t_sends: Arc<Mutex<Vec<Instant>>> = Arc::new(Mutex::new(Vec::with_capacity(n_chunks)));
        let chunks: Vec<String> = (0..n_chunks).map(enc).collect();
        let t0 = Instant::now();
        let sender_log = t_sends.clone();
        let tx2 = tx.clone();
        let th = std::thread::spawn(move || -> Result<(), String> {
            for (k, c) in chunks.into_iter().enumerate() {
                let due = t0 + Duration::from_secs_f64(k as f64 * FRAME_SECS);
                let now = Instant::now();
                if due > now {
                    std::thread::sleep(due - now);
                }
                sender_log.lock().unwrap().push(Instant::now());
                tx2.send(&ClientMsg::Chunk { id: ID.into(), seq: k as u64, pcm: c }).map_err(|e| e.to_string())?;
            }
            Ok(())
        });
        let mut rows: Vec<(u64, f64, f64, String)> = Vec::new();
        let sends = t_sends.clone();
        // frame seq s (>=1) answers chunk s-1
        recv_until(&rx, n_chunks as u64, &mut frames, &mut |s, now, t| {
            let g = sends.lock().unwrap();
            let k = (s - 1) as usize;
            if let Some(ts) = g.get(k) {
                let ms = now.duration_since(*ts).as_secs_f64() * 1000.0;
                let lag = g.len() as f64 * FRAME_SECS - s as f64 * FRAME_SECS;
                rows.push((s, ms, lag.max(0.0), t.to_string()));
            }
        })?;
        th.join().map_err(|_| "sender thread panicked".to_string())??;
        for (s, ms, lag, t) in &rows {
            println!("frame {s:>4}  compute {ms:>6.1} ms  lag {lag:.2} s  text {t:?}");
            compute_ms.push(*ms);
        }
        let max_lag = rows.iter().map(|r| r.2).fold(0.0, f64::max);
        println!("realtime max lag {max_lag:.2} s");
    }

    tx.send(&ClientMsg::HangUp { id: ID.into() }).map_err(|e| e.to_string())?;
    let end_frames = loop {
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(WorkerEvent::Message(WorkerMsg::End { frames: f, .. })) => break f,
            Ok(WorkerEvent::Message(WorkerMsg::Chunk { seq, pcm, text, .. })) => {
                // frames that were still in flight when hang_up arrived
                if let Ok(s) = decode_i16(&pcm) {
                    if seq as usize == frames.received {
                        frames.audio.extend(s);
                        frames.text.push_str(&text.unwrap_or_default());
                        frames.received += 1;
                    }
                }
            }
            Ok(WorkerEvent::Message(WorkerMsg::Error { error, .. })) => return Err(format!("worker error {}: {}", error.code, error.message)),
            Ok(WorkerEvent::Message(_)) => {}
            Ok(other) => return Err(format!("unexpected event while waiting for end: {other:?}")),
            Err(e) => return Err(format!("waiting for end: {e}")),
        }
    };
    if end_frames != Some(n_chunks as u64 + 1) {
        return Err(format!("end.frames = {end_frames:?}, expected {}", n_chunks + 1));
    }

    if let Some(out) = &a.out {
        if let Some(d) = out.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let spec = hound::WavSpec { channels: 1, sample_rate: 24000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut wr = hound::WavWriter::create(out, spec).map_err(|e| e.to_string())?;
        for s in &frames.audio {
            wr.write_sample(*s).map_err(|e| e.to_string())?;
        }
        wr.finalize().map_err(|e| e.to_string())?;
        println!("wrote {}", out.display());
    }

    let mut sorted = compute_ms.clone();
    sorted.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let (p50, p90, p99) = (percentile(&sorted, 50.0), percentile(&sorted, 90.0), percentile(&sorted, 99.0));
    let max = sorted.last().copied().unwrap_or(0.0);
    let over = compute_ms.iter().filter(|m| **m > 133.0).count();
    let (verdict, code) = if p50 < 100.0 {
        ("PASS", 0)
    } else if p50 < 133.0 {
        ("MARGINAL", 2)
    } else {
        ("FAIL", 1)
    };
    println!(
        "load {load_s:.2} s | first frame {first_frame_s:.2} s | frames {} | compute p50 {p50:.1} p90 {p90:.1} p99 {p99:.1} max {max:.1} ms | over 133 ms: {over} | text: {:?} | verdict {verdict}",
        frames.received, frames.text
    );
    w.shutdown(Duration::from_secs(5));
    Ok(ExitCode::from(code))
}

fn main() -> ExitCode {
    let a = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    match run(&a) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("FAIL: {e}");
            ExitCode::from(1)
        }
    }
}
