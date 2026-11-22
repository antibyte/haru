//! Virtual acoustic loop (headless tuning only): what the virtual player "plays" travels through a simulated room
//! (device latency + acoustic delay, a deterministic room impulse response, gain, optional loudspeaker soft clip) and
//! is mixed into the mic samples the WAV feeder delivers. Together with the render tap fed by the virtual player it
//! reproduces the loudspeaker scenario without hardware.

use std::time::Instant;

use super::resample::FrameResampler;
use crate::error::HaruError;

/// Drive of the loudspeaker soft clip `tanh(k x) / k` (mild: ~10 % compression at |x| = 0.3).
const SOFT_CLIP_DRIVE: f32 = 1.5;
/// The first reflection comes this long after the direct path.
const TAIL_START_MS: f64 = 3.0;
/// Energy of the reverberant tail relative to the direct path (about 8 dB direct-to-reverberant).
const TAIL_ENERGY: f64 = 0.15;

/// Parameters of the simulated loudspeaker -> room -> microphone path.
#[derive(Clone, Debug, PartialEq)]
pub struct EchoSim {
    /// Linear gain of the whole path (speaker volume x coupling).
    pub gain: f32,
    /// Acoustic delay speaker -> microphone in ms.
    pub delay_ms: f32,
    /// Reverberation time (-60 dB) of the exponentially decaying noise tail in seconds; 0 = direct path only.
    pub rt60_s: f32,
    /// Constant delay between the render tap and the sound actually leaving the speaker, in ms.
    pub device_latency_ms: f32,
    /// Mild loudspeaker nonlinearity.
    pub soft_clip: bool,
    /// Seed of the impulse response noise.
    pub seed: u64,
}

impl Default for EchoSim {
    fn default() -> Self {
        Self { gain: 0.5, delay_ms: 30.0, rt60_s: 0.3, device_latency_ms: 30.0, soft_clip: true, seed: 0xEC40_5EED }
    }
}

/// xorshift64, uniform in [-1, 1).
struct Rng(u64);

impl Rng {
    fn uniform(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        ((x >> 40) as f32 / (1u32 << 23) as f32) - 1.0
    }
}

impl EchoSim {
    /// Total delay between the render tap and the echo reaching the microphone, in samples at `rate`.
    pub fn total_delay_samples(&self, rate: u32) -> usize {
        (((self.delay_ms + self.device_latency_ms).max(0.0) as f64) * rate as f64 / 1000.0).round() as usize
    }

    /// Room impulse response at `rate` (without `gain`): tap 0 is the direct path (1.0), followed by an exponentially
    /// decaying noise tail (RT60 = `rt60_s`) of `TAIL_ENERGY` relative energy. Deterministic for a given seed.
    pub fn impulse_response(&self, rate: u32) -> Vec<f32> {
        let mut h = vec![1.0f32];
        let len = (self.rt60_s.max(0.0) as f64 * rate as f64).round() as usize;
        let start = (TAIL_START_MS * rate as f64 / 1000.0).round().max(1.0) as usize;
        if len <= start {
            return h;
        }
        h.resize(len + 1, 0.0);
        let mut rng = Rng(self.seed.max(1));
        // Amplitude decays by 60 dB (factor 1e-3) over RT60.
        let tau = len as f64 / (3.0 * 10f64.ln());
        let mut energy = 0.0f64;
        for (k, tap) in h.iter_mut().enumerate().skip(start) {
            let v = rng.uniform() as f64 * (-((k - start) as f64) / tau).exp();
            *tap = v as f32;
            energy += v * v;
        }
        let scale = (TAIL_ENERGY / energy.max(1e-30)).sqrt();
        h.iter_mut().skip(start).for_each(|x| *x = (*x as f64 * scale) as f32);
        h
    }
}

/// Streaming room: `push_played` takes what the device played (output rate), `echo_block` yields what the microphone
/// hears of it, by absolute time since `t0` at the capture rate.
pub struct Room {
    sim: EchoSim,
    t0: Instant,
    ir: Vec<f32>,
    delay: usize,
    /// Played audio at the capture rate (after the soft clip), index = capture-rate sample since `t0`.
    hist: Vec<f32>,
    resampler: FrameResampler,
    chunk_in: usize,
    pending: Vec<f32>,
    /// Echo samples that were requested before the played audio was available (treated as silence).
    pub starved: u64,
}

impl Room {
    pub fn new(sim: EchoSim, play_rate: u32, mic_rate: u32) -> Result<Room, HaruError> {
        let chunk_in = (play_rate / 100).max(1) as usize;
        let resampler = FrameResampler::new(play_rate, mic_rate, chunk_in)?;
        let ir = sim.impulse_response(mic_rate);
        let delay = sim.total_delay_samples(mic_rate);
        Ok(Room { sim, t0: Instant::now(), ir, delay, hist: Vec::new(), resampler, chunk_in, pending: Vec::new(), starved: 0 })
    }

    /// Time zero of the played audio's sample clock (the virtual player paces itself on it).
    pub fn t0(&self) -> Instant {
        self.t0
    }

    pub fn sim(&self) -> &EchoSim {
        &self.sim
    }

    pub fn impulse_response(&self) -> &[f32] {
        &self.ir
    }

    /// Capture-rate samples of played audio known so far.
    pub fn available(&self) -> usize {
        self.hist.len()
    }

