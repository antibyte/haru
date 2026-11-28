import { Labels, S, fill } from './strings';

/** Longest topic in code points (the core cuts there too). */
export const TOPIC_MAX = 120;

/** A random index in 0..count-1 that is not `avoid` (unless there is only one). */
export function pickTopicIndex(count: number, rand: () => number = Math.random, avoid: number | null = null): number {
  if (count <= 1) return 0;
  const r = Math.min(Math.max(rand(), 0), 0.999999);
  if (avoid === null || avoid < 0 || avoid >= count) return Math.floor(r * count);
  const i = Math.floor(r * (count - 1));
  return i >= avoid ? i + 1 : i;
}

/** The topic that is sent: trimmed, the suggestion when empty, at most TOPIC_MAX code points. */
export function battleTopic(input: string, suggestion: string): string {
  const t = input.trim() || suggestion.trim();
  return [...t].slice(0, TOPIC_MAX).join('').trim();
}

const DICE = '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" aria-hidden="true">'
  + '<rect x="4" y="4" width="16" height="16" rx="3.5"/><g fill="currentColor" stroke="none"><circle cx="9" cy="9" r="1.2"/>'
  + '<circle cx="15" cy="9" r="1.2"/><circle cx="12" cy="12" r="1.2"/><circle cx="9" cy="15" r="1.2"/><circle cx="15" cy="15" r="1.2"/></g></svg>';

/** The battle's topic: a field prefilled with a random suggestion, a dice for another, Start or Enter to begin. */
export class TopicSheet {
  private readonly rootEl: HTMLElement;
  private readonly input: HTMLInputElement;
  private readonly labels = new Labels();
  private index = 0;
  /** The suggestion text last written into the field (an edited field keeps the user's text on relabel). */
  private shown = '';
  private onStart: ((topic: string) => void) | null = null;
  private onClose: (() => void) | null = null;

  constructor(root: HTMLElement, private readonly rand: () => number = Math.random) {
    this.rootEl = document.createElement('section');
    this.rootEl.className = 'overlay topic';
    this.rootEl.setAttribute('role', 'dialog');
    this.rootEl.setAttribute('aria-modal', 'true');
    this.rootEl.setAttribute('aria-labelledby', 'topic-label');
    const scrim = document.createElement('div');
    scrim.className = 'topic-scrim';
    scrim.addEventListener('click', () => this.close());
    const form = document.createElement('form');
    form.className = 'glass topic-card';
    form.noValidate = true;
    const label = this.labels.text(document.createElement('label'), () => S.battle.topicLabel);
    label.className = 'topic-label';
    label.id = 'topic-label';
    label.htmlFor = 'topic-input';
    const row = document.createElement('div');
    row.className = 'topic-row';
    this.input = document.createElement('input');
    this.input.type = 'text';
    this.input.id = 'topic-input';
    this.input.className = 'topic-input';
    this.input.maxLength = TOPIC_MAX;
    this.input.autocomplete = 'off';
    this.input.spellcheck = false;
    this.input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && (e.isComposing || e.keyCode === 229)) e.preventDefault(); // commit the IME, do not submit
    });
    const dice = document.createElement('button');
    dice.type = 'button';
    dice.className = 'icon-btn topic-dice';
    dice.innerHTML = DICE;
    this.labels.attr(dice, 'aria-label', () => S.battle.dice);
    this.labels.attr(dice, 'title', () => S.battle.dice);
    dice.addEventListener('click', () => {
      this.suggest(pickTopicIndex(S.battle.topics.length, this.rand, this.index));
      this.input.focus({ preventScroll: true });
    });
    row.append(this.input, dice);
    const actions = document.createElement('div');
    actions.className = 'topic-actions';
    const cancel = this.labels.text(document.createElement('button'), () => S.settings.cancel);
    cancel.type = 'button';
    cancel.className = 'btn btn-ghost topic-cancel';
    cancel.addEventListener('click', () => this.close());
    const start = this.labels.text(document.createElement('button'), () => S.battle.start);
    start.type = 'submit';
    start.className = 'btn btn-primary topic-start';
    actions.append(cancel, start);
    form.append(label, row, actions);
    form.addEventListener('submit', (e) => {
      e.preventDefault();
      const cb = this.onStart;
      if (!cb) return;
      const topic = battleTopic(this.input.value, this.suggestion);
      this.onClose = null; // starting is not a cancel: focus does not go back to the VS
      this.close();
      cb(topic);
    });
    this.rootEl.append(scrim, form);
    this.rootEl.addEventListener('keydown', (e) => {
      if (e.key === 'Tab') {
        const f: HTMLElement[] = [this.input, dice, cancel, start];
        e.preventDefault();
        const i = f.indexOf(document.activeElement as HTMLElement);
        f[i < 0 ? (e.shiftKey ? f.length - 1 : 0) : (i + (e.shiftKey ? f.length - 1 : 1)) % f.length].focus();
      }
      if (e.key === 'Escape' && this.isOpen && !e.isComposing && e.keyCode !== 229) { e.preventDefault(); this.close(); }
    });
    root.append(this.rootEl);
    this.renderPlaceholder();
  }

  get isOpen(): boolean { return this.rootEl.classList.contains('is-visible'); }

  /** The suggested topic in the active language. */
  get suggestion(): string { return S.battle.topics[this.index] ?? ''; }

  open(onStart: (topic: string) => void, onClose?: () => void): void {
    this.onStart = onStart;
    this.onClose = onClose ?? null;
    this.suggest(pickTopicIndex(S.battle.topics.length, this.rand, null));
    this.rootEl.classList.add('is-visible');
    this.input.focus({ preventScroll: true });
    this.input.select();
  }

  close(): void {
    if (!this.isOpen) return;
    this.rootEl.classList.remove('is-visible');
    this.onStart = null;
    const cb = this.onClose;
    this.onClose = null;
    cb?.();
  }

  /** Labels again; an untouched suggestion becomes the same topic in the new language. */
  relabel(): void {
    const untouched = this.input.value === this.shown;
    this.labels.apply();
    if (untouched) this.fillSuggestion();
    else this.renderPlaceholder();
  }

  private suggest(i: number): void {
    this.index = i;
    this.fillSuggestion();
  }

  private fillSuggestion(): void {
    this.shown = this.suggestion;
    this.input.value = this.shown;
    this.renderPlaceholder();
  }

  private renderPlaceholder(): void {
    this.input.placeholder = fill(S.battle.topicPlaceholder, { topic: this.suggestion });
  }
}
