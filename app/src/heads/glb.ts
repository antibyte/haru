/**
 * Minimal glTF 2.0 binary reader for the Haru head contract (assets/heads/README.md):
 * one mesh "Head", one primitive, POSITION/NORMAL/TEXCOORD_0/indices, morph targets with POSITION (+NORMAL)
 * deltas, mesh.extras.targetNames, scene.extras.haru landmarks and an optional embedded base-colour image.
 * Node transforms, sparse accessors, materials and animations are deliberately ignored.
 */

export const HARU_LANDMARKS = ['mouth', 'mouthL', 'mouthR', 'eyeL', 'eyeR', 'chin', 'crown', 'noseTip', 'jawPivot'] as const;
export type LandmarkName = (typeof HARU_LANDMARKS)[number];
export type Vec3 = [number, number, number];

export interface MorphTarget {
  name: string;
  /** xyz per vertex */
  position: Float32Array;
  /** xyz per vertex (zeros when the file has no NORMAL deltas) */
  normal: Float32Array;
}

export interface GlbHead {
  positions: Float32Array;
  normals: Float32Array;
  /** null when the primitive has no TEXCOORD_0 */
  uvs: Float32Array | null;
  indices: Uint16Array | Uint32Array;
  vertexCount: number;
  triangleCount: number;
  targets: MorphTarget[];
  landmarks: Record<LandmarkName, Vec3>;
  /** embedded base-colour image (JPEG/PNG), or null */
  image: Blob | null;
}

const MAGIC = 0x46546c67; // 'glTF'
const CHUNK_JSON = 0x4e4f534a;
const CHUNK_BIN = 0x004e4942;

const COMPONENT: Record<number, { size: number; read: (dv: DataView, o: number) => number; norm: number }> = {
  5120: { size: 1, read: (d, o) => d.getInt8(o), norm: 127 },
  5121: { size: 1, read: (d, o) => d.getUint8(o), norm: 255 },
  5122: { size: 2, read: (d, o) => d.getInt16(o, true), norm: 32767 },
  5123: { size: 2, read: (d, o) => d.getUint16(o, true), norm: 65535 },
  5125: { size: 4, read: (d, o) => d.getUint32(o, true), norm: 1 },
  5126: { size: 4, read: (d, o) => d.getFloat32(o, true), norm: 1 },
};
const TYPE_COUNT: Record<string, number> = { SCALAR: 1, VEC2: 2, VEC3: 3, VEC4: 4 };

/* eslint-disable @typescript-eslint/no-explicit-any */
type Json = any;

function fail(msg: string): never {
  throw new Error(`GLB: ${msg}`);
}

interface Chunks { json: Json; bin: Uint8Array | null }

function readChunks(buf: ArrayBuffer): Chunks {
  if (buf.byteLength < 20) fail('file too short');
  const dv = new DataView(buf);
  if (dv.getUint32(0, true) !== MAGIC) fail('bad magic (not a binary glTF file)');
  const version = dv.getUint32(4, true);
  if (version !== 2) fail(`unsupported glTF version ${version} (need 2)`);
  const total = Math.min(dv.getUint32(8, true), buf.byteLength);
  let off = 12;
  let json: Json = null;
  let bin: Uint8Array | null = null;
  while (off + 8 <= total) {
    const len = dv.getUint32(off, true);
    const type = dv.getUint32(off + 4, true);
    const start = off + 8;
    if (start + len > buf.byteLength) fail('chunk exceeds file size');
    if (type === CHUNK_JSON && json === null) {
      try {
        json = JSON.parse(new TextDecoder().decode(new Uint8Array(buf, start, len)));
      } catch (e) {
        fail(`invalid JSON chunk (${(e as Error).message})`);
      }
    } else if (type === CHUNK_BIN && bin === null) {
      bin = new Uint8Array(buf, start, len);
    }
    off = start + ((len + 3) & ~3);
  }
  if (json === null) fail('missing JSON chunk');
  return { json, bin };
}

