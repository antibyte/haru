import type { BattlePhase } from '../ipc/types';
import { S } from './strings';

/** While a battle prepares or its buffer refills: the fog keeps moving, one line of text and a subtle progress. */
export class PreparingOverlay {
  private readonly rootEl: HTMLElement;
  private readonly text: HTMLElement;
  private readonly fill: HTMLElement;

  constructor(root: HTMLElement) {
    this.rootEl = document.createElement('section');
    this.rootEl.className = 'overlay preparing';
    this.rootEl.setAttribute('aria-live', 'polite');
    this.text = document.createElement('p');
    this.text.className = 'preparing-text';
    const track = document.createElement('div');
    track.className = 'preparing-track';
    this.fill = document.createElement('div');
    this.fill.className = 'preparing-fill';
    track.append(this.fill);
    this.rootEl.append(this.text, track);
    root.append(this.rootEl);
    this.relabel();
  }

  get isVisible(): boolean { return this.rootEl.classList.contains('is-visible'); }

  /** Visible in `preparing` and `paused`, hidden in `playing`; `prepared` (0..1) is the progress. */
  update(phase: BattlePhase, prepared: number): void {
    const p = Number.isFinite(prepared) ? Math.min(1, Math.max(0, prepared)) : 0;
    this.fill.style.width = `${Math.round(p * 1000) / 10}%`;
    this.rootEl.classList.toggle('is-visible', phase !== 'playing');
  }

  hide(): void { this.rootEl.classList.remove('is-visible'); }

  relabel(): void { this.text.textContent = S.battle.preparing; }
}
