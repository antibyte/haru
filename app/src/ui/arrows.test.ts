import { describe, expect, it } from 'vitest';
import { ARROW_MIN_LEN, ARROW_REACH, ARROW_TARGET_Y, ARROW_TIP_CLEAR, ARROW_WIDTH, arrowEnvelope, arrowGeometry, arrowPolygon, arrowsFor, type Pt, type RectLike } from './arrows';

const r = (left: number, top: number, width: number, height: number): RectLike => ({ left, top, width, height });
const CANVAS = r(0, 0, 1600, 900);
const WIDE = { vs: r(745, 180, 200, 200), angel: r(45, 308, 800, 592), daemon: r(845, 308, 709, 592) };
const STACKED = { vs: r(250, 436, 100, 100), angel: r(0, 73, 600, 413), daemon: r(0, 486, 600, 413) };
const near = (a: Pt, b: readonly number[]) => { expect(a[0]).toBeCloseTo(b[0], 6); expect(a[1]).toBeCloseTo(b[1], 6); };

describe('arrows', () => {
  it('start at the VS rim and point exactly at the heads', () => {
    const p = arrowGeometry(WIDE, CANVAS);
    near(p.angel.target, [445, 308 + ARROW_TARGET_Y * 592]);
    near(p.daemon.target, [1199.5, 308 + ARROW_TARGET_Y * 592]);
    for (const a of [p.angel, p.daemon]) {
      expect(Math.hypot(a.from[0] - 845, a.from[1] - 280)).toBeCloseTo(100, 6);
      const d = [a.to[0] - a.from[0], a.to[1] - a.from[1]];
      const t = [a.target[0] - 845, a.target[1] - 280];
      expect(d[0] * t[1] - d[1] * t[0]).toBeCloseTo(0, 4);
      expect(d[0] * t[0] + d[1] * t[1]).toBeGreaterThan(0);
    }
    expect(p.angel.to[0]).toBeLessThan(845);
    expect(p.daemon.to[0]).toBeGreaterThan(845);
  });

  it('side by side the tip stays above the figure, in the open space, and the shaft keeps a length', () => {
    expect([ARROW_REACH, ARROW_TARGET_Y, ARROW_TIP_CLEAR, ARROW_MIN_LEN, ARROW_WIDTH]).toEqual([0.8, 0.2, 0.12, 0.7, 0.14]);
    const p = arrowGeometry(WIDE, CANVAS);
    for (const [a, img] of [[p.angel, WIDE.angel], [p.daemon, WIDE.daemon]] as const) {
      expect(a.to[1]).toBeCloseTo(img.top + ARROW_TIP_CLEAR * img.height, 6);
      expect(Math.hypot(a.to[0] - 845, a.to[1] - 280)).toBeGreaterThanOrEqual(100 * (1 + ARROW_MIN_LEN) - 1e-6);
      expect(a.width).toBeCloseTo(ARROW_WIDTH * 200, 6);
    }
  });

  it('a short way to the head: the reach applies when the clearance is not hit', () => {
    const vs = r(745, 100, 200, 200);
    const a = arrowGeometry({ vs, angel: r(45, 1000, 800, 800), daemon: r(845, 1000, 700, 800) }, r(0, 0, 1600, 2000)).angel;
    const len = Math.hypot(445 - 845, 1160 - 200);
    expect(Math.hypot(a.to[0] - 845, a.to[1] - 200)).toBeCloseTo(100 + ARROW_REACH * (len - 100), 6);
  });

  it('stacked: the angel arrow points up, the daemon arrow down', () => {
    const p = arrowGeometry(STACKED, CANVAS);
    expect(p.angel.to[1]).toBeLessThan(p.angel.from[1]);
    expect(p.daemon.to[1]).toBeGreaterThan(p.daemon.from[1]);
    expect(p.angel.to[0]).toBeCloseTo(300, 6);
    expect(p.daemon.to[0]).toBeCloseTo(300, 6);
    near(p.angel.target, [300, 73 + ARROW_TARGET_Y * 413]);
  });

  it('a VS centred exactly on an image gives finite numbers', () => {
    const p = arrowGeometry({ vs: r(345, 316.4, 100, 100), angel: r(45, 308, 700, 292), daemon: r(845, 308, 709, 592) }, CANVAS);
    for (const v of [...p.angel.from, ...p.angel.to, ...p.angel.target, p.angel.width]) expect(Number.isFinite(v)).toBe(true);
    expect(arrowPolygon(p.angel.from, p.angel.to, p.angel.width).every((q) => Number.isFinite(q[0]) && Number.isFinite(q[1]))).toBe(true);
  });

  it('arrows are given whatever the motion preference is', () => {
    expect(arrowsFor(WIDE, CANVAS)).not.toBeNull();
  });

  it('no arrows without rects, with an empty canvas or an empty VS', () => {
    expect(arrowsFor(null, CANVAS)).toBeNull();
    expect(arrowsFor(WIDE, r(0, 0, 0, 0))).toBeNull();
    expect(arrowsFor({ ...WIDE, vs: r(0, 0, 0, 0) }, CANVAS)).toBeNull();
  });

  it('arrowPolygon: tip at `to`, symmetric about the axis, head wider than the shaft', () => {
    const p = arrowPolygon([0, 0], [100, 0], 10);
    expect(p).toHaveLength(7);
    near(p[3], [100, 0]);
    near(p[0], [0, 3]); near(p[6], [0, -3]);
    near(p[1], [76, 5]); near(p[5], [76, -5]);
    near(p[2], [76, 13]); near(p[4], [76, -13]);
  });

  it('arrowEnvelope builds up in 0.4 s and dissolves after release', () => {
    expect(arrowEnvelope(0, true, 200)).toBeCloseTo(0.5, 6);
    expect(arrowEnvelope(0.5, true, 200)).toBe(1);
    expect(arrowEnvelope(1, true, 500)).toBe(1);
    expect(arrowEnvelope(1, false, 550)).toBeCloseTo(0.5, 6);
    expect(arrowEnvelope(0.5, false, 2000)).toBe(0);
  });
});
