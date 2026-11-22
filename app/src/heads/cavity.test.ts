import { describe, expect, it } from 'vitest';
import { CAVITY_MIN_TRIANGLES, findCavityUv } from './cavity';
import type { GlbHead, LandmarkName, Vec3 } from './glb';
import headFrag from './shaders/head.frag?raw';
import headPassSrc from './headpass.ts?raw';
import compositeFrag from '../post/shaders/composite.frag?raw';

type Head = Pick<GlbHead, 'positions' | 'uvs' | 'indices' | 'landmarks'>;

const MOUTH: Vec3 = [0, -0.28, 0.45];
const LANDMARKS: Record<LandmarkName, Vec3> = {
  mouth: MOUTH, mouthL: [0.09, -0.29, 0.43], mouthR: [-0.09, -0.29, 0.43], eyeL: [0.12, 0.05, 0.38],
  eyeR: [-0.12, 0.05, 0.38], chin: [0, -0.5, 0.4], crown: [0, 0.5, 0], noseTip: [0, -0.1, 0.55], jawPivot: [0, -0.15, 0.05],
};
const POUCH_UV: [number, number] = [0.03125, 0.9765625];

/** Builds a head from triangles given as (centre, uv per corner); each triangle gets its own three vertices. */
function mesh(tris: Array<{ at: Vec3; uv: [[number, number], [number, number], [number, number]] }>, withUv = true): Head {
  const positions = new Float32Array(tris.length * 9);
  const uvs = new Float32Array(tris.length * 6);
  tris.forEach((t, i) => {
    const corners: Vec3[] = [[t.at[0] - 0.004, t.at[1] - 0.003, t.at[2]], [t.at[0] + 0.004, t.at[1] - 0.003, t.at[2]], [t.at[0], t.at[1] + 0.005, t.at[2]]];
    corners.forEach((c, k) => { positions.set(c, i * 9 + k * 3); uvs.set(t.uv[k]!, i * 6 + k * 2); });
  });
  const indices = new Uint32Array(tris.length * 3).map((_, i) => i);
  return { positions, uvs: withUv ? uvs : null, indices, landmarks: LANDMARKS };
}
const point = (uv: [number, number]): [[number, number], [number, number], [number, number]] => [uv, uv, uv];
/** n point-mapped triangles spread along the lip slit, a little behind it (like build_pouch) */
const pouch = (n: number, uv = POUCH_UV) =>
  Array.from({ length: n }, (_, i) => ({ at: [-0.08 + (0.16 * i) / Math.max(1, n - 1), MOUTH[1] - 0.01 * (i % 3), MOUTH[2] - 0.03 - 0.01 * (i % 4)] as Vec3, uv: point(uv) }));
/** ordinary skin triangles around the mouth with distinct UVs */
const skin = (n: number) =>
  Array.from({ length: n }, (_, i) => {
    const u = 0.4 + 0.01 * i;
    return { at: [-0.1 + 0.2 * (i / n), MOUTH[1] + 0.02, MOUTH[2]] as Vec3, uv: [[u, 0.5], [u + 0.004, 0.5], [u, 0.505]] as [[number, number], [number, number], [number, number]] };
  });

describe('findCavityUv', () => {
  it('returns the single UV shared by the inner-mouth pouch triangles', () => {
    expect(findCavityUv(mesh([...skin(30), ...pouch(24)]))).toEqual(POUCH_UV);
  });

  it('ignores point-mapped triangles away from the mouth, even when they are more (filled holes at the back / crown)', () => {
    const back = Array.from({ length: 60 }, (_, i) => ({ at: [0.2, -0.27 + 0.001 * i, -0.25] as Vec3, uv: point([0, 0.9199219]) }));
    const crown = Array.from({ length: 40 }, () => ({ at: [0.19, 0.43, 0.04] as Vec3, uv: point([0, 0.9199219]) }));
    const beside = Array.from({ length: 40 }, () => ({ at: [0.3, MOUTH[1], MOUTH[2]] as Vec3, uv: point([0.5, 0.5]) }));
    expect(findCavityUv(mesh([...back, ...crown, ...beside, ...skin(10), ...pouch(16)]))).toEqual(POUCH_UV);
  });

  it('picks the pouch over a few stray point-mapped triangles near the mouth', () => {
    expect(findCavityUv(mesh([...pouch(5, [0, 0.92]), ...pouch(20)]))).toEqual(POUCH_UV);
  });

  it('only counts triangles whose three UVs coincide exactly', () => {
    const almost = Array.from({ length: 30 }, (_, i) => ({
      at: [0.001 * i, MOUTH[1], MOUTH[2] - 0.03] as Vec3,
      uv: [[0.2, 0.2], [0.2, 0.2], [0.2, 0.2 + 1e-6]] as [[number, number], [number, number], [number, number]],
    }));
    expect(findCavityUv(mesh([...almost, ...pouch(CAVITY_MIN_TRIANGLES)]))).toEqual(POUCH_UV);
  });

  it('returns null without a pouch, with too few point-mapped triangles, or without UVs', () => {
    expect(findCavityUv(mesh(skin(40)))).toBeNull();
    expect(findCavityUv(mesh([...skin(10), ...pouch(CAVITY_MIN_TRIANGLES - 1)]))).toBeNull();
    expect(findCavityUv(mesh([...skin(10), ...pouch(24)], false))).toBeNull();
  });
});

describe('mouth cavity shaders', () => {
  it('the head pass masks the exact pouch of textured heads via uCavityUv', () => {
    expect(headFrag).toContain('uniform vec2 uCavityUv;');
    expect(headFrag).toMatch(/abs\(vUv - uCavityUv\)/);
    expect(headPassSrc).toContain("uniforms['uCavityUv']");
    expect(headPassSrc).toContain('findCavityUv(glb)');
  });
  it('porcelain heads keep the jaw-scaled blob and the original darkening exactly', () => {
    expect(headFrag).toContain('mouth = uJaw * exp(-dot(d, d)) * front;');
    expect(headFrag).toContain('vec2 d = vec2((vObj.x - uMouth.x) / 0.07, (vObj.y - (uMouth.y - 0.03 * uJaw)) / (0.006 + 0.03 * uJaw));');
    expect(compositeFrag).toContain('col *= 1.0 - 0.55 * hg.a * cov * smoothstep(0.0, 0.04, vis);');
  });
  it('the textured cavity does not depend on the jaw weight', () => {
    const textured = headFrag.slice(headFrag.indexOf('if (uHasTex > 0.5) {'), headFrag.indexOf('} else {'));
    expect(textured).toContain('uCavityUv');
    expect(textured).not.toContain('uJaw');
  });
});
