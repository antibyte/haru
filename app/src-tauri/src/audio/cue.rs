//! The short sound cue mixed into the output: embedded WAV, decoded once, resampled per device rate and cached.

use super::resample::FrameResampler;
use crate::error::HaruError;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex, OnceLock};

static DAEMON_WAV: &[u8] = include_bytes!("../../assets/cues/daemon.wav");

const CHUNK: usize = 1024;

/// Decodes a WAV (16/24/32-bit int or 32-bit float, any channel count) to mono f32 and its sample rate.
pub fn decode_wav(bytes: &[u8]) -> Result<(u32, Vec<f32>), String> {
    let mut r = hound::WavReader::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<Result<_, _>>().map_err(|e| e.to_string())?
        }
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>().map_err(|e| e.to_string())?,
    };
    Ok((spec.sample_rate, raw.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect()))
}

/// Resamples a whole mono buffer; the result has `round(len * to / from)` samples.
pub fn resample_all(samples: &[f32], from: u32, to: u32) -> Result<Vec<f32>, HaruError> {
    if from == to {
        return Ok(samples.to_vec());
    }
    let want = (samples.len() as f64 * to as f64 / from as f64).round() as usize;
    let mut r = FrameResampler::new(from, to, CHUNK)?;
    let mut out = Vec::with_capacity(want + CHUNK * 4);
    let mut pos = 0;
    // Zero-padded tail chunks flush the resampler's delay.
    while out.len() < want + CHUNK && pos < samples.len() + CHUNK * 2 {
        let mut chunk = [0.0f32; CHUNK];
        let end = samples.len().min(pos + CHUNK);
        if pos < end {
            chunk[..end - pos].copy_from_slice(&samples[pos..end]);
        }
        out.extend(r.process(&chunk));
        pos += CHUNK;
    }
    out.truncate(want);
    Ok(out)
}

fn cache() -> &'static Mutex<HashMap<u32, Arc<Vec<f32>>>> {
    static C: OnceLock<Mutex<HashMap<u32, Arc<Vec<f32>>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The embedded cue at `rate` (mono). Cached per rate; `None` when the embedded file cannot be decoded.
pub fn cue_samples(rate: u32) -> Option<Arc<Vec<f32>>> {
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(v) = c.get(&rate) {
        return Some(v.clone());
    }
    let (from, mono) = match decode_wav(DAEMON_WAV) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("haru: sound cue unreadable: {e}");
            return None;
        }
    };
    let v = match resample_all(&mono, from, rate) {
        Ok(v) => Arc::new(v),
        Err(e) => {
            eprintln!("haru: sound cue resample failed: {e}");
            return None;
        }
    };
    c.insert(rate, v.clone());
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, ch: u16, data: &[i16]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        let spec = hound::WavSpec { channels: ch, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::new(&mut buf, spec).unwrap();
        for &s in data {
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
        buf.into_inner()
    }

    #[test]
    fn decode_downmixes_stereo() {
        let (rate, m) = decode_wav(&wav(44100, 2, &[16384, 0, -16384, 16384])).unwrap();
        assert_eq!(rate, 44100);
        assert_eq!(m, vec![0.25, 0.0]);
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode_wav(b"nope").is_err());
    }

    #[test]
    fn embedded_cue_is_mono_48k_short_and_loud() {
        let (rate, m) = decode_wav(DAEMON_WAV).unwrap();
        assert_eq!(rate, 48000);
        let secs = m.len() as f64 / 48000.0;
        assert!((1.0..=2.5).contains(&secs), "{secs} s");
        let peak = m.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak > 0.5 && peak <= 0.9, "peak {peak}");
    }

    #[test]
    fn resampled_length_44k1_within_one_percent() {
        let (_, m) = decode_wav(DAEMON_WAV).unwrap();
        let expected = m.len() as f64 * 44100.0 / 48000.0;
        let v = cue_samples(44100).unwrap();
        assert!((v.len() as f64 - expected).abs() < expected * 0.01, "{} vs {expected}", v.len());
        assert!(v.iter().any(|x| x.abs() > 0.1));
    }

    #[test]
    fn same_rate_is_unchanged_and_cached() {
        let a = cue_samples(48000).unwrap();
        let b = cue_samples(48000).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let (_, m) = decode_wav(DAEMON_WAV).unwrap();
        assert_eq!(*a, m);
    }
}
