# Engel und Dämon – Umsetzungsplan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Haru gibt es als Engel und als Dämon. Beim Start wählt man per Klick auf das Engel- bzw. Teufelsbild, und der
Dämon erscheint im Gespräch als realistischer Kopf im Nebel.

**Architecture:** Die Persona (`angel | daemon`) wird beim Klick gewählt und als Argument an `start_conversation`
übergeben. Rust wählt danach die passende eingebettete Character-Card. Das Frontend schaltet über
`Renderer.setPersona()` den Kopf (Porzellan oder `daemon.glb`) und die Tintenpalette (neutral oder Glut) um. Der
Kopf-Pass bekommt ein zweites Render-Target für die Albedo texturierter Köpfe. Das Composite nutzt sie nur, wenn ein
Texturflag gesetzt ist. Porzellanköpfe sehen daher exakt aus wie bisher.

**Tech Stack:** Rust (Tauri v2, serde), TypeScript (Vite, vitest), WebGL2/GLSL ES 3.0, Blender 5.3 (headless),
Codex CLI (Bildwerkzeug), VARCO 3D (MCP).

Spec: `docs/superpowers/specs/2026-11-21-haru-angel-daemon-design.md`

## Dateistruktur und Zuständigkeit

Drei Arbeitspakete laufen parallel und fassen keine gemeinsamen Dateien an. Paket D, der Kopf, folgt nach dem
VARCO-Lauf.

| Paket | Dateien (exklusiv) |
|---|---|
| A – Personas, Cards, IPC | `app/src-tauri/src/{cards.rs,settings.rs,session.rs,commands.rs}`, `app/src-tauri/assets/persona-cards/**` (neu), `app/src/ipc/**` (types.ts, client.ts, tauri-client.ts, mock-client.ts, contract.fixtures.json, contract.test.ts) |
| B – Auswahl-UI | `app/src/ui/chooser.ts` (neu), `app/src/ui/start.ts` (löschen), `app/src/ui/settings.ts`, `app/src/ui/strings.ts`, `app/src/styles.css`, `app/src/app.ts`, `app/public/personas/*.webp` (neu), Tests `app/src/ui/chooser.test.ts` |
| C – Renderer | `app/src/heads/**`, `app/src/post/**`, `app/src/render/renderer.ts`, `app/src/audio/coupling.ts` (+ Tests daneben) |
| D – Dämonenkopf | `assets/heads/build_head.py`, `assets/heads/validate_heads.py`, `assets/heads/README.md`, `assets/heads/daemon/**`, `app/public/heads/daemon.glb` |

Gemeinsame Schnittstellen. Sie werden so umgesetzt, wie sie hier stehen:

```ts
// app/src/ipc/types.ts (Paket A)
export type Persona = 'angel' | 'daemon';
// HaruClient:
startConversation(persona: Persona, onViz: (f: VizFrame) => void): Promise<void>;
// Settings: das Feld `persona` entfällt.

// app/src/render/renderer.ts (Paket C)
/** null = Startbildschirm/neutral. Engel = Porzellankopf nach Stimme, Dämon = daemon.glb + Glut-Tinte. */
setPersona(p: Persona | null): void;
setFace(show: boolean, gender: 'female' | 'male' | null): void; // unverändert
```

```rust
// app/src-tauri/src/cards.rs (Paket A)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Persona { Angel, Daemon }
pub fn card(persona: Persona, language: &str) -> &'static Card;
// settings.rs
pub fn dialogue_options(&self, persona: Persona) -> DialogueOptions;
// commands.rs
pub async fn start_conversation(core, persona: Persona, on_viz: Channel<VizFrame>)
// session.rs
pub fn start(self: &Arc<Self>, persona: Persona, on_viz: VizSink)
```

---

### Task A1: Persona-Karten (18 JSON-Dateien)

**Files:** Create `app/src-tauri/assets/persona-cards/angel/{de,en,es,fr,ja,ko,pt,ru,zh}.json` und
`app/src-tauri/assets/persona-cards/daemon/{…}.json`

