# Haru: Q8_0, 8-GB-GPU-Regel, Gesprächslänge aus freiem Speicher mit Erneuerung, offizielle Karte

> Umsetzungsplan (plan-agent, 2026-11-21), freigegeben vom Nutzer: „8 GB GPU als Voraussetzung, Sessionlänge so viel wie
> der freie Speicher hergibt, baue das so wie du denkst.“ Pfade relativ zu `C:\Users\Andi\Documents\repo\haru`.
> Reihenfolge 1 → 2 → 3 → 4 (2 und 3 ändern beide `Core::load_from`).

**Abweichungen gegenüber dem Plan-Agenten (Entscheidung des Orchestrators):**
- `MIN_GPU_MEMORY` = **7 GiB** (nicht 7,5 GiB): manche 8-GB-Karten melden über Vulkan weniger als 7,5 GiB; alles unter
  7 GiB ist sicher eine 6-GB-Klasse. Tests entsprechend (`auto_skips_gpu_below_7_gib` mit 6 GiB, `auto_accepts_7_gib_and_8e9_bytes`).
- Die E2E-Prüfung misst zusätzlich den tatsächlichen KV-Verbrauch pro Sekunde (siehe unten), um `KV_BYTES_PER_SEC`
  zu bestätigen.

## Was die speech.cpp-Quellen klären
- **Ende bei max_seconds:** `extern/speech.cpp/src/engines/haru-engine.cpp:57-73` — `Stream::push` wirft `OutOfRange`
  (Option `max_seconds`), sobald `max_frames` Frames gegeben sind und ein weiterer ganzer Mic-Frame kommt. Der Worker
  sendet nach dem letzten Frame `{"type":"error","id":X,"error":{"code":"out_of_range","option":"max_seconds",...}}`
  (`docs/worker.md:132-133`, `docs/models/haru.md:66-68`). Die KV gehören zum `haru::Dialogue` im `Stream` (pro Request,
  haru-engine.cpp:107) und werden mit Request-Ende frei. `Inbox::finish` (`tools/worker/inbox.cpp:126-133`) legt die alte
  id in `dropping_`; spätere `chunk`s dazu werden still verworfen (inbox.cpp:393), ein `hang_up` dafür still geschluckt
  (inbox.cpp:427). Heute: `fail!` → `ConversationEnd::Failed(worker_error)` → `Core::conversation_ended` → Ready + Fehler-Toast.
- **Neuer Dialog ohne Neuladen:** ja. Requests laufen nacheinander auf dem einen geladenen Modell (worker.md:30, 75-79);
  die nächste `dialogue`-Zeile startet ein frisches `haru::Dialogue` auf denselben Gewichten. Die per `--add-voice`
  hinzugefügte Stimme `user` bleibt. Nur der Prompt (Karte + Stimme) läuft erneut.
- **Freier Speicher:** `speech devices --json` fragt jedes Gerät (`tools/cli/model.cpp:337-359`). Unter Windows heißt der
  CPU-Eintrag "CPU", `memory_free = ullAvailPhys` (`ggml/src/ggml-cpu/ggml-cpu.cpp:365-382`).
- `DialogueOptions` (`app/src-tauri/src/worker/protocol.rs:9-27`) hat schon `other_name`, `relationship`, `situation`,
  `max_seconds: Option<f64>` — keine Protokolländerung nötig.

---

## Änderung 1: Q8_0 statt Q6_K

1.1 `app/src-tauri/src/paths.rs:35-41`: `HARU_Q6K` → `pub const HARU_MODEL: ModelSpec` (repo/revision gleich, file
`"Haru-4B-Q8_0.gguf"`, size `6_623_102_720`, sha256
`"c372a527e63ca4251429a48863535b8bca03ebd6f414bbf31f28a9d175349934"` — gegen Hugging Face verifiziert). Ein-Zeilen-Doc:
Q8_0 liegt dem offiziellen Modell am nächsten (docs/models/haru.md). Tests 212-235 (`model_path_layout`,
`model_url_matches_spec`) auf `HARU_MODEL`/neuen sha-Ordner/Dateinamen. `model_fetch.rs` löscht nur `cfg.dest` (Z. 112) —
die alte Q6_K-Datei im gemeinsamen speech.cpp-Cache bleibt; **kein** Aufräumen einbauen.

1.2 Nutzer: `app/src-tauri/src/lib.rs:37` → `paths::HARU_MODEL`; `app/src-tauri/examples/headless_talk.rs:15,73`.

