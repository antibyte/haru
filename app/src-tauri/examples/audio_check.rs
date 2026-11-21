//! Device / tone / microphone check.
//! `cargo run --release --example audio_check -- [--input NAME] [--output NAME] [--seconds 6]`

use haru_lib::audio::devices;
use haru_lib::audio::engine::AudioEngine;
use haru_lib::audio::features::Analyzer;
use haru_lib::audio::resample::FrameResampler;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const FRAME: usize = 3200;

fn main() {
    let mut input = None;
    let mut output = None;
    let mut seconds = 6.0f64;
    let mut amp = 0.2f32;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--input" => input = args.next(),
            "--output" => output = args.next(),
            "--amp" => amp = args.next().and_then(|v| v.parse().ok()).unwrap_or(0.2),
            "--seconds" => seconds = args.next().and_then(|v| v.parse().ok()).unwrap_or(6.0),
            other => eprintln!("ignoring argument {other}"),
        }
    }

    match devices::list() {
        Ok(d) => {
            println!("inputs:  {:?} (default {:?})", d.inputs, d.default_input);
            println!("outputs: {:?} (default {:?})", d.outputs, d.default_output);
        }
        Err(e) => println!("device list failed: {e} {:?}", e.detail),
    }

    let (engine, mut cap, mut play) = match AudioEngine::start(input, output, false) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("engine start failed: {e} {:?}", e.detail);
            std::process::exit(1);
        }
    };
    println!("capture rate {} Hz, playback rate {} Hz", cap.rate, play.rate);

    let mut resampler = FrameResampler::new(24000, play.rate, FRAME).expect("resampler");
    let mut analyzer = Analyzer::new(cap.rate, (cap.rate / 50) as usize, 8, 80.0, 8000.0);
    let block = (cap.rate / 50) as usize;

    let start = Instant::now();
    let mut frame_idx = 0usize;
    let mut next_print = 0.5f64;
    let mut mic_buf: Vec<f32> = Vec::new();
    let mut peak_rms = 0.0f32;
    let total = Duration::from_secs_f64(seconds);

    while start.elapsed() < total {
        let t = start.elapsed().as_secs_f64();
        // keep ~0.4 s of tone queued: real-time pace
        while (frame_idx as f64) * (FRAME as f64) / 24000.0 < t + 0.4 {
            let samples: Vec<f32> = (0..FRAME)
                .map(|i| {
                    let n = frame_idx * FRAME + i;
                    amp * (2.0 * std::f32::consts::PI * 440.0 * n as f32 / 24000.0).sin()
                })
                .collect();
            play.push(&resampler.process(&samples));
            frame_idx += 1;
        }
        while let Ok(s) = cap.consumer.pop() {
            mic_buf.push(s);
        }
        let mut last = None;
        while mic_buf.len() >= block {
            let b: Vec<f32> = mic_buf.drain(..block).collect();
            last = Some(analyzer.analyze(&b));
        }
        if let Some(f) = &last {
            peak_rms = peak_rms.max(f.rms);
        }
        if t >= next_print {
            next_print += 0.5;
            let bands = last
                .as_ref()
                .map(|f| f.bands.iter().map(|b| format!("{b:.2}")).collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            println!(
                "t={t:.1}s played={} buffered={} underruns={} mic_rms={:.4} bands=[{}] overruns={}",
                play.shared.played.load(Ordering::Relaxed),
                play.buffered(),
                play.shared.underruns.load(Ordering::Relaxed),
                last.as_ref().map(|f| f.rms).unwrap_or(0.0),
                bands,
                cap.overruns.load(Ordering::Relaxed),
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    println!("peak mic rms: {peak_rms:.4}");
    engine.stop();
}
