# Haru: Echounterdrückung (sonora 0.3.0) — Umsetzungsplan

> plan-agent, 2026-11-21. Spec: `docs/superpowers/specs/2026-11-21-haru-echo-cancellation-design.md` (freigegeben).
> Zeilennummern = Stand beim Lesen; vor jedem Edit prüfen. Pfade relativ zu `C:\Users\Andi\Documents\repo\haru`.
> Weggelassen gegenüber dem Agentenvorschlag: das optionale `audio_check --aec` (YAGNI).

## 0. Fakten aus den sonora-0.3.0-Quellen (`~\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\sonora-0.3.0`)
- Build: `AudioProcessing::builder().config(cfg).capture_config(StreamConfig::new(rate, 1)).render_config(StreamConfig::new(rate, 1)).build()` → `AudioProcessing` (kein Result, kann nicht scheitern). `StreamConfig::new(u32, u16)` const; `num_frames()` = rate/100.
- Config (`src/config.rs`), alle Stufen per Default `None`:
  - `high_pass_filter: Option<HighPassFilter { apply_in_full_band: bool /*true*/ }>`
  - `echo_canceller: Option<EchoCanceller { enforce_high_pass_filtering: bool /*true*/, transparent_mode: TransparentModeType /*Legacy*/ }>`
  - `noise_suppression: Option<NoiseSuppression { level: NoiseSuppressionLevel::{Low,Moderate,High,VeryHigh}, analyze_linear_aec_output_when_available: bool }>` (Default-Level Moderate)
  - `gain_controller2: Option<GainController2 { input_volume_controller: bool /*false*/, adaptive_digital: Option<AdaptiveDigital> /*None*/, fixed_digital: FixedDigital { gain_db: 0.0 } }>`; `AdaptiveDigital::default()` (headroom 5 dB, max 50, initial 15, 6 dB/s, noise −50 dBFS). Limiter läuft in GainController2 immer.
  - `pipeline.maximum_internal_processing_rate` Default `Rate32kHz` — beibehalten.
- `process_render_f32` / `process_capture_f32(&mut self, src: &[&[f32]], dest: &mut [&mut [f32]]) -> Result<(), Error>`: deinterleaved, [−1,1]; `src`/`dest` dürfen nicht aliasen (Scratch-Ausgabe nötig); Slices müssen ≥ rate/100 lang sein (kürzer → Panic in `audio_buffer.rs:268`; `debug_assert`s im Test-Profil). Raten 8 000–384 000 Hz, 44,1 kHz ok. Ausgabe auf [−1,1] geklemmt.
- `sonora::Error` (Copy, PartialEq): `InvalidSampleRate{rate}`, `InvalidChannelCount{count}`, `ChannelMismatch{..}`, `StreamParameterClamped`. `process_*` liefert nur die ersten drei (nur bei ungültigem Format). `set_stream_delay_ms(i32)` klemmt auf [0,500], gibt dann `Err(StreamParameterClamped)` — nie fatal.
- Delay-Hinweis wird an AEC3 gegeben, aber nur bei Render-Buffer-Reset zur Ausrichtung genutzt; interner Delay-Schätzer bleibt an. Vor „render properly started“ geht Capture unverändert durch AEC3.
- Render-Transfer-Queue 100 Frames (1 s), Überlauf still verworfen. `AudioProcessing` ist `Send + Sync`; jeder Aufruf setzt FTZ/DAZ temporär.
- `statistics() -> &AudioProcessingStats`: `echo_return_loss_enhancement: Option<f64>`, `echo_return_loss`, `delay_ms: Option<i32>` (nur für eprintln/Tests).
- Steady state allokiert nicht (nach Warm-up); irrelevant, da Pump-Thread.

## 1. Cargo
`app/src-tauri/Cargo.toml` nach Zeile 27 (`rtrb`):
```toml
sonora = "0.3.0"     # WebRTC APM port (AEC3/NS/AGC2), pure Rust; no default features; "examples" (cpal 0.18 etc.) stays off
```
Feature `examples` NICHT aktivieren. Abhängigkeiten liegen schon in der lokalen Registry (bei Netzproblemen `--offline`).

