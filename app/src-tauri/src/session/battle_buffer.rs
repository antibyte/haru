//! The battle's pre-buffer: when the mixed frames start to play, pause and resume. Pure: the dispatch thread
//! feeds it the frames generated and started and the time. It never ends a battle.

use std::time::Duration;

use crate::ipc::BattlePhase;

/// Real time of one double frame, 3200 samples at 24 kHz, in ms.
pub const FRAME_MS: f64 = 3200.0 * 1000.0 / 24000.0;
/// Frames buffered before a pause ends, and the least a start waits for: 3 s.
pub const RESUME_FRAMES: u64 = 22;
/// Generated frames from which the rate counts.
pub const RATE_FRAMES: u64 = 15;
/// Preparing lasts at most this long after the first frame; playback then starts and accepts pauses later.
pub const PREPARE_CAP: Duration = Duration::from_secs(20);

#[derive(Clone, Debug)]
pub struct BattleBuffer {
    max_frames: u64,
    phase: BattlePhase,
    generated: u64,
    started: u64,
    first_at: Option<Duration>,
    last_at: Duration,
    ended: bool,
}

impl BattleBuffer {
    pub fn new(max_frames: u64) -> Self {
        Self { max_frames, phase: BattlePhase::Preparing, generated: 0, started: 0, first_at: None, last_at: Duration::ZERO, ended: false }
    }

    /// Both speakers' frame of one more `seq` arrived at `now`.
    pub fn generated(&mut self, now: Duration) {
        self.first_at.get_or_insert(now);
        self.last_at = now;
        self.generated += 1;
    }

    /// `frames` frames have begun to play; it only grows and never passes the frames generated.
    pub fn started(&mut self, frames: u64) {
        self.started = frames.clamp(self.started, self.generated);
    }

    pub fn worker_ended(&mut self) {
        self.ended = true;
    }

    pub fn phase(&self) -> BattlePhase {
        self.phase
    }

    /// Frames generated that have not begun to play.
    pub fn buffered(&self) -> u64 {
        self.generated - self.started
    }

    /// Wall time per generated double frame in ms, from the first frame on, once 15 are generated.
    pub fn rate_ms(&self) -> Option<f64> {
        let first = self.first_at?;
        (self.generated >= RATE_FRAMES).then(|| self.last_at.saturating_sub(first).as_secs_f64() * 1000.0 / (self.generated - 1) as f64)
    }

    /// Frames to buffer before preparing ends. On a device slower than real time, this is enough that the frames
    /// not yet heard play without a pause: G ≥ remaining × (1 − 133.33 / r). Otherwise 3 s.
    pub fn start_threshold(&self) -> u64 {
        match self.rate_ms() {
            Some(r) if r > FRAME_MS => {
                let remaining = self.max_frames.saturating_sub(self.started) as f64;
                RESUME_FRAMES.max((remaining * (1.0 - FRAME_MS / r)).ceil() as u64)
            }
            _ => RESUME_FRAMES,
        }
    }

    pub fn update(&mut self, now: Duration) -> BattlePhase {
        let waited = self.first_at.is_some_and(|f| now.saturating_sub(f) >= PREPARE_CAP);
        self.phase = match self.phase {
            BattlePhase::Preparing if self.ended || waited || self.buffered() >= self.start_threshold() => BattlePhase::Playing,
            BattlePhase::Paused if self.ended || self.buffered() >= RESUME_FRAMES => BattlePhase::Playing,
            BattlePhase::Playing if !self.ended && self.buffered() == 0 => BattlePhase::Paused,
            p => p,
        };
        self.phase
    }

    /// Progress toward leaving `preparing` or `paused`, 0..1; 1 while playing.
    pub fn prepared(&self) -> f64 {
        let threshold = match self.phase {
            BattlePhase::Preparing => self.start_threshold(),
            BattlePhase::Paused => RESUME_FRAMES,
            BattlePhase::Playing => return 1.0,
        };
        (self.buffered() as f64 / threshold as f64).clamp(0.0, 1.0)
    }

