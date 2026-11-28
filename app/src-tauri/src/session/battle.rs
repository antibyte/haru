//! The angel-vs-daemon battle: one worker request whose two speakers are paired, voiced, mixed into the one
//! playback ring and pre-buffered. A battle has no microphone, no echo cancellation, no segment renewal and no lag
//! limit, so it runs beside `Conversation` and shares its pure pieces.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::audio::engine::{AudioEngine, PREBUFFER_FRAMES};
use crate::audio::features::{Analyzer, Features, BLOCK, MODEL_BANDS};
use crate::audio::playback::{JitterGate, PlaybackHandle, PlaybackShared};
use crate::audio::resample::FrameResampler;
use crate::audio::voice_fx::{effective_style, VoiceFx};
use crate::cards::{self, Persona};
use crate::error::{ErrorCode, HaruError};
use crate::ipc::{BattlePhase, Speaker};
use crate::messages::MsgKey;
use crate::session::battle_buffer::BattleBuffer;
use crate::session::conversation::{
    block_len_out, block_starts, flush_padding, prebuffer_samples, ConversationEnd, ConversationIo, VizSink, FIRST_FRAME_TIMEOUT, FRAME_SAMPLES, TICK,
};
use crate::session::timeline::FrameTimeline;
use crate::session::viz::{battle_frame, FeatureClock};
use crate::settings::Settings;
use crate::worker::pcm::decode_f32;
use crate::worker::process::WorkerSender;
use crate::worker::protocol::{battle_error, worker_error, ClientMsg, DialogueOptions, WorkerMsg};

/// Frames each speaker gives at most: 10 minutes.
pub const MAX_FRAMES: u32 = 4500;
/// Output rate of the virtual (no-playback) sink.
const VIRTUAL_RATE: u32 = 48000;
/// Unstarted frames kept in the ring while playing: one more than the device gate's prebuffer, so it never starves.
const RING_LEAD: usize = PREBUFFER_FRAMES + 1;

/// The topic a battle starts with, or why it cannot start: a worker without the battle feature, or an empty topic.
pub fn battle_start(topic: &str, supported: bool) -> Result<String, HaruError> {
    if !supported {
        return Err(HaruError::new(ErrorCode::Worker, MsgKey::BattleUnsupported));
    }
    cards::battle_topic(topic).ok_or_else(|| HaruError::new(ErrorCode::Settings, MsgKey::BattleTopic))
}

/// The two voices summed sample by sample; overlap is allowed, the sum is clamped like the cue mix.
pub fn mix(angel: &[f32], daemon: &[f32]) -> Vec<f32> {
    angel.iter().zip(daemon).map(|(a, d)| (a + d).clamp(-1.0, 1.0)).collect()
}

/// Both speakers' frame of one `seq`: index 0 the angel, 1 the daemon.
#[derive(Debug, PartialEq)]
pub struct Pair {
    pub seq: u64,
    pub frames: [Vec<f32>; 2],
    pub texts: [String; 2],
}

/// Joins the worker's chunks, which come speaker 0's and then speaker 1's for each `seq` (worker.md, battle).
#[derive(Default)]
pub struct FramePairs {
    held: Option<(u64, Vec<f32>, String)>,
    /// The `seq` of the last pair given out; a pair never comes twice.
    last: Option<u64>,
}

impl FramePairs {
    /// An angel frame is waiting for its daemon frame.
    pub fn pending(&self) -> bool {
        self.held.is_some()
    }

    pub fn accept(&mut self, speaker: Option<u8>, seq: u64, pcm: Vec<f32>, text: String) -> Result<Option<Pair>, String> {
        match (speaker, self.held.take()) {
            (Some(0), None) if self.last.is_some_and(|l| seq <= l) => Err(format!("angel frame with seq {seq} after the pair {:?}", self.last)),
            (Some(0), None) => {
                self.held = Some((seq, pcm, text));
                Ok(None)
            }
            (Some(1), Some((held, angel, _))) if held == seq && angel.len() != pcm.len() => {
                Err(format!("frames of seq {seq} have {} and {} samples", angel.len(), pcm.len()))
            }
            (Some(1), Some((held, angel, angel_text))) if held == seq => {
                self.last = Some(seq);
                Ok(Some(Pair { seq, frames: [angel, pcm], texts: [angel_text, text] }))
            }
            (speaker, held) => Err(format!("chunk of speaker {speaker:?} with seq {seq} after {:?}", held.map(|h| h.0))),
        }
    }
}
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn bad_frame(detail: String) -> HaruError {
    HaruError::new(ErrorCode::Worker, MsgKey::WorkerBadFrame).with_detail(detail).backend_failure()
}

fn unresponsive(detail: &str) -> HaruError {
    HaruError::new(ErrorCode::Worker, MsgKey::WorkerUnresponsive).with_detail(detail.to_string()).backend_failure()
}

/// A pair after the voices: the 24 kHz mix, each voice's block features and texts, waiting for the ring.
struct MixedFrame {
    seq: u64,
    samples: Vec<f32>,
    features: [Vec<Features>; 2],
    texts: [String; 2],
}