## 2. Neu `app/src-tauri/src/audio/aec.rs`
In `audio/mod.rs`: `pub mod aec;` und `#[cfg(test)] pub(crate) mod test_signals;`.
```rust
//! Acoustic echo cancellation (spec 2026-11-21) — the only module that knows sonora.
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use sonora::config::{AdaptiveDigital, EchoCanceller, GainController2, HighPassFilter, NoiseSuppression, NoiseSuppressionLevel};
use sonora::{AudioProcessing, Config, StreamConfig};
use super::resample::FrameResampler;

pub const LATENCY_UNKNOWN: u32 = u32::MAX;
pub const DEFAULT_LATENCY_MS: f64 = 40.0; // out+in when no timestamps
pub const MAX_DELAY_MS: i32 = 500;

/// Stores a callback latency in µs; None or > 1 s is ignored (keeps the previous value).
pub fn store_latency(slot: &AtomicU32, d: Option<Duration>);
pub fn load_latency_ms(slot: &AtomicU32) -> Option<f64>;
/// out.unwrap_or(20) + in.unwrap_or(20) + capture_backlog − render_backlog, clamped 0..=500.
pub fn delay_hint_ms(out_ms: Option<f64>, in_ms: Option<f64>, capture_backlog_ms: f64, render_backlog_ms: f64) -> f64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stages { Full, EchoOnly }

#[derive(Debug, Clone, PartialEq)]
pub enum AecError {
    UnsupportedRate(u32),                       // capture % 100 != 0 or outside 8_000..=384_000
    Resampler(String),                          // from FrameResampler errors
    FrameLength { len: usize, frame: usize },
    Processing(sonora::Error),
    Panicked,
}
// impl Display (technical, English) + std::error::Error

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
    resampler: FrameResampler, // passthrough when rates are equal
    chunk_in: usize,           // (render_rate/100).max(1)
    input: Vec<f32>,           // waiting for a full resampler chunk
    output: Vec<f32>,          // capture-rate, waiting for a full frame; capped at 1 s (drop oldest)
    frame: usize, render_rate: u32, capture_rate: u32,
}
impl RenderFramer {
    pub fn new(render_rate: u32, capture_rate: u32) -> Result<Self, AecError>;
    pub fn push(&mut self, s: &[f32]);                 // extend input; while >= chunk_in: output.extend(resampler.process(..)); drain consumed
    pub fn pop_frame(&mut self, out: &mut [f32]) -> bool; // copies exactly `frame` samples
    pub fn pending_samples(&self) -> usize;            // output.len() + input.len()*capture/render
}

pub struct VoiceProcessor {
    apm: AudioProcessing, frame: usize, capture_rate: u32,
    render: RenderFramer,
    render_frame: Vec<f32>, render_out: Vec<f32>, capture_out: Vec<f32>, // each len = frame
    render_frames: u64, capture_frames: u64,
    #[cfg(test)] fail_next: bool,
}
impl VoiceProcessor {
    pub fn new(capture_rate: u32, render_rate: u32, stages: Stages) -> Result<Self, AecError>; // validate rate, then build
    pub fn frame_len(&self) -> usize;
    pub fn push_render(&mut self, samples: &[f32]) -> Result<(), AecError>;
    //   render.push(samples); while render.pop_frame(&mut render_frame) { guarded(|| apm.process_render_f32(&[&render_frame], &mut [&mut render_out]))?; render_frames += 1 }
    pub fn set_delay_hint_ms(&mut self, ms: i32);   // let _ = apm.set_stream_delay_ms(ms.clamp(0, MAX_DELAY_MS));
    pub fn process_capture(&mut self, buf: &mut [f32]) -> Result<(), AecError>;
    //   empty or len % frame != 0 -> Err(FrameLength), buf untouched
    //   #[cfg(test)] if mem::take(&mut fail_next) { return Err(AecError::Panicked) }
    //   for f in buf.chunks_exact_mut(frame) { guarded(|| apm.process_capture_f32(&[&*f], &mut [&mut capture_out[..]]))?; f.copy_from_slice(&capture_out); capture_frames += 1 }
    pub fn pending_render_ms(&self) -> f64;
    pub fn erle_db(&self) -> Option<f64>;          // statistics().echo_return_loss_enhancement
    pub fn estimated_delay_ms(&self) -> Option<i32>;
    pub fn capture_frames(&self) -> u64; pub fn render_frames(&self) -> u64;
    #[cfg(test)] pub(crate) fn fail_next_for_test(&mut self);
}
fn guarded(f: impl FnOnce() -> Result<(), sonora::Error>) -> Result<(), AecError> // catch_unwind(AssertUnwindSafe(f)): Ok(Err(e)) -> Processing(e), Err(_) -> Panicked
const _: fn() = || { fn send<T: Send>() {} send::<VoiceProcessor>(); };
```
Render-Pfad nutzt `FrameResampler` unverändert (allokiert pro Aufruf — ok im Pump-Thread). `FftFixedIn` fügt bei
unterschiedlichen Raten ~5–10 ms Render-Lag hinzu — unkritisch.

