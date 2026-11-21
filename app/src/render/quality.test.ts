import { describe, expect, it } from 'vitest';
import { QualityController } from './quality';

/** Feeds constant frameMs every stepMs from t0 to t1 inclusive; returns [time, level] of each change. */
function drive(q: QualityController, frameMs: number, t0: number, t1: number, stepMs = 25): [number, string][] {
  const changes: [number, string][] = [];
  for (let t = t0; t <= t1; t += stepMs) {
    const r = q.update(frameMs, t);
    if (r) changes.push([t, r]);
  }
  return changes;
}

describe('QualityController', () => {
  it('fixed_mode_never_changes', () => {
    const q = new QualityController('low');
    expect(q.level).toBe('low');
    expect(drive(q, 50, 0, 10000)).toEqual([]);
    expect(drive(q, 2, 10000, 30000)).toEqual([]);
    expect(q.level).toBe('low');
    q.setMode('ultra');
    expect(q.level).toBe('ultra');
  });
  it('downgrades_after_2s_over_20ms', () => {
    const q = new QualityController('auto');
    const c = drive(q, 25, 0, 3000);
    expect(c[0][1]).toBe('medium');
    expect(c[0][0]).toBeGreaterThanOrEqual(2000);
    expect(c[0][0]).toBeLessThan(2100);
  });
  it('no_downgrade_on_brief_spike', () => {
    const q = new QualityController('auto');
    expect(drive(q, 30, 0, 1000)).toEqual([]);
    expect(drive(q, 10, 1025, 4000)).toEqual([]);
    expect(q.level).toBe('high');
  });
  it('upgrades_after_5s_under_12ms', () => {
    const q = new QualityController('auto', 'medium');
    expect(drive(q, 8, 0, 4900)).toEqual([]);
    const c = drive(q, 8, 4925, 5200);
    expect(c[0][1]).toBe('high');
  });
  it('clamps_at_bounds', () => {
    const lo = new QualityController('auto', 'low');
    expect(drive(lo, 40, 0, 20000)).toEqual([]);
    expect(lo.level).toBe('low');
    const hi = new QualityController('auto', 'ultra');
    expect(drive(hi, 4, 0, 20000)).toEqual([]);
    expect(hi.level).toBe('ultra');
  });
  it('timers_reset_after_change', () => {
    const q = new QualityController('auto');
    const c = drive(q, 25, 0, 6000);
    expect(c.length).toBeGreaterThanOrEqual(2);
    expect(c[1][1]).toBe('low');
    expect(c[1][0]).toBeGreaterThanOrEqual(4000);
  });
});
