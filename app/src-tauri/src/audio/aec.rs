//! Acoustic echo cancellation (spec 2026-11-21) — the only module that knows sonora.
//!
//! `VoiceProcessor` owns one sonora `AudioProcessing` (mono, capture rate) and is driven by a single thread (the
//! mic pump): `push_render` with what the speaker played, `process_capture` with what the microphone heard.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use sonora::config::{AdaptiveDigital, EchoCanceller, GainController2, HighPassFilter, NoiseSuppression, NoiseSuppressionLevel};
use sonora::{AudioProcessing, Config, StreamConfig};

use super::resample::FrameResampler;

/// Marker in a latency slot: no timestamp seen yet.
pub const LATENCY_UNKNOWN: u32 = u32::MAX;
/// Output + input latency assumed when the device gives no timestamps.
pub const DEFAULT_LATENCY_MS: f64 = 40.0;
pub const MAX_DELAY_MS: i32 = 500;
/// Latencies above this are treated as timestamp garbage.
const MAX_LATENCY: Duration = Duration::from_secs(1);

/// Stores a callback latency in microseconds; `None` or > 1 s is ignored (keeps the previous value).
pub fn store_latency(slot: &AtomicU32, d: Option<Duration>) {
    if let Some(d) = d {
        if d <= MAX_LATENCY {
            slot.store(d.as_micros() as u32, Ordering::Relaxed);
        }
    }
}

pub fn load_latency_ms(slot: &AtomicU32) -> Option<f64> {
    match slot.load(Ordering::Relaxed) {
        LATENCY_UNKNOWN => None,
        us => Some(us as f64 / 1000.0),
    }
}

/// Stream delay hint for sonora: out (or 20 ms) + in (or 20 ms) + capture backlog - render backlog, clamped 0..=500.
pub fn delay_hint_ms(out_ms: Option<f64>, in_ms: Option<f64>, capture_backlog_ms: f64, render_backlog_ms: f64) -> f64 {
    let half = DEFAULT_LATENCY_MS / 2.0;
    (out_ms.unwrap_or(half) + in_ms.unwrap_or(half) + capture_backlog_ms - render_backlog_ms).clamp(0.0, MAX_DELAY_MS as f64)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stages {
    /// High-pass + AEC3 + noise suppression + AGC2 (what a browser's getUserMedia does).
    Full,
    /// AEC3 only (tests measure the cancellation itself).
    EchoOnly,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AecError {
    /// Capture rate not divisible by 100 or outside 8 000..=384 000 (no exact 10 ms frames).
    UnsupportedRate(u32),
    Resampler(String),
    FrameLength { len: usize, frame: usize },
    Processing(sonora::Error),
    Panicked,
}

impl std::fmt::Display for AecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AecError::UnsupportedRate(r) => write!(f, "unsupported sample rate {r} Hz (need a multiple of 100 in 8000..=384000)"),
            AecError::Resampler(e) => write!(f, "render resampler: {e}"),
            AecError::FrameLength { len, frame } => write!(f, "buffer of {len} samples is not a multiple of the {frame}-sample frame"),
            AecError::Processing(e) => write!(f, "audio processing: {e:?}"),
            AecError::Panicked => write!(f, "audio processing panicked"),
        }
    }
}

impl std::error::Error for AecError {}

fn config_for(stages: Stages) -> Config {
    let mut c = Config { echo_canceller: Some(EchoCanceller::default()), ..Default::default() };
    if stages == Stages::Full {
        c.high_pass_filter = Some(HighPassFilter::default());
        c.noise_suppression = Some(NoiseSuppression { level: NoiseSuppressionLevel::Moderate, ..Default::default() });
        c.gain_controller2 = Some(GainController2 { adaptive_digital: Some(AdaptiveDigital::default()), ..Default::default() });
    }
    c
}

/// Render reference: render-rate samples -> capture-rate 10 ms frames.
pub struct RenderFramer {
    resampler: FrameResampler, // passthrough when the rates are equal
    chunk_in: usize,           // resampler input chunk: render_rate / 100 (10 ms)
    input: Vec<f32>,           // waiting for a full resampler chunk
    output: Vec<f32>,          // capture rate, waiting for a full frame; capped at 1 s (drops the oldest)
    frame: usize,
    render_rate: u32,
    capture_rate: u32,
}

