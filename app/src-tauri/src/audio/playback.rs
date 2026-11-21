//! Speaker playback (Task 1.4 gate, Task 1.5 stream).

use super::aec::{store_latency, LATENCY_UNKNOWN};
use crate::error::{ErrorCode, HaruError};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

/// Pure prebuffer / underrun logic of the playback callback.
pub struct JitterGate {
    prebuffer: usize,
    playing: bool,
    pub underruns: u64,
}

impl JitterGate {
    pub fn new(prebuffer: usize) -> Self {
        Self { prebuffer, playing: false, underruns: 0 }
    }

    /// Number of samples to read; the caller zero-fills the rest.
    pub fn decide(&mut self, available: usize, need: usize) -> usize {
        if !self.playing {
            if available >= self.prebuffer {
                self.playing = true;
                return available.min(need);
            }
            return 0;
        }
        if available < need {
            self.playing = false;
            self.underruns += 1;
        }
        available.min(need)
    }
}

pub struct PlaybackShared {
    /// Set by the stream error callback (device unplugged etc.).
    pub failed: AtomicBool,
    pub played: AtomicU64,
    pub underruns: AtomicU64,
    pub playing: AtomicBool,
    /// Last output latency (playback - callback timestamp) in microseconds; `LATENCY_UNKNOWN` until seen.
    pub output_latency_us: AtomicU32,
    /// Render-tap values dropped because the tap ring was full.
    pub tap_dropped: AtomicU64,
}

impl Default for PlaybackShared {
    fn default() -> Self {
        Self {
            failed: AtomicBool::new(false),
            played: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            output_latency_us: AtomicU32::new(LATENCY_UNKNOWN),
            tap_dropped: AtomicU64::new(0),
        }
    }
}

pub struct PlaybackHandle {
    pub rate: u32,
    pub producer: rtrb::Producer<f32>,
    pub shared: Arc<PlaybackShared>,
    /// Mono copy of every output frame (incl. prebuffer/underrun silence); only when requested.
    pub render_tap: Option<rtrb::Consumer<f32>>,
    pushed: u64,
}

impl PlaybackHandle {
    /// Queues samples; returns the stream index of the first one. A full ring drops the tail.
    pub fn push(&mut self, s: &[f32]) -> u64 {
        let start = self.pushed;
        for &x in s {
            if self.producer.push(x).is_err() {
                break;
            }
            self.pushed += 1;
        }
        start
    }

    pub fn pushed(&self) -> u64 {
        self.pushed
    }

    /// Samples queued but not yet played.
    pub fn buffered(&self) -> u64 {
        self.pushed.saturating_sub(self.shared.played.load(Ordering::Relaxed))
    }
}

fn err(msg: &str, detail: impl ToString) -> HaruError {
    HaruError::new(ErrorCode::AudioDevice, msg).with_detail(detail.to_string())
}

/// One output callback: the gate decides how many queued samples play, the rest is silence. Every value given to
/// `write` (incl. prebuffer/underrun silence) is also pushed to `tap`. Returns (popped, tap_dropped).
pub fn render_block(
    gate: &mut JitterGate,
    consumer: &mut rtrb::Consumer<f32>,
    mut tap: Option<&mut rtrb::Producer<f32>>,
    frames: usize,
    mut write: impl FnMut(usize, f32),
) -> (usize, u64) {
    let n = gate.decide(consumer.slots(), frames);
    let mut dropped = 0u64;
    for i in 0..frames {
        let v = if i < n { consumer.pop().unwrap_or(0.0) } else { 0.0 };
        write(i, v);
        if let Some(t) = tap.as_deref_mut() {
            if t.push(v).is_err() {
                dropped += 1;
            }
        }
    }
    (n, dropped)
}

fn build_typed<T>(
    dev: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: rtrb::Consumer<f32>,
    mut tap: Option<rtrb::Producer<f32>>,
    shared: Arc<PlaybackShared>,
    prebuffer: usize,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels.max(1) as usize;
    let mut gate = JitterGate::new(prebuffer);
    let failed = shared.clone();
    dev.build_output_stream(
        config,
        move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
            let ts = info.timestamp();
            store_latency(&shared.output_latency_us, ts.playback.duration_since(&ts.callback));
            let frames = data.len() / channels;
            let (n, dropped) = render_block(&mut gate, &mut consumer, tap.as_mut(), frames, |i, v| {
                let s = <T as FromSample<f32>>::from_sample_(v);
                for c in &mut data[i * channels..(i + 1) * channels] {
                    *c = s;
                }
            });
            shared.played.fetch_add(n as u64, Ordering::Relaxed);
            if dropped > 0 {
                shared.tap_dropped.fetch_add(dropped, Ordering::Relaxed);
            }
            shared.underruns.store(gate.underruns, Ordering::Relaxed);
            shared.playing.store(n > 0, Ordering::Relaxed);
        },
        move |e| {
            eprintln!("playback stream error: {e}");
            failed.failed.store(true, Ordering::Release);
        },
        None,
    )
}