/// Each voice through its own effect (the daemon's deep voice, the angel as configured), analysed, then mixed.
fn voice_and_mix(pair: Pair, voices: &mut [VoiceFx; 2], analyzers: &mut [Analyzer; 2]) -> MixedFrame {
    let Pair { seq, mut frames, texts } = pair;
    let mut features: [Vec<Features>; 2] = Default::default();
    for i in 0..2 {
        voices[i].process_in_place(&mut frames[i]);
        features[i] = frames[i].chunks_exact(BLOCK).map(|b| analyzers[i].analyze(b)).collect();
    }
    MixedFrame { seq, samples: mix(&frames[0], &frames[1]), features, texts }
}

/// The one playback ring, a real device or a virtual real-time sink.
struct Out {
    rate: u32,
    shared: Arc<PlaybackShared>,
    pushed: Arc<AtomicU64>,
    real: Option<PlaybackHandle>,
    #[cfg(test)]
    tap: Option<Arc<Mutex<Vec<f32>>>>,
}

impl Out {
    fn push(&mut self, s: &[f32]) -> u64 {
        #[cfg(test)]
        if let Some(t) = &self.tap {
            lock(t).extend_from_slice(s);
        }
        match &mut self.real {
            Some(h) => {
                let start = h.push(s);
                self.pushed.store(h.pushed(), Ordering::Release);
                start
            }
            None => self.pushed.fetch_add(s.len() as u64, Ordering::AcqRel),
        }
    }
}

struct Inner {
    id: String,
    sender: WorkerSender,
    stop: AtomicBool,
    terminal: Mutex<bool>,
    terminal_cv: Condvar,
    end_fired: AtomicBool,
    hanging_up: AtomicBool,
    buffer: Mutex<BattleBuffer>,
    timeline: Mutex<FrameTimeline>,
    /// Block features of the angel (0) and the daemon (1) at their output positions.
    clocks: [Mutex<FeatureClock>; 2],
    on_end: Arc<dyn Fn(ConversationEnd) + Send + Sync>,
}

impl Inner {
    /// Reports the end to the owner, unless the battle is being stopped: a stop (`hang_up`, `abort`, drop) is the
    /// owner's own act and it makes its own state transition, and `on_end` could take a lock the stopping caller holds.
    fn fire_end(&self, e: ConversationEnd) {
        if self.stop.load(Ordering::Acquire) {
            return;
        }
        if !self.end_fired.swap(true, Ordering::AcqRel) {
            (self.on_end)(e);
        }
    }
    fn set_terminal(&self) {
        *lock(&self.terminal) = true;
        self.terminal_cv.notify_all();
    }
    fn has_terminal(&self) -> bool {
        *lock(&self.terminal)
    }
    fn wait_terminal(&self, timeout: Duration) -> bool {
        let g = lock(&self.terminal);
        let (g, _) = self.terminal_cv.wait_timeout_while(g, timeout, |t| !*t).unwrap_or_else(|e| e.into_inner());
        *g
    }
    /// Stamps a frame just pushed at `start` for the viz: each voice's blocks and each speaker's text.
    fn enqueue(&self, f: MixedFrame, start: u64, rate: u32) {
        let starts = block_starts(start, rate);
        for (i, clock) in self.clocks.iter().enumerate() {
            let mut c = lock(clock);
            for (b, features) in f.features[i].iter().enumerate() {
                c.push(starts[b], features.clone());
            }
        }
        let [angel, daemon] = f.texts;
        let mut t = lock(&self.timeline);
        t.push_spoken(f.seq, angel, start, Speaker::Angel);
        t.push_spoken(f.seq, daemon, start, Speaker::Daemon);
    }
}

fn spawn_virtual_sink(rate: u32, shared: Arc<PlaybackShared>, pushed: Arc<AtomicU64>, inner: Arc<Inner>) -> JoinHandle<()> {
    thread::Builder::new()
        .name("haru-battle-virtual-out".into())
        .spawn(move || {
            let mut gate = JitterGate::new(prebuffer_samples(rate) as usize);
            let t0 = Instant::now();
            let mut done = 0u64;
            while !inner.stop.load(Ordering::Acquire) {
                let need = (t0.elapsed().as_secs_f64() * rate as f64) as u64 - done;
                if need >= (rate / 100) as u64 {
                    let avail = pushed.load(Ordering::Acquire).saturating_sub(shared.played.load(Ordering::Relaxed));
                    let n = gate.decide(avail as usize, need as usize);
                    shared.played.fetch_add(n as u64, Ordering::Relaxed);
                    shared.playing.store(n > 0, Ordering::Relaxed);
                    done += need;
                }
                thread::sleep(Duration::from_millis(2));
            }
        })
        .expect("spawn battle virtual sink")
}

