# Haru – Flüssiges Licht (Design-Spezifikation)

| Feld | Wert |
|------|------|
| Datum | 2026-11-20 |
| Status | Genehmigt |
| Repository | `C:\Users\Andi\Documents\repo\haru` |
| Submodul | `extern\speech.cpp` (v0.11.1) |

---

## 1. Ziel

Die Anwendung ist eine Desktop-App, die das Full-Duplex-Sprachmodell **Haru-4B** (GGUF, Quantisierung Q6_K) über **speech.cpp** (github.com/nyosegawa/speech.cpp, MIT, v0.11.1) ausführt. Der Nutzer spricht über ein Live-Mikrofon mit Haru; die Nutzung mit Kopfhörern wird empfohlen.

Harus Sprachausgabe wird visualisiert: Die gesprochenen Wörter erscheinen als leuchtende Tinte in einer GPU-Fluidsimulation („Flüssiges Licht") mit Post-Effekten.

Weitere Eckpunkte:

- **Plattform:** Windows zuerst, portierbar nach Linux und macOS.
- **Modell-Download:** Das Modell wird beim ersten Start heruntergeladen.
- **GPU-Backend:** Automatische Erkennung. CUDA, falls NVIDIA-GPU vorhanden; sonst Vulkan; sonst CPU.

---

## 2. Fakten zum Modell

Die folgenden Eigenschaften stammen aus der Recherche und bilden die Grundlage für die Auslegung.

### 2.1 Verarbeitung

- Haru ist ein Full-Duplex-Speech-to-Speech-Modell unter Apache-2.0.
- Unterstützte Sprachen: de, en, es, fr, ja, ko, pt, ru, zh.
- Alle **133 ms (7,5 Hz)** verarbeitet das Modell einen Frame:
  - Eingabe: ein Frame Nutzer-Audio mit 3200 Samples bei 24 kHz.
  - Ausgabe: ein Frame eigenes Audio mit 3200 Samples bei 24 kHz im Wertebereich [-1, 1].
  - Zusätzlich der Text, den Haru in diesem Frame zu sprechen begonnen hat. Dieser ist oft leer, bei Stille als `" "`.
- Es gibt **keine Wort-Timings**. Die kleinste zeitliche Granularität ist der Frame.
- Multibyte-Zeichen können einen Frame später erscheinen als ihr Beginn.
- Haru spricht nur, während es Audio erhält. Stille bzw. Nullen sind gültige Eingabe und müssen durchgehend gesendet werden.
- Mit `speak_first = true` eröffnet Haru das Gespräch.

### 2.2 Optionen

| Option | Bedeutung |
|--------|-----------|
| `language` | Pflicht für die Charakterkarte; keine automatische Erkennung |
| `name` | Name des Charakters |
| `persona` | Freitext-Persona |
| `gender` | `female` oder `male` |
| `backchannel` | `low`, `mid` oder `high` |
| `other_name` | Name des Gesprächspartners |
| `relationship` | Beziehung zum Gesprächspartner |
| `situation` | Gesprächssituation |
| `voice` | WAV-Datei, 3 s bis 3 min, optional |
| `temperature` | Standard 0,7 |
| `top_k` | Standard 25 |
| `top_p` | Sampling-Parameter |
| `seed` | Zufallsstartwert |
| `guidance_scale` | Standard 1,5 |
| `max_seconds` | Standard 3600 |
| `speak_first` | Haru eröffnet das Gespräch |

### 2.3 Modelldatei

| Eigenschaft | Wert |
|-------------|------|
| Repository | `sakasegawa/Haru-4B-GGUF` |
| Revision | `fc026182f7bbfaa0400b2d44358a5e9e327b957f` |
| Datei | `Haru-4B-Q6_K.gguf` |
| Größe | 5605707520 Bytes |
| SHA256 | `8db05e86acca046bc183d4e30fbda22ac775ba4767ae5a10bcd0d48f8d7db793` |
| URL | `https://huggingface.co/{repo}/resolve/{revision}/{file}` |

### 2.4 Upstream-Messungen und Risiko

- Q6_K benötigt laut Upstream ca. **54 ms pro Frame** (Apple M5, Metal).
- Vulkan ist upstream nur auf NVIDIA geprüft.
- Die Intel Arc B580 des Entwicklungsrechners ist **ungetestet**. Dies ist das größte technische Risiko, weshalb der Machbarkeitstest (Meilenstein M0) vorgezogen wird.

---

## 3. Architektur

### 3.1 Überblick

Die Anwendung ist eine **Tauri-v2-App** mit zwei Teilen:

- **UI:** TypeScript, reines WebGL2, gebaut mit Vite.
- **Rust-Kern:** Module `model_fetch`, `backend`, `worker`, `audio`, `session`, `settings`.

Der Rust-Kern startet `speech worker` (das speech.cpp-Executable der gewählten Backend-Variante) als Kindprozess und spricht dessen JSON-Lines-Protokoll über stdin/stdout.

UI und Rust-Kern kommunizieren über **Tauri-Commands** (UI → Kern) und einen **`tauri::ipc::Channel`** (Kern → UI, Visualisierungs-Feed mit ca. 60 Hz).

```
+-------------------------------------------------------------+
|  Tauri-v2-App                                               |
|                                                             |
|  UI (TypeScript, WebGL2, Vite)                              |
|    Fluid-Sim · Wortpfade · Post-FX · Overlays               |
|        ^                              |                     |
|   tauri::ipc::Channel          Tauri-Commands               |
|   (~60 Hz Viz-Feed)                   v                     |
|  Rust-Kern: model_fetch · backend · worker · audio          |
|             session · settings                              |
+-----------------------------------|-------------------------+
                                    | stdin/stdout (JSON Lines)
                                    v
                 speech worker (Kindprozess, Backend-Variante)
                                    |
                                    v
                       Haru-4B-Q6_K.gguf (Modell)
```

### 3.2 Begründung der Prozess-Isolation

Der Worker läuft als eigener Prozess. Die Gründe:

- Kein eigenes C++/FFI in der App nötig.
- Treiber- oder GPU-Abstürze reißen die UI nicht mit.
- Backend-Fallback ist durch einen Neustart mit einer anderen Variante möglich.
- Das Protokoll ist upstream getestet.
- Die Lösung ist portabel.

---

## 4. Komponenten

### 4.1 model_fetch (Rust)

**Zielpfad.** Das Modell wird im speech.cpp-Cache-Layout abgelegt, damit CLI und App es gemeinsam nutzen:

```
<cache_root>/speech.cpp/models/sakasegawa--Haru-4B-GGUF/<sha256>/Haru-4B-Q6_K.gguf
```

- `cache_root` entspricht exakt `extern/speech.cpp/tools/catalog/cache.cpp`:
  - Windows: `%LOCALAPPDATA%`
  - Linux: `$XDG_CACHE_HOME`, sonst `~/.cache`
  - macOS: `~/Library/Caches`
- Die Umgebungsvariable `SPEECH_MODEL_DIR` (absoluter Pfad) überschreibt den models-Ordner.

**Ablauf.**

1. Existiert die Datei mit exakter Größe, wird sie sofort verwendet. Sie wird nur nach erfolgreicher Hash-Prüfung umbenannt.
2. Andernfalls wird in `<file>.part` heruntergeladen:
   - HTTP-Range-Resume: Antwort `206` → an die vorhandene Datei anhängen; Antwort `200` → neu beginnen.
   - SHA256 wird inkrementell berechnet. Bei Resume wird der bereits vorhandene Teil zuerst gehasht.
   - Vor dem Download erfolgt eine Speicherplatzprüfung (Restgröße + 64 MiB).
3. Nach Abschluss wird der Hash geprüft und die Datei atomar umbenannt.
4. Bei Hash-Mismatch wird `.part` gelöscht und ein Fehler gemeldet.

**Events.**

| Event | Nutzlast |
|-------|----------|
| `progress` | `{downloaded, total, bytesPerSec}` |
| `verifying` | `{done, total}` |
| `done` | `{path}` |
| `error` | `{message}` |

### 4.2 backend (Rust)

**Varianten.** Jede Variante liegt in einem Unterordner `backends/<name>/` mit dem Executable `speech` (unter Windows `speech.exe`). Varianten: `cuda`, `vulkan`, `metal`, `cpu`.

**Suchreihenfolge für das Backend-Verzeichnis:**

1. Umgebungsvariable `HARU_BACKENDS_DIR`
2. Tauri-Resource-Dir (installierte Version)
3. Entwicklungspfad `app/src-tauri/backends`

**CUDA-Probe.** Die Bibliothek `nvcuda.dll` (Windows) bzw. `libcuda.so.1` (Linux) wird per `libloading` geladen. Die Probe ist erfolgreich, wenn `cuInit(0) == 0` und `cuDeviceGetCount > 0`.

**Geräteermittlung.** Die Geräte werden über `speech devices --json` der jeweiligen Variante abgefragt.

**Auswahl des Ausführungsplans.** Die reine Funktion

```
choose_plan(probe, variants, devices, overrides) -> [Versuch {variant, device}]
```

liefert eine geordnete Liste von Versuchen. Die Reihenfolge lautet:

1. **Override zuerst:** `HARU_BACKEND` / `HARU_DEVICE` bzw. die Einstellungen.
2. **cuda**, sofern die Probe erfolgreich ist und die Variante vorhanden ist. Gerät: CUDA-Gerät mit dem meisten Speicher.
3. **vulkan**. Gerät: diskrete GPU mit dem meisten Speicher; falls keine vorhanden, die integrierte GPU.
4. **metal**.
5. **cpu** als letzter Versuch (Gerät `cpu`), mit Warnung, dass dies langsam ist.

**Sitzungsstart.** Der Start probiert die Versuche der Reihe nach:

1. Worker starten.
2. Auf `ready` warten.
3. `dialogue start` senden.
4. Ersten Frame innerhalb des Timeouts empfangen.

Schlägt ein Schritt fehl, folgt der nächste Versuch. Das gewählte Backend und Gerät wird als Badge angezeigt, z. B. „Vulkan · Intel Arc B580".

**Hinweis.** speech.cpp linkt ggml statisch und nimmt sonst die erste GPU. Deshalb werden eigene Varianten gebaut, statt `GGML_BACKEND_DL` zu verwenden. Eine Änderung an speech.cpp ist nicht nötig.

### 4.3 worker (Rust)

**Start.** Der Worker wird mit

```
speech worker <model-path> --device <name> [--add-voice <name>=<wav>]
```

gestartet. Die exakte CLI ist in `extern/speech.cpp/docs/worker.md` definiert. Unter Windows wird das Konsolenfenster mit `CREATE_NO_WINDOW` unterdrückt. stderr wird in eine Logdatei geschrieben.

**Protokoll (JSON Lines).**

Vom Worker empfangen:

```json
{"type":"ready","protocol":N, ...}
```

Vom Client gesendet:

```json
{"type":"dialogue","id":"...","sample_rate":24000, "<Optionen>":"..."}
{"type":"chunk","id":"...","seq":0,"pcm":"<base64 int16 LE mono>"}
{"type":"hang_up","id":"..."}
{"type":"cancel","id":"..."}
```

Vom Worker gesendet:

```json
{"type":"chunk","id":"...","seq":0,"pcm":"<base64>","text":"..."}
{"type":"end","id":"...","frames":0,"samples":0}
{"type":"error","id":"...","code":"...","option":"...","message":"..."}
```

Das Feld `text` ist im Worker-Chunk immer vorhanden.

### 4.4 audio (Rust, cpal)

**Capture.**

- Eingabe über das Standardgerät oder das gewählte Eingabegerät.
- Beliebige Sampleformate werden in mono f32 gewandelt und in einen Ringpuffer (2 s) geschrieben.
- Ein Mic-Pump-Thread sendet ca. 20-ms-Chunks (int16, base64) mit der Mikrofonrate an den Worker. Der Worker resampelt selbst.
- **Mute** sendet Nullen. Haru benötigt durchgehend Eingabe.

**Playback.**

- Worker-Frames mit 24 kHz werden auf die Ausgaberate resampelt und in einen Ringpuffer geschrieben.
- Der Output-Callback liest aus dem Ringpuffer und zählt die gespielten Samples.
- Jitter-Puffer: Ein Frame wird vor dem Start vorgepuffert. Bei Unterlauf wird neu vorgepuffert.

**Feature-Analyse.**

- Jeder Modell-Frame wird in 8 Blöcke zu je 400 Samples (16,7 ms, ca. 60 Hz) zerlegt.
- Pro Block: RMS und 16 logarithmische Bänder (FFT).
- Die Werte werden mit der Ausgabe-Sampleposition gestempelt.
- Mikrofon: RMS und 8 Bänder pro Chunk.

### 4.5 session (Rust)

Die Komponente orchestriert Start, Stopp und Mute sowie folgende Teilprozesse:

- Mic-Pump
- Worker-Reader
- Frame-Timeline mit Einträgen `{seq, text, startSample}`

**Viz-Feed** (ca. 60 Hz über den Channel):

```
{t, model{rms, bands[16]}, mic{rms, bands[8]}, latencyMs, words:[{seq, text}]}
```

- Der Text eines Frames wird genau dann gemeldet, wenn sein Frame hörbar wird. So bleiben Wort und Ton synchron.

**State-Events** (zusätzlich):

```
{state, backend, device, error?}
```

**Zustände:** `booting` → `downloading` → `loading` → `ready` → `talking` → `error`.

**Latenz-Policy.**

```
Rückstand = gesendete Mic-Zeit − (empfangene Frames × 133 ms) + gepufferte Wiedergabe
```

- Rückstand > 0,5 s: Warnung.
- Rückstand > 1,0 s (einstellbar): Gespräch wird beendet, mit Fehlermeldung (analog zur speech-CLI).

### 4.6 settings (Rust)

Die Einstellungen werden als JSON im App-Config-Ordner gespeichert. Standardwerte:

| Schlüssel | Standard |
|-----------|----------|
| `language` | `"de"` |
| `name` | `"Haru"` |
| `persona` | `""` |
| `gender` | `null` |
| `backchannel` | `null` |
| `speakFirst` | `true` |
| `temperature` | `0.7` |
| `topK` | `25` |
| `voicePath` | `null` |
| `inputDevice` | `null` |
| `outputDevice` | `null` |
| `backend` | `"auto"` |
| `device` | `"auto"` |
| `quality` | `"auto"` |
| `subtitles` | `false` |
| `lagLimitSecs` | `1.0` |

Nur explizit gesetzte Optionen werden an den Worker übergeben.

### 4.7 UI – Visualisierung „Flüssiges Licht" (TypeScript, WebGL2)

**Fluid-Simulation** (Stable Fluids, Dobryakov-Stil):

- Geschwindigkeitsfeld ca. 256², Farbfeld als HDR (RGBA16F) bis 2048².
- Schritte: Advektion, Divergenz, Curl, Vorticity Confinement, Druck (Jacobi), Gradient-Subtraktion, Splat, Masken-Splat, Ruhemaske.

**Qualitätsstufen.** Sim- und Dye-Auflösung je Stufe:

| Stufe | Sim | Dye |
|-------|-----|-----|
| low | 128 | 512 |
| medium | 192 | 1024 |
| high | 256 | 1440 |
| ultra | 384 | 2048 |

Die Stufe `auto` regelt adaptiv über die Framezeit (gleitender Mittelwert, EMA):

- Framezeit > 20 ms für 2 s → Stufe senken.
- Framezeit < 12 ms für 5 s → Stufe erhöhen.

**Wörter.**

- **WordAssembler:** Text-Stücke pro Frame werden zu Wörtern zusammengesetzt und als Ereignisse `wordStart`, `wordAppend`, `wordEnd` ausgegeben.
  - Grenze bei führendem Leerzeichen, bei `" "` (Stille) oder bei Satzzeichen.
  - CJK ohne Leerzeichen: Wortende bei Satzzeichen, nach ≥ 2 leeren Frames oder nach 8 Zeichen.
- **GlyphRasterizer:** OffscreenCanvas mit 2D-Kontext. Systemschriften decken alle Schriftsysteme ab; für Latein wird die gebündelte Schrift Inter Variable verwendet. Ergebnis ist eine R8-Textur.
- **WordPath:** Langsam driftende Lissajous-Bahn, die die letzten Wortboxen meidet.
- **WordInjector-Lebenszyklus:**

| Phase | Dauer | Wirkung |
|-------|-------|---------|
| ink | 0–250 ms | starke Farbinjektion |
| hold | 600–1200 ms (je nach Länge) | schwache Nachinjektion und Ruhemaske, damit das Wort lesbar ist |
| release | danach | Ruhemaske aus; Wirbel-Splats entlang der Glyphenkanten aus dem Masken-Gradienten, das Wort zerfließt |

- Teilwörter wachsen mit, während sie gesprochen werden.

**Audio-Kopplung.**

- Harus RMS steuert die Vorticity (Basis 20, Zuwachs bis +40) sowie Splats um das aktuelle Wort.
- Der spektrale Schwerpunkt bestimmt den Farbton. Haru erhält warme Gold- und Magentatöne.
- Das Mikrofon erzeugt kühle Cyan- und Blau-Splats vom unteren Rand („Haru hört zu").
- Im Leerlauf entstehen langsame Ambient-Splats.

**Post-Effekte.**

- Bloom (Dual-Kawase, Threshold 0,6, Soft-Knee)
- God Rays aus der Luminanz der Tinte
- „Flüssigglas"-Shading: Normalen aus dem Farbgradienten, Spekular-Licht
- Chromatische Aberration, proportional zu RMS
- Film Grain
- Vignette
- ACES-Tonemapping
- Dithering

**Overlays (HTML/CSS).**

- **Boot-Screen:** Backend-Erkennung; Download-Fortschritt als glühende Linie mit Bytes, Geschwindigkeit und Restzeit; Verifizieren. Die Fluidsimulation reagiert auf die Download-Rate.
- **Kopfhörer-Hinweis** beim ersten Start.
- **Start-Button.**
- **HUD:** Backend-Badge, Latenz, Mikrofonpegel, Mute [M], Einstellungen [S], Vollbild [F].
- **Untertitel** (optional).
- **Fehler-Toast** mit Retry.

---

## 5. Projektstruktur

```
haru/
├── extern/speech.cpp/                 Submodul, v0.11.1
├── scripts/
│   ├── build-backends.ps1             Windows-Build der Varianten
│   └── build-backends.sh              POSIX-Build der Varianten
├── app/
│   ├── src/                           TypeScript-Frontend
│   │   ├── gl/                        WebGL2-Hilfsschicht
│   │   ├── fluid/                     Fluid-Simulation
│   │   ├── words/                     WordAssembler, GlyphRasterizer, WordPath, WordInjector
│   │   ├── audio/                     Feature-Verarbeitung für die Anzeige
│   │   ├── post/                      Post-Effekte
│   │   └── ui/                        Overlays und HUD
│   └── src-tauri/
│       ├── backends/                  Gebaute Varianten (nicht versioniert)
│       └── src/
│           ├── paths.rs
│           ├── model_fetch.rs
│           ├── backend/
│           ├── worker/
│           ├── audio/
│           ├── session.rs
│           ├── settings.rs
│           ├── commands.rs
│           └── error.rs
├── .github/workflows/build.yml        CI-Build
└── docs/                              Dokumentation und Spezifikationen
```

**Build-Skripte.** `scripts/build-backends.ps1` und `scripts/build-backends.sh` bauen `speech-cli` je Variante nach `build/speech-<variante>`. Anschließend wird `speech` (bzw. `speech.exe`) nach `app/src-tauri/backends/<variante>/` kopiert. Für die Variante `cuda` werden zusätzlich die CUDA-Runtime-DLLs `cudart`, `cublas` und `cublasLt` kopiert.

---

## 6. Build und Packaging

### 6.1 Vulkan-Variante (Windows, wie upstream-CI)

```
cmake -A x64 -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded -DGGML_VULKAN=ON \
      -DGGML_NATIVE=OFF -DGGML_OPENMP=OFF -DBUILD_SHARED_LIBS=OFF
cmake --build . --target speech-cli
```

Voraussetzung: Vulkan SDK.

### 6.2 CUDA-Variante

- Gleiche Optionen wie bei Vulkan, jedoch `-DGGML_CUDA=ON` statt `-DGGML_VULKAN=ON`.
- Voraussetzung: CUDA Toolkit 12.x.
- **Lokal nicht gebaut**, da keine NVIDIA-GPU vorhanden ist. Der Build erfolgt in GitHub Actions:
  - Runner: `windows-2025`
  - Vulkan SDK: `1.4.357.0`
  - CUDA: über `Jimver/cuda-toolkit`
- Upstream bezeichnet den CUDA-Build als „baut, ungeprüft". Die Fallback-Kette mildert dieses Risiko.

### 6.3 Paketierung

- Tauri bündelt `backends/**` als Ressourcen.
- Auslieferung als NSIS-Installer.

---

## 7. Fehlerbehandlung

| Situation | Reaktion |
|-----------|----------|
| Download-Fehler | Retry mit Resume |
| Hash-Mismatch | Modell neu laden |
| Keine Backend-Variante vorhanden | Meldung mit Hinweis auf den Build |
| Worker-Absturz oder Worker-Fehler | Nächster Backend-Versuch; ist kein Versuch mehr übrig, Fehlerzustand mit stderr-Auszug |
| Kein Mikrofon | Meldung an den Nutzer |
| Rückstand > Limit | Gespräch beenden; Erklärung (GPU zu langsam oder ausgelastet) |

---

## 8. Tests

### 8.1 Rust-Unit-Tests

- Protokoll-Serialisierung und -Deserialisierung
- PCM-Base64-Codec
- `model_fetch` gegen einen lokalen Test-HTTP-Server:
  - frischer Download
  - Resume mit `206`
  - Server ignoriert Range und antwortet mit `200`
  - Hash-Mismatch
- `choose_plan`-Tabellentests
- Resampler
- Latenzrechnung

### 8.2 TypeScript-Tests (vitest)

- `WordAssembler`: deutsche Subwörter, Satzzeichen, Stille, CJK
- `WordPath`
- Qualitätsregler

### 8.3 Integration

- Rust-Example `dialogue_smoke`: startet echten Worker mit echtem Modell, spielt eine WAV-Datei in Echtzeit ab und gibt pro Frame den Text sowie die Rechenzeit aus.
- App-Lauf mit Screenshots.

---

## 9. Meilensteine

| Meilenstein | Inhalt | Ziel |
|-------------|--------|------|
| M0 Machbarkeit | speech.cpp mit Vulkan bauen; Q6_K laden; Offline-Dialog per WAV; Rechenzeit pro Frame auf Arc B580 messen | p50 < 100 ms; muss < 133 ms sein |
| M1 Rust-Kern | model_fetch, backend, worker, audio, session, settings | Funktionierender Gesprächsablauf ohne UI |
| M2 Visualisierung | Fluid-Simulation, Wortpfade, Audio-Kopplung, Post-Effekte, Overlays | Visualisierung läuft mit Live-Daten |
| M3 Integration | Gesamtintegration, CI, README | Lauffähiges Installationspaket |

M0 wird vor allen anderen Meilensteinen umgesetzt, da die Machbarkeit auf der Intel Arc B580 die größte Unsicherheit darstellt.

---

## 10. Risiken

| Risiko | Auswirkung | Gegenmaßnahme |
|--------|------------|---------------|
| Haru auf Intel Arc über Vulkan ungetestet | Gespräch möglicherweise zu langsam | Machbarkeitstest M0 zuerst; Fallback-Kette |
| CUDA upstream ungeprüft | CUDA-Variante funktioniert eventuell nicht | Fallback auf Vulkan oder CPU |
| Echo ohne Kopfhörer | Haru hört die eigene Stimme | Kopfhörer-Hinweis beim ersten Start |
| WebGL2-Performance in WebView2 | Ruckeln der Visualisierung | Adaptive Qualitätsstufen |
| 5,6-GB-Download | Lange Wartezeit beim ersten Start | Resume, Fortschrittsanzeige, Hash-Prüfung |

---

## 11. Nicht im Umfang (YAGNI)

- Echo-Cancellation
- Stimmaufnahme-UI (die Stimme wird nur als WAV-Pfad angegeben)
- Gesprächsverlauf und Export
- Auto-Update
- Linux- und macOS-Pakete (die Anwendung ist portabel ausgelegt und es existiert ein Build-Skript, mehr nicht)
- Mehrere parallele Gespräche
