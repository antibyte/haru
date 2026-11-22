//! Offline daemon-voice renderer:
//! `cargo run --release --example voice_fx -- <in.wav> <out.wav|-> [dry24k.wav] [--v1] [--pitch ST] [--formant ST]
//!      [--sub MIX] [--wet MIX] [--presence DB]`
//!
//! The flags override the voice_fx.rs consts for this run only; `--v1` renders the old (v1) chain instead
//! (examples/voice_fx_v1). `out` = `-` only prints the statistics (rms, median f0 of the voiced parts).
//!
//! Reads a mono/stereo WAV (16/24/32-bit int or float, any rate), mixes to mono, resamples to 24 kHz,
//! runs `DemonVoice` in 3200-sample chunks (like the live path) and writes a 24 kHz mono 16-bit WAV.
//! The optional third path receives the dry 24 kHz input for A/B listening.

use haru_lib::audio::resample::FrameResampler;
use haru_lib::audio::voice_fx::{DemonParams, DemonVoice, Stages};

#[path = "voice_fx_v1/mod.rs"]
mod v1;

const CHUNK: usize = 3200;

fn read_mono(path: &str) -> Result<(Vec<f32>, u32), String> {
    let mut r = hound::WavReader::open(path).map_err(|e| format!("{path}: {e}"))?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let all: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>().map_err(|e| e.to_string())?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>().map_err(|e| e.to_string())?
        }
    };
    let mono = all.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    Ok((mono, spec.sample_rate))
}

fn to_24k(x: &[f32], rate: u32) -> Result<Vec<f32>, String> {
    if rate == 24_000 {
        return Ok(x.to_vec());
    }
    let mut rs = FrameResampler::new(rate, 24_000, CHUNK).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for c in x.chunks(CHUNK) {
        let mut buf = c.to_vec();
        buf.resize(CHUNK, 0.0);
        out.extend(rs.process(&buf));
    }
    out.truncate((x.len() as u64 * 24_000 / rate as u64) as usize);
    Ok(out)
}

fn write_wav(path: &str, x: &[f32]) -> Result<(), String> {
    let spec = hound::WavSpec { channels: 1, sample_rate: 24_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| format!("{path}: {e}"))?;
    for s in x {
        w.write_sample((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Median f0 (Hz, 50..400) over the voiced 40 ms windows, by normalised autocorrelation; 0 = none found.
fn median_f0(x: &[f32]) -> (f32, usize) {
    let (lo, hi) = (60usize, 480usize);
    let (win, hop) = (960usize, 480usize);
    let mut v = Vec::new();
    let mut i = 0;
    while i + win + hi < x.len() {
        let seg = &x[i..i + win + hi];
        let e0: f64 = seg[..win].iter().map(|s| (*s as f64).powi(2)).sum();
        if (e0 / win as f64).sqrt() > 0.02 {
            let mut best = (0.0f64, 0usize);
            for k in lo..hi {
                let r: f64 = (0..win).map(|j| seg[j] as f64 * seg[j + k] as f64).sum();
                let e1: f64 = (0..win).map(|j| (seg[j + k] as f64).powi(2)).sum();
                let c = r / (e0 * e1).sqrt().max(1e-12);
                if c > best.0 {
                    best = (c, k);
                }
            }
            if best.0 > 0.6 {
                v.push(24_000.0 / best.1 as f32);
            }
        }
        i += hop;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    (v.get(v.len() / 2).copied().unwrap_or(0.0), v.len())
}

fn run() -> Result<(), String> {
    let mut pos: Vec<String> = Vec::new();
    let mut p = DemonParams::default();
    let mut use_v1 = false;
    let mut args = std::env::args().skip(1);
    let num = |v: Option<String>, name: &str| -> Result<f64, String> {
        v.and_then(|v| v.parse().ok()).ok_or_else(|| format!("{name} needs a number"))
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--v1" => use_v1 = true,
            "--pitch" => p.pitch_semitones = num(args.next(), "--pitch")? as f32,
            "--formant" => p.formant_semitones = num(args.next(), "--formant")? as f32,
            "--sub" => p.sub_mix = num(args.next(), "--sub")? as f32,
            "--wet" => p.reverb_wet = num(args.next(), "--wet")? as f32,
            "--presence" => p.presence_db = num(args.next(), "--presence")?,
            _ => pos.push(a),
        }
    }
    if pos.len() < 2 {
        return Err("usage: voice_fx <in.wav> <out.wav|-> [dry24k.wav] [--v1] [--pitch ST] [--formant ST] [--sub MIX] [--wet MIX] [--presence DB]".into());
    }
    let (raw, rate) = read_mono(&pos[0])?;
    let dry = to_24k(&raw, rate)?;
    let mut wet = dry.clone();
    // Render the reverb tail too: one extra second of silence.
    wet.extend(std::iter::repeat_n(0.0, 24_000));
    let latency = if use_v1 {
        let mut voice = v1::DemonVoice::new();
        for c in wet.chunks_mut(CHUNK) {
            voice.process_in_place(c);
        }
        voice.latency_samples()
    } else {
        let mut voice = DemonVoice::with_params(Stages::ALL, p);
        for c in wet.chunks_mut(CHUNK) {
            voice.process_in_place(c);
        }
        voice.latency_samples()
    };
    if pos[1] != "-" {
        write_wav(&pos[1], &wet)?;
    }
    if let Some(d) = pos.get(2) {
        write_wav(d, &dry)?;
    }
    let (rd, rw) = (rms(&dry), rms(&wet[..dry.len()]));
    // The effect delays the signal by `latency`; compare the f0 of the aligned part.
    let (f_in, n_in) = median_f0(&dry);
    let (f_out, n_out) = median_f0(&wet[latency.min(wet.len())..]);
    println!(
        "{} -> {}: {:.1} s @ {rate} Hz, latency {} samples ({:.1} ms), rms {:.4} -> {:.4} ({:+.2} dB), median f0 {:.0} Hz (n={}) -> {:.0} Hz (n={}), ratio {:.3} ({:+.2} st)",
        pos[0],
        pos[1],
        dry.len() as f32 / 24_000.0,
        latency,
        latency as f32 / 24.0,
        rd,
        rw,
        20.0 * (rw / rd.max(1e-9)).log10(),
        f_in,
        n_in,
        f_out,
        n_out,
        f_out / f_in.max(1e-9),
        12.0 * (f_out / f_in.max(1e-9)).max(1e-9).log2()
    );
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
