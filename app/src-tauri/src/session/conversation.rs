//! One conversation: dispatch, playback feed, viz ticker (Task 1.7).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::audio::aec::{Stages, VoiceProcessor, LATENCY_UNKNOWN};
use crate::audio::capture::CaptureHandle;
use crate::audio::engine::{AudioEngine, PREBUFFER_FRAMES};
use crate::audio::features::{Analyzer, BLOCK, MODEL_BANDS};
use crate::audio::playback::{JitterGate, PlaybackHandle, PlaybackShared};
use crate::audio::resample::FrameResampler;
use crate::error::{ErrorCode, HaruError};
use crate::ipc::VizFrame;
use crate::session::latency::{backlog_secs, classify, LagLevel};
use crate::session::mic_pump::{spawn_mic_pump, spawn_sample_feeder, EchoInput, MicShared};
use crate::session::timeline::FrameTimeline;
use crate::session::viz::{build_frame, FeatureClock};
use crate::settings::Settings;
use crate::worker::pcm::decode_f32;
use crate::worker::process::WorkerSender;
use crate::worker::protocol::{is_segment_limit, worker_error, ClientMsg, DialogueOptions, WorkerMsg};

/// Shown when echo cancellation is wanted but cannot be set up for this conversation.
pub const AEC_UNAVAILABLE_WARNING: &str = "Echounterdrückung nicht verfügbar – bitte Kopfhörer verwenden.";
pub const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(30);
pub const FRAME_SAMPLES: usize = 3200;
pub const TICK: Duration = Duration::from_millis(16);
/// A queued tail shorter than the prebuffer is flushed after this much silence from the worker.
pub const TAIL_IDLE: Duration = Duration::from_millis(400);
/// Output rate of the virtual (no-playback) sink when no device is open.
const VIRTUAL_RATE: u32 = 48000;

pub type VizSink = Arc<dyn Fn(VizFrame) + Send + Sync>;

/// How a conversation continues when the worker ends a request because `max_seconds` ran out:
/// the next request (a fresh dialogue on the same loaded model) starts inside the same conversation.
#[derive(Clone)]
pub struct Renewal {
    /// Fresh max_seconds of the next segment (blocking: runs `speech devices --json`).
    pub max_seconds: Arc<dyn Fn() -> f64 + Send + Sync>,
    /// Called once the next segment's dialogue line is queued.
    pub on_renewed: Arc<dyn Fn(u32) + Send + Sync>,
}

/// The worker request currently serving the conversation.
struct Request {
    id: String,
    /// False between segments (the old request ended, the next one is not queued yet).
    in_flight: bool,
    segment: u32,
}

#[derive(Debug, Clone)]
pub enum ConversationEnd {
    HungUp,
    Failed(HaruError),
}

/// Test / headless input and output substitutes. The default uses the real devices.
#[derive(Clone, Default)]
pub struct ConversationIo {
    /// Mono samples fed to the worker instead of the microphone (paced in real time, starting
    /// at frame 0), followed by silence.
    pub input_samples: Option<(u32, Vec<f32>)>,
    /// Do not make any sound: Haru's audio is consumed by a virtual real-time sink.
    pub no_playback: bool,
}

/// Output-rate index of each of the 8 model blocks of one frame (`start + round(i*400*rate/24000)`).
pub fn block_starts(start: u64, out_rate: u32) -> Vec<u64> {
    let n = FRAME_SAMPLES / BLOCK;
    (0..n as u64).map(|i| start + (i * BLOCK as u64 * out_rate as u64 + 12000) / 24000).collect()
}

pub fn block_len_out(out_rate: u32) -> u64 {
    (BLOCK as u64 * out_rate as u64 + 12000) / 24000
}

pub fn prebuffer_samples(out_rate: u32) -> u64 {
    (3200 * out_rate as u64 / 24000) * PREBUFFER_FRAMES as u64
}

/// Silence (output samples) to append so that a queued tail shorter than the prebuffer becomes
/// playable. Without it the jitter gate keeps the tail after an underrun and `played` never
/// reaches the last frames, so the last words would never become audible.
pub fn flush_padding(buffered: u64, prebuffer: u64, gate_playing: bool, idle: Duration, ended: bool) -> u64 {
    if buffered == 0 || buffered >= prebuffer || gate_playing {
        return 0;
    }
    if ended || idle >= TAIL_IDLE {
        prebuffer - buffered
    } else {
        0
    }
}

/// Where Haru's audio goes: the real device queue or a virtual real-time consumer.
struct Out {
    rate: u32,
    shared: Arc<PlaybackShared>,
    pushed: Arc<AtomicU64>,
    real: Option<PlaybackHandle>,
}

