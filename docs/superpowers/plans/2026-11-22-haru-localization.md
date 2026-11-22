# Haru: Sprache aus dem Betriebssystem und lokalisierte Oberfläche – Umsetzungsplan

Stand 2026-11-22. Vom Nutzer freigegeben: „Ja, ganze Oberfläche“.

**Ziel:** Beim ersten Start kommt die Sprache aus dem Betriebssystem statt immer „de“. Die ganze Oberfläche gibt es in den
9 Sprachen de, en, es, fr, ja, ko, pt, ru, zh. Ein Sprachwechsel in den Einstellungen beschriftet die sichtbare
Oberfläche sofort neu. Ein Feld `settings.language` steuert weiter Oberfläche **und** Gesprächssprache (Karte, Worker).

**Fertig heißt:** `cargo test`, `npm run typecheck` und `npm test` in `app/` laufen grün; keine neuen Clippy-Warnungen.
Nichts committen.

Zeilennummern beziehen sich auf den Stand vor der Umsetzung (geprüft).

## 0. Fakten und Korrekturen
- `Settings::default()` bleibt „de“ (Test `defaults_match_spec`, settings.rs:214). Die Erkennung greift in `load()`.
- `sys-locale` ist neu (nicht in Cargo.lock).
- `ErrorCode` existiert und wird serialisiert (error.rs:3, types.ts:4-5, tauri-client.ts:9-21), ist aber zu grob für
  einen Text. Deshalb ein feiner `key` neben `code`.
- Deutscher Backend-Text kommt auch außerhalb von Fehlern: `StateEvent.warning`/`notice` (ipc.rs:17-19),
  `BootInfo.warning` (ipc.rs:59), `DownloadEvent::Error.message` (ipc.rs:28, gefüllt model_fetch.rs:108, angezeigt
  boot.ts:71). Quellen: session.rs:35-49, :404-408 (Wörter aus :490 „abgestürzt“, :597 „fehlgeschlagen“), session.rs:611,
  conversation.rs:29, mic_pump.rs:21 (gesendet :200).
- Weitere deutsche Texte im Frontend: tauri-client.ts:20 `'Unerwarteter Fehler.'`; mock-client.ts:69, 99, 110, 145, 148,
  178; settings.ts:25-26 (`de1`/`de2` mit Komma), :94 (Sprachauswahl-Default 'de'), :199 (zeigt `err.message`);
  model_fetch.rs:87-89 (`gb()` mit Komma).
- app.ts:81-83 und :146-148 vergleichen Warnungen als Strings → künftig `sameText`.
- Ungenutzte Keys: strings.ts:9 `boot.starting`, :15 `start.label`, :26 `toast.close` → entfernen.
- `hud.unmute` ('Stumm', strings.ts:19) ist die Beschriftung im stummen Zustand (hud.ts:116) → umbenennen in `hud.muted`.
- `showFatal` (main.ts:31) läuft, wenn `new App(...)` wirft, also vor den Einstellungen → Sprache in `main()` setzen,
  bevor `App` gebaut wird.
- vitest läuft standardmäßig in node (vitest.config.ts:4); DOM-Tests brauchen `// @vitest-environment jsdom`.

## 1. Entscheidungen
- **Backend-Meldungen: Schlüssel statt Text.** `HaruError` bekommt `key` (+ optionale `params`) neben `code`.
  `message` bleibt Deutsch (Logs, technische Details, Fallback). Das Frontend zeigt `S.messages[key]` mit den Params.
  `HaruError::new(code, key: MsgKey)` statt String – der Compiler findet jede Aufrufstelle. Warnungen und Hinweise nutzen
  dieselbe Form `UiText {key, params, message}`.
- **Live-Wechsel:** ES-Modul-Live-Binding `export let S` + `setLanguage()`. Jede Komponente bekommt `relabel()`, das
  `App` aufruft. Kleiner Helfer `Labels` registriert statische Beschriftungen als Getter.
- **Wann:** nach erfolgreichem Speichern in `App.applySettings`, nicht bei jeder Auswahländerung.
- **Nicht übersetzt (Konstanten):** `APP_NAME = 'Haru'`, `LANGUAGE_NAMES` (Eigenbezeichnungen, aus strings.ts:59-61),
  `BACKEND_NAMES` (aus strings.ts:65; ersetzt das Duplikat format.ts:40), Tastenbuchstaben M/S/F.
