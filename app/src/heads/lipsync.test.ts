import { describe, expect, it } from 'vitest';
import { analysePiece, LipsyncController, MORPH, PRESENCE_FLOOR, type LipMode } from './lipsync';

function seeded(seed = 7): () => number {
  let s = seed;
  return () => { s = (s * 1664525 + 1013904223) % 4294967296; return s / 4294967296; };
}

interface Run { ctl: LipsyncController; w: (i: number) => number; step: (ms: number, o?: { rms?: number; mic?: number; mode?: LipMode; pieces?: string[] }) => ReturnType<LipsyncController['update']> }

function make(): Run {
  const ctl = new LipsyncController(seeded());
  let last = ctl.update({ dtMs: 16, mode: 'idle', modelRms: 0, micRms: 0, pieces: [] });
  const step: Run['step'] = (ms, o = {}) => {
    const n = Math.max(1, Math.round(ms / 16));
    for (let i = 0; i < n; i++) {
      last = ctl.update({ dtMs: 16, mode: o.mode ?? 'talking', modelRms: o.rms ?? 0, micRms: o.mic ?? 0, pieces: i === 0 ? (o.pieces ?? []) : [] });
    }
    return last;
  };
  return { ctl, w: (i) => last.weights[i], step };
}

describe('analysePiece', () => {
  it('maps letters to visemes', () => {
    expect(analysePiece(' Hal').vowel).toBe('a');
    expect(analysePiece('lo').vowel).toBe('o');
    expect(analysePiece(' bin').vowel).toBe('e');
    expect(analysePiece(' Schön').vowel).toBe('o');
    expect(analysePiece(' möch').vowel).toBe('o');
    expect(analysePiece(' Hal').press).toBe(false);
    expect(analysePiece(' bin').press).toBe(true);
    expect(analysePiece('ma').press).toBe(true);
    expect(analysePiece('?').question).toBe(true);
    expect(analysePiece('!').exclaim).toBe(true);
    expect(analysePiece('?').voiced).toBe(false);
    expect(analysePiece('')).toMatchObject({ vowel: null, press: false, voiced: false });
  });
  it('recognises CJK and leaves vowels out', () => {
    const p = analysePiece('こんに');
    expect(p.cjk).toBe(true);
    expect(p.vowel).toBeNull();
  });
});

describe('LipsyncController mouth', () => {
  it('opens the jaw with the RMS envelope and closes in silence', () => {
    const r = make();
    r.step(300, { rms: 0.2 });
    const open = r.w(MORPH.jawOpen);
    expect(open).toBeGreaterThan(0.45);
    r.step(60, { rms: 0.05 });
    expect(r.w(MORPH.jawOpen)).toBeLessThan(open);
    r.step(600, { rms: 0 });
    expect(r.w(MORPH.jawOpen)).toBeLessThan(0.02);
  });

  it('keeps all weights within 0..1', () => {
    const r = make();
    for (let i = 0; i < 400; i++) {
      const o = r.step(16, { rms: Math.random() * 1.5, mic: Math.random(), pieces: i % 7 === 0 ? [' Wor?', '!', ' ba'] : [] });
      for (const v of o.weights) { expect(v).toBeGreaterThanOrEqual(0); expect(v).toBeLessThanOrEqual(1); }
    }
  });

  it('rounds for o/u, widens for e/i and opens wider for a', () => {
    const a = make(); a.step(300, { rms: 0.15, pieces: [' Hal'] });
    const e = make(); e.step(300, { rms: 0.15, pieces: [' die'] });
    const o = make(); o.step(300, { rms: 0.15, pieces: [' so'] });
    expect(o.w(MORPH.lipsRound)).toBeGreaterThan(0.4);
    expect(o.w(MORPH.lipsWide)).toBeLessThan(0.05);
    expect(e.w(MORPH.lipsWide)).toBeGreaterThan(0.4);
    expect(e.w(MORPH.lipsRound)).toBeLessThan(0.05);
    expect(a.w(MORPH.jawOpen)).toBeGreaterThan(e.w(MORPH.jawOpen));
  });

  it('presses the lips briefly on m/b/p', () => {
    const r = make();
    r.step(200, { rms: 0.15 });
    r.step(60, { rms: 0.15, pieces: [' bin'] });
    expect(r.w(MORPH.lipsPress)).toBeGreaterThan(0.4);
    r.step(400, { rms: 0.15 });
    expect(r.w(MORPH.lipsPress)).toBeLessThan(0.05);
  });

  it('uses a gentle rounding modulation for CJK text', () => {
    const r = make();
    let max = 0;
    for (let i = 0; i < 40; i++) max = Math.max(max, r.step(16, { rms: 0.15, pieces: i === 0 ? ['こんに'] : [] }).weights[MORPH.lipsRound]);
    expect(max).toBeGreaterThan(0.05);
    expect(max).toBeLessThan(0.5);
    expect(r.w(MORPH.jawOpen)).toBeGreaterThan(0.4);
  });
});

