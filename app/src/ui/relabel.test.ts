// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import fixtures from '../ipc/contract.fixtures.json';
import type { AudioDevices, BackendOptions, DownloadEvent, HaruError, Settings, StateEvent } from '../ipc/types';
import { BootScreen } from './boot';
import { PersonaChooser } from './chooser';
import { HeadphoneHint } from './hint';
import { Hud } from './hud';
import { TABLES } from './locales';
import { SettingsPanel } from './settings';
import { setLanguage } from './strings';
import { ErrorToast } from './toast';

const de = TABLES.de!;
const en = TABLES.en!;
const ja = TABLES.ja;

const SETTINGS = fixtures.settingsDefault as Settings;
const AUDIO: AudioDevices = { inputs: ['USB Mic', 'Headset Mic'], outputs: ['Speakers', 'USB Headphones'], defaultInput: 'USB Mic', defaultOutput: 'USB Headphones' };
const BACKENDS: BackendOptions = {
  variants: ['vulkan', 'cpu'],
  devices: [
    { variant: 'vulkan', name: 'Vulkan1', description: 'Intel(R) Arc(TM) B580 Graphics', kind: 'gpu', memoryTotal: 12 * 1024 ** 3 },
    { variant: 'cpu', name: 'CPU', description: 'CPU', kind: 'cpu', memoryTotal: 32 * 1024 ** 3 },
  ],
  cudaAvailable: false,
  backendsDir: null,
};
const ERROR: HaruError = { code: 'noBackend', key: 'noBackendBuild', message: de.messages.noBackendBuild, detail: 'searched: backends/vulkan/speech.exe' };
const PROGRESS: DownloadEvent = { event: 'progress', data: { downloaded: 1.35e9, total: 6.6e9, bytesPerSec: 5e7 } };

// ------------------------------------------------------------------ German leftover detection

type Tree = { readonly [k: string]: string | readonly string[] | Tree };
const leafTexts = (t: Tree): string[] => Object.values(t).flatMap((v) => (typeof v === 'string' ? [v] : Array.isArray(v) ? [...v] : leafTexts(v as Tree)));
/** The literal parts of a template (around its placeholders) that contain a word. */
const fragments = (s: string) => s.split(/\{\w+\}/).map((f) => f.trim()).filter((f) => /\p{L}{2,}/u.test(f));
const english = new Set(leafTexts(en).flatMap((s) => [s, ...fragments(s)]));
/** German texts that English spells differently ("Audio", "Backend", "Top-K" are the same in both and do not count). */
const GERMAN_ONLY = [...new Set(leafTexts(de).flatMap(fragments))].filter((f) => !english.has(f));
const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
type Pattern = { g: string; re: RegExp };
const wordPattern = (g: string): Pattern => ({ g, re: new RegExp(`(?<![\\p{L}\\p{N}])${escape(g)}(?![\\p{L}\\p{N}])`, 'u') });
const PATTERNS = GERMAN_ONLY.map(wordPattern);
const japanese = new Set(leafTexts(ja).flatMap((s) => [s, ...fragments(s)]));
/** German or English texts that Japanese spells differently ("Top-K" is the same in all three and does not count). */
const NOT_JAPANESE = [...new Set([...leafTexts(de), ...leafTexts(en)].flatMap(fragments))].filter((f) => !japanese.has(f)).map(wordPattern);

/** Text nodes plus title, aria-label and placeholder of every element under `root`. */
function visibleTexts(root: HTMLElement): string[] {
  const out: string[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) if (n.textContent?.trim()) out.push(n.textContent);
  for (const el of root.querySelectorAll('*')) {
    for (const a of ['title', 'aria-label', 'placeholder']) {
      const v = el.getAttribute(a);
      if (v) out.push(v);
    }
  }
  return out;
}

const leftovers = (root: HTMLElement, patterns: Pattern[]) =>
  visibleTexts(root).flatMap((t) => patterns.filter((p) => p.re.test(t)).map((p) => `"${p.g}" in "${t}"`));
const germanLeftovers = (root: HTMLElement) => leftovers(root, PATTERNS);

// ------------------------------------------------------------------ helpers

let root: HTMLElement;
const label = (p: string) => root.querySelector(`.persona[data-persona="${p}"] .persona-label`)!.textContent;
const button = (text: string) => [...root.querySelectorAll('button')].find((b) => b.textContent === text)!;
/** The control of the settings field whose label currently reads `text`. */
function control<E extends HTMLElement>(text: string): E {
  const field = [...root.querySelectorAll('label.field')].find((f) => f.querySelector('.field-label')?.textContent === text);
  if (!field) throw new Error(`no field "${text}"`);
  return field.querySelector<E>('select, input')!;
}
const flush = () => new Promise<void>((r) => setTimeout(r, 0));

