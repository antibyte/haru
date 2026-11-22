import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { GLContext } from '../gl/context';
import compositeFrag from '../post/shaders/composite.frag?raw';
import {
  FaceLayer, HEAD_GAIN, HEAD_GRADE, HEAD_KEYS, HEAD_MORPH_GAIN, NEUTRAL_GRADE, TEXTURED_HEADS, parseHeadOverride, selectHead,
  type HeadGrade,
} from './face';

// FaceLayer without WebGL: the G-buffer pass and the GLB parser are stubbed, fetch is faked per test.
vi.mock('./headpass', () => ({
  HeadPass: class {
    head: unknown = null;
    setHead(g: unknown): void { this.head = g; }
    setAlbedo(): void { /* no GL */ }
    get hasHead(): boolean { return this.head !== null; }
    weights = new Float32Array(8);
    spots = Array.from({ length: 9 }, () => ({ x: 0.5, y: 0.5 }));
    texture = {};
    textured = false;
    albedoTexture = null;
    width = 1;
    height = 1;
    render(): void { /* no GL */ }
    blank(): void { /* no GL */ }
    dispose(): void { /* no GL */ }
  },
}));
vi.mock('./glb', () => ({ parseGlb: () => ({ image: null }) }));

type RGB = [number, number, number];

describe('selectHead', () => {
  it('angel picks the textured angel head by voice gender (unset = female)', () => {
    expect(selectHead('angel', 'female')).toBe('angel-female');
    expect(selectHead('angel', null)).toBe('angel-female');
    expect(selectHead('angel', 'male')).toBe('angel-male');
  });
  it('daemon always shows the daemon head', () => {
    expect(selectHead('daemon', 'female')).toBe('daemon');
    expect(selectHead('daemon', 'male')).toBe('daemon');
    expect(selectHead('daemon', null)).toBe('daemon');
  });
  it('the neutral start screen (no persona) behaves like the angel', () => {
    expect(selectHead(null, null)).toBe('angel-female');
    expect(selectHead(null, 'male')).toBe('angel-male');
  });
  it('never selects the porcelain heads at runtime', () => {
    for (const p of ['angel', 'daemon', null] as const) {
      for (const g of ['female', 'male', null] as const) expect(['female', 'male']).not.toContain(selectHead(p, g));
    }
  });
});

describe('parseHeadOverride (?debug&head=)', () => {
  it('pins the angel heads and the daemon', () => {
    expect([...HEAD_KEYS]).toEqual(['angel-female', 'angel-male', 'daemon']);
    for (const k of HEAD_KEYS) expect(parseHeadOverride(k)).toEqual({ key: k });
  });
  it('rejects the retired porcelain keys', () => {
    for (const k of ['female', 'male', 'female-textured']) expect(parseHeadOverride(k)).toBeNull();
  });
  it('ignores missing and unknown values', () => {
    expect(parseHeadOverride(null)).toBeNull();
    expect(parseHeadOverride('')).toBeNull();
    expect(parseHeadOverride('angel')).toBeNull();
    expect(parseHeadOverride('Daemon')).toBeNull();
  });
});

describe('head tables', () => {
  it('the angel heads and the daemon are the only textured heads', () => {
    expect([...TEXTURED_HEADS].sort()).toEqual(['angel-female', 'angel-male', 'daemon']);
  });
  it('every head has a presence gain; the angels are 1.0, the daemon unchanged', () => {
    for (const k of HEAD_KEYS) expect(HEAD_GAIN[k]).toBeGreaterThan(0);
    expect(HEAD_GAIN['angel-female']).toBe(1.0);
    expect(HEAD_GAIN['angel-male']).toBe(1.0);
    expect(HEAD_GAIN).toMatchObject({ daemon: 1.3 });
  });
});

