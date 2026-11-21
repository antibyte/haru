import type { DownloadEvent } from '../ipc/types';
import { formatBytes, formatEta, formatRate } from './format';
import { S } from './strings';

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

  constructor(root: HTMLElement) {
    this.rootEl = el('section', 'overlay boot');
    this.rootEl.setAttribute('aria-live', 'polite');
    const mark = el('h1', 'boot-mark', S.appName);
    this.status = el('p', 'boot-status', S.boot.detecting);
    this.progress = el('div', 'boot-progress');
    const track = el('div', 'boot-track');
    this.fill = el('div', 'boot-fill');
    track.append(this.fill);
    this.meta = el('p', 'boot-meta');
    this.progress.append(track, this.meta);
    this.rootEl.append(mark, this.status, this.progress);
    root.append(this.rootEl);
  }

  show(): void { this.rootEl.classList.add('is-visible'); }
  hide(): void { this.rootEl.classList.remove('is-visible'); }

  setStatus(text: string): void { this.status.textContent = text; }

  onDownload(e: DownloadEvent): void {
    switch (e.event) {
      case 'progress': {
        const { downloaded, total, bytesPerSec } = e.data;
        this.smoothedBps = this.smoothedBps === 0 ? bytesPerSec : this.smoothedBps * 0.8 + bytesPerSec * 0.2;
        this.progress.classList.add('is-active');
        this.progress.classList.remove('is-verifying', 'is-error');
        this.fill.style.width = `${total > 0 ? Math.min(100, (downloaded / total) * 100) : 0}%`;
        const eta = this.smoothedBps > 0 ? (total - downloaded) / this.smoothedBps : Infinity;
        this.setMeta([
          `${formatBytes(downloaded)} / ${formatBytes(total)}`,
          formatRate(this.smoothedBps),
          `noch ${formatEta(eta)}`,
        ]);
        break;
      }
      case 'verifying': {
        const pct = e.data.total > 0 ? Math.min(100, (e.data.done / e.data.total) * 100) : 0;
        this.progress.classList.add('is-active', 'is-verifying');
        this.fill.style.width = `${pct}%`;
        this.setMeta([`${S.boot.verifying.replace('…', '')}… ${Math.round(pct)} %`]);
        break;
      }
      case 'done':
        this.fill.style.width = '100%';
        this.progress.classList.remove('is-verifying');
        this.setMeta([]);
        break;
      case 'error':
        this.progress.classList.add('is-active', 'is-error');
        this.setMeta([e.data.message]);
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
