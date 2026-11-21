import { LANGUAGES, type AudioDevices, type BackendOptions, type HaruError, type Quality, type Settings } from '../ipc/types';
import { S } from './strings';

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

function fill(sel: HTMLSelectElement, opts: [string, string][], value: string): void {
  sel.replaceChildren(...opts.map(([v, l]) => option(v, l)));
  if (!opts.some(([v]) => v === value)) sel.append(option(value, value));
  sel.value = value;
}

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
const de1 = (n: number) => n.toFixed(1).replace('.', ',');
const de2 = (n: number) => n.toFixed(2).replace('.', ',');

/** Right-hand glass drawer; every field maps one-to-one onto `Settings`. */
export class SettingsPanel {
  private readonly rootEl: HTMLElement;
  private readonly form: HTMLElement;
  private readonly status: HTMLElement;
  private readonly saveBtn: HTMLButtonElement;
  private cb: ((s: Settings) => Promise<void>) | null = null;
  private base: Settings | null = null;
  private options: BackendOptions | null = null;
  private open_ = false;

  private readonly language = el('select');
  private readonly name = el('input');
  private readonly persona = el('textarea');
  private readonly gender = el('select');
  private readonly backchannel = el('select');
  private readonly speakFirst = el('input');
  private readonly temperature = el('input');
  private readonly temperatureOut = el('output', 'num');
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
    this.rootEl.setAttribute('aria-label', S.settings.title);
    const scrim = el('div', 'settings-scrim');
    scrim.addEventListener('click', () => this.close());
    const drawer = el('div', 'glass settings-drawer');

    const head = el('header', 'settings-head');
    head.append(el('h2', undefined, S.settings.title));
    const x = el('button', 'icon-btn', '×');
    x.type = 'button';
    x.setAttribute('aria-label', S.settings.cancel);
    x.addEventListener('click', () => this.close());
    head.append(x);

    this.form = el('div', 'settings-body');
    const T = S.settings;

    this.name.type = 'text';
    this.name.maxLength = 40;
    this.persona.rows = 3;
    this.persona.placeholder = T.personaPlaceholder;
    this.speakFirst.type = 'checkbox';
    this.subtitles.type = 'checkbox';
    this.showFace.type = 'checkbox';
    this.echoCancellation.type = 'checkbox';
    this.temperature.type = 'range';
    Object.assign(this.temperature, { min: '0.1', max: '1.5', step: '0.05' });
    this.topK.type = 'number';
    Object.assign(this.topK, { min: '1', max: '200', step: '1' });
    this.voicePath.type = 'text';
    this.voicePath.placeholder = T.voicePathPlaceholder;
    this.voicePath.spellcheck = false;
    this.lagLimit.type = 'range';
    Object.assign(this.lagLimit, { min: '0.5', max: '5', step: '0.1' });

    fill(this.language, LANGUAGES.map((l) => [l, S.languages[l] ?? l]), 'de');
    this.temperature.addEventListener('input', () => this.syncRanges());
    this.lagLimit.addEventListener('input', () => this.syncRanges());
    this.backend.addEventListener('change', () => this.fillDevices(this.device.value));

    this.section(T.sections.dialogue, [
      this.field(T.language, this.language),
      this.field(T.name, this.name),
      this.field(T.persona, this.persona),
      this.field(T.gender, this.gender),
      this.field(T.backchannel, this.backchannel),
      this.toggle(T.speakFirst, this.speakFirst),
      this.slider(T.temperature, this.temperature, this.temperatureOut),
      this.field(T.topK, this.topK),
    ]);
    this.section(T.sections.audio, [
      this.field(T.inputDevice, this.inputDevice),
      this.field(T.outputDevice, this.outputDevice),
      this.toggle(T.echoCancellation, this.echoCancellation),
      el('p', 'hint-note', T.echoCancellationHint),
      this.slider(T.lagLimit, this.lagLimit, this.lagLimitOut),
    ]);
    this.section(T.sections.engine, [
      this.field(T.backend, this.backend),
      this.field(T.device, this.device),
      this.field(T.voicePath, this.voicePath),
      el('p', 'hint-note', T.reloadNote),
    ]);
    this.section(T.sections.display, [
      this.field(T.quality, this.quality),
      this.toggle(T.subtitles, this.subtitles),
      this.toggle(T.showFace, this.showFace),
    ]);

    const foot = el('footer', 'settings-foot');
    this.status = el('p', 'settings-status');
    this.status.setAttribute('role', 'status');
    const cancel = el('button', 'btn btn-ghost', T.cancel);
    cancel.type = 'button';
    cancel.addEventListener('click', () => this.close());
    this.saveBtn = el('button', 'btn btn-primary', T.save);
    this.saveBtn.type = 'button';
    this.saveBtn.addEventListener('click', () => void this.save());
    const btns = el('div', 'settings-btns');
    btns.append(cancel, this.saveBtn);
    foot.append(this.status, btns);