impl Out {
    fn push(&mut self, s: &[f32]) -> u64 {
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

/// Echo cancellation needs the real speaker output as reference and the real microphone: not with the virtual
/// (no-playback) sink or a WAV feeder.
pub fn wants_echo_cancellation(s: &Settings, io: &ConversationIo) -> bool {
    s.echo_cancellation && !io.no_playback && io.input_samples.is_none()
}

fn new_shared_playback() -> Arc<PlaybackShared> {
    Arc::new(PlaybackShared::default())
}

fn spawn_virtual_player(rate: u32, shared: Arc<PlaybackShared>, pushed: Arc<AtomicU64>, stop: Arc<Inner>) -> JoinHandle<()> {
    thread::Builder::new()
        .name("haru-virtual-out".into())
        .spawn(move || {
            let mut gate = JitterGate::new(prebuffer_samples(rate) as usize);
            let t0 = Instant::now();
            let mut done = 0u64;
            while !stop.stop.load(Ordering::Acquire) {
                let due = (t0.elapsed().as_secs_f64() * rate as f64) as u64;
                let need = due - done;
                if need >= (rate / 100) as u64 {
                    let avail = pushed.load(Ordering::Acquire).saturating_sub(shared.played.load(Ordering::Relaxed));
                    let n = gate.decide(avail as usize, need as usize);
                    shared.played.fetch_add(n as u64, Ordering::Relaxed);
                    shared.underruns.store(gate.underruns, Ordering::Relaxed);
                    shared.playing.store(n > 0, Ordering::Relaxed);
                    done += need;
                }
                thread::sleep(Duration::from_millis(2));
            }
        })
        .expect("spawn virtual player")
}

struct Inner {
    /// Stable key of the conversation (`Conversation::id`); worker request ids derive from it.
    id: String,
    request: Mutex<Request>,
    options: DialogueOptions,
    renewal: Option<Renewal>,
    sender: WorkerSender,
    stop: AtomicBool,
    terminal: Mutex<bool>,
    terminal_cv: Condvar,
    end_fired: AtomicBool,
    hanging_up: AtomicBool,
    /// We sent a `cancel` because of the lag limit (a later worker `cancelled` is then LagLimit).
    lag_cancelled: AtomicBool,
    frames: AtomicU64,
    timeline: Mutex<FrameTimeline>,
    clock: Mutex<FeatureClock>,
    mic: Arc<MicShared>,
    mic_rate: u32,
    on_end: Arc<dyn Fn(ConversationEnd) + Send + Sync>,
}

impl Inner {
    fn fire_end(&self, e: ConversationEnd) {
        if !self.end_fired.swap(true, Ordering::AcqRel) {
            (self.on_end)(e);
        }
    }
    /// Sends to the request in flight; Ok(false) when none is (between segments).
    fn send_to_current(&self, msg: impl FnOnce(String) -> ClientMsg) -> Result<bool, HaruError> {
        let r = self.request.lock().unwrap_or_else(|e| e.into_inner());
        if !r.in_flight {
            return Ok(false);
        }
        self.sender.send(&msg(r.id.clone())).map(|_| true)
    }
    fn set_terminal(&self) {
        *self.terminal.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.terminal_cv.notify_all();
    }
    fn has_terminal(&self) -> bool {
        *self.terminal.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn wait_terminal(&self, timeout: Duration) -> bool {
        let g = self.terminal.lock().unwrap_or_else(|e| e.into_inner());
        let (g, _) = self.terminal_cv.wait_timeout_while(g, timeout, |t| !*t).unwrap_or_else(|e| e.into_inner());
        *g
    }
}

/// Ends the current segment's request bookkeeping and queues the next dialogue line (blocking while
/// the fresh budget is measured). Ok(None): the conversation is being hung up or stopped.
fn renew_segment(inner: &Inner) -> Result<Option<String>, HaruError> {
    let Some(renewal) = inner.renewal.as_ref() else { return Ok(None) };
    inner.request.lock().unwrap_or_else(|e| e.into_inner()).in_flight = false;
    // Audio heard from now until frame 0 of the next segment is dropped by the pump.
    inner.mic.started.store(false, Ordering::Release);
    // The old KV cache is free (the worker answered); measure without holding any lock.
    let secs = (renewal.max_seconds)();
    let (segment, new_id) = {
        let mut r = inner.request.lock().unwrap_or_else(|e| e.into_inner());
        if inner.hanging_up.load(Ordering::Acquire) || inner.stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        let segment = r.segment + 1;
        let new_id = format!("{}-s{}", inner.id, segment);
        let mut options = inner.options.clone();
        options.speak_first = None;
        options.max_seconds = Some(secs);
        // The dialogue line must be queued before the pump learns the new id (a chunk of an unknown
        // request would be an id-less worker error).
        inner.sender.send(&ClientMsg::Dialogue { id: new_id.clone(), sample_rate: inner.mic_rate, options })?;
        r.segment = segment;
        r.id = new_id.clone();
        r.in_flight = true;
        *inner.mic.request_id.lock().unwrap_or_else(|e| e.into_inner()) = new_id.clone();
        inner.mic.segment.fetch_add(1, Ordering::Release);
        inner.mic.sent_samples.store(0, Ordering::Relaxed);
        inner.frames.store(0, Ordering::Release);
        (segment, new_id)
    };
    eprintln!("haru: conversation {}: segment {segment} ({new_id}), max_seconds {secs}", inner.id);
    (renewal.on_renewed)(segment);
    Ok(Some(new_id))
}

pub struct Conversation {
    inner: Arc<Inner>,
    engine: Option<AudioEngine>,
    threads: Vec<JoinHandle<()>>,
    lag_limit_secs: f64,
    startup_warning: Option<String>,
}

fn dispatch_loop(
    inner: Arc<Inner>,
    events: Receiver<WorkerMsg>,
    mut out: Out,
    first_tx: mpsc::Sender<Result<(), HaruError>>,
) {
    let mut resampler = match FrameResampler::new(24000, out.rate, FRAME_SAMPLES) {
        Ok(r) => r,
        Err(e) => {
            let _ = first_tx.send(Err(e));
            return;
        }
    };
    let mut analyzer = Analyzer::new(24000, BLOCK, MODEL_BANDS, 60.0, 12000.0);
    let prebuffer = prebuffer_samples(out.rate);
    let first_deadline = Instant::now() + FIRST_FRAME_TIMEOUT;
    let mut got_first = false;
    // Worker request id of the current segment; frame numbering continues across segments.
    let mut rid = inner.id.clone();
    let mut seg_first = false;
    let mut seg_deadline = first_deadline;
    let mut seq_base = 0u64;
    let mut next_seq = 0u64;
    let mut ended = false;
    let mut last_push = Instant::now();
    let lost = |d: &str| {
        HaruError::new(ErrorCode::Worker, "Die Verbindung zum Sprachmodell-Worker ist abgebrochen.")
            .with_detail(d.to_string())
            .backend_failure()
    };
    // Reports a failure to whoever is waiting: `start` before frame 0, `on_end` after.
    macro_rules! fail {
        ($e:expr) => {{
            let e: HaruError = $e;
            inner.set_terminal();
            if got_first {
                inner.fire_end(ConversationEnd::Failed(e));
            } else {
                let _ = first_tx.send(Err(e));
                return;
            }
            ended = true;
        }};
    }
    while !inner.stop.load(Ordering::Acquire) {
        let wait = if got_first { TICK * 3 } else { first_deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(50)) };
        match events.recv_timeout(wait) {
            Ok(WorkerMsg::Chunk { id, seq, pcm, text }) if id == rid => {
                let pcm = match decode_f32(&pcm) {
                    Ok(p) if p.len() == FRAME_SAMPLES => p,
                    Ok(p) => {
                        fail!(HaruError::new(ErrorCode::Worker, "Der Worker lieferte ein ungültiges Audio-Frame.")
                            .with_detail(format!("frame {seq} has {} samples", p.len()))
                            .backend_failure());
                        continue;
                    }
                    Err(e) => {
                        fail!(e.backend_failure());
                        continue;
                    }
                };
                let resampled = resampler.process(&pcm);
                let start = out.push(&resampled);
                inner.timeline.lock().unwrap_or_else(|e| e.into_inner()).push(seq_base + seq, text.unwrap_or_default(), start);
                next_seq = next_seq.max(seq_base + seq + 1);
                {
                    let starts = block_starts(start, out.rate);
                    let mut clock = inner.clock.lock().unwrap_or_else(|e| e.into_inner());
                    for (i, block) in pcm.chunks_exact(BLOCK).enumerate() {
                        clock.push(starts[i], analyzer.analyze(block));
                    }
                }
                inner.frames.fetch_add(1, Ordering::AcqRel);
                last_push = Instant::now();
                if !seg_first && seq == 0 {
                    seg_first = true;
                    inner.mic.started.store(true, Ordering::Release);
                    if !got_first {
                        got_first = true;
                        let _ = first_tx.send(Ok(()));
                    }
                }
            }
            Ok(WorkerMsg::End { id, .. }) if id == rid => {
                inner.set_terminal();
                ended = true;
                if !inner.hanging_up.load(Ordering::Acquire) {
                    inner.fire_end(ConversationEnd::HungUp);
                }
            }
            Ok(WorkerMsg::Cancelled { id }) if id == rid => {
                inner.set_terminal();
                ended = true;
                if !inner.hanging_up.load(Ordering::Acquire) {
                    let e = if inner.lag_cancelled.load(Ordering::Acquire) {
                        lag_error(None)
                    } else {
                        HaruError::new(ErrorCode::Worker, "Das Gespräch wurde abgebrochen.").backend_failure()
                    };
                    inner.fire_end(ConversationEnd::Failed(e));
                }
            }
            Ok(WorkerMsg::Error { id, error }) if id.as_deref().map_or(true, |i| i == rid) => {
                let renewable = got_first
                    && seg_first
                    && id.is_some()
                    && is_segment_limit(&error)
                    && inner.renewal.is_some()
                    && !inner.hanging_up.load(Ordering::Acquire);
                if renewable {
                    match renew_segment(&inner) {
                        Ok(Some(new_id)) => {
                            // Closes the word the old segment cut off (" " ends a word in the assembler).
                            inner.timeline.lock().unwrap_or_else(|e| e.into_inner()).push(
                                next_seq,
                                " ".into(),
                                out.pushed.load(Ordering::Acquire),
                            );
                            seq_base = next_seq + 1;
                            next_seq = seq_base;
                            rid = new_id;
                            seg_first = false;
                            seg_deadline = Instant::now() + FIRST_FRAME_TIMEOUT;
                        }
                        Ok(None) => {
                            inner.set_terminal();
                            ended = true;
                        }
                        Err(e) => fail!(e),
                    }
                } else {
                    let e = worker_error(&error);
                    fail!(if got_first { e } else { e.backend_failure() });
                }
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {
                if !got_first && Instant::now() >= first_deadline {
                    let _ = inner.send_to_current(|id| ClientMsg::Cancel { id });
                    let _ = first_tx.send(Err(HaruError::new(
                        ErrorCode::Worker,
                        "Der Sprachmodell-Worker antwortet nicht.",
                    )
                    .with_detail("no frame 0 within 30 s")
                    .backend_failure()));
                    return;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                if !ended {
                    fail!(lost("worker event channel closed"));
                } else {
                    thread::sleep(Duration::from_millis(20));
                }
            }
        }
        if got_first && !seg_first && !ended && Instant::now() >= seg_deadline {
            let _ = inner.send_to_current(|id| ClientMsg::Cancel { id });
            fail!(HaruError::new(ErrorCode::Worker, "Der Sprachmodell-Worker antwortet nicht.")
                .with_detail("no frame 0 of the next segment within 30 s")
                .backend_failure());
        }
        let buffered = out.pushed.load(Ordering::Acquire).saturating_sub(out.shared.played.load(Ordering::Relaxed));
        let pad = flush_padding(buffered, prebuffer, out.shared.playing.load(Ordering::Relaxed), last_push.elapsed(), ended);
        if pad > 0 {
            out.push(&vec![0.0; pad as usize]);
        }
    }
}

fn lag_error(backlog: Option<f64>) -> HaruError {
    let msg = match backlog {
        Some(b) => format!("Haru kommt nicht hinterher (Rückstand {b:.1} s). Die GPU ist zu langsam oder ausgelastet."),
        None => "Haru kommt nicht hinterher. Die GPU ist zu langsam oder ausgelastet.".to_string(),
    };
    HaruError::new(ErrorCode::LagLimit, msg)
}

#[allow(clippy::too_many_arguments)]
fn ticker_loop(
    inner: Arc<Inner>,
    on_viz: VizSink,
    shared: Arc<PlaybackShared>,
    pushed: Arc<AtomicU64>,
    out_rate: u32,
    lag_limit: f64,
    capture_failed: Arc<AtomicBool>,
) {
    while !inner.stop.load(Ordering::Acquire) {
        if !inner.has_terminal()
            && !inner.end_fired.load(Ordering::Acquire)
            && (shared.failed.load(Ordering::Acquire) || capture_failed.load(Ordering::Acquire))
        {
            let _ = inner.send_to_current(|id| ClientMsg::Cancel { id });
            inner.fire_end(ConversationEnd::Failed(HaruError::new(
                ErrorCode::AudioDevice,
                "Audiogerät nicht mehr verfügbar.",
            )));
        }
        let played = shared.played.load(Ordering::Relaxed);
        let words = inner.timeline.lock().unwrap_or_else(|e| e.into_inner()).drain_audible(played);
        let model = inner.clock.lock().unwrap_or_else(|e| e.into_inner()).current(played);
        let mic = inner.mic.levels.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let buffered = pushed.load(Ordering::Acquire).saturating_sub(played);
        let backlog = backlog_secs(
            inner.mic.sent_samples.load(Ordering::Relaxed),
            inner.mic_rate,
            inner.frames.load(Ordering::Acquire),
            buffered,
            out_rate,
        );
        if !inner.has_terminal() {
            on_viz(build_frame(played as f64 / out_rate as f64, model, mic, backlog * 1000.0, words));
            if !inner.end_fired.load(Ordering::Acquire) && classify(backlog, lag_limit) == LagLevel::Exceeded {
                inner.lag_cancelled.store(true, Ordering::Release);
                let _ = inner.send_to_current(|id| ClientMsg::Cancel { id });
                inner.fire_end(ConversationEnd::Failed(lag_error(Some(backlog))));
            }
        } else if !words.is_empty() {
            // Keep delivering the last audible words after the worker finished.
            on_viz(build_frame(played as f64 / out_rate as f64, model, mic, backlog * 1000.0, words));
        }
        thread::sleep(TICK);
    }
}

impl Conversation {
    #[allow(clippy::too_many_arguments)]
    pub fn start_with(
        sender: WorkerSender,
        events: Receiver<WorkerMsg>,
        settings: &Settings,
        options: DialogueOptions,
        renewal: Option<Renewal>,
        muted: bool,
        on_viz: VizSink,
        on_end: Arc<dyn Fn(ConversationEnd) + Send + Sync>,
        on_warning: Arc<dyn Fn(String) + Send + Sync>,
        id: String,
        io: &ConversationIo,
    ) -> Result<Conversation, HaruError> {
        // 1. Audio.
        let virtual_both = io.no_playback && io.input_samples.is_some();
        let want_aec = wants_echo_cancellation(settings, io);
        let (engine, cap, mut play) = if virtual_both {
            (None, None, None)
        } else {
            let (e, c, p) = AudioEngine::start(settings.input_device.clone(), settings.output_device.clone(), want_aec)?;
            (Some(e), Some(c), Some(p))
        };

        let mic = Arc::new(MicShared::default());
        *mic.request_id.lock().unwrap_or_else(|e| e.into_inner()) = id.clone();
        mic.muted.store(muted, Ordering::Relaxed);
        let mut feeder: Option<(Vec<f32>, u32, rtrb::Producer<f32>)> = None;
        let capture = match &io.input_samples {
            Some((rate, samples)) => {
                let (producer, consumer) = rtrb::RingBuffer::new(*rate as usize * 2);
                feeder = Some((samples.clone(), *rate, producer));
                CaptureHandle {
                    rate: *rate,
                    consumer,
                    overruns: Arc::new(AtomicU64::new(0)),
                    failed: Arc::new(AtomicBool::new(false)),
                    input_latency_us: Arc::new(AtomicU32::new(LATENCY_UNKNOWN)),
                }
            }
            None => cap.expect("capture present without sample input"),
        };
        let mic_rate = capture.rate;
        let capture_failed = capture.failed.clone();

        // Echo cancellation: reference = render tap of the real output, processed on the mic pump thread.
        let mut startup_warning = None;
        let echo = if want_aec {
            let p = play.as_mut().expect("real playback when echo cancellation is wanted");
            match (p.render_tap.take(), VoiceProcessor::new(mic_rate, p.rate, Stages::Full)) {
                (Some(tap), Ok(vp)) => {
                    eprintln!("haru: echo cancellation on (capture {mic_rate} Hz, output {} Hz)", p.rate);
                    Some(EchoInput::new(vp, tap, p.shared.clone(), on_warning.clone()))
                }
                (_, r) => {
                    eprintln!("haru: echo cancellation unavailable: {}", r.err().map_or("no render tap".into(), |e| e.to_string()));
                    startup_warning = Some(AEC_UNAVAILABLE_WARNING.to_string());
                    None
                }
            }
        } else {
            None
        };

        let inner_probe = Arc::new(Inner {
            id: id.clone(),
            request: Mutex::new(Request { id: id.clone(), in_flight: true, segment: 1 }),
            options,
            renewal,
            sender: sender.clone(),
            stop: AtomicBool::new(false),
            terminal: Mutex::new(false),
            terminal_cv: Condvar::new(),
            end_fired: AtomicBool::new(false),
            hanging_up: AtomicBool::new(false),
            lag_cancelled: AtomicBool::new(false),
            frames: AtomicU64::new(0),
            timeline: Mutex::new(FrameTimeline::new()),
            clock: Mutex::new(FeatureClock::new(1)), // replaced below once the output rate is known
            mic: mic.clone(),
            mic_rate,
            on_end,
        });

        let mut threads: Vec<JoinHandle<()>> = Vec::new();
        let out = if io.no_playback {
            let rate = play.as_ref().map(|p| p.rate).unwrap_or(VIRTUAL_RATE);
            let shared = new_shared_playback();
            let pushed = Arc::new(AtomicU64::new(0));
            threads.push(spawn_virtual_player(rate, shared.clone(), pushed.clone(), inner_probe.clone()));
            Out { rate, shared, pushed, real: None }
        } else {
            let p = play.expect("playback present");
            Out { rate: p.rate, shared: p.shared.clone(), pushed: Arc::new(AtomicU64::new(0)), real: Some(p) }
        };
        let out_rate = out.rate;
        *inner_probe.clock.lock().unwrap() = FeatureClock::new(block_len_out(out_rate));
        let inner = inner_probe;
        let out_shared = out.shared.clone();
        let out_pushed = out.pushed.clone();

        // 2. Dialogue.
        let abort = |inner: &Arc<Inner>, threads: Vec<JoinHandle<()>>| {
            inner.stop.store(true, Ordering::Release);
            inner.mic.stop.store(true, Ordering::Release);
            for t in threads {
                let _ = t.join();
            }
        };
        if let Err(e) = sender.send(&ClientMsg::Dialogue {
            id: id.clone(),
            sample_rate: mic_rate,
            options: inner.options.clone(),
        }) {
            abort(&inner, threads);
            return Err(e);
        }

        // 3. Mic pump (not started yet) and optional sample feeder.
        let fatal_inner = inner.clone();
        let on_fatal: Arc<dyn Fn(HaruError) + Send + Sync> = Arc::new(move |e| fatal_inner.fire_end(ConversationEnd::Failed(e)));
        threads.push(spawn_mic_pump(capture, sender.clone(), mic.clone(), on_fatal, echo));
        if let Some((samples, rate, producer)) = feeder {
            threads.push(spawn_sample_feeder(samples, rate, producer, mic.clone()));
        }

        // 4. Dispatch thread, wait for frame 0.
        let (first_tx, first_rx) = mpsc::channel();
        {
            let inner = inner.clone();
            threads.push(
                thread::Builder::new()
                    .name("haru-dispatch".into())
                    .spawn(move || dispatch_loop(inner, events, out, first_tx))
                    .expect("spawn dispatch thread"),
            );
        }
        let first = first_rx
            .recv_timeout(FIRST_FRAME_TIMEOUT + Duration::from_secs(5))
            .unwrap_or_else(|_| Err(HaruError::new(ErrorCode::Worker, "Der Sprachmodell-Worker antwortet nicht.").backend_failure()));
        if let Err(e) = first {
            let _ = sender.send(&ClientMsg::Cancel { id: id.clone() });
            abort(&inner, threads);
            return Err(e);
        }

        // 5. Viz ticker.
        {
            let inner = inner.clone();
            let lag = settings.lag_limit_secs;
            threads.push(
                thread::Builder::new()
                    .name("haru-viz".into())
                    .spawn(move || ticker_loop(inner, on_viz, out_shared, out_pushed, out_rate, lag, capture_failed))
                    .expect("spawn viz thread"),
            );
        }
        Ok(Conversation { inner, engine, threads, lag_limit_secs: settings.lag_limit_secs, startup_warning })
    }

    pub fn id(&self) -> &str {
        &self.inner.id
    }

    pub fn lag_limit_secs(&self) -> f64 {
        self.lag_limit_secs
    }

    /// Warning for the UI when the conversation started without a feature it was supposed to have.
    pub fn startup_warning(&self) -> Option<String> {
        self.startup_warning.clone()
    }

    pub fn set_muted(&self, muted: bool) {
        self.inner.mic.muted.store(muted, Ordering::Relaxed);
    }

    /// Sends `hang_up`, waits up to `timeout` for `end`, otherwise cancels; then stops threads and audio.
    pub fn hang_up(mut self, timeout: Duration) -> Result<(), HaruError> {
        let stuck = || {
            HaruError::new(ErrorCode::Worker, "Der Sprachmodell-Worker reagiert nicht mehr.")
                .with_detail("no end/cancelled after hang_up")
                .backend_failure()
        };
        self.inner.hanging_up.store(true, Ordering::Release);
        let mut result = Ok(());
        if !self.inner.has_terminal() {
            match self.inner.send_to_current(|id| ClientMsg::HangUp { id }) {
                Err(e) => result = Err(e),
                // Between segments nothing is in flight; the renewal sees `hanging_up` and stops.
                Ok(false) => {}
                Ok(true) => {
                    if !self.inner.wait_terminal(timeout) {
                        match self.inner.send_to_current(|id| ClientMsg::Cancel { id }) {
                            Ok(true) if self.inner.wait_terminal(Duration::from_secs(1)) => {}
                            Ok(false) => {}
                            _ => result = Err(stuck()),
                        }
                    }
                }
            }
        }
        self.teardown();
        result
    }

    /// Ends a conversation that failed: cancels it if the worker has not finished it yet.
    pub fn abort(mut self) -> Result<(), HaruError> {
        self.inner.hanging_up.store(true, Ordering::Release);
        let mut result = Ok(());
        if !self.inner.has_terminal() {
            let sent = self.inner.send_to_current(|id| ClientMsg::Cancel { id });
            let stuck = match sent {
                Ok(true) => !self.inner.wait_terminal(Duration::from_secs(2)),
                Ok(false) => false,
                Err(_) => true,
            };
            if stuck {
                result = Err(HaruError::new(ErrorCode::Worker, "Der Sprachmodell-Worker reagiert nicht mehr.")
                    .with_detail("no cancelled after cancel")
                    .backend_failure());
            }
        }
        self.teardown();
        result
    }

    fn teardown(&mut self) {
        self.inner.stop.store(true, Ordering::Release);
        self.inner.mic.stop.store(true, Ordering::Release);
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

impl Drop for Conversation {
    fn drop(&mut self) {
        self.teardown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_starts_at_48k() {
        assert_eq!(block_starts(0, 48000), vec![0, 800, 1600, 2400, 3200, 4000, 4800, 5600]);
        assert_eq!(block_starts(1000, 48000)[1], 1800);
    }

    #[test]
    fn block_starts_at_44k1() {
        let s = block_starts(0, 44100);
        for (i, v) in s.iter().enumerate() {
            assert_eq!(*v, (i as f64 * 735.0).round() as u64);
        }
        assert_eq!(block_len_out(44100), 735);
    }

    #[test]
    fn flush_pads_short_tail_only_when_stalled() {
        let pre = prebuffer_samples(48000);
        assert_eq!(pre, 12800);
        let idle = TAIL_IDLE;
        assert_eq!(flush_padding(0, pre, false, idle, true), 0);
        assert_eq!(flush_padding(pre, pre, false, idle, true), 0);
        assert_eq!(flush_padding(3000, pre, true, idle, true), 0, "gate still playing");
        assert_eq!(flush_padding(3000, pre, false, Duration::from_millis(100), false), 0, "not idle yet");
        assert_eq!(flush_padding(3000, pre, false, idle, false), pre - 3000);
        assert_eq!(flush_padding(3000, pre, false, Duration::ZERO, true), pre - 3000);
    }

    /// Simulates the jitter gate: a tail shorter than the prebuffer is never played without
    /// padding, and fully played with it, so `played` reaches `pushed`.
    #[test]
    fn short_tail_becomes_playable_after_padding() {
        let rate = 48000u32;
        let pre = prebuffer_samples(rate);
        let need = 480usize;
        let mut gate = JitterGate::new(pre as usize);
        let mut pushed = 0u64;
        let mut played = 0u64;
        let step = |gate: &mut JitterGate, pushed: u64, played: &mut u64| -> bool {
            let n = gate.decide((pushed - *played) as usize, need);
            *played += n as u64;
            n > 0
        };
        // Stream plays, then runs dry (underrun) and a short 1-frame tail (6400 < 12800) arrives.
        pushed += pre + 4000;
        while step(&mut gate, pushed, &mut played) {}
        assert_eq!(played, pushed, "first part plays out");
        pushed += 6400;
        for _ in 0..100 {
            step(&mut gate, pushed, &mut played);
        }
        assert!(played < pushed, "tail stays queued without padding (the known issue)");
        let buffered = pushed - played;
        let pad = flush_padding(buffered, pre, false, TAIL_IDLE, true);
        assert!(pad > 0);
        pushed += pad;
        for _ in 0..200 {
            step(&mut gate, pushed, &mut played);
        }
        assert_eq!(played, pushed, "tail plays completely once padded");
    }

    // ------------------------------------------------------------ segment renewal

    use crate::worker::pcm::encode_f32;
    use crate::worker::protocol::WorkerErrorBody;
    use serde_json::Value;
    use std::io::Write;
    use std::sync::atomic::AtomicU32;
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
                let v: Value = serde_json::from_slice(&line[..line.len() - 1]).expect("json line");
                let _ = self.tx.send(v);
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
        let sender = WorkerSender::with_writer(LineTap { buf: Vec::new(), tx: ltx }, 512);
        Fixture { lines, worker, sender, events }
    }

    fn frame(id: &str, seq: u64) -> WorkerMsg {
        WorkerMsg::Chunk { id: id.into(), seq, pcm: encode_f32(&vec![0.0; FRAME_SAMPLES]), text: Some("x".into()) }
    }

    fn segment_limit(id: &str) -> WorkerMsg {
        WorkerMsg::Error {
            id: Some(id.into()),
            error: WorkerErrorBody { code: "out_of_range".into(), option: Some("max_seconds".into()), message: "limit".into() },
        }
    }

    fn test_io() -> ConversationIo {
        ConversationIo { input_samples: Some((16000, vec![0.0; 16000])), no_playback: true }
    }

    /// Next written line matching `pred` (other lines are skipped), within 5 s.
    fn next_line(rx: &Receiver<Value>, pred: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let v = rx.recv_timeout(left).expect("expected line within 5 s");
            if pred(&v) {
                return v;
            }
        }
    }

    fn is(v: &Value, ty: &str, id: &str) -> bool {
        v["type"] == ty && v["id"] == id
    }

    fn base_options() -> DialogueOptions {
        DialogueOptions { speak_first: Some(true), max_seconds: Some(5.0), ..Default::default() }
    }

    #[test]
    fn echo_cancellation_only_with_real_devices() {
        let on = Settings::default();
        let off = Settings { echo_cancellation: false, ..Default::default() };
        assert!(wants_echo_cancellation(&on, &ConversationIo::default()));
        assert!(!wants_echo_cancellation(&on, &ConversationIo { no_playback: true, ..Default::default() }));
        assert!(!wants_echo_cancellation(&on, &ConversationIo { input_samples: Some((16000, vec![0.0])), ..Default::default() }));
        assert!(!wants_echo_cancellation(&off, &ConversationIo::default()));
    }

    #[test]
    fn renews_segment_on_max_seconds_without_ending() {
        let fx = fixture();
        fx.worker.send(frame("c1", 0)).unwrap();
        let renewed = Arc::new(AtomicU32::new(0));
        let ended = Arc::new(AtomicU32::new(0));
        let r2 = renewed.clone();
        let renewal = Renewal {
            max_seconds: Arc::new(|| 42.0),
            on_renewed: Arc::new(move |seg| r2.store(seg, Ordering::SeqCst)),
        };
        let e2 = ended.clone();
        let on_end: Arc<dyn Fn(ConversationEnd) + Send + Sync> = Arc::new(move |_| {
            e2.fetch_add(1, Ordering::SeqCst);
        });
        let settings = Settings { lag_limit_secs: 10.0, ..Default::default() };
        let conv = Conversation::start_with(
            fx.sender.clone(),
            fx.events,
            &settings,
            base_options(),
            Some(renewal),
            false,
            Arc::new(|_| {}),
            on_end,
            Arc::new(|_| {}),
            "c1".into(),
            &test_io(),
        )
        .expect("start");
        let first = next_line(&fx.lines, |v| is(v, "dialogue", "c1"));
        assert_eq!(first["speak_first"], true);
        next_line(&fx.lines, |v| is(v, "chunk", "c1"));

        fx.worker.send(segment_limit("c1")).unwrap();
        let next = next_line(&fx.lines, |v| is(v, "dialogue", "c1-s2"));
        assert_eq!(next["max_seconds"], 42.0);
        assert!(next.get("speak_first").is_none(), "the next segment must not speak first: {next}");
        let deadline = Instant::now() + Duration::from_secs(5);
        while renewed.load(Ordering::SeqCst) != 2 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(renewed.load(Ordering::SeqCst), 2);

        fx.worker.send(frame("c1-s2", 0)).unwrap();
        let chunk = next_line(&fx.lines, |v| is(v, "chunk", "c1-s2"));
        assert_eq!(chunk["seq"], 0);

        let t = thread::spawn(move || conv.hang_up(Duration::from_secs(3)));
        next_line(&fx.lines, |v| is(v, "hang_up", "c1-s2"));
        fx.worker.send(WorkerMsg::End { id: "c1-s2".into(), frames: None, samples: None }).unwrap();
        assert!(t.join().unwrap().is_ok());
        assert_eq!(ended.load(Ordering::SeqCst), 0, "on_end must never fire for a renewal or a hang up");
    }

    #[test]
    fn hang_up_between_segments_sends_nothing_and_returns() {
        let fx = fixture();
        fx.worker.send(frame("c1", 0)).unwrap();
        let (entered_tx, entered_rx) = mpsc::channel::<()>();
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let gate_rx = Mutex::new(gate_rx);
        let renewed = Arc::new(AtomicU32::new(0));
        let r2 = renewed.clone();
        let renewal = Renewal {
            max_seconds: Arc::new(move || {
                let _ = entered_tx.send(());
                let _ = gate_rx.lock().unwrap().recv();
                42.0
            }),
            on_renewed: Arc::new(move |seg| r2.store(seg, Ordering::SeqCst)),
        };
        let on_end: Arc<dyn Fn(ConversationEnd) + Send + Sync> = Arc::new(|_| {});
        let settings = Settings { lag_limit_secs: 10.0, ..Default::default() };
        let conv = Conversation::start_with(
            fx.sender.clone(),
            fx.events,
            &settings,
            base_options(),
            Some(renewal),
            false,
            Arc::new(|_| {}),
            on_end,
            Arc::new(|_| {}),
            "c1".into(),
            &test_io(),
        )
        .expect("start");
        next_line(&fx.lines, |v| is(v, "chunk", "c1"));

        fx.worker.send(segment_limit("c1")).unwrap();
        entered_rx.recv_timeout(Duration::from_secs(5)).expect("renewal started");
        let t0 = Instant::now();
        let t = thread::spawn(move || conv.hang_up(Duration::from_secs(3)));
        thread::sleep(Duration::from_millis(100));
        gate_tx.send(()).unwrap();
        assert!(t.join().unwrap().is_ok());
        assert!(t0.elapsed() < Duration::from_secs(2), "hang up between segments must not wait for the worker");
        assert_eq!(renewed.load(Ordering::SeqCst), 0);
        thread::sleep(Duration::from_millis(50));
        let written: Vec<Value> = fx.lines.try_iter().collect();
        assert!(
            written.iter().all(|v| v["type"] == "chunk"),
            "no hang_up, cancel or dialogue line may follow the limit: {written:?}"
        );
    }
}
