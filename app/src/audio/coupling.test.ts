import { describe, expect, it } from 'vitest';
import type { FluidSim } from '../fluid/sim';
import type { SmoothedAudio } from './features';
import { AudioCoupling, haruHue, hsvToRgb, micHue, vorticityFor } from './coupling';

const fakeSim = (): FluidSim => ({} as unknown as FluidSim);
const silentAudio = (): SmoothedAudio => ({ modelRms: 0, modelCentroid: 0.5, micRms: 0, micCentroid: 0.5 });

describe('coupling math', () => {
  it('vorticity_base_and_max', () => {
    expect(vorticityFor(0)).toBe(3);
    expect(vorticityFor(0.125)).toBeCloseTo(5, 10);
    expect(vorticityFor(0.25)).toBeCloseTo(7, 10);
    expect(vorticityFor(1)).toBe(7);
  });
  it('haru_hue_range', () => {
    expect(haruHue(0)).toBe(42);
    expect(haruHue(0.5)).toBeCloseTo(1, 10);
    expect(haruHue(1)).toBe(320);
  });
  it('mic_hue_range', () => {
    expect(micHue(0)).toBe(185);
    expect(micHue(1)).toBe(225);
  });
  it('hsv_primaries', () => {
    expect(hsvToRgb(0, 1, 1)).toEqual([1, 0, 0]);
    expect(hsvToRgb(120, 1, 1)).toEqual([0, 1, 0]);
    expect(hsvToRgb(240, 1, 1)).toEqual([0, 0, 1]);
  });
});

describe('ink palettes', () => {
  it('ember palette keeps Haru ink in red-orange', () => {
    const c = new AudioCoupling(fakeSim());
    c.setPalette('ember');
    for (const cen of [0, 0.25, 0.5, 0.75, 1]) {
      const [r, g, b] = c.haruTint({ ...silentAudio(), modelCentroid: cen });
      expect(r).toBeGreaterThan(g);
      expect(g).toBeGreaterThanOrEqual(b); // red >= green >= blue: hue 0-45 degrees
      const [cr, cg, cb] = c.haruColor({ ...silentAudio(), modelCentroid: cen });
      expect(cr).toBeGreaterThan(cg);
      expect(cg).toBeGreaterThanOrEqual(cb);
    }
  });
  it('neutral palette is unchanged', () => {
    const c = new AudioCoupling(fakeSim());
    expect(c.haruTint({ ...silentAudio(), modelCentroid: 0.3 })).toEqual(hsvToRgb(haruHue(0.3 * 0.55), 0.85, 1));
    c.setPalette('ember');
    c.setPalette('neutral');
    expect(c.haruTint({ ...silentAudio(), modelCentroid: 0.3 })).toEqual(hsvToRgb(haruHue(0.3 * 0.55), 0.85, 1));
  });
});
