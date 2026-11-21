import type { FluidSim } from '../fluid/sim';
import type { Box, RGB } from '../fluid/params';
import { PAD_EM, type GlyphBitmap, type GlyphRasterizer } from './glyphs';
import { RELEASE_MS, wordPhase } from './lifecycle';
import type { WordEvent } from './assembler';
import type { WordPath } from './path';

/** Em height of a word as a fraction of the viewport height (spec: 10 to 14 %). */
export const WORD_EM_HEIGHT = 0.12;
const MAX_VISUAL_WIDTH = 0.86;
const MARGIN = 0.04;
/** Dye added per frame while the ink phase runs (colour is ~1 per channel; tuned against the soft ceiling). */
const INK_GAIN = 0.062;
const APPEND_INK_GAIN = 0.035;
const HOLD_GAIN = 0.01;
/** The word being spoken right now is held brighter than the fading ones. */
const ACTIVE_HOLD_GAIN = 0.022;
/** After the release phase the edges keep unravelling for this long. */
const UNRAVEL_TAIL_MS = 1000;
const SWIRL = 70;
const PUSH = 30;
/** At most this many words may be in the ink/hold phase; the oldest is released early beyond it. */
const MAX_LIVE = 4;
/** A word keeps blocking placement during this much of its release (glyph region still present). */
const RELEASE_BLOCK_MS = 400;
/** A word is never forced out before it has been visible this long. */
const MIN_VISIBLE_MS = 350;

interface ActiveWord {
  id: number;
  text: string;
  bmp: GlyphBitmap;
  box: Box;
  /** visual (text) width in normalized units, for overflow shifting */
  visW: number;
  /** time of the last ink-restarting event */
  startMs: number;
  endedAt: number | null;
  releaseAt: number | null;
  color: RGB;
  reink: boolean;
  /** display scale chosen by the path when space was tight (1 = full size) */
  scale: number;
  /** set when the word was pushed into release early (too many live words) */
  forceReleaseAt: number | null;
}

/** Drives glyph masks into the fluid: ink, hold (readable), then unravelling edges. */
export class WordInjector {
  private words = new Map<number, ActiveWord>();
  private newest: number | null = null;
  private lastNow = 0;

  constructor(private readonly sim: FluidSim, private readonly glyphs: GlyphRasterizer, private readonly path: WordPath) {}

  private geometry(bmp: GlyphBitmap, scale = 1): { w: number; h: number; visW: number; visH: number } {
    const dom = this.sim.domainAspect;
    let h = WORD_EM_HEIGHT * bmp.heightEm * scale;
    let w = (h * bmp.aspect) / dom;
    const pad = (2 * PAD_EM * (h / bmp.heightEm)) / dom;
    let visW = w - pad;
    if (visW > MAX_VISUAL_WIDTH) {
      const s = MAX_VISUAL_WIDTH / visW;
      h *= s;
      w *= s;
      visW *= s;
    }
    return { w, h, visW, visH: h * (1 / bmp.heightEm) * 1.0 };
  }

  handle(ev: WordEvent, nowMs: number): void {
    const nowSec = nowMs / 1000;
    if (ev.type === 'wordStart') {
      const bmp = this.glyphs.rasterize(ev.text);
      const g = this.geometry(bmp);
      this.limitLive(nowMs);
      const spot = this.path.place(nowSec, g.visW, g.visH * 1.25, this.blockingBoxes(nowMs));
      const g2 = spot.scale < 1 ? this.geometry(bmp, spot.scale) : g;
      const box: Box = { x: spot.x, y: spot.y, w: g2.w, h: g2.h };
      this.words.set(ev.id, {
        id: ev.id, text: ev.text, bmp, box, visW: g2.visW, startMs: nowMs, endedAt: null, releaseAt: null,
        color: [1, 0.6, 0.2], reink: false, scale: spot.scale, forceReleaseAt: null,
      });
      this.newest = ev.id;
      return;
    }
    const w = this.words.get(ev.id);
    if (!w) return;
    if (ev.type === 'wordAppend') {
      if (ev.text !== w.text) {
        const left = w.box.x - w.box.w / 2;
        w.text = ev.text;
        w.bmp = this.glyphs.rasterize(ev.text);
        const g = this.geometry(w.bmp, w.scale);
        let x = left + g.w / 2;
        // keep the visible text inside the margins
        const over = x + g.visW / 2 - (1 - MARGIN);
        if (over > 0) x -= over;
        if (x - g.visW / 2 < MARGIN) x = MARGIN + g.visW / 2;
        w.box = { x, y: w.box.y, w: g.w, h: g.h };
        w.visW = g.visW;
      }
      w.startMs = nowMs;
      w.reink = true;
      this.newest = ev.id;
    } else {
      w.endedAt = nowMs;
      if (ev.text !== w.text) {
        w.text = ev.text;
        w.bmp = this.glyphs.rasterize(ev.text);
      }
    }
  }

