/** Center + size in normalized canvas coordinates, y up. Structurally identical to fluid/sim.ts `Box`. */
export interface Box { x: number; y: number; w: number; h: number }

/** A placed box plus the display scale (1 = requested size) that was needed to find free space. */
export interface Placement extends Box { scale: number }

export interface WordPathOptions {
  ax: number; ay: number; fx: number; fy: number; phase: number; margin: number; candidates: number; step: number;
  /** Gap kept between words, as a fraction of the new word's height. */
  gap: number;
  /** Grid pitch of the free-area search (normalized units). */
  grid: number;
  /** Display scales tried in order when no free spot exists. */
  scales: number[];
}

const DEFAULTS: WordPathOptions = {
  ax: 0.32, ay: 0.28, fx: 0.13, fy: 0.17, phase: Math.PI / 2, margin: 0.04, candidates: 12, step: 0.4,
  gap: 0.15, grid: 0.02, scales: [1, 0.9, 0.8, 0.7, 0.6],
};

function overlapArea(a: Box, b: Box, gap: number): number {
  const ox = (a.w + b.w) / 2 + gap - Math.abs(a.x - b.x);
  const oy = (a.h + b.h) / 2 + gap - Math.abs(a.y - b.y);
  return ox > 0 && oy > 0 ? ox * oy : 0;
}

/** Slow Lissajous path that decides where the next word appears, avoiding words that are still live. */
export class WordPath {
  private readonly o: WordPathOptions;
  private boxes: { box: Box; until: number }[] = [];

  constructor(o: Partial<WordPathOptions> = {}) {
    this.o = { ...DEFAULTS, ...o };
  }

  positionAt(t: number): { x: number; y: number } {
    const { ax, ay, fx, fy, phase } = this.o;
    return { x: 0.5 + ax * Math.sin(fx * t + phase), y: 0.5 + ay * Math.sin(fy * t) };
  }

  /**
   * Finds a spot for a w x h box. Blocking boxes are those registered (and unexpired) plus `live`.
   * Order: Lissajous candidates (t + step·i²), then a grid over the free area sorted by distance to the path
   * position, at full size; if nothing is free the size is shrunk step-wise (down to scales[last]) and the
   * search repeated. Last resort: the least overlapping candidate at the smallest scale.
   */
  place(t: number, w: number, h: number, live: Box[] = []): Placement {
    const { margin, candidates, step, gap, grid, scales } = this.o;
    this.boxes = this.boxes.filter((b) => b.until > t);
    const blockers: Box[] = [...this.boxes.map((b) => b.box), ...live];
    const total = (box: Box, g: number): number => {
      let s = 0;
      for (const b of blockers) s += overlapArea(box, b, g);
      return s;
    };
    const p0 = this.positionAt(t);
    let fallback: Placement | null = null;
    let fallbackOverlap = Infinity;
    for (let si = 0; si < scales.length; si++) {
      const sc = scales[si];
      const bw = w * sc, bh = h * sc;
      const g = gap * bh;
      const make = (cx: number, cy: number): Placement => ({ x: this.clampAxis(cx, bw, margin), y: this.clampAxis(cy, bh, margin), w: bw, h: bh, scale: sc });
      const cands: Placement[] = [];
      for (let i = 0; i < candidates; i++) {
        const p = this.positionAt(t + step * i * i);
        cands.push(make(p.x, p.y));
      }
      const lo = { x: margin + bw / 2, y: margin + bh / 2 };
      const hi = { x: 1 - margin - bw / 2, y: 1 - margin - bh / 2 };
      const grid2: Placement[] = [];
      if (lo.x <= hi.x && lo.y <= hi.y) {
        for (let x = lo.x; x <= hi.x + 1e-9; x += grid) {
          for (let y = lo.y; y <= hi.y + 1e-9; y += grid) grid2.push({ x, y, w: bw, h: bh, scale: sc });
        }
        grid2.sort((a, b) => (a.x - p0.x) ** 2 + (a.y - p0.y) ** 2 - ((b.x - p0.x) ** 2 + (b.y - p0.y) ** 2));
      }
      for (const c of cands.concat(grid2)) {
        const ov = total(c, g);
        if (ov === 0) return c;
        if (si === scales.length - 1 && ov < fallbackOverlap) { fallbackOverlap = ov; fallback = c; }
      }
    }
    return fallback!;
  }

  register(box: Box, untilSec: number): void {
    this.boxes.push({ box: { x: box.x, y: box.y, w: box.w, h: box.h }, until: untilSec });
  }

  private clampAxis(c: number, size: number, margin: number): number {
    const lo = margin + size / 2;
    const hi = 1 - margin - size / 2;
    if (lo > hi) return 0.5;
    return Math.min(hi, Math.max(lo, c));
  }
}