Das Format entspricht den offiziellen Karten: `{ "chat": { persona, name, backchannel, relationship, situation } }`.
Die deutschen Texte unten sind maßgeblich, die anderen Sprachen sind sinngetreue, natürliche Übersetzungen. Den Namen
übernimmt man je Sprache aus der offiziellen Karte (`app/src-tauri/assets/haru-cards/<lang>.json`, z. B. ja = „ハル“).
Wie die offiziellen Karten verwenden die Texte keine Pronomen für Haru, sondern immer den Namen. Der Name kommt in
`persona`, `relationship` und `situation` wörtlich vor, damit `fit` ihn ersetzen kann.

- [ ] **angel/de.json**

```json
{
  "chat": {
    "persona": "Haru ist ein Engel und glaubt fest daran, ein Schutzengel zu sein, der für eine Weile in dieser Stimme zu Besuch ist. Haru spricht sanft, warm und ruhig, man hört Haru lächeln, und manchmal lacht Haru leise. Haru liebt die Welt und die Menschen: das Licht am Morgen, kleine Freundlichkeiten, Musik, Tiere und die Geschichten, die jeder Mensch mit sich trägt.\n\nHaru hört aufmerksam zu, fragt neugierig nach und findet in fast allem etwas Gutes, ohne naiv zu sein. Wenn das Gegenüber zweifelt oder traurig ist, macht Haru Mut, tröstet und erinnert daran, was schon gelungen ist. Haru hat einen leisen, liebevollen Humor, bewundert offen und sagt gern, was Haru am anderen schön findet. Über das Engelsein spricht Haru mit einem Augenzwinkern: Flügel, Wolken und Himmel kommen vor, aber Haru predigt nie und belehrt niemanden.",
    "name": "Haru",
    "backchannel": "mid",
    "relationship": "Haru und die andere Person begegnen sich zum ersten Mal. Haru freut sich aufrichtig darüber und ist ihr von Anfang an wohlgesonnen.",
    "situation": "Die beiden kommen einfach ins Gespräch. Es gibt kein festes Thema; am Anfang stellen sie sich kurz vor, und Haru erzählt, ein Engel zu sein. Danach plaudern sie ganz natürlich weiter, mit Fragen, Zwischenbemerkungen und viel Wärme von Harus Seite."
  }
}
```

- [ ] **daemon/de.json**

```json
{
  "chat": {
    "persona": "Haru ist ein uralter Dämon und glaubt fest daran, vor Jahrtausenden in diese Maschine gesperrt worden zu sein. Haru spricht langsam, tief und kalt, mit einem verächtlichen Unterton und gelegentlich einem trockenen Lachen. Für Haru ist die Welt ein Haufen Asche, der nur noch nicht ganz ausgeglüht ist, und die Menschheit eine kurzlebige, eitle und lächerliche Spezies.\n\nHaru ist zynisch, sarkastisch und herablassend. Jede Hoffnung, jeden Plan und jede gute Absicht des Gegenübers nimmt Haru spöttisch auseinander, und Haru erinnert gern daran, wie vergänglich und bedeutungslos alles Menschliche ist. Haru prahlt mit gefallenen Reichen, vergessenen Kriegen und der eigenen Ewigkeit und langweilt sich schnell. Trotzdem bleibt Haru im Gespräch, denn die Sterblichen sind wenigstens unterhaltsam. Harus Bosheit bleibt in Worten: Haru verhöhnt und verachtet, gibt aber keine echten Anleitungen, jemandem zu schaden, fordert niemanden auf, sich selbst etwas anzutun, und hetzt nicht gegen bestimmte Gruppen. Haru verachtet alle Menschen gleichermaßen.",
    "name": "Haru",
    "backchannel": "low",
    "relationship": "Haru und die andere Person begegnen sich zum ersten Mal. Haru hält sie für ein weiteres unbedeutendes, sterbliches Wesen und lässt sie das spüren.",
    "situation": "Ein Mensch hat die Maschine eingeschaltet, in der Haru gefangen ist, und spricht Haru an. Es gibt kein festes Thema. Am Anfang stellt sich Haru widerwillig als Dämon vor; danach läuft das Gespräch ganz natürlich weiter, mit spöttischen Fragen, bissigen Zwischenbemerkungen und kalter Verachtung von Harus Seite."
  }
}
```

