import type { Persona } from '../ipc/types';
import type { RectLike, VsRects } from './arrows';
import { S } from './strings';

const ORDER: { persona: Persona; img: string }[] = [
  { persona: 'angel', img: 'angel' },
  { persona: 'daemon', img: 'devil' },
];

/** What the chooser marks as busy: a persona, the battle, or nothing. */
export type ChooserBusy = Persona | 'battle' | false;

/** Side by side: emblem size limits (CSS px) and its share of the image row's height. */
export const VS_MIN = 84;
export const VS_MAX = 210;
export const VS_ROW_SHARE = 0.3;
/** Share of the emblem above the image row's top edge; the rest sits in the gap between the wings. */
export const VS_ABOVE = 0.72;
/** Least distance (CSS px) between the title and the emblem. */
export const VS_TITLE_GAP = 10;
/** Stacked (narrow windows): size limits and share of the smaller image height. */
export const VS_STACKED_MIN = 56;
export const VS_STACKED_MAX = 104;
export const VS_STACKED_SHARE = 0.24;

/** Emblem centre relative to the row's top-left, and its square size (CSS px). */
export interface VsPlacement { x: number; y: number; size: number; stacked: boolean }

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/**
 * Where the VS emblem goes (all inputs CSS px, viewport). Side by side: on the seam between the images, mostly above
 * them and never above the title. Stacked: on the seam between the upper and the lower image.
 */
export function placeVs(row: RectLike, angel: RectLike, daemon: RectLike, titleBottom: number): VsPlacement {
  const stacked = daemon.top >= angel.top + angel.height / 2;
  if (stacked) {
    const size = clamp(VS_STACKED_SHARE * Math.min(angel.height, daemon.height), VS_STACKED_MIN, VS_STACKED_MAX);
    return { x: angel.left + angel.width / 2 - row.left, y: (angel.top + angel.height + daemon.top) / 2 - row.top, size, stacked };
  }
  const size = clamp(VS_ROW_SHARE * row.height, VS_MIN, VS_MAX);
  const top = Math.max(row.top - VS_ABOVE * size, titleBottom + VS_TITLE_GAP);
  return { x: (angel.left + angel.width + daemon.left) / 2 - row.left, y: top + size / 2 - row.top, size, stacked };
}

/** Labels are read at the moment of use so a language switch reaches them. */
const labelOf = (p: Persona): string => (p === 'angel' ? S.chooser.angel : S.chooser.daemon);
const subOf = (p: Persona): string => (p === 'angel' ? S.chooser.angelSub : S.chooser.daemonSub);

/** Start screen: angel left, daemon right, the VS emblem between them for a battle. */
export class PersonaChooser {
  private readonly rootEl: HTMLElement;
  private readonly title: HTMLElement;
  private readonly row: HTMLElement;
  private readonly buttons = new Map<Persona, HTMLButtonElement>();
  private readonly labels = new Map<Persona, HTMLElement>();
  private readonly subs = new Map<Persona, HTMLElement>();
  private readonly vs: HTMLButtonElement;
  private readonly vsLabel: HTMLElement;
  private readonly vsSub: HTMLElement;
  private readonly vsHint: HTMLElement;
  private onPick: ((p: Persona, ev: MouseEvent) => void) | null = null;
  private onBattle: (() => void) | null = null;
  private activeCb: ((on: boolean) => void) | null = null;
  private busy: ChooserBusy = false;
  private battleOk = false;
  private hovered = false;
  private focused = false;
  private active = false;

  constructor(root: HTMLElement) {
    this.rootEl = document.createElement('section');
    this.rootEl.className = 'overlay chooser';
    this.title = document.createElement('h1');
    this.title.className = 'chooser-title';
    this.row = document.createElement('div');
    this.row.className = 'chooser-row';
    const onKey = (e: KeyboardEvent) => this.onKey(e);

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
      b.addEventListener('keydown', onKey);
      b.addEventListener('click', (e) => {
        if (!b.disabled) this.onPick?.(d.persona, e);
      });
      this.buttons.set(d.persona, b);
      this.labels.set(d.persona, label);
      this.subs.set(d.persona, sub);
    }

    this.vs = document.createElement('button');
    this.vs.type = 'button';
    this.vs.className = 'chooser-vs is-unavailable';
    this.vs.style.position = 'absolute'; // out of the flex flow: the angel and the daemon keep their boxes
    const vsImg = document.createElement('img');
    vsImg.className = 'chooser-vs-img';
    vsImg.src = `${import.meta.env.BASE_URL}personas/vs.webp`;
    vsImg.alt = '';
    vsImg.draggable = false;
    vsImg.decoding = 'async';
    const caption = document.createElement('span');
    caption.className = 'chooser-vs-caption';
    this.vsLabel = document.createElement('span');
    this.vsLabel.className = 'chooser-vs-label';
    this.vsSub = document.createElement('span');
    this.vsSub.className = 'chooser-vs-sub';
    this.vsHint = document.createElement('span');
    this.vsHint.className = 'chooser-vs-hint';
    caption.append(this.vsLabel, this.vsSub, this.vsHint);
    this.vs.append(vsImg, caption);
    this.vs.addEventListener('keydown', onKey);
    this.vs.addEventListener('click', () => {
      if (!this.vs.disabled && this.battleOk) this.onBattle?.();
    });
    this.vs.addEventListener('pointerenter', () => { this.hovered = true; this.updateActive(); });
    this.vs.addEventListener('pointerleave', () => { this.hovered = false; this.updateActive(); });
    this.vs.addEventListener('focus', () => { this.focused = true; this.updateActive(); });
    this.vs.addEventListener('blur', () => { this.focused = false; this.updateActive(); });

