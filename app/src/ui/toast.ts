import type { HaruError } from '../ipc/types';
import { S } from './strings';

const WARNING_MS = 6000;
const INFO_MS = 4000;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** Top-centre notice: errors (with optional retry and technical details) and auto-hiding warnings and infos. */
export class ErrorToast {
  private readonly rootEl: HTMLElement;
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(root: HTMLElement) {
    this.rootEl = el('div', 'toast');
    this.rootEl.setAttribute('role', 'alert');
    root.append(this.rootEl);
  }

  show(e: HaruError, onRetry?: () => void): void {
    this.reset('is-error');
    const body = el('div', 'toast-body');
    body.append(el('p', 'toast-msg', e.message));

    const actions = el('div', 'toast-actions');
    if (e.detail) {
      const pre = el('pre', 'toast-detail', e.detail);
      pre.hidden = true;
      const toggle = el('button', 'btn btn-link', S.toast.details);
      toggle.type = 'button';
      toggle.setAttribute('aria-expanded', 'false');
      toggle.addEventListener('click', () => {
        pre.hidden = !pre.hidden;
        toggle.setAttribute('aria-expanded', String(!pre.hidden));
      });
      actions.append(toggle);
      body.append(pre);
    }
    if (onRetry) {
      const retry = el('button', 'btn btn-primary btn-small', S.toast.retry);
      retry.type = 'button';
      retry.addEventListener('click', () => { this.hide(); onRetry(); });
      actions.append(retry);
    }
    if (actions.childElementCount > 0) body.append(actions);
    this.rootEl.append(el('i', 'toast-accent'), body);
    this.rootEl.classList.add('is-visible');
  }

  showWarning(text: string): void {
    this.reset('is-warning');
    const body = el('div', 'toast-body');
    body.append(el('p', 'toast-msg', text));
    this.rootEl.append(el('i', 'toast-accent'), body);
    this.rootEl.classList.add('is-visible');
    this.timer = setTimeout(() => this.hide(), WARNING_MS);
  }

  showInfo(text: string): void {
    this.reset('is-info');
    const body = el('div', 'toast-body');
    body.append(el('p', 'toast-msg', text));
    this.rootEl.append(el('i', 'toast-accent'), body);
    this.rootEl.classList.add('is-visible');
    this.timer = setTimeout(() => this.hide(), INFO_MS);
  }

  hide(): void {
    if (this.timer !== null) { clearTimeout(this.timer); this.timer = null; }
    this.rootEl.classList.remove('is-visible');
  }

  private reset(kind: 'is-error' | 'is-warning' | 'is-info'): void {
    if (this.timer !== null) { clearTimeout(this.timer); this.timer = null; }
    this.rootEl.replaceChildren();
    this.rootEl.classList.remove('is-error', 'is-warning', 'is-info');
    this.rootEl.classList.add(kind);
  }
}