fn dispatch_loop(inner: Arc<Inner>, events: Receiver<WorkerMsg>, mut out: Out, first_tx: mpsc::Sender<Result<(), HaruError>>) {
    let mut resampler = match FrameResampler::new(24000, out.rate, FRAME_SAMPLES) {
        Ok(r) => r,
        Err(e) => {
            let _ = first_tx.send(Err(e));
            return;
        }
    };
    let mut voices = [Persona::Angel, Persona::Daemon].map(|p| VoiceFx::new(effective_style(p.voice_style())));
    let mut analyzers = [(); 2].map(|_| Analyzer::new(24000, BLOCK, MODEL_BANDS, 60.0, 12000.0));
    let mut pairs = FramePairs::default();
    let mut pending: VecDeque<MixedFrame> = VecDeque::new();
    let mut in_ring: VecDeque<u64> = VecDeque::new();
    let mut started = 0u64;
    let mut origin: Option<Instant> = None;
    let mut got_first = false;
    let mut generated = 0u64;
    let mut worker_ended = false;
    let mut dead = false;
    let mut last_push = Instant::now();
    let prebuffer = prebuffer_samples(out.rate);
    let first_deadline = Instant::now() + FIRST_FRAME_TIMEOUT;
    // Reports a failure to whoever waits: `start_with` before the first frame, `on_end` after.
    macro_rules! fail {
        ($e:expr) => {{
            let e: HaruError = $e;
            inner.set_terminal();
            if !got_first {
                let _ = first_tx.send(Err(e));
                return;
            }
            inner.fire_end(ConversationEnd::Failed(e));
            dead = true;
        }};
    }
    while !inner.stop.load(Ordering::Acquire) {
        let wait = if got_first { TICK } else { first_deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(50)) };
        match events.recv_timeout(wait) {
            Ok(WorkerMsg::Chunk { id, seq, pcm, text, speaker }) if id == inner.id && !dead => {
                let pcm = match decode_f32(&pcm) {
                    Ok(p) if p.len() == FRAME_SAMPLES => p,
                    Ok(p) => {
                        fail!(bad_frame(format!("frame {seq} of speaker {speaker:?} has {} samples", p.len())));
                        continue;
                    }
                    Err(e) => {
                        fail!(e.backend_failure());
                        continue;
                    }
                };
                match pairs.accept(speaker, seq, pcm, text.unwrap_or_default()) {
                    Ok(Some(pair)) => {
                        let at = *origin.get_or_insert_with(Instant::now);
                        pending.push_back(voice_and_mix(pair, &mut voices, &mut analyzers));
                        lock(&inner.buffer).generated(at.elapsed());
                        generated += 1;
                        if !got_first {
                            got_first = true;
                            let _ = first_tx.send(Ok(()));
                        }
                    }
                    Ok(None) => {}
                    Err(detail) => {
                        fail!(bad_frame(detail));
                        continue;
                    }
                }
            }
            Ok(WorkerMsg::End { id, frames, .. }) if id == inner.id => {
                if !got_first {
                    fail!(HaruError::new(ErrorCode::Worker, MsgKey::WorkerNoAnswer).with_detail("the battle ended before its first frame").backend_failure());
                }
                if pairs.pending() {
                    fail!(bad_frame("the battle ended after an angel frame without its daemon frame".into()));
                } else if let Some(n) = frames.filter(|n| *n != generated) {
                    fail!(bad_frame(format!("the battle ended after {n} frames, but {generated} pairs arrived")));
                }
                inner.set_terminal();
                worker_ended = true;
                lock(&inner.buffer).worker_ended();
            }
            Ok(WorkerMsg::Cancelled { id }) if id == inner.id => {
                if inner.hanging_up.load(Ordering::Acquire) {
                    inner.set_terminal();
                    dead = true;
                } else {
                    fail!(HaruError::new(ErrorCode::Worker, MsgKey::ConversationAborted).backend_failure());
                }
            }
            Ok(WorkerMsg::Error { id, error }) if id.as_deref().map_or(true, |i| i == inner.id) => fail!(battle_error(&error)),
            Ok(WorkerMsg::Fatal { error }) => fail!(worker_error(&error).backend_failure()),
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {
                if !got_first && Instant::now() >= first_deadline {
                    // `start_with` cancels the request when it gets this error.
                    let _ = first_tx.send(Err(HaruError::new(ErrorCode::Worker, MsgKey::WorkerNoAnswer)
                        .with_detail("no frame 0 of the battle within 30 s")
                        .backend_failure()));
                    return;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                if !worker_ended && !dead {
                    fail!(HaruError::new(ErrorCode::Worker, MsgKey::WorkerConnectionLost).with_detail("worker event channel closed").backend_failure());
                } else {
                    thread::sleep(Duration::from_millis(20));
                }
            }
        }
        let Some(at) = origin else { continue };
        let played = out.shared.played.load(Ordering::Relaxed);
        while in_ring.front().is_some_and(|s| *s < played) {
            in_ring.pop_front();
            started += 1;
        }
        let (phase, finished) = {
            let mut b = lock(&inner.buffer);
            b.started(started);
            (b.update(at.elapsed()), b.finished())
        };
        // The ring is fed only while playing and never more than RING_LEAD frames ahead: a pause plays silence.
        if phase == BattlePhase::Playing && !dead {
            while in_ring.len() < RING_LEAD {
                let Some(frame) = pending.pop_front() else { break };
                let samples = resampler.process(&frame.samples);
                let start = out.push(&samples);
                in_ring.push_back(start);
                inner.enqueue(frame, start, out.rate);
                last_push = Instant::now();
            }
        }
        let pushed = out.pushed.load(Ordering::Acquire);
        let pad = flush_padding(pushed.saturating_sub(played), prebuffer, out.shared.playing.load(Ordering::Relaxed), last_push.elapsed(), worker_ended && pending.is_empty());
        if pad > 0 {
            out.push(&vec![0.0; pad as usize]);
        }
        if finished && !dead && played >= out.pushed.load(Ordering::Acquire) && !inner.hanging_up.load(Ordering::Acquire) {
            inner.fire_end(ConversationEnd::HungUp);
        }
    }
}