- **Einheiten:** B/KB/MB/GB, /s, ms, s/min/h bleiben Symbole; Zahlen über `Intl.NumberFormat` der aktiven Sprache.
- **Fehlende/kaputte settings.json:** beide Fälle nutzen die erkannte Sprache.
- **Nicht unterstützte Sprache:** Englisch. zh = vereinfachtes Chinesisch, auch für zh-Hant.
- **Sprachtabellen in zwei Schritten:** Schritt A legt `de.ts` und `en.ts` an, `TABLES` ist zunächst
  `Partial<Record<Language, Strings>>` mit Fallback auf `en`. Schritt B ergänzt es, fr, ja, ko, pt, ru, zh und macht
  `TABLES` zu `Record<Language, Strings>`.
- Installer (NSIS, tauri.conf.json) ist nicht im Umfang.

## 2. Gemeinsamer Vertrag
JSON in camelCase, `params` entfällt, wenn leer:
- `HaruError`: `{code, key, params?, message, detail}`
- `UiText`: `{key, params?, message}`
- `StateEvent.warning`: `UiText|null`, `StateEvent.notice?`: `UiText|null`, `BootInfo.warning`: `UiText|null`
- `DownloadEvent` Fehler: `{"event":"error","data":UiText}`
- Param-Werte: String oder Zahl. Zahlen werden mit genau einer Nachkommastelle in der aktiven Sprache gezeigt.

`app/src/ipc/contract.fixtures.json` (gehört dem Backend-Umsetzer):
- `stateEventNotice`: `"notice":{"key":"segmentRenewed","message":"Neuer Gesprächsabschnitt – Haru fängt frisch an."}`
- `stateEventError`: error = `{"code":"noBackend","key":"noBackend","message":"Keine Backend-Variante gefunden.","detail":null}`
- `downloadError`: `{"event":"error","data":{"key":"downloadFailed","message":"Der Download ist fehlgeschlagen. Bitte Internetverbindung prüfen."}}`
- neu `stateEventWarning`: `{"state":"ready","backend":"vulkan","device":"Intel(R) UHD Graphics","error":null,"warning":{"key":"smallGpu","params":{"device":"Intel(R) UHD Graphics"},"message":"Intel(R) UHD Graphics hat weniger als 8 GB Grafikspeicher – Haru braucht mindestens 8 GB, das Gespräch kann abbrechen."}}`
- neu `errorDiskSpace`: `{"code":"diskSpace","key":"diskSpace","params":{"need":7.5,"free":2.5},"message":"Nicht genug Speicherplatz: 7,5 GB benötigt, 2,5 GB frei.","detail":null}`
- neu `messageKeys`: die 62 Keys aus §5.3 in dieser Reihenfolge.
- `settingsDefault` bleibt („de“).

## 3. Backend (app/src-tauri)
- Cargo.toml: `sys-locale = "0.3"`. lib.rs: `pub mod locale;`, `pub mod messages;`.
- **`src/locale.rs`:**
  ```rust
  pub const FALLBACK_LANGUAGE: &str = "en";
  pub fn os_locales() -> Vec<String>                       // sys_locale::get_locales().collect()
  pub fn match_language(tag: &str) -> Option<&'static str> // Primär-Subtag: split an - _ . @, lowercase, in settings::LANGUAGES
  pub fn pick_language(locales: impl IntoIterator<Item = impl AsRef<str>>) -> &'static str
  ```
  `pick_language` teilt jeden Eintrag zusätzlich an ':' (Linux `LANGUAGE`), nimmt den ersten Treffer, sonst "en".
