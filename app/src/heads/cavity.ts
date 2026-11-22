import type { GlbHead } from './glb';

/** Fewer point-mapped triangles than this around the mouth are strays, not the inner-mouth pouch. */
export const CAVITY_MIN_TRIANGLES = 8;

/**
 * UV of the inner-mouth pouch of a built head (glTF UV convention), or null when the head has none.
 *
 * build_head.py (`cut_mouth` / `build_pouch`) closes the ripped lip slit with a pouch whose faces all map to one single
 * point of a reserved dark atlas cell that no skin UV covers. So the pouch triangles are exactly the triangles around
 * the mouth whose three UVs coincide, and they all share that one UV. The head pass compares the interpolated UV
 * against it to mask the mouth cavity per pixel, independent of the jaw and of the texture colour (JPEG-proof).
 *
 * Other point-mapped triangles exist (small holes filled with default UVs, e.g. at the back of the head or the crown):
 * only triangles whose centroid lies in a generous box around the `mouth` landmark count, and the UV shared by most of
 * them wins (at least CAVITY_MIN_TRIANGLES).
 */
export function findCavityUv(head: Pick<GlbHead, 'positions' | 'uvs' | 'indices' | 'landmarks'>): [number, number] | null {
  const { positions: p, uvs, indices: ix, landmarks: lm } = head;
  if (!uvs) return null;
  const [mx, my, mz] = lm.mouth;
  // half mouth width from the corners; the pouch spans about the lip slit, a little behind and below the lip line
  const halfW = Math.max(Math.abs(lm.mouthL[0] - mx), Math.abs(lm.mouthR[0] - mx), 0.05);
  const counts = new Map<string, { n: number; u: number; v: number }>();
  for (let t = 0; t + 2 < ix.length; t += 3) {
    const a = ix[t]!, b = ix[t + 1]!, c = ix[t + 2]!;
    const u = uvs[2 * a]!, v = uvs[2 * a + 1]!;
    if (uvs[2 * b] !== u || uvs[2 * c] !== u || uvs[2 * b + 1] !== v || uvs[2 * c + 1] !== v) continue;
    const cx = (p[3 * a]! + p[3 * b]! + p[3 * c]!) / 3;
    const cy = (p[3 * a + 1]! + p[3 * b + 1]! + p[3 * c + 1]!) / 3;
    const cz = (p[3 * a + 2]! + p[3 * b + 2]! + p[3 * c + 2]!) / 3;
    if (Math.abs(cx - mx) > 1.5 * halfW + 0.02 || Math.abs(cy - my) > 0.12 || cz < mz - 0.25 || cz > mz + 0.05) continue;
    const key = `${u},${v}`;
    const e = counts.get(key);
    if (e) e.n++;
    else counts.set(key, { n: 1, u, v });
  }
  let best: { n: number; u: number; v: number } | null = null;
  for (const e of counts.values()) if (!best || e.n > best.n) best = e;
  return best && best.n >= CAVITY_MIN_TRIANGLES ? [best.u, best.v] : null;
}
