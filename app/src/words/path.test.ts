import { describe, expect, it } from 'vitest';
import { WordPath, type Box } from './path';

const intersects = (a: Box, b: Box) =>
  Math.abs(a.x - b.x) < (a.w + b.w) / 2 && Math.abs(a.y - b.y) < (a.h + b.h) / 2;

describe('WordPath', () => {
  it('deterministic_position', () => {
    const p = new WordPath().positionAt(0);
    expect(p.x).toBeCloseTo(0.82, 10);
    expect(p.y).toBeCloseTo(0.5, 10);
  });
  it('stays_within_bounds', () => {
    const path = new WordPath();
    for (let t = 0; t <= 1000; t += 0.37) {
      const b = path.place(t, 0.6, 0.12);
      expect(b.x - b.w / 2).toBeGreaterThanOrEqual(0);
      expect(b.x + b.w / 2).toBeLessThanOrEqual(1);
      expect(b.y - b.h / 2).toBeGreaterThanOrEqual(0);
      expect(b.y + b.h / 2).toBeLessThanOrEqual(1);
    }
  });
  it('avoids_recent_boxes', () => {
    const path = new WordPath();
    const a = path.place(10, 0.3, 0.1);
    path.register(a, 20);
    const b = path.place(10, 0.3, 0.1);
    expect(intersects(a, b)).toBe(false);
  });
  it('falls_back_to_min_overlap', () => {
    const path = new WordPath();
    path.register({ x: 0.5, y: 0.5, w: 1, h: 1 }, 100);
    const b = path.place(1, 0.3, 0.1);
    expect(b.scale).toBe(0.6);
    expect(b.w).toBeCloseTo(0.18, 10);
  });
  it('expired_boxes_ignored', () => {
    const path = new WordPath();
    path.register({ x: 0.5, y: 0.5, w: 1, h: 1 }, 5);
    const b = path.place(10, 0.3, 0.1);
    const p = path.positionAt(10);
    expect(b.x).toBeCloseTo(p.x, 10);
    expect(b.y).toBeCloseTo(p.y, 10);
  });
  it('dense_burst_no_overlap', () => {
    const path = new WordPath();
    const widths = [0.26, 0.18, 0.32, 0.22, 0.28, 0.2, 0.3, 0.24];
    const placed: { box: Box; until: number }[] = [];
    for (let i = 0; i < 8; i++) {
      const t = 10 + i * 0.133;
      const live = placed.filter((p) => p.until > t).map((p) => p.box);
      const b = path.place(t, widths[i], 0.12 * 1.25, live);
      placed.push({ box: b, until: t + 3 });
    }
    for (let i = 0; i < placed.length; i++) {
      for (let j = i + 1; j < placed.length; j++) expect(intersects(placed[i].box, placed[j].box)).toBe(false);
    }
  });
});
