import { getCurrentWindow } from '@tauri-apps/api/window';
import { IDLE_MS } from './hud';
import { Labels, S } from './strings';

/** What the title bar needs from the native window; injectable so tests and the browser need no Tauri. */
export interface WindowControls {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
  isMaximized(): Promise<boolean>;
  /** Called whenever the window is resized (including maximize/restore). */
  onResized(cb: () => void): void;
}

/** The real window through Tauri. */
export function tauriWindowControls(): WindowControls {
  const win = getCurrentWindow();
  return {
    minimize: () => win.minimize(),
    toggleMaximize: () => win.toggleMaximize(),
    close: () => win.close(),
    isMaximized: () => win.isMaximized(),
    onResized: (cb) => { void win.onResized(() => cb()).catch(() => {}); },
  };
}

/** Browser / mock mode: there is no native window. */
export function noopWindowControls(): WindowControls {
  return {
    minimize: async () => {},
    toggleMaximize: async () => {},
    close: async () => {},
    isMaximized: async () => false,
    onResized: () => {},
  };
}

/** Same detection as `createClient()`: Tauri only inside the app and not with `?mock`. */
export function createWindowControls(): WindowControls {
  const search = typeof location !== 'undefined' ? location.search : '';
  if (search.includes('mock') || !('__TAURI_INTERNALS__' in window)) return noopWindowControls();
  return tauriWindowControls();
}

const SVG_NS = 'http://www.w3.org/2000/svg';

function icon(shapes: readonly string[]): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('viewBox', '0 0 16 16');
  svg.setAttribute('width', '14');
  svg.setAttribute('height', '14');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '1.2');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('aria-hidden', 'true');
  for (const d of shapes) {
    const p = document.createElementNS(SVG_NS, 'path');
    p.setAttribute('d', d);
    svg.append(p);
  }
  return svg;
}

const ICON_MINIMIZE = ['M3 8.5h10'];
const ICON_MAXIMIZE = ['M3.5 3.5h9v9h-9z'];
const ICON_RESTORE = ['M5.5 5.5h7v7h-7z', 'M3.5 10.5v-7h7'];
const ICON_CLOSE = ['M3.5 3.5l9 9', 'M12.5 3.5l-9 9'];

/**
 * Own window buttons (the native frame is off) plus a transparent drag strip across the top. Dims like the HUD after
 * `IDLE_MS` without pointer or key activity.
 */
export class TitleBar {
  private readonly bar: HTMLElement;
  private readonly strip: HTMLElement;
  private readonly maxBtn: HTMLButtonElement;
  private readonly labels = new Labels();
  private maximized = false;
  private idleTimer: ReturnType<typeof setTimeout> | null = null;
  private readonly wake = () => this.touch();

  constructor(root: HTMLElement, private readonly controls: WindowControls) {
    // The strip comes first, so the buttons are above it in z-order.
    this.strip = document.createElement('div');
    this.strip.className = 'drag-strip';
    this.strip.setAttribute('data-tauri-drag-region', '');

    this.bar = document.createElement('div');
    this.bar.className = 'titlebar glass';

    const mk = (cls: string, shapes: readonly string[], label: () => string, run: () => Promise<void>): HTMLButtonElement => {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = `titlebar-btn titlebar-${cls}`;
      b.append(icon(shapes));
      this.labels.attr(b, 'aria-label', label);
      this.labels.attr(b, 'title', label);
      b.addEventListener('click', () => { void run().catch((e) => console.error(e)); });
      this.bar.append(b);
      return b;
    };
    mk('minimize', ICON_MINIMIZE, () => S.window.minimize, () => this.controls.minimize());
    this.maxBtn = mk('maximize', ICON_MAXIMIZE, () => (this.maximized ? S.window.restore : S.window.maximize), () => this.controls.toggleMaximize());
    mk('close', ICON_CLOSE, () => S.window.close, () => this.controls.close());

    root.append(this.strip, this.bar);

    window.addEventListener('pointermove', this.wake, { passive: true });
    window.addEventListener('pointerdown', this.wake, { passive: true });
    window.addEventListener('keydown', this.wake);
    this.touch();

    this.controls.onResized(() => { void this.refresh(); });
    void this.refresh();
  }

  /** Labels again after a language switch. */
  relabel(): void { this.labels.apply(); }

  /** Re-reads the maximized state and flips the icon and label. */
  async refresh(): Promise<void> {
    let m = false;
    try { m = await this.controls.isMaximized(); } catch { /* keep the normal icon */ }
    if (m === this.maximized) return;
    this.maximized = m;
    this.bar.classList.toggle('is-maximized', m);
    this.maxBtn.replaceChildren(icon(m ? ICON_RESTORE : ICON_MAXIMIZE));
    this.labels.apply();
  }

  private touch(): void {
    this.bar.classList.remove('is-dim');
    if (this.idleTimer !== null) clearTimeout(this.idleTimer);
    this.idleTimer = setTimeout(() => this.bar.classList.add('is-dim'), IDLE_MS);
  }
}