1.3 Testdaten/UI-Text: `app/src-tauri/src/ipc.rs:78-82` und `app/src/ipc/contract.fixtures.json:4-6` (total `6623102720`,
Pfad `C:\\models\\Haru-4B-Q8_0.gguf`); `app/src/ipc/mock-client.ts:9` (`MOCK_TOTAL_BYTES = 6_623_102_720`) und Z. 99
(`Haru-4B-Q8_0.gguf`); `app/src/ui/format.test.ts:6` → `expect(formatBytes(6623102720)).toBe('6,62 GB')`, Doc-Beispiel
`format.ts:7`; `app/src-tauri/src/worker/protocol.rs:152` `"weight_type":"Q6_K"` → `"Q8_0"`. `README.md` :30 → „ca. **7 GB**“,
:50 → „6,6 GB“, :54 → `...\c372a527e63ca4251429a48863535b8bca03ebd6f414bbf31f28a9d175349934\Haru-4B-Q8_0.gguf` + Satz, dass
ein früheres Q6_K im gemeinsamen speech.cpp-Cache bleibt (manuell löschbar), :230 → `Haru-4B-Q8_0.gguf`, :258 (historische
Q6_K-Messung) behalten mit „(gemessen mit Q6_K)“. `docs/` (Spec/Plan/m0) unverändert lassen.

---

## Änderung 2: 8-GB-GPU-Voraussetzung

2.1 `app/src-tauri/src/backend/plan.rs`:
```rust
pub const MIN_GPU_MEMORY: u64 = 7 * 1024 * 1024 * 1024; // 7 GiB: 8-GB-Karten melden teils < 7,5 GiB
pub fn qualifies(d: &Device) -> bool { d.kind != DeviceKind::Cpu && d.memory_total >= MIN_GPU_MEMORY }
```
`largest` (38-41) filtert `kind(d.kind) && qualifies(d)` (nur von `auto_attempt` 55-75 genutzt): Auto wählt nie eine
kleinere GPU/iGPU; ohne qualifizierendes Gerät liefert die Variante keinen Versuch und der Plan fällt auf den bestehenden
CPU-Versuch; der explizite Pfad in `choose_plan` (92-114, `find_listed`) bleibt ungefiltert; fehlende Geräteliste gibt
weiter `"auto"` (65-67). Reiner Klassifizierer:
```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeviceNote { Cpu, CpuNoCapableGpu, SmallGpu }
pub fn device_note(a: &Attempt, devices: &HashMap<Variant, Vec<Device>>, ov: &Overrides) -> Option<DeviceNote> {
    let is_cpu = a.variant == Variant::Cpu || a.device.eq_ignore_ascii_case("cpu");
    if is_cpu {
        let explicit = ov.backend == Some(Variant::Cpu) || ov.device.as_deref().is_some_and(|d| d.eq_ignore_ascii_case("cpu"));
        let capable = devices.values().flatten().any(qualifies);
        return Some(if !explicit && !capable { DeviceNote::CpuNoCapableGpu } else { DeviceNote::Cpu });
    }
    let d = devices.get(&a.variant)?.iter().find(|d| d.name.eq_ignore_ascii_case(&a.device))?;
    (!qualifies(d)).then_some(DeviceNote::SmallGpu)
}
```
Tests: `auto_skips_gpu_below_7_gib` (6 GiB → `["vulkan/cpu"]`), `auto_accepts_7_gib_and_8e9_bytes`,
`explicit_small_device_is_respected` (Override `Vulkan0` 6 GiB → `plan[0]` = `vulkan/Vulkan0`),
`device_note_cpu_without_capable_gpu`, `device_note_cpu_explicit_keeps_plain_warning`,
`device_note_cpu_after_capable_gpu_failed_is_plain`, `device_note_small_explicit_gpu`, `device_note_auto_large_gpu_is_none`,
`device_note_unlisted_auto_attempt_is_none`. Bestehende Tests bleiben grün (prüfen).

