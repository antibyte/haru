//! Fixed-chunk mono resampler (Task 1.4).

use crate::error::{ErrorCode, HaruError};
use rubato::{FftFixedIn, Resampler};

enum Inner {
    Passthrough(usize),
    Fft(Box<FftFixedIn<f32>>),
}

pub struct FrameResampler {
    inner: Inner,
}

impl FrameResampler {
    pub fn new(from: u32, to: u32, chunk_in: usize) -> Result<Self, HaruError> {
        if from == to {
            return Ok(Self { inner: Inner::Passthrough(chunk_in) });
        }
        let r = FftFixedIn::<f32>::new(from as usize, to as usize, chunk_in, 2, 1).map_err(|e| {
            HaruError::new(ErrorCode::AudioDevice, "Audio konnte nicht umgewandelt werden.").with_detail(e.to_string())
        })?;
        Ok(Self { inner: Inner::Fft(Box::new(r)) })
    }

    /// `input.len()` must equal `chunk_in`.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        match &mut self.inner {
            Inner::Passthrough(_) => input.to_vec(),
            Inner::Fft(r) => match r.process(&[input], None) {
                Ok(mut out) => out.pop().unwrap_or_default(),
                Err(_) => Vec::new(),
            },
        }
    }

    pub fn output_len(&self) -> usize {
        match &self.inner {
            Inner::Passthrough(n) => *n,
            Inner::Fft(r) => r.output_frames_next(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: f32, start: usize, n: usize) -> Vec<f32> {
        (start..start + n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin() * 0.5)
            .collect()
    }

    #[test]
    fn passthrough_when_rates_equal() {
        let mut r = FrameResampler::new(24000, 24000, 3200).unwrap();
        let x = sine(440.0, 24000.0, 0, 3200);
        assert_eq!(r.process(&x), x);
        assert_eq!(r.output_len(), 3200);
    }

    #[test]
    fn upsample_24k_to_48k_length() {
        let mut r = FrameResampler::new(24000, 48000, 3200).unwrap();
        let mut total = 0;
        for i in 0..10 {
            total += r.process(&sine(1000.0, 24000.0, i * 3200, 3200)).len();
        }
        assert_eq!(total, 64000);
    }

    #[test]
    fn resample_24k_to_44k1_length() {
        let mut r = FrameResampler::new(24000, 44100, 3200).unwrap();
        assert_eq!(r.output_len(), 5880);
        let mut total = 0;
        for i in 0..10 {
            total += r.process(&sine(1000.0, 24000.0, i * 3200, 3200)).len();
        }
        assert_eq!(total, 58800);
    }

    #[test]
    fn sine_frequency_preserved() {
        let mut r = FrameResampler::new(24000, 48000, 3200).unwrap();
        let mut out = Vec::new();
        for i in 0..10 {
            let o = r.process(&sine(1000.0, 24000.0, i * 3200, 3200));
            if i >= 3 {
                out.extend(o);
            }
        }
        let crossings = out.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f64;
        let ms = out.len() as f64 / 48.0;
        let expected = ms * 1.0; // 1 kHz = 1 rising crossing per ms
        assert!((crossings - expected).abs() / expected < 0.02, "{crossings} vs {expected}");
    }
}
