import { describe, expect, it } from 'vitest';
import { holdMs, wordPhase } from './lifecycle';

describe('lifecycle', () => {
  it('ink_for_first_250ms', () => {
    const s = wordPhase(1100, 1000, null, 3);
    expect(s.phase).toBe('ink');
    expect(s.inkStrength).toBe(1);
    expect(wordPhase(1249, 1000, null, 3).phase).toBe('ink');
    expect(wordPhase(1250, 1000, null, 3).phase).toBe('hold');
  });
  it('hold_duration_scales_with_length', () => {
    expect(holdMs(0)).toBe(600);
    expect(holdMs(3)).toBe(780);
    expect(holdMs(20)).toBe(1200);
  });
  it('no_release_before_word_end', () => {
    const s = wordPhase(6000, 1000, null, 3);
    expect(s.phase).toBe('hold');
    expect(s.holdStrength).toBe(0.15);
    expect(s.rest).toBe(true);
  });
  it('release_after_hold_and_end', () => {
    // len 3: hold until 1000+250+780 = 2030; ended at 1500 -> end+300 = 1800, so 2030 rules.
    expect(wordPhase(2029, 1000, 1500, 3).phase).toBe('hold');
    const s = wordPhase(2230, 1000, 1500, 3);
    expect(s.phase).toBe('release');
    expect(s.releaseT).toBeCloseTo(0.5, 5);
    expect(s.rest).toBe(false);
    // ended late: end + 300 dominates
    expect(wordPhase(3299, 1000, 3000, 3).phase).toBe('hold');
    expect(wordPhase(3300, 1000, 3000, 3).phase).toBe('release');
  });
  it('done_after_release', () => {
    expect(wordPhase(2030 + 400, 1000, 1500, 3).phase).toBe('done');
  });
});
