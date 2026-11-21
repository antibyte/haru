import { invoke } from '@tauri-apps/api/core';
import type { AppHooks } from './app';

let noRender = false;

/** Dev/test switches from the Rust side (HARU_TEST_* env vars, see src-tauri/src/test_hooks.rs). Null in normal use. */
export async function loadTestHooks(): Promise<AppHooks | null> {
  if (!('__TAURI_INTERNALS__' in window)) return null;
  let h: { autostart: boolean; debug: boolean; noRender: boolean; quality: string | null };
  try {
    h = await invoke('test_hooks');
  } catch {
    return null;
  }
  if (!h.autostart && !h.debug) return null;
  if (h.debug && !location.search.includes('debug')) {
    const q = h.quality ? `&quality=${h.quality}` : '';
    history.replaceState(null, '', `${location.pathname}?debug${q}${location.hash}`);
  }
  noRender = h.noRender;
  const log = (msg: string): void => void invoke('test_log', { msg }).catch(() => {});
  return { autostart: h.autostart, log };
}

/** Periodic diagnostics from the debug overlay's renderer handle. */
export function startDiagnostics(log: (msg: string) => void): void {
  const r = (window as unknown as { __haru?: Record<string, any> }).__haru;
  if (!r) return;
  if (noRender) {
    (r as unknown as { stop(): void }).stop();
    log('render loop stopped (HARU_TEST_NO_RENDER)');
  }
  const gl = r.glc?.gl as WebGL2RenderingContext | undefined;
  if (gl) {
    const ext = gl.getExtension('WEBGL_debug_renderer_info');
    log(`GL renderer: ${ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER)} | vendor: ${ext ? gl.getParameter(ext.UNMASKED_VENDOR_WEBGL) : gl.getParameter(gl.VENDOR)} | ${gl.getParameter(gl.VERSION)}`);
  }
  setInterval(() => {
    log(`fps ${Number(r.fpsValue).toFixed(1)} frame ${Number(r.frameEma).toFixed(1)}ms quality ${r.level} words ${r.injector?.count} mode ${r.mode} latency ${r.lastViz ? Math.round(r.lastViz.latencyMs) : '-'}ms canvas ${r.canvas?.width}x${r.canvas?.height}`);
  }, 2000);
}