## 3. Render-Tap und Latenzen
**`audio/playback.rs`:** `PlaybackShared` (38-44) + `pub output_latency_us: AtomicU32` (init `LATENCY_UNKNOWN`),
`pub tap_dropped: AtomicU64`, manuelles `impl Default`; Literal 125-130 → `Arc::new(PlaybackShared::default())`.
`PlaybackHandle` (46-51) + `pub render_tap: Option<rtrb::Consumer<f32>>` („mono copy of every output frame, only when
requested“). Neue reine Funktion:
```rust
/// One output callback: gate decides how many queued samples play, rest is silence; every value given to
/// `write` (incl. prebuffer/underrun silence) is also pushed to `tap`. Returns (popped, tap_dropped).
pub fn render_block(gate: &mut JitterGate, consumer: &mut rtrb::Consumer<f32>, mut tap: Option<&mut rtrb::Producer<f32>>,
                    frames: usize, mut write: impl FnMut(usize, f32)) -> (usize, u64)
```
(`gate.decide(consumer.slots(), frames)`; `for i in 0..frames { v = if i < n { pop().unwrap_or(0.0) } else { 0.0 }; write(i, v); tap push fail → dropped += 1 }`).
`build_typed` (80-115): Parameter `tap: Option<rtrb::Producer<f32>>`; Closure `move |data: &mut [T], info: &cpal::OutputCallbackInfo|`:
`let ts = info.timestamp(); store_latency(&shared.output_latency_us, ts.playback.duration_since(&ts.callback));` dann
`render_block(&mut gate, &mut consumer, tap.as_mut(), frames, |i, v| { let s = T::from_sample_(v); for c in &mut data[i*channels..(i+1)*channels] { *c = s } })`;
danach `played`/`underruns`/`playing` wie heute plus `tap_dropped` addieren. Keine Allokation im Callback.
`build_playback(dev, prebuffer_frames, render_tap: bool)` (117): bei true `rtrb::RingBuffer::new(rate as usize * 2)`;
Producer in den Match-Arm, Consumer in `PlaybackHandle.render_tap`. `JitterGate` + Tests unverändert.

**`audio/capture.rs`:** `CaptureHandle` (9-15) + `pub input_latency_us: Arc<AtomicU32>`; `build_typed` (21-53)
Parameter `latency: Arc<AtomicU32>`, Closure `move |data: &[T], info: &cpal::InputCallbackInfo|` beginnt mit
`let ts = info.timestamp(); store_latency(&latency, ts.callback.duration_since(&ts.capture));`; `build_capture` (55-86)
legt `Arc::new(AtomicU32::new(LATENCY_UNKNOWN))` an, reicht Klon an die Arme, legt ihn in den Handle.

**`audio/engine.rs`:** `AudioEngine::start(input, output, render_tap: bool)` (22) → `build_playback(&out_dev, PREBUFFER_FRAMES, render_tap)` (33).
Aufrufer: `conversation.rs:489`, `examples/audio_check.rs:37` (`false`).

