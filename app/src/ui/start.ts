import { S } from './strings';

/** The single glowing call to action shown while Haru is ready. */
export class StartButton {
  private readonly rootEl: HTMLElement;
  private readonly button: HTMLButtonElement;
  private readonly label: HTMLSpanElement;
  private onStart: ((ev: MouseEvent) => void) | null = null;

  constructor(root: HTMLElement) {
    this.rootEl = document.createElement('section');
    this.rootEl.className = 'overlay start';
    this.button = document.createElement('button');
    this.button.type = 'button';
    this.button.className = 'start-btn';
    const halo = document.createElement('span');
    halo.className = 'start-halo';
    halo.setAttribute('aria-hidden', 'true');
    this.label = document.createElement('span');
    this.label.className = 'start-label';
    this.label.textContent = S.start.label;
    this.button.append(halo, this.label);
    this.button.addEventListener('keydown', (e) => {
      if (e.repeat) e.preventDefault(); // a held key must never activate Start
    });
    this.button.addEventListener('click', (e) => {
      if (!this.button.disabled) this.onStart?.(e);
    });
    this.rootEl.append(this.button);
    root.append(this.rootEl);
  }

  show(onStart: (ev: MouseEvent) => void): void {
    this.onStart = onStart;
    this.setBusy(false);
    this.rootEl.classList.add('is-visible');
  }

  setBusy(b: boolean): void {
    this.button.disabled = b;
    this.rootEl.classList.toggle('is-busy', b);
    this.label.textContent = b ? S.start.busy : S.start.label;
  }

  hide(): void {
    this.rootEl.classList.remove('is-visible');
  }
}
