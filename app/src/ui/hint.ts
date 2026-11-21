import { S } from './strings';

const HEADPHONES = `<svg viewBox="0 0 48 48" width="44" height="44" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
<path d="M8 28v-4a16 16 0 0 1 32 0v4"/><rect x="6" y="28" width="8" height="12" rx="3.5"/><rect x="34" y="28" width="8" height="12" rx="3.5"/></svg>`;

/** One-time hint card shown before the first conversation. */
export class HeadphoneHint {
  private readonly rootEl: HTMLElement;
  private readonly button: HTMLButtonElement;
  private resolve: (() => void) | null = null;

  constructor(root: HTMLElement) {
    this.rootEl = document.createElement('section');
    this.rootEl.className = 'overlay hint';
    this.rootEl.setAttribute('role', 'dialog');
    this.rootEl.setAttribute('aria-modal', 'true');
    const card = document.createElement('div');
    card.className = 'glass hint-card';
    const icon = document.createElement('div');
    icon.className = 'hint-icon';
    icon.innerHTML = HEADPHONES;
    const text = document.createElement('p');
    text.className = 'hint-text';
    text.textContent = S.hint.text;
    this.button = document.createElement('button');
    this.button.className = 'btn btn-primary';
    this.button.type = 'button';
    this.button.textContent = S.hint.ok;
    this.button.addEventListener('click', () => this.close());
    card.append(icon, text, this.button);
    this.rootEl.append(card);
    root.append(this.rootEl);
  }

  /** Resolves when the user confirms with "Verstanden". */
  show(): Promise<void> {
    return new Promise<void>((resolve) => {
      this.resolve = resolve;
      this.rootEl.classList.add('is-visible');
      this.button.focus({ preventScroll: true });
    });
  }

  /** Closes the card as if confirmed (test automation). */
  dismiss(): void {
    this.close();
  }

  private close(): void {
    this.rootEl.classList.remove('is-visible');
    const r = this.resolve;
    this.resolve = null;
    r?.();
  }
}