## 4. `session/mic_pump.rs`
`pub const AEC_FAILED_WARNING: &str = "Echounterdrückung ausgefallen – bitte Kopfhörer verwenden.";`
```rust
pub struct EchoInput {
    processor: VoiceProcessor, tap: rtrb::Consumer<f32>, playback: Arc<PlaybackShared>,
    on_warning: Arc<dyn Fn(String) + Send + Sync>, hint_ms: Option<f64>,
}
impl EchoInput {
    pub fn new(processor: VoiceProcessor, tap: rtrb::Consumer<f32>, playback: Arc<PlaybackShared>, on_warning: Arc<dyn Fn(String) + Send + Sync>) -> Self;
    fn discard_reference(&mut self);   // n = tap.slots(); if n>0 { if let Ok(c) = tap.read_chunk(n) { c.commit_all() } }
    fn process(&mut self, buf: &mut [f32], capture: &CaptureHandle) -> Result<(), AecError> {
        // 1. reference: n = tap.slots(); read_chunk(n) -> (a, b) = as_slices(); push_render(a)?; push_render(b)?; commit_all()
        // 2. hint: raw = delay_hint_ms(load_latency_ms(&playback.output_latency_us), load_latency_ms(&capture.input_latency_us),
        //          capture.consumer.slots() as f64 * 1000.0 / capture.rate as f64, processor.pending_render_ms());
        //          hint = Some(prev + 0.1*(raw-prev)) (first = raw); processor.set_delay_hint_ms(hint.round() as i32)
        // 3. processor.process_capture(buf)   // 20 ms chunk = 2 frames
    }
}
```
`spawn_mic_pump(capture, sender, shared, on_fatal, echo: Option<EchoInput>)` (41-98), im Thread `let mut echo = echo;`.
Im `!started`-Zweig (58-64) nach dem Capture-Drain `if let Some(e) = echo.as_mut() { e.discard_reference(); }`.
Nach Zeile 74 (`last_data = Instant::now();`), vor Mute:
```rust
let failed = echo.as_mut().and_then(|e| e.process(&mut buf, &capture).err());
if let Some(err) = failed {
    eprintln!("haru: echo cancellation off for this conversation: {err}");
    if let Some(e) = echo.take() { (e.on_warning)(AEC_FAILED_WARNING.to_string()); }
}
```
Danach unverändert (Mute, Pegel — jetzt auf bereinigtem Signal —, Segment/Request-id, Send, seq, sent_samples). Optional
nach der Schleife eine stderr-Zusammenfassung (ERLE, Delay, tap_dropped). Fehlerhafter Frame bleibt roh (nur bei Erfolg
zurückkopiert); AEC bleibt für den Rest des Gesprächs aus.

## 5. `session/conversation.rs` und `session.rs`
**conversation.rs:** Imports (9-23) `AtomicU32`, `crate::audio::aec::{Stages, VoiceProcessor, LATENCY_UNKNOWN}`, `mic_pump::EchoInput`. Neu:
`pub const AEC_UNAVAILABLE_WARNING: &str = "Echounterdrückung nicht verfügbar – bitte Kopfhörer verwenden.";`
`pub fn wants_echo_cancellation(s: &Settings, io: &ConversationIo) -> bool { s.echo_cancellation && !io.no_playback && io.input_samples.is_none() }`.
`new_shared_playback` (118-125) → `Arc::new(PlaybackShared::default())`. `start_with` (472-483) + Parameter
`on_warning: Arc<dyn Fn(String) + Send + Sync>` nach `on_end`; Z. 486 `let (engine, cap, mut play) = ...`, Z. 489
`want_aec` an `AudioEngine::start`; Z. 501 (Feeder-`CaptureHandle`) `input_latency_us: Arc::new(AtomicU32::new(LATENCY_UNKNOWN))`. Nach Z. 506:
```rust
let mut startup_warning = None;
let echo = if want_aec {
    let p = play.as_mut().expect("real playback when echo cancellation is wanted");
    match (p.render_tap.take(), VoiceProcessor::new(mic_rate, p.rate, Stages::Full)) {
        (Some(tap), Ok(vp)) => { eprintln!("haru: echo cancellation on (capture {mic_rate} Hz, output {} Hz)", p.rate);
                                 Some(EchoInput::new(vp, tap, p.shared.clone(), on_warning.clone())) }
        (_, r) => { eprintln!("haru: echo cancellation unavailable: {}", r.err().map_or("no render tap".into(), |e| e.to_string()));
                    startup_warning = Some(AEC_UNAVAILABLE_WARNING.to_string()); None }
    }
} else { None };
```
Z. 565 `spawn_mic_pump(capture, sender.clone(), mic.clone(), on_fatal, echo)`. `Conversation` (237-242) + Feld
`startup_warning: Option<String>` (bei 601 setzen) + `pub fn startup_warning(&self) -> Option<String>`. Tests 848-859
und 905-916: `Arc::new(|_| {})` nach `on_end` ergänzen.