2.2 `app/src-tauri/src/session.rs` (bestehenden Warnpfad nutzen: `load_from` → `BootInfo.warning` und `Loaded`-
`StateEvent.warning`; `app.ts` `onState` 80-83 / `runBoot` 145-148 → `toast.showWarning`). `overrides: Overrides`
(Default) in `Inner` (43-62) und `Core::new` (89-107); in `boot_inner` (254-259) `g.overrides = ov`. Neben `CPU_WARNING`
(32): `NO_GPU_WARNING = "Keine Grafikkarte mit mindestens 8 GB gefunden – Haru läuft auf der CPU und kann zu langsam sein."`
und `fn note_text(n, &Attempt) -> String` (`SmallGpu` → `format!("{} hat weniger als 8 GB Grafikspeicher – Haru braucht
mindestens 8 GB, das Gespräch kann abbrechen.", a.description)`). In `load_from` (355-363, 381): `cpu`-bool entfernen;
`note = g.detection.as_ref().and_then(|d| device_note(&attempt, &d.devices, &g.overrides))`;
`warning = match (&switched, note) { (Some(..), _) => switched-Text (wie bisher), (None, Some(n)) => Some(note_text(n, &attempt)), _ => None }`.
Wirkungslose Z. 381 löschen. `README.md:125-127`: Regel ≥ 7 GiB und neue Warnung.

---

## Änderung 3: Gesprächslänge aus freiem Speicher, automatische Erneuerung

3.1 Neu `app/src-tauri/src/session/budget.rs` (`pub mod budget;` in `session.rs:3-8`):
```rust
pub const KV_BYTES_PER_SEC: u64 = 500_000;
pub const GPU_MARGIN: u64 = 512 << 20;  pub const CPU_MARGIN: u64 = 1 << 30;
pub const MIN_SECONDS: u32 = 120; pub const MAX_SECONDS: u32 = 3600; pub const FALLBACK_SECONDS: u32 = 600;
pub const OVERRIDE_ENV: &str = "HARU_MAX_SECONDS";
pub fn margin_for(kind: DeviceKind) -> u64
/// clamp(floor((free - margin) / KV_BYTES_PER_SEC), 120, 3600) — DIE reine Funktion
pub fn max_seconds_for(memory_free: u64, kind: DeviceKind) -> u32
pub fn override_seconds(raw: Option<&str>) -> Option<f64>   // f64 > 0, endlich; clamp [10, 3600]
pub fn find_device<'a>(list: &'a [Device], name: &str) -> Option<&'a Device> // Name case-insensitiv; "cpu" trifft auch kind Cpu
#[derive(Clone, Debug)] pub struct Probe { pub exe: PathBuf, pub device: String }
#[derive(Clone, Copy, Debug, PartialEq)] pub enum Source { Override, Memory { free: u64 }, Fallback }
#[derive(Clone, Copy, Debug, PartialEq)] pub struct Budget { pub seconds: f64, pub source: Source }
impl Budget { pub fn measured(&self) -> Option<f64>; pub fn describe(&self) -> String /* "free X B - margin M B = R s -> N s" (R vor Clamp) */ }
pub fn measure_with(env: &dyn Fn(&str)->Option<String>, query: &dyn Fn(&Path)->Result<Vec<Device>,HaruError>,
                    probe: Option<&Probe>, fallback: Option<f64>) -> Budget // Override > Speicher > fallback.unwrap_or(600)
pub fn measure(probe: Option<&Probe>, fallback: Option<f64>) -> Budget // std::env + devices::query_devices
```
Override-Untergrenze 10 s (unter einem Frame würde der erste Dialog als Backend-Fehler → CPU-Fallback scheitern).
Tests: `gpu_budget_uses_512_mib_margin`, `cpu_budget_uses_1_gib_margin`, `budget_floors_partial_seconds`,
`budget_clamps_to_120_and_3600` (free 0, free < margin, free 100 GB), `override_parses_and_clamps` ("30", " 45.5 ", "0",
"-5", "abc", "", "5"→10, "99999"→3600), `find_device_by_name_and_cpu_kind`, `measure_prefers_override_without_querying`
(Query-Fake zählt Aufrufe), `measure_uses_fallback_on_query_error_or_missing_device` (Some(700)→700, None→600).

3.2 `app/src-tauri/src/worker/protocol.rs` nach `worker_error` (111-116):
`pub fn is_segment_limit(e: &WorkerErrorBody) -> bool { e.code == "out_of_range" && e.option.as_deref() == Some("max_seconds") }`.
Test `segment_limit_is_out_of_range_max_seconds_only` (true für out_of_range/max_seconds; false für
out_of_memory/max_seconds und out_of_range/speed).

3.3 `app/src-tauri/src/worker/process.rs:62`: `fn with_writer` → `pub(crate) fn with_writer` (Tests mit In-Memory-Writer).

