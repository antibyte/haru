import { S } from './strings';

export type HudAction = 'mute' | 'settings' | 'fullscreen' | 'stop';

const SEGMENTS = 8;
const IDLE_MS = 3000;
const LATENCY_WARN_MS = 500;
const LATENCY_UPDATE_MS = 250;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** Bottom bar: backend badge, latency, mic meter and the action buttons. */
export class Hud {
  private readonly rootEl: HTMLElement;
  private readonly badge: HTMLElement;
  private readonly latency: HTMLElement;
  private readonly segments: HTMLElement[] = [];
  private readonly muteBtn: HTMLButtonElement;
  private readonly muteLabel: HTMLElement;
  private cb: ((a: HudAction) => void) | null = null;
  private idleTimer: ReturnType<typeof setTimeout> | null = null;
  private lit = 0;
  private lastLatencyAt = 0;
  private lastLatencyText = '';
  private shown = false;
  private readonly wake = () => this.touch();

  constructor(root: HTMLElement) {
    this.rootEl = el('section', 'hud');
    const left = el('div', 'hud-left glass');
    this.badge = el('span', 'hud-badge');
    this.badge.prepend(el('i', 'hud-dot'));
    this.badge.append(el('span', 'hud-badge-text'));
    this.latency = el('span', 'hud-latency num', '– ms');
    this.latency.title = S.hud.latency;
    const meter = el('span', 'hud-meter');
    meter.title = S.hud.mic;
    for (let i = 0; i < SEGMENTS; i++) {
      const s = el('i');
      this.segments.push(s);
      meter.append(s);
    }
    left.append(this.badge, el('span', 'hud-sep'), this.latency, el('span', 'hud-sep'), meter);

    const right = el('div', 'hud-right glass');
    const mk = (action: HudAction, label: string, key?: string): [HTMLButtonElement, HTMLElement] => {
      const b = el('button', `hud-btn hud-${action}`);
      b.type = 'button';
      const l = el('span', 'hud-btn-label', label);
      b.append(l);
      if (key) b.append(el('kbd', undefined, key));
      b.addEventListener('click', () => this.cb?.(action));
      right.append(b);
      return [b, l];
    };
    [this.muteBtn, this.muteLabel] = mk('mute', S.hud.mute, 'M');
    this.muteBtn.setAttribute('aria-pressed', 'false');
    mk('settings', S.hud.settings, 'S');
    mk('fullscreen', S.hud.fullscreen, 'F');
    mk('stop', S.hud.stop);

    this.rootEl.append(left, right);
    root.append(this.rootEl);
  }

  show(): void {
    this.shown = true;
    this.rootEl.classList.add('is-visible');
    window.addEventListener('pointermove', this.wake, { passive: true });
    window.addEventListener('pointerdown', this.wake, { passive: true });
    window.addEventListener('keydown', this.wake);
    this.touch();
  }

  hide(): void {
    this.shown = false;
    this.rootEl.classList.remove('is-visible', 'is-dim');
    window.removeEventListener('pointermove', this.wake);
    window.removeEventListener('pointerdown', this.wake);
    window.removeEventListener('keydown', this.wake);
    if (this.idleTimer !== null) { clearTimeout(this.idleTimer); this.idleTimer = null; }
    this.setMicLevel(0);
  }

  setBadge(label: string): void {
    this.badge.querySelector('.hud-badge-text')!.textContent = label;
  }

  setLatency(ms: number): void {
    const now = performance.now();
    if (now - this.lastLatencyAt < LATENCY_UPDATE_MS) return;
    this.lastLatencyAt = now;
    const text = `${Math.round(ms)} ms`;
    if (text !== this.lastLatencyText) {
      this.lastLatencyText = text;
      this.latency.textContent = text;
    }
    this.latency.classList.toggle('warn', ms > LATENCY_WARN_MS);
  }

  setMicLevel(rms: number): void {
    const lit = Math.round(Math.min(1, Math.sqrt(Math.max(0, rms))) * SEGMENTS);
    if (lit === this.lit) return;
    this.lit = lit;
    this.segments.forEach((s, i) => s.classList.toggle('on', i < lit));
  }

  setMuted(m: boolean): void {
    this.rootEl.classList.toggle('is-muted', m);
    this.muteBtn.setAttribute('aria-pressed', String(m));
    this.muteLabel.textContent = m ? S.hud.unmute : S.hud.mute;
    if (m) this.setMicLevel(0);
  }

  onAction(cb: (a: HudAction) => void): void { this.cb = cb; }

  private touch(): void {
    if (!this.shown) return;
    this.rootEl.classList.remove('is-dim');
    if (this.idleTimer !== null) clearTimeout(this.idleTimer);
    this.idleTimer = setTimeout(() => this.rootEl.classList.add('is-dim'), IDLE_MS);
  }
}
