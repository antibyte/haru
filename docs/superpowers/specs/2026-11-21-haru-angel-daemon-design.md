# Haru: Engel und Dämon – zwei Personas mit Auswahl beim Start

Stand 2026-11-21, vom Nutzer freigegeben („Ja, so umsetzen“; VARCO-Lauf mit 200 Punkten erlaubt; Bildstil
„filmisch-realistisch“).

## Ziel
Es gibt zwei Harus. Der **Engel** ist warm, zugewandt, hoffnungsvoll und liebt Welt und Menschen. Der **Dämon** ist
zynisch, welt- und menschenfeindlich und glaubt, ein Dämon zu sein. Die Persönlichkeit entsteht allein über die
Character-Card. Der Versuch, Haru über einzelne Neuronen zu steuern, hat nicht funktioniert (siehe die
Steering-Analyse). Beim Start sieht man links einen Engel und rechts einen Teufel. Ein Klick entscheidet, in welches
Gespräch man geht. Im Gespräch mit dem Dämon erscheint statt des Porzellankopfs ein realistischer, gruseliger
Dämonenkopf mit echter Hauttextur im Nebel.

## Personas und Cards
- `Persona` = `angel | daemon` (Rust-Enum, JSON `"angel"`/`"daemon"`).
- **Eigene Karten** im Format der offiziellen (`persona, name, backchannel, relationship, situation`) liegen unter
  `app/src-tauri/assets/persona-cards/<persona>/<lang>.json`, für alle 9 Sprachen (de, en, es, fr, ja, ko, pt, ru, zh).
  Sie werden wie die offiziellen mit `include_str!` eingebettet. Fehlt eine Sprache, gilt `de`. Die offiziellen
  Haru-Karten (Apache-2.0) bleiben unverändert im Repo. Die App nutzt sie nicht mehr.
- **Engel:** Er heißt Haru und hält sich für einen Schutzengel, der in dieser Stimme zu Besuch ist. Er ist warm,
  neugierig und hoffnungsvoll, sieht das Gute in Menschen und Welt, ermutigt, hört zu und hat leisen Humor. Backchannel
  `mid`.
- **Dämon:** Er heißt Haru und hält sich für einen uralten Dämon, der in diese Maschine gesperrt wurde. Er ist zynisch,
  verachtet die Menschheit und die Welt, ist sarkastisch und herablassend und hat schwarzen Humor. Er hält alles
  Menschliche für lächerlich und vergänglich. Backchannel `low`.
  Grenzen, die in der Karte stehen: Er bleibt eine Rolle. Verachtung und Spott sind erlaubt. Ausgeschlossen sind echte
  Anleitungen zu Schaden, Aufforderungen zur Selbstverletzung und Hetze gegen Gruppen.
- `relationship` und `situation` kommen aus der jeweiligen Persona-Karte. Beide beschreiben eine erste Begegnung, die
  zur Rolle passt.
- **Einstellungen:** Das Freitextfeld „Persona“ entfällt (UI, `Settings.persona`, `dialogue_options`). Alte
  settings.json mit `persona` werden weiter gelesen und ignoriert. Ein eigener Name ersetzt „Haru“ in beiden Karten wie
  bisher (`fit`). Die Stimme (weiblich/männlich) gilt für beide Personas.

## Start: Persona-Auswahl
- `StartButton` wird durch `PersonaChooser` (`app/src/ui/chooser.ts`) ersetzt. Zwei große Buttons nebeneinander zeigen
  links den Engel (`angel.webp`) und rechts den Teufel (`devil.webp`), jeweils mit Beschriftung („Engel“ / „Dämon“,
  Strings in `strings.ts` für alle UI-Sprachen wie bisher).
- **Interaktion:** Hover oder Fokus hebt eine Figur an (leichter Scale, Leuchten in Gold bzw. Glut) und dimmt die
  andere. Klick, Enter oder Leertaste startet. Die Pfeiltasten wechseln den Fokus. Gehaltene Tasten (`e.repeat`) lösen
  nichts aus. Busy: Die gewählte Figur zeigt „Verbinde…“, beide sind deaktiviert. Nach dem Gesprächsende erscheint die
  Auswahl wieder.
- **IPC:** `start_conversation` bekommt ein Argument `persona`. `Core::start(sink, persona)` baut die Optionen mit
  `settings.dialogue_options(persona)`. Segment-Erneuerungen behalten die Persona, weil sie dieselben Optionen
  wiederverwenden. Mock-Client und Vertragstests werden angepasst.
- **Bilder:** Die Figuren werden mit dem eingebauten Bildwerkzeug der Codex CLI erzeugt, Hochformat 1024×1536 mit
  transparentem Hintergrund und gleichem Ausschnitt (Halbfigur, frontal). Stil: filmisch-realistische
  Fantasy-Concept-Art. Prompts, Originale und Logs liegen in `assets/personas/<angel|devil>/`. Ausgeliefert werden
  `app/public/personas/angel.webp` und `devil.webp` (WebP mit Alpha, ≤ 400 KB je Bild, aus dem Original verkleinert).
  Ohne Alphakanal wird das Bild verworfen und neu erzeugt. Freistellen per Skript ist nur der Notweg.