fn ticker_loop(inner: Arc<Inner>, on_viz: VizSink, shared: Arc<PlaybackShared>, out_rate: u32) {
    while !inner.stop.load(Ordering::Acquire) {
        if shared.failed.load(Ordering::Acquire) && !inner.end_fired.load(Ordering::Acquire) {
            if !inner.has_terminal() {
                let _ = inner.sender.send(&ClientMsg::Cancel { id: inner.id.clone() });
            }
            inner.fire_end(ConversationEnd::Failed(HaruError::new(ErrorCode::AudioDevice, MsgKey::AudioDeviceLost)));
        }
        let played = shared.played.load(Ordering::Relaxed);
        let words = lock(&inner.timeline).drain_audible(played);
        let [angel, daemon] = [0, 1].map(|i| lock(&inner.clocks[i]).current(played));
        let (phase, prepared) = {
            let b = lock(&inner.buffer);
            (b.phase(), b.prepared())
        };
        on_viz(battle_frame(played as f64 / out_rate as f64, angel, daemon, phase, prepared, words));
        thread::sleep(TICK);
    }
}

pub struct Battle {
    #[cfg(test)]
    probe: Arc<PlaybackShared>,
    inner: Arc<Inner>,
    engine: Option<AudioEngine>,
    threads: Vec<JoinHandle<()>>,
}

impl Battle {
    /// Opens the output (never the microphone), sends the battle line and returns once both speakers' first frame
    /// has arrived.
    #[allow(clippy::too_many_arguments)]
    pub fn start_with(
        sender: WorkerSender,
        events: Receiver<WorkerMsg>,
        settings: &Settings,
        speakers: [DialogueOptions; 2],
        max_frames: u32,
        on_viz: VizSink,
        on_end: Arc<dyn Fn(ConversationEnd) + Send + Sync>,
        id: String,
        io: &ConversationIo,
    ) -> Result<Battle, HaruError> {
        let (engine, out) = if io.no_playback {
            let out = Out {
                rate: VIRTUAL_RATE,
                shared: Arc::new(PlaybackShared::default()),
                pushed: Arc::new(AtomicU64::new(0)),
                real: None,
                #[cfg(test)]
                tap: io.push_tap.clone(),
            };
            (None, out)
        } else {
            let (e, p) = AudioEngine::start_output(settings.output_device.clone())?;
            let out = Out {
                rate: p.rate,
                shared: p.shared.clone(),
                pushed: Arc::new(AtomicU64::new(0)),
                real: Some(p),
                #[cfg(test)]
                tap: io.push_tap.clone(),
            };
            (Some(e), out)
        };
        let rate = out.rate;
        let inner = Arc::new(Inner {
            id,
            sender: sender.clone(),
            stop: AtomicBool::new(false),
            terminal: Mutex::new(false),
            terminal_cv: Condvar::new(),
            end_fired: AtomicBool::new(false),
            hanging_up: AtomicBool::new(false),
            buffer: Mutex::new(BattleBuffer::new(max_frames as u64)),
            timeline: Mutex::new(FrameTimeline::new()),
            clocks: [(); 2].map(|_| Mutex::new(FeatureClock::new(block_len_out(rate)))),
            on_end,
        });
        let mut threads: Vec<JoinHandle<()>> = Vec::new();
        if out.real.is_none() {
            threads.push(spawn_virtual_sink(rate, out.shared.clone(), out.pushed.clone(), inner.clone()));
        }
        let shared = out.shared.clone();
        let abort = |inner: &Arc<Inner>, threads: Vec<JoinHandle<()>>| {
            inner.stop.store(true, Ordering::Release);
            for t in threads {
                let _ = t.join();
            }
        };
        if let Err(e) = sender.send(&ClientMsg::Battle { id: inner.id.clone(), speakers, max_frames }) {
            abort(&inner, threads);
            return Err(e);
        }
        let (first_tx, first_rx) = mpsc::channel();
        {
            let inner = inner.clone();
            threads.push(
                thread::Builder::new()
                    .name("haru-battle".into())
                    .spawn(move || dispatch_loop(inner, events, out, first_tx))
                    .expect("spawn battle dispatch thread"),
            );
        }
        let first = first_rx
            .recv_timeout(FIRST_FRAME_TIMEOUT + Duration::from_secs(5))
            .unwrap_or_else(|_| Err(HaruError::new(ErrorCode::Worker, MsgKey::WorkerNoAnswer).backend_failure()));
        if let Err(e) = first {
            let _ = sender.send(&ClientMsg::Cancel { id: inner.id.clone() });
            abort(&inner, threads);
            return Err(e);
        }
        #[cfg(test)]
        let probe = shared.clone();
        {
            let inner = inner.clone();
            threads.push(
                thread::Builder::new()
                    .name("haru-battle-viz".into())
                    .spawn(move || ticker_loop(inner, on_viz, shared, rate))
                    .expect("spawn battle viz thread"),
            );
        }
        Ok(Battle {
            #[cfg(test)]
            probe,
            inner,
            engine,
            threads,
        })
    }

    pub fn id(&self) -> &str {
        &self.inner.id
    }

