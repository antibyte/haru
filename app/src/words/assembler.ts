import type { Speaker, WordPiece } from '../ipc/types';

export type WordEvent = { type: 'wordStart' | 'wordAppend' | 'wordEnd'; id: number; text: string; seq: number; speaker?: Speaker };

const PUNCT = new Set('.,!?;:…。、！？，；：');
const CJK_MAX_LEN = 8;
const CJK_EMPTY_END = 2;
const LATIN_EMPTY_END = 6;

function isCjk(ch: string): boolean {
  const c = ch.codePointAt(0)!;
  return (c >= 0x3400 && c <= 0x4dbf) || (c >= 0x4e00 && c <= 0x9fff) // Han
    || (c >= 0x3040 && c <= 0x309f)                                    // Hiragana
    || (c >= 0x30a0 && c <= 0x30ff) || (c >= 0xff66 && c <= 0xff9f);   // Katakana
}

interface OpenWord { id: number; text: string; cjk: boolean; len: number }
interface Touch { id: number; started: boolean; changed: boolean; ended: boolean; text: string }

/** An id source 1, 2, 3, …; assemblers that share one never hand out the same word id. */
export function wordIds(): () => number {
  let next = 1;
  return () => next++;
}

/**
 * Turns the stream of model text frames (sub-word pieces) into word events.
 * Ids are monotonic over the lifetime of the id source (reset() keeps counting so
 * words still dissolving from an earlier conversation never collide).
 */
export class WordAssembler {
  /** `nextId` hands out word ids (share one source between the two voices of a battle); `speaker` tags every event. */
  constructor(private readonly nextId: () => number = wordIds(), private readonly speaker?: Speaker) {}

  private cur: OpenWord | null = null;
  private emptyRun = 0;
  private lastSeq = 0;

  push(p: WordPiece): WordEvent[] {
    this.lastSeq = p.seq;
    const touches: Touch[] = [];
    const touch = (w: OpenWord): Touch => {
      let t = touches.find((x) => x.id === w.id);
      if (!t) { t = { id: w.id, started: false, changed: false, ended: false, text: w.text }; touches.push(t); }
      return t;
    };
    const end = () => {
      if (!this.cur) return;
      const t = touch(this.cur);
      t.ended = true;
      t.text = this.cur.text;
      this.cur = null;
    };

    if (p.text === '') {
      this.emptyRun++;
      if (this.cur) {
        const limit = this.cur.cjk ? CJK_EMPTY_END : LATIN_EMPTY_END;
        if (this.emptyRun >= limit) end();
      }
    } else {
      this.emptyRun = 0;
      for (const ch of p.text) {
        if (/\s/u.test(ch)) { end(); continue; }
        if (PUNCT.has(ch)) {
          if (this.cur) {
            this.cur.text += ch;
            this.cur.len++;
            const t = touch(this.cur);
            t.changed = true;
            end();
          }
          continue;
        }
        const c = isCjk(ch);
        if (this.cur && (this.cur.cjk !== c || (c && this.cur.len >= CJK_MAX_LEN))) end();
        if (!this.cur) {
          this.cur = { id: this.nextId(), text: '', cjk: c, len: 0 };
          touch(this.cur).started = true;
        }
        this.cur.text += ch;
        this.cur.len++;
        touch(this.cur).changed = true;
      }
      if (this.cur) touch(this.cur).text = this.cur.text;
    }

    const out: WordEvent[] = [];
    for (const t of touches) {
      if (t.started) out.push(this.event('wordStart', t.id, t.text, p.seq));
      else if (t.changed) out.push(this.event('wordAppend', t.id, t.text, p.seq));
      if (t.ended) out.push(this.event('wordEnd', t.id, t.text, p.seq));
    }
    return out;
  }

  /** Ends the open word (conversation end). */
  flush(): WordEvent[] {
    if (!this.cur) return [];
    const w = this.cur;
    this.cur = null;
    this.emptyRun = 0;
    return [this.event('wordEnd', w.id, w.text, this.lastSeq)];
  }

  private event(type: WordEvent['type'], id: number, text: string, seq: number): WordEvent {
    return this.speaker ? { type, id, text, seq, speaker: this.speaker } : { type, id, text, seq };
  }

  reset(): void {
    this.cur = null;
    this.emptyRun = 0;
  }
}
