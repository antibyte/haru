// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './app';
import {
  DEFAULT_SETTINGS, type AudioDevices, type BackendOptions, type BattlePhase, type BootInfo, type HaruClient, type HaruError,
  type Persona, type SaveSettingsResult, type Settings, type StateEvent, type VizFrame,
} from './ipc/types';
import { TABLES } from './ui/locales';
import { setLanguage } from './ui/strings';

const rec = vi.hoisted(() => ({ calls: [] as string[] }));
vi.mock('./render/renderer', () => ({
  Renderer: class {
    start(): void {}
    setMode(m: string): void { rec.calls.push(`mode:${m}`); }
    setDownloadRate(): void {}
    setPersona(p: unknown): void { rec.calls.push(`persona:${String(p)}`); }
    setBattle(on: boolean): void { rec.calls.push(`battle:${on}`); }
    setArrows(a: unknown): void { rec.calls.push(`arrows:${a ? 'on' : 'off'}`); }
    pushViz(): void {}
    keepFaceReady(): void {}
    onWordEvent(): void {}
    resetWords(): void {}
    setQualityMode(): void {}
    setFace(): void {}
  },
}));

const READY: StateEvent = { state: 'ready', backend: 'vulkan', device: 'B580', error: null, warning: null };
const COUNTER = '{"remaining":1,"armed":false}';

class StubClient implements HaruClient {
  calls: string[] = [];
  personas: Persona[] = [];
  topics: string[] = [];
  onViz: ((f: VizFrame) => void) | null = null;
  /** holds startBattle open until released (a start that is still pending) */
  gate: Promise<void> | null = null;
  private stateCb: ((e: StateEvent) => void) | null = null;
  constructor(private readonly battle = true, private readonly refuse: HaruError | null = null) {}
  async getStatus(): Promise<StateEvent> { return READY; }
  async onState(cb: (e: StateEvent) => void): Promise<() => void> { this.stateCb = cb; return () => {}; }
  async getSettings(): Promise<Settings> { return { ...DEFAULT_SETTINGS, headphoneHintSeen: true }; }
  async saveSettings(s: Settings): Promise<SaveSettingsResult> { return { settings: s, reloadRequired: false }; }
  async listAudioDevices(): Promise<AudioDevices> { return { inputs: [], outputs: [], defaultInput: null, defaultOutput: null }; }
  async backendOptions(): Promise<BackendOptions> { return { variants: [], devices: [], cudaAvailable: false, backendsDir: null }; }
  async boot(): Promise<BootInfo> { return { backend: 'vulkan', device: 'Vulkan1', deviceName: 'B580', modelPath: 'x', plan: [], warning: null, battle: this.battle }; }
  async startConversation(p: Persona, onViz: (f: VizFrame) => void): Promise<void> { this.personas.push(p); this.onViz = onViz; }
  async startBattle(topic: string, onViz: (f: VizFrame) => void): Promise<void> {
    if (this.gate) await this.gate;
    if (this.refuse) throw this.refuse;
    this.topics.push(topic); this.onViz = onViz;
  }
  async stopConversation(): Promise<void> { this.calls.push('stop'); }
  async setMuted(m: boolean): Promise<void> { this.calls.push(`mute:${m}`); }
  async playCue(): Promise<void> { this.calls.push('cue'); }
  emit(e: StateEvent): void { this.stateCb?.(e); }
}

const lv = (rms: number) => ({ rms, bands: new Array<number>(16).fill(0) });
const frame = (phase: BattlePhase, prepared: number): VizFrame => ({
  t: 0, model: lv(0.15), mic: { rms: 0, bands: new Array<number>(8).fill(0) }, latencyMs: 0,
  words: [{ seq: 0, text: ' Hal', speaker: 'angel' }, { seq: 0, text: '', speaker: 'daemon' }],
  battle: { angel: lv(0.15), daemon: lv(0.01), phase, prepared },
});
const flush = async () => { for (let i = 0; i < 5; i++) await new Promise<void>((r) => setTimeout(r, 0)); };

let root: HTMLElement;
const q = <E extends Element = HTMLElement>(s: string) => root.querySelector<E>(s)!;
const visible = (s: string) => q(s).classList.contains('is-visible');

async function started(client: StubClient): Promise<App> {
  const app = new App(client, document.createElement('canvas'), root);
  await app.start();
  return app;
}
async function battle(): Promise<void> {
  q<HTMLButtonElement>('.chooser-vs').click();
  q<HTMLButtonElement>('.topic-start').click();
  await flush();
}

beforeEach(() => {
  setLanguage('de');
  localStorage.clear();
  localStorage.setItem('haru.presence', COUNTER);
  rec.calls = [];
  document.body.innerHTML = '<div id="r"></div>';
  root = document.getElementById('r')!;
});
afterEach(() => { localStorage.clear(); });