    /// Sends `hang_up` (the worker ends after the frame under way), waits up to `timeout`, otherwise cancels; then
    /// stops the threads and the audio at once.
    pub fn hang_up(mut self, timeout: Duration) -> Result<(), HaruError> {
        self.inner.hanging_up.store(true, Ordering::Release);
        let mut result = Ok(());
        if !self.inner.has_terminal() {
            let id = self.inner.id.clone();
            match self.inner.sender.send(&ClientMsg::HangUp { id: id.clone() }) {
                Err(e) => result = Err(e),
                Ok(()) => {
                    if !self.inner.wait_terminal(timeout) {
                        let cancelled = self.inner.sender.send(&ClientMsg::Cancel { id }).is_ok() && self.inner.wait_terminal(Duration::from_secs(1));
                        if !cancelled {
                            result = Err(unresponsive("no end or cancelled after hang_up"));
                        }
                    }
                }
            }
        }
        self.teardown();
        result
    }

    /// Ends a battle that ended or failed by itself: cancels it if the worker has not finished it yet.
    pub fn abort(mut self) -> Result<(), HaruError> {
        self.inner.hanging_up.store(true, Ordering::Release);
        let mut result = Ok(());
        if !self.inner.has_terminal() {
            let sent = self.inner.sender.send(&ClientMsg::Cancel { id: self.inner.id.clone() }).is_ok();
            if !sent || !self.inner.wait_terminal(Duration::from_secs(2)) {
                result = Err(unresponsive("no cancelled after cancel"));
            }
        }
        self.teardown();
        result
    }

    fn teardown(&mut self) {
        self.inner.stop.store(true, Ordering::Release);
        let me = thread::current().id();
        for t in self.threads.drain(..) {
            if t.thread().id() != me {
                let _ = t.join();
            }
        }
        if let Some(e) = self.engine.take() {
            e.stop();
        }
    }
}

impl Drop for Battle {
    fn drop(&mut self) {
        self.teardown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battle_start_refuses_an_old_worker_and_an_empty_topic() {
        assert_eq!(battle_start("x", false).unwrap_err().key, MsgKey::BattleUnsupported);
        let e = battle_start(" \t ", true).unwrap_err();
        assert_eq!((e.key, e.code), (MsgKey::BattleTopic, ErrorCode::Settings));
        assert_eq!(battle_start("  Darf man den letzten Keks nehmen?  ", true).unwrap(), "Darf man den letzten Keks nehmen?");
    }

    #[test]
    fn mix_sums_both_voices_and_clamps() {
        assert_eq!(mix(&[0.1, 0.8, -0.8, 0.0], &[0.2, 0.7, -0.7, 0.0]), vec![0.1 + 0.2, 1.0, -1.0, 0.0]);
    }

    #[test]
    fn frame_pairs_join_the_angel_and_the_daemon_of_one_seq() {
        let mut p = FramePairs::default();
        assert!(p.accept(Some(0), 4, vec![0.1], "a".into()).unwrap().is_none());
        let pair = p.accept(Some(1), 4, vec![0.2], "d".into()).unwrap().unwrap();
        assert_eq!(pair.seq, 4);
        assert_eq!(pair.frames, [vec![0.1], vec![0.2]]);
        assert_eq!(pair.texts, ["a".to_string(), "d".to_string()]);
    }

    #[test]
    fn frame_pairs_never_give_a_pair_twice_and_know_what_waits() {
        let mut p = FramePairs::default();
        assert!(!p.pending());
        p.accept(Some(0), 3, vec![0.1], String::new()).unwrap();
        assert!(p.pending());
        assert!(p.accept(Some(1), 3, vec![0.2], String::new()).unwrap().is_some());
        assert!(!p.pending());
        assert!(p.accept(Some(0), 3, vec![0.1], String::new()).is_err(), "a duplicate pair");
        assert!(p.accept(Some(0), 2, vec![0.1], String::new()).is_err(), "an older pair");
        assert!(p.accept(Some(0), 4, vec![0.1], String::new()).is_ok());
    }

    #[test]
    fn frame_pairs_refuse_frames_of_unequal_length() {
        let mut p = FramePairs::default();
        p.accept(Some(0), 0, vec![0.0; 3], String::new()).unwrap();
        assert!(p.accept(Some(1), 0, vec![0.0; 2], String::new()).is_err());
    }

    #[test]
    fn frame_pairs_refuse_what_the_worker_never_sends() {
        let mut p = FramePairs::default();
        assert!(p.accept(None, 0, vec![], String::new()).is_err(), "a chunk without speaker");
        assert!(p.accept(Some(1), 0, vec![], String::new()).is_err(), "the daemon first");
        assert!(p.accept(Some(2), 0, vec![], String::new()).is_err(), "a third speaker");
        p.accept(Some(0), 0, vec![], String::new()).unwrap();
        assert!(p.accept(Some(1), 1, vec![], String::new()).is_err(), "another seq");
        p.accept(Some(0), 0, vec![], String::new()).unwrap();
        assert!(p.accept(Some(0), 1, vec![], String::new()).is_err(), "two angel frames in a row");
    }

    use crate::audio::voice_fx::VoiceStyle;
    use crate::ipc::VizFrame;
    use crate::worker::pcm::encode_f32;
    use crate::worker::protocol::WorkerErrorBody;
    use serde_json::{json, Value};
    use std::io::Write;
    use std::sync::mpsc::Sender;

    /// Fake worker stdin: splits the written bytes into lines and forwards each as JSON.
    struct LineTap {
        buf: Vec<u8>,
        tx: Sender<Value>,
    }

    impl Write for LineTap {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.buf.extend_from_slice(data);
            while let Some(pos) = self.buf.iter().position(|b| *b == 10) {
                let line: Vec<u8> = self.buf.drain(..=pos).collect();
                let _ = self.tx.send(serde_json::from_slice(&line[..line.len() - 1]).expect("json line"));
            }
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct Fixture {
        lines: Receiver<Value>,
        worker: Sender<WorkerMsg>,
        sender: WorkerSender,
        events: Receiver<WorkerMsg>,
    }

    fn fixture() -> Fixture {
        let (ltx, lines) = mpsc::channel();
        let (worker, events) = mpsc::channel();
        Fixture { lines, worker, sender: WorkerSender::with_writer(LineTap { buf: Vec::new(), tx: ltx }, 512), events }
    }

    fn next_line(rx: &Receiver<Value>, pred: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let v = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("expected line within 5 s");
            if pred(&v) {
                return v;
            }
        }
    }

    fn wait_until(what: &str, f: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !f() {
            assert!(Instant::now() < deadline, "{what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn chunk(speaker: u8, seq: u64, pcm: &[f32], text: &str) -> WorkerMsg {
        WorkerMsg::Chunk { id: "b1".into(), seq, pcm: encode_f32(pcm), text: Some(text.into()), speaker: Some(speaker) }
    }

    /// `frames` frames of a sine at `hz` as the wire quantises them.
    fn sine(hz: f32, frames: usize) -> Vec<f32> {
        let x: Vec<f32> = (0..frames * FRAME_SAMPLES).map(|i| 0.3 * (2.0 * std::f32::consts::PI * hz * i as f32 / 24000.0).sin()).collect();
        decode_f32(&encode_f32(&x)).unwrap()
    }

    fn frame(x: &[f32], seq: usize) -> &[f32] {
        &x[seq * FRAME_SAMPLES..(seq + 1) * FRAME_SAMPLES]
    }

    /// Queues `frames` pairs of the two signals with texts `a<seq>` / `d<seq>`, then `end` when asked.
    fn queue(fx: &Fixture, angel: &[f32], daemon: &[f32], frames: usize, end: bool) {
        for seq in 0..frames {
            fx.worker.send(chunk(0, seq as u64, frame(angel, seq), &format!("a{seq}"))).unwrap();
            fx.worker.send(chunk(1, seq as u64, frame(daemon, seq), &format!("d{seq}"))).unwrap();
        }
        if end {
            fx.worker.send(WorkerMsg::End { id: "b1".into(), frames: Some(frames as u64), samples: None }).unwrap();
        }
    }

    struct Run {
        battle: Battle,
        ended: Arc<Mutex<Vec<ConversationEnd>>>,
        viz: Arc<Mutex<Vec<VizFrame>>>,
        tap: Arc<Mutex<Vec<f32>>>,
    }

    fn start(events: Receiver<WorkerMsg>, sender: WorkerSender) -> Result<Run, HaruError> {
        let (ended, viz, tap) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())));
        let (e2, v2) = (ended.clone(), viz.clone());
        let io = ConversationIo { no_playback: true, push_tap: Some(tap.clone()), ..Default::default() };
        let settings = Settings::default();
        let battle = Battle::start_with(sender, events, &settings, settings.battle_options("Ist eine Notlüge erlaubt?"), MAX_FRAMES,
            Arc::new(move |f| v2.lock().unwrap().push(f)), Arc::new(move |e| e2.lock().unwrap().push(e)), "b1".into(), &io)?;
        Ok(Run { battle, ended, viz, tap })
    }