3.4 `app/src-tauri/src/session/mic_pump.rs`: `MicShared` (18-25) + `pub request_id: Mutex<String>`, `pub segment: AtomicU64`.
`spawn_mic_pump` (37-87) ohne `id: String`-Parameter; lokal `seg = u64::MAX`, `id`, `seq`; direkt vor dem Bau des `Chunk` (76):
```rust
let cur = shared.segment.load(Acquire);
if cur != seg { seg = cur; id = shared.request_id.lock().unwrap_or_else(|e| e.into_inner()).clone(); seq = 0; }
```
Der bestehende `!started`-Drain (53-59) verwirft Audio zwischen Abschnitten.

3.5 `app/src-tauri/src/session/conversation.rs` — Erneuerung innerhalb der laufenden Conversation. Nichts wird neu
gestartet (AudioEngine, Capture-Ring, Playback-Queue, virtueller Player, Viz-Ticker, Mic-Pump laufen weiter); nur der
Worker-Request wechselt. Mic-Audio in der Lücke (Gerätequery + Prompt, ~0,5–2 s) wird verworfen.
```rust
#[derive(Clone)]
pub struct Renewal {
    /// Fresh max_seconds of the next segment (blocking: runs `speech devices --json`).
    pub max_seconds: Arc<dyn Fn() -> f64 + Send + Sync>,
    /// Called once the next segment's dialogue line is queued.
    pub on_renewed: Arc<dyn Fn(u32) + Send + Sync>,
}
struct Request { id: String, in_flight: bool, segment: u32 }
```
`Inner` (133-149): `id` bleibt stabiler Gesprächsschlüssel (`Conversation::id()`, Core matcht darauf); neu
`request: Mutex<Request>` (init `{id: id.clone(), in_flight: true, segment: 1}`), `options: DialogueOptions`,
`renewal: Option<Renewal>`. Helfer:
```rust
/// Sends to the request in flight; Ok(false) when none is (between segments).
fn send_to_current(&self, msg: impl FnOnce(String) -> ClientMsg) -> Result<bool, HaruError> {
    let r = self.request.lock().unwrap_or_else(|e| e.into_inner());
    if !r.in_flight { return Ok(false); }
    self.sender.send(&msg(r.id.clone())).map(|_| true)
}
```
Neu `fn renew_segment(inner: &Inner) -> Result<Option<String>, HaruError>`:
1. `request` sperren, `in_flight = false`, freigeben; `inner.mic.started.store(false, Release)`.
2. `let secs = (renewal.max_seconds)();` — blockiert, ohne Lock (alte KV sind frei, der Worker hat geantwortet).
3. `request` sperren. Bei `hanging_up || stop` → `Ok(None)`. Sonst: `segment += 1`; `new_id = format!("{}-s{}", inner.id, segment)`;
   `opts = inner.options.clone(); opts.speak_first = None; opts.max_seconds = Some(secs);`
   `sender.send(Dialogue{new_id, sample_rate: inner.mic_rate, opts})?`; `r.id = new_id; r.in_flight = true;`
   `*inner.mic.request_id.lock() = new_id.clone()`; `inner.mic.segment.fetch_add(1, Release)`;
   `inner.mic.sent_samples.store(0)`; `inner.frames.store(0)`; Lock freigeben, Abschnittszeile per `eprintln!`,
   `(on_renewed)(segment)`, `Ok(Some(new_id))`.
   **Reihenfolge einhalten:** Die dialogue-Zeile wird eingereiht, bevor die Pumpe die neue id erfährt (ein Chunk mit neuer
   id vor seiner dialogue-Zeile ergäbe einen Fehler ohne id, den der Dispatch-Loop als fatal behandelt).

