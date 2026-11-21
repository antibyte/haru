# Haru – Flüssiges Licht Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Tauri v2 desktop app for live full-duplex conversation with Haru-4B (Q6_K) through `speech worker` (speech.cpp v0.11.1). Haru's spoken words appear as glowing ink in a WebGL2 fluid simulation ("Flüssiges Licht"). The app targets Windows first and stays portable to Linux and macOS.

**Architecture:** The Rust core (`app/src-tauri`) runs `speech worker` from the chosen backend variant (`backends/<variant>/speech[.exe]`) as a child process and talks to it in JSON Lines. It also owns model download, backend selection with fallback, audio (cpal), latency policy and settings. The UI (`app/src`, TypeScript + WebGL2 + Vite) sends Tauri commands. The core sends state events (`haru:state`), a download `Channel`, and a viz `Channel` at about 60 Hz. The UI is built against a mock client, so the Rust track and the UI track run in parallel against one IPC contract (section "IPC Contract").

**Tech Stack:**
- Rust 1.96: tauri 2.5.1, tauri-build 2.2.0, serde 1.0.219, serde_json 1.0.140, thiserror 2.0.12, base64 0.22.1, sha2 0.10.8, reqwest 0.12.15 (rustls-tls + stream), futures-util 0.3.31, tokio 1.44.2, cpal 0.15.3, rubato 0.16, rustfft 6.2.0, libloading 0.8, rtrb 0.3, fs2 0.4.3.
- Rust dev dependencies: tiny_http 0.12.0, tempfile 3, hound 3.5.1.
- Web: Node 24, @tauri-apps/api 2.5.0, @tauri-apps/cli 2.5.0, vite 6.3.5, typescript 5.8.3, vitest 3.1.1, @fontsource-variable/inter 5.1.0.

**Source of truth:** `docs/superpowers/specs/2026-11-20-haru-fluid-light-design.md`. Where this plan departs from the spec, it is because the upstream source says otherwise. Each such case is marked **(source over spec)** and cited.

---

## Verified speech.cpp v0.11.1 facts

All paths are relative to `extern/speech.cpp/`.

| # | Fact | Source |
|---|------|--------|
| F1 | Command line: `speech worker MODEL [--add-voice NAME=FILE]... [--device NAME] [--threads N] [--no-warmup] [request options]` | `docs/worker.md:7`, `docs/cli.md:542`, `tools/worker/worker.cpp:334-339`, flag names in `tools/common/command-line.cpp:227-241` |
| F2 | MODEL is a file path when it ends in `.gguf` (case-insensitive). Otherwise it is a catalog name that gets fetched. The app always passes the absolute `.gguf` path. | `tools/catalog/catalog.cpp:113-118`, `tools/catalog/fetch.cpp:210-216` |
| F3 | `--device`: `auto` (first GPU, else CPU), `gpu`, `cpu`, or a name from `speech devices` such as `Vulkan1`, `CUDA0`, `MTL0`, `CPU`. Matching is case-insensitive. A named device that is missing is an error, with no silent fallback. | `include/speech.h:175-178`, `include/speech.h:600-606` |
| F4 | Ready message: `{"type":"ready","protocol":4,"version":"0.11.1","defaults":{...},"model":{...}}`. Protocol is **4**. `model` has `name`, `task` (`"dialogue"`), `sample_rate`, `frame_samples`, `weight_type`, `device`, `threads`. | `tools/worker/worker.cpp:35,289-290`, `docs/worker.md:36`, `docs/c-api.md:450-495` |
| F5 | If start fails, the worker sends `{"type":"fatal","error":{"code","option","message"}}` and exits 1 (2 on a usage error). | `docs/worker.md:37,46`, `tools/worker/worker.cpp:313-320` |
| F6 | Client messages: `{"type":"dialogue","id","sample_rate",<options>}`, `{"type":"chunk","id","seq","pcm"}`, `{"type":"hang_up","id"}`, `{"type":"cancel","id"}`. `seq` starts at 0 and has no gaps. Chunks can have any length. `sample_rate` can be any rate (the worker resamples). Unknown members are refused. A `null` member counts as omitted. | `docs/worker.md:26-28,57-81`, `tools/worker/inbox.cpp:434-462` |
| F7 | Worker frame: `{"type":"chunk","id","seq","pcm","text"}`. Each frame has `frame_samples` samples (3200 at 24 kHz). `text` may be `""`, or `" "` when the model goes silent. **Frame 0 is sent right after `dialogue`, before any audio.** | `docs/worker.md:75-76,107,130-133`, `tools/worker/worker.cpp:181-187` |
| F8 | Terminal messages: `{"type":"end","id","frames","samples"}`, `{"type":"cancelled","id"}`, and `{"type":"error","id"?, "error":{"code","option","message"}}`. **The error members are nested** (source over spec §4.3, which shows them flat). An `error` without `id` is a reply to a line that belongs to no request. | `docs/worker.md:112,116-117,142-144`, `tools/worker/worker.cpp:48-55` |
| F9 | Error codes: `internal, io, out_of_memory, device, model_file, out_of_range, unsupported, invalid_argument, cancelled`. Reaching `max_seconds` gives `out_of_range` with option `max_seconds`. | `src/api/speech.cpp:169-170`, `docs/worker.md:132-133` |
| F10 | PCM is base64 of i16 little-endian mono. Worker output is `round(clamp(x,-1,1)*32767)`. Input is read as `x/32768`. | `docs/worker.md:82-84`, `tools/worker/worker.cpp:70-74,95-99` |
| F11 | When stdin closes, the worker ends the running conversation as `hang_up` would, then exits 0. | `docs/worker.md:99-101` |
| F12 | A dialogue `voice` is a **voice name** added with `--add-voice NAME=FILE` (a WAV is accepted for Haru). It is not a path. | `src/engines/haru-engine.cpp:121,137-140`, `docs/models/haru.md:94-112` |
| F13 | Haru option names: `voice, language (default "auto"), seed, max_seconds (3600), top_k (25), top_p (1), temperature (0.7), name, persona, gender, backchannel, other_name, relationship, situation, guidance_scale (1.5), eop_bias (0), speak_first (false)`. Card fields require a `language` other than `auto`. | `src/engines/haru-engine.cpp:22-25,103-121,178-197`, `src/api/speech.cpp:99-165`, `docs/models/haru.md:110-120` |
| F14 | `max_seconds` reserves its KV memory when the conversation starts (about 1.7 GB for 3600 s). | `docs/models/haru.md:66-67`, `src/engines/haru-engine.cpp:127-132` |
| F15 | `speech devices --json` prints `{"devices":[{"name":"Vulkan0","description":"…","kind":"gpu"\|"igpu"\|"cpu","memory_total":N,"memory_free":N}]}` | `tools/cli/model.cpp:337-361,460`, `tools/common/library.h:97-101`, `docs/cli.md:402-413` |
| F16 | Cache root: Windows `%LOCALAPPDATA%\speech.cpp` (error if unset). macOS `$HOME/Library/Caches/speech.cpp`. Linux `$XDG_CACHE_HOME/speech.cpp` if that path is absolute, otherwise `$HOME/.cache/speech.cpp` (error if HOME is unset). An empty variable counts as unset. Models go in `<root>/models`. `SPEECH_MODEL_DIR` replaces the whole models folder and must be absolute (otherwise error). | `tools/catalog/cache.cpp:39-47,92-117` |
| F17 | Layout: `<models>/<owner>--<name>/<sha256>/<file>`. The partial file is `<file>.part`. The CLI holds `<file>.lock` while it fetches. | `tools/catalog/cache.cpp:50-54,119-127,161-163` |
| F18 | Only a terminal triggers the update check, and the worker never runs it. `SPEECH_NO_UPDATE_CHECK=1` turns it off. | `docs/cli.md:97-100` |
| F19 | CMake target `speech-cli` builds `speech.exe`. The MSVC output is `build/<dir>/Release/speech.exe` (checked on disk: `build\speech-vulkan\Release\speech.exe`). macOS needs `-DGGML_METAL_EMBED_LIBRARY=ON`. | `CMakeLists.txt:392,445`, `.github/workflows/build.yml:32,119` |
| F20 | `speech talk MODEL -o reply.wav in.wav` prints RTFx to stderr. Running with a WAVE file skips warm-up. | `docs/cli.md:325-354` |

---

## Conventions

- Run everything from the repo root `C:\Users\Andi\Documents\repo\haru` in PowerShell 7 (where `&&` works).
- Rust crate: `app/src-tauri`, package `haru`, library `haru_lib`. Rust tests: `cargo test --manifest-path app/src-tauri/Cargo.toml --lib <filter>`.
- UI commands: `npm --prefix app run <script>`. UI tests: `npm --prefix app run test -- <file>`.
- Code and comments are English. All user-facing strings are German (the UI language): Rust `HaruError.message` and `app/src/ui/strings.ts`. `HaruError.detail` carries technical text such as stderr tails and IO errors.
- Session variables for manual steps:
  ```powershell
  $SPEECH = "build\speech-vulkan\Release\speech.exe"
  $MODEL  = "$env:LOCALAPPDATA\speech.cpp\models\sakasegawa--Haru-4B-GGUF\8db05e86acca046bc183d4e30fbda22ac775ba4767ae5a10bcd0d48f8d7db793\Haru-4B-Q6_K.gguf"
  ```
- Only the owner task edits `lib.rs`, `Cargo.toml` and `package.json`. Tasks T1.0 and T1.8 own them, so parallel tasks never touch shared files.

---

## File Structure

```
haru/
├── .github/workflows/build.yml          T3.4  tests, Vulkan + CUDA backends, NSIS bundle
├── README.md                            T3.5
├── docs/m0-feasibility.md               T0.3  measured per-frame compute on this machine
├── docs/screenshots/*.png               T2.8, T3.1
├── scripts/make-test-wav.ps1            T0.1  24 kHz mono 16-bit test question (System.Speech)
├── scripts/build-backends.ps1           T3.2  Windows variant builds → app/src-tauri/backends/<v>/
├── scripts/build-backends.sh            T3.2  POSIX variant builds
└── app/
    ├── package.json, package-lock.json, tsconfig.json, vite.config.ts, vitest.config.ts, index.html, app-icon.svg
    ├── src/
    │   ├── main.ts                       bootstrap: font, CSS, client selection, App
    │   ├── app.ts                        App controller: client ↔ renderer ↔ overlays
    │   ├── styles.css                    overlay/HUD styles
    │   ├── vite-env.d.ts                 /// <reference types="vite/client" />
    │   ├── ipc/types.ts                  THE CONTRACT (TS side)
    │   ├── ipc/contract.fixtures.json    shared JSON fixtures (checked by Rust and TS tests)
    │   ├── ipc/contract.test.ts
    │   ├── ipc/client.ts                 createClient(): Tauri or mock
    │   ├── ipc/tauri-client.ts           invoke/Channel/listen implementation
    │   ├── ipc/mock-client.ts            scripted boot/download/conversation for browser dev
    │   ├── gl/context.ts, program.ts, fbo.ts, quad.ts
    │   ├── fluid/sim.ts, fluid/params.ts, fluid/shaders/*.glsl
    │   ├── words/assembler.ts(+test), path.ts(+test), lifecycle.ts(+test), glyphs.ts, injector.ts
    │   ├── audio/features.ts(+test)      smoothing, spectral centroid
    │   ├── audio/coupling.ts(+test)      audio → vorticity/colors/splats
    │   ├── post/pipeline.ts, post/shaders/*.glsl
    │   ├── render/quality.ts(+test)      QualityController + QUALITY_TABLE
    │   ├── render/renderer.ts            frame loop
    │   └── ui/strings.ts, format.ts(+test), boot.ts, hint.ts, start.ts, hud.ts, settings.ts, subtitles.ts, toast.ts, keys.ts
    └── src-tauri/
        ├── Cargo.toml, Cargo.lock, build.rs, tauri.conf.json, capabilities/default.json, icons/*
        ├── backends/README.md            (variant folders are gitignored)
        ├── examples/dialogue_smoke.rs    T0.3  M0 spike, drives the real worker
        ├── examples/audio_check.rs       T1.5  device/tone/mic check
        ├── examples/headless_talk.rs     T1.8  M1 acceptance: conversation without UI
        └── src/
            ├── main.rs                   generate_context!() + haru_lib::run(ctx)
            ├── lib.rs                    module tree, Tauri builder, command registration
            ├── error.rs                  HaruError, ErrorCode
            ├── ipc.rs                    THE CONTRACT (Rust side) + fixture test
            ├── paths.rs                  cache/model/backends paths (mirrors cache.cpp)
            ├── settings.rs               Settings load/save/validate → DialogueOptions
            ├── model_fetch.rs            resumable verified download
            ├── commands.rs               #[tauri::command] adapters → Core
            ├── session.rs                Core: state machine, boot/start/stop/mute, fallback
            ├── session/state.rs          pure transition table
            ├── session/conversation.rs   one conversation: dispatch, playback feed, viz ticker
            ├── session/mic_pump.rs       capture → 20 ms chunks → worker
            ├── session/timeline.rs       frame timeline {seq,text,startSample}
            ├── session/latency.rs        backlog + policy
            ├── session/viz.rs            FeatureClock (stamped model blocks)
            ├── backend/mod.rs            detect(): variants + devices + cuda
            ├── backend/variants.rs, devices.rs, cuda_probe.rs, plan.rs
            ├── worker/mod.rs, worker/protocol.rs, worker/pcm.rs, worker/process.rs
            └── audio/mod.rs, devices.rs, capture.rs, playback.rs, resample.rs, features.rs, engine.rs
```

---

## IPC Contract

T1.0 creates these files exactly as written. Every later task codes against them.

### Rust: `app/src-tauri/src/error.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode { Download, HashMismatch, DiskSpace, NoBackend, Worker, NoMicrophone, AudioDevice, LagLimit, Settings, Io, Busy, Internal }

#[derive(Debug, Clone, PartialEq, serde::Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{code:?}: {message}")]
pub struct HaruError {
    pub code: ErrorCode,
    pub message: String,          // German, user-facing
    pub detail: Option<String>,   // technical: stderr tail, io error text
    #[serde(skip)]
    pub backend_failure: bool,    // true → the next backend attempt may succeed (device/out_of_memory/internal/crash/timeout)
}
impl HaruError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self;
    pub fn with_detail(self, detail: impl Into<String>) -> Self;
    pub fn backend_failure(self) -> Self; // sets backend_failure = true
}
impl From<std::io::Error> for HaruError { /* code Io, message "Dateifehler", detail = e.to_string() */ }
```

### Rust: `app/src-tauri/src/ipc.rs`

```rust
use serde::{Deserialize, Serialize};
use crate::{error::HaruError, settings::Settings};