    #[test]
    fn sends_the_battle_line_and_mixes_the_daemon_with_its_effect_into_the_ring() {
        let fx = fixture();
        let (angel, daemon) = (sine(220.0, 2), sine(140.0, 2));
        queue(&fx, &angel, &daemon, 2, true);
        let run = start(fx.events, fx.sender.clone()).expect("start");
        let line = next_line(&fx.lines, |v| v["type"] == "battle");
        assert_eq!((line["id"].clone(), line["max_frames"].clone()), (json!("b1"), json!(4500)));
        assert!(line.get("sample_rate").is_none() && line.get("seed").is_none());
        let speakers = line["speakers"].as_array().unwrap();
        assert_eq!((speakers[0]["name"].clone(), speakers[1]["name"].clone()), (json!("Haru"), json!("Mephisto")));
        assert!(speakers.iter().all(|s| s.get("max_seconds").is_none() && s.get("sample_rate").is_none()));
        wait_until("the battle plays out and ends", || !run.ended.lock().unwrap().is_empty());
        assert!(matches!(run.ended.lock().unwrap()[0], ConversationEnd::HungUp));
        let mut demon = VoiceFx::new(VoiceStyle::Demon);
        let (mut rs, mut rs_dry) = (FrameResampler::new(24000, VIRTUAL_RATE, FRAME_SAMPLES).unwrap(), FrameResampler::new(24000, VIRTUAL_RATE, FRAME_SAMPLES).unwrap());
        let (mut expected, mut dry) = (Vec::new(), Vec::new());
        for seq in 0..2 {
            let mut d = frame(&daemon, seq).to_vec();
            dry.extend(rs_dry.process(&mix(frame(&angel, seq), &d)));
            demon.process_in_place(&mut d);
            expected.extend(rs.process(&mix(frame(&angel, seq), &d)));
        }
        let tap = run.tap.lock().unwrap().clone();
        assert!(tap.len() >= expected.len(), "pushed {} of {}", tap.len(), expected.len());
        assert_eq!(&tap[..expected.len()], &expected[..]);
        assert_ne!(&tap[..dry.len()], &dry[..], "the daemon's effect must change the mix");
    }

