//! M1 acceptance: a live conversation without the UI.
//! `cargo run --release --example headless_talk -- [--seconds 60] [--language de] [--speak-first true]
//!      [--wav FILE] [--no-playback] [--dump-wav FILE] [--dump-fx-wav FILE]
//!      [--temperature F] [--top-k N] [--backchannel low|mid|high]
//!      [--events 20,32] [--voice-threshold 0.01]
//!      [--virtual-echo [--aec on|off] [--echo-gain 0.5] [--echo-delay-ms 30] [--echo-rt60 0.3] [--echo-device-latency-ms 30]]`
//!
//! `--wav FILE` feeds the file (paced in real time, from frame 0 on) instead of the microphone;
//! `--no-playback` consumes Haru's audio with a virtual real-time sink, so the run is silent.
//! `--dump-wav FILE` writes Haru's decoded 24 kHz frames before the voice effect (mono 32-bit float WAV),
//! `--dump-fx-wav FILE` the ones after it.
//! `--temperature/--top-k/--backchannel` override the Settings used for the run.
//! `--events T1,T2` are the seconds in the input WAV where the user interjects; the stats block
//! reports how long Haru kept talking after each. `--voice-threshold` is the model-RMS level
//! (per 133 ms frame) above which a frame counts as speech.
//! `--virtual-echo` (needs `--wav` and `--no-playback`) closes an acoustic loop without hardware: what the virtual
//! player plays also goes to the AEC render tap and, through a simulated room (delay, impulse response with RT60,
//! gain, soft clip), into the mic samples. `--aec on|off` (default on) selects echo cancellation in that loop;
//! its summary (ERLE, delay, ...) is printed to stderr when the conversation ends. `HARU_DUMP_DIR` works as in the app.
//! Time axis of the stats: model frame k covers [k*133 ms, (k+1)*133 ms) of conversation time (the
//! input WAV is fed in real time from frame 0, so this is also input-WAV time), not audible time.
//! Exit 0 if no error occurred and (with `--speak-first`) at least one word was heard.

use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use haru_lib::audio::echo_sim::EchoSim;
use haru_lib::cards::Persona;
use haru_lib::ipc::{AppState, BootInfo, StateEvent, VizFrame};
use haru_lib::paths::{models_dir, HARU_MODEL};
use haru_lib::session::conversation::{ConversationIo, VizSink};
use haru_lib::session::{Core, CoreConfig};
use haru_lib::settings::{Settings, TEMPERATURE_MAX, TEMPERATURE_MIN};

const FRAME: usize = 3200;
const FRAME_SECS: f64 = FRAME as f64 / 24000.0;

#[derive(Default)]
struct Stats {
    frames: u64,
    pieces: u64,
    words: String,
    max_latency_ms: f64,
    last_latency_ms: f64,
    last_t: f64,
    sum_model_rms: f64,
    nonzero_model: u64,
    last_latency_print: Option<Instant>,
    /// True while inside a word (the previous non-whitespace char was not followed by whitespace yet).
    in_word: bool,
    /// (frame seq, pieces, words started) per text-carrying frame.
    text_frames: Vec<(u64, u32, u32)>,
    /// Mic RMS accumulation per audible second: (sum of squares, count).
    mic_by_sec: Vec<(f64, u64)>,
}

use haru_lib::test_hooks::load_wav;

fn write_wav(path: &str, samples: &[f32]) -> Result<(), hound::Error> {
    let spec = hound::WavSpec { channels: 1, sample_rate: 24000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec)?;
    for &x in samples {
        w.write_sample(x)?;
    }
    w.finalize()
}

fn frame_rms(samples: &[f32]) -> Vec<f64> {
    samples
        .chunks_exact(FRAME)
        .map(|c| (c.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>() / FRAME as f64).sqrt())
        .collect()
}