- **`src/messages.rs`:** Makro `msg_keys!{ Variant => "deutsche Vorlage", ... }` erzeugt
  `#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)] #[serde(rename_all="camelCase")] pub enum MsgKey`,
  `pub const ALL: &[MsgKey]`, `pub fn german(self) -> &'static str`. Dazu:
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize)] #[serde(untagged)] pub enum ParamValue { Text(String), Number(f64) } // From<&str>, From<String>, From<f64>
  pub type Params = BTreeMap<&'static str, ParamValue>;
  pub fn render(template: &str, p: &Params) -> String  // {name}; Number als {:.1} mit Komma
  #[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all="camelCase")]
  pub struct UiText { pub key: MsgKey, #[serde(skip_serializing_if="BTreeMap::is_empty")] pub params: Params, pub message: String }
  impl UiText { pub fn new(key: MsgKey) -> Self; pub fn with_param(self, name: &'static str, v: impl Into<ParamValue>) -> Self }
  ```
- **error.rs:8-28:** `HaruError` + `pub key: MsgKey`, `#[serde(skip_serializing_if="BTreeMap::is_empty")] pub params: Params`.
  `pub fn new(code: ErrorCode, key: MsgKey) -> Self` (message = key.german()),
  `pub fn with_param(self, name, v) -> Self` (rendert message neu), `pub fn text(&self) -> UiText`. error.rs:32 → `MsgKey::IoError`.
- Hilfsfunktionen mit `&str` → `MsgKey`: capture.rs:20, audio/devices.rs:7, playback.rs:96, paths.rs:50, settings.rs:115,
  conversation.rs:713 (`lag_error`). Alle `HaruError::new`-Aufrufe laut §5.3.
- **ipc.rs:** :17 `warning: Option<UiText>`, :19 `notice: Option<UiText>`, :28 `Error(UiText)`, :59
  `BootInfo.warning: Option<UiText>`. model_fetch.rs:108 → `DownloadEvent::Error(e.text())`. model_fetch.rs:165-167 →
  `HaruError::new(DiskSpace, MsgKey::DiskSpace).with_param("need", need as f64/1e9).with_param("free", available as f64/1e9)`;
  `gb()` (:87-89) entfällt.
- **session.rs:** Konstanten :35-49 entfallen; `note_text(..) -> UiText` (SmallGpu mit Param `device`). :342
  `switched: Option<(String, MsgKey)>`. :404-411 `UiText::new(why).with_param("from", ..).with_param("to", ..)`.
  :490 `MsgKey::BackendCrashedSwitched`, :597 `MsgKey::BackendFailedSwitched`. :152, :164, :168, :175 `Option<UiText>`.
  :538 `Arc<dyn Fn(UiText)+Send+Sync>`. :611 `UiText::new(MsgKey::SegmentRenewed)`. :617 `text: UiText`.
- **conversation.rs:** :29 (Konstante weg), :524, :783, :847, :859, :863, :1008, :1020-1021 `String` → `UiText`.
- **mic_pump.rs:** :21 weg, :53, :65, :200 `UiText::new(MsgKey::AecFailed)`.
- **settings.rs:**
  ```rust
  pub fn load(path: &Path) -> Settings { load_with(path, crate::locale::os_locales) }
  pub fn load_with(path: &Path, os_locales: impl FnOnce() -> Vec<String>) -> Settings
  pub fn sanitized(self, os_locales: impl FnOnce() -> Vec<String>) -> Settings
  ```
  Fehlende Datei (:79) und kaputte/unlesbare Datei (:80-83, :87-92) → über einen Helfer
  `Settings { language: pick_language(os_locales()).into(), ..Default::default() }`, erkannte Sprache per `eprintln!`
  loggen. `sanitized`: ist `language` nicht in `LANGUAGES`, dann
  `match_language(lang).unwrap_or_else(|| pick_language(os_locales()))` (lazy: OS nur bei Bedarf fragen).
  `Settings::default()` und `validate()` bleiben bis auf die `MsgKey`-Argumente.

## 4. Frontend (app/src)
- **ipc/types.ts:**
  ```ts
  export type Language = typeof LANGUAGES[number];
  export const MESSAGE_KEYS = [/* 62 Keys aus §5.3, gleiche Reihenfolge */] as const;
  export type MessageKey = typeof MESSAGE_KEYS[number];
  export type MessageParams = Record<string, string | number>;
  export interface UiText { key: MessageKey; params?: MessageParams; message: string }
  export interface HaruError { code: ErrorCode; key?: MessageKey | 'unexpected'; params?: MessageParams; message: string; detail: string | null }
  ```
  `StateEvent.warning: UiText|null`, `notice?: UiText|null` (:7); DownloadEvent-Fehler `{ event: 'error'; data: UiText }`
  (:12); `BootInfo.warning: UiText|null` (:35). `DEFAULT_SETTINGS.language` bleibt 'de'.
- **ipc/tauri-client.ts:13-21:** `toHaruError` übernimmt `key` nur, wenn er in `MESSAGE_KEYS` liegt, `params` nur als
  einfaches Objekt; Fallback-Fehler mit `key: 'unexpected'`.
- **ipc/mock-client.ts:** :69, :99, :145, :148 key `busy`; :110 `noBackendBuild`; :178 `lagLimit`; `err()` (:200) nimmt
  `(code, key, detail?)`.
- **ui/locales/de.ts (neu):**
  ```ts
  export type DeepString<T> = { readonly [K in keyof T]: T[K] extends string ? string : DeepString<T[K]> };
  const messages = { /* §5.3 + unexpected */ } as const satisfies Record<MessageKey | 'unexpected', string>;
  export const de = { boot: {...}, ..., messages } as const;
  export type Strings = DeepString<typeof de>;
  ```
  `messages` als eigene Konstante (ein `satisfies` auf oberster Ebene würde die anderen Keys bemängeln).
  `ui/locales/en.ts` usw.: `import type { Strings } from './de'; export const en: Strings = {...}`.
  `ui/locales/index.ts`: `export const TABLES` (siehe §1, zwei Schritte).
- **ui/strings.ts (neu geschrieben):**
  ```ts
  export const APP_NAME = 'Haru';
  export const LANGUAGE_NAMES: Record<Language, string>;
  export let S: Strings;                         // = TABLES.de bis setLanguage
  export function getLanguage(): Language;
  export function resolveLanguage(code: string | null | undefined): Language; // Primär-Subtag wie in Rust, sonst 'en'
  export function setLanguage(code: string | null | undefined): Language;     // S = Tabelle; document.documentElement.lang (typeof document prüfen)
  export function fill(template: string, params?: MessageParams): string;     // {name}; Zahlen → formatNumber(v, 1)
  export function uiText(t: UiText): string;
  export function errorText(e: Pick<HaruError, 'key' | 'params' | 'message'>): string; // key ? fill(S.messages[key]) : message
  export function sameText(a?: UiText | null, b?: UiText | null): boolean;   // key + params gleich
  export function formatNumber(n: number, digits: number): string;           // gecachtes Intl.NumberFormat(lang,{min/max digits, useGrouping:false})
  export function formatPercent(pct: number): string;                        // Intl percent, 0 Stellen; pct 0..100
  export class Labels { text<E extends HTMLElement>(el: E, get: () => string): E; attr<E extends Element>(el: E, name: 'title'|'aria-label'|'placeholder', get: () => string): E; apply(): void }
  ```
- **ui/format.ts:** `de()` (:3-5) weg; `formatBytes`/`formatRate` über `formatNumber`; `export const BACKEND_NAMES` (:40);
  `formatEta` behält die Symbole.
- **Komponenten:** `BootScreen.setStatus(key: BootStatus)` mit
  `export type BootStatus = 'detecting'|'loading'|'downloading'|'verifying'`; `ErrorToast.show(e: HaruError, onRetry?)`,
  `showWarning(t: UiText)`, `showInfo(t: UiText)`; jede Komponente `relabel(): void`.
- **app.ts:** :37 `lastWarning: UiText | null`, Vergleich mit `sameText` (:81-83, :146-148); :79 `e.warning?.key` loggen;
  :91/:127 Status-Keys; `applySettings` (:299-304):
  `if (resolveLanguage(s.language) !== getLanguage()) { setLanguage(s.language); this.relabel(); }`;
  neues `private relabel()` → boot, hint, chooser, hud, settingsPanel, toast.
- **main.ts:** `async function initLanguage(client: HaruClient)` = `setLanguage((await client.getSettings()).language)`,
  bei Fehler 'en'. `main()` (:24-29) baut den Client, wartet auf `initLanguage`, dann `new App`. `showFatal` (:16-19)
  nutzt `S.fatal.webgl2` und `fill(S.fatal.graphics, {detail: msg})`.
- **index.html:2:** `lang="en"` (wird beim Start überschrieben).

## 5. String-Keys
### 5.1 Bestehende Keys (strings.ts:3-65)
- boot: detecting, loading, downloading, verifying
- hint: text, ok
- start: busy
- chooser: title, angel, angelSub, daemon, daemonSub
- hud: mute, muted (statt `unmute`), settings, fullscreen, stop, latency, mic
- toast: retry, details
- settings: title, save, cancel, saving, reloadNote, sections.{dialogue, audio, engine, display}, language, name, gender,
  backchannel, speakFirst, temperature, temperatureHint, topK, voicePath, voicePathPlaceholder, inputDevice,
  outputDevice, echoCancellation, echoCancellationHint, backend, device, quality, subtitles, showFace, lagLimit, seconds,
  default, auto, none
- gender: female, male; backchannel: low, mid, high; quality: auto, low, medium, high, ultra
- Entfernt: boot.starting, start.label, toast.close. Als Konstanten: appName, languages, backends.

### 5.2 Neue UI-Keys
- `boot.remaining` = "noch {eta}" (boot.ts:53)
- `boot.verifyingProgress` = "Prüfe Modell… {pct}" (ersetzt das replace() in boot.ts:61)
- `fatal.webgl2` = Text aus main.ts:18
- `fatal.graphics` = "Grafik konnte nicht gestartet werden: {detail}" (main.ts:19)
- `messages.unexpected` = "Unerwarteter Fehler." (nur Frontend, tauri-client.ts:20)

### 5.3 `messages.*` (Rust `MsgKey` = PascalCase, Reihenfolge von `ALL` und Fixture)
Fehler:
1. internal: "Interner Fehler." (commands.rs:19, session.rs:99)
2. busy: "Gerade nicht möglich." (session.rs:103, state.rs:24)
3. ioError: "Dateifehler" (error.rs:32)
4. settingsSaveFailed: "Einstellungen konnten nicht gespeichert werden." (settings.rs:105)
5. languageUnsupported: "Diese Sprache wird nicht unterstützt." (settings.rs:133)
6. temperatureRange: "Die Temperatur muss zwischen 0,3 und 1,0 liegen." (:136)
7. topKRange: "Top-K muss zwischen 0 und 500 liegen." (:139)
8. lagLimitRange: "Die Verzögerungsgrenze muss zwischen 0,5 und 10 Sekunden liegen." (:143)
9. backendUnknown: "Unbekanntes Backend." (:148)
10. qualityUnknown: "Unbekannte Qualitätsstufe." (:151)
11. genderUnknown: "Unbekanntes Geschlecht." (:155)
12. backchannelUnknown: "Unbekannte Rückmeldungsstärke." (:160)
13. voiceFileMissing: "Die Stimmdatei wurde nicht gefunden." (:165)
14. modelDirRelative: "SPEECH_MODEL_DIR muss ein absoluter Pfad sein." (paths.rs:68)
15. modelDirUnknown: "Der Modellordner konnte nicht bestimmt werden." (paths.rs:75)
16. audioDevicesUnreadable: "Audiogeräte konnten nicht gelesen werden." (audio/devices.rs:15, 20, 41, 66)
17. noMicrophone: "Kein Mikrofon gefunden." (devices.rs:36, 49)
18. inputDeviceNotFound {name}: "Mikrofon „{name}“ wurde nicht gefunden." (devices.rs:51)
19. noOutputDevice: "Kein Ausgabegerät gefunden." (devices.rs:62)
20. outputDeviceNotFound {name}: "Ausgabegerät „{name}“ wurde nicht gefunden." (devices.rs:72)
21. micOpenFailed: "Das Mikrofon konnte nicht geöffnet werden." (capture.rs:64, 88)
22. micFormatUnsupported: "Das Mikrofon-Format wird nicht unterstützt." (capture.rs:85)
23. micStartFailed: "Das Mikrofon konnte nicht gestartet werden." (capture.rs:91)
24. outputOpenFailed: "Das Ausgabegerät konnte nicht geöffnet werden." (playback.rs:171, 193)
25. outputFormatUnsupported: "Das Ausgabeformat wird nicht unterstützt." (playback.rs:190)
26. outputStartFailed: "Das Ausgabegerät konnte nicht gestartet werden." (playback.rs:196)
27. audioStartFailed: "Audio konnte nicht gestartet werden." (engine.rs:56, 65)
28. audioConvertFailed: "Audio konnte nicht umgewandelt werden." (resample.rs:21)
29. audioDeviceLost: "Audiogerät nicht mehr verfügbar." (conversation.rs:737-740)
30. micNoData: "Das Mikrofon liefert keine Daten." (mic_pump.rs:182)
31. noBackendBuild: "Keine Backend-Variante gefunden. Baue sie mit scripts/build-backends.ps1 oder setze HARU_BACKENDS_DIR." (backend/mod.rs:28-31)
32. noBackend: "Keine Backend-Variante gefunden." (session.rs:285)
33. backendDevicesInvalid: "Die Geräteliste des Backends ist ungültig." (backend/devices.rs:47)
34. backendDevicesFailed: "Die Geräte des Backends konnten nicht abgefragt werden." (backend/devices.rs:68)
35. noBackendStarted: "Kein Backend konnte gestartet werden." (plan.rs:172)
36. downloadStartFailed: "Der Download konnte nicht gestartet werden." (model_fetch.rs:47)
37. downloadFailed: "Der Download ist fehlgeschlagen. Bitte Internetverbindung prüfen." (model_fetch.rs:130)
38. diskSpace {need, free in GB}: "Nicht genug Speicherplatz: {need} GB benötigt, {free} GB frei." (model_fetch.rs:165-167)
39. downloadRejected: "Der Download-Server hat die Anfrage abgelehnt." (model_fetch.rs:220)
40. downloadCorrupt: "Die heruntergeladene Datei ist beschädigt. Bitte erneut versuchen." (model_fetch.rs:285)
41. workerConnectionLost: "Die Verbindung zum Sprachmodell-Worker ist abgebrochen." (process.rs:81, conversation.rs:555)
42. workerStartFailed: "Der Sprachmodell-Worker konnte nicht gestartet werden." (process.rs:134)
43. workerExited: "Der Sprachmodell-Worker wurde unerwartet beendet." (process.rs:189, session.rs:479)
44. workerNotReady: "Der Sprachmodell-Worker wurde nicht rechtzeitig bereit." (process.rs:194)
45. workerBadReply: "Unerwartete Antwort vom Sprachmodell-Worker" (protocol.rs:94)
46. workerIncompatible: "Das Modell oder der Worker ist nicht kompatibel." (protocol.rs:105)
47. workerError: "Der Sprachmodell-Worker meldet einen Fehler." (protocol.rs:114)
48. workerBadAudio: "Ungültige Audiodaten vom Worker" (pcm.rs:25, 27)
49. workerBadFrame: "Der Worker lieferte ein ungültiges Audio-Frame." (conversation.rs:580)
50. conversationAborted: "Das Gespräch wurde abgebrochen." (conversation.rs:640)
51. workerNoAnswer: "Der Sprachmodell-Worker antwortet nicht." (conversation.rs:682-685, 701, 990)
52. workerUnresponsive: "Der Sprachmodell-Worker reagiert nicht mehr." (conversation.rs:1031, 1069)
53. lagLimitBacklog {backlog in s}: "Haru kommt nicht hinterher (Rückstand {backlog} s). Die GPU ist zu langsam oder ausgelastet." (conversation.rs:715)
54. lagLimit: "Haru kommt nicht hinterher. Die GPU ist zu langsam oder ausgelastet." (conversation.rs:716)

Warnungen und Hinweise:
55. cpuSlow: "Läuft auf der CPU – das Gespräch kann zu langsam sein." (session.rs:35)
56. noCapableGpu: "Keine Grafikkarte mit mindestens 8 GB gefunden – Haru läuft auf der CPU und kann zu langsam sein." (session.rs:38-39)
57. smallGpu {device}: "{device} hat weniger als 8 GB Grafikspeicher – Haru braucht mindestens 8 GB, das Gespräch kann abbrechen." (session.rs:45-48)
58. backendCrashedSwitched {from, to}: "Backend {from} abgestürzt – gewechselt auf {to}." (session.rs:406 + :490)
59. backendFailedSwitched {from, to}: "Backend {from} fehlgeschlagen – gewechselt auf {to}." (session.rs:406 + :597)
60. segmentRenewed: "Neuer Gesprächsabschnitt – Haru fängt frisch an." (session.rs:37)
61. aecUnavailable: "Echounterdrückung nicht verfügbar – bitte Kopfhörer verwenden." (conversation.rs:29)
62. aecFailed: "Echounterdrückung ausgefallen – bitte Kopfhörer verwenden." (mic_pump.rs:21)

Hinweise für Übersetzungen: Zahlen in 6-8 synchron mit settings.rs:17-18, :138-141 halten. Jeden `{platzhalter}`
unverändert übernehmen. Persona-Namen: Engel/Dämon sinngemäß (en Angel/Demon). „Haru“ bleibt „Haru“.

## 6. Neu beschriften, Komponente für Komponente
- **BootScreen (boot.ts):** :24 Wortmarke `APP_NAME` (kein relabel). :25 Status über gespeicherten `statusKey`. :30 Meta:
  letztes `DownloadEvent` speichern, Darstellung in `renderMeta(e)`; Glättung bleibt in `onDownload` (:45). :53
  `boot.remaining`, :61 `boot.verifyingProgress` mit `formatPercent`, :71 `uiText(e.data)`. `relabel()` zeichnet Status
  und Meta neu.
- **HeadphoneHint (hint.ts):** :24 Text als Feld, :28 Button; `relabel()` setzt beide.
- **PersonaChooser (chooser.ts):** :4-7 `ORDER` nur `{persona, img}`; Beschriftungen über Getter `labelOf(p)`/`subOf(p)`
  zur Laufzeit. Felder für :21 Titel, :30 aria-label, :40 Label, :43 Untertitel je Persona. `busy: Persona | false` in
  `setBusy` (:69) speichern; :76 liest `S.start.busy` bzw. `labelOf()` zur Laufzeit. `relabel()` respektiert `busy`.
- **Hud (hud.ts):** :40 Latenz-Title, :42 Meter-Title (Feld), Labels aus `mk` (:61-65): settings, fullscreen, stop;
  Mute-Label (:116) über gespeichertes `muted` aus `setMuted`. Badge (:90) bleibt sprachunabhängig.
- **ErrorToast (toast.ts):** :28, :58, :67 über `errorText`/`uiText`. Referenzen auf Meldung, Details-Button (:34) und
  Retry-Button (:45) halten; `current = {kind:'error', e} | {kind:'warning'|'info', t} | null`. `relabel()` aktualisiert
  sichtbare Inhalte, ohne Timer zurückzusetzen.
- **SettingsPanel (settings.ts):** :62 aria-label (Titel), :68 h2, :71 ×-Button aria-label (cancel), :89 Placeholder;
  :94 Default `getLanguage()`. :99-126 Abschnitte, Labels und die 3 Hinweise (:106, :113, :120): `section`/`field`/
  `toggle`/`slider` (:258-284) nehmen `() => string` und registrieren bei `Labels`; Getter lesen `S.settings.x`, nicht das
  gecachte `T` (:76). :131 cancel. Save-Button (:134, :177, :192, :202) über `saving`-Flag. Auswahllisten (:155-168,
  :235) → `fillChoices(current: Settings, a: AudioDevices)`; `this.audio` in `open()` merken; relabel ruft sie mit
  `this.collect(this.base)`, damit ungespeicherte Änderungen bleiben. :199 Status: `HaruError` speichern, mit
  `errorText` zeigen. :249-250 `formatNumber(…, 2)` und `formatNumber(…, 1) + S.settings.seconds`; `de1`/`de2` (:25-26)
  weg. `relabel()` = `labels.apply()`, `fillChoices` (wenn `base`), `syncRanges`, Save-Label, Status. Auch geschlossen
  neu beschriften.
- Nicht betroffen: subtitles.ts (nur gesprochene Wörter), keys.ts, Debug-Overlay (renderer.ts:397-401).

## 7. Tests
**Rust neu:**
- locale.rs: `pick_language_returns_first_supported_in_order` (["fr-CA","de-AT"] → fr),
  `pick_language_reads_primary_subtag_of_any_form` (pt-BR, zh-Hant-TW, en_GB.UTF-8, de_DE@euro, "JA"),
  `pick_language_skips_unsupported_and_posix` (["C","nl-NL","ko-KR"] → ko), `pick_language_falls_back_to_english`
  ([] und ["nl"] → en), `pick_language_splits_colon_lists` ("nl:de" → de).
- messages.rs: `all_keys_are_unique`, `german_message_renders_params` (diskSpace „7,5“; inputDeviceNotFound),
  `params_omitted_when_empty`, `with_param_rerenders_message`.
- settings.rs: `missing_file_detects_os_language` (["ja-JP"] → ja, sonst Default),
  `missing_file_with_unsupported_locale_is_english`, `stored_language_wins_over_os_locale` (Closure panikt, falls
  aufgerufen), `hand_edited_language_tag_is_normalized` ("DE-at" → de, Closure nicht aufgerufen),
  `unsupported_stored_language_is_replaced_by_detected` ("xx" mit ["es-MX"] → es, keine .bad-Datei),
  `corrupt_file_uses_detected_language`.

**Rust ändern:** settings.rs:229-233, :271-278 → `load_with(&p, || vec!["de-DE".into()])`; :290 zusätzlich
`e.key == MsgKey::TemperatureRange`; :311 Closure an `sanitized`. ipc.rs:67-105: :77, :80, :89 anpassen, plus
`stateEventWarning`, `errorDiskSpace`, `messageKeys == MsgKey::ALL` serialisiert. plan.rs:443, :455, :465, :469-470
`MsgKey::WorkerStartFailed` (Detailzeilen enthalten dessen deutschen Text; `key == NoBackendStarted`). budget.rs:226
`MsgKey::NoBackend`. mic_pump.rs:412 `vec![UiText::new(MsgKey::AecFailed)]`. model_fetch.rs:503
`DownloadEvent::Error(t)` mit `t.key == DownloadCorrupt`; :547 prüft die Params.

**vitest neu:**
- `src/ui/strings.test.ts` (jsdom): every locale has exactly the German key set; no locale has empty strings;
  placeholders match German in every locale; messages cover every Rust key plus unexpected (gegen
  `fixtures.messageKeys`); resolveLanguage maps tags and falls back to en; setLanguage sets document lang; fill formats
  numeric params per language (de "2,5", en "2.5"); errorText falls back to message without key; sameText compares key
  and params.
- `src/ui/relabel.test.ts` (jsdom): BootScreen, HeadphoneHint, PersonaChooser, Hud, SettingsPanel (mit Fixture-Daten
  geöffnet), ErrorToast (Fehler mit Detail und Retry) bauen, `setLanguage('en')` + `relabel()`; kein rein deutscher Text
  bleibt in text/title/aria-label/placeholder; settings relabel keeps unsaved select values; saving label follows
  language; chooser busy label survives relabel.

**vitest ändern:** format.test.ts `beforeEach(() => setLanguage('de'))`, plus en-Fälle ('6.62 GB', '42.1 MB/s') und
`formatPercent`. chooser.test.ts `setLanguage('de')` in `beforeEach`, :37/:39 gegen `TABLES.de.start.busy` /
`TABLES.de.chooser.daemon`. contract.test.ts :19 `error?.key === 'noBackend'`, :26 `n.notice?.key === 'segmentRenewed'`,
plus `MESSAGE_KEYS equals fixtures.messageKeys`, Warnungs- und Fehler-Params, `toHaruError keeps key/params and drops
unknown key`, `fallback uses 'unexpected'`.

## 8. Fallstricke und Reihenfolge
1. `S` nie destrukturieren, nie `S.x` auf Modulebene oder in langlebigen Werten cachen (chooser.ts:5-6, `T` in
   settings.ts:76/:153).
2. Warnungs-Deduplizierung nur mit `sameText`, sonst erscheint der Toast bei jedem State-Event neu.
3. Intl nutzt geschützte Leerzeichen (U+00A0, U+202F), z. B. de „42 %“. Tests normalisieren oder prüfen exakt.
4. Speicherreihenfolge `saveSettings` → `applySettings` → `relabel` läuft bei offenem Drawer; dessen `finally` (:202)
   liest dann das neue `S` – korrekt nur mit Live-Binding.
5. Sprachwechsel mitten im Gespräch: Die Oberfläche wechselt sofort, das laufende Gespräch behält seine Karte bis zum
   nächsten Start (session.rs:530).
6. `setLanguage` prüft `typeof document` (format.test läuft in node).
7. `main()` wartet auf `initLanguage` vor `new App`, sonst blitzt Deutsch auf.
8. Eine erkannte Sprache wird beim nächsten Speichern geschrieben; das erste Speichern ist die Bestätigung des
   Kopfhörer-Hinweises (app.ts:160).
9. `message` bleibt Deutsch (Logs backend/mod.rs:51, Details plan.rs:169). Die Oberfläche nutzt `key`, wenn vorhanden.

**Arbeitsteilung:** Backend-Umsetzer besitzt `app/src-tauri/**` und `app/src/ipc/contract.fixtures.json`.
Frontend-Umsetzer besitzt `app/src/**` (außer der Fixture) und `app/index.html`. Gemeinsam: §2 und die Key-Liste §5.3.
Abweichungen fallen auf: Rust `ALL` ⇄ Fixture (cargo test), Fixture ⇄ `MESSAGE_KEYS` (vitest), `MESSAGE_KEYS` ⇄
`de.messages` (tsc), `de` ⇄ andere Tabellen (tsc + Laufzeittest). Beide Hälften gehören zusammen: Schickt das Backend
`warning`/`notice` als Objekte, zeigt das alte Frontend „[object Object]“.