  /** Phase of a word taking a forced early release into account. */
  private phaseOf(w: ActiveWord, nowMs: number) {
    const ps = wordPhase(nowMs, w.startMs, w.endedAt, [...w.text].length);
    if (w.forceReleaseAt !== null && nowMs >= w.forceReleaseAt && (ps.phase === 'ink' || ps.phase === 'hold')) {
      const dt = nowMs - w.forceReleaseAt;
      return dt >= RELEASE_MS ? { ...ps, phase: 'done' as const, releaseT: 1 } : { ...ps, phase: 'release' as const, releaseT: Math.max(0, dt) / RELEASE_MS };
    }
    return ps;
  }

  /** Text boxes (with the 1.25 height allowance used for placement) of words that still occupy space. */
  private blockingBoxes(nowMs: number): Box[] {
    const out: Box[] = [];
    for (const w of this.words.values()) {
      const ps = this.phaseOf(w, nowMs);
      const live = ps.phase === 'ink' || ps.phase === 'hold' || (ps.phase === 'release' && ps.releaseT * RELEASE_MS < RELEASE_BLOCK_MS);
      if (live) out.push({ x: w.box.x, y: w.box.y, w: w.visW, h: w.box.h * 1.25 });
    }
    return out;
  }

  /** Forces the oldest ink/hold words into release so that room remains for one more. */
  private limitLive(nowMs: number): void {
    for (;;) {
      const live = [...this.words.values()].filter((w) => { const p = this.phaseOf(w, nowMs).phase; return p === 'ink' || p === 'hold'; });
      if (live.length < MAX_LIVE) return;
      live.sort((a, b) => a.startMs - b.startMs);
      const oldest = live[0];
      oldest.forceReleaseAt = Math.max(nowMs, oldest.startMs + MIN_VISIBLE_MS);
      // a not-yet-due forced release still counts as live; stop to avoid looping on it
      if (this.phaseOf(oldest, nowMs).phase !== 'release') return;
    }
  }

  update(nowMs: number, color: RGB): void {
    const sim = this.sim;
    const fk = this.lastNow > 0 ? Math.min(3, Math.max(0, (nowMs - this.lastNow) / (1000 / 60))) : 1; // per-60 Hz-frame gains
    this.lastNow = nowMs;
    sim.beginRestMask();
    for (const [id, w] of this.words) {
      const bmp = this.glyphs.rasterize(w.text); // refreshes the LRU so an active texture is never evicted
      w.bmp = bmp;
      const ps = this.phaseOf(w, nowMs);
      if (ps.phase === 'ink') {
        w.color = color;
        const gain = (w.reink ? APPEND_INK_GAIN : INK_GAIN);
        sim.maskSplatDye(bmp.texture, w.box, hot(w.color), gain * fk);
      } else if (ps.phase === 'hold') {
        sim.maskSplatDye(bmp.texture, w.box, hot(w.color, 1), (id === this.newest ? ACTIVE_HOLD_GAIN : HOLD_GAIN) * fk);
        sim.addRestMask(bmp.texture, w.box);
      } else {
        w.releaseAt ??= nowMs - (ps.phase === 'done' ? RELEASE_MS : ps.releaseT * RELEASE_MS);
        const t = nowMs - w.releaseAt;
        const total = RELEASE_MS + UNRAVEL_TAIL_MS;
        if (t >= total) {
          this.words.delete(id);
          if (this.newest === id) this.newest = null;
          continue;
        }
        const k = (1 - t / total) ** 1.5;
        sim.edgeSplatVelocity(bmp.texture, bmp.texel, w.box, SWIRL * k, PUSH * k);
      }
    }
  }

  /** Centre of the most recently touched word that is still visible, or null. */
  activeCenter(): { x: number; y: number } | null {
    if (this.newest === null) return null;
    const w = this.words.get(this.newest);
    return w ? { x: w.box.x, y: w.box.y } : null;
  }

  /** Number of words currently in the fluid (debug overlay). */
  get count(): number {
    return this.words.size;
  }

  /** Ends all words that are still open. */
  endAll(nowMs: number): void {
    for (const w of this.words.values()) w.endedAt ??= nowMs;
  }
}

/** Pushes the ink colour toward a white-hot core; ageing then cools it through magenta to violet. */
function hot(c: RGB, k = 1): RGB {
  const m = 0.72 * k;
  // warm white-gold core: blue stays low so the edges fall to gold/amber instead of pink
  return [c[0] + (1.0 - c[0]) * m, c[1] + (0.88 - c[1]) * m, c[2] + (0.58 - c[2]) * m];
}
