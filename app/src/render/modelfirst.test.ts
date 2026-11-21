import { describe, expect, it } from 'vitest';
import { BUDGET_TIERS, ModelFirstController, minQuality } from './modelfirst';

/** Feeds constant latency every 16 ms from t0 (inclusive) to t1 (exclusive); returns the tier at the end and the first time each tier change happened. */
function drive(c: ModelFirstController, latency: number, t0: number, t1: number): { tier: number; changes: [number, number][] } {
  const changes: [number, number][] = [];
  let prev = c.tier;
  for (let t = t0; t < t1; t += 16) {
    const b = c.update(latency, t);
    if (b.tier !== prev) { changes.push([t, b.tier]); prev = b.tier; }
  }
  return { tier: c.tier, changes };
}

describe('ModelFirstController', () => {
  it('starts_at_tier0_and_stays_with_healthy_latency', () => {
    const c = new ModelFirstController();
    expect(drive(c, 280, 0, 60_000).tier).toBe(0);
    expect(c.budget).toEqual(BUDGET_TIERS[0]);
    expect(c.budget.fpsCap).toBeNull();
    expect(c.budget.renderScale).toBe(1);
  });

  it('steps_down_one_tier_after_500ms_above_450', () => {
    const c = new ModelFirstController();
    const r = drive(c, 500, 0, 2000);
    expect(r.changes[0][1]).toBe(1);
    expect(r.changes[0][0]).toBeGreaterThanOrEqual(500);
    expect(r.changes[0][0]).toBeLessThan(530);
  });

  it('rising_latency_keeps_stepping_down_with_settle_time', () => {
    const c = new ModelFirstController();
    const r = drive(c, 600, 0, 6000);
    expect(r.changes.map((x) => x[1])).toEqual([1, 2, 3]);
    expect(r.changes[1][0] - r.changes[0][0]).toBeGreaterThanOrEqual(1500);
  });

  it('severe_latency_jumps_to_most_frugal_tier', () => {
    const c = new ModelFirstController();
    const r = drive(c, 800, 0, 1000);
    expect(r.tier).toBe(3);
    expect(r.changes).toHaveLength(1);
    expect(r.changes[0][0]).toBeLessThan(250);
    const b = c.budget;
    expect(b).toMatchObject({ maxQuality: 'low', renderScale: 0.5, fpsCap: 30, post: 'minimal' });
  });

  it('short_spikes_do_not_trigger', () => {
    const c = new ModelFirstController();
    drive(c, 300, 0, 1000);
    drive(c, 600, 1000, 1400); // 400 ms < 500 ms
    drive(c, 300, 1400, 2000);
    drive(c, 900, 2000, 2150); // 150 ms < 200 ms severe
    drive(c, 300, 2150, 3000);
    expect(c.tier).toBe(0);
  });

  it('broken_up_high_periods_do_not_accumulate', () => {
    const c = new ModelFirstController();
    for (let i = 0; i < 20; i++) {
      drive(c, 600, i * 1000, i * 1000 + 400);
      drive(c, 300, i * 1000 + 400, i * 1000 + 1000);
    }
    expect(c.tier).toBe(0);
  });

  it('recovery_needs_sustained_low_latency_and_goes_one_tier_at_a_time', () => {
    const c = new ModelFirstController();
    drive(c, 800, 0, 1000);
    expect(c.tier).toBe(3);
    // 5.9 s low is not enough
    drive(c, 300, 1000, 6900);
    expect(c.tier).toBe(3);
    // the band 350..450 holds the tier and restarts the low timer
    drive(c, 400, 6900, 20_000);
    expect(c.tier).toBe(3);
    const r = drive(c, 300, 20_000, 40_000);
    expect(r.changes.map((x) => x[1])).toEqual([2, 1, 0]);
    expect(r.changes[0][0] - 20_000).toBeGreaterThanOrEqual(6000);
    expect(r.changes[1][0] - r.changes[0][0]).toBeGreaterThanOrEqual(6000);
  });

  it('a_dip_in_the_low_period_restarts_the_recovery_timer', () => {
    const c = new ModelFirstController();
    drive(c, 800, 0, 1000);
    drive(c, 300, 1000, 5000);
    drive(c, 400, 5000, 5100); // leaves the low zone
    drive(c, 300, 5100, 10_000); // only 4.9 s since the dip
    expect(c.tier).toBe(3);
  });

  it('reset_returns_to_tier0', () => {
    const c = new ModelFirstController();
    drive(c, 800, 0, 1000);
    c.reset();
    expect(c.tier).toBe(0);
    drive(c, 300, 1000, 2000);
    expect(c.tier).toBe(0);
  });

  it('ignores_non_finite_samples', () => {
    const c = new ModelFirstController();
    expect(c.update(NaN, 0).tier).toBe(0);
    expect(c.update(Infinity, 100).tier).toBe(0);
  });

  it('budgets_only_get_cheaper_with_the_tier_and_never_exceed_user_quality', () => {
    const order = ['low', 'medium', 'high', 'ultra'];
    for (let i = 1; i < BUDGET_TIERS.length; i++) {
      const a = BUDGET_TIERS[i - 1], b = BUDGET_TIERS[i];
      expect(order.indexOf(b.maxQuality)).toBeLessThanOrEqual(order.indexOf(a.maxQuality));
      expect(b.renderScale).toBeLessThanOrEqual(a.renderScale);
      expect(b.fpsCap ?? Infinity).toBeLessThanOrEqual(a.fpsCap ?? Infinity);
    }
    // effective level = min(user choice, cap)
    for (const user of ['low', 'medium', 'high', 'ultra'] as const) {
      for (const t of BUDGET_TIERS) {
        const eff = minQuality(user, t.maxQuality);
        expect(order.indexOf(eff)).toBeLessThanOrEqual(order.indexOf(user));
      }
    }
    expect(minQuality('low', 'ultra')).toBe('low');
    expect(minQuality('ultra', 'medium')).toBe('medium');
  });
});