/** Reads an accessor into a tightly packed Float32Array (components normalised when `normalized`). */
function readFloat(j: Json, bin: Uint8Array | null, index: number, expectType: string, label: string): Float32Array {
  const a = j.accessors?.[index];
  if (!a) fail(`${label}: accessor ${index} missing`);
  const n = TYPE_COUNT[a.type];
  if (n === undefined || a.type !== expectType) fail(`${label}: expected ${expectType}, got ${a.type}`);
  const comp = COMPONENT[a.componentType];
  if (!comp) fail(`${label}: unsupported componentType ${a.componentType}`);
  const out = new Float32Array(a.count * n);
  const scale = a.normalized ? 1 / comp.norm : 1;
  if (a.bufferView !== undefined) readDense(j, bin, a, n, comp, scale, out, label);
  if (a.sparse) applySparse(j, bin, a, n, comp, scale, out, label); // Blender exports morph deltas this way
  return out;
}

type Comp = (typeof COMPONENT)[number];

function readDense(j: Json, bin: Uint8Array | null, a: Json, n: number, comp: Comp, scale: number, out: Float32Array, label: string): void {
  const bv = j.bufferViews?.[a.bufferView];
  if (!bv) fail(`${label}: bufferView ${a.bufferView} missing`);
  if (!bin) fail(`${label}: no BIN chunk`);
  const stride = bv.byteStride || comp.size * n;
  const base = (bv.byteOffset || 0) + (a.byteOffset || 0);
  const last = base + stride * (a.count - 1) + comp.size * n;
  if (a.count > 0 && last > bin.byteLength) fail(`${label}: accessor reads past the BIN chunk`);
  const dv = new DataView(bin.buffer, bin.byteOffset, bin.byteLength);
  for (let i = 0; i < a.count; i++) {
    const o = base + i * stride;
    for (let c = 0; c < n; c++) {
      const v = comp.read(dv, o + c * comp.size) * scale;
      out[i * n + c] = a.normalized && v < -1 ? -1 : v;
    }
  }
}

function applySparse(j: Json, bin: Uint8Array | null, a: Json, n: number, comp: Comp, scale: number, out: Float32Array, label: string): void {
  const sp = a.sparse;
  if (!bin) fail(`${label}: no BIN chunk`);
  const ib = j.bufferViews?.[sp.indices?.bufferView];
  const vb = j.bufferViews?.[sp.values?.bufferView];
  if (!ib || !vb) fail(`${label}: sparse bufferView missing`);
  const ic = COMPONENT[sp.indices.componentType];
  if (!ic || sp.indices.componentType === 5126) fail(`${label}: unsupported sparse index type ${sp.indices.componentType}`);
  const ioff = (ib.byteOffset || 0) + (sp.indices.byteOffset || 0);
  const voff = (vb.byteOffset || 0) + (sp.values.byteOffset || 0);
  if (ioff + ic.size * sp.count > bin.byteLength || voff + comp.size * n * sp.count > bin.byteLength) {
    fail(`${label}: sparse data reads past the BIN chunk`);
  }
  const dv = new DataView(bin.buffer, bin.byteOffset, bin.byteLength);
  for (let k = 0; k < sp.count; k++) {
    const idx = ic.read(dv, ioff + k * ic.size);
    if (idx >= a.count) fail(`${label}: sparse index ${idx} out of range`);
    for (let c = 0; c < n; c++) out[idx * n + c] = comp.read(dv, voff + (k * n + c) * comp.size) * scale;
  }
}

function readIndices(j: Json, bin: Uint8Array | null, index: number, vertexCount: number): Uint16Array | Uint32Array {
  const a = j.accessors?.[index];
  if (!a) fail(`indices: accessor ${index} missing`);
  if (a.type !== 'SCALAR') fail(`indices: expected SCALAR, got ${a.type}`);
  if (a.componentType !== 5123 && a.componentType !== 5125 && a.componentType !== 5121) {
    fail(`indices: unsupported componentType ${a.componentType} (need uint16/uint32)`);
  }
  const f = readFloat(j, bin, index, 'SCALAR', 'indices');
  if (a.count % 3 !== 0) fail('indices: count is not a multiple of 3');
  const out = vertexCount > 65535 || a.componentType === 5125 ? new Uint32Array(f.length) : new Uint16Array(f.length);
  for (let i = 0; i < f.length; i++) {
    const v = f[i];
    if (v >= vertexCount) fail(`indices: index ${v} out of range (${vertexCount} vertices)`);
    out[i] = v;
  }
  return out;
}

