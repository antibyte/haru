import { describe, expect, it } from 'vitest';
import { ageInk } from './params';
import { hot } from '../words/injector';
import { emberHue, hsvToRgb } from '../audio/coupling';
import type { RGB } from './params';

function hueOf([r, g, b]: RGB): number {
  const mx = Math.max(r, g, b);
  const d = mx - Math.min(r, g, b);
  if (d < 1e-9) return 0;
  let h = mx === r ? ((g - b) / d) % 6 : mx === g ? (b - r) / d + 2 : (r - g) / d + 4;
  h *= 60;
  return (h + 360) % 360;
}
const inEmber = (h: number) => h >= 340 || h <= 45;

describe('ink ageing', () => {
  it('neutral (ember 0) is the original math', () => {
    const c: RGB = [1, 0.6, 0.2];
    const k = 0.02;
    const o = ageInk(c, k, 0);
    expect(o[0]).toBeCloseTo(1 * Math.exp(-0.75 * k), 12);
    expect(o[1]).toBeCloseTo(0.6 * Math.exp(-1.5 * k), 12);
    expect(o[2]).toBeCloseTo((0.2 + 0.6 * 0.55 * k) * Math.exp(-0.32 * k), 12);
  });
  it('ember ink ages through crimson to ash red and never turns violet', () => {
    for (const c of [0, 0.5, 1]) {
      let col = hot(hsvToRgb(emberHue(c), 0.85, 1), 1, true);
      for (let i = 0; i < 3000; i++) {
        col = ageInk(col, 0.016, 1);
        if (i % 100 === 0) expect(inEmber(hueOf(col))).toBe(true);
      }
      expect(inEmber(hueOf(col))).toBe(true);
    }
  });
  it('ember word core is orange-amber, neutral core is unchanged', () => {
    expect(inEmber(hueOf(hot([1, 0.2, 0.05], 1, true)))).toBe(true);
    expect(hot([1, 0.6, 0.2])).toEqual(hot([1, 0.6, 0.2], 1, false));
  });
});
