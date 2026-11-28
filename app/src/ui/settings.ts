import { LANGUAGES, type AudioDevices, type BackendOptions, type HaruError, type Quality, type Settings } from '../ipc/types';
import { BACKEND_NAMES } from './format';
import { LANGUAGE_NAMES, Labels, S, errorText, formatNumber, getLanguage } from './strings';

type ErrorLike = Pick<HaruError, 'key' | 'params' | 'message'>;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function option(value: string, label: string): HTMLOptionElement {
  const o = document.createElement('option');
  o.value = value;
  o.textContent = label;
  return o;
}

function fillSelect(sel: HTMLSelectElement, opts: [string, string][], value: string): void {
  sel.replaceChildren(...opts.map(([v, l]) => option(v, l)));
  if (!opts.some(([v]) => v === value)) sel.append(option(value, value));
  sel.value = value;
}

/** A rejected save: a HaruError (key shown in the active language) or anything else as plain text. */
function asError(e: unknown): ErrorLike {
  const o = e as Partial<HaruError> | null | undefined;
  return o && typeof o === 'object' && typeof o.message === 'string'
    ? { key: o.key, params: o.params, message: o.message }
    : { message: String(e) };
}

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
const QUALITIES: Quality[] = ['auto', 'low', 'medium', 'high', 'ultra'];

/** Right-hand glass drawer; every field maps one-to-one onto `Settings`. */
export class SettingsPanel {
  private readonly rootEl: HTMLElement;
  private readonly form: HTMLElement;
  private readonly status: HTMLElement;
  private readonly saveBtn: HTMLButtonElement;
  private readonly labels = new Labels();
  private cb: ((s: Settings) => Promise<void>) | null = null;
  private base: Settings | null = null;
  private options: BackendOptions | null = null;
  private audio: AudioDevices | null = null;
  private open_ = false;
  private saving = false;
  private statusError: ErrorLike | null = null;

  private readonly language = el('select');
  private readonly name = el('input');
  private readonly gender = el('select');
  private readonly backchannel = el('select');
  private readonly speakFirst = el('input');
  private readonly temperature = el('input');
  private readonly temperatureOut = el('output', 'num');
  private readonly battleTemperature = el('input');
  private readonly battleTemperatureOut = el('output', 'num');
  private readonly topK = el('input');
  private readonly voicePath = el('input');
  private readonly inputDevice = el('select');
  private readonly outputDevice = el('select');
  private readonly echoCancellation = el('input');
  private readonly backend = el('select');
  private readonly device = el('select');
  private readonly quality = el('select');
  private readonly subtitles = el('input');
  private readonly showFace = el('input');
  private readonly lagLimit = el('input');
  private readonly lagLimitOut = el('output', 'num');

