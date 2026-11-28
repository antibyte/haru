import { describe, expect, it } from 'vitest';
import { SHOT_DISSOLVE_MS, SHOT_START, SHOT_SWITCH_MS, ShotDirector, nextShot, shotView, type ShotLevels, type ShotState } from './shot';

const run = (s: ShotState, lv: ShotLevels, ms: number): ShotState => { for (let t = 0; t < ms; t += 10) s = nextShot(s, lv, 10); return s; };
const HARU = { angel: 0.15, daemon: 0.01 };
const MEPH = { angel: 0.01, daemon: 0.15 };

describe('shot hysteresis', () => {
  it('starts on Haru', () => { expect(SHOT_START.shown).toBe('angel'); });
  it('switches only after the other speaker was clearly louder for 0.7 s', () => {
    const s = run(SHOT_START, MEPH, 690);
    expect(s.shown).toBe('angel');
    expect(nextShot(s, MEPH, 10)).toEqual({ shown: 'daemon', louderMs: 0 });
  });
  it('a short interjection does not cut, and its time drains away', () => {
    let s = run(SHOT_START, MEPH, 400);
    s = run(s, HARU, 1000);
    expect(s).toEqual({ shown: 'angel', louderMs: 0 });
    expect(run(s, MEPH, 690).shown).toBe('angel');
  });
  it('keeps the last speaker while both are silent', () => {
    const s = run(run(SHOT_START, MEPH, 700), { angel: 0.005, daemon: 0.005 }, 10000);
    expect(s.shown).toBe('daemon');
  });
  it('a voice under the quiet floor never cuts in, even when the shown one is silent', () => {
    expect(run(SHOT_START, { angel: 0, daemon: 0.01 }, 3000).shown).toBe('angel');
  });
  it('equal levels never cut', () => {
    expect(run(SHOT_START, { angel: 0.15, daemon: 0.15 }, 3000).shown).toBe('angel');
  });
  it('a cut cannot happen during a dissolve', () => {
    expect(SHOT_SWITCH_MS).toBeGreaterThanOrEqual(SHOT_DISSOLVE_MS);
  });
  it('slightly louder is not clearly louder', () => {
    expect(run(SHOT_START, { angel: 0.12, daemon: 0.15 }, 3000)).toEqual({ shown: 'angel', louderMs: 0 });
  });
});

describe('shot dissolve', () => {
  it('fades the old head out, switches in the middle and fades the new one in over 0.6 s', () => {
    expect(shotView('angel', 'daemon', 0)).toEqual({ speaker: 'angel', gain: 1, ember: 0 });
    expect(shotView('angel', 'daemon', 150).gain).toBeCloseTo(0.5, 6);
    expect(shotView('angel', 'daemon', 300)).toMatchObject({ speaker: 'daemon', gain: 0 });
    expect(shotView('angel', 'daemon', 300).ember).toBeCloseTo(0.5, 6);
    expect(shotView('angel', 'daemon', 450)).toMatchObject({ speaker: 'daemon' });
    expect(shotView('angel', 'daemon', 600)).toEqual({ speaker: 'daemon', gain: 1, ember: 1 });
  });
  it('ShotDirector runs the hysteresis and the dissolve together', () => {
    const d = new ShotDirector();
    expect(d.update(HARU, 16)).toEqual({ speaker: 'angel', gain: 1, ember: 0 });
    let v = d.update(MEPH, 0);
    for (let t = 0; t < 700; t += 10) v = d.update(MEPH, 10);
    expect(v).toEqual({ speaker: 'angel', gain: 1, ember: 0 });
    for (let t = 0; t < 300; t += 10) v = d.update(MEPH, 10);
    expect(v.speaker).toBe('daemon');
    for (let t = 0; t < 300; t += 10) v = d.update(MEPH, 10);
    expect(v).toEqual({ speaker: 'daemon', gain: 1, ember: 1 });
  });
});
