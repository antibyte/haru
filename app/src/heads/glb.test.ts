import { describe, expect, it } from 'vitest';
import { parseGlb, HARU_LANDMARKS } from './glb';

interface BuildOpts { sparse?: boolean; u32?: boolean; omitLandmark?: string; version?: number; image?: boolean; badTargetNames?: boolean }

const pad4 = (n: number) => (n + 3) & ~3;

/** A tiny quad (4 vertices, 2 triangles) with an interleaved POSITION+NORMAL view (byteStride 24) and 2 morph targets. */
function buildGlb(o: BuildOpts = {}): ArrayBuffer {
  const parts: ArrayBuffer[] = [];
  const views: Array<Record<string, unknown>> = [];
  const add = (data: ArrayBufferView, extra: Record<string, unknown> = {}): number => {
    const offset = parts.reduce((s, p) => s + p.byteLength, 0);
    const bytes = new Uint8Array(pad4(data.byteLength));
    bytes.set(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
    parts.push(bytes.buffer);
    views.push({ buffer: 0, byteOffset: offset, byteLength: data.byteLength, ...extra });
    return views.length - 1;
  };
  const pos = [[-1, -1, 0], [1, -1, 0], [1, 1, 0], [-1, 1, 0]];
  const inter = new Float32Array(4 * 6);
  pos.forEach((p, i) => inter.set([...p, 0, 0, 1], i * 6));
  const vInter = add(inter, { byteStride: 24 });
  const uv = new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]);
  const vUv = add(uv);
  const idx = o.u32 ? new Uint32Array([0, 1, 2, 0, 2, 3]) : new Uint16Array([0, 1, 2, 0, 2, 3]);
  const vIdx = add(idx);
  const d0 = new Float32Array([0, -0.5, 0, 0, -0.5, 0, 0, 0, 0, 0, 0, 0]); // jawOpen: first two verts down
  const d1 = new Float32Array([0.1, 0, 0, 0.1, 0, 0, 0.1, 0, 0, 0.1, 0, 0]);
  const dn = new Float32Array(12).fill(0.25);
  const v0 = add(d0), v1 = add(d1), vn = add(dn);
  let sIdx = -1, sVal = -1;
  if (o.sparse) {
    sIdx = add(new Uint16Array([1, 3]));
    sVal = add(new Float32Array([0.5, 0.25, 0, -1, -2, -3]));
  }
  let image = -1;
  if (o.image) image = add(new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 1, 2, 3]));

  const acc = [
    { bufferView: vInter, byteOffset: 0, componentType: 5126, count: 4, type: 'VEC3' },
    { bufferView: vInter, byteOffset: 12, componentType: 5126, count: 4, type: 'VEC3' },
    { bufferView: vUv, componentType: 5126, count: 4, type: 'VEC2' },
    { bufferView: vIdx, componentType: o.u32 ? 5125 : 5123, count: 6, type: 'SCALAR' },
    { bufferView: v0, componentType: 5126, count: 4, type: 'VEC3' },
    { bufferView: vn, componentType: 5126, count: 4, type: 'VEC3' },
    { bufferView: v1, componentType: 5126, count: 4, type: 'VEC3' },
    o.sparse
      ? { componentType: 5126, count: 4, type: 'VEC3', sparse: { count: 2, indices: { bufferView: sIdx, componentType: 5123 }, values: { bufferView: sVal } } }
      : { componentType: 5126, count: 4, type: 'VEC3' }, // no bufferView: zero deltas
  ];
  const haru: Record<string, number[]> = {};
  HARU_LANDMARKS.forEach((n, i) => { haru[n] = [i, i + 0.5, -i]; });
  if (o.omitLandmark) delete haru[o.omitLandmark];
  const json = {
    asset: { version: '2.0' },
    scene: 0,
    scenes: [{ nodes: [0], extras: { haru } }],
    nodes: [{ mesh: 0 }],
    meshes: [{
      name: 'Head',
      extras: { targetNames: o.badTargetNames ? ['a'] : ['jawOpen', 'lipsWide'] },
      primitives: [{
        attributes: { POSITION: 0, NORMAL: 1, TEXCOORD_0: 2 }, indices: 3,
        targets: [{ POSITION: 4, NORMAL: 5 }, { POSITION: 6, NORMAL: 7 }],
      }],
    }],
    materials: o.image ? [{ pbrMetallicRoughness: { baseColorTexture: { index: 0 } } }] : undefined,
    textures: o.image ? [{ source: 0 }] : undefined,
    images: o.image ? [{ bufferView: image, mimeType: 'image/jpeg' }] : undefined,
    accessors: acc,
    bufferViews: views,
    buffers: [{ byteLength: parts.reduce((s, p) => s + p.byteLength, 0) }],
  };
  const jb = new TextEncoder().encode(JSON.stringify(json));
  const jpad = new Uint8Array(pad4(jb.length)).fill(0x20);
  jpad.set(jb);
  const bin = new Uint8Array(parts.reduce((s, p) => s + p.byteLength, 0));
  let off = 0;
  for (const p of parts) { bin.set(new Uint8Array(p), off); off += p.byteLength; }
  const total = 12 + 8 + jpad.length + 8 + bin.length;
  const out = new ArrayBuffer(total);
  const dv = new DataView(out);
  dv.setUint32(0, 0x46546c67, true);
  dv.setUint32(4, o.version ?? 2, true);
  dv.setUint32(8, total, true);
  dv.setUint32(12, jpad.length, true);
  dv.setUint32(16, 0x4e4f534a, true);
  new Uint8Array(out, 20, jpad.length).set(jpad);
  const b0 = 20 + jpad.length;
  dv.setUint32(b0, bin.length, true);
  dv.setUint32(b0 + 4, 0x004e4942, true);
  new Uint8Array(out, b0 + 8, bin.length).set(bin);
  return out;
}

