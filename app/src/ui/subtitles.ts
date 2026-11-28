import type { Speaker } from '../ipc/types';
import type { WordEvent } from '../words/assembler';

const MAX_LINE_UNITS = 46; // CJK glyphs count double
const MAX_WORDS = 40;
const FADE_AFTER_MS = 4000;
const CLEAR_AFTER_MS = 5400;
const SENTENCE_END = /[.!?。！？…]$/u;

const units = (s: string) => [...s].reduce((n, ch) => n + (ch.charCodeAt(0) > 0x2e80 ? 2 : 1), 0);

interface Word { id: number; text: string; speaker?: Speaker }

/** Optional bottom-centre captions: the last two lines, fading 4 s after the last event; in battles one colour per voice and a new line at each change. */
export class Subtitles {
  private readonly rootEl: HTMLElement;
  private words: Word[] = [];
  private enabled = false;
  private fadeTimer: ReturnType<typeof setTimeout> | null = null;
  private clearTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(root: HTMLElement) {
    this.rootEl = document.createElement('div');
    this.rootEl.className = 'subtitles';
    this.rootEl.setAttribute('aria-live', 'off');
    root.append(this.rootEl);
  }

  setEnabled(b: boolean): void {
    this.enabled = b;
    if (!b) this.clear();
  }

  handle(e: WordEvent): void {
    if (!this.enabled) return;
    const i = this.words.findIndex((w) => w.id === e.id);
    if (e.type === 'wordEnd') {
      if (i < 0) return;
      this.words[i].text = e.text;
    } else if (i >= 0) {
      this.words[i].text = e.text;
    } else {
      this.words.push(e.speaker ? { id: e.id, text: e.text, speaker: e.speaker } : { id: e.id, text: e.text });
      if (this.words.length > MAX_WORDS) this.words.shift();
    }
    this.render();
    this.rootEl.classList.add('is-visible');
    this.rootEl.classList.remove('is-fading');
    this.armTimers();
  }

  clear(): void {
    this.words = [];
    this.cancelTimers();
    this.rootEl.classList.remove('is-visible', 'is-fading');
    this.rootEl.replaceChildren();
  }

  private render(): void {
    const lines: Word[][] = [[]];
    let len = 0;
    for (const w of this.words) {
      const cur = lines[lines.length - 1];
      const prev = cur[cur.length - 1];
      const w_u = units(w.text) + 1;
      const wrap = cur.length > 0 && (prev.speaker !== w.speaker || len + w_u > MAX_LINE_UNITS || (SENTENCE_END.test(prev.text) && len > 24));
      if (wrap) { lines.push([w]); len = w_u; } else { cur.push(w); len += w_u; }
    }
    const last = this.words[this.words.length - 1];
    const frag = document.createDocumentFragment();
    for (const line of lines.slice(-2)) {
      const row = document.createElement('div');
      row.className = line[0]?.speaker ? `subtitle-line is-${line[0].speaker}` : 'subtitle-line';
      line.forEach((w, k) => {
        const span = document.createElement('span');
        span.className = w === last ? 'sub-word is-latest' : 'sub-word';
        span.textContent = w.text;
        if (k > 0) row.append(' ');
        row.append(span);
      });
      frag.append(row);
    }
    this.rootEl.replaceChildren(frag);
  }

  private armTimers(): void {
    this.cancelTimers();
    this.fadeTimer = setTimeout(() => this.rootEl.classList.add('is-fading'), FADE_AFTER_MS);
    this.clearTimer = setTimeout(() => this.clear(), CLEAR_AFTER_MS);
  }

  private cancelTimers(): void {
    if (this.fadeTimer !== null) clearTimeout(this.fadeTimer);
    if (this.clearTimer !== null) clearTimeout(this.clearTimer);
    this.fadeTimer = this.clearTimer = null;
  }
}
