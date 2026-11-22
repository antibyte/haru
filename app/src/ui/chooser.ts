import type { Persona } from '../ipc/types';
import { S } from './strings';

const ORDER: { persona: Persona; img: string }[] = [
  { persona: 'angel', img: 'angel' },
  { persona: 'daemon', img: 'devil' },
];

/** Labels are read at the moment of use so a language switch reaches them. */
const labelOf = (p: Persona): string => (p === 'angel' ? S.chooser.angel : S.chooser.daemon);
const subOf = (p: Persona): string => (p === 'angel' ? S.chooser.angelSub : S.chooser.daemonSub);

/** Start screen: an angel on the left, a devil on the right; a click picks who Haru is. */
export class PersonaChooser {
  private readonly rootEl: HTMLElement;
  private readonly title: HTMLElement;
  private readonly buttons = new Map<Persona, HTMLButtonElement>();
  private readonly labels = new Map<Persona, HTMLElement>();
  private readonly subs = new Map<Persona, HTMLElement>();
  private onPick: ((p: Persona, ev: MouseEvent) => void) | null = null;
  private busy: Persona | false = false;

  constructor(root: HTMLElement) {
    this.rootEl = document.createElement('section');
    this.rootEl.className = 'overlay chooser';
    this.title = document.createElement('h1');
    this.title.className = 'chooser-title';
    const row = document.createElement('div');
    row.className = 'chooser-row';

    for (const d of ORDER) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'persona';
      b.dataset.persona = d.persona;
      const img = document.createElement('img');
      img.className = 'persona-img';
      img.src = `${import.meta.env.BASE_URL}personas/${d.img}.webp`;
      img.alt = '';
      img.draggable = false;
      img.decoding = 'async';
      img.addEventListener('error', () => { img.hidden = true; });
      const label = document.createElement('span');
      label.className = 'persona-label';
      const sub = document.createElement('span');
      sub.className = 'persona-sub';
      b.append(img, label, sub);
      b.addEventListener('keydown', (e) => {
        if (e.repeat) { e.preventDefault(); return; } // a held key must never activate a persona
        if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
          e.preventDefault();
          this.buttons.get(d.persona === 'angel' ? 'daemon' : 'angel')?.focus();
        }
      });
      b.addEventListener('click', (e) => {
        if (!b.disabled) this.onPick?.(d.persona, e);
      });
      this.buttons.set(d.persona, b);
      this.labels.set(d.persona, label);
      this.subs.set(d.persona, sub);
      row.append(b);
    }
    this.rootEl.append(this.title, row);
    root.append(this.rootEl);
    this.relabel();
  }

  show(onPick: (p: Persona, ev: MouseEvent) => void): void {
    this.onPick = onPick;
    this.setBusy(false);
    this.rootEl.classList.add('is-visible');
  }

  setBusy(p: Persona | false): void {
    this.busy = p;
    this.rootEl.classList.toggle('is-busy', p !== false);
    for (const d of ORDER) {
      const b = this.buttons.get(d.persona)!;
      b.disabled = p !== false;
      b.classList.toggle('is-chosen', p === d.persona);
    }
    this.renderLabels();
  }

  hide(): void {
    this.rootEl.classList.remove('is-visible');
  }

  /** Title, persona names and subtitles in the active language; a busy persona keeps its busy label. */
  relabel(): void {
    this.title.textContent = S.chooser.title;
    for (const d of ORDER) {
      this.buttons.get(d.persona)!.setAttribute('aria-label', `${labelOf(d.persona)} – ${subOf(d.persona)}`);
      this.subs.get(d.persona)!.textContent = subOf(d.persona);
    }
    this.renderLabels();
  }

  private renderLabels(): void {
    for (const d of ORDER) {
      this.labels.get(d.persona)!.textContent = this.busy === d.persona ? S.start.busy : labelOf(d.persona);
    }
  }
}