    #[test]
    fn the_angel_reaches_the_ring_unchanged_when_the_daemon_is_silent() {
        let fx = fixture();
        let angel = sine(220.0, 2);
        queue(&fx, &angel, &vec![0.0; 2 * FRAME_SAMPLES], 2, true);
        let run = start(fx.events, fx.sender.clone()).expect("start");
        wait_until("the battle ends", || !run.ended.lock().unwrap().is_empty());
        let mut rs = FrameResampler::new(24000, VIRTUAL_RATE, FRAME_SAMPLES).unwrap();
        let expected: Vec<f32> = (0..2).flat_map(|seq| rs.process(frame(&angel, seq))).collect();
        assert_eq!(&run.tap.lock().unwrap()[..expected.len()], &expected[..]);
    }

    #[test]
    fn viz_frames_carry_both_voices_and_speaker_words_with_a_silent_mic() {
        let fx = fixture();
        queue(&fx, &sine(220.0, 2), &sine(140.0, 2), 2, true);
        let run = start(fx.events, fx.sender.clone()).expect("start");
        wait_until("the battle ends", || !run.ended.lock().unwrap().is_empty());
        thread::sleep(Duration::from_millis(50));
        let frames = run.viz.lock().unwrap().clone();
        let words: Vec<_> = frames.iter().flat_map(|f| f.words.clone()).collect();
        assert!(words.iter().any(|w| w.speaker == Some(Speaker::Angel) && w.text == "a0"), "{words:?}");
        assert!(words.iter().any(|w| w.speaker == Some(Speaker::Daemon) && w.text == "d0"), "{words:?}");
        assert!(frames.iter().all(|f| f.battle.is_some() && f.latency_ms == 0.0 && f.mic.rms == 0.0 && f.mic.bands == [0.0; 8]));
        assert!(frames.iter().any(|f| f.battle.as_ref().unwrap().daemon.rms > 0.0 && f.model.rms > 0.0));
        drop(run.battle);
    }

    #[test]
    fn stop_sends_hang_up_and_returns_once_the_worker_ends() {
        let fx = fixture();
        queue(&fx, &sine(220.0, 1), &sine(140.0, 1), 1, false);
        let run = start(fx.events, fx.sender.clone()).expect("start");
        let ended = run.ended.clone();
        let battle = run.battle;
        let t = thread::spawn(move || battle.hang_up(Duration::from_secs(3)));
        next_line(&fx.lines, |v| v["type"] == "hang_up" && v["id"] == "b1");
        fx.worker.send(WorkerMsg::End { id: "b1".into(), frames: Some(1), samples: None }).unwrap();
        assert!(t.join().unwrap().is_ok());
        assert!(ended.lock().unwrap().is_empty(), "a stop never reports an end of its own");
    }

    #[test]
    fn an_angel_frame_without_its_daemon_frame_at_the_end_is_a_bad_frame() {
        let fx = fixture();
        queue(&fx, &sine(220.0, 1), &sine(140.0, 1), 1, false);
        fx.worker.send(chunk(0, 1, &vec![0.0; FRAME_SAMPLES], "a1")).unwrap();
        fx.worker.send(WorkerMsg::End { id: "b1".into(), frames: Some(2), samples: None }).unwrap();
        let run = start(fx.events, fx.sender.clone()).expect("start");
        wait_until("the battle fails", || !run.ended.lock().unwrap().is_empty());
        assert!(matches!(&run.ended.lock().unwrap()[0], ConversationEnd::Failed(e) if e.key == MsgKey::WorkerBadFrame));
    }

    #[test]
    fn a_refusal_before_the_first_frame_is_a_worker_error_that_keeps_the_backend() {
        let fx = fixture();
        fx.worker.send(WorkerMsg::Error { id: Some("b1".into()), error: WorkerErrorBody { code: "out_of_memory".into(),
            option: Some("max_frames".into()), message: "speaker 1: the keys and values do not fit".into() } }).unwrap();
        let e = start(fx.events, fx.sender.clone()).err().expect("must fail");
        assert_eq!(e.key, MsgKey::WorkerError);
        assert!(!e.backend_failure);
        assert!(e.detail.unwrap().contains("out_of_memory"));
    }

    /// Pairs of silence for the seqs `from..to`.
    fn queue_silence(worker: &Sender<WorkerMsg>, from: usize, to: usize) {
        for seq in from..to {
            worker.send(chunk(0, seq as u64, &vec![0.0; FRAME_SAMPLES], "a")).unwrap();
            worker.send(chunk(1, seq as u64, &vec![0.0; FRAME_SAMPLES], "d")).unwrap();
        }
    }

    fn failed_with(run: &Run) -> HaruError {
        match &run.ended.lock().unwrap()[0] {
            ConversationEnd::Failed(e) => e.clone(),
            ConversationEnd::HungUp => panic!("the battle was expected to fail"),
        }
    }