pub const STATE_EVENT: &str = "haru:state";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppState { Booting, Downloading, Loading, Ready, Talking, Error }

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateEvent {
    pub state: AppState,
    pub backend: Option<String>,  // "cuda" | "vulkan" | "metal" | "cpu"
    pub device: Option<String>,   // device description, e.g. "Intel(R) Arc(TM) B580 Graphics"
    pub error: Option<HaruError>,
    pub warning: Option<String>,  // e.g. CPU-is-slow, backend switched after crash
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "event", content = "data")]
pub enum DownloadEvent {
    Progress { downloaded: u64, total: u64, bytes_per_sec: f64 },
    Verifying { done: u64, total: u64 },
    Done { path: String },
    Error { message: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct ModelLevels { pub rms: f32, pub bands: [f32; 16] }
#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct MicLevels { pub rms: f32, pub bands: [f32; 8] }
#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct WordPiece { pub seq: u64, pub text: String }

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VizFrame {
    pub t: f64,               // seconds of Haru audio actually played in this conversation
    pub model: ModelLevels,   // block audible now; zeros when nothing is audible
    pub mic: MicLevels,       // latest mic chunk; zeros while muted
    pub latency_ms: f64,      // backlog per latency policy, in ms
    pub words: Vec<WordPiece>,// every frame that became audible since the last VizFrame, seq order, text may be "" or " "
}

#[derive(Clone, Debug, Serialize)] #[serde(rename_all = "camelCase")]
pub struct SaveSettingsResult { pub settings: Settings, pub reload_required: bool }
#[derive(Clone, Debug, Serialize)] #[serde(rename_all = "camelCase")]
pub struct AudioDevices { pub inputs: Vec<String>, pub outputs: Vec<String>, pub default_input: Option<String>, pub default_output: Option<String> }
#[derive(Clone, Debug, Serialize)] #[serde(rename_all = "camelCase")]
pub struct DeviceInfo { pub variant: String, pub name: String, pub description: String, pub kind: String, pub memory_total: u64 }
#[derive(Clone, Debug, Serialize)] #[serde(rename_all = "camelCase")]
pub struct BackendOptions { pub variants: Vec<String>, pub devices: Vec<DeviceInfo>, pub cuda_available: bool, pub backends_dir: Option<String> }
#[derive(Clone, Debug, Serialize)] #[serde(rename_all = "camelCase")]
pub struct AttemptInfo { pub backend: String, pub device: String, pub description: String }
#[derive(Clone, Debug, Serialize)] #[serde(rename_all = "camelCase")]
pub struct BootInfo { pub backend: String, pub device: String, pub device_name: String, pub model_path: String, pub plan: Vec<AttemptInfo>, pub warning: Option<String> }
```

### Rust: `Settings` (defined in `settings.rs`, part of the contract)

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub language: String,            // "de"; one of de,en,es,fr,ja,ko,pt,ru,zh
    pub name: String,                // "Haru"
    pub persona: String,             // ""
    pub gender: Option<String>,      // None | "female" | "male"
    pub backchannel: Option<String>, // None | "low" | "mid" | "high"
    pub speak_first: bool,           // true
    pub temperature: f64,            // 0.7
    pub top_k: u32,                  // 25
    pub voice_path: Option<String>,  // None
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub backend: String,             // "auto" | "cuda" | "vulkan" | "metal" | "cpu"
    pub device: String,              // "auto" | device name from `speech devices` (e.g. "Vulkan1")
    pub quality: String,             // "auto" | "low" | "medium" | "high" | "ultra"
    pub subtitles: bool,             // false
    pub lag_limit_secs: f64,         // 1.0
    pub headphone_hint_seen: bool,   // false — addition: persists the first-start headphone hint (spec §4.7)
}
```

### TypeScript: `app/src/ipc/types.ts`

```ts
export const STATE_EVENT = 'haru:state';
export type AppStateName = 'booting' | 'downloading' | 'loading' | 'ready' | 'talking' | 'error';
export type Backend = 'cuda' | 'vulkan' | 'metal' | 'cpu';
export type ErrorCode = 'download' | 'hashMismatch' | 'diskSpace' | 'noBackend' | 'worker' | 'noMicrophone'
  | 'audioDevice' | 'lagLimit' | 'settings' | 'io' | 'busy' | 'internal';
export interface HaruError { code: ErrorCode; message: string; detail: string | null }
export interface StateEvent { state: AppStateName; backend: Backend | null; device: string | null; error: HaruError | null; warning: string | null }
export type DownloadEvent =
  | { event: 'progress'; data: { downloaded: number; total: number; bytesPerSec: number } }
  | { event: 'verifying'; data: { done: number; total: number } }
  | { event: 'done'; data: { path: string } }
  | { event: 'error'; data: { message: string } };
export interface WordPiece { seq: number; text: string }
export interface VizFrame { t: number; model: { rms: number; bands: number[] }; mic: { rms: number; bands: number[] }; latencyMs: number; words: WordPiece[] }
export type Quality = 'auto' | 'low' | 'medium' | 'high' | 'ultra';
export type QualityLevel = Exclude<Quality, 'auto'>;
export const LANGUAGES = ['de', 'en', 'es', 'fr', 'ja', 'ko', 'pt', 'ru', 'zh'] as const;
export interface Settings {
  language: string; name: string; persona: string; gender: 'female' | 'male' | null; backchannel: 'low' | 'mid' | 'high' | null;
  speakFirst: boolean; temperature: number; topK: number; voicePath: string | null; inputDevice: string | null; outputDevice: string | null;
  backend: 'auto' | Backend; device: string; quality: Quality; subtitles: boolean; lagLimitSecs: number; headphoneHintSeen: boolean;
}
export const DEFAULT_SETTINGS: Settings = { language: 'de', name: 'Haru', persona: '', gender: null, backchannel: null, speakFirst: true,
  temperature: 0.7, topK: 25, voicePath: null, inputDevice: null, outputDevice: null, backend: 'auto', device: 'auto', quality: 'auto',
  subtitles: false, lagLimitSecs: 1.0, headphoneHintSeen: false };
export interface SaveSettingsResult { settings: Settings; reloadRequired: boolean }
export interface AudioDevices { inputs: string[]; outputs: string[]; defaultInput: string | null; defaultOutput: string | null }
export interface DeviceInfo { variant: Backend; name: string; description: string; kind: 'cpu' | 'gpu' | 'igpu'; memoryTotal: number }
export interface BackendOptions { variants: Backend[]; devices: DeviceInfo[]; cudaAvailable: boolean; backendsDir: string | null }
export interface AttemptInfo { backend: Backend; device: string; description: string }
export interface BootInfo { backend: Backend; device: string; deviceName: string; modelPath: string; plan: AttemptInfo[]; warning: string | null }

export interface HaruClient {
  getStatus(): Promise<StateEvent>;
  onState(cb: (e: StateEvent) => void): Promise<() => void>;
  getSettings(): Promise<Settings>;
  saveSettings(s: Settings): Promise<SaveSettingsResult>;
  listAudioDevices(): Promise<AudioDevices>;
  backendOptions(): Promise<BackendOptions>;
  boot(onDownload: (e: DownloadEvent) => void): Promise<BootInfo>;
  startConversation(onViz: (f: VizFrame) => void): Promise<void>;
  stopConversation(): Promise<void>;
  setMuted(muted: boolean): Promise<void>;
}
```

### Commands

Rejected promises carry a `HaruError` object.

| invoke name | JS args | Resolves | Rust signature (`core: tauri::State<'_, Arc<Core>>`) | Semantics |
|---|---|---|---|---|
| `get_status` | – | `StateEvent` | `async fn get_status(core) -> Result<StateEvent, HaruError>` | Returns the last emitted state. |
| `get_settings` | – | `Settings` | `async fn get_settings(core) -> Result<Settings, HaruError>` | |
| `save_settings` | `{ settings }` | `SaveSettingsResult` | `async fn save_settings(core, settings: Settings) -> Result<SaveSettingsResult, HaruError>` | Validates (fails with `settings`) and saves atomically. `reloadRequired` is true when `backend`, `device` or `voicePath` changed. In that case the core stops the idle worker and sets the state to `booting`. While talking it returns `busy`. |
| `list_audio_devices` | – | `AudioDevices` | `async fn list_audio_devices() -> Result<AudioDevices, HaruError>` | |
| `backend_options` | – | `BackendOptions` | `async fn backend_options(core) -> Result<BackendOptions, HaruError>` | Variants and devices, for the settings dropdowns. |
| `boot` | `{ onDownload: Channel<DownloadEvent> }` | `BootInfo` | `async fn boot(core, on_download: tauri::ipc::Channel<DownloadEvent>) -> Result<BootInfo, HaruError>` | Allowed in booting/ready/error; talking → `busy`. If already ready with no reload pending, it returns at once without download events. Otherwise it runs: detect backends → ensure model (`downloading` only when needed) → load worker per plan (`loading`) → `ready`. A failure sets state `error` and rejects. |
| `start_conversation` | `{ onViz: Channel<VizFrame> }` | `void` | `async fn start_conversation(core, on_viz: Channel<VizFrame>) -> Result<(), HaruError>` | Requires ready (else `busy`). Opens audio (`noMicrophone`/`audioDevice` keep the state at ready), sends `dialogue`, and waits up to 30 s for frame 0. Resolves in `talking`. A backend failure moves to the next attempt (`loading`) and retries. |
| `stop_conversation` | – | `void` | `async fn stop_conversation(core) -> Result<(), HaruError>` | In talking: `hang_up`, wait up to 3 s for `end`, close audio → `ready`. In any other state it does nothing. |
| `set_muted` | `{ muted }` | `void` | `async fn set_muted(core, muted: bool) -> Result<(), HaruError>` | Any state. Applies to the current and later conversations. Not persisted. |

Every blocking command body runs inside `tauri::async_runtime::spawn_blocking`. State changes are emitted with `app.emit(STATE_EVENT, StateEvent)` through `tauri::Emitter`. TS listens with `listen<StateEvent>(STATE_EVENT, …)`.

### Fixtures: `app/src/ipc/contract.fixtures.json`

The file holds one object with keys `stateEvent`, `stateEventError`, `downloadProgress`, `downloadVerifying`, `downloadDone`, `downloadError`, `vizFrame`, `settingsDefault`. Values are written exactly as Rust serializes them. Floats are written with a decimal point, e.g. `52428800.0`, `1.0`.
- `stateEvent` = `{"state":"talking","backend":"vulkan","device":"Intel(R) Arc(TM) B580 Graphics","error":null,"warning":null}`
- `stateEventError` = error `{"code":"noBackend","message":"Keine Backend-Variante gefunden.","detail":null}`
- `downloadProgress` = `{"event":"progress","data":{"downloaded":1048576,"total":5605707520,"bytesPerSec":52428800.0}}`
- `vizFrame` = `t` 1.5, model rms 0.25 with bands `[0.0, 0.5, 0.0 ×14]`, mic rms 0.0 with 8 zeros, latencyMs 312.0, words `[{"seq":7,"text":" Hal"}]`
- `settingsDefault` = `Settings::default()`

The Rust test `ipc::tests::contract_fixtures_match` builds the same values, serializes them to `serde_json::Value`, and compares them with `include_str!("../../src/ipc/contract.fixtures.json")`. The TS test `contract.test.ts` assigns each fixture to its type with `satisfies` and asserts `fixtures.settingsDefault` deep-equals `DEFAULT_SETTINGS`.

---

## Execution Order and Parallelism

```
M0 (sequential):  T0.1 → T0.2 → T0.3 → GATE (p50 < 133 ms required; < 100 ms target)
Scaffold:         T1.0 (blocks both tracks)
Track R (Rust):   [T1.1 ∥ T1.2 ∥ T1.3 ∥ T1.4 ∥ T1.6] → T1.5 (needs T1.4) → T1.7 (needs T1.1–T1.6) → T1.8
Track U (UI):     T2.1 → [T2.2 ∥ T2.3 ∥ T2.7] → [T2.4 (T2.2+T2.3) ∥ T2.5 (T2.2)] → T2.6 → T2.8 (needs T2.7)
Any time after T0.1:  T3.2 (backend build scripts)
M3:               T3.1 (needs T1.8 + T2.8) → T3.3 (needs T3.2) → T3.4 → T3.5
```

Track R and Track U run fully in parallel after T1.0. Inside each bracket the tasks touch disjoint files.

---

## Milestone M0 – Feasibility

### Task 0.1: Initial commit, CLI sanity check, test WAV

**Files:** Create `scripts/make-test-wav.ps1`.

- [ ] **Step 1: Initial commit.** Run `git submodule update --init --recursive`, then `git add .gitignore .gitmodules extern/speech.cpp docs/superpowers && git commit -m "Add speech.cpp v0.11.1 submodule, design spec and plan"`.
- [ ] **Step 2: Write `scripts/make-test-wav.ps1`.**
  - It runs under Windows PowerShell 5.1, because `System.Speech` is missing in PowerShell 7.
  - Parameters: `-Out` (default `build\test\question-de.wav`), `-Text` (default `"Hallo Haru! Wie geht es dir heute? Erzähl mir bitte etwas über dich."`), `-Culture` (default `de-DE`).
  - It uses `Add-Type -AssemblyName System.Speech` and `SpeechSynthesizer`. It selects a voice for `-Culture` if one is installed. Otherwise it warns, uses the default voice, and the caller should switch to English text and `--language en`.
  - It calls `SetOutputToWaveFile($Out, (New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo 24000, ([System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen), ([System.Speech.AudioFormat.AudioChannel]::Mono)))` and creates the output folder.
- [ ] **Step 3: Generate the WAV.** Run `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\make-test-wav.ps1`. Expect `build\test\question-de.wav`: 24000 Hz, mono, 16-bit, about 4–6 s.
- [ ] **Step 4: Devices.** Run `& $SPEECH devices --json`. Expect the F15 shape listing the Arc B580 (`kind":"gpu"`) and the UHD (`"igpu"`). Note which `VulkanN` is the B580 (call it `$DEV`).
- [ ] **Step 5: Quick speed check.** Run `& $SPEECH talk $MODEL --device $DEV --language de --name Haru -o build\test\reply.wav build\test\question-de.wav`. Record the RTFx line from stderr (F20). This is informational only: no warm-up runs, so early frames include shader compilation.
- [ ] **Step 6: Driver version.** Run `Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion`.
- [ ] **Step 7: Commit.** `git add scripts/make-test-wav.ps1 && git commit -m "Add a script that generates a 24 kHz test question"`.

**Acceptance:** `reply.wav` exists and plays Haru's audible reply. The devices JSON and RTFx are noted for T0.3.

### Task 0.2: Crate bootstrap, worker protocol and PCM codec (TDD)

**Files:** Create:
- `app/src-tauri/Cargo.toml` (M0 subset: `serde`, `serde_json`, `thiserror`, `base64`; dev dependency `hound`; `[lib] name="haru_lib" crate-type=["rlib"]`, edition 2021)
- `app/src-tauri/src/lib.rs` (`pub mod error; pub mod worker;`)
- `src/error.rs` (exactly as in the contract)
- `src/worker/mod.rs` (`pub mod protocol; pub mod pcm; pub mod process;`)
- `src/worker/protocol.rs`, `src/worker/pcm.rs`
- `src/worker/process.rs` (stub for now: `//! Worker child process (Task 0.3).`)

**Interface (`protocol.rs`):**
```rust
pub const PROTOCOL: u32 = 4;
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DialogueOptions {   // every field: #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>, pub name: Option<String>, pub persona: Option<String>, pub gender: Option<String>,
    pub backchannel: Option<String>, pub other_name: Option<String>, pub relationship: Option<String>, pub situation: Option<String>,
    pub voice: Option<String>, pub temperature: Option<f64>, pub top_k: Option<u32>, pub top_p: Option<f64>, pub seed: Option<u64>,
    pub guidance_scale: Option<f64>, pub max_seconds: Option<f64>, pub speak_first: Option<bool>,
}
pub enum ClientMsg { Dialogue { id: String, sample_rate: u32, options: DialogueOptions }, Chunk { id: String, seq: u64, pcm: String }, HangUp { id: String }, Cancel { id: String } }
impl ClientMsg { pub fn to_line(&self) -> String }  // one JSON object + "\n"; options flattened into the dialogue object
#[derive(Debug, Clone, PartialEq, Deserialize)] pub struct WorkerErrorBody { pub code: String, pub option: Option<String>, pub message: String }
#[derive(Debug, Clone, PartialEq, Deserialize)] pub struct ModelInfo { pub name: String, pub task: String, pub sample_rate: u32,
    #[serde(default)] pub frame_samples: Option<u32>, #[serde(default)] pub device: Option<String>, #[serde(default)] pub weight_type: Option<String> }
#[derive(Debug, Clone, PartialEq, Deserialize)] pub struct Ready { pub protocol: u32, pub version: String, #[serde(default)] pub defaults: serde_json::Value, pub model: ModelInfo }
#[derive(Debug, Clone, PartialEq, Deserialize)] #[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerMsg {
    Ready(Ready),
    Fatal { error: WorkerErrorBody },
    Chunk { id: String, seq: u64, pcm: String, #[serde(default)] text: Option<String> },
    Progress { id: String, done: f64 },
    End { id: String, #[serde(default)] frames: Option<u64>, #[serde(default)] samples: Option<u64> },
    Error { #[serde(default)] id: Option<String>, error: WorkerErrorBody },
    Cancelled { id: String },
}
pub fn parse_line(line: &str) -> Result<WorkerMsg, HaruError>;   // non-JSON or unknown type → ErrorCode::Worker (worker defect, F-doc worker.md:24-25)
pub fn check_ready(r: &Ready) -> Result<(), HaruError>;          // protocol == 4 && model.task == "dialogue" && frame_samples == Some(3200) && sample_rate == 24000
pub fn worker_error(body: &WorkerErrorBody) -> HaruError;       // Worker code; backend_failure for codes device|out_of_memory|internal
```
**Interface (`pcm.rs`):** `encode_f32(&[f32]) -> String` (each sample `round(clamp(x,-1,1)*32767)`), `encode_i16(&[i16]) -> String`, `decode_i16(&str) -> Result<Vec<i16>, HaruError>`, `decode_f32(&str) -> Result<Vec<f32>, HaruError>` (each sample `/32768.0`).

**Tests.** Compare with `serde_json::Value`, so member order does not matter.

| Test | Input | Expected |
|---|---|---|
| `serializes_dialogue_with_only_set_options` | id `d1`, rate 48000, language `de`, speak_first true | `{"type":"dialogue","id":"d1","sample_rate":48000,"language":"de","speak_first":true}` |
| `serializes_chunk` | id `d1`, seq 3, pcm `AAA=` | `{"type":"chunk","id":"d1","seq":3,"pcm":"AAA="}` |
| `serializes_hang_up_and_cancel` | id `d1` | `{"type":"hang_up","id":"d1"}`, `{"type":"cancel","id":"d1"}`; line ends with `\n` |
| `parses_ready_with_model_info` | `{"type":"ready","protocol":4,"version":"0.11.1","defaults":{},"model":{"name":"Haru-4B","task":"dialogue","sample_rate":24000,"frame_samples":3200,"device":"Vulkan1","weight_type":"Q6_K","options":[]}}` | `Ready` with device `Vulkan1`; `check_ready` Ok |
| `parses_dialogue_chunk_with_text` | `{"type":"chunk","id":"d","seq":12,"pcm":"AAA=","text":"そう"}` | Chunk with text `Some("そう")` |
| `parses_chunk_without_text_as_none` | `{"type":"chunk","id":"a","seq":0,"pcm":"AAA="}` | text `None` |
| `parses_dialogue_end` | `{"type":"end","id":"d","frames":151,"samples":483200}` | frames 151, samples 483200 |
| `ignores_unknown_members` | `{"type":"end","id":"a","seed":1,"samples":9,"stop":"complete"}` | End, frames None |
| `parses_error_with_nested_error_object` | `{"type":"error","id":"g","error":{"code":"out_of_range","option":"max_seconds","message":"m"}}` | id `Some("g")`, code `out_of_range` |
| `parses_error_without_id` | `{"type":"error","error":{"code":"invalid_argument","option":"id","message":"m"}}` | id None |
| `parses_fatal` / `parses_cancelled` | per F5 / F8 | variants match |
| `rejects_non_json_line` / `rejects_unknown_type` | `hello` / `{"type":"nope"}` | Err code Worker |
| `check_ready_rejects_wrong_protocol` / `..._non_dialogue_model` | protocol 5 / task `synthesis` | Err Worker |
| `worker_error_marks_device_as_backend_failure` | code `device` vs `model_file` | `backend_failure` true vs false |
| `encode_f32_rounds_and_clamps` | `[0.0,1.0,-1.0,0.5,2.0,-2.0]` | i16 `[0,32767,-32767,16384,32767,-32767]` |
| `encode_known_bytes` / `decode_known_bytes` | `[0,32767]` ↔ `"AAD/fw=="` | decode_f32 gives `[0.0, 32767.0/32768.0]` |
| `decode_rejects_odd_length` / `decode_rejects_invalid_base64` | `"AA=="` / `"not base64!"` | Err Worker |
| `roundtrip_i16` | 1000 values from an LCG | equal |

- [ ] **Step 1:** Write the tests listed above. Run `cargo test --manifest-path app/src-tauri/Cargo.toml --lib worker::`. Expect compile failure or FAIL.
- [ ] **Step 2:** Implement `error.rs`, `protocol.rs` and `pcm.rs`. If derive + `flatten` misbehaves, `to_line` may build a `serde_json::Map` by hand.
- [ ] **Step 3:** Run the same command. Expect all tests to PASS.
- [ ] **Step 4:** `git add app/src-tauri && git commit -m "Add worker protocol types and PCM codec with tests"`. `target/` is ignored.

### Task 0.3: Worker process and `dialogue_smoke` spike

**Files:** Implement `src/worker/process.rs`. Create `app/src-tauri/examples/dialogue_smoke.rs` and `docs/m0-feasibility.md`.

**Interface (`process.rs`):**
```rust
pub const VOICE_NAME: &str = "user";
pub const READY_TIMEOUT: Duration = Duration::from_secs(300);
pub struct WorkerConfig { pub exe: PathBuf, pub model: PathBuf, pub device: String, pub voice: Option<PathBuf>, pub log_path: PathBuf }
pub fn worker_args(cfg: &WorkerConfig) -> Vec<OsString>; // ["worker", <model>, "--device", <device>] + ["--add-voice", "user=<voice>"] if voice
pub enum WorkerEvent { Message(WorkerMsg), Malformed { line: String }, Exited { code: Option<i32> } }
#[derive(Clone)] pub struct WorkerSender { /* Arc<Mutex<BufWriter<ChildStdin>>> */ }
impl WorkerSender { pub fn send(&self, m: &ClientMsg) -> Result<(), HaruError>; }   // write line + flush; broken pipe → Worker + backend_failure
pub struct WorkerProcess { /* child, sender, rx: Option<Receiver<WorkerEvent>>, log_path */ }
impl WorkerProcess {
    pub fn spawn(cfg: &WorkerConfig) -> Result<WorkerProcess, HaruError>; // stdin/stdout piped, stderr → log file (truncated), env SPEECH_NO_UPDATE_CHECK=1,
        // Windows: CommandExt::creation_flags(0x0800_0000 /* CREATE_NO_WINDOW */); Linux: prepend exe dir to LD_LIBRARY_PATH; reader thread: BufRead lines → parse_line → events
    pub fn wait_ready(&mut self, timeout: Duration) -> Result<Ready, HaruError>; // Ready → check_ready; Fatal → worker_error; Exited/timeout → Worker + backend_failure, detail = stderr_tail
    pub fn sender(&self) -> WorkerSender;
    pub fn take_events(&mut self) -> Receiver<WorkerEvent>;   // once; panics if taken twice
    pub fn stderr_tail(&self, max_bytes: usize) -> String;   // last bytes of log file, lossy UTF-8
    pub fn shutdown(&mut self, grace: Duration);             // drop stdin, wait up to grace, then kill
}
impl Drop for WorkerProcess { fn drop(&mut self) { self.shutdown(Duration::from_secs(2)) } }
```

**Unit tests:**
- `worker_args_without_voice` gives `["worker", "C:/m/h.gguf", "--device", "Vulkan1"]`.
- `worker_args_with_voice` adds `"--add-voice", "user=C:/v/me.wav"`.
- `spawn_missing_exe_is_worker_error`: `exe = "Z:/nope/speech.exe"` gives Err code Worker.

**`dialogue_smoke` CLI:** `--speech <exe> --model <gguf> --wav <24k mono i16> [--device auto] [--mode fast|realtime] [--tail-secs 6] [--language de] [--name Haru] [--speak-first] [--out reply.wav] [--log build/test/worker.log]`

- Read the WAV with hound. Reject anything that is not 24000 Hz, mono, 16-bit, and print a message saying to convert it. Append `tail-secs` of zeros. Zero-pad to whole 3200-sample frames.
- Spawn, then `wait_ready(READY_TIMEOUT)`. Print `load_s` = time from spawn to ready, plus `ready.model.device`.
- Send `dialogue` (id `smoke`, `sample_rate` 24000, options language/name/speak_first). Wait for chunk seq 0 and print `first_frame_s`.
- **fast mode (lockstep):** for each k, record `t_send[k]`, send chunk k (3200 samples, `encode_i16`), wait for chunk seq k+1. `compute_ms[k+1] = now − t_send[k]`.
- **realtime mode:** a sender thread sends chunk k at `t0 + k·133.333 ms`. The main thread receives frames. `compute_ms[k+1] = t_recv[k+1] − t_send[k]`. `lag_s = sent_secs − (frames_received−1)·0.133333`.
- Per frame print `frame {seq:>4}  compute {ms:>6.1} ms  lag {s:.2} s  text {text:?}`.
- Then send `hang_up` and wait for `end`. Assert `frames == n_chunks + 1`. Write the reply audio (24 kHz i16) to `--out`.
- Summary line: `load {s} s | first frame {s} s | frames {n} | compute p50 {} p90 {} p99 {} max {} ms | over 133 ms: {count} | text: "<concatenated>" | verdict PASS|MARGINAL|FAIL`.
- Exit codes: 0 PASS (p50 < 100 ms), 2 MARGINAL (100 ≤ p50 < 133), 1 FAIL (p50 ≥ 133 or a protocol error). Percentiles use nearest-rank over frames 1..n (frame 0 excluded).

- [ ] **Step 1:** Write the 3 unit tests. Run `cargo test --manifest-path app/src-tauri/Cargo.toml --lib worker::process`. Expect FAIL.
- [ ] **Step 2:** Implement `process.rs`. Run the tests. Expect PASS.
- [ ] **Step 3:** Write `examples/dialogue_smoke.rs` (percentiles in a local helper).
- [ ] **Step 4: Fast run on the B580.** `cargo run --release --manifest-path app/src-tauri/Cargo.toml --example dialogue_smoke -- --speech $SPEECH --model $MODEL --wav build\test\question-de.wav --device $DEV --mode fast --out build\test\smoke-reply.wav`
- [ ] **Step 5:** Run `--mode realtime` on `$DEV`, then `--mode fast` on the other Vulkan device (UHD), then `--device cpu --tail-secs 2` (reference only).
- [ ] **Step 6:** Write `docs/m0-feasibility.md` with:
  - machine, GPU names, driver versions, devices JSON;
  - exact commands;
  - one table row per run: device, mode, frames, load s, first frame s, p50, p90, p99, max, over-133 count, verdict;
  - the transcribed reply text excerpt;
  - the T0.1 RTFx;
  - a decision line.
- [ ] **Step 7:** `git add app/src-tauri/src/worker/process.rs app/src-tauri/examples/dialogue_smoke.rs docs/m0-feasibility.md && git commit -m "Add worker process client and dialogue_smoke spike with B580 measurements"`.

**GATE:**
- **PASS** (p50 < 100 ms on the B580, realtime lag stays < 0.5 s): continue to M1.
- **MARGINAL** (100–133 ms): continue, and report the numbers to the user.
- **FAIL** (p50 ≥ 133 ms): stop and report to the user with the measurements. Options are Q4_K (54→46 ms ratio, `docs/models/haru.md:31-37`), or a ggml/driver investigation. Do not continue.

---

## Milestone M1 – Rust Core (and shared scaffold)

### Task 1.0: Scaffold Tauri app, Vite frontend and the IPC contract

**Depends on:** GATE.

**Files:**
- Modify `app/src-tauri/Cargo.toml` to the full list below.
- Create `build.rs` (`fn main() { tauri_build::build() }`).
- Create `tauri.conf.json`, `capabilities/default.json`, `backends/README.md`.
- Create `src/main.rs`, `src/ipc.rs` (contract and fixture test).
- Create stubs, each containing only a `//!` doc line naming its responsibility and task: `src/paths.rs`, `settings.rs` (with the full `Settings` struct, `Default` impl and serde attributes from the contract), `model_fetch.rs`, `commands.rs`, `session.rs`, `session/{state,conversation,mic_pump,timeline,latency,viz}.rs`, `backend/{mod,variants,devices,cuda_probe,plan}.rs`, `audio/{mod,devices,capture,playback,resample,features,engine}.rs`.
- Modify `src/lib.rs`: declare all modules and add `pub fn run(context: tauri::Context<tauri::Wry>)` with a plain `tauri::Builder::default()` and no commands yet.
- Create `app/package.json`, `tsconfig.json`, `vite.config.ts`, `vitest.config.ts`, `index.html`, `src/main.ts` (renders the black canvas `#fluid` and `#overlay-root`), `src/vite-env.d.ts`, `src/ipc/types.ts`, `src/ipc/contract.fixtures.json`, `src/ipc/contract.test.ts`, `app-icon.svg` (1024² dark background with a gold-magenta radial glow).

**Cargo.toml dependencies:**
```toml
[build-dependencies]
tauri-build = { version = "2.2.0", features = [] }
[dependencies]
tauri = { version = "2.5.1", features = [] }
serde = { version = "1.0.219", features = ["derive"] }
serde_json = "1.0.140"
thiserror = "2.0.12"
base64 = "0.22.1"
sha2 = "0.10.8"
reqwest = { version = "0.12.15", default-features = false, features = ["rustls-tls", "stream"] }
futures-util = "0.3.31"
tokio = { version = "1.44.2", features = ["rt-multi-thread", "macros", "fs", "io-util", "time", "sync"] }
cpal = "0.15.3"
rubato = "0.16"      # stay on 0.16.x: 1.x has a different API
rustfft = "6.2.0"
libloading = "0.8"
rtrb = "0.3"
fs2 = "0.4.3"
[dev-dependencies]
tiny_http = "0.12.0"
tempfile = "3"
hound = "3.5.1"
```
`dirs` is not used. `paths.rs` reads environment variables exactly as `cache.cpp` does (F16), and Tauri's path API covers the config and log folders.

**package.json:**
- scripts: `dev`: `vite`; `build`: `tsc --noEmit && vite build`; `typecheck`: `tsc --noEmit`; `test`: `vitest run`; `tauri`: `tauri`.
- deps: `@tauri-apps/api ^2.5.0`, `@fontsource-variable/inter ^5.1.0`.
- devDeps: `@tauri-apps/cli ^2.5.0`, `typescript ^5.8.3`, `vite ^6.3.5`, `vitest ^3.1.1`.

**tsconfig:** `strict`, `target` ES2022, `module` ESNext, `moduleResolution` bundler, `lib` [ES2022, DOM, DOM.Iterable], `resolveJsonModule`, `types` ["vite/client"], `noEmit`.

**vite.config.ts:** `clearScreen:false`, `server {port:5173, strictPort:true}`, `envPrefix ['VITE_','TAURI_ENV_']`, `build {target:'es2022', sourcemap:true}`.

**vitest.config.ts:** `environment:'node'`, `include ['src/**/*.test.ts']`.

**tauri.conf.json (v2):**
- `productName` "Haru", `version` "0.1.0", `identifier` "dev.haru.fluidlight".
- `build`: `beforeDevCommand` "npm run dev", `devUrl` "http://localhost:5173", `beforeBuildCommand` "npm run build", `frontendDist` "../dist".
- `app.windows`: `[{ "title":"Haru","width":1280,"height":800,"minWidth":800,"minHeight":500 }]`, `app.security.csp`: null.
- `bundle`: `active` true, `targets` ["nsis"], `resources` ["backends/**/*"], `icon` [generated list].

**capabilities/default.json:** windows ["main"], permissions ["core:default","core:window:allow-set-fullscreen","core:window:allow-is-fullscreen"].

**main.rs:** `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] fn main() { haru_lib::run(tauri::generate_context!()) }`. Keeping `generate_context!` out of the library lets `cargo test --lib` and the examples build without `dist`.

- [ ] **Step 1:** Create all files. Run `npm --prefix app install` (this produces `package-lock.json`).
- [ ] **Step 2:** Run `npx --prefix app tauri icon app/app-icon.svg -o app/src-tauri/icons`. If the CLI rejects SVG, render a 1024² PNG with a one-off node script using an OffscreenCanvas-free approach: write a PNG through a pure-JS zlib encoder in the scratch folder, and keep only `app-icon.png` in the repo.
- [ ] **Step 3:** Run `npm --prefix app run build` (creates `app/dist`), then `cargo test --manifest-path app/src-tauri/Cargo.toml --lib` and `npm --prefix app run test`. Expect PASS, including `contract_fixtures_match` and `contract.test.ts`.
- [ ] **Step 4:** Run `npm --prefix app run tauri dev`. Expect a black 1280×800 window titled "Haru". Close it.
- [ ] **Step 5:** `git add app && git commit -m "Scaffold Tauri app, Vite frontend and the IPC contract"`.

**Acceptance:** Both test suites are green. The window opens. Every module file exists, so parallel tasks only fill their own files.

### Task 1.1: paths and settings (TDD)

**Parallel with:** T1.2, T1.3, T1.4, T1.6.
**Files:** `src/paths.rs`, `src/settings.rs`.

**Interface:**
```rust
// paths.rs
#[derive(Clone, Copy, Debug, PartialEq)] pub enum Os { Windows, Linux, MacOs }
impl Os { pub fn current() -> Os }
pub struct ModelSpec { pub repo: &'static str, pub revision: &'static str, pub file: &'static str, pub size: u64, pub sha256: &'static str }
pub const HARU_Q6K: ModelSpec = ModelSpec { repo: "sakasegawa/Haru-4B-GGUF", revision: "fc026182f7bbfaa0400b2d44358a5e9e327b957f",
    file: "Haru-4B-Q6_K.gguf", size: 5_605_707_520, sha256: "8db05e86acca046bc183d4e30fbda22ac775ba4767ae5a10bcd0d48f8d7db793" };
impl ModelSpec { pub fn url(&self, base: &str) -> String }  // "{base}/{repo}/resolve/{revision}/{file}", base "https://huggingface.co"
pub fn models_dir_with(os: Os, env: &dyn Fn(&str) -> Option<OsString>) -> Result<PathBuf, HaruError>; // F16; empty value = unset;
    // "absolute" is judged per `os` (Unix: starts with '/'; Windows: Path::is_absolute) so tests run on any host
pub fn models_dir() -> Result<PathBuf, HaruError>;
pub fn model_path(models_dir: &Path, spec: &ModelSpec) -> PathBuf; // <models>/sakasegawa--Haru-4B-GGUF/<sha256>/<file>
pub fn partial_path(path: &Path) -> PathBuf;                      // "<file>.part"
pub fn find_backends_dir(env_override: Option<PathBuf>, resource_dir: Option<PathBuf>, dev_dir: Option<PathBuf>) -> Option<PathBuf>;
    // first candidate containing ≥1 "<variant>/speech[.exe]"; dev_dir = Some(CARGO_MANIFEST_DIR/backends) only in debug builds
// settings.rs
impl Default for Settings { /* spec §4.6 + headphone_hint_seen=false */ }
pub fn load(path: &Path) -> Settings;            // missing → default; corrupt → rename to settings.json.bad, default
pub fn save(path: &Path, s: &Settings) -> Result<(), HaruError>; // validate, write settings.json.tmp, rename
impl Settings {
    pub fn validate(&self) -> Result<(), HaruError>; // code Settings
    pub fn dialogue_options(&self) -> DialogueOptions;
    pub fn needs_reload(&self, new: &Settings) -> bool; // backend | device | voice_path changed
}
```
**Validation rules:**
- language ∈ LANGUAGES
- temperature ∈ [0,2]
- top_k ∈ [0,500]
- lag_limit_secs ∈ [0.5,10]
- backend ∈ {auto,cuda,vulkan,metal,cpu}
- quality ∈ {auto,low,medium,high,ultra}
- gender ∈ {female,male}; backchannel ∈ {low,mid,high}
- voice_path exists as a file when set

**`dialogue_options` rules:**
- Always send language. Card fields need it (F13).
- Send name if non-empty, persona if non-empty, gender and backchannel if set.
- Send speak_first only when true. The model default is false.
- Send temperature only when ≠ 0.7, top_k only when ≠ 25.
- Send `voice` = `"user"` when voice_path is set.

**Tests:**
- paths:
  - `windows_uses_localappdata`: `C:\L\speech.cpp\models`
  - `windows_without_localappdata_errors`
  - `linux_uses_absolute_xdg_cache_home`: XDG `/x/c` gives `/x/c/speech.cpp/models`
  - `linux_ignores_relative_xdg_cache_home`: gives `HOME/.cache/speech.cpp/models`
  - `linux_without_home_errors`
  - `macos_uses_library_caches`
  - `speech_model_dir_absolute_overrides`: returned as is, nothing appended
  - `speech_model_dir_relative_errors`
  - `empty_values_count_as_unset`
  - `model_path_layout`
  - `partial_path_appends_part`
  - `model_url_matches_spec`
  - `backends_dir_search_order`: three tempdirs; only the 2nd and 3rd contain `vulkan/speech.exe`, so the 2nd is returned; env override wins when valid
- settings:
  - `defaults_match_spec`
  - `missing_file_gives_defaults`
  - `partial_json_fills_defaults`: `{"language":"en"}`
  - `save_then_load_roundtrip`
  - `save_leaves_no_tmp`
  - `corrupt_json_gives_defaults_and_backup`
  - `validate_rejects_unknown_language`: `"xx"`
  - `validate_rejects_temperature_2_5`
  - `validate_rejects_backend_opencl`
  - `validate_rejects_lag_limit_0_2`
  - `validate_rejects_missing_voice_file`
  - `dialogue_options_defaults`: exactly `{"language":"de","name":"Haru","speak_first":true}`
  - `dialogue_options_include_changed_sampling`: temperature 0.9, top_k 40
  - `dialogue_options_include_card_fields`
  - `dialogue_options_omit_empty_name`
  - `dialogue_options_voice_uses_added_name`
  - `needs_reload_on_backend_device_voice_only`: changing persona gives false

- [ ] **Step 1:** Write the tests. Run `cargo test --manifest-path app/src-tauri/Cargo.toml --lib paths:: settings::`. Expect FAIL.
- [ ] **Step 2:** Implement. Run again. Expect PASS.
- [ ] **Step 3:** `git add app/src-tauri/src/paths.rs app/src-tauri/src/settings.rs && git commit -m "Add model/cache/backends paths and persisted settings"`.

### Task 1.2: model_fetch (TDD against a local HTTP server)

**Parallel with:** T1.1, T1.3, T1.4, T1.6.
**Files:** `src/model_fetch.rs` (tests in `#[cfg(test)] mod tests`, with a tiny_http helper in the same module).

**Interface:**
```rust
pub struct FetchConfig { pub url: String, pub dest: PathBuf, pub size: u64, pub sha256: String }
pub type DownloadSink = Arc<dyn Fn(DownloadEvent) + Send + Sync>;
pub type FreeSpace = fn(&Path) -> std::io::Result<u64>;
pub const FREE_MARGIN: u64 = 64 * 1024 * 1024;
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
pub const STALL_TIMEOUT: Duration = Duration::from_secs(30);
pub const RETRY_DELAYS: [Duration; 3] = [Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(4)];
pub fn is_present(cfg: &FetchConfig) -> bool; // dest is a file of exactly cfg.size bytes
pub async fn ensure_model(cfg: &FetchConfig, sink: DownloadSink) -> Result<PathBuf, HaruError>; // fs2::available_space, default client, RETRY_DELAYS
pub async fn ensure_model_with(cfg: &FetchConfig, sink: DownloadSink, free: FreeSpace, client: &reqwest::Client, retry_delays: &[Duration]) -> Result<PathBuf, HaruError>;
```

**Algorithm:**
1. If dest has exactly the expected size, emit `Done` and return. It is trusted, because only a verified rename creates it. If dest exists with the wrong size, delete it.
2. Create the parent folders. Let `offset` = size of `.part`, or 0. If `offset` > size, delete `.part` and set `offset` = 0.
3. Require `free(parent) ≥ size − offset + FREE_MARGIN`. Otherwise fail with `DiskSpace` and a message giving needed and available GB.
4. Hash the existing part in 1 MiB reads, emitting `Verifying{done,total=offset}` at most every 100 ms.
5. GET with `Range: bytes={offset}-` when offset > 0. On 206, append. On 200, truncate, reset the hasher and offset, and write from 0. Any other status fails with `Download`, detail `HTTP {status}`.
6. Stream with `bytes_stream()`. Each `next()` is wrapped in `tokio::time::timeout(STALL_TIMEOUT)`. Write each chunk, update the hash, and emit `Progress` at most every 100 ms plus once at the end. `bytesPerSec` is an EMA (α = 0.3) of 500 ms window rates.
7. Network or stream error: retry with the delays, resuming from the current `.part` (back to step 2). Once retries are exhausted, emit `Error` and return `Download`.
8. When done: if downloaded ≠ size, retry as in step 7. Emit `Verifying{done:size,total:size}`. Compare the hex sha. On mismatch, delete `.part`, emit `Error`, and return `HashMismatch`. On match, rename `.part` → dest, emit `Done{path}`, and return.

User-Agent is `haru-app/0.1`, connect timeout 15 s, redirects followed (Hugging Face redirects to a CDN).

**Test server helper:** `serve(blob: Arc<Vec<u8>>, mode: Mode) -> (String /*base url*/, Arc<Mutex<Vec<Option<String>>>> /*Range headers seen*/)`. `Mode`:
- `Normal`: honours `bytes=N-` with 206 and `Content-Range: bytes N-{len-1}/{len}`.
- `IgnoreRange`: always 200 with the full body.
- `Corrupt`: flips byte 1000.
- `DropFirstAfter(n)`: on the first request, declares the full Content-Length but sends only n bytes. Later requests behave as Normal.

The blob is 1 MiB from an LCG. Tests use `retry_delays = [10ms; 3]`.

| Test | Setup | Expected |
|---|---|---|
| `fresh_download_writes_verified_file` | no part, Normal | Ok; dest bytes == blob; no `.part`; Range seen `[None]`; events contain Progress (last downloaded == total), Verifying, Done |
| `resume_with_206_appends` | `.part` = first 300 000 bytes, Normal | Range seen `[Some("bytes=300000-")]`; dest == blob; first Verifying has total 300000 |
| `server_ignoring_range_restarts_with_200` | part 300 000 bytes, IgnoreRange | dest == blob; size == 1 MiB (no duplication) |
| `hash_mismatch_deletes_part` | Corrupt | Err HashMismatch; no `.part`, no dest; last event Error |
| `existing_file_with_exact_size_is_used_without_request` | dest with 1 MiB of zeros | Ok; requests seen == 0 |
| `insufficient_disk_space_fails_before_request` | `free` returns 1000 | Err DiskSpace; requests == 0 |
| `oversized_part_is_discarded` | part 2 MiB | fresh download Ok; first Range None |
| `dropped_connection_resumes_automatically` | DropFirstAfter(400 000) | Ok; Range seen `[None, Some("bytes=400000-")]` |

- [ ] **Step 1:** Write the helper and tests. Run `cargo test --manifest-path app/src-tauri/Cargo.toml --lib model_fetch::`. Expect FAIL.
- [ ] **Step 2:** Implement. Run again. Expect 8 PASS.
- [ ] **Step 3:** `git add app/src-tauri/src/model_fetch.rs && git commit -m "Add resumable, hash-verified model download"`.

### Task 1.3: backend detection, CUDA probe, choose_plan (TDD)

**Parallel with:** T1.1, T1.2, T1.4, T1.6.
**Files:** `src/backend/{mod,variants,devices,cuda_probe,plan}.rs`.

**Interface:**
```rust
// variants.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)] #[serde(rename_all = "lowercase")]
pub enum Variant { Cuda, Vulkan, Metal, Cpu }
impl Variant { pub const ALL: [Variant; 4] = [Variant::Cuda, Variant::Vulkan, Variant::Metal, Variant::Cpu];
    pub fn as_str(self) -> &'static str; pub fn parse(s: &str) -> Option<Variant>; pub fn label(self) -> &'static str /* "CUDA","Vulkan","Metal","CPU" */ }
pub fn exe_name() -> &'static str; // "speech.exe" on Windows else "speech"
#[derive(Clone, Debug, PartialEq)] pub struct VariantExe { pub variant: Variant, pub exe: PathBuf }
pub fn discover(dir: &Path) -> Vec<VariantExe>; // order of Variant::ALL; only folders that contain the exe
// devices.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)] #[serde(rename_all = "lowercase")] pub enum DeviceKind { Cpu, Gpu, Igpu }
#[derive(Clone, Debug, PartialEq, Deserialize)] pub struct Device { pub name: String, pub description: String, pub kind: DeviceKind, pub memory_total: u64, pub memory_free: u64 }
pub fn parse_devices_json(s: &str) -> Result<Vec<Device>, HaruError>; // F15 shape {"devices":[...]}
pub fn query_devices(exe: &Path) -> Result<Vec<Device>, HaruError>; // `<exe> devices --json`, CREATE_NO_WINDOW, SPEECH_NO_UPDATE_CHECK=1, 20 s timeout
// cuda_probe.rs
pub fn probe_cuda() -> bool; // libloading "nvcuda.dll" (Windows) / "libcuda.so.1" (Linux); macOS false;
    // cuInit: unsafe extern "C" fn(u32) -> i32 == 0 && cuDeviceGetCount: unsafe extern "C" fn(*mut i32) -> i32 == 0 && count > 0
// plan.rs
#[derive(Clone, Debug, Default, PartialEq)] pub struct Overrides { pub backend: Option<Variant>, pub device: Option<String> }
impl Overrides { pub fn resolve(env: &dyn Fn(&str) -> Option<String>, s: &Settings) -> Overrides } // HARU_BACKEND/HARU_DEVICE beat settings; "auto" → None
#[derive(Clone, Debug, PartialEq)] pub struct Attempt { pub variant: Variant, pub exe: PathBuf, pub device: String, pub description: String }
pub fn choose_plan(cuda_ok: bool, variants: &[VariantExe], devices: &HashMap<Variant, Vec<Device>>, ov: &Overrides) -> Vec<Attempt>;
pub fn run_attempts<T>(plan: &[Attempt], try_one: impl FnMut(&Attempt) -> Result<T, HaruError>) -> Result<(Attempt, T), HaruError>;
// mod.rs
pub struct Detection { pub dir: PathBuf, pub variants: Vec<VariantExe>, pub devices: HashMap<Variant, Vec<Device>>, pub cuda_ok: bool }
pub fn detect(dir: Option<&Path>) -> Result<Detection, HaruError>; // None or empty → NoBackend ("Keine Backend-Variante gefunden. Baue sie mit scripts/build-backends.ps1 oder setze HARU_BACKENDS_DIR."), detail = searched dirs;
    // query devices for every variant except cuda when !cuda_ok; a failed query → no entry (logged)
pub fn to_backend_options(d: &Detection) -> BackendOptions;
```

**`choose_plan` rules, in this order:**
1. Override first. If `ov.backend` is available, its attempt uses `ov.device` (written as the listed name, matched case-insensitively) or the auto device rule for that variant. If only `ov.device` is set, use the first variant whose device list contains it. Unavailable overrides are ignored.
2. **cuda**, if `cuda_ok` and the variant is present. Device: the `gpu` with the largest `memory_total`.
3. **vulkan**: the `gpu` with the largest memory, else the `igpu` with the largest memory. Skip vulkan if neither exists.
4. **metal**: the largest non-cpu device. Skip metal if there is none.
5. **cpu**: the `cpu` variant exe if present, otherwise the first available variant's exe, with device `"cpu"`, description `"CPU"`.
- When a variant has no device list entry, its device is `"auto"` with description `"auto"`.
- Remove duplicates by `(exe, lowercase device)`, keeping the first.
- No variants gives an empty plan.

**`run_attempts`:** try in order. Stop at the first Ok. If an Err has `backend_failure == false`, return it at once. If all fail, return a Worker error with message "Kein Backend konnte gestartet werden." and detail = one `"{variant}/{device}: {message}"` line per attempt.

**Fixture devices:**
- `UHD` = `{Vulkan0, "Intel(R) UHD Graphics 770", igpu, 16e9}`
- `B580` = `{Vulkan1, "Intel(R) Arc(TM) B580 Graphics", gpu, 12e9}`
- `CPU` = `{CPU, "CPU", cpu}`

| Test | Inputs | Expected plan `(variant/device)` |
|---|---|---|
| `vulkan_only_picks_discrete_over_integrated` | vulkan; [UHD,B580,CPU] | vulkan/Vulkan1, vulkan/cpu |
| `vulkan_only_integrated_when_no_discrete` | vulkan; [UHD,CPU] | vulkan/Vulkan0, vulkan/cpu |
| `cuda_first_when_probe_ok` | cuda,vulkan,cpu; cuda [CUDA0 8G, CUDA1 24G]; vulkan [Vulkan0 gpu 24G, Vulkan1 gpu 8G] | cuda/CUDA1, vulkan/Vulkan0, cpu/cpu |
| `cuda_skipped_when_probe_fails` | same, cuda_ok=false | vulkan/Vulkan0, cpu/cpu |
| `cuda_skipped_when_variant_missing` | cuda_ok=true; vulkan [B580] | vulkan/Vulkan1, vulkan/cpu |
| `override_backend_and_device_first` | ov vulkan+Vulkan0; vulkan [UHD,B580] | vulkan/Vulkan0, vulkan/Vulkan1, vulkan/cpu |
| `override_device_only_case_insensitive` | ov device "vulkan0" | first vulkan/Vulkan0 |
| `override_backend_cpu_first_without_duplicate` | ov cpu; vulkan [B580] | vulkan(exe)/cpu, vulkan/Vulkan1 |
| `override_unavailable_variant_ignored` | ov cuda; vulkan [B580] | vulkan/Vulkan1, vulkan/cpu |
| `no_variants_empty_plan` | – | [] |
| `metal_variant` | metal,cpu; metal [MTL0 gpu] | metal/MTL0, cpu/cpu |
| `missing_device_list_uses_auto` | vulkan; {} | vulkan/auto, vulkan/cpu |

**Other tests:**
- `parses_cli_doc_example`: the F15 line
- `parses_cpu_and_igpu_kinds`
- `rejects_malformed_devices_json`
- `discover_finds_variants_in_fixed_order`: tempdir with cpu and vulkan gives [vulkan, cpu]
- `discover_ignores_dirs_without_exe`
- `env_overrides_settings`
- `auto_means_none`
- `probe_does_not_panic`: prints the result; false on this machine
- `first_success_wins`
- `falls_through_backend_failures`
- `stops_on_config_error`
- `all_failed_collects_messages`

- [ ] **Step 1:** Write the tests. Run `cargo test --manifest-path app/src-tauri/Cargo.toml --lib backend::`. Expect FAIL.
- [ ] **Step 2:** Implement. Run again. Expect PASS. Also run once: `cargo test ... backend::devices -- --ignored` is not needed. Instead, manually call `query_devices` on `$SPEECH` from the `probe_does_not_panic` test output: add `#[ignore] fn query_real_vulkan_variant()` reading `HARU_TEST_SPEECH`, and run it with `--ignored`.
- [ ] **Step 3:** `git add app/src-tauri/src/backend && git commit -m "Add backend variant discovery, CUDA probe and execution plan"`.

### Task 1.4: audio DSP: resampler, feature analysis, jitter gate (TDD)

**Parallel with:** T1.1, T1.2, T1.3, T1.6.
**Files:** `src/audio/resample.rs`, `src/audio/features.rs`, the `JitterGate` part of `src/audio/playback.rs`.

**Interface:**
```rust
// resample.rs
pub struct FrameResampler { /* rubato::FftFixedIn<f32> or passthrough */ }
impl FrameResampler { pub fn new(from: u32, to: u32, chunk_in: usize) -> Result<Self, HaruError>;   // FftFixedIn::new(from, to, chunk_in, 2, 1)
    pub fn process(&mut self, input: &[f32]) -> Vec<f32>;  // input.len() == chunk_in; passthrough copies when from == to
    pub fn output_len(&self) -> usize; }
// features.rs
pub const BLOCK: usize = 400; pub const MODEL_BANDS: usize = 16; pub const MIC_BANDS: usize = 8;
#[derive(Clone, Debug, PartialEq)] pub struct Features { pub rms: f32, pub bands: Vec<f32> }
pub struct Analyzer { /* rustfft plan (next_pow2(len)), Hann window, band bin ranges */ }
impl Analyzer { pub fn new(rate: u32, block_len: usize, bands: usize, fmin: f32, fmax: f32) -> Self;  // model: (24000,400,16,60,12000); mic: (rate,rate/50,8,80,8000)
    pub fn analyze(&mut self, block: &[f32]) -> Features;  // rms linear; band = mean |X_k|·2/Σwindow over log-spaced edges, dB = 20·log10(v+1e-9), value = clamp((dB+80)/80, 0, 1)
    pub fn band_edges_hz(&self) -> &[(f32, f32)]; }
// playback.rs (pure part)
pub struct JitterGate { prebuffer: usize, playing: bool, pub underruns: u64 }
impl JitterGate { pub fn new(prebuffer: usize) -> Self;
    pub fn decide(&mut self, available: usize, need: usize) -> usize; } // samples to read; caller zero-fills the rest
```
**JitterGate rules:**
- When not playing: with `available ≥ prebuffer`, start playing and read `min(available, need)`. Otherwise read 0.
- When playing: read `min(available, need)`. If `available < need`, stop playing and count an underrun.

**Tests:**
- resample:
  - `passthrough_when_rates_equal`: identical output
  - `upsample_24k_to_48k_length`: 10 frames of 3200 give 64000 total
  - `resample_24k_to_44k1_length`: 10 frames give 58800 total (5880 per frame)
  - `sine_frequency_preserved`: 1 kHz at 24k → 48k; zero crossings in frames 3–9 within ±2 % of 2/ms
- features:
  - `silence_gives_zero`: rms 0, all bands 0
  - `full_scale_sine_rms`: 0.707 ± 0.01
  - `sine_1khz_peaks_in_its_band`: the band whose edges contain 1000 Hz is the max and > 0.8
  - `band_counts`: 16 and 8
  - `band_edges_log_spaced`: monotonic; consecutive ratios equal within 1 %
- gate:
  - `silent_until_prebuffer`: prebuffer 6400, `decide(6000,480)==0`
  - `starts_at_prebuffer`: `decide(6400,480)==480`
  - `underrun_reads_partial_and_rebuffers`: `decide(100,480)==100`, then `decide(3000,480)==0`
  - `counts_underruns`

- [ ] **Step 1:** Write the tests. Run `cargo test --manifest-path app/src-tauri/Cargo.toml --lib audio::`. Expect FAIL.
- [ ] **Step 2:** Implement. Run again. Expect PASS.
- [ ] **Step 3:** `git add app/src-tauri/src/audio/resample.rs app/src-tauri/src/audio/features.rs app/src-tauri/src/audio/playback.rs && git commit -m "Add frame resampler, spectral features and jitter gate"`.

### Task 1.5: audio devices, capture, playback, engine (cpal) + `audio_check`

**Depends on:** T1.4.
**Files:** `src/audio/{devices,capture,playback,engine,mod}.rs`, `examples/audio_check.rs`.

**Interface:**
```rust
// devices.rs
pub fn list() -> Result<AudioDevices, HaruError>;
pub fn input_device(name: Option<&str>) -> Result<cpal::Device, HaruError>;  // none at all → NoMicrophone ("Kein Mikrofon gefunden."); unknown name → AudioDevice
pub fn output_device(name: Option<&str>) -> Result<cpal::Device, HaruError>; // → AudioDevice
// capture.rs
pub struct CaptureHandle { pub rate: u32, pub consumer: rtrb::Consumer<f32>, pub overruns: Arc<AtomicU64> }
pub fn build_capture(dev: &cpal::Device) -> Result<(cpal::Stream, CaptureHandle), HaruError>;
    // default_input_config; any SampleFormat (I8..F64) → f32 via cpal::Sample; average channels; ring 2 s; full → drop newest, count
// playback.rs
pub struct PlaybackShared { pub played: AtomicU64, pub underruns: AtomicU64, pub playing: AtomicBool }
pub struct PlaybackHandle { pub rate: u32, pub producer: rtrb::Producer<f32>, pub shared: Arc<PlaybackShared>, pushed: u64 }
impl PlaybackHandle { pub fn push(&mut self, s: &[f32]) -> u64; /* returns start index; ring full → drop tail */ pub fn pushed(&self) -> u64; pub fn buffered(&self) -> u64; }
pub fn build_playback(dev: &cpal::Device, prebuffer_frames: usize) -> Result<(cpal::Stream, PlaybackHandle), HaruError>;
    // F32/I16/U16 outputs; ring 4 s; callback uses JitterGate(prebuffer = 3200·rate/24000·prebuffer_frames), copies mono to all channels,
    // played += samples actually read (not zero fill)
// engine.rs
pub struct AudioEngine { /* thread JoinHandle, stop flag */ }
impl AudioEngine {
    pub fn start(input: Option<String>, output: Option<String>) -> Result<(AudioEngine, CaptureHandle, PlaybackHandle), HaruError>;
        // builds and plays both streams ON ITS OWN THREAD (cpal::Stream is not Send on every platform), returns handles via a channel, parks until stop
    pub fn stop(self);
}
```
**`audio_check`:** `cargo run --release --example audio_check -- [--input NAME] [--output NAME] [--seconds 6]`.
- Print `list()`.
- Play a 440 Hz sine, amplitude 0.2, generated at 24 kHz in 3200-sample frames through `FrameResampler(24000, out_rate, 3200)` into `PlaybackHandle` at real-time pace.
- Every 0.5 s print `played`, `buffered`, `underruns`, and the mic RMS plus 8 bands from `Analyzer(rate, rate/50, 8, 80, 8000)` over the drained capture.

- [ ] **Step 1:** Implement the modules and the example. Run `cargo build --manifest-path app/src-tauri/Cargo.toml --examples`. Expect success.
- [ ] **Step 2:** With headphones, run `cargo run --release --manifest-path app/src-tauri/Cargo.toml --example audio_check`. Check:
  - a clean tone with no crackle;
  - `underruns` stays at 0 after the first print;
  - `played` grows by about 0.5 s × out_rate per print;
  - mic RMS rises above 0.02 when you speak.
- [ ] **Step 3:** `git add app/src-tauri/src/audio app/src-tauri/examples/audio_check.rs && git commit -m "Add cpal capture/playback engine and audio check example"`.

### Task 1.6: session timeline, latency, viz clock (TDD)

**Parallel with:** T1.1–T1.4.
**Files:** `src/session/timeline.rs`, `latency.rs`, `viz.rs`.

**Interface:**
```rust
// timeline.rs
pub struct FrameTimeline { /* VecDeque<(seq, text, start_sample)> */ }
impl FrameTimeline { pub fn push(&mut self, seq: u64, text: String, start_sample: u64);
    pub fn drain_audible(&mut self, played: u64) -> Vec<WordPiece>; } // entries with start_sample <= played, in order, once
// latency.rs
pub const FRAME_SECS: f64 = 3200.0 / 24000.0; pub const LAG_WARN_SECS: f64 = 0.5;
pub fn backlog_secs(sent_mic_samples: u64, mic_rate: u32, frames_received: u64, buffered_out: u64, out_rate: u32) -> f64;
    // sent/mic_rate − frames_received.saturating_sub(1)·FRAME_SECS + buffered_out/out_rate
    // (frame 0 arrives before any audio, F7, so it pays for no mic time — refinement of spec §4.5 formula)
#[derive(Debug, PartialEq)] pub enum LagLevel { Ok, Warn, Exceeded }
pub fn classify(backlog: f64, limit_secs: f64) -> LagLevel; // > limit → Exceeded; > 0.5 → Warn
// viz.rs
pub struct FeatureClock { /* VecDeque<(start_out_sample, Features)>, block_len_out */ }
impl FeatureClock { pub fn new(block_len_out: u64) -> Self; pub fn push(&mut self, start: u64, f: Features);
    pub fn current(&mut self, played: u64) -> Option<Features>; } // block with start <= played < start+block_len_out; drops older blocks
pub fn build_frame(t: f64, model: Option<Features>, mic: Option<Features>, latency_ms: f64, words: Vec<WordPiece>) -> VizFrame; // None → zeros
```
**Tests:**
- timeline:
  - `frames_released_when_audible`: seq0@0, seq1@6400; `drain(0)` = [0], `drain(6399)` = [], `drain(6400)` = [1]
  - `released_once_in_order`
  - `empty_and_space_text_included`
- latency:
  - `backlog_zero_when_in_sync`: 96000 @48k, 16 frames, 0 buffered → 0.0 ± 1e-9
  - `counts_buffered_playback`: +4800 @48k → 0.1
  - `grows_when_model_slow`: 96000 @48k, 8 frames → 2.0 − 7·0.13333 = 1.0667
  - `frame_zero_costs_nothing`: 0 sent, 1 frame → 0.0
  - `classify_thresholds`: 0.4 Ok, 0.6 Warn, 1.01 Exceeded at limit 1.0, 1.5 Warn at limit 2.0
- viz:
  - `returns_block_containing_played`
  - `none_before_first_block`
  - `none_after_last_block_end`
  - `drops_old_blocks`
  - `build_frame_zero_fills`: 16 and 8 zeros

- [ ] **Step 1:** Write the tests. Run `cargo test --manifest-path app/src-tauri/Cargo.toml --lib session::`. Expect FAIL.
- [ ] **Step 2:** Implement. Run again. Expect PASS.
- [ ] **Step 3:** `git add app/src-tauri/src/session/timeline.rs app/src-tauri/src/session/latency.rs app/src-tauri/src/session/viz.rs && git commit -m "Add frame timeline, latency policy and feature clock"`.

### Task 1.7: conversation engine and mic pump

**Depends on:** T1.1–T1.6.
**Files:** `src/session/mic_pump.rs`, `src/session/conversation.rs`.

**Interface:**
```rust
// mic_pump.rs
pub struct MicShared { pub muted: AtomicBool, pub started: AtomicBool, pub sent_samples: AtomicU64, pub levels: Mutex<Option<Features>>, pub stop: AtomicBool }
pub fn spawn_mic_pump(capture: CaptureHandle, sender: WorkerSender, id: String, shared: Arc<MicShared>, on_fatal: Arc<dyn Fn(HaruError) + Send + Sync>) -> JoinHandle<()>;
// conversation.rs
pub type VizSink = Arc<dyn Fn(VizFrame) + Send + Sync>;
pub enum ConversationEnd { HungUp, Failed(HaruError) }
pub struct Conversation { /* id, engine, threads, shared */ }
impl Conversation {
    pub fn start(sender: WorkerSender, events: Receiver<WorkerMsg>, settings: &Settings, muted: bool, on_viz: VizSink,
                 on_end: Arc<dyn Fn(ConversationEnd) + Send + Sync>, id: String) -> Result<Conversation, HaruError>;
    pub fn set_muted(&self, muted: bool);
    pub fn hang_up(self, timeout: Duration) -> Result<(), HaruError>; // send hang_up, wait End ≤ 3 s, else cancel; stop threads + audio
}
```

**Mic pump behaviour:**
- Chunk = `rate/50` samples. Poll every 5 ms.
- Until `started`, drain and discard the capture. This matches `speech talk` dropping audio heard during the prompt (`tools/cli/talk.cpp:127-128`).
- After that, for each full chunk:
  - analyze it into `levels` (zeros while muted);
  - replace the samples with zeros if muted, still consuming the capture;
  - send `ClientMsg::Chunk{seq++, pcm: encode_f32}`;
  - add to `sent_samples`.
- No capture data for 1000 ms → `on_fatal(NoMicrophone "Das Mikrofon liefert keine Daten.")`.
- Send errors → `on_fatal`.

**Conversation start sequence:**
1. `AudioEngine::start(input_device, output_device)`.
2. Send `Dialogue{id, sample_rate: capture.rate, options: settings.dialogue_options()}`.
3. Start the mic pump (not yet started).
4. Dispatch thread: wait for `Chunk seq 0` with `FIRST_FRAME_TIMEOUT = 30 s`. A timeout, `Error` or channel close fails start with `backend_failure`.
5. Then set `started = true` and keep dispatching.

**For every worker `Chunk`:**
- `decode_f32` the frame.
- `FrameResampler(24000, out_rate, 3200).process` the frame.
- `start = playback.push(out)`.
- `timeline.push(seq, text.unwrap_or_default(), start)`.
- Analyze 8 blocks of 400 at 24 kHz. Push each into `FeatureClock` at `start + round(i·400·out_rate/24000)`, with `block_len_out = round(400·out_rate/24000)`.
- Count `frames_received`.

**Other worker messages:**
- `Error` → `on_end(Failed(worker_error))`.
- `End` → `on_end(HungUp)`.
- `Cancelled` → `on_end(Failed(LagLimit…))` only when it was cancelled for lag.

**Viz ticker thread (every 16 ms):**
- `played = shared.played`.
- `words = timeline.drain_audible(played)`.
- `model = clock.current(played)`.
- `backlog = backlog_secs(sent, mic_rate, frames_received, playback.buffered(), out_rate)`.
- `on_viz(build_frame(played/out_rate, model, mic_levels, backlog·1000, words))`.
- If `classify(backlog, settings.lag_limit_secs) == Exceeded`: send `Cancel{id}` and call `on_end(Failed(HaruError::new(LagLimit, format!("Haru kommt nicht hinterher (Rückstand {:.1} s). Die GPU ist zu langsam oder ausgelastet.", backlog))))`.

**Tests (pure helpers extracted in `conversation.rs`):**
- `block_starts_at_48k`: blocks at 0, 800, …, 5600 for start 0
- `block_starts_at_44k1`: rounding gives `round(i·735)`

Live behaviour is verified in T1.8.

- [ ] **Step 1:** Write the 2 helper tests. Run `cargo test --manifest-path app/src-tauri/Cargo.toml --lib session::conversation`. Expect FAIL.
- [ ] **Step 2:** Implement `mic_pump.rs` and `conversation.rs`. Run the tests and `cargo build --examples`. Expect PASS and a clean build.
- [ ] **Step 3:** `git add app/src-tauri/src/session/mic_pump.rs app/src-tauri/src/session/conversation.rs && git commit -m "Add conversation engine with mic pump, playback feed and viz ticker"`.

### Task 1.8: Core state machine, commands, Tauri wiring, `headless_talk` (M1 acceptance)

**Depends on:** T1.7.
**Files:** `src/session/state.rs`, `src/session.rs`, `src/commands.rs`, modify `src/lib.rs`, `examples/headless_talk.rs`.

**Interface:**
```rust
// session/state.rs
pub enum Action { Boot, Downloading, Loading, Loaded, Start, Started, Stop, Stopped, Fail, WorkerLost { attempts_left: bool } }
pub fn transition(from: AppState, a: &Action) -> Result<AppState, HaruError>; // invalid → Busy ("Gerade nicht möglich.")
// session.rs
pub type StateSink = Arc<dyn Fn(StateEvent) + Send + Sync>;
pub type DownloadSink = crate::model_fetch::DownloadSink;
pub struct CoreConfig { pub settings_path: PathBuf, pub log_dir: PathBuf, pub backends_dir: Option<PathBuf>, pub models_dir: PathBuf, pub model: ModelSpec, pub hf_base: String }
pub struct Core { /* Mutex<Inner{state, settings, detection, plan, attempt_idx, worker: Option<WorkerProcess>, conversation, muted, reload_pending, boot_info}>, on_state */ }
impl Core {
    pub fn new(cfg: CoreConfig, on_state: StateSink) -> Arc<Core>;
    pub fn status(&self) -> StateEvent;
    pub fn settings(&self) -> Settings;
    pub fn save_settings(&self, s: Settings) -> Result<SaveSettingsResult, HaruError>;
    pub fn backend_options(&self) -> Result<BackendOptions, HaruError>;
    pub async fn boot(self: &Arc<Self>, on_download: DownloadSink) -> Result<BootInfo, HaruError>;
    pub fn start(self: &Arc<Self>, on_viz: VizSink) -> Result<(), HaruError>;  // blocking
    pub fn stop(&self) -> Result<(), HaruError>;
    pub fn set_muted(&self, muted: bool);
    pub fn shutdown(&self); // hang up, close stdin, kill after 2 s
}
```

**Core behaviour:**
- **boot:**
  1. `detect(backends_dir)`.
  2. `Overrides::resolve(env, settings)`, then `choose_plan`.
  3. If `!is_present(model)`, set `Downloading` and run `ensure_model`.
  4. Set `Loading` and `run_attempts`: spawn `WorkerConfig{exe, model, device, voice, log_path = log_dir/"worker-<unix secs>.log"}`, then `wait_ready(READY_TIMEOUT)`.
  5. Set `Ready`, with backend/device in the StateEvent. `warning` = "Läuft auf der CPU – das Gespräch kann zu langsam sein." when the variant or device is cpu.
- A **watcher thread** owns the worker's event receiver. It forwards `Message` to the active conversation's `Sender<WorkerMsg>`. On `Exited` or `Malformed`:
  - if a later attempt exists: set `Loading`, load it, set `Ready`, with warning "Backend {X} abgestürzt – gewechselt auf {Y}.";
  - otherwise set `Error`, with a Worker error whose detail is `stderr_tail(4096)`.
- **start:** if `Conversation::start` fails with `backend_failure`, advance the attempt (reload) and retry. Each remaining attempt is tried at most once.
- Keep the last 5 worker logs.

**commands.rs:** each command from the contract table maps one-to-one onto a Core method:
- `boot` wraps `on_download` as `Arc::new(move |e| { let _ = ch.send(e); })`;
- `start_conversation` wraps `on_viz` the same way;
- blocking calls go through `tauri::async_runtime::spawn_blocking`.

**lib.rs `run`:** in `setup`, build `CoreConfig`:
- `settings_path` = `app.path().app_config_dir()/settings.json`;
- `log_dir` = `app_log_dir()`;
- `backends_dir` = `find_backends_dir(env HARU_BACKENDS_DIR, resource_dir()/backends, debug ? CARGO_MANIFEST_DIR/backends : None)`;
- `models_dir()`, `HARU_Q6K`, `"https://huggingface.co"`.

Then: `on_state` = emit `STATE_EVENT`; `app.manage(core)`; `invoke_handler(tauri::generate_handler![...9 commands])`; `RunEvent::Exit` → `core.shutdown()`.

**headless_talk:** `cargo run --release --example headless_talk -- [--seconds 60] [--language de] [--speak-first true]`.
- Uses `#[tokio::main]`, a temp settings path, `log_dir=build/test`, and the real `models_dir`.
- Prints every StateEvent and the BootInfo.
- Prints each audible non-empty word piece as `[{t:.1}s] {text}`, and `latency {ms} ms` once per second.
- Stops after N seconds.
- Exit 0 if no error occurred and (with `--speak-first`) at least one non-empty piece was heard.

**Tests (`state.rs`):**
- `boot_allowed_from_booting_ready_error`
- `boot_while_talking_is_busy`
- `start_only_from_ready`
- `stop_from_talking_goes_ready`
- `worker_lost_with_attempts_goes_loading`
- `worker_lost_without_attempts_goes_error`

- [ ] **Step 1:** Write the `state.rs` tests. Run `cargo test --manifest-path app/src-tauri/Cargo.toml --lib session::state`. Expect FAIL.
- [ ] **Step 2:** Implement `state.rs`, `session.rs`, `commands.rs` and the `lib.rs` wiring. Run all lib tests. Expect PASS.
- [ ] **Step 3:** Copy the backend: `New-Item -ItemType Directory -Force app\src-tauri\backends\vulkan; Copy-Item $SPEECH app\src-tauri\backends\vulkan\speech.exe`.
- [ ] **Step 4: Live run with headphones.** `cargo run --release --manifest-path app/src-tauri/Cargo.toml --example headless_talk -- --seconds 90 --speak-first true`. Talk to Haru.
- [ ] **Step 5: Fallback check.** `$env:HARU_DEVICE="Vulkan9"` gives a fatal `device`, then the next attempt, and Ready is reached. Then `Remove-Item Env:HARU_DEVICE`.
- [ ] **Step 6:** `git add app/src-tauri/src app/src-tauri/examples/headless_talk.rs && git commit -m "Add session core with backend fallback, Tauri commands and headless conversation example"`.

**Acceptance (M1):**
- Haru greets you, and you hear it.
- Words print in sync with the audio, with no visible lead over the sound.
- The latency line stays below 500 ms for the whole run.
- No lag stop happens.
- The forced bad device falls back to the B580.

---

## Milestone M2 – Visualization UI (runs on the mock client; switches to live in T3.1)

### Task 2.1: Client implementations, formatting, strings

**Depends on:** T1.0.
**Files:** `src/ipc/client.ts`, `tauri-client.ts`, `mock-client.ts`, `src/ui/format.ts` (+test), `src/ui/strings.ts`.

**Interface:**
- `createClient(): HaruClient` returns `MockClient` when `location.search` contains `mock` or `!('__TAURI_INTERNALS__' in window)`. Otherwise it returns `TauriClient`.
- `TauriClient`:
  - `invoke` from `@tauri-apps/api/core`;
  - `new Channel<T>()` with `onmessage`;
  - `listen(STATE_EVENT)` from `@tauri-apps/api/event`;
  - errors rethrown as `HaruError`.
- `MockClient(options: {lang:'de'|'ja', fail?:'boot'})`:
  - `boot`: `downloading` for 6 s with progress from 0 to 5 605 707 520 bytes at about 900 MB/s, then `verifying` 0.5 s, then `done`, then `loading` 1.5 s, then `ready`. BootInfo has backend `vulkan`, device `"Intel(R) Arc(TM) B580 Graphics"`, deviceName `Vulkan1`.
  - `?mock=error` makes `boot` reject with `{code:'noBackend', message:'Keine Backend-Variante gefunden.', detail:'searched: …'}`.
  - `startConversation`: frames at 7.5 Hz from a script, looping with 3 s of `""` frames between loops. VizFrames at 60 Hz carry those frames as `words` and `model.rms = 0.12+0.1·|sin(2π·1.3t)|` while a word is active (0.01 otherwise), with synthetic bands (a peak that moves with time). `mic.rms` is 0.05–0.2 noise during loop gaps.
    - de script: `[" Hal","lo",","," ich"," bin"," Ha","ru","."," ",""," Schön",","," dass"," du"," da"," bist","!"," ",""," Wor","über"," möch","test"," du"," heu","te"," spre","chen","?"," "]`
    - ja script: `["こんに","ちは","。","","","今日は","いい","天気","ですね","。","",""]`
- `format.ts`:
  - `formatBytes(n)`: German, 2 decimals, e.g. `"5,61 GB"`, `"512,00 MB"`
  - `formatRate(bps)`: `"42,1 MB/s"`
  - `formatEta(secs)`: `"45 s"`, `"1 min 45 s"`, `"1 h 2 min"`, `"–"` for non-finite
  - `badgeLabel(backend, device)`: strips `(R)`/`(TM)` (any case), collapses spaces, drops a trailing ` Graphics`; CPU → `"CPU"`

**Tests (`format.test.ts`):**
- `formatBytes(5605707520) === "5,61 GB"`
- `formatBytes(536870912) === "536,87 MB"`
- `formatRate(42100000) === "42,1 MB/s"`
- `formatEta(45) === "45 s"`
- `formatEta(105) === "1 min 45 s"`
- `formatEta(3725) === "1 h 2 min"`
- `formatEta(Infinity) === "–"`
- `badgeLabel('vulkan','Intel(R) Arc(TM) B580 Graphics') === "Vulkan · Intel Arc B580"`
- `badgeLabel('cpu','CPU') === "CPU"`
- `badgeLabel('cuda','NVIDIA GeForce RTX 4090') === "CUDA · NVIDIA GeForce RTX 4090"`

- [ ] **Step 1:** Write the tests. Run `npm --prefix app run test -- src/ui/format.test.ts`. Expect FAIL.
- [ ] **Step 2:** Implement all files. Run `npm --prefix app run test` and `npm --prefix app run typecheck`. Expect PASS.
- [ ] **Step 3:** `git add app/src/ipc app/src/ui/format.ts app/src/ui/format.test.ts app/src/ui/strings.ts && git commit -m "Add Tauri and mock clients, formatting and UI strings"`.

### Task 2.2: WebGL2 layer and fluid simulation

**Parallel with:** T2.3, T2.7.
**Files:** `src/gl/{context,program,fbo,quad}.ts`, `src/fluid/{sim,params}.ts`, `src/fluid/shaders/*.glsl`, `src/render/renderer.ts` (first version), modify `src/main.ts`.

**Interface:**
```ts
// gl/context.ts
export interface GLContext { gl: WebGL2RenderingContext; canvas: HTMLCanvasElement; maxTextureSize: number; timer: unknown | null /* EXT_disjoint_timer_query_webgl2 */ }
export function createGL(canvas: HTMLCanvasElement): GLContext; // throws Error('WEBGL2_UNAVAILABLE' | 'COLOR_BUFFER_FLOAT_UNAVAILABLE'); requires EXT_color_buffer_float
// gl/program.ts
export class Program { constructor(gl: WebGL2RenderingContext, vert: string, frag: string); readonly uniforms: Record<string, WebGLUniformLocation>; bind(): void } // throws with info log
// gl/fbo.ts
export type TexFormat = 'rgba16f' | 'rg16f' | 'r16f' | 'r8';
export class Target { readonly texture: WebGLTexture; readonly fbo: WebGLFramebuffer; readonly width: number; readonly height: number; readonly texel: [number, number]; attach(unit: number): number; dispose(): void }
export class DoubleTarget { read: Target; write: Target; swap(): void; dispose(): void }
export function createTarget(gl, w: number, h: number, fmt: TexFormat, filter: 'linear' | 'nearest'): Target;
export function createDouble(gl, w, h, fmt, filter): DoubleTarget;
export function resizeDouble(gl, t: DoubleTarget, w: number, h: number, copy: Program): DoubleTarget; // blits old content
export function resolution(res: number, aspect: number, max: number): { w: number; h: number }; // min side = res, clamp to max
// gl/quad.ts
export function drawQuad(gl: WebGL2RenderingContext, target: Target | null, viewport?: [number, number]): void; // fullscreen triangle VAO
// fluid/params.ts
export interface FluidParams { densityDissipation: number; velocityDissipation: number; pressure: number; pressureIterations: number; curl: number; restStrength: number }
export const DEFAULT_FLUID: FluidParams = { densityDissipation: 0.6, velocityDissipation: 0.25, pressure: 0.8, pressureIterations: 20, curl: 20, restStrength: 0.9 };
export type RGB = [number, number, number];
export interface Box { x: number; y: number; w: number; h: number } // center + size, normalized canvas coords, y up
// fluid/sim.ts
export class FluidSim {
  constructor(glc: GLContext, simRes: number, dyeRes: number);
  params: FluidParams;
  resize(simRes: number, dyeRes: number): void;
  splat(x: number, y: number, dx: number, dy: number, color: RGB, radius: number): void;   // velocity + dye gaussian
  maskSplatDye(mask: WebGLTexture, box: Box, color: RGB, strength: number): void;
  edgeSplatVelocity(mask: WebGLTexture, maskTexel: [number, number], box: Box, swirl: number, push: number): void;
  beginRestMask(): void; addRestMask(mask: WebGLTexture, box: Box): void;               // MAX-blend into R16F rest target (cleared per frame)
  step(dt: number): void;
  readonly dye: Target; readonly velocity: Target;
}
```

**Shaders (`fluid/shaders/`):**

| File | Responsibility |
|---|---|
| `base.vert` | Fullscreen triangle. Outputs `vUv` and neighbour UVs `vL, vR, vT, vB` from `uTexel`. |
| `copy.frag` | Blit (used by resizeDouble). |
| `advect.frag` | `coord = vUv − dt·vel·uTexel·(1 − uRestStrength·rest)`; `out = texture(uSource, coord) / (1 + uDissipation·dt)`. |
| `divergence.frag` | Central-difference divergence with reflecting borders. |
| `curl.frag` | Scalar curl. |
| `vorticity.frag` | Confinement force from \|curl\| gradient × `uCurl`; `vel += force·dt`; clamp ±1000. |
| `clear.frag` | `pressure *= uValue` (uses `params.pressure`). |
| `pressure.frag` | One Jacobi iteration. |
| `gradient.frag` | `vel −= ∇p`. |
| `splat.frag` | `base + exp(−\|p−point\|²/radius)·color`, aspect-corrected. |
| `mask_splat.frag` | Inside the box: `base + color·strength·mask((uv−boxMin)/boxSize)`. |
| `edge_splat.frag` | Mask gradient `g` from 4 `textureOffset` taps; `vel += (perp(g)·swirl + g·push)` inside the box. |
| `rest_mask.frag` | Writes the mask sample inside the box (blended with `gl.MAX`). |
| `display.frag` | Temporary: `dye` → Reinhard → gamma 2.2 (replaced by post in T2.5). |

**`FluidSim.step(dt)` order:** curl → vorticity → divergence → clear → pressure × N → gradient → advect velocity (rest) → advect dye (rest). The caller clamps `dt` to [0, 1/30].

**Renderer v1:**
- `new Renderer(canvas)`, `start()` with a rAF loop.
- Canvas resized to clientSize × min(devicePixelRatio, 2).
- Sim at 256/1440 (high).
- `?debug` enables pointer drag splats (dx, dy from movement ×1000, random hue).
- `display.frag` to screen.

- [ ] **Step 1:** Implement. Run `npm --prefix app run typecheck`. Expect PASS.
- [ ] **Step 2:** Run `npm --prefix app run dev` and open `http://localhost:5173/?mock&debug` in Edge. Dragging should produce swirling coloured ink that dissipates over about 3–5 s, with no GL errors in the console.
- [ ] **Step 3:** `git add app/src/gl app/src/fluid app/src/render/renderer.ts app/src/main.ts && git commit -m "Add WebGL2 helpers and stable-fluids simulation"`.

**Acceptance:**
- Screenshot `docs/screenshots/m2-fluid.png`.
- 1920×1080 at high reaches ≥ 58 fps in the `?debug` fps readout. The readout is a `<div>` that T2.6 extends.

### Task 2.3: Pure UI logic (TDD): WordAssembler, WordPath, lifecycle, QualityController, features, coupling

**Parallel with:** T2.2, T2.7.
**Files (each with a `.test.ts` sibling):** `src/words/assembler.ts`, `words/path.ts`, `words/lifecycle.ts`, `src/render/quality.ts`, `src/audio/features.ts`, `src/audio/coupling.ts` (math exports only here; the `AudioCoupling` class arrives in T2.6).

**WordAssembler:**
```ts
export type WordEvent = { type: 'wordStart' | 'wordAppend' | 'wordEnd'; id: number; text: string; seq: number };
export class WordAssembler { push(p: WordPiece): WordEvent[]; flush(): WordEvent[]; reset(): void }
```
**Rules:**
- Ids start at 1.
- A piece is processed char by char. For each word touched by the frame, emit at most one `wordStart` (a new word, with its text after this frame) or one `wordAppend` (full text), then `wordEnd` if it ended.
- Whitespace ends the current word. A piece of only whitespace (`" "`) ends the word and starts nothing.
- Punctuation `. , ! ? ; : … 。 、 ！ ？ ， ； ：` is appended to the current word and then ends it. Standalone punctuation with no current word is dropped.
- CJK = Han `\u3400-\u4DBF\u4E00-\u9FFF`, Hiragana `\u3040-\u309F`, Katakana `\u30A0-\u30FF\uFF66-\uFF9F`. Hangul is a spaced script.
- A change between CJK and non-CJK script ends the word.
- A CJK word ends at punctuation, at 8 chars (the rest starts a new word), or after ≥ 2 consecutive `""` frames.
- A non-CJK word ends after ≥ 6 consecutive `""` frames.
- `flush()` ends the open word.

| Test | Frames | Expected events `(type:id:text)` |
|---|---|---|
| `german_subwords` | `[" Hal","lo"," Wel","t","."]` | S1 Hal · A1 Hallo · E1 Hallo, S2 Wel · A2 Welt · A2 Welt. , E2 Welt. |
| `multiple_words_in_one_frame` | `[" ich bin"," da"]` | S1 ich, E1 ich, S2 bin · E2 bin, S3 da |
| `silence_ends_word` | `[" Ja"," "," "]` | S1 Ja · E1 Ja · (none) |
| `empty_frames_do_not_split_latin` | `[" Ver","","","stehe"]` | S1 Ver · · · A1 Verstehe |
| `latin_idle_timeout` | `[" Gut",""×6]` | E1 Gut on the 6th empty frame |
| `punctuation_mid_piece` | `[" Ja,","und"," so"]` | S1 Ja,, E1 Ja, · S2 und · E2 und, S3 so |
| `standalone_punctuation_dropped` | `[" ."]` | (none) |
| `cjk_punctuation` | `["こんに","ちは","。"]` | S1 こんに · A1 こんにちは · A1 こんにちは。, E1 |
| `cjk_two_empty_frames` | `["今日","",""]` | S1 今日 · · E1 今日 |
| `cjk_eight_char_cap` | `["一二三四五六七八九十"]` | S1 一二三四五六七八, E1, S2 九十 |
| `script_change_ends_word` | `["日本","Tokyo"]` | S1 日本 · E1 日本, S2 Tokyo |
| `korean_uses_spaces` | `[" 안녕","하세요"," 반가워요"]` | S1 안녕 · A1 안녕하세요 · E1, S2 반가워요 |
| `flush_ends_open_word` | `[" Hal"]` then `flush()` | E1 Hal |

In the table, `·` separates frames.

**WordPath:**
```ts
export interface WordPathOptions { ax: number; ay: number; fx: number; fy: number; phase: number; margin: number; candidates: number; step: number; pad: number }
// defaults: ax 0.32, ay 0.28, fx 0.13, fy 0.17 (rad/s), phase π/2, margin 0.04, candidates 12, step 0.4 s, pad 0.02
export class WordPath { constructor(o?: Partial<WordPathOptions>); positionAt(t: number): { x: number; y: number };
  place(t: number, w: number, h: number): Box; register(box: Box, untilSec: number): void; }
```
- `positionAt(t) = (0.5 + ax·sin(fx·t + phase), 0.5 + ay·sin(fy·t))`.
- `place` tries `t, t+step, …` (`candidates` positions). It clamps each box inside `[margin, 1−margin]` and returns the first whose padded box overlaps no box registered with `until > t`. Otherwise it returns the candidate with the minimal total overlap area.

**Tests:**
- `deterministic_position`: `positionAt(0)` = (0.82, 0.5)
- `stays_within_bounds`: t from 0 to 1000 in steps of 0.37, w=0.6, h=0.12; the box lies within [0,1]
- `avoids_recent_boxes`: register `place(10,0.3,0.1)` until 20; the next `place(10,0.3,0.1)` does not intersect it
- `falls_back_to_min_overlap`: register a full-canvas box; returns a box (no throw)
- `expired_boxes_ignored`: until 5, query at t=10, returns the `t` candidate

**lifecycle:**
```ts
export const INK_MS = 250; export const RELEASE_MS = 400;
export function holdMs(len: number): number; // clamp(600 + 60·len, 600, 1200)
export interface PhaseState { phase: 'ink' | 'hold' | 'release' | 'done'; inkStrength: number; holdStrength: number; rest: boolean; releaseT: number }
export function wordPhase(nowMs: number, startMs: number, endedAtMs: number | null, len: number): PhaseState;
```
- **ink:** for `age < 250`, `inkStrength = 1`.
- **hold:** after ink until `releaseStart = max(start+250+holdMs(len), (endedAt ?? ∞) + 300)`. `holdStrength = 0.15`, `rest = true`.
- **release:** for 400 ms. `releaseT` goes 0→1, `rest = false`.
- **done:** after that.

**Tests:**
- `ink_for_first_250ms`
- `hold_duration_scales_with_length`: `holdMs(0)=600`, `holdMs(3)=780`, `holdMs(20)=1200`
- `no_release_before_word_end`: endedAt null at +5000 is still hold
- `release_after_hold_and_end`
- `done_after_release`

**QualityController:**
```ts
export interface QualityParams { sim: number; dye: number; pressureIterations: number; bloomLevels: number; raySamples: number; rayScale: number }
export const QUALITY_TABLE: Record<QualityLevel, QualityParams> = {
  low: { sim: 128, dye: 512, pressureIterations: 12, bloomLevels: 3, raySamples: 24, rayScale: 0.25 },
  medium: { sim: 192, dye: 1024, pressureIterations: 16, bloomLevels: 4, raySamples: 32, rayScale: 0.25 },
  high: { sim: 256, dye: 1440, pressureIterations: 20, bloomLevels: 5, raySamples: 48, rayScale: 0.5 },
  ultra: { sim: 384, dye: 2048, pressureIterations: 24, bloomLevels: 6, raySamples: 64, rayScale: 0.5 } };
export class QualityController { constructor(mode: Quality, initial?: QualityLevel /* 'high' */); readonly level: QualityLevel;
  setMode(m: Quality): void; update(frameMs: number, nowMs: number): QualityLevel | null; }
```
- EMA α = 0.1, seeded with the first sample.
- EMA > 20 continuously for ≥ 2000 ms → step down.
- EMA < 12 continuously for ≥ 5000 ms → step up.
- Timers reset on any change and whenever the condition breaks.
- A fixed mode never changes the level and sets it to the mode.

**Tests:**
- `fixed_mode_never_changes`
- `downgrades_after_2s_over_20ms`: constant 25 ms every 25 ms; `null` before t=2000; `'medium'` at ≥ 2000
- `no_downgrade_on_brief_spike`: 1 s at 30 ms, then 10 ms
- `upgrades_after_5s_under_12ms`
- `clamps_at_bounds`
- `timers_reset_after_change`: the second downgrade comes no earlier than 4000

**features / coupling math:**
- `spectralCentroid(bands) = Σ i·b_i / Σ b_i / (n−1)`, and 0.5 when the sum is 0.
- `Envelope(attackMs=30, releaseMs=200).step(x, dtMs)`.
- `vorticityFor(rms) = 20 + 40·clamp(rms/0.25, 0, 1)`.
- `haruHue(c) = (42 − 82·c + 360) % 360`.
- `micHue(c) = 185 + 40·c`.
- `hsvToRgb(h, s, v): RGB`.

**Tests:**
- `centroid_flat_is_half`
- `centroid_low_band_is_zero`
- `centroid_silence_is_half`
- `envelope_attack_faster_than_release`: after 30 ms of input 1, value > 0.6; after 30 ms of input 0, value > 0.8 of its previous value
- `vorticity_base_and_max`: 0 → 20, 0.125 → 40, 0.25 → 60, 1 → 60
- `haru_hue_range`: 0 → 42, 0.5 → 1, 1 → 320
- `mic_hue_range`: 0 → 185, 1 → 225
- `hsv_primaries`: (0,1,1) → [1,0,0]; (120,1,1) → [0,1,0]

- [ ] **Step 1:** Write all tests. Run `npm --prefix app run test`. Expect FAIL.
- [ ] **Step 2:** Implement. Run `npm --prefix app run test` and `npm --prefix app run typecheck`. Expect PASS.
- [ ] **Step 3:** `git add app/src/words/assembler.* app/src/words/path.* app/src/words/lifecycle.* app/src/render/quality.* app/src/audio && git commit -m "Add word assembly, word path, lifecycle, quality control and audio mapping"`.

### Task 2.4: GlyphRasterizer and WordInjector

**Depends on:** T2.2, T2.3.
**Files:** `src/words/glyphs.ts`, `src/words/injector.ts`, modify `src/render/renderer.ts`.

**Interface:**
```ts
export interface GlyphBitmap { texture: WebGLTexture; width: number; height: number; aspect: number; texel: [number, number]; text: string }
export class GlyphRasterizer { constructor(gl: WebGL2RenderingContext, fontPx?: number /* 96 */); ready(): Promise<void>; rasterize(text: string): GlyphBitmap; dispose(): void }
export class WordInjector { constructor(sim: FluidSim, glyphs: GlyphRasterizer, path: WordPath);
  handle(ev: WordEvent, nowMs: number): void; update(nowMs: number, color: RGB): void; activeCenter(): { x: number; y: number } | null; }
```
**GlyphRasterizer:**
- `ready()` awaits `document.fonts.load('600 96px "Inter Variable"')`.
- Font stack: `600 {px}px "Inter Variable", "Segoe UI", "Yu Gothic UI", "Meiryo", "Malgun Gothic", "Microsoft YaHei", "Noto Sans", "PingFang SC", "Hiragino Sans", sans-serif`.
- Draws on an OffscreenCanvas sized `ceil(measure.width + 2·pad) × ceil(1.4·px + 2·pad)` with `pad = 0.25·px`. White text on black, then `shadowBlur = 0.08·px` for soft edges.
- Uploads the R channel of `getImageData` as R8 (`texImage2D(R8, RED, UNSIGNED_BYTE)`, `UNPACK_ALIGNMENT 1`, LINEAR).
- LRU cache of 64 entries keyed by text. Evicted textures are deleted.

**WordInjector:**
- Word box height `h = 0.11` (canvas-normalized). Width `w = h·aspect·(canvasH/canvasW)`.
- `wordStart`: `path.place(nowSec, w, h)` and `path.register(box, nowSec + 3)`.
- `wordAppend`: re-rasterize, keep the left edge, grow to the right, shift left if it overflows the margin.
- `wordEnd` records `endedAt`.
- `update` uses `wordPhase`:
  - ink: `maskSplatDye(color×2.5, inkStrength)`;
  - hold: `maskSplatDye(color, 0.15)` and `addRestMask`;
  - release: `edgeSplatVelocity(swirl = 900·(1−releaseT), push = 250·(1−releaseT))`;
  - done: remove.
- `beginRestMask()` is called each frame before the adds.

- [ ] **Step 1:** Implement. In `renderer.ts`, under `?mock`, feed MockClient frames through `WordAssembler` → `WordInjector` with a fixed gold colour.
- [ ] **Step 2:** Open `/?mock&debug` and run `npm --prefix app run typecheck`. Words appear as gold ink, stay readable through hold, then dissolve into swirls. Partial words grow ("Hal" → "Hallo"). `/?mock=ja&debug` renders Japanese glyphs.
- [ ] **Step 3:** `git add app/src/words/glyphs.ts app/src/words/injector.ts app/src/render/renderer.ts && git commit -m "Add glyph rasterizer and word ink injector"`.

**Acceptance:** Screenshots `docs/screenshots/m2-word-hold.png` (a word readable mid-hold) and `m2-word-release.png` (the word dissolving).

### Task 2.5: Post-processing pipeline

**Depends on:** T2.2. **Parallel with:** T2.4.
**Files:** `src/post/pipeline.ts`, `src/post/shaders/{bloom_prefilter,kawase_down,kawase_up,godrays,composite}.frag`, modify `src/render/renderer.ts` (replace `display.frag`).

**Interface:**
```ts
export interface PostUniforms { timeSec: number; rms: number; lightPos: { x: number; y: number } }
export class PostFX { constructor(glc: GLContext, q: QualityParams); resize(w: number, h: number, q: QualityParams): void;
  render(dye: Target, u: PostUniforms): void; }  // draws to the default framebuffer
```
| Shader | Responsibility and constants |
|---|---|
| `bloom_prefilter.frag` | Brightness `max(r,g,b)`. Soft knee with threshold 0.6 and knee 0.3: `rq = clamp(br−t+k, 0, 2k)²/(4k+1e-4)`; weight `max(rq, br−t)/max(br, 1e-4)`. Output at ½ dye res. |
| `kawase_down.frag` | Dual-Kawase downsample (centre ×4 + 4 diagonal taps at ±texel, /8). Chain of `bloomLevels` half-res targets. |
| `kawase_up.frag` | Dual-Kawase upsample (8 taps), additively blended up the chain. |
| `godrays.frag` | Radial blur of bloom level 0 toward `lightPos`: `raySamples` taps, density 0.8, decay 0.96, weight 0.3, exposure 0.25. Rendered at `rayScale` × screen. |
| `composite.frag` | Glass shading: luminance gradient of dye → `n = normalize(vec3(−dx·6, −dy·6, 1))`; light `normalize(vec3(−0.4, 0.6, 0.7))`; diffuse `0.85+0.15·ndotl`; Blinn-Phong spec power 48 × 0.6. Then + bloom×0.8 + rays. Chromatic aberration offset `(0.0015 + 0.004·rms)·(uv−0.5)` for R and B. Exposure 1.0 → ACES (Narkowicz) → vignette `mix(1, smoothstep(0.85, 0.25, length((uv−0.5)·vec2(aspect,1))), 0.35)` → film grain (hash noise, amplitude 0.035, seeded by time) → gamma 1/2.2 → dither ±0.5/255. |

- [ ] **Step 1:** Implement and wire it in. Use `lightPos` = active word centre, else (0.5, 0.6).
- [ ] **Step 2:** Open `/?mock&debug` and run `npm --prefix app run typecheck`. Bright ink blooms with a soft glow and faint rays, the surface reads as glossy liquid, there is no banding in dark gradients, and the frame is still ≥ 58 fps at high.
- [ ] **Step 3:** `git add app/src/post app/src/render/renderer.ts && git commit -m "Add bloom, god rays, liquid-glass shading and filmic composite"`.

### Task 2.6: Audio coupling, quality switching, debug overlay

**Depends on:** T2.3, T2.4, T2.5.
**Files:** modify `src/audio/coupling.ts` (add the `AudioCoupling` class), `src/audio/features.ts` (add `FeatureSmoother`), `src/render/renderer.ts`.

**Interface:**
```ts
export interface SmoothedAudio { modelRms: number; modelCentroid: number; micRms: number; micCentroid: number }
export class FeatureSmoother { update(f: VizFrame | null, dtMs: number): SmoothedAudio }  // Envelope(30,200) per rms, centroid EMA α 0.2
export type CouplingMode = 'boot' | 'idle' | 'talking';
export class AudioCoupling { constructor(sim: FluidSim, rng?: () => number);
  update(a: SmoothedAudio, dtMs: number, nowMs: number, wordCenter: { x: number; y: number } | null, mode: CouplingMode, downloadBps?: number): void;
  haruColor(a: SmoothedAudio): RGB; }  // hsvToRgb(haruHue(c), 0.85, 1)·(1.5 + 2·rms)
// Renderer additions
setMode(m: CouplingMode): void; pushViz(f: VizFrame): void; setDownloadRate(bps: number): void; setQualityMode(q: Quality): void;
onWordEvent(cb: (e: WordEvent) => void): void; resetWords(): void; // flush assembler (on conversation end)
```
**Coupling constants:**
- `sim.params.curl = vorticityFor(modelRms)`.
- When `modelRms > 0.03` and a word centre exists: 2 splats per frame on the word box perimeter, tangential velocity `200·modelRms`, radius 0.0025, Haru colour × 0.4.
- When `micRms > 0.02`: splats at `20·clamp(micRms/0.2)` per second from `y=0.02`, x uniform in [0.1, 0.9], velocity `(0, 300 + 300·micRms)`, colour `hsvToRgb(micHue, 0.8, 0.9)·(0.6 + 2·micRms)`.
- Idle (both env < 0.01 for > 1.5 s, or mode `idle`): one splat every 1.2 s, random position and direction, magnitude 80, v = 0.25, alternating Haru and mic palettes.
- Boot: the idle rate is multiplied by `1 + clamp(downloadBps/50e6, 0, 3)` and the magnitude by the same factor.

**Renderer loop:** dt → smoother → coupling → injector.update → sim.step → post.render → `quality.update(frameMs)`. On a level change: `sim.resize` and `post.resize` from `QUALITY_TABLE`, and `sim.params.pressureIterations` from the table. `frameMs` is the GPU time from `EXT_disjoint_timer_query_webgl2` when available, otherwise the rAF delta. With rAF delta on a 60 Hz display, auto never raises above its start level `high`. This is accepted.

**Debug overlay (`?debug`):** fps, frameMs EMA, quality level, `latencyMs`, active words.

- [ ] **Step 1:** Implement. Run `npm --prefix app run test` and `npm --prefix app run typecheck`. Expect PASS.
- [ ] **Step 2:** Open `/?mock&debug`. Loud mock speech visibly increases swirling. Colours shift between gold and magenta. Cyan splats rise from the bottom during mic gaps. Slow ambient splats appear when idle. Forcing `?quality=low` and `?quality=ultra` (debug-only query) changes the resolution without losing ink.
- [ ] **Step 3:** `git add app/src/audio app/src/render/renderer.ts && git commit -m "Couple audio features to the fluid and add adaptive quality"`.

### Task 2.7: Overlays, HUD, settings, subtitles, toast, keys

**Depends on:** T2.1. **Parallel with:** T2.2–T2.6.
**Files:** `src/ui/{boot,hint,start,hud,settings,subtitles,toast,keys}.ts`, `src/styles.css`.

**Interface:**
```ts
export class BootScreen { constructor(root: HTMLElement); show(): void; hide(): void; setStatus(text: string): void; onDownload(e: DownloadEvent): void }
export class HeadphoneHint { constructor(root: HTMLElement); show(): Promise<void> } // resolves on "Verstanden"
export class StartButton { constructor(root: HTMLElement); show(onStart: () => void): void; setBusy(b: boolean): void; hide(): void }
export type HudAction = 'mute' | 'settings' | 'fullscreen' | 'stop';
export class Hud { constructor(root: HTMLElement); show(): void; hide(): void; setBadge(label: string): void; setLatency(ms: number): void;
  setMicLevel(rms: number): void; setMuted(m: boolean): void; onAction(cb: (a: HudAction) => void): void }
export class SettingsPanel { constructor(root: HTMLElement); open(s: Settings, a: AudioDevices, b: BackendOptions): void; close(): void;
  readonly isOpen: boolean; onSave(cb: (s: Settings) => Promise<void>): void }
export class Subtitles { constructor(root: HTMLElement); setEnabled(b: boolean): void; handle(e: WordEvent): void; clear(): void }
export class ErrorToast { constructor(root: HTMLElement); show(e: HaruError, onRetry?: () => void): void; showWarning(text: string): void; hide(): void }
export function bindKeys(h: { mute(): void; settings(): void; fullscreen(): void; escape(): void }): () => void; // ignores events from input/textarea/select
```
**Behaviour:**
- **BootScreen:**
  - centered "Haru" wordmark;
  - status line ("Suche Backend…", "Lade Modell…");
  - a glowing progress line (2 px, gold→magenta gradient, box-shadow glow, width = %);
  - `"{formatBytes(d)} / {formatBytes(t)} · {formatRate(bps)} · noch {formatEta((t−d)/bps)}"`;
  - verifying shows "Prüfe Modell… {pct} %".
- **Hint:** "Bitte Kopfhörer verwenden – sonst hört Haru sich selbst."
- **HUD:**
  - bottom-left badge;
  - latency "{n} ms", with a `warn` class above 500;
  - an 8-segment mic meter (`sqrt(rms)`);
  - buttons Mute [M], Einstellungen [S], Vollbild [F], Auflegen;
  - fades to 20 % opacity after 3 s without pointer movement.
- **Settings** fields map one-to-one onto `Settings`:
  - language select (9);
  - name; persona (textarea);
  - gender (–/weiblich/männlich); backchannel (–/niedrig/mittel/hoch);
  - speakFirst;
  - temperature slider 0.1–1.5 step 0.05;
  - topK 1–200;
  - voicePath text;
  - input/output selects (null = "Standard");
  - backend select (auto + variants); device select (auto + devices of the chosen variant);
  - quality; subtitles;
  - lagLimitSecs 0.5–5.
- **Subtitles:** bottom-centre, the last 2 lines. Words are added on start/append and fade 4 s after the last event.
- **Toast:** top-centre, message plus a collapsible `detail` (`<pre>`) and a "Erneut versuchen" button when `onRetry` is given. A warning has amber styling and auto-hides after 6 s.

- [ ] **Step 1:** Implement. Run `npm --prefix app run typecheck`. Expect PASS.
- [ ] **Step 2:** Mount each overlay temporarily from `?ui-demo` in main.ts (removed in T2.8) and check the layout at 1280×800 and 1920×1080.
- [ ] **Step 3:** `git add app/src/ui app/src/styles.css && git commit -m "Add boot screen, HUD, settings panel, subtitles and toasts"`.

### Task 2.8: App controller and M2 acceptance (mock)

**Depends on:** T2.6, T2.7.
**Files:** `src/app.ts`, modify `src/main.ts` (import `@fontsource-variable/inter`, `styles.css`, `createClient()`, `new App(client, canvas, root).start()`).

**App flow:**
1. `onState` subscription. `boot(onDownload)` with renderer mode `boot` and `setDownloadRate(bps)`.
2. On `ready`: if `!settings.headphoneHintSeen`, run `HeadphoneHint.show()` and then `saveSettings({...s, headphoneHintSeen:true})`. Then show StartButton and set renderer mode `idle`.
3. Start → `startConversation(f => { renderer.pushViz(f); hud.setLatency(f.latencyMs); hud.setMicLevel(f.mic.rms); })`, HUD visible, mode `talking`.
4. `stop` → `stopConversation()` and `renderer.resetWords()`.
5. State `error` → toast with Retry → `boot` again.
6. `warning` → `showWarning`.
7. Keys: M toggles `setMuted`, S toggles settings, F toggles fullscreen (Tauri `getCurrentWindow().setFullscreen(!(await isFullscreen()))`; in the browser, `document.documentElement.requestFullscreen()`), Escape closes settings.
8. Saving settings: `saveSettings`. If `reloadRequired`, call `boot()`. Apply `quality`/`subtitles` immediately.

- [ ] **Step 1:** Implement and remove the `?ui-demo` hooks. Run `npm --prefix app run typecheck` and `npm --prefix app run test`. Expect PASS.
- [ ] **Step 2:** Capture screenshots via `/?mock` in Edge at 1920×1080 into `docs/screenshots/`:
  - `m2-boot.png`: download line with bytes, rate and ETA, fluid reacting
  - `m2-hint.png`
  - `m2-talking.png`: HUD badge "Vulkan · Intel Arc B580", latency, mic meter, a readable word
  - `m2-settings.png`
  - `m2-error.png`: `/?mock=error`, toast with Retry
  - `m2-cjk.png`: `/?mock=ja`
- [ ] **Step 3:** `git add app/src/app.ts app/src/main.ts docs/screenshots && git commit -m "Wire the app controller and capture mock screenshots"`.

**Acceptance (M2):**
- All screenshots match the descriptions.
- ≥ 58 fps at high, 1920×1080, in `?mock&debug`.
- No console errors through a full mock loop: boot → hint → start → 60 s talk → stop → start.

---

## Milestone M3 – Integration

### Task 3.1: Live integration and real conversation test

**Depends on:** T1.8, T2.8.
**Files:** fixes as needed (keep each fix in its owning file); `docs/screenshots/m3-*.png`; add a "Measurements with UI" section to `docs/m0-feasibility.md`.

- [ ] **Step 1:** Make sure `app/src-tauri/backends/vulkan/speech.exe` exists (T1.8 step 3, or T3.2). Run `npm --prefix app run tauri dev`.
- [ ] **Step 2:** First-start path. Use a fresh profile: delete `%APPDATA%\dev.haru.fluidlight\settings.json`. Expect booting → loading (no download, the model is present) → headphone hint → Start.
- [ ] **Step 3:** Hold a real 3-minute conversation with headphones, in German, with `speakFirst` on. Check:
  - words appear as ink in sync with the voice;
  - HUD latency stays < 500 ms;
  - mute [M] silences the input, Haru keeps running, and the cyan splats stop;
  - stop/start works twice in a row.
- [ ] **Step 4:** Repeat a 2-minute conversation at quality `low`, `high` and `ultra`. Read compute pressure from the HUD latency and the worker log. Record the median latency per level in `docs/m0-feasibility.md`. If lag warnings appear at `high`, note the highest level that stays clean.
- [ ] **Step 5:** Run the error paths:
  - `$env:HARU_BACKENDS_DIR="C:\nope"` gives the noBackend toast (start from a shell with the variable set; dev path disabled by setting the variable to a dir with no variants);
  - disabling the microphone in Windows settings gives the noMicrophone toast;
  - `lagLimitSecs = 0.5` while another GPU load runs (e.g. FurMark) stops the conversation with the lagLimit toast. Optional, if a load tool is available.
- [ ] **Step 6:** Capture screenshots `m3-boot-live.png`, `m3-talking-live.png`, `m3-settings-live.png`.
- [ ] **Step 7:** `git add -A app docs && git commit -m "Integrate the UI with the live core and record conversation measurements"`.

**Acceptance:** The 3-minute live conversation runs without a lag stop. Ink and voice are visibly synchronized. All error toasts appear with German messages and working Retry.

### Task 3.2: Backend build scripts

**Can run:** any time after T0.1.
**Files:** `scripts/build-backends.ps1`, `scripts/build-backends.sh`.

**PowerShell:** `param([string[]]$Variants = @('vulkan'), [string]$Config = 'Release', [switch]$Clean)`, with `$ErrorActionPreference='Stop'` and an exit-code check after every native call.
1. `git submodule update --init --recursive extern/speech.cpp`.
2. For each variant `v`:
   - `cmake -S extern/speech.cpp -B build/speech-$v -A x64 -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded -DGGML_NATIVE=OFF -DGGML_OPENMP=OFF -DBUILD_SHARED_LIBS=OFF -DGGML_BUILD_TESTS=OFF -DGGML_BUILD_EXAMPLES=OFF`, plus `-DGGML_VULKAN=ON` (vulkan), `-DGGML_CUDA=ON "-DCMAKE_CUDA_ARCHITECTURES=75;80;86;89"` (cuda), or nothing (cpu).
   - `cmake --build build/speech-$v --config $Config --target speech-cli -j`.
   - Copy `build/speech-$v/$Config/speech.exe` (F19) to `app/src-tauri/backends/$v/`.
   - For cuda, also copy `cudart64_*.dll`, `cublas64_*.dll` and `cublasLt64_*.dll` from `$env:CUDA_PATH\bin`. Fail if `CUDA_PATH` is unset.
   - Run `backends/$v/speech.exe --version` and `devices --json`, and print both.
3. `-Clean` removes `build/speech-$v` first.

**POSIX (`#!/usr/bin/env bash`, `set -euo pipefail`):**
- Variants come from args. The default is `metal` on Darwin and `vulkan cpu` on Linux.
- Common flags: `-DCMAKE_BUILD_TYPE=Release -DGGML_NATIVE=OFF -DBUILD_SHARED_LIBS=OFF -DGGML_BUILD_TESTS=OFF -DGGML_BUILD_EXAMPLES=OFF`.
- Linux adds `-DGGML_OPENMP=OFF`. Metal adds `-DGGML_METAL_EMBED_LIBRARY=ON` (F19). vulkan adds `-DGGML_VULKAN=ON`. cuda adds `-DGGML_CUDA=ON` and copies `libcudart.so.12`, `libcublas.so.12`, `libcublasLt.so.12` from `$CUDA_PATH/lib64`.
- Build with `--target speech-cli -j "$(nproc || sysctl -n hw.ncpu)"` and copy `build/speech-$v/speech` to `app/src-tauri/backends/$v/speech`.

- [ ] **Step 1:** Write both scripts.
- [ ] **Step 2:** Run `pwsh scripts/build-backends.ps1 -Variants vulkan`. Expect `app/src-tauri/backends/vulkan/speech.exe`, with `--version` printing `speech.cpp 0.11.1`.
- [ ] **Step 3:** Run `bash -n scripts/build-backends.sh` (syntax check; the Linux and macOS builds are exercised only in CI or by users).
- [ ] **Step 4:** `git add scripts/build-backends.ps1 scripts/build-backends.sh && git commit -m "Add scripts that build speech backend variants into the app"`.

### Task 3.3: Packaging (NSIS)

**Depends on:** T3.1, T3.2.
**Files:** modify `app/src-tauri/tauri.conf.json` if needed (resources `["backends/**/*"]`, targets `["nsis"]`, icons). The paths from T1.8 already use `resource_dir()/backends`.

- [ ] **Step 1:** Run `npm --prefix app run tauri build -- --bundles nsis`. Expect `app/src-tauri/target/release/bundle/nsis/Haru_0.1.0_x64-setup.exe`.
- [ ] **Step 2:** Install it, start Haru from the Start menu, and hold a 1-minute conversation. Then check that `%LOCALAPPDATA%\Programs\Haru\backends\vulkan\speech.exe` (or the install dir shown by the installer) exists, that no console window appears, and that the worker log is in `%LOCALAPPDATA%\dev.haru.fluidlight\logs`.
- [ ] **Step 3:** Uninstall. Check that the model in the speech.cpp cache stays untouched.
- [ ] **Step 4:** `git add app/src-tauri/tauri.conf.json && git commit -m "Configure NSIS bundle with backend resources"`. If nothing changed, skip.

### Task 3.4: CI workflow

**Depends on:** T3.3.
**Files:** `.github/workflows/build.yml`.

**Jobs** (all on `windows-2025`; triggers: `push`, `pull_request`, `workflow_dispatch`):
1. **`test`:**
   - checkout@v6 (no submodules), setup-node@v4 (node 24, npm cache), dtolnay/rust-toolchain@stable, Swatinem/rust-cache@v2 (`workspaces: app/src-tauri`);
   - `npm --prefix app ci`;
   - `npm --prefix app run test`;
   - `npm --prefix app run build`;
   - `cargo test --manifest-path app/src-tauri/Cargo.toml --lib`.
2. **`backend-vulkan`:**
   - checkout@v6 with `submodules: recursive`;
   - install Vulkan SDK `1.4.357.0`, exactly as upstream does (`extern/speech.cpp/.github/workflows/build.yml:94-99`);
   - `pwsh scripts/build-backends.ps1 -Variants vulkan`;
   - upload-artifact@v4 `backend-vulkan` from `app/src-tauri/backends/vulkan`.
3. **`backend-cuda`:**
   - checkout recursive;
   - `Jimver/cuda-toolkit@v0.2.24` with `cuda: '12.6.3'`, `method: network`, `sub-packages: '["nvcc","cudart","cublas","cublas_dev","thrust","visual_studio_integration"]'`;
   - `pwsh scripts/build-backends.ps1 -Variants cuda`;
   - upload `backend-cuda`.
   - This build is not tested (no GPU on the runners). The runtime fallback chain covers a broken CUDA build.
4. **`bundle`** (needs: test, backend-vulkan, backend-cuda):
   - checkout;
   - download-artifact@v4 for both into `app/src-tauri/backends/{vulkan,cuda}`;
   - node + rust setup;
   - `npm --prefix app ci`;
   - `npm --prefix app run tauri build -- --bundles nsis`;
   - upload `app/src-tauri/target/release/bundle/nsis/*.exe` as `haru-windows-x64-nsis`.

- [ ] **Step 1:** Write the workflow. Lint it locally with `npx --yes action-validator .github/workflows/build.yml` if available. Otherwise check the YAML parses: `node -e "require('fs').readFileSync('.github/workflows/build.yml','utf8')"` plus a careful review.
- [ ] **Step 2:** `git add .github/workflows/build.yml && git commit -m "Add CI that tests, builds Vulkan and CUDA backends and bundles the installer"`.
- [ ] **Step 3:** After the user pushes, all four jobs must be green. Fix the CUDA version or action pin if Jimver rejects it, and record the final values in the workflow.

### Task 3.5: README

**Files:** `README.md`.

**Sections, in this order:**
1. What Haru is (one paragraph plus `docs/screenshots/m3-talking-live.png`).
2. Requirements: Windows 11, a Vulkan GPU with ≥ 8 GB (or an NVIDIA GPU with CUDA), headphones, about 6 GB of disk for the model.
3. Install from the CI installer.
4. First start: model download of 5.6 GB to `%LOCALAPPDATA%\speech.cpp\models\…`, shared with the speech CLI; `SPEECH_MODEL_DIR`.
5. Usage and keys (M/S/F), settings table (mirrors spec §4.6), quality levels.
6. Backend selection: the auto order, `HARU_BACKEND`, `HARU_DEVICE`, `HARU_BACKENDS_DIR`, the badge.
7. Build from source:
   - submodule;
   - `scripts/build-backends.ps1`;
   - `npm --prefix app install`;
   - `npm --prefix app run tauri dev`/`build`;
   - prerequisites (VS 2022+, CMake ≥ 3.20, Vulkan SDK, Rust, Node 24).
8. Troubleshooting: lag stop (smaller quality, close GPU apps), no backend, worker logs location, echo without headphones.
9. Licenses: app; speech.cpp MIT; Haru-4B Apache-2.0 (research base model, `docs/models/haru.md:10-13`); Inter OFL.

- [ ] **Step 1:** Write the README.
- [ ] **Step 2:** `git add README.md && git commit -m "Add README"`.

---

## Self-Review

**Spec coverage:**

| Spec section | Where covered |
|---|---|
| §1 goals, platform, auto backend | T1.3, T1.8, T3.2–T3.4 |
| §2 model facts and options | F7–F14, T1.1 `dialogue_options` |
| §2.3 model file | `HARU_Q6K` in T1.1 |
| §3 architecture, process isolation | T0.3, T1.8 |
| §4.1 model_fetch (layout, `SPEECH_MODEL_DIR`, Range/206/200, incremental SHA, disk check, atomic rename, events) | T1.1, T1.2 |
| §4.2 backend (dirs order, CUDA probe, devices, `choose_plan`, start sequence, badge) | T1.1, T1.3, T1.8, T2.1 `badgeLabel` |
| §4.3 worker (CLI, `CREATE_NO_WINDOW`, stderr log, protocol) | T0.2, T0.3 |
| §4.4 audio (capture any format → mono f32, 2 s ring, 20 ms chunks at mic rate, mute = zeros, resampled playback, jitter buffer, 8×400 analysis with 16 bands, mic 8 bands) | T1.4, T1.5, T1.7 |
| §4.5 session (timeline, viz feed shape, word-audible sync, state events/states, latency policy) | T1.6–T1.8 |
| §4.6 settings (defaults, only explicit options) | T1.1 |
| §4.7 UI (fluid steps, quality table + auto rules, WordAssembler rules, GlyphRasterizer, WordPath, injector lifecycle, audio coupling, all post effects, all overlays) | T2.1–T2.8 |
| §5 structure | File Structure |
| §6 builds/CI/packaging | T3.2–T3.4 |
| §7 error handling | T1.2 retry/mismatch, T1.3 noBackend, T1.8 fallback + stderr tail, T1.5 noMicrophone, T1.7 lag stop |
| §8 tests | Every listed test area has named tests; `dialogue_smoke` in T0.3; screenshots in T2.8/T3.1 |
| §9 milestones | M0–M3 |
| §10 risks | T0.3 GATE, fallback chain, headphone hint, quality controller, resume |
| §11 YAGNI | Respected (no AEC, no voice recording UI, no history, no auto-update, no Linux/macOS packages) |

No gaps remain. Additions beyond the spec, each with a reason:
- the `headphoneHintSeen` setting, needed for "beim ersten Start";
- `warning` on `StateEvent`, needed for the CPU-slow warning in spec §4.2;
- the `backend_options` / `get_status` commands, needed for the settings dropdowns and UI reloads;
- the latency formula excludes frame 0, because of F7.

**Departures from the spec text, following the source:**
- The worker `error` payload is nested (F8).
- `fatal` and `cancelled` messages exist (F5, F8).
- `voice` is an added voice name, not a path (F12).
- `language` is always sent, because card fields need it (F13).

**Facts I could not verify in speech.cpp or offline:**
1. Crate and npm versions were not checked against the registries (no network). The lockfiles from T1.0 pin what resolves, and any version that fails to resolve should be bumped to the nearest existing patch.
2. The `Jimver/cuda-toolkit@v0.2.24` tag and the `12.6.3` version string are unverified, and so is whether `cublasLt64_12.dll` keeps the installer within a practical size.
3. Whether `tauri icon` accepts SVG (T1.0 has a fallback).
4. Exact rubato 0.16 `FftFixedIn` constructor and `process` signatures. The T1.4 tests define the required behaviour.
5. Whether cpal 0.15 `Stream` is `Send` on Windows. The plan sidesteps the question by owning streams on the engine thread.
6. Which `VulkanN` index is the Arc B580 on this machine, and the B580's actual per-frame time. Both are measured in T0.1/T0.3.
7. GPU contention between the WebView2 fluid rendering and the worker on the same B580. This is measured in T3.1 step 4.