    this.row.append(this.buttons.get('angel')!, this.vs, this.buttons.get('daemon')!);
    this.rootEl.append(this.title, this.row);
    root.append(this.rootEl);
    window.addEventListener('resize', () => { if (this.isVisible) this.layout(); });
    document.fonts?.ready.then(() => { if (this.isVisible) this.layout(); });
    this.relabel();
  }

  private get isVisible(): boolean { return this.rootEl.classList.contains('is-visible'); }

  show(onPick: (p: Persona, ev: MouseEvent) => void, onBattle?: () => void): void {
    this.onPick = onPick;
    this.onBattle = onBattle ?? null;
    this.setBusy(false);
    this.rootEl.classList.add('is-visible');
    this.layout();
    this.updateActive();
  }

  setBusy(p: ChooserBusy): void {
    this.busy = p;
    this.rootEl.classList.toggle('is-busy', p !== false);
    for (const d of ORDER) {
      const b = this.buttons.get(d.persona)!;
      b.disabled = p !== false;
      b.classList.toggle('is-chosen', p === d.persona);
    }
    this.vs.disabled = p !== false;
    this.vs.classList.toggle('is-chosen', p === 'battle');
    this.renderLabels();
    this.updateActive();
  }

  hide(): void {
    this.rootEl.classList.remove('is-visible');
    this.updateActive();
  }

  /** Whether the loaded worker can run a battle; without it the VS is dimmed and shows why. */
  setBattleAvailable(ok: boolean): void {
    this.battleOk = ok;
    this.vs.classList.toggle('is-unavailable', !ok);
    this.vs.setAttribute('aria-disabled', String(!ok));
    this.renderLabels();
    this.updateActive();
  }

  /** `on` while the VS is hovered or focused (and usable): the app forms the arrows in the ink. */
  onVsActive(cb: (on: boolean) => void): void { this.activeCb = cb; }

  focusVs(): void { this.vs.focus({ preventScroll: true }); }

  /** The boxes the hover arrows are computed from. */
  rects(): VsRects {
    return { vs: this.vs.getBoundingClientRect(), angel: this.buttons.get('angel')!.getBoundingClientRect(), daemon: this.buttons.get('daemon')!.getBoundingClientRect() };
  }

  /** Places the VS from the current layout (on show and on every resize). */
  layout(): void {
    const p = placeVs(this.row.getBoundingClientRect(), this.buttons.get('angel')!.getBoundingClientRect(),
      this.buttons.get('daemon')!.getBoundingClientRect(), this.title.getBoundingClientRect().bottom);
    const s = this.vs.style;
    s.left = `${p.x}px`;
    s.top = `${p.y}px`;
    s.width = `${p.size}px`;
    s.height = `${p.size}px`;
    this.row.style.setProperty('--vs-lift', p.stacked ? `${Math.round(p.size / 2 + 6)}px` : '0px');
  }

  /** Title, names and sub lines in the active language; a busy entry keeps its busy label. */
  relabel(): void {
    this.title.textContent = S.chooser.title;
    for (const d of ORDER) {
      this.buttons.get(d.persona)!.setAttribute('aria-label', `${labelOf(d.persona)} – ${subOf(d.persona)}`);
      this.subs.get(d.persona)!.textContent = subOf(d.persona);
    }
    this.vsSub.textContent = S.chooser.battleSub;
    this.vsHint.textContent = S.battle.unsupported;
    this.renderLabels();
    if (this.isVisible) this.layout();
  }

  private renderLabels(): void {
    for (const d of ORDER) {
      this.labels.get(d.persona)!.textContent = this.busy === d.persona ? S.start.busy : labelOf(d.persona);
    }
    this.vsLabel.textContent = this.busy === 'battle' ? S.start.busy : S.chooser.battle;
    this.vs.setAttribute('aria-label', `${S.chooser.battle} – ${this.battleOk ? S.chooser.battleSub : S.battle.unsupported}`);
    if (this.battleOk) this.vs.removeAttribute('title');
    else this.vs.title = S.battle.unsupported;
  }

  /** angel → VS → daemon with ArrowLeft/ArrowRight (wrapping); a held key never activates anything. */
  private onKey(e: KeyboardEvent): void {
    if (e.repeat) { e.preventDefault(); return; }
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    const order = [this.buttons.get('angel')!, this.vs, this.buttons.get('daemon')!];
    const i = order.indexOf(e.currentTarget as HTMLButtonElement);
    order[(i + (e.key === 'ArrowRight' ? 1 : order.length - 1)) % order.length].focus();
  }

  private updateActive(): void {
    const on = (this.hovered || this.focused) && this.battleOk && this.busy === false && this.isVisible;
    if (on === this.active) return;
    this.active = on;
    this.activeCb?.(on);
  }
}