  constructor(root: HTMLElement) {
    this.rootEl = el('aside', 'overlay settings');
    this.rootEl.setAttribute('role', 'dialog');
    this.labels.attr(this.rootEl, 'aria-label', () => S.settings.title);
    const scrim = el('div', 'settings-scrim');
    scrim.addEventListener('click', () => this.close());
    const drawer = el('div', 'glass settings-drawer');

    const head = el('header', 'settings-head');
    head.append(this.labels.text(el('h2'), () => S.settings.title));
    const x = el('button', 'icon-btn', '×');
    x.type = 'button';
    this.labels.attr(x, 'aria-label', () => S.settings.cancel);
    x.addEventListener('click', () => this.close());
    head.append(x);

    this.form = el('div', 'settings-body');

    this.name.type = 'text';
    this.name.maxLength = 40;
    this.speakFirst.type = 'checkbox';
    this.subtitles.type = 'checkbox';
    this.showFace.type = 'checkbox';
    this.echoCancellation.type = 'checkbox';
    this.temperature.type = 'range';
    Object.assign(this.temperature, { min: '0.3', max: '1', step: '0.05' });
    this.battleTemperature.type = 'range';
    Object.assign(this.battleTemperature, { min: '0.3', max: '1', step: '0.05' });
    this.topK.type = 'number';
    Object.assign(this.topK, { min: '1', max: '200', step: '1' });
    this.voicePath.type = 'text';
    this.labels.attr(this.voicePath, 'placeholder', () => S.settings.voicePathPlaceholder);
    this.voicePath.spellcheck = false;
    this.lagLimit.type = 'range';
    Object.assign(this.lagLimit, { min: '0.5', max: '5', step: '0.1' });

    fillSelect(this.language, LANGUAGES.map((l) => [l, LANGUAGE_NAMES[l]]), getLanguage());
    this.temperature.addEventListener('input', () => this.syncRanges());
    this.battleTemperature.addEventListener('input', () => this.syncRanges());
    this.lagLimit.addEventListener('input', () => this.syncRanges());
    this.backend.addEventListener('change', () => this.fillDevices(this.device.value));

    // Getters read S.settings at the moment of use (never a cached table), so relabel() reaches every label.
    this.section(() => S.settings.sections.dialogue, [
      this.field(() => S.settings.language, this.language),
      this.field(() => S.settings.name, this.name),
      this.field(() => S.settings.gender, this.gender),
      this.field(() => S.settings.backchannel, this.backchannel),
      this.toggle(() => S.settings.speakFirst, this.speakFirst),
      this.slider(() => S.settings.temperature, this.temperature, this.temperatureOut),
      this.note(() => S.settings.temperatureHint),
      this.slider(() => S.settings.battleTemperature, this.battleTemperature, this.battleTemperatureOut),
      this.note(() => S.settings.battleTemperatureHint),
      this.field(() => S.settings.topK, this.topK),
    ]);
    this.section(() => S.settings.sections.audio, [
      this.field(() => S.settings.inputDevice, this.inputDevice),
      this.field(() => S.settings.outputDevice, this.outputDevice),
      this.toggle(() => S.settings.echoCancellation, this.echoCancellation),
      this.note(() => S.settings.echoCancellationHint),
      this.slider(() => S.settings.lagLimit, this.lagLimit, this.lagLimitOut),
    ]);
    this.section(() => S.settings.sections.engine, [
      this.field(() => S.settings.backend, this.backend),
      this.field(() => S.settings.device, this.device),
      this.field(() => S.settings.voicePath, this.voicePath),
      this.note(() => S.settings.reloadNote),
    ]);
    this.section(() => S.settings.sections.display, [
      this.field(() => S.settings.quality, this.quality),
      this.toggle(() => S.settings.subtitles, this.subtitles),
      this.toggle(() => S.settings.showFace, this.showFace),
    ]);

    const foot = el('footer', 'settings-foot');
    this.status = el('p', 'settings-status');
    this.status.setAttribute('role', 'status');
    const cancel = this.labels.text(el('button', 'btn btn-ghost'), () => S.settings.cancel);
    cancel.type = 'button';
    cancel.addEventListener('click', () => this.close());
    this.saveBtn = el('button', 'btn btn-primary');
    this.saveBtn.type = 'button';
    this.saveBtn.addEventListener('click', () => void this.save());
    const btns = el('div', 'settings-btns');
    btns.append(cancel, this.saveBtn);
    foot.append(this.status, btns);

    drawer.append(head, this.form, foot);
    this.rootEl.append(scrim, drawer);
    root.append(this.rootEl);
    this.renderSave();
  }

  get isOpen(): boolean { return this.open_; }

  onSave(cb: (s: Settings) => Promise<void>): void { this.cb = cb; }

  open(s: Settings, a: AudioDevices, b: BackendOptions): void {
    this.base = s;
    this.options = b;
    this.audio = a;

    this.fillChoices(s, a);
    this.name.value = s.name;
    this.speakFirst.checked = s.speakFirst;
    this.temperature.value = String(s.temperature);
    this.battleTemperature.value = String(s.battleTemperature);
    this.topK.value = String(s.topK);
    this.voicePath.value = s.voicePath ?? '';
    this.echoCancellation.checked = s.echoCancellation;
    this.subtitles.checked = s.subtitles;
    this.showFace.checked = s.showFace;
    this.lagLimit.value = String(s.lagLimitSecs);
    this.syncRanges();

    this.setStatus(null);
    this.saving = false;
    this.renderSave();
    this.open_ = true;
    this.rootEl.classList.add('is-visible');
    this.form.scrollTop = 0;
  }

  close(): void {
    this.open_ = false;
    this.rootEl.classList.remove('is-visible');
  }

  /**
   * Every label, option list, number and the status in the active language, open or closed. Option lists are rebuilt
   * from the current control values, so unsaved changes survive.
   */
  relabel(): void {
    this.labels.apply();
    if (this.base && this.audio) {
      this.fillChoices(this.collect(this.base), this.audio);
      this.syncRanges();
    }
    this.renderSave();
    this.renderStatus();
  }

  private async save(): Promise<void> {
    if (!this.cb || !this.base) return;
    const next = this.collect(this.base);
    this.saving = true;
    this.renderSave();
    this.setStatus(null);
    try {
      await this.cb(next); // may switch the language and relabel this open drawer
      this.close();
    } catch (e) {
      this.setStatus(asError(e));
    } finally {
      this.saving = false;
      this.renderSave(); // reads the live S, so a language changed by this save shows here too
    }
  }

