# Haru: Echounterdrückung (AEC) für den Betrieb mit Lautsprechern

Stand 2026-11-21, vom Nutzer freigegeben („Ja, so umsetzen“).

## Ziel
Haru hört und spricht gleichzeitig (Full-Duplex). Mit Lautsprechern landet Harus eigene Stimme im Mikrofon, und das Modell
hört sich selbst (speech.cpp-Doku: „with speakers the model hears itself“). Die App soll Harus Stimme aus dem
Mikrofonsignal entfernen, bevor es ans Modell geht, so wie Chrome es im funktionierenden Linux-Setup (agollm) über
`getUserMedia({echoCancellation, noiseSuppression, autoGainControl})` tut. Unterbrechen und Rückmeldungen („mhm“) müssen
erhalten bleiben; kein Halbduplex.

## Entscheidung: sonora
- `sonora` 0.3 (BSD-3-Clause, reines Rust, kein C/C++-Build): Portierung des WebRTC-Audio-Processing-Moduls (M145 + Fixes
  bis M156) mit AEC3, Rauschunterdrückung, AGC2 und Hochpass; laut Projekt besteht es die C++-Referenztestsuite; Windows
  x86_64 in CI; SSE2/AVX2. MSRV 1.91 (installiert: rustc 1.96).
- Verworfen: Windows Voice-Capture-DSP (nur Windows, geräteabhängig), Halbduplex-Gating (zerstört Full-Duplex),
  `aec3`-Crate (laut Autor WIP, API im Umbau), `webrtc-audio-processing` (C++-Build unter MSVC ungelöst).
- Risiko junge Bibliothek → in einem eigenen Modul `audio/aec.rs` gekapselt; der Rest der App kennt nur dessen API.

## Architektur
**Referenzsignal (Render-Tap).** Der Ausgabe-Callback (`audio/playback.rs`) schreibt jeden tatsächlich abgespielten
Mono-Wert — inklusive Stille in Prebuffer und Underruns — zusätzlich in einen zweiten lock-freien Ringpuffer (rtrb,
~2 s). Volle Ringe verwerfen still und zählen. Damit bekommt die AEC genau das, was der Lautsprecher abstrahlt, mit der
Geräte-Zeitbasis der Ausgabe.

**Verarbeitung im Mikrofon-Thread** (`session/mic_pump.rs`). Ein `VoiceProcessor` (in `audio/aec.rs`) gehört allein dem
Pump-Thread; sonora verlangt `&mut self` für Render und Capture, daher kein Lock. Pro 20-ms-Mikrochunk (2 × 10 ms):
1. alle im Render-Tap verfügbaren Samples an `push_render` (intern auf Capture-Rate resampelt, in 10-ms-Frames an
   `process_render_f32`);
2. Verzögerungshinweis setzen (`set_stream_delay_ms`), dann die beiden 10-ms-Mikroframes in place mit
   `process_capture_f32` bereinigen;
3. danach wie bisher: Stummschaltung, Pegelanalyse (jetzt auf dem bereinigten Signal = was Haru hört), Chunk an den Worker.
Solange das Gespräch noch nicht gestartet ist (`!started`), werden beide Ringe geleert, damit kein Rückstand entsteht.

**Raten.** sonora läuft auf der Capture-Rate (Mono, Render und Capture gleich konfiguriert). Hat das Ausgabegerät eine
andere Rate, resampelt `VoiceProcessor` die Referenz mit rubato (bereits Abhängigkeit) auf die Capture-Rate. Capture-
Raten, die nicht durch 100 teilbar sind (z. B. 22 050 Hz), lassen keine exakten 10-ms-Frames zu → AEC für dieses
Gespräch aus, einmalige Warnung.

**Verzögerung.** Hinweis = Ausgabelatenz + Eingabelatenz aus den cpal-Callback-Zeitstempeln (playback − callback bzw.
callback − capture, geglättet) plus Wartezeit im Capture-Ring minus Wartezeit im Render-Tap; ohne Zeitstempel 40 ms;
geklemmt auf 0–500 ms. AEC3 schätzt die tatsächliche Verzögerung selbst; der Hinweis muss nur grob stimmen.

**Stufen.** Hochpass + AEC3 (Default) + Rauschunterdrückung (moderat) + AGC2 (adaptive digitale Verstärkung mit Limiter).
Genaue Felder/Stufen beim Umsetzen gegen die sonora-Quellen prüfen.

**Einstellung.** `echoCancellation: bool` in den Einstellungen, Standard an (fehlt das Feld in einer alten
settings.json → an). Aus = Mikrofonsignal unverändert (bitgenau wie heute). Wirkt ab dem nächsten Gesprächsstart.
UI-Schalter „Echounterdrückung“ mit kurzem Hinweis. Ohne echte Wiedergabe (headless `no_playback`, WAV-Feeder) gibt es
keine Referenz → AEC aus.

**Kopfhörer-Hinweis.** Neuer Text: Kopfhörer klingen am besten; über Lautsprecher sorgt die Echounterdrückung dafür, dass
Haru sich nicht selbst hört.

## Fehlerbehandlung
- Initialisierung unmöglich (Rate nicht durch 100 teilbar, sonora-Fehler) → Rohsignal, einmalige Warnung im UI
  (bestehender Warn-Pfad), Gespräch läuft.
- Fehler von `process_*` während des Gesprächs → `VoiceProcessor` für den Rest des Gesprächs aus, Rohsignal, einmalige
  Warnung „Echounterdrückung ausgefallen – bitte Kopfhörer verwenden.“
- Render-Tap voll/leer → verwerfen bzw. nichts nachreichen (AEC3 verträgt Lücken; Ausrichtung schätzt es selbst nach).
- Echtzeit: keine Allokation im Audio-Callback (Tap ist ein vorab angelegter Ring); Verarbeitung nur im Pump-Thread
  (~15 µs pro 10 ms laut sonora-Benchmark).

## Tests
- **Echo-Dämpfung (automatisch, deterministisch):** synthetisches, sprachähnliches Fernsignal (nicht-stationäre
  Bursts/Chirps), Echo = Faltung mit synthetischer Raumimpulsantwort (abklingendes Rauschen, ~100 ms, Gain 0,5) plus 60 ms
  Verzögerung. Nur-AEC-Konfiguration (ohne NS/AGC). Nach Einschwingen (letzte 3 s von 8 s) ERLE ≥ 20 dB.
- **Doppelsprechen:** zusätzlich unabhängiges Nahsignal im Mikro → bleibt erhalten (Energie im Doppelsprechabschnitt
  nicht stärker als ~6 dB gedämpft, Korrelation mit dem Nahsignal hoch).
- **Unit-Tests:** Aus-Schalter = bitgenaues Passthrough; 20-ms-Chunk = 2 Frames; Rate nicht durch 100 teilbar → aus;
  Render-Resampling 44,1 → 48 kHz; Render-Tap erhält genau die gespielten Samples inkl. Prebuffer-Stille; Settings-Default
  und alte settings.json ohne Feld → an; Volle-Kette-Smoke-Test (AEC+NS+AGC+HPF) ohne Fehler.
- **E2E:** bestehende Tests und Headless-Lauf bleiben grün. Den echten Lautsprechertest macht der Nutzer auf dem AMD-PC.

## Nicht im Umfang
Stimmreferenz (Voice-Datei), Diagnose-Log, Erkennung von Kopfhörern, Einstellung der einzelnen Stufen.
