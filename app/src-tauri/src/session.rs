//! Core: state machine, boot/start/stop/mute, backend fallback (Task 1.8).

pub mod battle;
pub mod battle_buffer;
pub mod budget;
pub mod conversation;
pub mod latency;
pub mod mic_pump;
pub mod state;
pub mod timeline;
pub mod viz;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::backend::plan::{choose_plan, device_note, run_attempts, Attempt, DeviceNote, Overrides};
use crate::backend::{detect, to_backend_options, Detection};
use crate::cards::Persona;
use crate::error::{ErrorCode, HaruError};
use crate::messages::{MsgKey, UiText};
use crate::ipc::{AppState, AttemptInfo, BackendOptions, BootInfo, SaveSettingsResult, StateEvent};
use crate::model_fetch::{ensure_model, is_present, FetchConfig};
use crate::paths::{model_path, ModelSpec};
use crate::settings::{self, Settings};
use crate::worker::process::{WorkerConfig, WorkerEvent, WorkerProcess, WorkerSender, READY_TIMEOUT};
use crate::worker::protocol::WorkerMsg;
use battle::{battle_start, Battle, MAX_FRAMES};
use budget::Probe;
use conversation::{Conversation, ConversationEnd, ConversationIo, Renewal, VizSink};
use state::{transition, Action};

pub type StateSink = Arc<dyn Fn(StateEvent) + Send + Sync>;
pub type DownloadSink = crate::model_fetch::DownloadSink;

const KEEP_LOGS: usize = 5;
fn note_text(n: DeviceNote, a: &Attempt) -> UiText {
    match n {
        DeviceNote::Cpu => UiText::new(MsgKey::CpuSlow),
        DeviceNote::CpuNoCapableGpu => UiText::new(MsgKey::NoCapableGpu),
        DeviceNote::SmallGpu => UiText::new(MsgKey::SmallGpu).with_param("device", a.description.clone()),
    }
}

pub struct CoreConfig {
    pub settings_path: PathBuf,
    pub log_dir: PathBuf,
    pub backends_dir: Option<PathBuf>,
    pub models_dir: PathBuf,
    pub model: ModelSpec,
    pub hf_base: String,
}

struct Inner {
    state: AppState,
    settings: Settings,
    detection: Option<Arc<Detection>>,
    plan: Vec<Attempt>,
    /// Backend/device overrides of the last boot (needed to classify the chosen device).
    overrides: Overrides,
    attempt_idx: usize,
    /// Where to ask for free device memory (the loaded worker's executable and device).
    probe: Option<Probe>,
    /// Last measured conversation length; fallback when a later measurement fails.
    ready_budget: Option<f64>,
    worker: Option<WorkerProcess>,
    worker_gen: u64,
    conversation: Option<Conversation>,
    battle: Option<Battle>,
    muted: bool,
    reload_pending: bool,
    boot_info: Option<BootInfo>,
    loading: Option<(String, String)>,
    last: StateEvent,
    booting: bool,
    starting: bool,
    early_end: Option<(String, ConversationEnd)>,
    /// Set by `shutdown`: nothing may be stored (and left running) afterwards.
    shutting_down: bool,
}

pub struct Core {
    cfg: CoreConfig,
    on_state: StateSink,
    inner: Mutex<Inner>,
    conv_tx: Mutex<Option<Sender<WorkerMsg>>>,
    next_conv: AtomicU64,
    log_counter: AtomicU64,
    test_io: Mutex<ConversationIo>,
}

fn join_err(e: tokio::task::JoinError) -> HaruError {
    HaruError::new(ErrorCode::Internal, MsgKey::Internal).with_detail(e.to_string())
}

fn busy() -> HaruError {
    HaruError::new(ErrorCode::Busy, MsgKey::Busy)
}