describe('HEAD_MORPH_GAIN', () => {
  it('has 8 entries for every head', () => {
    for (const k of HEAD_KEYS) expect(HEAD_MORPH_GAIN[k]).toHaveLength(8);
  });
  it('the daemon is undamped', () => {
    expect([...HEAD_MORPH_GAIN.daemon]).toEqual([1, 1, 1, 1, 1, 1, 1, 1]);
  });
  it('the angels damp jaw / wide / round', () => {
    for (const k of ['angel-female', 'angel-male'] as const) {
      expect([...HEAD_MORPH_GAIN[k]]).toEqual([0.5, 0.7, 0.7, 1, 1, 1, 1, 1]);
    }
  });
});

describe('HEAD_GRADE', () => {
  // mirrors the textured skin path in composite.frag
  const mix = (a: number, b: number, t: number): number => a * (1 - t) + b * t;
  const grade = (alb: RGB, g: HeadGrade): RGB => {
    const s = alb.map((c) => Math.pow(Math.max(c, 0), g.gamma) * g.lift) as RGB;
    const l = s[0] * 0.299 + s[1] * 0.587 + s[2] * 0.114;
    return s.map((c, i) => mix(l * g.tint[i]!, c, g.saturation)) as RGB;
  };
  // the hard-coded demon grade the table replaced
  const legacyDaemon = (alb: RGB): RGB => {
    const s = alb.map((c) => Math.pow(Math.max(c, 0), 0.9) * 1.3) as RGB;
    const l = s[0] * 0.299 + s[1] * 0.587 + s[2] * 0.114;
    const tint: RGB = [1.1, 0.78, 0.7];
    return s.map((c, i) => mix(l * tint[i]!, c, 0.4)) as RGB;
  };
  const samples: RGB[] = [[0, 0, 0], [0.05, 0.02, 0.015], [0.21, 0.12, 0.09], [0.5, 0.42, 0.38], [0.9, 0.8, 0.7], [1, 1, 1]];

  it('daemon keeps exactly the previous values', () => {
    expect(HEAD_GRADE.daemon).toEqual({ gamma: 0.9, lift: 1.3, tint: [1.1, 0.78, 0.7], saturation: 0.4 });
  });
  it('daemon reproduces the previous hard-coded skin grade', () => {
    for (const a of samples) {
      const got = grade(a, HEAD_GRADE.daemon);
      legacyDaemon(a).forEach((v, i) => expect(got[i]).toBeCloseTo(v, 12));
    }
  });
  it('angels use the measured skin grade', () => {
    const g = { gamma: 0.9, lift: 1.45, tint: [1, 1, 1], saturation: 0.95 };
    expect(HEAD_GRADE['angel-female']).toEqual(g);
    expect(HEAD_GRADE['angel-male']).toEqual(g);
    expect(NEUTRAL_GRADE).toEqual({ gamma: 1, lift: 1, tint: [1, 1, 1], saturation: 1 });
  });
  it('every head has a grade', () => {
    for (const k of HEAD_KEYS) expect(HEAD_GRADE[k]).toBeDefined();
  });
  it('composite.frag takes the grade from uniforms instead of the hard-coded demon grade', () => {
    for (const u of ['uniform float uSkinGamma;', 'uniform float uSkinLift;', 'uniform vec3 uSkinTint;', 'uniform float uSkinSat;']) {
      expect(compositeFrag).toContain(u);
    }
    expect(compositeFrag).toContain('vec3(uSkinGamma)) * uSkinLift;');
    expect(compositeFrag).toContain('* uSkinTint, skin, uSkinSat);');
    expect(compositeFrag).not.toContain('vec3(1.1, 0.78, 0.7)');
  });
});