`dispatch_loop` (178-301): neue Locals `rid = inner.id.clone()`, `seg_first = false`, `seg_deadline`, `seq_base = 0`,
`next_seq = 0`. Chunk/End/Cancelled/Error-Guards vergleichen mit `rid` statt `inner.id`. Chunk-Arm (219-250): `seq_base + seq`
in die Timeline, `next_seq` nachführen; den `!got_first && seq == 0`-Block ersetzen durch
`if !seg_first && seq == 0 { seg_first = true; inner.mic.started.store(true); if !got_first { got_first = true; first_tx.send(Ok(())) } }`.
Error-Arm (270-273):
```rust
let renewable = got_first && seg_first && id.is_some() && is_segment_limit(&error)
    && inner.renewal.is_some() && !inner.hanging_up.load(Acquire);
if renewable {
    match renew_segment(&inner) {
        Ok(Some(new_id)) => {
            // closes the word the old segment cut off (" " ends a word in the assembler)
            inner.timeline.lock()...push(next_seq, " ".into(), out.pushed.load(Acquire));
            seq_base = next_seq + 1; next_seq = seq_base;
            rid = new_id; seg_first = false; seg_deadline = Instant::now() + FIRST_FRAME_TIMEOUT;
        }
        Ok(None) => { inner.set_terminal(); ended = true; }
        Err(e) => fail!(e),
    }
} else { let e = worker_error(&error); fail!(if got_first { e } else { e.backend_failure() }); }
```
Kein `continue` (das Padding 295-299 muss laufen). Timeout-Arm (275-286): Z. 277 `send_to_current(Cancel)`; zusätzlich
`if got_first && !seg_first && now >= seg_deadline { let _ = inner.send_to_current(Cancel); fail!(Worker "Der Sprachmodell-Worker antwortet nicht." detail "no frame 0 of the next segment within 30 s" .backend_failure()) }`.
`has_terminal()` bleibt über die Erneuerung false → `ticker_loop` sendet weiter Viz-Frames, das Gesicht läuft weiter.

`ticker_loop` (326, 348): beide `Cancel` → `let _ = inner.send_to_current(|id| ClientMsg::Cancel { id });`.

`hang_up` (513-536):
```rust
match self.inner.send_to_current(|id| ClientMsg::HangUp { id }) {
    Err(e) => result = Err(e),
    Ok(false) => {}                                   // between segments: nothing in flight
    Ok(true) => if !self.inner.wait_terminal(timeout) {
        match self.inner.send_to_current(|id| ClientMsg::Cancel { id }) {
            Ok(true) if self.inner.wait_terminal(Duration::from_secs(1)) => {}
            Ok(false) => {}
            _ => result = Err(stuck()),
        }
    }
}
```
(verhindert das falsche „reagiert nicht mehr“ samt Worker-Neuladen — ein `hang_up` für die alte id wird still geschluckt).
`abort` (539-552) analog mit `Cancel`; `Ok(false)` gilt als erledigt.

`start_with` (374-498): neue Signatur `(sender, events, settings: &Settings, options: DialogueOptions, renewal: Option<Renewal>, muted, on_viz, on_end, id, io)`;
`mic.request_id = id` vor dem Pumpen-Spawn; `options`/`renewal` in `Inner`; dialogue-Send (449-453) mit
`inner.options.clone()` statt `settings.dialogue_options()`; `spawn_mic_pump` ohne `id`. Settings weiter für Audiogeräte
und `lag_limit_secs`. `Conversation::start` (359-371) ohne Aufrufer → löschen.

Tests in `conversation.rs` mit `ConversationIo { input_samples: Some((16000, vec![0.0; 16000])), no_playback: true }`
(kein Audiogerät nötig): Fake-Writer `LineTap: Write` (zerlegt Zeilen, schickt `serde_json::Value`s in einen Channel),
`WorkerSender::with_writer(tap, 512)`; Fake-Worker speist `Sender<WorkerMsg>`; Frames `encode_f32(&vec![0.0; 3200])`;
`Settings { lag_limit_secs: 10.0, ..Default::default() }`.
- `renews_segment_on_max_seconds_without_ending`: dialogue `c1` mit Frame 0 beantworten, auf Chunk `c1` warten,
  Segment-Limit-Fehler senden → nächste dialogue-Zeile hat id `c1-s2`, `max_seconds == 42.0`, kein `speak_first`;
  `on_renewed(2)` gefeuert; Frame 0 von `c1-s2` senden → nächster Chunk `c1-s2` mit `seq == 0`; `hang_up` → Zeile
  `hang_up` `c1-s2`; `End` beantworten → `Ok`; `on_end` nie aufgerufen.
- `hang_up_between_segments_sends_nothing_and_returns`: `max_seconds`-Closure blockiert an einem Channel; nach dem
  Limit-Fehler `hang_up(3 s)` im Thread, Gate nach 100 ms öffnen → `hang_up` gibt `Ok` in < 2 s; keine `hang_up`-,
  `cancel`- oder zweite `dialogue`-Zeile geschrieben; `on_renewed` nicht aufgerufen.