impl RenderFramer {
    pub fn new(render_rate: u32, capture_rate: u32) -> Result<Self, AecError> {
        if render_rate == 0 {
            return Err(AecError::UnsupportedRate(render_rate));
        }
        let chunk_in = (render_rate / 100).max(1) as usize;
        let resampler = FrameResampler::new(render_rate, capture_rate, chunk_in)
            .map_err(|e| AecError::Resampler(e.detail.clone().unwrap_or_else(|| e.to_string())))?;
        Ok(Self {
            resampler,
            chunk_in,
            input: Vec::with_capacity(chunk_in * 2),
            output: Vec::with_capacity(capture_rate as usize / 10),
            frame: (capture_rate / 100) as usize,
            render_rate,
            capture_rate,
        })
    }

    pub fn push(&mut self, s: &[f32]) {
        self.input.extend_from_slice(s);
        let mut used = 0;
        while self.input.len() - used >= self.chunk_in {
            let out = self.resampler.process(&self.input[used..used + self.chunk_in]);
            self.output.extend_from_slice(&out);
            used += self.chunk_in;
        }
        self.input.drain(..used);
        let cap = self.capture_rate as usize;
        if self.output.len() > cap {
            let excess = self.output.len() - cap;
            self.output.drain(..excess);
        }
    }

    /// Copies exactly one frame (`out.len()` must be the frame length) if available.
    pub fn pop_frame(&mut self, out: &mut [f32]) -> bool {
        if out.len() != self.frame || self.output.len() < self.frame {
            return false;
        }
        out.copy_from_slice(&self.output[..self.frame]);
        self.output.drain(..self.frame);
        true
    }

    pub fn pending_samples(&self) -> usize {
        self.output.len() + (self.input.len() as u64 * self.capture_rate as u64 / self.render_rate as u64) as usize
    }
}

pub struct VoiceProcessor {
    apm: AudioProcessing,
    frame: usize,
    capture_rate: u32,
    render: RenderFramer,
    render_frame: Vec<f32>,
    render_out: Vec<f32>,
    capture_out: Vec<f32>,
    render_frames: u64,
    capture_frames: u64,
    #[cfg(test)]
    fail_next: bool,
}

fn guarded(f: impl FnOnce() -> Result<(), sonora::Error>) -> Result<(), AecError> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(AecError::Processing(e)),
        Err(_) => Err(AecError::Panicked),
    }
}

impl VoiceProcessor {
    pub fn new(capture_rate: u32, render_rate: u32, stages: Stages) -> Result<Self, AecError> {
        if capture_rate % 100 != 0 || !(8_000..=384_000).contains(&capture_rate) {
            return Err(AecError::UnsupportedRate(capture_rate));
        }
        let render = RenderFramer::new(render_rate, capture_rate)?;
        let frame = (capture_rate / 100) as usize;
        let apm = AudioProcessing::builder()
            .config(config_for(stages))
            .capture_config(StreamConfig::new(capture_rate, 1))
            .render_config(StreamConfig::new(capture_rate, 1))
            .build();
        Ok(Self {
            apm,
            frame,
            capture_rate,
            render,
            render_frame: vec![0.0; frame],
            render_out: vec![0.0; frame],
            capture_out: vec![0.0; frame],
            render_frames: 0,
            capture_frames: 0,
            #[cfg(test)]
            fail_next: false,
        })
    }

    /// Samples per 10 ms frame at the capture rate.
    pub fn frame_len(&self) -> usize {
        self.frame
    }

    /// Feeds what the speaker played (render rate, mono).
    pub fn push_render(&mut self, samples: &[f32]) -> Result<(), AecError> {
        self.render.push(samples);
        while self.render.pop_frame(&mut self.render_frame) {
            let (apm, src, dst) = (&mut self.apm, &self.render_frame, &mut self.render_out);
            guarded(|| apm.process_render_f32(&[src.as_slice()], &mut [dst.as_mut_slice()]))?;
            self.render_frames += 1;
        }
        Ok(())
    }