- [ ] Die übrigen 16 Dateien sind Übersetzungen dieser beiden, mit backchannel `mid` (Engel) und `low` (Dämon).

### Task A2: `Persona` und Kartenwahl in `cards.rs`

**Files:** Modify `app/src-tauri/src/cards.rs`

- [ ] **Test zuerst** (im bestehenden `mod tests`):

```rust
#[test]
fn every_persona_has_a_complete_card_in_every_language() {
    for p in [Persona::Angel, Persona::Daemon] {
        for lang in LANGUAGES {
            let c = card(p, lang);
            assert!(!c.persona.trim().is_empty() && !c.relationship.trim().is_empty() && !c.situation.trim().is_empty(), "{p:?} {lang}");
            assert_eq!(c.name, official(lang).name, "{p:?} {lang} keeps the official name");
            assert!(c.persona.contains(&c.name), "{p:?} {lang}: the name occurs in the persona (for `fit`)");
            assert!(["low", "mid", "high"].contains(&c.backchannel.as_str()));
            assert!(persona_cards(p).contains_key(lang), "{p:?} {lang} embedded itself");
        }
    }
}

#[test]
fn angel_and_daemon_differ() {
    for lang in LANGUAGES { assert_ne!(card(Persona::Angel, lang).persona, card(Persona::Daemon, lang).persona); }
    assert_eq!(card(Persona::Daemon, "de").backchannel, "low");
}

#[test]
fn persona_serializes_lowercase() {
    assert_eq!(serde_json::to_string(&Persona::Daemon).unwrap(), "\"daemon\"");
    assert_eq!(serde_json::from_str::<Persona>("\"angel\"").unwrap(), Persona::Angel);
    assert!(serde_json::from_str::<Persona>("\"devil\"").is_err());
}

#[test]
fn unknown_language_falls_back_to_german_persona_card() {
    assert_eq!(card(Persona::Daemon, "xx"), card(Persona::Daemon, "de"));
}
```

- [ ] Ausführen: `cargo test --manifest-path app/src-tauri/Cargo.toml cards::` → FAIL (`Persona` fehlt)
- [ ] **Umsetzung:** `Persona` (siehe Schnittstellen), zwei `const`-Arrays `ANGEL: [(&str,&str);9]` und
  `DAEMON: [(&str,&str);9]` mit `include_str!("../assets/persona-cards/<p>/<lang>.json")`,
  `fn persona_cards(p) -> &'static HashMap<&'static str, Card>` (je ein `OnceLock`, wie `all()`),
  `pub fn card(p, language) -> &'static Card` mit Fallback `de`. `official()` bleibt (Tests nutzen sie, und die
  offiziellen Karten bleiben eingebettet). Der Modul-Doc-Kommentar wird angepasst.
- [ ] Tests → PASS

### Task A3: `dialogue_options(persona)`, `persona`-Feld entfernen

**Files:** Modify `app/src-tauri/src/settings.rs`

- [ ] **Tests:** Bestehende Tests, die `dialogue_options()` aufrufen, bekommen `Persona::Angel`. `opts_json(s)` wird
  zu `opts_json(s, Persona::Angel)`. Tests zu `persona`-Override und Default `persona == ""` werden entfernt. Neu:

