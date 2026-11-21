import type { HaruClient } from './types';
import { MockClient } from './mock-client';
import { TauriClient } from './tauri-client';

/** Mock in the browser (or with `?mock`), Tauri client inside the app. */
export function createClient(): HaruClient {
  const search = typeof location !== 'undefined' ? location.search : '';
  if (search.includes('mock') || !('__TAURI_INTERNALS__' in window)) {
    const v = new URLSearchParams(search).get('mock');
    return new MockClient({ lang: v === 'ja' ? 'ja' : 'de', fail: v === 'error' ? 'boot' : v === 'lag' ? 'lag' : undefined, slow: v === 'slow' });
  }
  return new TauriClient();
}
