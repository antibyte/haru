import type { DownloadEvent } from '../ipc/types';
import { formatBytes, formatEta, formatRate } from './format';
import { APP_NAME, S, fill, formatPercent, uiText } from './strings';

export type BootStatus = 'detecting' | 'loading' | 'downloading' | 'verifying';

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** Centered wordmark, status line and the glowing download line. */
export class BootScreen {
  private readonly rootEl: HTMLElement;
  private readonly status: HTMLElement;
  private readonly progress: HTMLElement;
  private readonly fill: HTMLElement;
  private readonly meta: HTMLElement;
  private smoothedBps = 0;
  private statusKey: BootStatus = 'detecting';
  /** The last download event, so the meta line can be drawn again in another language. */
  private lastEvent: DownloadEvent | null = null;

  constructor(root: HTMLElement) {
    this.rootEl = el('section', 'overlay boot');
    this.rootEl.setAttribute('aria-live', 'polite');
    const mark = el('h1', 'boot-mark', APP_NAME);
    this.status = el('p', 'boot-status');
    this.progress = el('div', 'boot-progress');
    const track = el('div', 'boot-track');
    this.fill = el('div', 'boot-fill');
    track.append(this.fill);
    this.meta = el('p', 'boot-meta');
    this.progress.append(track, this.meta);
    this.rootEl.append(mark, this.status, this.progress);
    root.append(this.rootEl);
    this.renderStatus();
  }

  show(): void { this.rootEl.classList.add('is-visible'); }
  hide(): void { this.rootEl.classList.remove('is-visible'); }

  setStatus(key: BootStatus): void {
    this.statusKey = key;
    this.renderStatus();
  }

  onDownload(e: DownloadEvent): void {
    switch (e.event) {
      case 'progress': {
        const { downloaded, total, bytesPerSec } = e.data;
        this.smoothedBps = this.smoothedBps === 0 ? bytesPerSec : this.smoothedBps * 0.8 + bytesPerSec * 0.2;
        this.progress.classList.add('is-active');
        this.progress.classList.remove('is-verifying', 'is-error');
        this.fill.style.width = `${total > 0 ? Math.min(100, (downloaded / total) * 100) : 0}%`;
        break;
      }
      case 'verifying':
        this.progress.classList.add('is-active', 'is-verifying');
        this.fill.style.width = `${verifyPercent(e.data)}%`;
        break;
      case 'done':
        this.fill.style.width = '100%';
        this.progress.classList.remove('is-verifying');
        break;
      case 'error':
        this.progress.classList.add('is-active', 'is-error');
        break;
    }
    this.lastEvent = e;
    this.renderMeta(e);
  }

  /** Draws status and meta line again in the active language. */
  relabel(): void {
    this.renderStatus();
    this.renderMeta(this.lastEvent);
  }

  private renderStatus(): void {
    this.status.textContent = S.boot[this.statusKey];
  }

  private renderMeta(e: DownloadEvent | null): void {
    switch (e?.event) {
      case 'progress': {
        const { downloaded, total } = e.data;
        const eta = this.smoothedBps > 0 ? (total - downloaded) / this.smoothedBps : Infinity;
        this.setMeta([
          `${formatBytes(downloaded)} / ${formatBytes(total)}`,
          formatRate(this.smoothedBps),
          fill(S.boot.remaining, { eta: formatEta(eta) }),
        ]);
        break;
      }
      case 'verifying':
        this.setMeta([fill(S.boot.verifyingProgress, { pct: formatPercent(verifyPercent(e.data)) })]);
        break;
      case 'error':
        this.setMeta([uiText(e.data)]);
        break;
      default: // done, or nothing yet
        this.setMeta([]);
        break;
    }
  }

  private setMeta(parts: string[]): void {
    this.meta.replaceChildren();
    parts.forEach((p, i) => {
      if (i > 0) this.meta.append(el('span', 'sep', '·'));
      this.meta.append(el('span', undefined, p));
    });
  }
}

function verifyPercent(d: { done: number; total: number }): number {
  return d.total > 0 ? Math.min(100, (d.done / d.total) * 100) : 0;
}