    drawer.append(head, this.form, foot);
    this.rootEl.append(scrim, drawer);
    root.append(this.rootEl);
  }

  get isOpen(): boolean { return this.open_; }

  onSave(cb: (s: Settings) => Promise<void>): void { this.cb = cb; }

  open(s: Settings, a: AudioDevices, b: BackendOptions): void {
    this.base = s;
    this.options = b;
    const T = S.settings;

    fill(this.language, LANGUAGES.map((l) => [l, S.languages[l] ?? l]), s.language);
    this.name.value = s.name;
    this.persona.value = s.persona;
    fill(this.gender, [['', T.none], ['female', S.gender.female], ['male', S.gender.male]], s.gender ?? '');
    fill(this.backchannel, [['', T.none], ['low', S.backchannel.low], ['mid', S.backchannel.mid], ['high', S.backchannel.high]], s.backchannel ?? '');
    this.speakFirst.checked = s.speakFirst;
    this.temperature.value = String(s.temperature);
    this.topK.value = String(s.topK);
    this.voicePath.value = s.voicePath ?? '';
    fill(this.inputDevice, [['', T.default], ...a.inputs.map((d): [string, string] => [d, d])], s.inputDevice ?? '');
    fill(this.outputDevice, [['', T.default], ...a.outputs.map((d): [string, string] => [d, d])], s.outputDevice ?? '');
    fill(this.backend, [['auto', T.auto], ...b.variants.map((v): [string, string] => [v, S.backends[v] ?? v])], s.backend);
    this.fillDevices(s.device);
    const q: Quality[] = ['auto', 'low', 'medium', 'high', 'ultra'];
    fill(this.quality, q.map((v): [string, string] => [v, S.quality[v]]), s.quality);
    this.echoCancellation.checked = s.echoCancellation;
    this.subtitles.checked = s.subtitles;
    this.showFace.checked = s.showFace;
    this.lagLimit.value = String(s.lagLimitSecs);
    this.syncRanges();

    this.setStatus('', false);
    this.saveBtn.disabled = false;
    this.saveBtn.textContent = T.save;
    this.open_ = true;
    this.rootEl.classList.add('is-visible');
    this.form.scrollTop = 0;
  }

  close(): void {
    this.open_ = false;
    this.rootEl.classList.remove('is-visible');
  }

  private async save(): Promise<void> {
    if (!this.cb || !this.base) return;
    const next = this.collect(this.base);
    this.saveBtn.disabled = true;
    this.saveBtn.textContent = S.settings.saving;
    this.setStatus('', false);
    try {
      await this.cb(next);
      this.close();
    } catch (e) {
      const err = e as Partial<HaruError> | undefined;
      this.setStatus(err?.message ?? String(e), true);
    } finally {
      this.saveBtn.disabled = false;
      this.saveBtn.textContent = S.settings.save;
    }
  }

  private collect(base: Settings): Settings {
    const num = (v: string, fallback: number) => (Number.isFinite(parseFloat(v)) ? parseFloat(v) : fallback);
    const orNull = (v: string) => (v === '' ? null : v);
    return {
      ...base,
      language: this.language.value,
      name: this.name.value.trim() || base.name,
      persona: this.persona.value.trim(),
      gender: orNull(this.gender.value) as Settings['gender'],
      backchannel: orNull(this.backchannel.value) as Settings['backchannel'],
      speakFirst: this.speakFirst.checked,
      temperature: clamp(num(this.temperature.value, base.temperature), 0.1, 1.5),
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

  private fillDevices(current: string): void {
    const b = this.options;
    const variant = this.backend.value;
    const devs = (b?.devices ?? []).filter((d) => variant === 'auto' || d.variant === variant);
    const opts: [string, string][] = [['auto', S.settings.auto]];
    for (const d of devs) opts.push([d.name, d.description && d.description !== d.name ? `${d.name} – ${d.description}` : d.name]);
    const keep = opts.some(([v]) => v === current) || (b === null) ? current : 'auto';
    fill(this.device, opts, keep);
  }

  private syncRanges(): void {
    const paint = (input: HTMLInputElement) => {
      const lo = parseFloat(input.min);
      const hi = parseFloat(input.max);
      input.style.setProperty('--p', `${((parseFloat(input.value) - lo) / (hi - lo)) * 100}%`);
    };
    paint(this.temperature);
    paint(this.lagLimit);
    this.temperatureOut.textContent = de2(parseFloat(this.temperature.value));
    this.lagLimitOut.textContent = `${de1(parseFloat(this.lagLimit.value))} ${S.settings.seconds}`;
  }

  private setStatus(text: string, error: boolean): void {
    this.status.textContent = text;
    this.status.classList.toggle('is-error', error);
  }

  private section(title: string, children: HTMLElement[]): void {
    const s = el('section', 'settings-section');
    s.append(el('h3', undefined, title), ...children);
    this.form.append(s);
  }

  private field(label: string, control: HTMLElement): HTMLElement {
    const f = el('label', 'field');
    f.append(el('span', 'field-label', label), control);
    return f;
  }

  private toggle(label: string, input: HTMLInputElement): HTMLElement {
    const f = el('label', 'field field-row');
    const sw = el('span', 'switch');
    sw.append(input, el('i'));
    f.append(el('span', 'field-label', label), sw);
    return f;
  }

  private slider(label: string, input: HTMLInputElement, out: HTMLElement): HTMLElement {
    const f = el('label', 'field');
    const row = el('span', 'field-head');
    row.append(el('span', 'field-label', label), out);
    f.append(row, input);
    return f;
  }
}