**session.rs** (bestehender `StateEvent.warning`-Pfad): in `start_inner` neben `renewal` (~526):
```rust
let (ww, wk) = (Arc::downgrade(self), id.clone());
let on_warning: Arc<dyn Fn(String) + Send + Sync> = Arc::new(move |t| if let Some(c) = ww.upgrade() { c.conversation_warning(&wk, t) });
```
an `start_with` (Z. 541) übergeben. Im `Ok(conv)`-Zweig `let warning = conv.startup_warning();` vor dem Speichern und
`self.set(&mut g, Action::Started, warning, None)?` (Z. 552). Neu `fn conversation_warning(&self, id: &str, text: String)`
analog `segment_renewed` (590-599): nur bei Talking + passender id: `let ev = Self::event(&g, Some(text), None); g.last = ev.clone(); (self.on_state)(ev);`.

## 6. Settings, IPC, UI, README
- `settings.rs`: nach Z. 28 `pub echo_cancellation: bool, // true: remove Haru's voice from the mic (AEC)`, in `Default`
  `true` (struct-weites `#[serde(default)]` → fehlendes Feld = true; camelCase → `echoCancellation`). Keine Validierung,
  `needs_reload` unverändert (wirkt ab nächstem Gespräch).
- `app/src/ipc/types.ts` (18-25): `echoCancellation: boolean;` + `echoCancellation: true` in `DEFAULT_SETTINGS`.
- `app/src/ipc/contract.fixtures.json` Z. 10: `"echoCancellation":true` nach `"outputDevice":null`.
- `app/src/ipc/contract.test.ts` (38-41): `expect(DEFAULT_SETTINGS.echoCancellation).toBe(true)`.
- `app/src/ui/settings.ts`: Feld `private readonly echoCancellation = el('input');` (~50), `type = 'checkbox'` (82-84);
  Audio-Abschnitt (110-114) nach `outputDevice`: `this.toggle(T.echoCancellation, this.echoCancellation), el('p', 'hint-note', T.echoCancellationHint)`;
  `open()` (~164) `this.echoCancellation.checked = s.echoCancellation;`; `collect()` (~220) `echoCancellation: this.echoCancellation.checked,`.
- `app/src/ui/strings.ts`: `hint.text` (12) `'Kopfhörer klingen am besten. Über Lautsprecher sorgt die Echounterdrückung dafür, dass Haru sich nicht selbst hört.'`;
  in `settings` `echoCancellation: 'Echounterdrückung'`, `echoCancellationHint: 'Entfernt Harus Stimme aus dem Mikrofon, wenn du Lautsprecher nutzt. Gilt ab dem nächsten Gespräch.'`.
  (Prüfen, ob es die CSS-Klasse `hint-note` gibt; sonst eine vorhandene Hinweis-Klasse nutzen.)
- `README.md`: Funktionen-Bullet Echounterdrückung (sonora/WebRTC AEC3 + NS + AGC2, Unterbrechen bleibt möglich);
  Voraussetzungen „Kopfhörer oder Lautsprecher (Kopfhörer klingen am besten)“; neuer Hinweistext; Settings-Tabelle
  `echoCancellation` (Default `true`, wirkt ab nächstem Gespräch); Tests: Befehl für den ignorierten Echtzeit-Simulationstest;
  Module „Echounterdrückung (`aec.rs`)“; Fehlerbehebung (Schalter an? starkes Restecho z. B. durch sehr laute Wiedergabe
  oder Windows-Klangverbesserungen → Kopfhörer); Lizenzen `sonora` (BSD-3-Clause); bekannte Grenzen: „Keine
  Echo-Unterdrückung“ ersetzen durch „Echounterdrückung synthetisch geprüft; Praxistest mit Lautsprechern durch Nutzer“.
  `test_hooks.rs`-Doc: Headless/Feeder = Echounterdrückung aus.

## 7. Tests
**`audio/test_signals.rs`** (cfg(test)), deterministisch, 48 kHz:
- `Rng(u64)` xorshift64 (`<<13, >>7, <<17`), `uniform()` = `((x>>40) as f32 / 2^23) - 1.0`, `range(lo, hi)`.
- `Voice { seed, f_start: (f32,f32), f_end: (f32,f32), rms }`: `FAR = {0x5EED_0001, (120,300), (600,2800), 0.1}`, `NEAR = {0x5EED_0002, (180,400), (400,2000), 0.1}`.
- `speech_like(v, rate, samples)`: Silben 120–350 ms, Gain 0,5–1,0; harmonischer Chirp, f0 linear von `range(f_start)` zu
  `range(f_end)`, Phase pro Silbe zurückgesetzt, `voiced = sin φ + 0.5 sin 2φ + 0.25 sin 3φ`; Rauschen `lp = 0.6·lp + 0.4·u`,
  `x = 0.6·voiced + 0.8·lp`; 15-ms-Raised-Cosine-Flanken; Pausen 40–250 ms, jede 6. 400–700 ms; RMS über Nicht-Null-Samples = `v.rms`.