pub fn build_playback(
    dev: &cpal::Device,
    prebuffer_frames: usize,
    render_tap: bool,
) -> Result<(cpal::Stream, PlaybackHandle), HaruError> {
    let supported = dev
        .default_output_config()
        .map_err(|e| err("Das Ausgabegerät konnte nicht geöffnet werden.", e))?;
    let rate = supported.sample_rate().0;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let (producer, consumer) = rtrb::RingBuffer::new(rate as usize * 4);
    let shared = Arc::new(PlaybackShared::default());
    let (tap_producer, tap_consumer) = if render_tap {
        let (p, c) = rtrb::RingBuffer::new(rate as usize * 2);
        (Some(p), Some(c))
    } else {
        (None, None)
    };
    let prebuffer = 3200 * rate as usize / 24000 * prebuffer_frames;
    let s = shared.clone();
    let stream = match format {
        SampleFormat::F32 => build_typed::<f32>(dev, &config, consumer, tap_producer, s, prebuffer),
        SampleFormat::I16 => build_typed::<i16>(dev, &config, consumer, tap_producer, s, prebuffer),
        SampleFormat::U16 => build_typed::<u16>(dev, &config, consumer, tap_producer, s, prebuffer),
        other => {
            return Err(err("Das Ausgabeformat wird nicht unterstützt.", format!("sample format {other:?}")));
        }
    }
    .map_err(|e| err("Das Ausgabegerät konnte nicht geöffnet werden.", e))?;
    stream
        .play()
        .map_err(|e| err("Das Ausgabegerät konnte nicht gestartet werden.", e))?;
    Ok((stream, PlaybackHandle { rate, producer, shared, render_tap: tap_consumer, pushed: 0 }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_until_prebuffer() {
        let mut g = JitterGate::new(6400);
        assert_eq!(g.decide(6000, 480), 0);
    }

    #[test]
    fn starts_at_prebuffer() {
        let mut g = JitterGate::new(6400);
        assert_eq!(g.decide(6400, 480), 480);
    }

    #[test]
    fn underrun_reads_partial_and_rebuffers() {
        let mut g = JitterGate::new(6400);
        assert_eq!(g.decide(6400, 480), 480);
        assert_eq!(g.decide(100, 480), 100);
        assert_eq!(g.decide(3000, 480), 0);
    }

    #[test]
    fn counts_underruns() {
        let mut g = JitterGate::new(1000);
        assert_eq!(g.underruns, 0);
        g.decide(1000, 480);
        g.decide(10, 480);
        assert_eq!(g.underruns, 1);
        g.decide(1000, 480);
        g.decide(0, 480);
        assert_eq!(g.underruns, 2);
    }

    /// One `render_block` call of `frames`; appends what was written to `written`, returns the tap drops.
    fn drive(
        gate: &mut JitterGate,
        cons: &mut rtrb::Consumer<f32>,
        tap: &mut rtrb::Producer<f32>,
        frames: usize,
        written: &mut Vec<f32>,
    ) -> u64 {
        let mut out = vec![0.0f32; frames];
        let (_, dropped) = render_block(gate, cons, Some(tap), frames, |i, v| out[i] = v);
        written.extend_from_slice(&out);
        dropped
    }

    #[test]
    fn tap_receives_played_samples_including_prebuffer_silence() {
        let (mut prod, mut cons) = rtrb::RingBuffer::<f32>::new(2000);
        let (mut tp, mut tc) = rtrb::RingBuffer::<f32>::new(2000);
        let mut gate = JitterGate::new(600);
        let mut written = Vec::new();
        let v: Vec<f32> = (1..=1000).map(|i| i as f32).collect();
        for x in &v[..500] {
            prod.push(*x).unwrap();
        }
        // 500 queued < prebuffer 600: silence.
        assert_eq!(drive(&mut gate, &mut cons, &mut tp, 256, &mut written), 0);
        assert!(written.iter().all(|s| *s == 0.0));
        for x in &v[500..] {
            prod.push(*x).unwrap();
        }
        for _ in 0..4 {
            drive(&mut gate, &mut cons, &mut tp, 256, &mut written);
        }
        let mut expected = vec![0.0f32; 256];
        expected.extend_from_slice(&v);
        expected.extend(std::iter::repeat(0.0).take(24));
        assert_eq!(written, expected);
        let mut tapped = Vec::new();
        while let Ok(x) = tc.pop() {
            tapped.push(x);
        }
        assert_eq!(tapped, expected);
    }

    #[test]
    fn tap_full_drops_and_counts() {
        let (_prod, mut cons) = rtrb::RingBuffer::<f32>::new(1000);
        let (mut tp, _tc) = rtrb::RingBuffer::<f32>::new(300);
        let mut gate = JitterGate::new(600);
        let mut written = Vec::new();
        assert_eq!(drive(&mut gate, &mut cons, &mut tp, 256, &mut written), 0);
        assert_eq!(drive(&mut gate, &mut cons, &mut tp, 256, &mut written), 212);
    }

    #[test]
    fn playback_shared_default_latency_unknown() {
        let s = PlaybackShared::default();
        assert_eq!(s.output_latency_us.load(Ordering::Relaxed), LATENCY_UNKNOWN);
        assert_eq!(s.tap_dropped.load(Ordering::Relaxed), 0);
    }
}