3.6 `app/src-tauri/src/session.rs` (Core): Imports (16-26) `budget::{self, Probe}`, `conversation::Renewal`. `Inner`
+ `probe: Option<Probe>`, `ready_budget: Option<f64>` (in `new` initialisiert; `load_from` überschreibt beim Installieren
eines Workers). `load_from` (335-386): `ready` binden (heute `_ready`); vor dem Lock:
```rust
let probe = Probe { exe: attempt.exe.clone(), device: ready.model.device.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| attempt.device.clone()) };
let b = budget::measure(Some(&probe), None);
eprintln!("haru: worker ready on {} ({}), session budget {}", probe.device, ready.model.weight_type.as_deref().unwrap_or("?"), b.describe());
```
Unter dem Lock `g.probe = Some(probe); g.ready_budget = b.measured();`. `start_inner` (468-529): `g.probe.clone()` und
`g.ready_budget` zusammen mit dem Sender lesen (470-474); in jeder Schleifeniteration vor `start_with`:
```rust
let mut options = settings.dialogue_options();
let first = budget::measure(probe.as_ref(), fallback);
options.max_seconds = Some(first.seconds);
eprintln!("haru: conversation {id}: {}", first.describe());
let weak = Arc::downgrade(self); let key = id.clone();
let renewal = Renewal {
    max_seconds: Arc::new(move || { let b = budget::measure(probe.as_ref(), fallback); eprintln!("haru: next segment: {}", b.describe()); b.seconds }),
    on_renewed: Arc::new(move |seg| if let Some(c) = weak.upgrade() { c.segment_renewed(&key, seg) }),
};
```
(Clones für die Closures) → `Conversation::start_with(sender, rx, &settings, options, Some(renewal), muted, ...)`.
Neu `pub const SEGMENT_NOTICE: &str = "Neuer Gesprächsabschnitt – Haru fängt frisch an.";` und
`fn segment_renewed(&self, id: &str, _seg: u32)`: sperren; nur wenn `g.state == Talking` und `g.conversation.id() == id`:
`ev = Self::event(&g, None, None)`, `ev.notice = Some(SEGMENT_NOTICE.into())`, `g.last = ev.clone()`, `(self.on_state)(ev)`.
Kein Zustandswechsel. `Self::event` (137) und `Core::new` (85): `notice: None`.
**Zustandsautomat:** keine Änderung an `session/state.rs`, `conversation_ended` (531-565), `worker_lost` (413-445). Die
stale-end-Falle (537-541) greift nicht: `Conversation::id()` und `on_end` (480-481) behalten den stabilen Schlüssel (`c3`),
Worker-Request-ids sind `c3`, `c3-s2`, `c3-s3`. Core baut nie eine Conversation unter seinem Lock ab → `on_renewed`
(sperrt Core vom Dispatch-Thread) kann nicht deadlocken.

3.7 Events ans Frontend: `app/src-tauri/src/ipc.rs:10-18` + `#[serde(skip_serializing_if = "Option::is_none")] pub notice: Option<String>`;
Tests 65-95 `notice: None`; Fixture `"stateEventNotice"` in `contract.fixtures.json` (talking, B580,
`"notice":"Neuer Gesprächsabschnitt – Haru fängt frisch an."`), in `contract_fixtures_match` prüfen.
`app/src/ipc/types.ts:7` `notice?: string | null`; `app/src/ipc/contract.test.ts` `it('notice event carries the notice')`
und `stateEvent` ohne `notice`. `app/src/ui/toast.ts`: `showInfo(text)` wie `showWarning`, Klasse `is-info`,
`INFO_MS = 4000`; `reset`-Union und `classList.remove(...)` um `'is-info'` erweitern. `app/src/styles.css:352-354`:
`.toast.is-info .toast-accent { background: var(--cyan); box-shadow: 0 0 18px 1px rgba(98,230,255,.6); }`,
`.toast.is-info { border-color: rgba(98,230,255,.2); }`. `app/src/app.ts` `onState` (78-83): notice in die Hook-Logzeile;
`if (e.notice && e.state === 'talking' && this.phase === 'talking') this.toast.showInfo(e.notice);` (nicht deduplizieren).
Phase, HUD, Renderer unverändert.

3.8 Doku: README-Env-Tabelle (139-145) `HARU_MAX_SECONDS` (Debug, Sekunden, 10–3600). Neuer README-Abschnitt
„Gesprächsabschnitte“: Länge = (freier Speicher − 512 MiB GPU bzw. 1 GiB CPU) / 500 kB/s, 120–3600 s; danach beginnt Haru
automatisch frisch. Eine Zeile im Doc-Kommentar von `test_hooks.rs`.

