import { describe, expect, it } from 'vitest';
import { haruHue, hsvToRgb, micHue, vorticityFor } from './coupling';

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