function readLandmarks(j: Json): Record<LandmarkName, Vec3> {
  const src = j.scenes?.[j.scene ?? 0]?.extras?.haru ?? j.scenes?.find((s: Json) => s?.extras?.haru)?.extras?.haru;
  if (!src || typeof src !== 'object') fail('scene.extras.haru (landmarks) missing');
  const out = {} as Record<LandmarkName, Vec3>;
  for (const name of HARU_LANDMARKS) {
    const v = src[name];
    if (!Array.isArray(v) || v.length !== 3 || v.some((x: unknown) => typeof x !== 'number' || !Number.isFinite(x))) {
      fail(`landmark "${name}" missing or not [x,y,z]`);
    }
    out[name] = [v[0], v[1], v[2]];
  }
  return out;
}

function readImage(j: Json, bin: Uint8Array | null): Blob | null {
  try {
    const tex = j.materials?.[0]?.pbrMetallicRoughness?.baseColorTexture?.index;
    if (tex === undefined) return null;
    const img = j.images?.[j.textures?.[tex]?.source];
    if (!img || img.bufferView === undefined || !bin) return null;
    const bv = j.bufferViews?.[img.bufferView];
    if (!bv) return null;
    const start = bv.byteOffset || 0;
    if (start + bv.byteLength > bin.byteLength) return null;
    const mime = img.mimeType === 'image/png' ? 'image/png' : 'image/jpeg';
    return new Blob([bin.slice(start, start + bv.byteLength)], { type: mime });
  } catch {
    return null;
  }
}

export function parseGlb(buf: ArrayBuffer): GlbHead {
  const { json: j, bin } = readChunks(buf);
  const mesh = j.meshes?.find((m: Json) => m.name === 'Head') ?? j.meshes?.[0];
  if (!mesh) fail('no mesh');
  const prim = mesh.primitives?.[0];
  if (!prim) fail('mesh has no primitive');
  if (prim.mode !== undefined && prim.mode !== 4) fail(`primitive mode ${prim.mode} is not TRIANGLES`);
  const at = prim.attributes ?? {};
  if (at.POSITION === undefined) fail('POSITION attribute missing');
  if (at.NORMAL === undefined) fail('NORMAL attribute missing');
  if (prim.indices === undefined) fail('indices missing (non-indexed meshes are not supported)');

  const positions = readFloat(j, bin, at.POSITION, 'VEC3', 'POSITION');
  const normals = readFloat(j, bin, at.NORMAL, 'VEC3', 'NORMAL');
  const vertexCount = positions.length / 3;
  if (normals.length !== positions.length) fail('NORMAL count differs from POSITION count');
  const uvs = at.TEXCOORD_0 !== undefined ? readFloat(j, bin, at.TEXCOORD_0, 'VEC2', 'TEXCOORD_0') : null;
  if (uvs && uvs.length / 2 !== vertexCount) fail('TEXCOORD_0 count differs from POSITION count');
  const indices = readIndices(j, bin, prim.indices, vertexCount);

  const names: unknown = mesh.extras?.targetNames ?? prim.extras?.targetNames ?? [];
  const rawTargets: Json[] = prim.targets ?? [];
  if (!Array.isArray(names)) fail('mesh.extras.targetNames is not an array');
  if (rawTargets.length > 0 && names.length !== rawTargets.length) {
    fail(`targetNames has ${names.length} entries but the primitive has ${rawTargets.length} morph targets`);
  }
  const targets: MorphTarget[] = rawTargets.map((t, i) => {
    if (t.POSITION === undefined) fail(`morph target ${i}: POSITION missing`);
    const position = readFloat(j, bin, t.POSITION, 'VEC3', `target ${i} POSITION`);
    if (position.length !== positions.length) fail(`morph target ${i}: POSITION count differs`);
    const normal = t.NORMAL !== undefined ? readFloat(j, bin, t.NORMAL, 'VEC3', `target ${i} NORMAL`) : new Float32Array(position.length);
    if (normal.length !== positions.length) fail(`morph target ${i}: NORMAL count differs`);
    return { name: String(names[i]), position, normal };
  });

  return {
    positions, normals, uvs, indices, vertexCount, triangleCount: indices.length / 3, targets,
    landmarks: readLandmarks(j), image: readImage(j, bin),
  };
}
