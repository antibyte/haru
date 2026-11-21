//! RMS and log-spaced band features (Task 1.4).

use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

pub const BLOCK: usize = 400;
pub const MODEL_BANDS: usize = 16;
pub const MIC_BANDS: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct Features {
    pub rms: f32,
    pub bands: Vec<f32>,
}

pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    fft_len: usize,
    block_len: usize,
    window: Vec<f32>,
    window_sum: f32,
    edges: Vec<(f32, f32)>,
    bins: Vec<(usize, usize)>, // [start, end) bin range per band
    buf: Vec<Complex<f32>>,
}

impl Analyzer {
    pub fn new(rate: u32, block_len: usize, bands: usize, fmin: f32, fmax: f32) -> Self {
        let fft_len = block_len.max(2).next_power_of_two();
        let fft = FftPlanner::<f32>::new().plan_fft_forward(fft_len);
        let window: Vec<f32> = (0..block_len)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / block_len as f32).cos())
            .collect();
        let window_sum = window.iter().sum::<f32>().max(1e-9);
        let ratio = (fmax / fmin).powf(1.0 / bands as f32);
        let edges: Vec<(f32, f32)> = (0..bands)
            .map(|b| (fmin * ratio.powi(b as i32), fmin * ratio.powi(b as i32 + 1)))
            .collect();
        let hz_per_bin = rate as f32 / fft_len as f32;
        let nyq_bin = fft_len / 2;
        let bins = edges
            .iter()
            .map(|&(lo, hi)| {
                let a = ((lo / hz_per_bin).round() as usize).min(nyq_bin);
                let b = ((hi / hz_per_bin).round() as usize).min(nyq_bin);
                if b > a {
                    (a, b)
                } else {
                    // narrow band: use the bin nearest the band centre
                    let c = (((lo * hi).sqrt() / hz_per_bin).round() as usize).min(nyq_bin);
                    (c, c + 1)
                }
            })
            .collect();
        Self { fft, fft_len, block_len, window, window_sum, edges, bins, buf: vec![Complex::new(0.0, 0.0); fft_len] }
    }

    pub fn analyze(&mut self, block: &[f32]) -> Features {
        let n = block.len().min(self.block_len);
        let rms = if block.is_empty() {
            0.0
        } else {
            (block.iter().map(|x| x * x).sum::<f32>() / block.len() as f32).sqrt()
        };
        for (i, c) in self.buf.iter_mut().enumerate() {
            *c = if i < n { Complex::new(block[i] * self.window[i], 0.0) } else { Complex::new(0.0, 0.0) };
        }
        self.fft.process(&mut self.buf);
        let scale = 2.0 / self.window_sum;
        let top = self.fft_len / 2 + 1;
        let bands = self
            .bins
            .iter()
            .map(|&(a, b)| {
                let b = b.min(top);
                let a = a.min(b.saturating_sub(1));
                let slice = &self.buf[a..b];
                let mean = if slice.is_empty() {
                    0.0
                } else {
                    slice.iter().map(|c| c.norm()).sum::<f32>() / slice.len() as f32 * scale
                };
                let db = 20.0 * (mean + 1e-9).log10();
                ((db + 80.0) / 80.0).clamp(0.0, 1.0)
            })
            .collect();
        Features { rms, bands }
    }

    pub fn band_edges_hz(&self) -> &[(f32, f32)] {
        &self.edges
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: f32, n: usize, amp: f32) -> Vec<f32> {
        (0..n).map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin() * amp).collect()
    }

    #[test]
    fn silence_gives_zero() {
        let mut a = Analyzer::new(24000, BLOCK, MODEL_BANDS, 60.0, 12000.0);
        let f = a.analyze(&vec![0.0; BLOCK]);
        assert_eq!(f.rms, 0.0);
        assert!(f.bands.iter().all(|&b| b == 0.0));
    }

    #[test]
    fn full_scale_sine_rms() {
        let mut a = Analyzer::new(24000, BLOCK, MODEL_BANDS, 60.0, 12000.0);
        let f = a.analyze(&sine(1000.0, 24000.0, BLOCK, 1.0));
        assert!((f.rms - 0.707).abs() < 0.01, "{}", f.rms);
    }

    #[test]
    fn sine_1khz_peaks_in_its_band() {
        let mut a = Analyzer::new(24000, BLOCK, MODEL_BANDS, 60.0, 12000.0);
        let f = a.analyze(&sine(1000.0, 24000.0, BLOCK, 1.0));
        let idx = a.band_edges_hz().iter().position(|&(lo, hi)| lo <= 1000.0 && 1000.0 < hi).unwrap();
        let max = f.bands.iter().cloned().fold(0.0f32, f32::max);
        assert_eq!(f.bands[idx], max, "{:?}", f.bands);
        assert!(f.bands[idx] > 0.8, "{:?}", f.bands);
    }

    #[test]
    fn band_counts() {
        let mut m = Analyzer::new(24000, BLOCK, MODEL_BANDS, 60.0, 12000.0);
        assert_eq!(m.analyze(&vec![0.1; BLOCK]).bands.len(), 16);
        let mut k = Analyzer::new(48000, 960, MIC_BANDS, 80.0, 8000.0);
        assert_eq!(k.analyze(&vec![0.1; 960]).bands.len(), 8);
    }

    #[test]
    fn band_edges_log_spaced() {
        let a = Analyzer::new(24000, BLOCK, MODEL_BANDS, 60.0, 12000.0);
        let e = a.band_edges_hz();
        for w in e.windows(2) {
            assert!(w[1].0 > w[0].0);
        }
        let r0 = e[0].1 / e[0].0;
        for &(lo, hi) in e {
            assert!(((hi / lo) / r0 - 1.0).abs() < 0.01);
        }
        for w in e.windows(2) {
            assert!(((w[1].0 / w[0].0) / r0 - 1.0).abs() < 0.01);
        }
    }
}