- `room_impulse_response(seed, rate)`: 100 ms (4800 Taps) `u·exp(−k/τ)`, τ = 0.1·rate/(3 ln 10) (RT60 100 ms), L2-Norm 0,5.
- `convolve(x, h)`: eine rustfft-Faltung in f64, Größe `(x.len()+h.len()−1).next_power_of_two()`, auf `x.len()` gekürzt.
- `echo_of(far, h, delay)`, `energy`, `ratio_db(num, den) = 10·log10(Σnum²/Σden²)`,
  `max_norm_xcorr(out, reference, range, max_lag)` = max über Lag 0..=max_lag der normierten Kreuzkorrelation.

**`audio/aec.rs`-Tests** — Helfer `run(far, mic, rate, stages)`: 20-ms-Chunks `push_render(far chunk)`, `set_delay_hint_ms(40)` (bewusst ≠ 60), `process_capture(mic chunk)`:
- `erle_at_least_20_db_after_convergence`: 8 s FAR; RIR Seed `0x5EED_0003`; 60 ms (2880 Samples) Verzögerung; mic = Echo + `1e-4·u` (Seed `0x5EED_0004`); `EchoOnly`; `ratio_db(mic[5..8 s], out[5..8 s]) >= 20.0`; sonora-ERLE/Delay per eprintln (keine Assertion).
- `double_talk_keeps_near_end`: wie oben; NEAR (2 s) bei 5,0–7,0 s; mic = Echo + Near + Rauschen; `EchoOnly`; ERLE 3–5 s ≥ 15 dB; `ratio_db(out[5..7 s], near[5..7 s]) >= −6.0`; `max_norm_xcorr(out, near, 5..7 s, 480) >= 0.7`.
- `chunk_of_20_ms_is_two_frames`: (48000,480), (44100,441), (16000,160) mit `Full`: `frame_len()`, ein `rate/50`-Puffer → `capture_frames() == 2`.
- `rejects_partial_frames`: 500 Samples bei 48 kHz → `Err(FrameLength{len:500, frame:480})`, Puffer unverändert, `capture_frames() == 0`.
- `rejects_rate_not_divisible_by_100`: 22050, 11025, 7000 → `Err(UnsupportedRate)`; 8000, 44100, 48000, 96000 → `Ok`.
- `render_resampling_44k1_to_48k`: 1 s 1-kHz-Sinus bei 44,1 kHz in 1000er-Stücken in `RenderFramer`, nach jedem Push Frames holen: alle 480 Samples, 98..=100 Frames, steigende Nulldurchgänge ab Frame 5 = 1/ms ±2 %.
- `render_passthrough_when_rates_equal`: 48k→48k, 1234 + 1166 Samples → 5 Frames bitgleich zur Eingabe.
- `full_chain_smoke`: `Full`, Capture 48k, Render 44,1k, 3 s synthetisch; keine Fehler, alle Samples endlich, |x| ≤ 1.
- `stages_config`: `Full` = HPF an, AEC an, NS Moderate, AGC2 adaptive digital an, `input_volume_controller` false; `EchoOnly` = nur AEC (über `vp.apm.config()`).
- `delay_hint_formula`: (None,None,0,0)→40; (12,8,15,5)→30; negativ→0; 700→500.
- `store_latency_ignores_absurd_values`: 12,5 ms gespeichert; danach 2 s und None lassen 12,5.

**`audio/playback.rs`-Tests:** `tap_receives_played_samples_including_prebuffer_silence` (Prebuffer 600, Ring 500 Werte, Frames 256; Aufruf 1 → 256 Nullen; 500 weitere Werte pushen, 4 weitere Aufrufe; Tap- und `write`-Folge = `[0;256] ++ v[0..1000] ++ [0;24]`, identisch); `tap_full_drops_and_counts` (Tap-Kapazität 300, zwei Aufrufe à 256 → dropped 0 dann 212); `playback_shared_default_latency_unknown`.

