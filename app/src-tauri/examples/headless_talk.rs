//! M1 acceptance: a live conversation without the UI.
//! `cargo run --release --example headless_talk -- [--seconds 60] [--language de] [--speak-first true]
//!      [--wav FILE] [--no-playback]`
//!
//! `--wav FILE` feeds the file (paced in real time, from frame 0 on) instead of the microphone;
//! `--no-playback` consumes Haru's audio with a virtual real-time sink, so the run is silent.
//! Exit 0 if no error occurred and (with `--speak-first`) at least one word was heard.

use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use haru_lib::ipc::{AppState, BootInfo, StateEvent, VizFrame};
use haru_lib::paths::{models_dir, HARU_MODEL};
use haru_lib::session::conversation::{ConversationIo, VizSink};
use haru_lib::session::{Core, CoreConfig};
use haru_lib::settings::Settings;

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
}

use haru_lib::test_hooks::load_wav;

#[tokio::main]
async fn main() -> ExitCode {
    let mut seconds = 60.0f64;
    let mut language = "de".to_string();
    let mut speak_first = true;
    let mut wav: Option<String> = None;
    let mut no_playback = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seconds" => seconds = args.next().and_then(|v| v.parse().ok()).unwrap_or(60.0),
            "--language" => language = args.next().unwrap_or(language),
            "--speak-first" => speak_first = args.next().map_or(true, |v| v != "false" && v != "0"),
            "--wav" => wav = args.next(),
            "--no-playback" => no_playback = true,
            other => eprintln!("ignoring argument {other}"),
        }
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

    let settings = Settings { language, speak_first, ..core.settings() };
    if let Err(e) = core.save_settings(settings) {
        eprintln!("settings: {e} {:?}", e.detail);
        return ExitCode::FAILURE;
    }
    if wav.is_some() || no_playback {
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
        core.set_test_io(ConversationIo { input_samples, no_playback });
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
        for w in &f.words {
            if !w.text.trim().is_empty() {
                s.pieces += 1;
                s.words.push_str(&w.text);
                println!("[{:.1}s] {}", f.t, w.text);
            }
        }
        if s.last_latency_print.map_or(true, |t| t.elapsed() >= Duration::from_secs(1)) {
            s.last_latency_print = Some(Instant::now());
            println!("latency {:.0} ms", f.latency_ms);
        }
    });

    let t_conv = Instant::now();
    let core2 = core.clone();
    let started = tokio::task::spawn_blocking(move || core2.start(sink)).await.unwrap();
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

    let s = stats.lock().unwrap();
    println!("---- summary ----");
    println!("backend/device: {} / {} ({})", info.backend, info.device, info.device_name);
    println!("load time: {load_secs:.1}s");
    println!("viz frames: {} ({:.0}/s), audible t = {:.1}s", s.frames, s.frames as f64 / run_secs.max(0.001), s.last_t);
    println!("frames with model audio: {} (mean rms {:.3})", s.nonzero_model, s.sum_model_rms / s.nonzero_model.max(1) as f64);
    println!("word pieces: {}", s.pieces);
    println!("text: {}", s.words);
    println!("latency: last {:.0} ms, max {:.0} ms", s.last_latency_ms, s.max_latency_ms);
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