describe('LipsyncController expression', () => {
  it('raises the brows on a question mark', () => {
    const r = make();
    r.step(300, { rms: 0.15 });
    r.step(500, { rms: 0.15, pieces: ['?'] });
    expect(r.w(MORPH.browRaise)).toBeGreaterThan(0.4);
    r.step(3000, { rms: 0 });
    expect(r.w(MORPH.browRaise)).toBeLessThan(0.1);
  });

  it('raises the brows when a phrase starts after a pause', () => {
    const r = make();
    r.step(1500, { rms: 0 });
    r.step(100, { rms: 0.15, pieces: [' Hal'] });
    expect(r.w(MORPH.browRaise)).toBeGreaterThan(0.15);
  });

  it('smiles on an exclamation mark', () => {
    const r = make();
    r.step(300, { rms: 0.15 });
    const before = r.w(MORPH.smile);
    r.step(600, { rms: 0.15, pieces: ['!'] });
    expect(r.w(MORPH.smile)).toBeGreaterThan(before + 0.3);
  });

  it('blinks every 2.5 to 6 s, sometimes twice', () => {
    const r = make();
    let blinks = 0;
    let prev = 0;
    let firstAt = -1;
    for (let i = 0; i < Math.round(60000 / 16); i++) {
      const v = r.step(16, { mode: 'idle' }).weights[MORPH.blink];
      if (v > 0.9 && prev <= 0.9) { blinks++; if (firstAt < 0) firstAt = i * 16; }
      prev = v;
    }
    expect(firstAt).toBeGreaterThanOrEqual(2300);
    expect(firstAt).toBeLessThanOrEqual(6200);
    expect(blinks).toBeGreaterThanOrEqual(10);
    expect(blinks).toBeLessThanOrEqual(30);
  });

  it('listens while the user speaks: brows up and head tilted', () => {
    const r = make();
    r.step(300, { rms: 0.15 });
    r.step(3000, { rms: 0, mic: 0.15 });
    expect(r.w(MORPH.browRaise)).toBeGreaterThan(0.15);
    const o = r.step(1500, { rms: 0, mic: 0.15 });
    expect(o.tilt).toBeGreaterThan(0.03);
    const after = r.step(4000, { rms: 0, mic: 0 });
    expect(after.tilt).toBeLessThan(0.01);
  });
});

describe('LipsyncController presence', () => {
  it('is 0 while idle or booting', () => {
    const r = make();
    expect(r.step(2000, { mode: 'idle', rms: 0.2 }).presence).toBe(0);
    expect(r.step(2000, { mode: 'boot' }).presence).toBe(0);
  });

  it('rises within about a second of speech, then falls slowly to the floor', () => {
    const r = make();
    const rise = r.step(1200, { rms: 0.15 });
    expect(rise.presence).toBeGreaterThan(0.9);
    const early = r.step(1000, { rms: 0 }); // 0.7 s hold + 0.3 s release
    expect(early.presence).toBeGreaterThan(0.8);
    const late = r.step(15000, { rms: 0 });
    expect(late.presence).toBeCloseTo(PRESENCE_FLOOR, 1);
    expect(late.presence).toBeGreaterThanOrEqual(PRESENCE_FLOOR - 1e-3);
  });

  it('fades to 0 after the conversation ends', () => {
    const r = make();
    r.step(1500, { rms: 0.15 });
    expect(r.step(15000, { mode: 'idle' }).presence).toBeLessThan(0.01);
  });
});
