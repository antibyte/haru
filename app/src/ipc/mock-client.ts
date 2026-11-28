import { de } from '../ui/locales/de';
import {
  DEFAULT_SETTINGS,
  type AudioDevices, type BackendOptions, type BootInfo, type DownloadEvent, type HaruClient, type HaruError, type MessageKey,
  type Persona, type SaveSettingsResult, type Settings, type StateEvent, type VizFrame, type WordPiece,
} from './types';

export interface MockOptions { lang: 'de' | 'en' | 'ja'; fail?: 'boot' | 'lag'; slow?: boolean }

export const MOCK_TOTAL_BYTES = 6_623_102_720;
const FRAME_HZ = 7.5;
const GAP_FRAMES = 22; // about 3 s of empty frames between loops
const DOWNLOAD_MS = 6000;
const VERIFY_MS = 500;
const LOADING_MS = 1500;
const START_MS = 1500;
const LAG_AFTER_MS = 22000;

export const MOCK_SCRIPTS: Record<'de' | 'en' | 'ja', string[]> = {
  de: [' Hal', 'lo', ',', ' ich', ' bin', ' Ha', 'ru', '.', ' ', '', ' Schön', ',', ' dass', ' du', ' da', ' bist', '!', ' ', '',
    ' Wor', 'über', ' möch', 'test', ' du', ' heu', 'te', ' spre', 'chen', '?', ' '],
  en: [' Hel', 'lo', ',', ' I', "'m", ' Ha', 'ru', '.', ' ', '', ' Good', ' to', ' see', ' you', '!', ' ', '',
    ' What', ' would', ' you', ' like', ' to', ' talk', ' a', 'bout', '?', ' '],
  ja: ['こんに', 'ちは', '。', '', '', '今日は', 'いい', '天気', 'ですね', '。', '', ''],
};

/** ?mock=slow: latency ramps 300 -> 900 ms over 20 s and back down over the next 20 s (triangle wave). */
export function slowLatency(tSec: number): number {
  const p = (tSec % 40) / 20;
  return 300 + 600 * (p <= 1 ? p : 2 - p);
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
const clamp01 = (x: number) => Math.min(1, Math.max(0, x));

function bandsFor(n: number, peak: number, amp: number, jitter: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    const d = (i - peak * (n - 1)) / (n * 0.18);
    out.push(clamp01(amp * Math.exp(-d * d) + jitter * Math.random()));
  }
  return out;
}

/** Scripted client for browser development: boot/download, a looping conversation, in-memory settings. */
export class MockClient implements HaruClient {
  private state: StateEvent = { state: 'booting', backend: null, device: null, error: null, warning: null };
  private listeners = new Set<(e: StateEvent) => void>();
  private settings: Settings;
  private modelPresent = false;
  private muted = false;
  private _lastPersona: Persona | null = null;
  /** The persona of the last startConversation call (for tests). */
  get lastPersona(): Persona | null { return this._lastPersona; }
  private timer: ReturnType<typeof setInterval> | null = null;
  private readonly timeline: string[];

  constructor(private readonly options: MockOptions = { lang: 'de' }) {
    // The UI language follows the scripted conversation (?mock=en / ?mock=ja); German stays the default.
    this.settings = { ...DEFAULT_SETTINGS, language: options.lang };
    this.timeline = [...MOCK_SCRIPTS[options.lang], ...new Array<string>(GAP_FRAMES).fill('')];
  }

  async getStatus(): Promise<StateEvent> { return this.state; }

  async onState(cb: (e: StateEvent) => void): Promise<() => void> {
    this.listeners.add(cb);
    return () => { this.listeners.delete(cb); };
  }

  async getSettings(): Promise<Settings> { return { ...this.settings }; }

  async saveSettings(s: Settings): Promise<SaveSettingsResult> {
    if (this.state.state === 'talking') throw this.err('busy', 'busy');
    const reloadRequired = s.backend !== this.settings.backend || s.device !== this.settings.device || s.voicePath !== this.settings.voicePath;
    this.settings = { ...s };
    if (reloadRequired) this.emit({ state: 'booting', backend: null, device: null, error: null, warning: null });
    return { settings: { ...this.settings }, reloadRequired };
  }

  async listAudioDevices(): Promise<AudioDevices> {
    return {
      inputs: ['Mikrofon (USB Audio)', 'Headset-Mikrofon'],
      outputs: ['Lautsprecher (Realtek)', 'Kopfhörer (USB Audio)'],
      defaultInput: 'Mikrofon (USB Audio)',
      defaultOutput: 'Kopfhörer (USB Audio)',
    };
  }

  async backendOptions(): Promise<BackendOptions> {
    return {
      variants: ['vulkan', 'cpu'],
      devices: [
        { variant: 'vulkan', name: 'Vulkan1', description: 'Intel(R) Arc(TM) B580 Graphics', kind: 'gpu', memoryTotal: 12 * 1024 ** 3 },
        { variant: 'vulkan', name: 'Vulkan0', description: 'Intel(R) UHD Graphics', kind: 'igpu', memoryTotal: 8 * 1024 ** 3 },
        { variant: 'cpu', name: 'CPU', description: 'CPU', kind: 'cpu', memoryTotal: 32 * 1024 ** 3 },
      ],
      cudaAvailable: false,
      backendsDir: null,
    };
  }