    /// Output + input latency estimate; sonora only needs it roughly (it estimates the delay itself).
    pub fn set_delay_hint_ms(&mut self, ms: i32) {
        let _ = self.apm.set_stream_delay_ms(ms.clamp(0, MAX_DELAY_MS));
    }

    /// Cleans `buf` in place; its length must be a non-zero multiple of the 10 ms frame. On error `buf` keeps the
    /// frames that were already processed and the failing frame stays raw.
    pub fn process_capture(&mut self, buf: &mut [f32]) -> Result<(), AecError> {
        if buf.is_empty() || buf.len() % self.frame != 0 {
            return Err(AecError::FrameLength { len: buf.len(), frame: self.frame });
        }
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next) {
            return Err(AecError::Panicked);
        }
        for f in buf.chunks_exact_mut(self.frame) {
            let (apm, out) = (&mut self.apm, &mut self.capture_out);
            let src: &[f32] = f;
            guarded(|| apm.process_capture_f32(&[src], &mut [out.as_mut_slice()]))?;
            f.copy_from_slice(&self.capture_out);
            self.capture_frames += 1;
        }
        Ok(())
    }

    /// Reference audio waiting in the render path, in ms of capture-rate audio.
    pub fn pending_render_ms(&self) -> f64 {
        self.render.pending_samples() as f64 * 1000.0 / self.capture_rate as f64
    }

    pub fn erle_db(&self) -> Option<f64> {
        self.apm.statistics().echo_return_loss_enhancement
    }

    pub fn estimated_delay_ms(&self) -> Option<i32> {
        self.apm.statistics().delay_ms
    }

    pub fn capture_frames(&self) -> u64 {
        self.capture_frames
    }

    pub fn render_frames(&self) -> u64 {
        self.render_frames
    }

    #[cfg(test)]
    pub(crate) fn fail_next_for_test(&mut self) {
        self.fail_next = true;
    }
}

