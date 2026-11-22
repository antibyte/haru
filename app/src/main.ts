import '@fontsource-variable/inter';
import './styles.css';
import { App } from './app';
import { createClient } from './ipc/client';
import type { HaruClient } from './ipc/types';
import { loadTestHooks, startDiagnostics } from './testhooks';
import { S, fill, setLanguage } from './ui/strings';

const canvas = document.getElementById('fluid') as HTMLCanvasElement | null;
const overlayRoot = document.getElementById('overlay-root');
if (!canvas || !overlayRoot) throw new Error('missing #fluid or #overlay-root');

function showFatal(root: HTMLElement, e: unknown): void {
  const msg = e instanceof Error ? e.message : String(e);
  const el = document.createElement('div');
  el.style.cssText =
    'position:fixed;inset:0;display:grid;place-items:center;color:#ddd;font:16px system-ui;text-align:center;padding:2rem;pointer-events:auto';
  el.textContent =
    msg === 'WEBGL2_UNAVAILABLE' || msg === 'COLOR_BUFFER_FLOAT_UNAVAILABLE'
      ? S.fatal.webgl2
      : fill(S.fatal.graphics, { detail: msg });
  root.appendChild(el);
  console.error(e);
}

/** UI language from the stored (or OS-detected) settings; English if they cannot be read. */
async function initLanguage(client: HaruClient): Promise<void> {
  try {
    setLanguage((await client.getSettings()).language);
  } catch {
    setLanguage('en');
  }
}

async function main(canvasEl: HTMLCanvasElement, root: HTMLElement): Promise<void> {
  const hooks = await loadTestHooks(); // null unless a HARU_TEST_* env var is set
  const client = createClient();
  await initLanguage(client); // before any overlay exists, so no German flashes up first
  const app = new App(client, canvasEl, root, hooks ?? undefined);
  if (hooks) startDiagnostics(hooks.log);
  app.start().catch((e) => console.error(e));
}

main(canvas, overlayRoot).catch((e) => showFatal(overlayRoot, e));