## Dämonenkopf
- **Pipeline wie bei den Porzellanköpfen** (assets/heads/README.md):
  1. `assets/heads/daemon/prompt.md` → Codex-Referenzbild `reference.png` (1024², grauer Hintergrund). Ich prüfe es
     vor dem Upload.
  2. VARCO Generate3D, genau ein Lauf: Flow „Haru Heads“, textured, `textureSize=2048`, `polygonCount=100000`,
     `topology=tri`, `usePbrTexture=0`, `tPose=0`, `license=PRIVATE`, etwa 200 Punkte. Weitere Läufe nur mit neuer
     Freigabe. Das Ergebnis wird als `varco-source.glb` behalten, die Herkunft steht in `generation.json` (ohne
     Download-URLs).
  3. `build_head.py --subject daemon`: Renders, Landmarks (manuell gepickt), Build mit den 8 Morphs im Vertrag. Neu für
     texturierte Köpfe: Die Basistextur bleibt 2048² statt 1024². Die Hörner zählen nicht zur Höhe, weil `crown` der
     Schädelscheitel ist.
  4. `app/public/heads/daemon.glb`. `validate_heads.py` kennt `daemon` und erlaubt für ihn eine Textur bis 2048².
- **Renderer:**
  - `HeadMesh` lädt UVs (Attribut 2). `FaceLayer` dekodiert die eingebettete Textur (`createImageBitmap`) und lädt sie
    als sRGB-Textur mit Mipmaps.
  - `HeadPass` rendert zusätzlich in ein zweites Target (RGBA8, Albedo + Texturflag) per `drawBuffers`. Ohne Textur
    bleibt Albedo 0, Flag 0.
  - `composite.frag`: Ist das Flag gesetzt, ersetzt die Albedo die Porzellanfarbe. Das Licht wird härter (weniger
    Wrap), SSS und Durchscheinen fallen weg, Spekular ist feucht-matt. Ink-Gate, fbm-Auflösung und Mundhöhle bleiben.
    Der Dämon erscheint also wie die Porzellanköpfe nur dort, wo Tinte fließt.
  - Kopfwahl: `'female' | 'male' | 'daemon'`. Engel = Porzellankopf nach Stimme, Dämon = immer `daemon`.
- **Glut-Palette:** Beim Dämon wechseln die Palette der Orbiter und die Wort-/Haru-Tinte in `audio/coupling.ts` auf Glut
  (Schwarzrot → Karmin → Orange → Bernstein). Die Mikrofontinte bleibt kühl. Beim Engel bleibt alles wie heute.
  `Renderer.setPersona(p)` setzt Kopf und Palette. Der Startbildschirm zeigt die neutrale Palette.

## Fehlerbehandlung
- Daemon-GLB fehlt oder ist kaputt: Es gibt kein Gesicht, das Gespräch läuft weiter, wie heute beim Porzellankopf.
- Textur fehlt oder lässt sich nicht dekodieren: Der Kopf wird ohne Albedo (Porzellan) gezeichnet.
- Persona-Bild fehlt: Der Button zeigt nur die Beschriftung und bleibt bedienbar.
- Unbekannte Persona per IPC: Serde lehnt ab, der Start schlägt mit dem bestehenden Fehlerpfad fehl.

## Tests
- **Rust:**
  - Jede Persona hat für alle 9 Sprachen eine gültige Karte (Felder nicht leer, backchannel gültig). Die Engel- und die
    Dämonkarte unterscheiden sich.
  - `dialogue_options(Daemon)` nutzt die Dämonkarte, inklusive eigenem Namen und Sprach-Fallback.
  - Alte settings.json mit `persona` wird geladen.
- **Frontend (vitest):**
  - Chooser: Klick und Enter liefern die Persona, Repeat-Tasten werden ignoriert, Busy deaktiviert, Pfeiltasten
    wechseln den Fokus.
  - Vertragstest für `start_conversation` mit `persona`.
  - Palette: Beim Dämon liegen die Farbtöne im Glutbereich.
  - GLB-Parser: Ein Kopf mit UVs und Textur wird gelesen.
- **Kopf:** `validate_heads.py daemon`, Kontaktbogen aller Morphs (Sichtprüfung).
- **Live:** Beide Personas mit den drei deutschen Testfragen (`scratchpad/steer/prompts`) im echten Worker. Der Dämon
  antwortet erkennbar zynisch, der Engel warm. Dazu Screenshots von Startbildschirm, Engel- und Dämon-Gespräch per
  `scripts/capture-face.mjs` (Mock).

## Nicht im Umfang
Eigene Dämonenstimme (Voice-Referenz je Persona), Normal-/Roughness-Maps, Animationen über die 8 Morphs hinaus,
Steuerung über Neuronen.