  private collect(base: Settings): Settings {
    const num = (v: string, fallback: number) => (Number.isFinite(parseFloat(v)) ? parseFloat(v) : fallback);
    const orNull = (v: string) => (v === '' ? null : v);
    return {
      ...base,
      language: this.language.value,
      name: this.name.value.trim() || base.name,
      gender: orNull(this.gender.value) as Settings['gender'],
      backchannel: orNull(this.backchannel.value) as Settings['backchannel'],
      speakFirst: this.speakFirst.checked,
      temperature: clamp(num(this.temperature.value, base.temperature), 0.3, 1),
      battleTemperature: clamp(num(this.battleTemperature.value, base.battleTemperature), 0.3, 1),
      topK: Math.round(clamp(num(this.topK.value, base.topK), 1, 200)),
      voicePath: orNull(this.voicePath.value.trim()),
      inputDevice: orNull(this.inputDevice.value),
      outputDevice: orNull(this.outputDevice.value),
      echoCancellation: this.echoCancellation.checked,
      backend: this.backend.value as Settings['backend'],
      device: this.device.value,
      quality: this.quality.value as Quality,
      subtitles: this.subtitles.checked,
      showFace: this.showFace.checked,
      lagLimitSecs: clamp(num(this.lagLimit.value, base.lagLimitSecs), 0.5, 5),
    };
  }

  /** All option lists with translated labels, selecting the values of `current`. */
  private fillChoices(current: Settings, a: AudioDevices): void {
    fillSelect(this.language, LANGUAGES.map((l) => [l, LANGUAGE_NAMES[l]]), current.language);
    fillSelect(this.gender, [['', S.settings.none], ['female', S.gender.female], ['male', S.gender.male]], current.gender ?? '');
    fillSelect(this.backchannel, [['', S.settings.none], ['low', S.backchannel.low], ['mid', S.backchannel.mid], ['high', S.backchannel.high]],
      current.backchannel ?? '');
    fillSelect(this.inputDevice, [['', S.settings.default], ...a.inputs.map((d): [string, string] => [d, d])], current.inputDevice ?? '');
    fillSelect(this.outputDevice, [['', S.settings.default], ...a.outputs.map((d): [string, string] => [d, d])], current.outputDevice ?? '');
    const variants = this.options?.variants ?? [];
    fillSelect(this.backend, [['auto', S.settings.auto], ...variants.map((v): [string, string] => [v, BACKEND_NAMES[v] ?? v])], current.backend);
    this.fillDevices(current.device);
    fillSelect(this.quality, QUALITIES.map((v): [string, string] => [v, S.quality[v]]), current.quality);
  }

  private fillDevices(current: string): void {
    const b = this.options;
    const variant = this.backend.value;
    const devs = (b?.devices ?? []).filter((d) => variant === 'auto' || d.variant === variant);
    const opts: [string, string][] = [['auto', S.settings.auto]];
    for (const d of devs) opts.push([d.name, d.description && d.description !== d.name ? `${d.name} – ${d.description}` : d.name]);
    const keep = opts.some(([v]) => v === current) || (b === null) ? current : 'auto';
    fillSelect(this.device, opts, keep);
  }

  private syncRanges(): void {
    const paint = (input: HTMLInputElement) => {
      const lo = parseFloat(input.min);
      const hi = parseFloat(input.max);
      input.style.setProperty('--p', `${((parseFloat(input.value) - lo) / (hi - lo)) * 100}%`);
    };
    paint(this.temperature);
    paint(this.battleTemperature);
    paint(this.lagLimit);
    this.temperatureOut.textContent = formatNumber(parseFloat(this.temperature.value), 2);
    this.battleTemperatureOut.textContent = formatNumber(parseFloat(this.battleTemperature.value), 2);
    this.lagLimitOut.textContent = `${formatNumber(parseFloat(this.lagLimit.value), 1)} ${S.settings.seconds}`;
  }

  private renderSave(): void {
    this.saveBtn.disabled = this.saving;
    this.saveBtn.textContent = this.saving ? S.settings.saving : S.settings.save;
  }

  private setStatus(error: ErrorLike | null): void {
    this.statusError = error;
    this.renderStatus();
  }

  private renderStatus(): void {
    this.status.textContent = this.statusError ? errorText(this.statusError) : '';
    this.status.classList.toggle('is-error', this.statusError !== null);
  }

  private section(title: () => string, children: HTMLElement[]): void {
    const s = el('section', 'settings-section');
    s.append(this.labels.text(el('h3'), title), ...children);
    this.form.append(s);
  }

  private field(label: () => string, control: HTMLElement): HTMLElement {
    const f = el('label', 'field');
    f.append(this.labels.text(el('span', 'field-label'), label), control);
    return f;
  }

  private toggle(label: () => string, input: HTMLInputElement): HTMLElement {
    const f = el('label', 'field field-row');
    const sw = el('span', 'switch');
    sw.append(input, el('i'));
    f.append(this.labels.text(el('span', 'field-label'), label), sw);
    return f;
  }

  private slider(label: () => string, input: HTMLInputElement, out: HTMLElement): HTMLElement {
    const f = el('label', 'field');
    const row = el('span', 'field-head');
    row.append(this.labels.text(el('span', 'field-label'), label), out);
    f.append(row, input);
    return f;
  }

  private note(text: () => string): HTMLElement {
    return this.labels.text(el('p', 'hint-note'), text);
  }
}