describe('parseGlb', () => {
  it('reads interleaved attributes, indices, morph targets and landmarks', () => {
    const g = parseGlb(buildGlb());
    expect(g.vertexCount).toBe(4);
    expect(g.triangleCount).toBe(2);
    expect(Array.from(g.positions.slice(0, 6))).toEqual([-1, -1, 0, 1, -1, 0]);
    expect(Array.from(g.normals.slice(0, 3))).toEqual([0, 0, 1]);
    expect(Array.from(g.normals.slice(9, 12))).toEqual([0, 0, 1]);
    expect(Array.from(g.uvs!)).toEqual([0, 0, 1, 0, 1, 1, 0, 1]);
    expect(g.indices).toBeInstanceOf(Uint16Array);
    expect(Array.from(g.indices)).toEqual([0, 1, 2, 0, 2, 3]);
    expect(g.targets.map((t) => t.name)).toEqual(['jawOpen', 'lipsWide']);
    expect(g.targets[0].position[1]).toBe(-0.5);
    expect(g.targets[0].normal[0]).toBe(0.25);
    expect(Array.from(g.targets[1].normal)).toEqual(new Array(12).fill(0)); // accessor without bufferView
    expect(g.landmarks.mouth).toEqual([0, 0.5, 0]);
    expect(g.landmarks.jawPivot).toEqual([8, 8.5, -8]);
    expect(g.image).toBeNull();
  });

  it('reads sparse accessors (Blender morph deltas)', () => {
    const g = parseGlb(buildGlb({ sparse: true }));
    expect(Array.from(g.targets[1].normal)).toEqual([0, 0, 0, 0.5, 0.25, 0, 0, 0, 0, -1, -2, -3]);
  });

  it('reads uint32 indices', () => {
    const g = parseGlb(buildGlb({ u32: true }));
    expect(g.indices).toBeInstanceOf(Uint32Array);
    expect(Array.from(g.indices)).toEqual([0, 1, 2, 0, 2, 3]);
  });

  it('extracts the embedded image as a Blob', () => {
    const g = parseGlb(buildGlb({ image: true }));
    expect(g.image).not.toBeNull();
    expect(g.image!.type).toBe('image/jpeg');
    expect(g.image!.size).toBe(7);
  });

  it('rejects a bad magic', () => {
    const b = buildGlb();
    new DataView(b).setUint32(0, 0x3c21444f, true); // "<!DO" (an HTML fallback page)
    expect(() => parseGlb(b)).toThrow(/bad magic/);
  });

  it('rejects another version', () => {
    expect(() => parseGlb(buildGlb({ version: 1 }))).toThrow(/version 1/);
  });

  it('rejects a truncated file', () => {
    expect(() => parseGlb(new ArrayBuffer(8))).toThrow(/too short/);
    expect(() => parseGlb(buildGlb().slice(0, 100))).toThrow(/GLB:/);
  });

  it('names a missing landmark', () => {
    expect(() => parseGlb(buildGlb({ omitLandmark: 'jawPivot' }))).toThrow(/jawPivot/);
  });

  it('rejects mismatched target names', () => {
    expect(() => parseGlb(buildGlb({ badTargetNames: true }))).toThrow(/targetNames/);
  });
});