    /// Played samples (output rate, as written to the device, silence included).
    pub fn push_played(&mut self, played: &[f32]) {
        if self.sim.soft_clip {
            self.pending.extend(played.iter().map(|x| (SOFT_CLIP_DRIVE * x).tanh() / SOFT_CLIP_DRIVE));
        } else {
            self.pending.extend_from_slice(played);
        }
        let mut used = 0;
        while self.pending.len() - used >= self.chunk_in {
            let out = self.resampler.process(&self.pending[used..used + self.chunk_in]);
            self.hist.extend_from_slice(&out);
            used += self.chunk_in;
        }
        self.pending.drain(..used);
    }

    /// The echo at the microphone for capture-rate samples `start .. start + len` (since `t0`; `start` may be negative).
    pub fn echo_block(&mut self, start: i64, len: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; len];
        let g = self.sim.gain;
        for (i, o) in out.iter_mut().enumerate() {
            // Newest played sample that can contribute: the one `delay` samples before this instant.
            let t = start + i as i64 - self.delay as i64;
            if t < 0 {
                continue;
            }
            let t = t as usize;
            if t >= self.hist.len() {
                self.starved += 1;
                continue;
            }
            let taps = self.ir.len().min(t + 1);
            let mut acc = 0.0f32;
            for (k, h) in self.ir[..taps].iter().enumerate() {
                acc += h * self.hist[t - k];
            }
            *o = g * acc;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(delay_ms: f32, latency_ms: f32, gain: f32, rt60: f32) -> EchoSim {
        EchoSim { gain, delay_ms, device_latency_ms: latency_ms, rt60_s: rt60, soft_clip: false, ..Default::default() }
    }

    #[test]
    fn impulse_response_is_deterministic_and_decays() {
        let sim = EchoSim::default();
        let a = sim.impulse_response(16000);
        assert_eq!(a, sim.impulse_response(16000));
        assert_ne!(a, EchoSim { seed: 1, ..sim.clone() }.impulse_response(16000));
        assert_eq!(a.len(), 4801);
        assert_eq!(a[0], 1.0);
        let e = |s: &[f32]| s.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
        // Direct path 1.0, tail 0.15 of it, and the tail decays: the first quarter holds most of the energy.
        assert!((e(&a[1..]) - TAIL_ENERGY).abs() < 1e-4, "{}", e(&a[1..]));
        assert!(e(&a[1..1200]) > 5.0 * e(&a[3600..]));
        assert!(a[1..48].iter().all(|x| *x == 0.0), "no reflection before 3 ms");
        // RT60 0: direct path only.
        assert_eq!(plain(30.0, 30.0, 1.0, 0.0).impulse_response(16000), vec![1.0]);
    }

    #[test]
    fn delay_and_gain_of_the_direct_path() {
        // 20 + 30 ms at 16 kHz = 800 samples, no tail, no clip: a click comes back 800 samples late, scaled.
        let mut room = Room::new(plain(20.0, 30.0, 0.5, 0.0), 16000, 16000).unwrap();
        let mut played = vec![0.0f32; 4000];
        played[100] = 0.8;
        room.push_played(&played);
        let echo = room.echo_block(0, 3200);
        let peak = echo.iter().enumerate().max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap()).unwrap();
        assert_eq!(peak.0, 900);
        assert!((peak.1 - 0.4).abs() < 1e-6);
        assert_eq!(echo.iter().filter(|x| **x != 0.0).count(), 1);
        // The same block asked at an offset start moves the peak accordingly (absolute time).
        let again = room.echo_block(500, 800);
        assert!((again[400] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn tail_follows_the_direct_path_and_clip_compresses() {
        let mut room = Room::new(EchoSim { soft_clip: true, ..plain(0.0, 0.0, 1.0, 0.3) }, 16000, 16000).unwrap();
        let mut played = vec![0.0f32; 8000];
        played[10] = 0.3;
        room.push_played(&played);
        let echo = room.echo_block(0, 6000);
        let clipped = (1.5f32 * 0.3).tanh() / 1.5;
        assert!((echo[10] - clipped).abs() < 1e-6 && clipped < 0.3 && clipped > 0.25);
        assert!(echo[11..58].iter().all(|x| *x == 0.0));
        assert!(echo[58..4000].iter().any(|x| x.abs() > 1e-4), "reverberant tail expected");
        assert_eq!(room.starved, 0);
        // Asking for audio that was never played counts as starved silence.
        let _ = room.echo_block(7990, 40);
        assert!(room.starved > 0);
    }

    #[test]
    fn resampled_loop_keeps_delay_and_level() {
        // 48 kHz played, 16 kHz mic: a 1 kHz burst returns ~60 ms later at gain 0.5 (resampler latency is small).
        let mut room = Room::new(plain(30.0, 30.0, 0.5, 0.0), 48000, 16000).unwrap();
        let mut played = vec![0.0f32; 48000];
        for (i, s) in played.iter_mut().enumerate().skip(4800).take(4800) {
            *s = 0.4 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin();
        }
        room.push_played(&played);
        let echo = room.echo_block(0, 12000);
        let energy: Vec<f64> = echo.chunks(160).map(|c| c.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / 160.0).collect();
        let first = energy.iter().position(|e| *e > 1e-4).unwrap();
        // Burst starts at 100 ms; + 60 ms = 160 ms = chunk 16 (10 ms chunks).
        assert!((15..=18).contains(&first), "echo starts in chunk {first}");
        let rms = energy[first + 2].sqrt();
        assert!((rms - 0.5 * 0.4 / 2f64.sqrt()).abs() < 0.02, "rms {rms}");
    }
}