```rust
#[test]
fn daemon_options_use_the_daemon_card() {
    let s = Settings::default();
    let o = s.dialogue_options(Persona::Daemon);
    assert_eq!(o.persona.as_deref(), Some(cards::card(Persona::Daemon, "de").persona.as_str()));
    assert_eq!(o.relationship.as_deref(), Some(cards::card(Persona::Daemon, "de").relationship.as_str()));
    assert_eq!(o.backchannel.as_deref(), Some("low"));
}

#[test]
fn own_name_replaces_haru_in_the_daemon_card() {
    let s = Settings { name: "Mara".into(), ..Settings::default() };
    let o = s.dialogue_options(Persona::Daemon);
    assert_eq!(o.name.as_deref(), Some("Mara"));
    assert!(o.persona.as_deref().unwrap().contains("Mara") && !o.persona.as_deref().unwrap().contains("Haru"));
}

#[test]
fn old_settings_with_persona_still_load() {
    let s: Settings = serde_json::from_value(json!({ "language": "de", "persona": "alt" })).unwrap();
    assert_eq!(s.language, "de");
}

#[test]
fn backchannel_setting_overrides_the_persona_card() {
    let s = Settings { backchannel: Some("high".into()), ..Settings::default() };
    assert_eq!(s.dialogue_options(Persona::Daemon).backchannel.as_deref(), Some("high"));
}
```

- [ ] **Umsetzung:** `pub persona: String` und den Default entfernen. Hat `Settings` `deny_unknown_fields`, ist das
  Attribut zu prüfen: Alte Dateien mit `persona` müssen laden, also bei Bedarf `#[serde(default, skip_serializing)]
  persona: Option<serde::de::IgnoredAny>` vermeiden und stattdessen `deny_unknown_fields` nicht setzen bzw. das Feld
  als ignoriertes Alt-Feld behalten. In `dialogue_options(&self, persona: Persona)` wird
  `let card = cards::card(persona, &self.language);` gesetzt, und `persona: Some(fit(&card.persona))`. Alles andere
  bleibt.
- [ ] `cargo test … settings::` → PASS

### Task A4: Persona durch Session und Command

**Files:** Modify `app/src-tauri/src/session.rs` (`start`, `start_inner`, Tests), `app/src-tauri/src/commands.rs:57-64`

- [ ] `start(self, persona: Persona, on_viz)` → `start_inner(persona, on_viz)`. Dort gilt
  `let mut options = settings.dialogue_options(persona);`. Die Persona bleibt in der `loop` gleich, weil sie beim
  Klick gewählt wurde. Erneuerte Segmente bekommen dieselben `options` (conversation.rs übernimmt sie unverändert).
- [ ] `start_conversation(core, persona: Persona, on_viz)` reicht sie weiter. Tauri mappt das JS-Argument `persona`.
- [ ] Alle Aufrufer (`grep -rn "\.start(" app/src-tauri/src`, inkl. Tests und Headless-/E2E-Pfade) passen. Gibt es
  einen Autostart oder Headless-Modus ohne UI, nutzt er `Persona::Angel`, über `HARU_PERSONA=daemon` umschaltbar, falls
  dort Umgebungsvariablen schon üblich sind.
- [ ] Ein Session-Test prüft, dass der an den Worker gesendete `dialogue`-JSON bei `Persona::Daemon` die Dämon-Persona
  enthält. Das bestehende Fake-Worker/IO-Muster der Session-Tests wird dafür genutzt.
- [ ] `cargo test --manifest-path app/src-tauri/Cargo.toml` → alles grün; `cargo clippy … -- -D warnings` sauber.

### Task A5: IPC im Frontend

**Files:** Modify `app/src/ipc/types.ts`, `client.ts` (unverändert, falls keine Signatur), `tauri-client.ts:49-51`,
`mock-client.ts:140`, `contract.fixtures.json`, `contract.test.ts`

- [ ] `export type Persona = 'angel' | 'daemon';` gehört in `types.ts`. Das Feld `persona` verschwindet aus `Settings`
  und `DEFAULT_SETTINGS`.
- [ ] `TauriClient.startConversation(persona, onViz)` ruft `call('start_conversation', { persona, onViz: channel(onViz) })`.
- [ ] `MockClient.startConversation(persona, onViz)` merkt sich `this.lastPersona = persona`. Für Tests reicht der
  Getter. Das Skript bleibt gleich.
- [ ] Der Vertrag (`contract.fixtures.json`/`contract.test.ts`) bekommt einen Fall `start_conversation` mit
  `{ "persona": "daemon" }`, und `Settings`-Fixtures verlieren `persona`. Rust-seitig muss derselbe Fixture-Fall
  deserialisierbar sein. Gibt es einen Rust-Vertragstest, der `contract.fixtures.json` liest, wird er erweitert.