    #[test]
    fn an_unexpected_cancelled_fails_the_battle() {
        let fx = fixture();
        queue(&fx, &sine(220.0, 1), &sine(140.0, 1), 1, false);
        let run = start(fx.events, fx.sender.clone()).expect("start");
        fx.worker.send(WorkerMsg::Cancelled { id: "b1".into() }).unwrap();
        wait_until("the battle fails", || !run.ended.lock().unwrap().is_empty());
        let e = failed_with(&run);
        assert_eq!(e.key, MsgKey::ConversationAborted);
        assert!(e.backend_failure);
    }

    #[test]
    fn a_fatal_mid_run_fails_the_battle_as_a_backend_failure() {
        let fx = fixture();
        queue(&fx, &sine(220.0, 1), &sine(140.0, 1), 1, false);
        let run = start(fx.events, fx.sender.clone()).expect("start");
        fx.worker.send(WorkerMsg::Fatal { error: WorkerErrorBody { code: "device".into(), option: None, message: "lost".into() } }).unwrap();
        wait_until("the battle fails", || !run.ended.lock().unwrap().is_empty());
        let e = failed_with(&run);
        assert_eq!(e.key, MsgKey::WorkerError);
        assert!(e.backend_failure);
    }

    #[test]
    fn an_end_with_another_frame_count_than_generated_is_a_bad_frame() {
        let fx = fixture();
        queue(&fx, &sine(220.0, 2), &sine(140.0, 2), 2, false);
        fx.worker.send(WorkerMsg::End { id: "b1".into(), frames: Some(3), samples: None }).unwrap();
        let run = start(fx.events, fx.sender.clone()).expect("start");
        wait_until("the battle fails", || !run.ended.lock().unwrap().is_empty());
        assert_eq!(failed_with(&run).key, MsgKey::WorkerBadFrame);
    }

    #[test]
    fn a_hang_up_the_worker_ignores_is_escalated_to_a_cancel() {
        let fx = fixture();
        queue(&fx, &sine(220.0, 1), &sine(140.0, 1), 1, false);
        let run = start(fx.events, fx.sender.clone()).expect("start");
        let battle = run.battle;
        let t = thread::spawn(move || battle.hang_up(Duration::from_millis(200)));
        next_line(&fx.lines, |v| v["type"] == "hang_up" && v["id"] == "b1");
        next_line(&fx.lines, |v| v["type"] == "cancel" && v["id"] == "b1");
        fx.worker.send(WorkerMsg::Cancelled { id: "b1".into() }).unwrap();
        assert!(t.join().unwrap().is_ok());
    }

    #[test]
    fn an_audio_failure_cancels_the_worker_and_fails_the_battle() {
        let fx = fixture();
        queue(&fx, &sine(220.0, 1), &sine(140.0, 1), 1, false);
        let run = start(fx.events, fx.sender.clone()).expect("start");
        run.battle.probe.failed.store(true, Ordering::Release);
        next_line(&fx.lines, |v| v["type"] == "cancel" && v["id"] == "b1");
        wait_until("the battle fails", || !run.ended.lock().unwrap().is_empty());
        assert_eq!(failed_with(&run).key, MsgKey::AudioDeviceLost);
    }

    #[test]
    fn nothing_ends_a_battle_after_a_stop() {
        let fx = fixture();
        queue(&fx, &sine(220.0, 1), &sine(140.0, 1), 1, false);
        let run = start(fx.events, fx.sender.clone()).expect("start");
        let (inner, ended) = (run.battle.inner.clone(), run.ended.clone());
        drop(run.battle);
        inner.fire_end(ConversationEnd::Failed(HaruError::new(ErrorCode::AudioDevice, MsgKey::AudioDeviceLost)));
        inner.fire_end(ConversationEnd::HungUp);
        assert!(ended.lock().unwrap().is_empty(), "a stopped battle must not call on_end: its caller may hold a lock");
    }

    #[test]
    fn the_buffer_pauses_when_it_runs_dry_and_resumes_after_22_new_frames() {
        let fx = fixture();
        queue_silence(&fx.worker, 0, 22);
        let run = start(fx.events, fx.sender.clone()).expect("start");
        let phases = |run: &Run| -> Vec<BattlePhase> {
            let mut p: Vec<BattlePhase> = run.viz.lock().unwrap().iter().filter_map(|f| f.battle.as_ref().map(|b| b.phase)).filter(|p| *p != BattlePhase::Preparing).collect();
            p.dedup();
            p
        };
        // The 22 frames play for about 2.9 s of real time, then nothing is left: paused.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !phases(&run).contains(&BattlePhase::Paused) {
            assert!(Instant::now() < deadline, "never paused: {:?}", phases(&run));
            thread::sleep(Duration::from_millis(10));
        }
        queue_silence(&fx.worker, 22, 44);
        let deadline = Instant::now() + Duration::from_secs(10);
        while phases(&run).last() != Some(&BattlePhase::Playing) || !phases(&run).contains(&BattlePhase::Paused) {
            assert!(Instant::now() < deadline, "never resumed: {:?}", phases(&run));
            thread::sleep(Duration::from_millis(10));
        }
        fx.worker.send(WorkerMsg::End { id: "b1".into(), frames: Some(44), samples: None }).unwrap();
        assert_eq!(phases(&run), vec![BattlePhase::Playing, BattlePhase::Paused, BattlePhase::Playing]);
    }
}
