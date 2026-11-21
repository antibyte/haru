//! Deterministic synthetic signals for the echo-cancellation tests (spec 2026-11-21).

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use std::f64::consts::PI;

/// xorshift64: tiny, deterministic, good enough for test noise.
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in [-1, 1).
    pub fn uniform(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32 / (1u32 << 23) as f32) - 1.0
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (self.uniform() * 0.5 + 0.5) * (hi - lo)
    }
}

#[derive(Clone, Copy)]
pub struct Voice {
    pub seed: u64,
    pub f_start: (f32, f32),
    pub f_end: (f32, f32),
    pub rms: f32,
}

pub const FAR: Voice = Voice { seed: 0x5EED_0001, f_start: (120.0, 300.0), f_end: (600.0, 2800.0), rms: 0.1 };
pub const NEAR: Voice = Voice { seed: 0x5EED_0002, f_start: (180.0, 400.0), f_end: (400.0, 2000.0), rms: 0.1 };

/// Speech-like signal: syllables of 120-350 ms (harmonic chirp + low-passed noise, raised-cosine edges),
/// separated by pauses; every 6th pause is long (400-700 ms). RMS over the non-zero samples is `v.rms`.
pub fn speech_like(v: Voice, rate: u32, samples: usize) -> Vec<f32> {
    let mut rng = Rng(v.seed);
    let mut out = vec![0.0f32; samples];
    let ms = |m: f32| (m * rate as f32 / 1000.0) as usize;
    let edge = ms(15.0).max(1);
    let mut pos = 0usize;
    let mut syllable = 0usize;
    let mut lp = 0.0f32;
    while pos < samples {
        let len = ms(rng.range(120.0, 350.0));
        let gain = rng.range(0.5, 1.0);
        let f0a = rng.range(v.f_start.0, v.f_start.1) as f64;
        let f0b = rng.range(v.f_end.0, v.f_end.1) as f64;
        let mut phase = 0.0f64;
        for k in 0..len {
            if pos + k >= samples {
                break;
            }
            let f0 = f0a + (f0b - f0a) * k as f64 / len as f64;
            phase += 2.0 * PI * f0 / rate as f64;
            let voiced = phase.sin() as f32 + 0.5 * (2.0 * phase).sin() as f32 + 0.25 * (3.0 * phase).sin() as f32;
            lp = 0.6 * lp + 0.4 * rng.uniform();
            let x = 0.6 * voiced + 0.8 * lp;
            let env = if k < edge {
                0.5 - 0.5 * (PI * k as f64 / edge as f64).cos()
            } else if len - k <= edge {
                0.5 - 0.5 * (PI * (len - k) as f64 / edge as f64).cos()
            } else {
                1.0
            } as f32;
            out[pos + k] = x * env * gain;
        }
        syllable += 1;
        let pause = if syllable % 6 == 0 { rng.range(400.0, 700.0) } else { rng.range(40.0, 250.0) };
        pos += len + ms(pause);
    }
    // Normalize the RMS over the non-zero samples.
    let (sum, n) = out.iter().filter(|s| **s != 0.0).fold((0.0f64, 0usize), |(a, n), s| (a + (*s as f64).powi(2), n + 1));
    if n > 0 {
        let scale = v.rms / (sum / n as f64).sqrt() as f32;
        out.iter_mut().for_each(|s| *s *= scale);
    }
    out
}

/// 100 ms exponentially decaying noise (RT60 100 ms), L2 norm 0.5.
pub fn room_impulse_response(seed: u64, rate: u32) -> Vec<f32> {
    let mut rng = Rng(seed);
    let taps = (0.1 * rate as f64) as usize;
    let tau = 0.1 * rate as f64 / (3.0 * 10f64.ln());
    let mut h: Vec<f32> = (0..taps).map(|k| rng.uniform() * (-(k as f64) / tau).exp() as f32).collect();
    let norm = h.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    h.iter_mut().for_each(|x| *x = (*x as f64 * 0.5 / norm) as f32);
    h
}

/// FFT convolution in f64, truncated to `x.len()`.
pub fn convolve(x: &[f32], h: &[f32]) -> Vec<f32> {
    let n = (x.len() + h.len() - 1).next_power_of_two();
    let mut planner = FftPlanner::<f64>::new();
    let fwd = planner.plan_fft_forward(n);
    let inv = planner.plan_fft_inverse(n);
    let load = |v: &[f32]| {
        let mut b: Vec<Complex<f64>> = v.iter().map(|s| Complex::new(*s as f64, 0.0)).collect();
        b.resize(n, Complex::new(0.0, 0.0));
        b
    };
    let (mut a, mut b) = (load(x), load(h));
    fwd.process(&mut a);
    fwd.process(&mut b);
    for (p, q) in a.iter_mut().zip(&b) {
        *p *= *q;
    }
    inv.process(&mut a);
    a.iter().take(x.len()).map(|c| (c.re / n as f64) as f32).collect()
}

/// Far-end signal through the room, `delay` samples late.
pub fn echo_of(far: &[f32], h: &[f32], delay: usize) -> Vec<f32> {
    let conv = convolve(far, h);
    let mut out = vec![0.0f32; far.len()];
    for i in delay..far.len() {
        out[i] = conv[i - delay];
    }
    out
}

pub fn energy(x: &[f32]) -> f64 {
    x.iter().map(|s| (*s as f64).powi(2)).sum()
}

/// 10 log10(sum num^2 / sum den^2).
pub fn ratio_db(num: &[f32], den: &[f32]) -> f64 {
    10.0 * (energy(num) / energy(den).max(1e-30)).max(1e-30).log10()
}

/// Max over lags 0..=max_lag of the normalized cross-correlation of `out[range]` with `reference` delayed by `lag`
/// samples, i.e. `out[i] ~ reference[i - lag]` (the processing adds a small, fixed output delay).
/// Returns (best correlation, best lag).
pub fn max_norm_xcorr(out: &[f32], reference: &[f32], range: std::ops::Range<usize>, max_lag: usize) -> (f64, usize) {
    let o = &out[range.clone()];
    let eo = energy(o);
    let mut best = (f64::MIN, 0usize);
    for lag in 0..=max_lag.min(range.start) {
        let r = &reference[range.start - lag..range.end - lag];
        let dot: f64 = o.iter().zip(r).map(|(a, b)| *a as f64 * *b as f64).sum();
        let c = dot / (eo * energy(r)).sqrt().max(1e-30);
        if c > best.0 {
            best = (c, lag);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_normalized() {
        let a = speech_like(FAR, 48000, 48000);
        assert_eq!(a, speech_like(FAR, 48000, 48000));
        let nz: Vec<f64> = a.iter().filter(|s| **s != 0.0).map(|s| *s as f64).collect();
        let rms = (nz.iter().map(|x| x * x).sum::<f64>() / nz.len() as f64).sqrt();
        assert!((rms - 0.1).abs() < 1e-3, "{rms}");
        assert!(a.iter().any(|s| *s == 0.0), "pauses expected");
    }

    #[test]
    fn rir_norm_and_convolution() {
        let h = room_impulse_response(7, 48000);
        assert_eq!(h.len(), 4800);
        assert!((energy(&h).sqrt() - 0.5).abs() < 1e-4);
        let x = [1.0f32, 0.0, 0.0, 2.0];
        let y = convolve(&x, &[0.5, 0.25]);
        assert!((y[0] - 0.5).abs() < 1e-6 && (y[1] - 0.25).abs() < 1e-6 && (y[3] - 1.0).abs() < 1e-6);
    }
}