**`session/mic_pump.rs`-Tests** (neues Modul, LineTap-`Write` + `WorkerSender::with_writer(.., 512)`, `started` vor Spawn setzen):
`pump_without_echo_sends_raw_samples` (16 kHz, 3 Chunks → JSON `pcm == encode_f32(input chunk k)`, `seq` k: bitgenauer Aus-Schalter);
`pump_disables_echo_after_error_and_warns_once` (`VoiceProcessor` 16k/16k `EchoOnly` mit `fail_next_for_test()` → genau eine Warnung = `AEC_FAILED_WARNING`, alle 3 Chunks roh);
`pump_drains_tap_while_not_started` (`started` false, 3000 Samples in Tap mit Kapazität 4800 → binnen 1 s `slots()` wieder 4800);
`pump_cancels_echo_in_real_time_simulation` (`#[ignore]`, ~8,5 s: „Geräte“-Thread schiebt alle 10 ms FAR in den Tap und Echo in den Capture-Ring, per `Instant` getaktet; echte Pumpe mit `EchoOnly`; Chunks dekodieren; ERLE letzte 3 s ≥ 15 dB).

**`session/conversation.rs`:** `echo_cancellation_only_with_real_devices` (`wants_echo_cancellation` nur true mit Setting an und Default-`ConversationIo`; false bei `no_playback`, `input_samples`, Setting aus).

**`settings.rs`:** `defaults_match_spec` + `assert!(s.echo_cancellation)`; neu `old_settings_without_echo_cancellation_turn_it_on` (alte JSON ohne Feld laden → true, keine `.bad`-Datei); neu `echo_cancellation_off_roundtrips` (false speichern/laden; JSON enthält `"echoCancellation": false`); `needs_reload_on_backend_device_voice_only` + `!needs_reload(echo_cancellation: false)`.

## 8. Verifikation
1. `cargo build --manifest-path app/src-tauri/Cargo.toml` (ggf. `--offline`)
2. `cargo test --manifest-path app/src-tauri/Cargo.toml` und `... --lib aec -- --nocapture` (ERLE-/Korrelationsmargen ansehen)
3. `cargo test --manifest-path app/src-tauri/Cargo.toml --lib pump_cancels_echo_in_real_time_simulation -- --ignored --nocapture`
4. `npm --prefix app test`, `npm --prefix app run typecheck`, `npm --prefix app run build`
5. `cargo build --release --manifest-path app/src-tauri/Cargo.toml --examples`
6. Headless (AEC-aus-Pfad unverändert): `cargo run --release --manifest-path app/src-tauri/Cargo.toml --example headless_talk -- --wav build\test\question-de.wav --no-playback --seconds 20` → RESULT: OK

Nur der Nutzer (AMD-PC, Lautsprecher): Haru antwortet sich nicht selbst; Unterbrechen und „mhm“ gehen; Schalter aus →
Echo kommt zurück (A/B); Kopfhörer klingen nicht schlechter.

## 9. Risiken
1. ERLE ≥ 20 dB mit 100-ms-RIR ungemessen (sonora-eigener Test fordert > 10 dB bei reiner Verzögerung). Scheitert es:
   erst sonoras Delay-Schätzung (~60 ms?) und Reihenfolge RIR/Delay prüfen, dann berichten — Schwelle NICHT still senken.
2. WASAPI-Zeitstempel unterschätzen die Latenz (frames_available/rate, ohne Engine/Treiber) → Hinweis zu niedrig; AEC3
   schätzt selbst; maximale Suchverzögerung (Bluetooth ≥ 200 ms) nicht geklärt.
3. Takt-Drift getrennter Ein-/Ausgabegeräte und Render-Bursts pro WASAPI-Periode → AEC3-Eigenbehandlung, real ungetestet.
4. AGC2 (Start 15 dB, bis 50 dB) und NS Moderate verändern, was das Modell hört (wie Chrome) — Nutzer soll beurteilen.
5. Laufzeitwarnung zwischen Frame 0 und Talking wird von `conversation_warning` verworfen (praktisch unmöglich).
6. `catch_unwind` greift nur bei `panic = "unwind"` (aktueller Default).
7. Windows-Klangverbesserungen / treiberseitige Mic-Verarbeitung können Nichtlinearität oder eine zweite AEC einbringen.
8. UI dedupliziert gleiche Warntexte pro App-Lauf (`lastWarning`) — akzeptabel als „einmalig“.
9. sonora jung → API nur in `audio/aec.rs`.