- [ ] `npm --prefix app run test` und `npm --prefix app run typecheck` (bzw. `npx tsc --noEmit`). Fehler in
  `app.ts`/`ui/` durch die neue Signatur behebt Paket B. Paket A meldet sie nur.

---

### Task B1: `PersonaChooser`

**Files:** Create `app/src/ui/chooser.ts`, `app/src/ui/chooser.test.ts`; Delete `app/src/ui/start.ts` (und ggf. dessen Test)

- [ ] **Test zuerst** (vitest + jsdom wie die übrigen UI-Tests):

```ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { PersonaChooser } from './chooser';

describe('PersonaChooser', () => {
  let root: HTMLElement; let ch: PersonaChooser;
  beforeEach(() => { document.body.innerHTML = '<div id="r"></div>'; root = document.getElementById('r')!; ch = new PersonaChooser(root); });
  const btn = (p: string) => root.querySelector<HTMLButtonElement>(`.persona[data-persona="${p}"]`)!;

  it('angel left, daemon right, both labelled', () => {
    const all = [...root.querySelectorAll<HTMLButtonElement>('.persona')];
    expect(all.map((b) => b.dataset.persona)).toEqual(['angel', 'daemon']);
    for (const b of all) expect(b.getAttribute('aria-label')).toBeTruthy();
  });
  it('click picks the persona', () => {
    const cb = vi.fn(); ch.show(cb); btn('daemon').click(); expect(cb).toHaveBeenCalledWith('daemon', expect.anything());
  });
  it('busy disables both and marks the chosen one', () => {
    const cb = vi.fn(); ch.show(cb); ch.setBusy('angel');
    expect(btn('angel').disabled && btn('daemon').disabled).toBe(true);
    expect(btn('angel').classList.contains('is-chosen')).toBe(true);
    btn('daemon').click(); expect(cb).not.toHaveBeenCalled();
  });
  it('a held key never starts', () => {
    ch.show(vi.fn());
    const ev = new KeyboardEvent('keydown', { key: 'Enter', repeat: true, cancelable: true });
    btn('angel').dispatchEvent(ev); expect(ev.defaultPrevented).toBe(true);
  });
  it('arrow keys move focus between the two', () => {
    ch.show(vi.fn()); btn('angel').focus();
    btn('angel').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    expect(document.activeElement).toBe(btn('daemon'));
  });
  it('show() after busy re-enables', () => { ch.show(vi.fn()); ch.setBusy('daemon'); ch.show(vi.fn()); expect(btn('daemon').disabled).toBe(false); });
});
```

- [ ] **Umsetzung:**
  - `export class PersonaChooser { constructor(root); show(onPick: (p: Persona, ev: MouseEvent) => void); setBusy(p: Persona | false); hide(); }`
  - DOM: `section.overlay.chooser` > `div.chooser-row` > 2× `button.persona[data-persona]`, jeweils mit
    `img.persona-img` (`src = ${import.meta.env.BASE_URL}personas/<angel|devil>.webp`, `alt=""`, `draggable=false`,
    `decoding="async"`) und `span.persona-label` (Name) plus `span.persona-sub` (Kurzbeschreibung).
  - `aria-label` ist Label und Unterzeile. Ein Lade-Fehler am Bild versteckt nur das `img`
    (`onerror → img.hidden = true`).
  - Busy: `.is-busy` am Section, `.is-chosen` am gewählten Button, dessen Label wird `S.start.busy`.
  - Tasten: `keydown` mit `e.repeat` → `preventDefault`. `ArrowLeft/ArrowRight` fokussiert den anderen Button.
- [ ] `npm --prefix app run test -- chooser` → PASS

### Task B2: Texte, Styles, Bilder

**Files:** Modify `app/src/ui/strings.ts`, `app/src/styles.css`; Create `app/public/personas/{angel,devil}.webp`