const _: fn() = || {
    fn send<T: Send>() {}
    send::<VoiceProcessor>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::test_signals::*;

    const RATE: u32 = 48_000;

    /// Runs far/mic through a processor in 20 ms chunks; returns the processed mic and the processor.
    fn run(far: &[f32], mic: &[f32], rate: u32, stages: Stages) -> (Vec<f32>, VoiceProcessor) {
        let mut vp = VoiceProcessor::new(rate, rate, stages).unwrap();
        let chunk = (rate / 50) as usize;
        let mut out = mic.to_vec();
        for (f, m) in far.chunks_exact(chunk).zip(out.chunks_exact_mut(chunk)) {
            vp.push_render(f).unwrap();
            vp.set_delay_hint_ms(40); // deliberately not the true 60 ms
            vp.process_capture(m).unwrap();
        }
        (out, vp)
    }

    fn noise(seed: u64, n: usize, amp: f32) -> Vec<f32> {
        let mut r = Rng(seed);
        (0..n).map(|_| amp * r.uniform()).collect()
    }

    fn echo_scene(seconds: usize) -> (Vec<f32>, Vec<f32>) {
        let n = seconds * RATE as usize;
        let far = speech_like(FAR, RATE, n);
        let h = room_impulse_response(0x5EED_0003, RATE);
        let echo = echo_of(&far, &h, 2880);
        (far, echo)
    }

    fn sec(s: f64) -> usize {
        (s * RATE as f64) as usize
    }

    #[test]
    fn erle_at_least_20_db_after_convergence() {
        let (far, echo) = echo_scene(8);
        let mic: Vec<f32> = echo.iter().zip(noise(0x5EED_0004, echo.len(), 1e-4)).map(|(e, n)| e + n).collect();
        let (out, vp) = run(&far, &mic, RATE, Stages::EchoOnly);
        let erle = ratio_db(&mic[sec(5.0)..sec(8.0)], &out[sec(5.0)..sec(8.0)]);
        eprintln!("ERLE 5-8 s: {erle:.1} dB; sonora ERLE stat {:?}, delay {:?} ms (true 60)", vp.erle_db(), vp.estimated_delay_ms());
        assert!(erle >= 20.0, "ERLE {erle:.1} dB");
    }

    #[test]
    fn double_talk_keeps_near_end() {
        let (far, echo) = echo_scene(8);
        let n = far.len();
        let mut near = vec![0.0f32; n];
        let ns = speech_like(NEAR, RATE, sec(2.0));
        near[sec(5.0)..sec(7.0)].copy_from_slice(&ns);
        let noise = noise(0x5EED_0004, n, 1e-4);
        let mic: Vec<f32> = (0..n).map(|i| echo[i] + near[i] + noise[i]).collect();
        let (out, vp) = run(&far, &mic, RATE, Stages::EchoOnly);
        // The pipeline is not sample-transparent (internal 32 kHz rate, filter banks, ~9 ms variable delay), so even
        // without any echo its output only correlates ~0.79 with the raw near signal. Reference for "near end kept":
        // the same near signal through the same processor with a silent far end.
        let near_mic: Vec<f32> = (0..n).map(|i| near[i] + noise[i]).collect();
        let (baseline, _) = run(&vec![0.0; n], &near_mic, RATE, Stages::EchoOnly);
        let span = sec(5.0)..sec(7.0);
        let erle = ratio_db(&mic[sec(3.0)..sec(5.0)], &out[sec(3.0)..sec(5.0)]);
        let kept = ratio_db(&out[span.clone()], &near[span.clone()]);
        let (corr_raw, raw_lag) = max_norm_xcorr(&out, &near, span.clone(), 480);
        let (ceiling, _) = max_norm_xcorr(&baseline, &near, span.clone(), 480);
        let (corr, corr_lag) = max_norm_xcorr(&out, &baseline, span.clone(), 480);
        eprintln!(
            "double talk: ERLE 3-5 s {erle:.1} dB, near kept {kept:.1} dB, xcorr vs baseline {corr:.3} (lag {corr_lag})"
        );
        eprintln!(
            "  vs raw near {corr_raw:.3} (lag {raw_lag}, no-echo ceiling {ceiling:.3}); sonora ERLE {:?}, delay {:?} ms",
            vp.erle_db(),
            vp.estimated_delay_ms()
        );
        assert!(erle >= 15.0, "ERLE {erle:.1} dB");
        assert!(kept >= -6.0, "near end attenuated to {kept:.1} dB");
        assert!(corr >= 0.7, "xcorr vs baseline {corr:.3}");
    }

    #[test]
    fn chunk_of_20_ms_is_two_frames() {
        for (rate, frame) in [(48_000, 480), (44_100, 441), (16_000, 160)] {
            let mut vp = VoiceProcessor::new(rate, rate, Stages::Full).unwrap();
            assert_eq!(vp.frame_len(), frame);
            let mut buf = vec![0.0f32; (rate / 50) as usize];
            vp.process_capture(&mut buf).unwrap();
            assert_eq!(vp.capture_frames(), 2);
        }
    }

    #[test]
    fn rejects_partial_frames() {
        let mut vp = VoiceProcessor::new(48_000, 48_000, Stages::EchoOnly).unwrap();
        let mut buf = vec![0.25f32; 500];
        assert_eq!(vp.process_capture(&mut buf), Err(AecError::FrameLength { len: 500, frame: 480 }));
        assert!(buf.iter().all(|s| *s == 0.25));
        assert_eq!(vp.capture_frames(), 0);
        assert_eq!(vp.process_capture(&mut []), Err(AecError::FrameLength { len: 0, frame: 480 }));
    }

    #[test]
    fn rejects_rate_not_divisible_by_100() {
        for r in [22_050, 11_025, 7_000] {
            assert!(matches!(VoiceProcessor::new(r, 48_000, Stages::Full), Err(AecError::UnsupportedRate(x)) if x == r));
        }
        for r in [8_000, 44_100, 48_000, 96_000] {
            assert!(VoiceProcessor::new(r, 48_000, Stages::Full).is_ok(), "{r}");
        }
    }

    #[test]
    fn render_resampling_44k1_to_48k() {
        let mut rf = RenderFramer::new(44_100, 48_000).unwrap();
        let sine: Vec<f32> = (0..44_100).map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 44_100.0).sin() * 0.5).collect();
        let mut frames: Vec<Vec<f32>> = Vec::new();
        let mut f = vec![0.0f32; 480];
        for c in sine.chunks(1000) {
            rf.push(c);
            while rf.pop_frame(&mut f) {
                frames.push(f.clone());
            }
        }
        assert!((98..=100).contains(&frames.len()), "{}", frames.len());
        let out: Vec<f32> = frames[5..].concat();
        let crossings = out.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f64;
        let expected = out.len() as f64 / 48.0;
        assert!((crossings - expected).abs() / expected < 0.02, "{crossings} vs {expected}");
    }

    #[test]
    fn render_passthrough_when_rates_equal() {
        let mut rf = RenderFramer::new(48_000, 48_000).unwrap();
        let x: Vec<f32> = (0..2400).map(|i| (i as f32 * 0.37).sin()).collect();
        rf.push(&x[..1234]);
        rf.push(&x[1234..]);
        let mut got = Vec::new();
        let mut f = vec![0.0f32; 480];
        let mut frames = 0;
        while rf.pop_frame(&mut f) {
            got.extend_from_slice(&f);
            frames += 1;
        }
        assert_eq!(frames, 5);
        assert_eq!(got, x);
    }

    #[test]
    fn full_chain_smoke() {
        let mut vp = VoiceProcessor::new(48_000, 44_100, Stages::Full).unwrap();
        let far = speech_like(FAR, 44_100, 3 * 44_100);
        let h = room_impulse_response(0x5EED_0003, 48_000);
        let far48 = speech_like(FAR, 48_000, 3 * 48_000);
        let echo = echo_of(&far48, &h, 2880);
        let near = speech_like(NEAR, 48_000, 3 * 48_000);
        let mut mic: Vec<f32> = echo.iter().zip(&near).map(|(e, n)| e + n).collect();
        for (i, m) in mic.chunks_exact_mut(960).enumerate() {
            let r = &far[(i * 882).min(far.len())..((i + 1) * 882).min(far.len())];
            vp.push_render(r).unwrap();
            vp.process_capture(m).unwrap();
            assert!(m.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
        }
        assert_eq!(vp.capture_frames(), 300);
        assert!(vp.render_frames() > 250, "{}", vp.render_frames());
    }

    #[test]
    fn stages_config() {
        let full = VoiceProcessor::new(48_000, 48_000, Stages::Full).unwrap();
        let c = full.apm.config();
        assert!(c.high_pass_filter.is_some() && c.echo_canceller.is_some());
        assert_eq!(c.noise_suppression.as_ref().unwrap().level, NoiseSuppressionLevel::Moderate);
        let g = c.gain_controller2.as_ref().unwrap();
        assert!(g.adaptive_digital.is_some() && !g.input_volume_controller);
        let only = VoiceProcessor::new(48_000, 48_000, Stages::EchoOnly).unwrap();
        let c = only.apm.config();
        assert!(c.echo_canceller.is_some());
        assert!(c.high_pass_filter.is_none() && c.noise_suppression.is_none() && c.gain_controller2.is_none());
    }

    #[test]
    fn delay_hint_formula() {
        assert_eq!(delay_hint_ms(None, None, 0.0, 0.0), 40.0);
        assert_eq!(delay_hint_ms(Some(12.0), Some(8.0), 15.0, 5.0), 30.0);
        assert_eq!(delay_hint_ms(Some(1.0), Some(1.0), 0.0, 50.0), 0.0);
        assert_eq!(delay_hint_ms(Some(400.0), Some(300.0), 0.0, 0.0), 500.0);
    }

    #[test]
    fn store_latency_ignores_absurd_values() {
        let slot = AtomicU32::new(LATENCY_UNKNOWN);
        assert_eq!(load_latency_ms(&slot), None);
        store_latency(&slot, Some(Duration::from_micros(12_500)));
        assert_eq!(load_latency_ms(&slot), Some(12.5));
        store_latency(&slot, Some(Duration::from_secs(2)));
        store_latency(&slot, None);
        assert_eq!(load_latency_ms(&slot), Some(12.5));
    }
}
