import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  STATE_EVENT,
  type AudioDevices, type BackendOptions, type BootInfo, type DownloadEvent, type HaruClient, type HaruError,
  type SaveSettingsResult, type Settings, type StateEvent, type VizFrame,
} from './types';

const CODES = new Set(['download', 'hashMismatch', 'diskSpace', 'noBackend', 'worker', 'noMicrophone',
  'audioDevice', 'lagLimit', 'settings', 'io', 'busy', 'internal']);

/** Rejections from Rust are serialized HaruError objects; anything else becomes an internal error. */
export function toHaruError(e: unknown): HaruError {
  if (e && typeof e === 'object') {
    const o = e as Record<string, unknown>;
    if (typeof o.code === 'string' && CODES.has(o.code) && typeof o.message === 'string') {
      return { code: o.code as HaruError['code'], message: o.message, detail: typeof o.detail === 'string' ? o.detail : null };
    }
  }
  return { code: 'internal', message: 'Unerwarteter Fehler.', detail: typeof e === 'string' ? e : e instanceof Error ? e.message : JSON.stringify(e) };
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
  startConversation(onViz: (f: VizFrame) => void): Promise<void> {
    return call('start_conversation', { onViz: channel(onViz) });
  }
  stopConversation(): Promise<void> { return call('stop_conversation'); }
  setMuted(muted: boolean): Promise<void> { return call('set_muted', { muted }); }
}