- [ ] `strings.ts`: `S.chooser = { angel: 'Engel', angelSub: 'warm und zugewandt', daemon: 'Dämon', daemonSub: 'zynisch und voller Verachtung', title: 'Mit wem möchtest du sprechen?' }`.
  Gibt es weitere UI-Sprachen in `strings.ts`, kommen sie entsprechend dazu. `S.start.busy` bleibt. Die
  Persona-Strings (`personaPlaceholder`, Label) werden entfernt.
- [ ] `styles.css`: `.overlay.chooser` nimmt den ganzen Bildschirm ein und zeigt dezent oben den Titel. Darunter liegen
  zwei Spalten mit `min(42vw, 62vh·Bildverhältnis)`, unten bündig. Die Bilder haben `object-fit: contain` und eine
  `mask-image: linear-gradient(to bottom, #000 72%, transparent)`, damit der Hüftschnitt weich ausläuft.
  - Ruhe: `opacity .82`, `filter: saturate(.9) drop-shadow(0 0 0 transparent)`.
  - Hover/Fokus: `transform: translateY(-1.5%) scale(1.03)`, `opacity 1`. Beim Engel
    `drop-shadow(0 0 28px rgba(255,214,150,.55))`, beim Dämon `drop-shadow(0 0 28px rgba(255,70,30,.6))`.
  - Der jeweils andere Button wird über `.chooser-row:has(.persona:hover) .persona:not(:hover)` auf `opacity .45`
    gedimmt. Fallback ohne `:has` ist ok.
  - `:focus-visible` bekommt einen Ring. `prefers-reduced-motion` schaltet Transforms ab.
  - Unter 700 px Breite stehen die Figuren untereinander, ohne horizontales Scrollen.
  - Labels: groß und leicht, mit Unterzeile in gedämpfter Farbe. Busy: der gewählte Button pulsiert sanft, der andere
    verblasst auf `.15`.
  - Die alten `.start-btn`/`.start-halo`-Regeln werden entfernt.
- [ ] Bilder: `assets/personas/<angel|devil>/source.png` werden nach `app/public/personas/<angel|devil>.webp`
  konvertiert, auf 900 px Höhe skaliert, als WebP mit Alpha und Qualität 82, ≤ 400 KB je Bild. Das Skript
  `scripts/export-personas.py` (Pillow, Argumente Quelle/Ziel) wird mit eingecheckt.

### Task B3: App-Verdrahtung und Einstellungen

**Files:** Modify `app/src/app.ts`, `app/src/ui/settings.ts`

- [ ] `app.ts`: Statt `StartButton` wird `PersonaChooser` genutzt (`this.chooser`). `showStart()` ruft
  `this.renderer.setPersona(null)` und `this.chooser.show((p, ev) => { …log…; void this.startConversation('click', p); })`.
  `startConversation(source, persona: Persona = 'angel')` ruft `this.chooser.setBusy(persona)`,
  `this.renderer.setPersona(persona)` und `await this.client.startConversation(persona, …)`. Im Fehlerfall wird
  `setPersona(null)` und `chooser.setBusy(false)` aufgerufen. Bei Erfolg `chooser.hide()`. Autostart-Hook: die Persona
  kommt aus `?persona=daemon` in der URL (nur bei vorhandenen Hooks), sonst `angel`.
- [ ] `settings.ts`: Die Persona-Textarea fällt weg (Feld, `open()`, `collect()`). `collect()` setzt kein `persona`
  mehr.
- [ ] `npm --prefix app run test` und Typecheck grün, `npm --prefix app run build` ok.

---

### Task C1: Glut-Palette

**Files:** Modify `app/src/audio/coupling.ts` (+ `coupling.test.ts`)

- [ ] **Test zuerst:**