impl Core {
    pub fn new(cfg: CoreConfig, on_state: StateSink) -> Arc<Core> {
        let settings = settings::load(&cfg.settings_path);
        let last = StateEvent { state: AppState::Booting, backend: None, device: None, error: None, warning: None, notice: None };
        Arc::new(Core {
            cfg,
            on_state,
            inner: Mutex::new(Inner {
                state: AppState::Booting,
                settings,
                detection: None,
                plan: Vec::new(),
                overrides: Overrides::default(),
                attempt_idx: 0,
                probe: None,
                ready_budget: None,
                worker: None,
                worker_gen: 0,
                conversation: None,
                battle: None,
                muted: false,
                reload_pending: false,
                boot_info: None,
                loading: None,
                last,
                booting: false,
                starting: false,
                early_end: None,
                shutting_down: false,
            }),
            conv_tx: Mutex::new(None),
            next_conv: AtomicU64::new(1),
            log_counter: AtomicU64::new(0),
            test_io: Mutex::new(ConversationIo::default()),
        })
    }

    /// Replaces the audio devices of later conversations with a sample feed / virtual output
    /// (used by `headless_talk`).
    pub fn set_test_io(&self, io: ConversationIo) {
        *self.test_io.lock().unwrap_or_else(|e| e.into_inner()) = io;
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn event(g: &Inner, warning: Option<UiText>, error: Option<HaruError>) -> StateEvent {
        let (backend, device) = match g.state {
            AppState::Ready | AppState::Talking => match &g.boot_info {
                Some(b) => (Some(b.backend.clone()), Some(b.device_name.clone())),
                None => (None, None),
            },
            AppState::Loading => match &g.loading {
                Some((b, d)) => (Some(b.clone()), Some(d.clone())),
                None => (None, None),
            },
            _ => (None, None),
        };
        StateEvent { state: g.state, backend, device, error, warning, notice: None }
    }

    /// Applies `a`, then emits the new state.
    fn set(&self, g: &mut Inner, a: Action, warning: Option<UiText>, error: Option<HaruError>) -> Result<(), HaruError> {
        g.state = transition(g.state, &a)?;
        self.emit(g, warning, error);
        Ok(())
    }

    /// Emits the current state again (e.g. with an error from a finished conversation).
    fn emit(&self, g: &mut Inner, warning: Option<UiText>, error: Option<HaruError>) {
        let ev = Self::event(g, warning, error);
        g.last = ev.clone();
        (self.on_state)(ev);
    }

    pub fn status(&self) -> StateEvent {
        self.lock().last.clone()
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    pub fn save_settings(&self, s: Settings) -> Result<SaveSettingsResult, HaruError> {
        s.validate()?;
        let (reload, old_worker) = {
            let mut g = self.lock();
            let reload = g.settings.needs_reload(&s);
            if reload && (g.state == AppState::Talking || g.starting) {
                return Err(busy());
            }
            settings::save(&self.cfg.settings_path, &s)?;
            g.settings = s.clone();
            let mut old = None;
            if reload {
                g.reload_pending = true;
                if matches!(g.state, AppState::Ready | AppState::Error) {
                    g.worker_gen += 1;
                    old = g.worker.take();
                    g.boot_info = None;
                    let _ = self.set(&mut g, Action::Boot, None, None);
                }
            }
            (reload, old)
        };
        drop(old_worker);
        Ok(SaveSettingsResult { settings: s, reload_required: reload })
    }

    /// Replaces the in-memory settings WITHOUT validating or persisting them. Only for experiments
    /// (`headless_talk --temperature` beyond the app's range); the app itself always goes through
    /// `save_settings`.
    pub fn set_settings_unchecked(&self, s: Settings) {
        self.lock().settings = s;
    }

    pub fn backend_options(&self) -> Result<BackendOptions, HaruError> {
        let cached = self.lock().detection.clone();
        let det = match cached {
            Some(d) => d,
            None => {
                let d = Arc::new(detect(self.cfg.backends_dir.as_deref())?);
                self.lock().detection = Some(d.clone());
                d
            }
        };
        Ok(to_backend_options(&det))
    }

    fn fetch_config(&self) -> FetchConfig {
        FetchConfig {
            url: self.cfg.model.url(&self.cfg.hf_base),
            dest: model_path(&self.cfg.models_dir, &self.cfg.model),
            size: self.cfg.model.size,
            sha256: self.cfg.model.sha256.to_string(),
        }
    }

    // ---------------------------------------------------------------- boot

    pub async fn boot(self: &Arc<Self>, on_download: DownloadSink) -> Result<BootInfo, HaruError> {
        let old_worker = {
            let mut g = self.lock();
            if g.booting {
                return Err(busy());
            }
            if g.state == AppState::Ready && !g.reload_pending {
                if let Some(b) = &g.boot_info {
                    return Ok(b.clone());
                }
            }
            self.set(&mut g, Action::Boot, None, None)?;
            g.booting = true;
            g.reload_pending = false;
            g.boot_info = None;
            g.worker_gen += 1;
            g.worker.take()
        };
        if let Some(w) = old_worker {
            let _ = tokio::task::spawn_blocking(move || drop(w)).await;
        }
        let result = self.boot_inner(on_download).await;
        let mut g = self.lock();
        g.booting = false;
        if let Err(e) = &result {
            if g.state != AppState::Error {
                let _ = self.set(&mut g, Action::Fail, None, Some(e.clone()));
            }
        }
        result
    }

    async fn boot_inner(self: &Arc<Self>, on_download: DownloadSink) -> Result<BootInfo, HaruError> {
        let settings = self.settings();
        let dir = self.cfg.backends_dir.clone();
        let det = Arc::new(tokio::task::spawn_blocking(move || detect(dir.as_deref())).await.map_err(join_err)??);
        let ov = Overrides::resolve(&|k| std::env::var(k).ok(), &settings);
        let plan = choose_plan(det.cuda_ok, &det.variants, &det.devices, &ov);
        if plan.is_empty() {
            return Err(HaruError::new(ErrorCode::NoBackend, MsgKey::NoBackend)
                .with_detail("empty backend plan"));
        }
        {
            let mut g = self.lock();
            g.detection = Some(det);
            g.plan = plan;
            g.overrides = ov;
            g.attempt_idx = 0;
        }
        let fc = self.fetch_config();
        if !is_present(&fc) {
            {
                let mut g = self.lock();
                self.set(&mut g, Action::Downloading, None, None)?;
            }
            ensure_model(&fc, on_download).await?;
        }
        {
            let mut g = self.lock();
            Self::show_loading(&mut g, 0);
            self.set(&mut g, Action::Loading, None, None)?;
        }
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.load_from(0, None)).await.map_err(join_err)?
    }

    fn show_loading(g: &mut Inner, idx: usize) {
        g.loading = g.plan.get(idx).map(|a| (a.variant.as_str().to_string(), a.description.clone()));
    }

    fn log_path(&self) -> PathBuf {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let n = self.log_counter.fetch_add(1, Ordering::Relaxed);
        self.cfg.log_dir.join(format!("worker-{secs}-{n:03}.log"))
    }

    fn prune_logs(&self) {
        let Ok(rd) = std::fs::read_dir(&self.cfg.log_dir) else { return };
        let mut logs: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map_or(false, |n| n.starts_with("worker-") && n.ends_with(".log"))
            })
            .collect();
        logs.sort();
        let excess = logs.len().saturating_sub(KEEP_LOGS);
        for p in logs.into_iter().take(excess) {
            let _ = std::fs::remove_file(p);
        }
    }

    /// Loads the first working attempt of `plan[from..]` (blocking). The caller already set `Loading`.
    /// `switched` = (previous backend, reason) adds the "backend switched" warning.
    fn load_from(self: &Arc<Self>, from: usize, switched: Option<(String, MsgKey)>) -> Result<BootInfo, HaruError> {
        let (plan, voice) = {
            let g = self.lock();
            (g.plan.get(from..).unwrap_or(&[]).to_vec(), g.settings.voice_path.clone().map(PathBuf::from))
        };
        let model = self.fetch_config().dest;
        let loaded = run_attempts(&plan, |a| {
            {
                let mut g = self.lock();
                let shown = Some((a.variant.as_str().to_string(), a.description.clone()));
                let changed = g.loading != shown;
                g.loading = shown;
                if changed && g.state == AppState::Loading {
                    self.emit(&mut g, None, None);
                }
            }
            let cfg = WorkerConfig {
                exe: a.exe.clone(),
                model: model.clone(),
                device: a.device.clone(),
                voice: voice.clone(),
                log_path: self.log_path(),
            };
            let mut w = WorkerProcess::spawn(&cfg)?;
            self.prune_logs();
            let ready = w.wait_ready(READY_TIMEOUT)?;
            Ok((w, ready))
        });
        let (attempt, (mut worker, ready)) = match loaded {
            Ok(v) => v,
            Err(e) => {
                let mut g = self.lock();
                let _ = self.set(&mut g, Action::Fail, None, Some(e.clone()));
                return Err(e);
            }
        };
        let events = worker.take_events();
        let probe = Probe {
            exe: attempt.exe.clone(),
            device: ready.model.device.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| attempt.device.clone()),
        };
        let b = budget::measure(Some(&probe), None);
        eprintln!(
            "haru: worker ready on {} ({}), session budget {}",
            probe.device,
            ready.model.weight_type.as_deref().unwrap_or("?"),
            b.describe()
        );
        let mut g = self.lock();
        if g.shutting_down {
            drop(g);
            worker.shutdown(Duration::from_secs(2));
            return Err(busy());
        }
        let idx = g.plan.iter().position(|p| *p == attempt).unwrap_or(from);
        g.worker_gen += 1;
        let gen = g.worker_gen;
        g.worker = Some(worker);
        g.probe = Some(probe);
        g.ready_budget = b.measured();
        g.attempt_idx = idx;
        let note = g.detection.as_ref().and_then(|d| device_note(&attempt, &d.devices, &g.overrides));
        let warning = match (&switched, note) {
            (Some((from_backend, why)), _) => Some(
                UiText::new(*why)
                    .with_param("from", from_backend.clone())
                    .with_param("to", attempt.variant.as_str()),
            ),
            (None, Some(n)) => Some(note_text(n, &attempt)),
            _ => None,
        };
        let info = BootInfo {
            backend: attempt.variant.as_str().to_string(),
            device: attempt.device.clone(),
            device_name: attempt.description.clone(),
            model_path: model.to_string_lossy().into_owned(),
            plan: g
                .plan
                .iter()
                .map(|a| AttemptInfo {
                    backend: a.variant.as_str().to_string(),
                    device: a.device.clone(),
                    description: a.description.clone(),
                })
                .collect(),
            warning: warning.clone(),
            battle: ready.supports_battle(),
        };
        g.boot_info = Some(info.clone());
        self.set(&mut g, Action::Loaded, warning, None)?;
        drop(g);
        self.spawn_watcher(gen, events);
        Ok(info)
    }

    // ------------------------------------------------------------- watcher

    fn spawn_watcher(self: &Arc<Self>, gen: u64, rx: mpsc::Receiver<WorkerEvent>) {
        let this = self.clone();
        std::thread::Builder::new()
            .name("haru-worker-watch".into())
            .spawn(move || {
                for ev in rx {
                    match ev {
                        WorkerEvent::Message(m) => {
                            let tx = this.conv_tx.lock().unwrap_or_else(|e| e.into_inner());
                            if let Some(tx) = tx.as_ref() {
                                let _ = tx.send(m);
                            }
                        }
                        WorkerEvent::Malformed { .. } | WorkerEvent::Exited { .. } => {
                            this.worker_lost(gen);
                            return;
                        }
                    }
                }
            })
            .expect("spawn worker watcher");
    }

    fn worker_lost(self: &Arc<Self>, gen: u64) {
        let (conv, battle, worker, tail, idx, attempts_left, from_backend) = {
            let mut g = self.lock();
            if g.worker_gen != gen || g.starting {
                // Replaced on purpose, or `start` is in flight and handles the failure itself.
                return;
            }
            g.worker_gen += 1;
            let worker = g.worker.take();
            let conv = g.conversation.take();
            let battle = g.battle.take();
            let idx = g.attempt_idx;
            let attempts_left = idx + 1 < g.plan.len();
            let from_backend = g.plan.get(idx).map(|a| a.variant.as_str().to_string()).unwrap_or_default();
            let tail = worker.as_ref().map(|w| w.stderr_tail(4096)).unwrap_or_default();
            g.boot_info = None;
            if attempts_left {
                Self::show_loading(&mut g, idx + 1);
                let _ = self.set(&mut g, Action::WorkerLost { attempts_left: true }, None, None);
            } else {
                let e = HaruError::new(ErrorCode::Worker, MsgKey::WorkerExited)
                    .with_detail(tail.clone());
                let _ = self.set(&mut g, Action::WorkerLost { attempts_left: false }, None, Some(e));
            }
            (conv, battle, worker, tail, idx, attempts_left, from_backend)
        };
        let _ = tail;
        *self.conv_tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
        drop(conv);
        drop(battle);
        drop(worker);
        if attempts_left {
            let _ = self.load_from(idx + 1, Some((from_backend, MsgKey::BackendCrashedSwitched)));
        }
    }

    // --------------------------------------------------------- conversation

    pub fn start(self: &Arc<Self>, persona: Persona, on_viz: VizSink) -> Result<(), HaruError> {
        {
            let mut g = self.lock();
            transition(g.state, &Action::Start)?;
            if g.starting || g.worker.is_none() {
                return Err(busy());
            }
            g.starting = true;
            g.early_end = None;
        }
        let r = self.start_inner(persona, on_viz);
        let mut g = self.lock();
        g.starting = false;
        if r.is_err() {
            *self.conv_tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        r
    }

    fn start_inner(self: &Arc<Self>, persona: Persona, on_viz: VizSink) -> Result<(), HaruError> {
        loop {
            let (sender, settings, muted, idx, plan_len, probe, fallback) = {
                let g = self.lock();
                let Some(w) = g.worker.as_ref() else { return Err(busy()) };
                (w.sender(), g.settings.clone(), g.muted, g.attempt_idx, g.plan.len(), g.probe.clone(), g.ready_budget)
            };
            let (tx, rx) = mpsc::channel();
            *self.conv_tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
            let id = format!("c{}", self.next_conv.fetch_add(1, Ordering::Relaxed));
            let this = self.clone();
            let end_id = id.clone();
            let on_end: Arc<dyn Fn(ConversationEnd) + Send + Sync> =
                Arc::new(move |e| this.conversation_ended(end_id.clone(), e));
            let io = self.test_io.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let mut options = settings.dialogue_options(persona);
            let first = budget::measure(probe.as_ref(), fallback);
            options.max_seconds = Some(first.seconds);
            eprintln!("haru: conversation {id}: {}", first.describe());
            let weak = Arc::downgrade(self);
            let key = id.clone();
            let next_probe = probe.clone();
            let (ww, wk) = (Arc::downgrade(self), id.clone());
            let on_warning: Arc<dyn Fn(UiText) + Send + Sync> = Arc::new(move |t| {
                if let Some(c) = ww.upgrade() {
                    c.conversation_warning(&wk, t)
                }
            });
            let renewal = Renewal {
                max_seconds: Arc::new(move || {
                    let b = budget::measure(next_probe.as_ref(), fallback);
                    eprintln!("haru: next segment: {}", b.describe());
                    b.seconds
                }),
                on_renewed: Arc::new(move |seg| {
                    if let Some(c) = weak.upgrade() {
                        c.segment_renewed(&key, seg)
                    }
                }),
            };
            match Conversation::start_with(sender, rx, &settings, options, Some(renewal), muted, on_viz.clone(), on_end, on_warning, id.clone(), &io, persona.voice_style()) {
                Ok(conv) => {
                    let early = {
                        let mut g = self.lock();
                        if g.shutting_down {
                            drop(g);
                            *self.conv_tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
                            let _ = conv.abort();
                            return Err(busy());
                        }
                        let warning = conv.startup_warning();
                        g.conversation = Some(conv);
                        self.set(&mut g, Action::Started, warning, None)?;
                        g.early_end.take()
                    };
                    if let Some((eid, end)) = early {
                        if eid == id {
                            self.conversation_ended(eid, end);
                        }
                    }
                    return Ok(());
                }
                Err(e) if e.backend_failure => {
                    *self.conv_tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    let (old, from_backend) = {
                        let mut g = self.lock();
                        let from_backend = g.plan.get(idx).map(|a| a.variant.as_str().to_string()).unwrap_or_default();
                        g.worker_gen += 1;
                        g.boot_info = None;
                        let old = g.worker.take();
                        if idx + 1 < plan_len {
                            Self::show_loading(&mut g, idx + 1);
                            self.set(&mut g, Action::Loading, None, None)?;
                        } else {
                            let _ = self.set(&mut g, Action::Fail, None, Some(e.clone()));
                        }
                        (old, from_backend)
                    };
                    drop(old);
                    if idx + 1 >= plan_len {
                        return Err(e);
                    }
                    self.load_from(idx + 1, Some((from_backend, MsgKey::BackendFailedSwitched)))?;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// The running conversation continued in a fresh segment: tell the UI, no state change.
    fn segment_renewed(&self, id: &str, _seg: u32) {
        let mut g = self.lock();
        if g.state != AppState::Talking || g.conversation.as_ref().map(|c| c.id()) != Some(id) {
            return;
        }
        let mut ev = Self::event(&g, None, None);
        ev.notice = Some(UiText::new(MsgKey::SegmentRenewed));
        g.last = ev.clone();
        (self.on_state)(ev);
    }

    /// A running conversation reports a non-fatal problem (e.g. echo cancellation dropped out): warn the UI, no state change.
    fn conversation_warning(&self, id: &str, text: UiText) {
        let mut g = self.lock();
        if g.state != AppState::Talking || g.conversation.as_ref().map(|c| c.id()) != Some(id) {
            return;
        }
        let ev = Self::event(&g, Some(text), None);
        g.last = ev.clone();
        (self.on_state)(ev);
    }

    /// Called from the conversation's own threads: the conversation ended by itself.
    fn conversation_ended(self: &Arc<Self>, id: String, end: ConversationEnd) {
        let conv = {
            let mut g = self.lock();
            match g.conversation.as_ref() {
                Some(c) if c.id() == id => g.conversation.take(),
                _ => {
                    if g.starting {
                        g.early_end = Some((id, end));
                    }
                    return;
                }
            }
        };
        let Some(conv) = conv else { return };
        let this = self.clone();
        // Teardown joins the conversation's threads, one of which is calling us.
        std::thread::spawn(move || {
            let stuck = conv.abort().is_err();
            *this.conv_tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
            let error = match end {
                ConversationEnd::HungUp => None,
                ConversationEnd::Failed(e) => Some(e),
            };
            {
                let mut g = this.lock();
                if this.set(&mut g, Action::Stopped, None, error).is_err() {
                    return; // the worker watcher already moved on
                }
            }
            if stuck {
                this.restart_current();
            }
        });
    }

    // ---------------------------------------------------------------- battle

    /// Starts a battle of the angel and the daemon on `topic`: `talking` while it runs, `ready` after.
    pub fn start_battle(self: &Arc<Self>, topic: String, on_viz: VizSink) -> Result<(), HaruError> {
        let topic = {
            let mut g = self.lock();
            transition(g.state, &Action::Start)?;
            if g.starting || g.worker.is_none() {
                return Err(busy());
            }
            let topic = battle_start(&topic, g.boot_info.as_ref().is_some_and(|b| b.battle))?;
            g.starting = true;
            g.early_end = None;
            topic
        };
        let r = self.start_battle_inner(&topic, on_viz);
        let mut g = self.lock();
        g.starting = false;
        if r.is_err() {
            *self.conv_tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        r
    }

    fn start_battle_inner(self: &Arc<Self>, topic: &str, on_viz: VizSink) -> Result<(), HaruError> {
        let (sender, settings) = {
            let g = self.lock();
            let Some(w) = g.worker.as_ref() else { return Err(busy()) };
            (w.sender(), g.settings.clone())
        };
        let (tx, rx) = mpsc::channel();
        *self.conv_tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        self.battle_with(sender, rx, settings, topic, on_viz)
    }

    /// Runs a battle on `sender` and `rx` and stores it in the core.
    fn battle_with(self: &Arc<Self>, sender: WorkerSender, rx: mpsc::Receiver<WorkerMsg>, settings: Settings, topic: &str, on_viz: VizSink) -> Result<(), HaruError> {
        let id = format!("b{}", self.next_conv.fetch_add(1, Ordering::Relaxed));
        let this = self.clone();
        let end_id = id.clone();
        let on_end: Arc<dyn Fn(ConversationEnd) + Send + Sync> = Arc::new(move |e| this.battle_ended(end_id.clone(), e));
        let io = self.test_io.lock().unwrap_or_else(|e| e.into_inner()).clone();
        eprintln!("haru: battle {id}: {MAX_FRAMES} frames per speaker");
        match Battle::start_with(sender, rx, &settings, settings.battle_options(topic), MAX_FRAMES, on_viz, on_end, id.clone(), &io) {
            Ok(battle) => {
                let early = {
                    let mut g = self.lock();
                    if g.shutting_down {
                        drop(g);
                        *self.conv_tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
                        let _ = battle.abort();
                        return Err(busy());
                    }
                    g.battle = Some(battle);
                    self.set(&mut g, Action::Started, None, None)?;
                    g.early_end.take()
                };
                if let Some((eid, end)) = early {
                    if eid == id {
                        self.battle_ended(eid, end);
                    }
                }
                Ok(())
            }
            Err(e) => {
                *self.conv_tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
                // A battle never moves to another backend; a worker that did not answer is replaced by a fresh one.
                if e.backend_failure {
                    self.restart_current();
                }
                Err(e)
            }
        }
    }

    /// Called from the battle's own threads: it played out after the worker's end, or failed.
    fn battle_ended(self: &Arc<Self>, id: String, end: ConversationEnd) {
        let battle = {
            let mut g = self.lock();
            match g.battle.as_ref() {
                Some(b) if b.id() == id => g.battle.take(),
                _ => {
                    if g.starting {
                        g.early_end = Some((id, end));
                    }
                    return;
                }
            }
        };
        let Some(battle) = battle else { return };
        let this = self.clone();
        // Teardown joins the battle's threads, one of which is calling us.
        std::thread::spawn(move || {
            let stuck = battle.abort().is_err();
            *this.conv_tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
            let error = match end {
                ConversationEnd::HungUp => None,
                ConversationEnd::Failed(e) => Some(e),
            };
            {
                let mut g = this.lock();
                if this.set(&mut g, Action::Stopped, None, error).is_err() {
                    return;
                }
            }
            if stuck {
                this.restart_current();
            }
        });
    }

    pub fn stop(self: &Arc<Self>) -> Result<(), HaruError> {
        let (conv, battle) = {
            let mut g = self.lock();
            if g.state != AppState::Talking {
                return Ok(());
            }
            (g.conversation.take(), g.battle.take())
        };
        let r = match (conv, battle) {
            (Some(conv), _) => conv.hang_up(Duration::from_secs(3)),
            (None, Some(battle)) => battle.hang_up(Duration::from_secs(3)),
            (None, None) => return Ok(()),
        };
        *self.conv_tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
        {
            let mut g = self.lock();
            let _ = self.set(&mut g, Action::Stopped, None, None);
        }
        if r.is_err() {
            self.restart_current();
        }
        Ok(())
    }

    /// The worker did not answer a hang_up/cancel: replace it with a fresh one of the same attempt.
    fn restart_current(self: &Arc<Self>) {
        let (old, idx, from_backend) = {
            let mut g = self.lock();
            let idx = g.attempt_idx;
            let from_backend = g.plan.get(idx).map(|a| a.variant.as_str().to_string()).unwrap_or_default();
            g.worker_gen += 1;
            g.boot_info = None;
            let old = g.worker.take();
            Self::show_loading(&mut g, idx);
            if self.set(&mut g, Action::Loading, None, None).is_err() {
                return;
            }
            (old, idx, from_backend)
        };
        drop(old);
        let _ = from_backend;
        let _ = self.load_from(idx, None);
    }

    pub fn set_muted(&self, muted: bool) {
        let mut g = self.lock();
        g.muted = muted;
        if let Some(c) = g.conversation.as_ref() {
            c.set_muted(muted);
        }
    }

    /// Plays the short sound cue in the running conversation; a no-op while none runs.
    pub fn play_cue(&self) {
        if let Some(c) = self.lock().conversation.as_ref() {
            c.play_cue();
        }
    }

    pub fn shutdown(&self) {
        let (conv, battle, worker) = {
            let mut g = self.lock();
            g.shutting_down = true;
            g.worker_gen += 1;
            (g.conversation.take(), g.battle.take(), g.worker.take())
        };
        if let Some(c) = conv {
            let _ = c.hang_up(Duration::from_secs(1));
        }
        if let Some(b) = battle {
            let _ = b.hang_up(Duration::from_secs(1));
        }
        if let Some(mut w) = worker {
            w.shutdown(Duration::from_secs(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::HARU_MODEL;

    #[test]
    fn play_cue_without_conversation_is_a_noop() {
        let d = tempfile::tempdir().unwrap();
        let core = Core::new(
            CoreConfig {
                settings_path: d.path().join("settings.json"),
                log_dir: d.path().join("logs"),
                backends_dir: None,
                models_dir: d.path().join("models"),
                model: HARU_MODEL,
                hf_base: "http://localhost".into(),
            },
            Arc::new(|_| {}),
        );
        core.play_cue();
    }

    #[test]
    fn start_battle_needs_a_ready_core() {
        let d = tempfile::tempdir().unwrap();
        let core = Core::new(
            CoreConfig {
                settings_path: d.path().join("settings.json"),
                log_dir: d.path().join("logs"),
                backends_dir: None,
                models_dir: d.path().join("models"),
                model: HARU_MODEL,
                hf_base: "http://localhost".into(),
            },
            Arc::new(|_| {}),
        );
        let e = core.start_battle("Ist eine Notlüge erlaubt?".into(), Arc::new(|_| {})).unwrap_err();
        assert_eq!(e.code, ErrorCode::Busy);
    }
    // ------------------------------------------------ battle lifecycle through the core

    use crate::session::battle::test_support::{fixture, next_line, queue, sine, wait_until};

    /// A core with no worker process, `Ready`, whose state events are collected; a fake-worker battle is started in it.
    fn core_with_battle(end: bool, frames: usize) -> (Arc<Core>, Arc<Mutex<Vec<StateEvent>>>, mpsc::Receiver<serde_json::Value>, Sender<WorkerMsg>, tempfile::TempDir) {
        let d = tempfile::tempdir().unwrap();
        let events: Arc<Mutex<Vec<StateEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let e2 = events.clone();
        let core = Core::new(
            CoreConfig {
                settings_path: d.path().join("settings.json"),
                log_dir: d.path().join("logs"),
                backends_dir: None,
                models_dir: d.path().join("models"),
                model: HARU_MODEL,
                hf_base: "http://localhost".into(),
            },
            Arc::new(move |e| e2.lock().unwrap().push(e)),
        );
        core.set_test_io(ConversationIo { no_playback: true, ..Default::default() });
        let fx = fixture();
        queue(&fx, &sine(220.0, frames), &sine(140.0, frames), frames, end);
        {
            let mut g = core.lock();
            g.state = AppState::Ready;
            g.starting = true;
        }
        let crate::session::battle::test_support::Fixture { lines, worker, sender, events: rx } = fx;
        core.battle_with(sender, rx, Settings::default(), "Ist eine Notlüge erlaubt?", Arc::new(|_| {})).expect("battle starts");
        core.lock().starting = false;
        (core, events, lines, worker, d)
    }

    fn states(events: &Arc<Mutex<Vec<StateEvent>>>) -> Vec<AppState> {
        events.lock().unwrap().iter().map(|e| e.state).collect()
    }

    #[test]
    fn stop_during_a_battle_returns_to_ready_once_and_ends_nothing() {
        let (core, events, lines, worker, _dir) = core_with_battle(false, 1);
        assert_eq!(states(&events), vec![AppState::Talking]);
        let c = core.clone();
        let t = std::thread::spawn(move || c.stop());
        next_line(&lines, |v| v["type"] == "hang_up");
        worker.send(WorkerMsg::End { id: "b1".into(), frames: Some(1), samples: None }).unwrap();
        assert!(t.join().unwrap().is_ok());
        assert!(core.stop().is_ok(), "a second stop is a no-op");
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(states(&events), vec![AppState::Talking, AppState::Ready]);
        assert!(events.lock().unwrap().iter().all(|e| e.error.is_none()), "a stop reports no end of its own");
        let g = core.lock();
        assert!(g.battle.is_none() && g.state == AppState::Ready);
    }

    #[test]
    fn a_battle_that_ends_by_itself_returns_to_ready_once() {
        let (core, events, _lines, _worker, _dir) = core_with_battle(true, 2);
        wait_until("the battle plays out and the core is ready", || states(&events).contains(&AppState::Ready));
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(states(&events), vec![AppState::Talking, AppState::Ready]);
        assert!(events.lock().unwrap().iter().all(|e| e.error.is_none()));
        assert!(core.lock().battle.is_none());
    }
}