describe('FaceLayer head loading', () => {
  const glc = { gl: {} } as unknown as GLContext;
  let missing: Set<string>;
  let fetchMock: ReturnType<typeof vi.fn>;
  const requested = (): string[] => fetchMock.mock.calls.map((c) => String(c[0]).replace(/^.*\/heads\//, ''));

  beforeEach(() => {
    missing = new Set();
    fetchMock = vi.fn(async (url: string) => {
      const name = String(url).replace(/^.*\/heads\//, '');
      return missing.has(name)
        ? { ok: false, status: 404, arrayBuffer: async () => new ArrayBuffer(0) }
        : { ok: true, status: 200, arrayBuffer: async () => new ArrayBuffer(8) };
    });
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => { vi.unstubAllGlobals(); });

  const talking = (layer: FaceLayer, persona: 'angel' | 'daemon' | null, gender: 'female' | 'male' | null): FaceLayer => {
    layer.setGender(gender);
    layer.setPersona(persona);
    layer.setMode('talking');
    return layer;
  };

  it('loads nothing before the conversation starts', async () => {
    const layer = new FaceLayer(glc, vi.fn());
    layer.setPersona('angel');
    await Promise.resolve();
    expect(fetchMock).not.toHaveBeenCalled();
    expect(layer.head).toBeNull();
  });

  it.each([
    ['angel', 'female', 'angel-female'],
    ['angel', null, 'angel-female'],
    ['angel', 'male', 'angel-male'],
    ['daemon', 'female', 'daemon'],
    ['daemon', 'male', 'daemon'],
  ] as const)('persona %s, voice %s -> %s.glb', async (persona, gender, key) => {
    const layer = talking(new FaceLayer(glc, vi.fn()), persona, gender);
    await vi.waitFor(() => expect(layer.head).toBe(key));
    expect(requested()).toEqual([`${key}.glb`]);
  });

  it('switches the head when the persona or the voice changes', async () => {
    const layer = talking(new FaceLayer(glc, vi.fn()), 'angel', 'female');
    await vi.waitFor(() => expect(layer.head).toBe('angel-female'));
    layer.setPersona('daemon');
    await vi.waitFor(() => expect(layer.head).toBe('daemon'));
    layer.setPersona('angel');
    layer.setGender('male');
    await vi.waitFor(() => expect(layer.head).toBe('angel-male'));
  });

  it('renders without a face when the angel GLB is missing (each URL is tried once)', async () => {
    missing = new Set(['angel-male.glb']);
    const log = vi.fn();
    const layer = talking(new FaceLayer(glc, log), 'angel', 'male');
    await vi.waitFor(() => expect(log).toHaveBeenCalledWith(expect.stringContaining('rendering without a face')));
    expect(layer.head).toBeNull();
    expect(requested()).toEqual(['angel-male.glb']);
  });

  it.each([
    ['angel', 'female', [0.5, 0.7, 0.7, 1, 1, 1, 1, 1]],
    ['angel', 'male', [0.5, 0.7, 0.7, 1, 1, 1, 1, 1]],
    ['daemon', 'male', [1, 1, 1, 1, 1, 1, 1, 1]],
  ] as const)('applies the morph gain of the loaded head (%s, %s)', async (persona, gender, expected) => {
    const layer = talking(new FaceLayer(glc, vi.fn()), persona, gender);
    await vi.waitFor(() => expect(layer.head).not.toBeNull());
    const priv = layer as unknown as {
      lip: { update: () => unknown }; pass: { weights: Float32Array };
    };
    priv.lip.update = () => ({ weights: new Float32Array(8).fill(1), presence: 1, speech: 1, nod: 0, tilt: 0 });
    layer.update({
      frameMs: 16, timeSec: 1, audio: { modelRms: 0.5, micRms: 0 }, mode: 'talking', tier: 0,
      dyeW: 100, dyeH: 100, aspect: 1, lightPos: { x: 0.5, y: 0.5 },
    } as unknown as Parameters<FaceLayer['update']>[0]);
    expect([...priv.pass.weights].map((v) => Math.round(v * 1000) / 1000)).toEqual([...expected]);
  });

  it('a debug override pins the head regardless of persona and voice', async () => {
    const layer = talking(new FaceLayer(glc, vi.fn(), TEXTURED_HEADS, 'angel-female'), 'daemon', 'male');
    await vi.waitFor(() => expect(layer.head).toBe('angel-female'));
    expect(requested()).toEqual(['angel-female.glb']);
  });
});