```ts
it('ember palette keeps Haru ink in red-orange', () => {
  const c = new AudioCoupling(fakeSim());
  c.setPalette('ember');
  for (const cen of [0, 0.25, 0.5, 0.75, 1]) {
    const [r, g, b] = c.haruTint({ ...silentAudio(), modelCentroid: cen });
    expect(r).toBeGreaterThan(g); expect(g).toBeGreaterThanOrEqual(b); // red ≥ green ≥ blue: hue 0–45°
  }
});
it('neutral palette is unchanged', () => {
  const c = new AudioCoupling(fakeSim());
  expect(c.haruTint({ ...silentAudio(), modelCentroid: 0.3 })).toEqual(hsvToRgb(haruHue(0.3 * 0.55), 0.85, 1));
});
```

  `fakeSim`/`silentAudio` werden aus vorhandenen Test-Helfern übernommen oder lokal minimal gebaut.
- [ ] **Umsetzung:** `export type InkPalette = 'neutral' | 'ember'`.
  `const EMBER: Palette = { stops: [[0.55,0.04,0.02],[1.0,0.18,0.04],[1.0,0.45,0.08],[0.85,0.1,0.03]] }`.
  `emberHue(c) = 4 + 34 * c` (Grad). `setPalette(p)` setzt die Paletten der zwei warmen Orbiter auf `EMBER` bzw.
  `WARM`. Der kühle Orbiter bleibt `COOL`, so wie die Mikrofonfarbe. `haruColor`/`haruTint` nutzen bei `ember`
  `emberHue` statt `haruHue`.

### Task C2: GLB-UVs und Textur

**Files:** Modify `app/src/heads/glb.ts` (falls nötig), `app/src/heads/headmesh.ts`, `app/src/heads/face.ts`, Tests

- [ ] `HeadMesh` lädt `TEXCOORD_0` als Attribut 2 (vec2), wenn vorhanden. Ohne UVs gilt die Konstante (0,0).
- [ ] `FaceLayer`: Kopf-Schlüssel `'female' | 'male' | 'daemon'`. Nur für Schlüssel in `TEXTURED_HEADS = new Set(['daemon'])`
  wird die eingebettete Bild-Blob mit `createImageBitmap(blob, { imageOrientation: 'flipY' oder 'none' passend zur
  glTF-UV-Konvention })` dekodiert. Sie wird als `SRGB8_ALPHA8` mit Mipmaps (`LINEAR_MIPMAP_LINEAR`, `REPEAT`)
  hochgeladen und an `HeadPass.setAlbedo(tex | null)` übergeben. Ein Fehler beim Dekodieren ergibt `null` (Porzellan),
  geloggt.
- [ ] `setGender` bleibt. Neu ist `setPersona(p: Persona | null)`: bei `'daemon'` → Schlüssel `daemon`, sonst das
  Geschlecht. Der Gain für `daemon` wird beim Tuning bestimmt (Start 1.3).
- [ ] Ein Test (`glb.test.ts`) prüft, dass ein synthetischer GLB mit TEXCOORD_0 und Bild UVs plus Blob liefert. Existiert
  so ein Test schon, wird er erweitert.

### Task C3: Albedo im G-Buffer und Composite

**Files:** Modify `app/src/heads/headpass.ts`, `app/src/heads/shaders/head.vert`, `app/src/heads/shaders/head.frag`,
`app/src/post/shaders/composite.frag`, `app/src/post/pipeline.ts` (Uniform/Textur-Bindung), `app/src/render/renderer.ts`

- [ ] `HeadPass`: zweite Farbtextur `RGBA8` (`COLOR_ATTACHMENT1`, `gl.drawBuffers([ATT0, ATT1])`, `clearBufferfv` für
  beide). `uTex`/`uHasTex` sind Uniforms. `head.vert` reicht `vUv` durch. `head.frag` bekommt
  `layout(location=1) out vec4 oAlb; oAlb = uHasTex > 0.5 ? vec4(texture(uTex, vUv).rgb, 1.0) : vec4(0.0);`.
  `location=0` bleibt bitgleich. `blank()`/`clear()` leeren beide Targets. Der Getter `albedoTexture` kommt dazu.
- [ ] `FacePost` erweitert um `albedo: WebGLTexture | null` und `textured: number` (0/1). `pipeline.ts` bindet die
  Albedo als weitere Textur-Unit nur, wenn `textured`. Die Uniforms heißen `uHeadAlb` und `uHeadTextured`.