/// Runs of speech frames, merging gaps shorter than `max_gap` frames; (first frame, last frame).
fn speech_runs(speaking: &[bool], max_gap: usize) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (i, &s) in speaking.iter().enumerate() {
        if !s {
            continue;
        }
        match runs.last_mut() {
            Some(r) if i - r.1 - 1 < max_gap => r.1 = i,
            _ => runs.push((i, i)),
        }
    }
    runs
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn print_stats(rms: &[f64], s: &Stats, threshold: f64, events: &[f64]) {
    let n = rms.len();
    let total_secs = n as f64 * FRAME_SECS;
    println!("---- stats (threshold {threshold}, {n} model frames = {total_secs:.1}s) ----");
    if n == 0 {
        println!("no model frames");
        return;
    }
    let speaking: Vec<bool> = rms.iter().map(|r| *r > threshold).collect();
    let nspeak = speaking.iter().filter(|b| **b).count();
    let mut sorted = rms.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "frame RMS percentiles: p10 {:.4}  p25 {:.4}  p50 {:.4}  p75 {:.4}  p90 {:.4}  max {:.4}",
        percentile(&sorted, 0.1),
        percentile(&sorted, 0.25),
        percentile(&sorted, 0.5),
        percentile(&sorted, 0.75),
        percentile(&sorted, 0.9),
        sorted[n - 1]
    );
    println!("speaking ratio: {:.1}% ({nspeak}/{n} frames)", 100.0 * nspeak as f64 / n as f64);
    // Gaps shorter than 400 ms (< 3 frames of 133 ms) stay inside a run.
    let runs = speech_runs(&speaking, 3);
    let longest = runs.iter().map(|r| r.1 - r.0 + 1).max().unwrap_or(0);
    println!("speech runs (gaps < 400 ms merged): {}, longest {:.1}s", runs.len(), longest as f64 * FRAME_SECS);
    for (a, b) in &runs {
        println!("  run {:6.1}s .. {:6.1}s ({:.1}s)", *a as f64 * FRAME_SECS, (*b + 1) as f64 * FRAME_SECS, (*b - *a + 1) as f64 * FRAME_SECS);
    }

    let pieces: u32 = s.text_frames.iter().map(|t| t.1).sum();
    let words: u32 = s.text_frames.iter().map(|t| t.2).sum();
    println!(
        "text density overall: {:.2} pieces/s, {:.2} words/s ({pieces} pieces, {words} words over {total_secs:.1}s; normal speech ~2-3 words/s)",
        pieces as f64 / total_secs,
        words as f64 / total_secs
    );
    println!("text density per 10 s window:");
    let windows = (total_secs / 10.0).ceil() as usize;
    for w in 0..windows {
        let (lo, hi) = (w as f64 * 10.0, (w as f64 + 1.0) * 10.0);
        let len = hi.min(total_secs) - lo;
        let (mut p, mut wd) = (0u32, 0u32);
        for (seq, pc, wc) in &s.text_frames {
            let t = *seq as f64 * FRAME_SECS;
            if t >= lo && t < hi {
                p += pc;
                wd += wc;
            }
        }
        println!("  {:3.0}-{:3.0}s: {:5.2} pieces/s  {:5.2} words/s", lo, hi, p as f64 / len, wd as f64 / len);
    }

    for &ev in events {
        // First stretch of >= 500 ms (4 frames) below the threshold that starts at or after the event.
        let first = (ev / FRAME_SECS).floor() as usize;
        let need = 4usize;
        let mut found: Option<usize> = None;
        let mut i = first;
        while i + need <= n {
            if speaking[i..i + need].iter().all(|b| !b) {
                found = Some(i);
                break;
            }
            i += 1;
        }
        match found {
            Some(i) => {
                let delay = (i as f64 * FRAME_SECS - ev).max(0.0);
                if delay <= 8.0 {
                    println!("event {ev:.1}s: Haru stopped speaking after {delay:.1}s (silent from {:.1}s)", i as f64 * FRAME_SECS);
                } else {
                    println!("event {ev:.1}s: did not stop within 8 s (next silence only after {delay:.1}s)");
                }
            }
            None if first + need > n => println!("event {ev:.1}s: run ended before the event could be judged"),
            None => println!("event {ev:.1}s: did not stop within 8 s (no 500 ms silence until the end of the run)"),
        }
    }

    println!("---- per-second timeline (model/words by model frame time; mic by audible time) ----");
    let secs = total_secs.ceil() as usize;
    for sec in 0..secs {
        let (lo, hi) = (sec as f64, sec as f64 + 1.0);
        let idx: Vec<usize> = (0..n)
            .filter(|k| {
                let t = *k as f64 * FRAME_SECS;
                t >= lo && t < hi
            })
            .collect();
        let m = if idx.is_empty() { 0.0 } else { (idx.iter().map(|k| rms[*k] * rms[*k]).sum::<f64>() / idx.len() as f64).sqrt() };
        let spk = idx.iter().filter(|k| speaking[**k]).count();
        let mic = s.mic_by_sec.get(sec).map_or(0.0, |(sq, c)| if *c > 0 { (sq / *c as f64).sqrt() } else { 0.0 });
        let (mut wd, mut pc) = (0u32, 0u32);
        for (seq, p, w) in &s.text_frames {
            let t = *seq as f64 * FRAME_SECS;
            if t >= lo && t < hi {
                pc += p;
                wd += w;
            }
        }
        let mark = events.iter().any(|e| *e >= lo && *e < hi);
        let bar = "#".repeat((m * 100.0).round().min(40.0) as usize);
        println!(
            "t={sec:3} model={m:.4} mic={mic:.4} words={wd:2} pieces={pc:2} speaking={spk}/{}{} {bar}",
            idx.len(),
            if mark { "  <== EVENT" } else { "" }
        );
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut seconds = 60.0f64;
    let mut language = "de".to_string();
    let mut speak_first = true;
    let mut wav: Option<String> = None;
    let mut no_playback = false;
    let mut dump_wav: Option<String> = None;
    let mut dump_fx_wav: Option<String> = None;
    let mut temperature: Option<f64> = None;
    let mut top_k: Option<u32> = None;
    let mut backchannel: Option<String> = None;
    let mut events: Vec<f64> = Vec::new();
    let mut threshold = 0.01f64;
    let mut virtual_echo = false;
    let mut aec = true;
    let mut sim = EchoSim::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seconds" => seconds = args.next().and_then(|v| v.parse().ok()).unwrap_or(60.0),
            "--language" => language = args.next().unwrap_or(language),
            "--speak-first" => speak_first = args.next().is_none_or(|v| v != "false" && v != "0"),
            "--wav" => wav = args.next(),
            "--no-playback" => no_playback = true,
            "--dump-wav" => dump_wav = args.next(),
            "--dump-fx-wav" => dump_fx_wav = args.next(),
            "--temperature" => temperature = args.next().and_then(|v| v.parse().ok()),
            "--top-k" => top_k = args.next().and_then(|v| v.parse().ok()),
            "--backchannel" => backchannel = args.next(),
            "--events" => events = args.next().unwrap_or_default().split(',').filter_map(|v| v.trim().parse().ok()).collect(),
            "--voice-threshold" => threshold = args.next().and_then(|v| v.parse().ok()).unwrap_or(threshold),
            "--virtual-echo" => virtual_echo = true,
            "--aec" => aec = args.next().is_none_or(|v| v != "off" && v != "false" && v != "0"),
            "--echo-gain" => sim.gain = args.next().and_then(|v| v.parse().ok()).unwrap_or(sim.gain),
            "--echo-delay-ms" => sim.delay_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(sim.delay_ms),
            "--echo-rt60" => sim.rt60_s = args.next().and_then(|v| v.parse().ok()).unwrap_or(sim.rt60_s),
            "--echo-device-latency-ms" => sim.device_latency_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(sim.device_latency_ms),
            other => eprintln!("ignoring argument {other}"),
        }
    }
    if let Some(b) = &backchannel {
        if !matches!(b.as_str(), "low" | "mid" | "high") {
            eprintln!("--backchannel must be low, mid or high (got {b})");
            return ExitCode::FAILURE;
        }
    }

    if virtual_echo && (wav.is_none() || !no_playback) {
        eprintln!("--virtual-echo needs --wav FILE and --no-playback");
        return ExitCode::FAILURE;
    }
    let tmp = std::env::temp_dir().join(format!("haru-headless-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    let models = match models_dir() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("models dir: {e} {:?}", e.detail);
            return ExitCode::FAILURE;
        }
    };
    let backends_dir = haru_lib::paths::find_backends_dir(
        std::env::var_os("HARU_BACKENDS_DIR").map(Into::into),
        None,
        Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("backends")),
    );
    let cfg = CoreConfig {
        settings_path: tmp.join("settings.json"),
        log_dir: std::path::PathBuf::from("build/test"),
        backends_dir,
        models_dir: models,
        model: HARU_MODEL,
        hf_base: "https://huggingface.co".into(),
    };

    let had_error = Arc::new(AtomicBool::new(false));
    let t_start = Instant::now();
    let he = had_error.clone();
    let core = Core::new(
        cfg,
        Arc::new(move |ev: StateEvent| {
            println!("[state {:5.1}s] {}", t_start.elapsed().as_secs_f64(), serde_json::to_string(&ev).unwrap());
            if ev.state == AppState::Error {
                he.store(true, Ordering::Relaxed);
            }
        }),
    );

    let base = core.settings();
    let settings = Settings {
        language,
        speak_first,
        temperature: temperature.unwrap_or(base.temperature),
        top_k: top_k.unwrap_or(base.top_k),
        echo_cancellation: if virtual_echo { aec } else { base.echo_cancellation },
        backchannel: backchannel.clone().or(base.backchannel.clone()),
        ..base
    };
    if virtual_echo {
        println!("virtual echo: {sim:?}, aec {}", if aec { "on" } else { "off" });
    }
    println!(
        "settings: temperature {}, top_k {}, backchannel {:?}, speak_first {}, language {}",
        settings.temperature, settings.top_k, settings.backchannel, settings.speak_first, settings.language
    );
    // The app keeps the temperature strictly inside its slider range; experiments may leave it, but
    // only loudly: the validated save gets the clamped value, then the raw one is set in memory only.
    let experimental = settings.clone();
    let settings = if (TEMPERATURE_MIN..=TEMPERATURE_MAX).contains(&settings.temperature) {
        settings
    } else {
        eprintln!(
            "WARNING: --temperature {} is outside the app's range {TEMPERATURE_MIN}..{TEMPERATURE_MAX};              using it for this experiment only (the app would clamp it)",
            settings.temperature
        );
        Settings { temperature: settings.temperature.clamp(TEMPERATURE_MIN, TEMPERATURE_MAX), ..settings }
    };
    if let Err(e) = core.save_settings(settings) {
        eprintln!("settings: {e} {:?}", e.detail);
        return ExitCode::FAILURE;
    }
    core.set_settings_unchecked(experimental);
    // The dry model tap is always recorded: the stats are computed from it.
    let model_tap = Arc::new(Mutex::new(Vec::<f32>::new()));
    let fx_tap = dump_fx_wav.as_ref().map(|_| Arc::new(Mutex::new(Vec::<f32>::new())));
    {
        let input_samples = match &wav {
            Some(p) => match load_wav(p) {
                Ok(v) => Some(v),
                Err(e) => {
                    eprintln!("wav: {e}");
                    return ExitCode::FAILURE;
                }
            },
            None => None,
        };
        core.set_test_io(ConversationIo { input_samples, no_playback, virtual_echo: virtual_echo.then(|| sim.clone()), model_tap: Some(model_tap.clone()), fx_tap: fx_tap.clone() });
    }

    let t_boot = Instant::now();
    let info: BootInfo = match core.boot(Arc::new(|ev| println!("[download] {}", serde_json::to_string(&ev).unwrap()))).await {
        Ok(i) => i,
        Err(e) => {
            eprintln!("boot failed: {e} {:?}", e.detail);
            return ExitCode::FAILURE;
        }
    };
    let load_secs = t_boot.elapsed().as_secs_f64();
    println!("BootInfo: {}", serde_json::to_string(&info).unwrap());
    println!("boot/load time: {load_secs:.1}s");

    let stats = Arc::new(Mutex::new(Stats::default()));
    let st = stats.clone();
    let sink: VizSink = Arc::new(move |f: VizFrame| {
        let mut s = st.lock().unwrap();
        s.frames += 1;
        s.last_t = f.t;
        s.last_latency_ms = f.latency_ms;
        s.max_latency_ms = s.max_latency_ms.max(f.latency_ms);
        if f.model.rms > 0.0 {
            s.nonzero_model += 1;
            s.sum_model_rms += f.model.rms as f64;
        }
        let sec = f.t.max(0.0) as usize;
        if s.mic_by_sec.len() <= sec {
            s.mic_by_sec.resize(sec + 1, (0.0, 0));
        }
        s.mic_by_sec[sec].0 += (f.mic.rms as f64) * (f.mic.rms as f64);
        s.mic_by_sec[sec].1 += 1;
        for w in &f.words {
            if !w.text.trim().is_empty() {
                s.pieces += 1;
                s.words.push_str(&w.text);
                println!("[{:.1}s] {}", f.t, w.text);
            }
            // Word starts: a non-whitespace char after whitespace (or at the beginning).
            let mut started = 0u32;
            for c in w.text.chars() {
                if c.is_whitespace() {
                    s.in_word = false;
                } else {
                    if !s.in_word {
                        started += 1;
                    }
                    s.in_word = true;
                }
            }
            let p = u32::from(!w.text.trim().is_empty());
            if p > 0 || started > 0 {
                s.text_frames.push((w.seq, p, started));
            }
        }
        if s.last_latency_print.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
            s.last_latency_print = Some(Instant::now());
            println!("latency {:.0} ms", f.latency_ms);
        }
    });

    let t_conv = Instant::now();
    // Angel unless HARU_PERSONA=daemon.
    let persona = if std::env::var("HARU_PERSONA").is_ok_and(|v| v == "daemon") { Persona::Daemon } else { Persona::Angel };
    let core2 = core.clone();
    let started = tokio::task::spawn_blocking(move || core2.start(persona, sink)).await.unwrap();
    if let Err(e) = started {
        eprintln!("start failed: {e} {:?}", e.detail);
        core.shutdown();
        return ExitCode::FAILURE;
    }
    println!("conversation started after {:.1}s (frame 0 received)", t_conv.elapsed().as_secs_f64());

    let end = Instant::now() + Duration::from_secs_f64(seconds);
    while Instant::now() < end {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if core.status().state != AppState::Talking {
            break; // ended by itself (lag limit, worker error, ...)
        }
    }
    let run_secs = t_conv.elapsed().as_secs_f64();
    // Let the audible tail (the last frames) reach the viz before hanging up.
    tokio::time::sleep(Duration::from_millis(600)).await;
    let final_status = core.status();
    let core3 = core.clone();
    let _ = tokio::task::spawn_blocking(move || core3.stop()).await;
    core.shutdown();

    let model_samples = model_tap.lock().unwrap().clone();
    if let Some(path) = &dump_wav {
        match write_wav(path, &model_samples) {
            Ok(()) => println!("dumped {:.1}s of 24 kHz audio to {path}", model_samples.len() as f64 / 24000.0),
            Err(e) => eprintln!("dump {path}: {e}"),
        }
    }
    if let (Some(tap), Some(path)) = (&fx_tap, &dump_fx_wav) {
        let samples = tap.lock().unwrap().clone();
        match write_wav(path, &samples) {
            Ok(()) => println!("dumped {:.1}s of 24 kHz audio to {path}", samples.len() as f64 / 24000.0),
            Err(e) => eprintln!("dump {path}: {e}"),
        }
    }

    let s = stats.lock().unwrap();
    println!("---- summary ----");
    println!("backend/device: {} / {} ({})", info.backend, info.device, info.device_name);
    println!("load time: {load_secs:.1}s");
    println!("viz frames: {} ({:.0}/s), audible t = {:.1}s", s.frames, s.frames as f64 / run_secs.max(0.001), s.last_t);
    println!("frames with model audio: {} (mean rms {:.3})", s.nonzero_model, s.sum_model_rms / s.nonzero_model.max(1) as f64);
    println!("word pieces: {}", s.pieces);
    println!("text: {}", s.words);
    println!("latency: last {:.0} ms, max {:.0} ms", s.last_latency_ms, s.max_latency_ms);
    print_stats(&frame_rms(&model_samples), &s, threshold, &events);
    if let Some(e) = &final_status.error {
        println!("conversation ended with error: {e} {:?}", e.detail);
    }
    let failed = had_error.load(Ordering::Relaxed) || final_status.error.is_some();
    let no_words = speak_first && s.pieces == 0;
    if failed || no_words {
        println!("RESULT: FAIL (error={failed}, no words with speak-first={no_words})");
        return ExitCode::FAILURE;
    }
    println!("RESULT: OK");
    ExitCode::SUCCESS
}