/** Every overlay with all its texts on screen (download progress, muted HUD, open drawer, error with detail and retry). */
function buildOverlays(): { relabel(): void }[] {
  const boot = new BootScreen(root);
  boot.setStatus('downloading');
  boot.onDownload(PROGRESS);
  const hint = new HeadphoneHint(root);
  const chooser = new PersonaChooser(root);
  chooser.show(() => {});
  const hud = new Hud(root);
  hud.setMuted(true);
  const panel = new SettingsPanel(root);
  panel.open(SETTINGS, AUDIO, BACKENDS);
  const toast = new ErrorToast(root);
  toast.show(ERROR, () => {});
  return [boot, hint, chooser, hud, panel, toast];
}

beforeEach(() => {
  setLanguage('de');
  document.body.innerHTML = '<div id="r"></div>';
  root = document.getElementById('r')!;
});
afterEach(() => {
  vi.useRealTimers();
  setLanguage('de');
});

describe('relabel', () => {
  it('the German leftover check finds the German texts', () => {
    expect(GERMAN_ONLY).toContain(de.hint.ok);
    expect(GERMAN_ONLY).toContain('noch');
    expect(GERMAN_ONLY).not.toContain('Backend');
  });

  it('no German text remains in any overlay after switching to English', () => {
    const overlays = buildOverlays();
    expect(germanLeftovers(root).length).toBeGreaterThan(20); // sanity: the check sees the German UI

    setLanguage('en');
    for (const c of overlays) c.relabel();
    expect(germanLeftovers(root)).toEqual([]);
    expect(root.querySelector('.boot-status')!.textContent).toBe(en.boot.downloading);
    expect(root.querySelector('.boot-meta')!.textContent).toContain('1 min 45 s left'); // 5.25 GB at 50 MB/s
    expect(root.querySelector('.hud-mute .hud-btn-label')!.textContent).toBe(en.hud.muted);
    expect(root.querySelector('.toast-msg')!.textContent).toBe(en.messages.noBackendBuild);
    expect(button(en.toast.retry)).toBeDefined();
  });

  it('no German or English text remains in any overlay after switching to Japanese', () => {
    const overlays = buildOverlays();
    expect(leftovers(root, NOT_JAPANESE).length).toBeGreaterThan(20); // sanity: the check sees the German UI

    setLanguage('ja');
    for (const c of overlays) c.relabel();
    expect(leftovers(root, NOT_JAPANESE)).toEqual([]);
    expect(root.querySelector('.boot-status')!.textContent).toBe(ja.boot.downloading);
    expect(root.querySelector('.boot-meta')!.textContent).toContain('残り 1 min 45 s');
    expect(root.querySelector('.hud-mute .hud-btn-label')!.textContent).toBe(ja.hud.muted);
    expect(root.querySelector('.toast-msg')!.textContent).toBe(ja.messages.noBackendBuild);
    expect(root.querySelector('.settings-head h2')!.textContent).toBe(ja.settings.title);
    expect(button(ja.toast.retry)).toBeDefined();
  });

  it('settings panel relabels while closed too', () => {
    const panel = new SettingsPanel(root);
    setLanguage('en');
    panel.relabel();
    expect(germanLeftovers(root)).toEqual([]);
    expect(root.querySelector('.settings-head h2')!.textContent).toBe(en.settings.title);
    expect(root.querySelector('.settings')!.getAttribute('aria-label')).toBe(en.settings.title);
  });

  it('settings relabel keeps unsaved select values', () => {
    const panel = new SettingsPanel(root);
    panel.open(SETTINGS, AUDIO, BACKENDS);
    const gender = control<HTMLSelectElement>(de.settings.gender);
    const backchannel = control<HTMLSelectElement>(de.settings.backchannel);
    const language = control<HTMLSelectElement>(de.settings.language);
    const input = control<HTMLSelectElement>(de.settings.inputDevice);
    const backend = control<HTMLSelectElement>(de.settings.backend);
    const device = control<HTMLSelectElement>(de.settings.device);
    const quality = control<HTMLSelectElement>(de.settings.quality);
    const name = control<HTMLInputElement>(de.settings.name);
    gender.value = 'male';
    backchannel.value = 'high';
    language.value = 'ja';
    input.value = 'Headset Mic';
    backend.value = 'vulkan';
    backend.dispatchEvent(new Event('change'));
    device.value = 'Vulkan1';
    quality.value = 'high';
    name.value = 'Aki';

    setLanguage('en');
    panel.relabel();
    expect([gender.value, backchannel.value, language.value, input.value, backend.value, device.value, quality.value])
      .toEqual(['male', 'high', 'ja', 'Headset Mic', 'vulkan', 'Vulkan1', 'high']);
    expect(name.value).toBe('Aki');
    expect(gender.selectedOptions[0].textContent).toBe(en.gender.male);
    expect(backchannel.selectedOptions[0].textContent).toBe(en.backchannel.high);
    expect(quality.selectedOptions[0].textContent).toBe(en.quality.high);
    expect(input.options[0].textContent).toBe(en.settings.default);
    expect(backend.options[0].textContent).toBe(en.settings.auto);
    expect(root.querySelectorAll('output')[0].textContent).toBe('0.60');
    expect(root.querySelectorAll('output')[1].textContent).toBe('1.0 s');
  });

  it('saving label follows language', async () => {
    const panel = new SettingsPanel(root);
    panel.open(SETTINGS, AUDIO, BACKENDS);
    let done!: () => void;
    panel.onSave(() => new Promise<void>((r) => { done = r; }));
    const save = button(de.settings.save);
    save.click();
    expect(save.textContent).toBe(de.settings.saving);
    expect(save.disabled).toBe(true);

    setLanguage('en');
    panel.relabel();
    expect(save.textContent).toBe(en.settings.saving);
    done();
    await flush();
    expect(save.textContent).toBe(en.settings.save);
    expect(save.disabled).toBe(false);
  });

  it('a save that switches the language leaves the drawer in the new language', async () => {
    const panel = new SettingsPanel(root);
    panel.open(SETTINGS, AUDIO, BACKENDS);
    // like App.saveSettings -> applySettings -> relabel, while the drawer is still open
    panel.onSave(async (s) => { setLanguage(s.language); panel.relabel(); });
    control<HTMLSelectElement>(de.settings.language).value = 'en';
    const save = button(de.settings.save);
    save.click();
    await flush();
    expect(save.textContent).toBe(en.settings.save);
    expect(germanLeftovers(root)).toEqual([]);
  });

  it('a save error follows language', async () => {
    const panel = new SettingsPanel(root);
    panel.open(SETTINGS, AUDIO, BACKENDS);
    const err: HaruError = { code: 'settings', key: 'temperatureRange', message: de.messages.temperatureRange, detail: null };
    panel.onSave(() => Promise.reject(err));
    button(de.settings.save).click();
    await flush();
    const status = root.querySelector('.settings-status')!;
    expect(status.textContent).toBe(de.messages.temperatureRange);
    setLanguage('en');
    panel.relabel();
    expect(status.textContent).toBe(en.messages.temperatureRange);
    expect(status.classList.contains('is-error')).toBe(true);
  });

  it('chooser busy label survives relabel', () => {
    const chooser = new PersonaChooser(root);
    chooser.show(() => {});
    chooser.setBusy('daemon');
    setLanguage('en');
    chooser.relabel();
    expect(label('daemon')).toBe(en.start.busy);
    expect(label('angel')).toBe(en.chooser.angel);
    expect(root.querySelector('.persona[data-persona="angel"]')!.getAttribute('aria-label')).toBe(`${en.chooser.angel} – ${en.chooser.angelSub}`);
    chooser.setBusy(false);
    expect(label('daemon')).toBe(en.chooser.daemon);
  });

  it('boot meta for verifying and download errors follows language', () => {
    const boot = new BootScreen(root);
    const meta = root.querySelector('.boot-meta')!;
    boot.onDownload({ event: 'verifying', data: { done: 42, total: 100 } });
    expect(meta.textContent!.replace(/[  ]/g, ' ')).toBe('Prüfe Modell… 42 %');
    setLanguage('en');
    boot.relabel();
    expect(meta.textContent).toBe('Verifying model… 42%');
    boot.onDownload(fixtures.downloadError as DownloadEvent);
    expect(meta.textContent).toBe(en.messages.downloadFailed);
    setLanguage('de');
    boot.relabel();
    expect(meta.textContent).toBe(de.messages.downloadFailed);
  });

  it('toast relabel keeps the auto-hide timer', () => {
    vi.useFakeTimers();
    const toast = new ErrorToast(root);
    const warning = (fixtures.stateEventWarning as StateEvent).warning!;
    toast.showWarning(warning);
    const el = root.querySelector('.toast')!;
    expect(el.textContent).toBe(warning.message);
    vi.advanceTimersByTime(4000);
    setLanguage('en');
    toast.relabel();
    expect(el.textContent).toBe('Intel(R) UHD Graphics has less than 8 GB of video memory – Haru needs at least 8 GB, so the conversation may break off.');
    expect(el.classList.contains('is-visible')).toBe(true);
    vi.advanceTimersByTime(2001); // 6 s after showWarning, not after relabel
    expect(el.classList.contains('is-visible')).toBe(false);
  });
});