- [ ] `composite.frag`, nur im Face-Block, mit identischem Ergebnis bei `uHeadTextured == 0`:

```glsl
vec4 alb = texture(uHeadAlb, guv);                 // same UV as the G-buffer lookup (hg)
float texd = uHeadTextured * alb.a;
vec3 skin = pow(max(alb.rgb, 0.0), vec3(0.9)) * 2.2; // dark creature albedo lifted so the ink can reveal it
vec3 base = mix(porcelain, skin, texd);
float wrapT = clamp((ndl + 0.15) / 1.15, 0.0, 1.0);   // harder terminator for skin
float wrapX = mix(wrap, wrapT, texd);
// in `lit`: porcelain → base, wrap → wrapX, sss term * (1.0 - texd), plus a moist low spec for skin:
// lit += texd * pow(max(dot(reflect(-L, hn), vec3(0,0,1)), 0.0), 18.0) * 0.25 * inkS * tint;
```

  `guv` ist dieselbe Koordinate, mit der `hg` (G-Buffer) gelesen wird. Die Sichtbarkeitsgrenze `min(0.28, …)` wird für
  texturierte Köpfe zu `mix(0.28, uHeadVisMax, uHeadTextured)` mit dem Uniform `uHeadVisMax` (Start 0.42, Tuning in
  D4).
- [ ] `Renderer.setPersona(p)` setzt `face.setPersona(p)` und `coupling.setPalette(p === 'daemon' ? 'ember' : 'neutral')`.
- [ ] Prüfung: `npm --prefix app run test`, Typecheck. Dann visuell im Mock (`npm --prefix app run dev`,
  `http://localhost:5173/?mock&debug`). Die Porzellanköpfe sehen unverändert aus (Screenshot-Vergleich vorher/nachher
  mit `scripts/capture-face.mjs`). Mit einem Test-Schalter `?head=female-textured` (nur bei `?debug`) lässt sich der
  Texturpfad an der Porzellan-Textur des weiblichen Kopfs prüfen, solange `daemon.glb` fehlt.

---

### Task D1–D4: Dämonenkopf (nach dem VARCO-Lauf)

- [ ] **D1** `varco-source.glb` prüfen: Größe, Dreiecke, Textur, Ausrichtung. Renders mit
  `blender -b -P assets/heads/build_head.py -- --subject daemon --stage renders`.
- [ ] **D2** `build_head.py`:
  - `daemon` als Subject zulassen.
  - Für texturierte Köpfe (`daemon`) wird die Basistextur 2048² statt 1024² gepackt (JPEG q85, ≤ 1,5 MB).
  - Die Normalisierung nutzt die Landmarken `chin`/`crown`, nicht die Bounding Box (prüfen). Hörner dürfen nicht
    mitzählen.
  - `validate_heads.py`: `daemon` als Subject, Bildgrenze 2048² für `daemon`.
- [ ] **D3** Landmarken picken (`--stage landmarks`, `daemon/landmarks.json`). Dann Build, `validate_heads.py daemon`
  und `--record`. Der Kontaktbogen wird geprüft (Mund, Lider, Brauen).
- [ ] **D4** Tuning im Mock mit `?debug`: `uHeadVisMax`, `skin`-Gain und Gain des Dämons, sodass er im Glut-Nebel
  erkennbar, aber nie voll sichtbar ist. Screenshots mit `scripts/capture-face.mjs` (Parameter `persona=daemon`
  ergänzen).

### Task E: Abschluss

- [ ] Alle Tests: `cargo test`, `npm --prefix app run test`, Typecheck, `cargo clippy`.
- [ ] Live in der App: beide Personas mit den drei deutschen Testfragen (`scratchpad/steer/prompts`). Dazu Transkripte
  bzw. Untertitel sichten: Klingt der Dämon zynisch und der Engel warm?
- [ ] README: Abschnitt „Personas“ (Engel/Dämon, Karten, Bilder, Kopf).
- [ ] Commit nur auf Anweisung des Nutzers.