---

## Änderung 4: offizielle Charakterkarte

4.1 Assets: Scratchpad-Dateien byte-genau nach `app/src-tauri/assets/haru-cards/{de,en,es,fr,ja,ko,pt,ru,zh}.json` + `LICENSE`
(Quelle: `C:\Users\Andi\AppData\Local\Temp\claude\d2d5f273fea910d4\d2d40dbb-9dfa-4a34-8509-f2c677322252\scratchpad\haru-cards\`).
Dazu `README.md`: Quelle
`https://github.com/higurashi-ai/Haru/tree/7fe6b7665f8bf035998f401f8cb779fbe53f2751/haru/haru/assets/cards`, Apache-2.0,
unverändert, nur das Preset `chat` wird genutzt.

4.2 Neu `app/src-tauri/src/cards.rs` (`pub mod cards;` in `lib.rs`):
```rust
#[derive(Clone, Debug, PartialEq, Deserialize)] #[serde(deny_unknown_fields)]
pub struct Card { pub persona: String, pub name: String, pub backchannel: String, pub relationship: String, pub situation: String }
#[derive(Deserialize)] struct CardFile { chat: Card }
const SOURCES: [(&str, &str); 9] = [("de", include_str!("../assets/haru-cards/de.json")), /* … */];
fn all() -> &'static HashMap<&'static str, Card> { static C: OnceLock<…> = OnceLock::new(); C.get_or_init(|| /* parse, expect("embedded card") */) }
/// The official chat card of `language`; German for a language without one.
pub fn official(language: &str) -> &'static Card
```
Tests: `every_language_has_a_complete_card` (über `settings::LANGUAGES`, alle Felder nicht leer, backchannel ∈ low/mid/high),
`japanese_card_is_named_haru_in_katakana` ("ハル"), `unknown_language_falls_back_to_german`.

4.3 `app/src-tauri/src/settings.rs`: `pub const DEFAULT_NAME: &str = "Haru";` (in `Default`, Z. 38). `dialogue_options` (150-163):
```rust
let card = cards::official(&self.language);
let own = self.name.trim();
let name = if own.is_empty() || own == DEFAULT_NAME { card.name.clone() } else { own.to_string() };
let fit = |t: &str| if name == card.name { t.to_string() } else { t.replace(&card.name, &name) };
DialogueOptions {
    language: Some(self.language.clone()),
    name: Some(name.clone()),
    persona: Some(if self.persona.trim().is_empty() { fit(&card.persona) } else { self.persona.clone() }),
    gender: self.gender.clone(),                                            // None stays omitted
    backchannel: Some(self.backchannel.clone().unwrap_or_else(|| card.backchannel.clone())),
    relationship: Some(fit(&card.relationship)),
    situation: Some(fit(&card.situation)),
    speak_first / temperature / top_k / voice: unverändert,
    ..Default::default()        // max_seconds setzt Core
}
```
Entscheidungen: Kartenname gewinnt, solange der eigene Name leer oder "Haru" ist; sonst gewinnt der eigene Name und
ersetzt den Kartennamen in den Kartentexten. Backchannel None → "mid" der Karte. `other_name` bleibt ungesetzt.
Tests (282-317): `dialogue_options_defaults` gegen `cards::official("de")` (Name Haru, backchannel mid, speak_first true,
kein gender, kein max_seconds); `dialogue_options_omit_empty_name` → `dialogue_options_empty_name_uses_card_name`;
`dialogue_options_include_card_fields` und `dialogue_options_include_changed_sampling` behalten; neu
`dialogue_options_custom_name_replaces_card_name_in_texts`, `dialogue_options_japanese_default_name_becomes_haru_katakana`,
`dialogue_options_japanese_custom_name_wins`, `dialogue_options_gender_none_is_omitted`,
`dialogue_options_backchannel_setting_overrides_card`, `dialogue_options_never_set_max_seconds`.

4.4 UI: `app/src/ui/strings.ts:36` `personaPlaceholder: 'Leer lassen für Harus offizielle Persönlichkeit.'`
(genutzt in `settings.ts:81`). Keine neuen Felder. README :74, :79-82: Karte = offizielle Karte der Sprache; eigener
Name/Persona/Backchannel überschreiben sie.

---

## Verifikation (Repo-Root)
```
cargo test --manifest-path app/src-tauri/Cargo.toml
cargo build --release --manifest-path app/src-tauri/Cargo.toml --examples
npm --prefix app test
npm --prefix app run typecheck
npm --prefix app run build
```

## Manuelle E2E-Prüfung (Arc B580, Vulkan)
0. Falls `build\test\question-de.wav` fehlt: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\make-test-wav.ps1`.
1. **Q8_0 + Budget:** `cargo run --release --manifest-path app/src-tauri/Cargo.toml --example headless_talk -- --seconds 30 --no-playback --wav build\test\question-de.wav`
   (lädt beim ersten Lauf 6 623 102 720 Bytes). `BootInfo.modelPath` endet auf `c372a527…\Haru-4B-Q8_0.gguf`; stderr zeigt
   `worker ready on Vulkan0 (Q8_0), session budget free X B - margin 536870912 B = R s -> N s` und dasselbe für
   `conversation c1`; N = clamp(floor(R), 120, 3600); `RESULT: OK`; Latenz mit Q8_0 notieren; der Ordner
   `…\8db05e86…\Haru-4B-Q6_K.gguf` existiert noch.
   **KV-Messung:** während des Gesprächs `speech.exe devices --json` (Vulkan-Backend) abfragen; die Differenz
   free(nach Laden) − free(während Gespräch) mit dem gesendeten max_seconds vergleichen → tatsächliche Bytes/s
   (inkl. Compute-Puffer). Liegt der Wert deutlich über 500 kB/s, `KV_BYTES_PER_SEC` anpassen bzw. berichten.
2. **Erneuerung:** `$env:HARU_MAX_SECONDS='20'`, gleicher Befehl mit `--seconds 75`, danach `Remove-Item Env:HARU_MAX_SECONDS`.
   Erwartet: `next segment`-Zeilen mit Quelle Override, Abschnitte `c1-s2`, `c1-s3`; ≥ 3 State-Zeilen mit
   `"notice":"Neuer Gesprächsabschnitt – Haru fängt frisch an."`, alle `talking`; Latenzzeilen laufen weiter; kein Fehler,
   kein zweites `worker-*.log` (kein Neuladen); `RESULT: OK`.
3. **UI:** `$env:HARU_MAX_SECONDS='30'; npm --prefix app run tauri dev`, mit Kopfhörern sprechen — cyanfarbener Toast etwa
   alle 30 s; HUD, Fluid und Gesicht laufen weiter; Haru antwortet nach ~1–2 s wieder; Auflegen genau beim Toast kehrt in
   ~1 s ohne Fehler/Bootscreen zurück.
4. **Warnungen:** `app\src-tauri\backends\cpu` nach `%TEMP%\haru-cpu-only\cpu` kopieren, `$env:HARU_BACKENDS_DIR` darauf →
   Toast „Keine Grafikkarte mit mindestens 8 GB…“. Mit normalen Backends und `$env:HARU_BACKEND='cpu'` → alte CPU-Warnung.
5. **Karte:** Sprache `ja` → Haru stellt sich als ハル vor; Persona-Feld zeigt den neuen Platzhalter.

## Risiken / Offenes
- Vulkan-Budget kann kurz noch die freigegebenen KV des alten Abschnitts enthalten → nächster Abschnitt nur kürzer (sicher).
  Treiber ohne `VK_EXT_memory_budget` melden free = Heap-Größe → Überschätzung; der nächste Abschnitt scheitert dann mit
  `out_of_memory` + Fehler-Toast. `out_of_memory` im ersten Abschnitt wird wie heute als Backend-Fehler behandelt.
- CUDA: Probe-Prozess legt eigenen Kontext an (~300 MB) → unterschätzt (sicher). CPU: `ullAvailPhys` optimistisch → 1 GiB Rand.
- Gerätequery blockiert den Dispatch-Thread (meist 0,2–1 s, bis 20 s Timeout); +0,3–1 s bei Start und Boot.
- iGPU mit ≥ 7 GiB Shared Memory qualifiziert weiter für Auto (iGPU-Reihenfolge bleibt), ohne 8-GB-Warnung.
- Namensersetzung ist ein einfaches Substring-Replace. Während eines Gesprächs geänderte Einstellungen erreichen erneuerte
  Abschnitte nicht (gleiche Optionen, gewollt).
- Neue dialogue-Zeile muss vor der neuen id in der Pumpe eingereiht sein (siehe 3.5).
- Nicht gemessen: Zeit bis Frame 0 eines erneuerten Abschnitts; Q8_0-Frame-Latenz auf der B580 (E2E-Schritte 1–2).
