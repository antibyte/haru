import { describe, expect, it } from 'vitest';
import { Envelope, spectralCentroid } from './features';

describe('features', () => {
  it('centroid_flat_is_half', () => {
    expect(spectralCentroid(new Array(16).fill(0.3))).toBeCloseTo(0.5, 10);
  });
  it('centroid_low_band_is_zero', () => {
    expect(spectralCentroid([1, 0, 0, 0, 0, 0, 0, 0])).toBe(0);
  });
  it('centroid_silence_is_half', () => {
    expect(spectralCentroid(new Array(8).fill(0))).toBe(0.5);
  });
  it('envelope_attack_faster_than_release', () => {
    const up = new Envelope(30, 200);
    expect(up.step(1, 30)).toBeGreaterThan(0.6);
    const down = new Envelope(30, 200);
    down.value = 1;
    expect(down.step(0, 30)).toBeGreaterThan(0.8);
  });
});

describe('FeatureSmoother', () => {
  const frame = (rms: number, bands: number[]) => ({ model: { rms, bands }, mic: { rms: 0, bands: [0, 0] } });
  it('follows_loudness_and_decays_when_frames_stop', async () => {
    const { FeatureSmoother } = await import('./features');
    const s = new FeatureSmoother();
    let a = s.update(frame(0.2, [0, 0, 1, 0]), 16);
    for (let i = 0; i < 30; i++) a = s.update(frame(0.2, [0, 0, 1, 0]), 16);
    expect(a.modelRms).toBeGreaterThan(0.15);
    for (let i = 0; i < 100; i++) a = s.update(null, 16);
    expect(a.modelRms).toBeLessThan(0.01);
  });
  it('centroid_ema_ignores_silence', async () => {
    const { FeatureSmoother } = await import('./features');
    const s = new FeatureSmoother();
    for (let i = 0; i < 40; i++) s.update(frame(0.2, [0, 0, 0, 1]), 16);
    const loud = s.update(frame(0.2, [0, 0, 0, 1]), 16).modelCentroid;
    expect(loud).toBeGreaterThan(0.9);
    expect(s.update(frame(0, [1, 0, 0, 0]), 16).modelCentroid).toBeCloseTo(loud, 10);
  });
});