    /// The worker ended and every generated frame has begun to play.
    pub fn finished(&self) -> bool {
        self.ended && self.buffered() == 0
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    /// Generates `n` frames, one every `step` ms from `from` ms; returns the time of the last.
    fn generate(b: &mut BattleBuffer, n: u64, from: u64, step: u64) -> Duration {
        let mut t = ms(from);
        for i in 0..n {
            t = ms(from + i * step);
            b.generated(t);
        }
        t
    }

    #[test]
    fn preparing_turns_playing_at_22_frames_on_a_fast_gpu() {
        let mut b = BattleBuffer::new(4500);
        let t = generate(&mut b, 21, 0, 50);
        assert_eq!(b.update(t), BattlePhase::Preparing);
        assert_eq!(b.start_threshold(), 22);
        let t = generate(&mut b, 1, 21 * 50, 50);
        assert_eq!(b.update(t), BattlePhase::Playing);
    }

    #[test]
    fn the_rate_counts_from_15_frames_on() {
        let mut b = BattleBuffer::new(4500);
        generate(&mut b, 14, 0, 300);
        assert_eq!(b.rate_ms(), None);
        generate(&mut b, 1, 14 * 300, 300);
        assert_eq!(b.rate_ms(), Some(300.0));
    }

    #[test]
    fn a_fast_gpu_keeps_the_threshold_at_22() {
        let mut b = BattleBuffer::new(4500);
        generate(&mut b, 40, 0, 50);
        assert_eq!(b.rate_ms(), Some(50.0));
        assert_eq!(b.start_threshold(), 22);
    }

    #[test]
    fn a_slow_gpu_prepares_enough_for_the_rest_without_a_pause() {
        let mut b = BattleBuffer::new(4500);
        let t = generate(&mut b, 15, 0, 270);
        assert_eq!(b.start_threshold(), 2278);
        assert_eq!(b.update(t), BattlePhase::Preparing);
        assert!((b.prepared() - 15.0 / 2278.0).abs() < 1e-12);
    }

    #[test]
    fn preparing_ends_20_s_after_the_first_frame_below_the_threshold() {
        let mut b = BattleBuffer::new(4500);
        generate(&mut b, 74, 1000, 270);
        assert_eq!(b.update(ms(20_999)), BattlePhase::Preparing);
        assert_eq!(b.update(ms(21_000)), BattlePhase::Playing);
    }

    #[test]
    fn the_worker_end_starts_playback_at_once() {
        let mut b = BattleBuffer::new(4500);
        let t = generate(&mut b, 3, 0, 270);
        b.worker_ended();
        assert_eq!(b.update(t), BattlePhase::Playing);
        assert_eq!(b.prepared(), 1.0);
    }

    #[test]
    fn running_dry_pauses_until_22_frames_are_buffered_again() {
        let mut b = BattleBuffer::new(4500);
        let t = generate(&mut b, 22, 0, 50);
        assert_eq!(b.update(t), BattlePhase::Playing);
        b.started(22);
        assert_eq!(b.update(t), BattlePhase::Paused);
        assert_eq!(b.prepared(), 0.0);
        let t = generate(&mut b, 21, 1100, 300);
        assert_eq!(b.update(t), BattlePhase::Paused);
        assert!((b.prepared() - 21.0 / 22.0).abs() < 1e-12);
        let t = generate(&mut b, 1, 1100 + 21 * 300, 300);
        assert_eq!(b.update(t), BattlePhase::Playing);
    }

    #[test]
    fn the_worker_end_resumes_a_pause_and_finishes_once_all_started() {
        let mut b = BattleBuffer::new(4500);
        let t = generate(&mut b, 22, 0, 50);
        b.update(t);
        b.started(22);
        assert_eq!(b.update(t), BattlePhase::Paused);
        let t = generate(&mut b, 2, 2000, 300);
        b.worker_ended();
        assert_eq!(b.update(t), BattlePhase::Playing);
        assert!(!b.finished());
        b.started(24);
        assert!(b.finished());
        assert_eq!(b.update(t), BattlePhase::Playing, "an ended battle never pauses");
    }

    #[test]
    fn threshold_never_drops_below_22_near_the_end() {
        let mut b = BattleBuffer::new(4500);
        for i in 0..4490u64 {
            b.generated(ms(i * 140));
        }
        b.started(4490);
        b.generated(ms(4490 * 140)); // rate 140 ms, remaining 10
        assert_eq!(b.start_threshold(), 22);
    }

    #[test]
    fn the_rate_survives_time_going_backwards() {
        let mut b = BattleBuffer::new(4500);
        generate(&mut b, 14, 1000, 100);
        b.generated(ms(0));
        assert_eq!(b.rate_ms(), Some(0.0));
    }

    #[test]
    fn prepared_is_clamped_between_0_and_1() {
        let mut b = BattleBuffer::new(4500);
        assert_eq!(b.prepared(), 0.0);
        generate(&mut b, 30, 0, 50);
        assert_eq!(b.prepared(), 1.0);
    }

    #[test]
    fn started_never_shrinks_nor_passes_generated() {
        let mut b = BattleBuffer::new(4500);
        generate(&mut b, 5, 0, 50);
        b.started(9);
        assert_eq!(b.buffered(), 0);
        b.started(2);
        assert_eq!(b.buffered(), 0);
    }

    #[test]
    fn finished_needs_the_end_and_every_frame_started() {
        let mut b = BattleBuffer::new(4500);
        generate(&mut b, 3, 0, 50);
        b.started(3);
        assert!(!b.finished());
        b.worker_ended();
        b.started(2);
        assert!(b.finished());
    }
}
