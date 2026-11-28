import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  MESSAGE_KEYS, STATE_EVENT,
  type AudioDevices, type BackendOptions, type BootInfo, type DownloadEvent, type HaruClient, type HaruError, type MessageKey,
  type MessageParams, type Persona, type SaveSettingsResult, type Settings, type StateEvent, type VizFrame,
} from './types';

const CODES = new Set(['download', 'hashMismatch', 'diskSpace', 'noBackend', 'worker', 'noMicrophone',
  'audioDevice', 'lagLimit', 'settings', 'io', 'busy', 'internal']);

/** Backend keys plus the frontend's own `'unexpected'`, so converting an already converted error keeps its key. */
const KEYS: ReadonlySet<string> = new Set<string>([...MESSAGE_KEYS, 'unexpected']);

/** Only a plain object counts as params; values other than strings and finite numbers are dropped. */
function toParams(v: unknown): MessageParams | undefined {
  if (!v || typeof v !== 'object') return undefined;
  const proto: unknown = Object.getPrototypeOf(v);
  if (proto !== Object.prototype && proto !== null) return undefined;
  const out: MessageParams = {};
  for (const [k, x] of Object.entries(v as Record<string, unknown>)) {
    if (typeof x === 'string' || (typeof x === 'number' && Number.isFinite(x))) out[k] = x;
  }
  return out;
}

/**
 * Rejections from Rust are serialized HaruError objects; anything else becomes an internal error. A key is kept only
 * when this frontend knows it (otherwise the UI falls back to `message`).
 */
export function toHaruError(e: unknown): HaruError {
  if (e && typeof e === 'object') {
    const o = e as Record<string, unknown>;
    if (typeof o.code === 'string' && CODES.has(o.code) && typeof o.message === 'string') {
      const err: HaruError = { code: o.code as HaruError['code'], message: o.message, detail: typeof o.detail === 'string' ? o.detail : null };
      if (typeof o.key === 'string' && KEYS.has(o.key)) err.key = o.key as MessageKey | 'unexpected';
      const params = toParams(o.params);
      if (params) err.params = params;
      return err;
    }
  }
  return {
    code: 'internal', key: 'unexpected', message: 'Unerwarteter Fehler.',
    detail: typeof e === 'string' ? e : e instanceof Error ? e.message : JSON.stringify(e),
  };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toHaruError(e);
  }
}

function channel<T>(cb: (v: T) => void): Channel<T> {
  const ch = new Channel<T>();
  ch.onmessage = cb;
  return ch;
}

export class TauriClient implements HaruClient {
  getStatus(): Promise<StateEvent> { return call('get_status'); }
  onState(cb: (e: StateEvent) => void): Promise<() => void> {
    return listen<StateEvent>(STATE_EVENT, (ev) => cb(ev.payload));
  }
  getSettings(): Promise<Settings> { return call('get_settings'); }
  saveSettings(settings: Settings): Promise<SaveSettingsResult> { return call('save_settings', { settings }); }
  listAudioDevices(): Promise<AudioDevices> { return call('list_audio_devices'); }
  backendOptions(): Promise<BackendOptions> { return call('backend_options'); }
  boot(onDownload: (e: DownloadEvent) => void): Promise<BootInfo> {
    return call('boot', { onDownload: channel(onDownload) });
  }
  startConversation(persona: Persona, onViz: (f: VizFrame) => void): Promise<void> {
    return call('start_conversation', { persona, onViz: channel(onViz) });
  }
  startBattle(topic: string, onViz: (f: VizFrame) => void): Promise<void> {
    return call('start_battle', { topic, onViz: channel(onViz) });
  }
  stopConversation(): Promise<void> { return call('stop_conversation'); }
  setMuted(muted: boolean): Promise<void> { return call('set_muted', { muted }); }
  playCue(): Promise<void> { return call('play_cue'); }
}
