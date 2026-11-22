import type { HaruError, UiText } from '../ipc/types';
import { S, errorText, uiText } from './strings';

const WARNING_MS = 6000;
const INFO_MS = 4000;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

type Current = { kind: 'error'; e: HaruError } | { kind: 'warning' | 'info'; t: UiText } | null;

/** Top-centre notice: errors (with optional retry and technical details) and auto-hiding warnings and infos. */
export class ErrorToast {
  private readonly rootEl: HTMLElement;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private current: Current = null;
  private msg: HTMLElement | null = null;
  private detailsBtn: HTMLButtonElement | null = null;
  private retryBtn: HTMLButtonElement | null = null;

  constructor(root: HTMLElement) {
    this.rootEl = el('div', 'toast');
    this.rootEl.setAttribute('role', 'alert');
    root.append(this.rootEl);
  }

  show(e: HaruError, onRetry?: () => void): void {
    this.reset('is-error');
    this.current = { kind: 'error', e };
    const body = el('div', 'toast-body');
    this.msg = el('p', 'toast-msg', errorText(e));
    body.append(this.msg);

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
      this.detailsBtn = toggle;
    }
    if (onRetry) {
      const retry = el('button', 'btn btn-primary btn-small', S.toast.retry);
      retry.type = 'button';
      retry.addEventListener('click', () => { this.hide(); onRetry(); });
      actions.append(retry);
      this.retryBtn = retry;
    }
    if (actions.childElementCount > 0) body.append(actions);
    this.rootEl.append(el('i', 'toast-accent'), body);
    this.rootEl.classList.add('is-visible');
  }

  showWarning(t: UiText): void {
    this.showText('warning', t, WARNING_MS);
  }

  showInfo(t: UiText): void {
    this.showText('info', t, INFO_MS);
  }

  hide(): void {
    if (this.timer !== null) { clearTimeout(this.timer); this.timer = null; }
    this.rootEl.classList.remove('is-visible');
  }

  /** Message and buttons in the active language; a running auto-hide timer keeps running. */
  relabel(): void {
    const c = this.current;
    if (!c || !this.msg) return;
    this.msg.textContent = c.kind === 'error' ? errorText(c.e) : uiText(c.t);
    if (this.detailsBtn) this.detailsBtn.textContent = S.toast.details;
    if (this.retryBtn) this.retryBtn.textContent = S.toast.retry;
  }

  private showText(kind: 'warning' | 'info', t: UiText, ms: number): void {
    this.reset(kind === 'warning' ? 'is-warning' : 'is-info');
    this.current = { kind, t };
    const body = el('div', 'toast-body');
    this.msg = el('p', 'toast-msg', uiText(t));
    body.append(this.msg);
    this.rootEl.append(el('i', 'toast-accent'), body);
    this.rootEl.classList.add('is-visible');
    this.timer = setTimeout(() => this.hide(), ms);
  }

  private reset(kind: 'is-error' | 'is-warning' | 'is-info'): void {
    if (this.timer !== null) { clearTimeout(this.timer); this.timer = null; }
    this.rootEl.replaceChildren();
    this.rootEl.classList.remove('is-error', 'is-warning', 'is-info');
    this.rootEl.classList.add(kind);
    this.current = null;
    this.msg = this.detailsBtn = this.retryBtn = null;
  }
}