  async boot(onDownload: (e: DownloadEvent) => void): Promise<BootInfo> {
    if (this.state.state === 'talking') throw this.err('busy', 'busy');
    const info: BootInfo = {
      backend: 'vulkan', device: 'Vulkan1', deviceName: 'Intel(R) Arc(TM) B580 Graphics',
      modelPath: 'C:\\mock\\Haru-4B-Q8_0.gguf',
      plan: [{ backend: 'vulkan', device: 'Vulkan1', description: 'Intel(R) Arc(TM) B580 Graphics' }],
      warning: null, battle: true,
    };
    if (this.state.state === 'ready') return info;
    this.emit({ state: 'booting', backend: null, device: null, error: null, warning: null });
    await sleep(600);
    if (this.options.fail === 'boot') {
      const error = this.err('noBackend', 'noBackendBuild',
        'searched: backends/vulkan/speech.exe, backends/cuda/speech.exe, backends/cpu/speech.exe');
      this.emit({ state: 'error', backend: null, device: null, error, warning: null });
      throw error;
    }
    if (!this.modelPresent) {
      this.emit({ state: 'downloading', backend: null, device: null, error: null, warning: null });
      const bps = MOCK_TOTAL_BYTES / (DOWNLOAD_MS / 1000);
      const t0 = performance.now();
      for (;;) {
        const f = Math.min(1, (performance.now() - t0) / DOWNLOAD_MS);
        onDownload({ event: 'progress', data: {
          downloaded: Math.round(f * MOCK_TOTAL_BYTES), total: MOCK_TOTAL_BYTES, bytesPerSec: bps * (0.92 + 0.16 * Math.random()),
        } });
        if (f >= 1) break;
        await sleep(100);
      }
      const v0 = performance.now();
      for (;;) {
        const f = Math.min(1, (performance.now() - v0) / VERIFY_MS);
        onDownload({ event: 'verifying', data: { done: Math.round(f * MOCK_TOTAL_BYTES), total: MOCK_TOTAL_BYTES } });
        if (f >= 1) break;
        await sleep(50);
      }
      onDownload({ event: 'done', data: { path: info.modelPath } });
      this.modelPresent = true;
    }
    this.emit({ state: 'loading', backend: 'vulkan', device: info.deviceName, error: null, warning: null });
    await sleep(LOADING_MS);
    this.emit({ state: 'ready', backend: 'vulkan', device: info.deviceName, error: null, warning: null });
    return info;
  }

  async startConversation(persona: Persona, onViz: (f: VizFrame) => void): Promise<void> {
    this._lastPersona = persona;
    if (this.state.state !== 'ready') throw this.err('busy', 'busy');
    this.stopTimer();
    await sleep(START_MS); // the real core waits for frame 0 (about 4 s)
    if (this.state.state !== 'ready') throw this.err('busy', 'busy');
    const scriptLen = MOCK_SCRIPTS[this.options.lang].length;
    const t0 = performance.now();
    let lastFrame = -1;
    this.timer = setInterval(() => {
      const t = (performance.now() - t0) / 1000;
      const cur = Math.floor(t * FRAME_HZ);
      const words: WordPiece[] = [];
      for (let n = lastFrame + 1; n <= cur; n++) words.push({ seq: n, text: this.timeline[n % this.timeline.length] });
      lastFrame = cur;
      const idx = cur % this.timeline.length;
      const active = this.timeline[idx].trim() !== '';
      const inGap = idx >= scriptLen;
      const modelRms = active ? 0.12 + 0.1 * Math.abs(Math.sin(2 * Math.PI * 1.3 * t)) : 0.01;
      const peak = 0.5 + 0.45 * Math.sin(2 * Math.PI * 0.35 * t);
      const micRms = !this.muted && inGap ? 0.05 + 0.15 * Math.random() : 0;
      onViz({
        t,
        model: { rms: modelRms, bands: active ? bandsFor(16, peak, modelRms * 3.2, 0.02) : new Array<number>(16).fill(0) },
        mic: { rms: micRms, bands: micRms > 0 ? bandsFor(8, Math.random(), micRms * 4, 0.05) : new Array<number>(8).fill(0) },
        latencyMs: this.options.slow ? slowLatency(t) : 280 + 40 * Math.sin(t * 0.7),
        words,
      });
    }, 1000 / 60);
    this.emit({ ...this.state, state: 'talking', error: null });
    if (this.options.fail === 'lag') {
      setTimeout(() => {
        if (this.state.state !== 'talking') return;
        this.stopTimer();
        // a conversation that ends by itself arrives as ready + error, like the real core
        this.emit({ ...this.state, state: 'ready', error: this.err('lagLimit', 'lagLimit') });
      }, LAG_AFTER_MS);
    }
  }

  async stopConversation(): Promise<void> {
    if (this.state.state !== 'talking') return;
    this.stopTimer();
    this.emit({ ...this.state, state: 'ready', error: null });
  }

  async setMuted(muted: boolean): Promise<void> { this.muted = muted; }
  async playCue(): Promise<void> {}

  private stopTimer(): void {
    if (this.timer !== null) { clearInterval(this.timer); this.timer = null; }
  }

  private emit(e: StateEvent): void {
    this.state = e;
    for (const l of this.listeners) l(e);
  }

  /** Like the real core: a key for the UI, its German text as `message`. */
  private err(code: HaruError['code'], key: MessageKey, detail: string | null = null): HaruError {
    return { code, key, message: de.messages[key], detail };
  }
}