describe('battle session', () => {
  it('starts from the VS with the chosen topic and never passes a conversation persona', async () => {
    const c = new StubClient();
    await started(c);
    await battle();
    expect(c.topics).toHaveLength(1);
    expect(TABLES.de!.battle.topics).toContain(c.topics[0]);
    expect(c.personas).toEqual([]);
    expect(rec.calls).toContain('battle:true');
    expect(visible('.chooser')).toBe(false);
    expect(visible('.hud')).toBe(true);
  });

  it('battles do not count as daemon conversations: the presence counter stays unchanged', async () => {
    const c = new StubClient();
    await started(c);
    await battle();
    for (const p of ['preparing', 'playing', 'paused', 'playing'] as const) c.onViz!(frame(p, 0.5));
    q<HTMLButtonElement>('.hud-stop').click();
    await flush();
    expect(c.calls).toContain('stop');
    expect(c.calls).not.toContain('cue');
    expect(localStorage.getItem('haru.presence')).toBe(COUNTER);
    expect(rec.calls.filter((x) => x.startsWith('battle:'))).toEqual(['battle:true', 'battle:false']);
  });

  it('a daemon conversation still counts down the presence counter', async () => {
    const c = new StubClient();
    await started(c);
    q<HTMLButtonElement>('.persona[data-persona="daemon"]').click();
    await flush();
    expect(c.personas).toEqual(['daemon']);
    expect(localStorage.getItem('haru.presence')).not.toBe(COUNTER);
    q<HTMLButtonElement>('.hud-stop').click();
    await flush();
  });

  it('the preparing overlay follows the battle phase', async () => {
    const c = new StubClient();
    await started(c);
    await battle();
    c.onViz!(frame('preparing', 0.4));
    expect(visible('.preparing')).toBe(true);
    expect(q('.preparing-fill').style.width).toBe('40%');
    c.onViz!(frame('playing', 1));
    expect(visible('.preparing')).toBe(false);
    c.onViz!(frame('paused', 0.2));
    expect(visible('.preparing')).toBe(true);
    q<HTMLButtonElement>('.hud-stop').click();
    await flush();
    expect(visible('.preparing')).toBe(false);
  });

  it('the HUD shows only Stop and M does not mute', async () => {
    const c = new StubClient();
    await started(c);
    await battle();
    expect(q('.hud-mute').hidden).toBe(true);
    expect(q('.hud-stop').hidden).toBe(false);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'm' }));
    await flush();
    expect(c.calls.filter((x) => x.startsWith('mute'))).toEqual([]);
    q<HTMLButtonElement>('.hud-stop').click();
    await flush();
    expect(q('.hud-mute').hidden).toBe(false);
  });

  it('a battle that ends by itself returns to the chooser', async () => {
    const c = new StubClient();
    await started(c);
    await battle();
    c.emit(READY);
    await flush();
    expect(visible('.chooser')).toBe(true);
    expect(visible('.hud')).toBe(false);
    expect(rec.calls).toContain('battle:false');
  });

  it('without battle support the VS opens nothing', async () => {
    const c = new StubClient(false);
    await started(c);
    expect(q('.chooser-vs').classList.contains('is-unavailable')).toBe(true);
    q<HTMLButtonElement>('.chooser-vs').click();
    expect(visible('.topic')).toBe(false);
    expect(c.topics).toEqual([]);
  });

  it('a battle the UI already left is stopped in the core when its start completes', async () => {
    const c = new StubClient();
    let release!: () => void;
    c.gate = new Promise<void>((r) => { release = r; });
    await started(c);
    await battle();
    c.emit({ state: 'error', backend: null, device: null, error: { code: 'worker', key: 'workerExited', message: 'x', detail: null }, warning: null });
    await flush();
    expect(c.calls).not.toContain('stop');
    release();
    await flush();
    expect(c.calls).toContain('stop');
  });

  it('the settings open during a battle', async () => {
    const c = new StubClient();
    await started(c);
    await battle();
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 's' }));
    await flush();
    expect(root.querySelector('.settings-drawer, .settings')!.classList.contains('is-visible')).toBe(true);
    expect(q('.hud-settings').hidden).toBe(false);
    expect(q('.hud-fullscreen').hidden).toBe(false);
  });

  it('a refused start shows the message and returns to the chooser', async () => {
    const err: HaruError = { code: 'worker', key: 'battleUnsupported', message: TABLES.de!.messages.battleUnsupported, detail: null };
    const c = new StubClient(true, err);
    await started(c);
    await battle();
    expect(q('.toast-msg').textContent).toBe(TABLES.de!.messages.battleUnsupported);
    expect(visible('.chooser')).toBe(true);
    expect(q<HTMLButtonElement>('.chooser-vs').disabled).toBe(false);
    expect(rec.calls.filter((x) => x.startsWith('battle:'))).toEqual(['battle:true', 'battle:false']);
  });
});
