import type { GlbHead } from './glb';
import { MORPH_NAMES } from './lipsync';

/** Morph deltas live in one RGBA32F texture, 2 texels (position, normal) per vertex and target, fetched by gl_VertexID. */
export const MORPH_TEX_WIDTH = 2048;
export const MAX_TARGETS = MORPH_NAMES.length;

/** GPU copy of a head: static vertex buffers + morph delta texture. Blending is done in the vertex shader. */
export class HeadMesh {
  readonly vao: WebGLVertexArrayObject;
  readonly morphTex: WebGLTexture;
  readonly indexCount: number;
  readonly indexType: number;
  /** current morph weights in contract order (0..1); uploaded as a uniform array by the pass */
  readonly weights = new Float32Array(MAX_TARGETS);
  readonly vertexCount: number;
  /** which of the 8 contract targets exist in the file */
  readonly present: boolean[];

  /** TEXCOORD_0 present (attribute 2); otherwise attribute 2 is the constant (0, 0) */
  readonly hasUv: boolean;

  private readonly buffers: WebGLBuffer[] = [];

  constructor(private readonly gl: WebGL2RenderingContext, glb: GlbHead) {
    this.vertexCount = glb.vertexCount;
    this.indexCount = glb.indices.length;
    this.indexType = glb.indices instanceof Uint32Array ? gl.UNSIGNED_INT : gl.UNSIGNED_SHORT;
    this.vao = gl.createVertexArray()!;
    gl.bindVertexArray(this.vao);
    const attr = (loc: number, data: Float32Array, size = 3): void => {
      const b = gl.createBuffer()!;
      this.buffers.push(b);
      gl.bindBuffer(gl.ARRAY_BUFFER, b);
      gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
      gl.enableVertexAttribArray(loc);
      gl.vertexAttribPointer(loc, size, gl.FLOAT, false, 0, 0);
    };
    attr(0, glb.positions);
    attr(1, glb.normals);
    this.hasUv = glb.uvs !== null;
    if (glb.uvs) attr(2, glb.uvs, 2);
    const ib = gl.createBuffer()!;
    this.buffers.push(ib);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, ib);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, glb.indices, gl.STATIC_DRAW);
    gl.bindVertexArray(null);

    // pack the deltas: texel (v * 8 + t) * 2 + {0: position, 1: normal}
    const byName = new Map(glb.targets.map((t) => [t.name, t]));
    this.present = MORPH_NAMES.map((n) => byName.has(n));
    const texels = glb.vertexCount * MAX_TARGETS * 2;
    const rows = Math.max(1, Math.ceil(texels / MORPH_TEX_WIDTH));
    const data = new Float32Array(MORPH_TEX_WIDTH * rows * 4);
    MORPH_NAMES.forEach((name, t) => {
      const tg = byName.get(name);
      if (!tg) return;
      for (let v = 0; v < glb.vertexCount; v++) {
        const o = ((v * MAX_TARGETS + t) * 2) * 4;
        data[o] = tg.position[v * 3]; data[o + 1] = tg.position[v * 3 + 1]; data[o + 2] = tg.position[v * 3 + 2];
        data[o + 4] = tg.normal[v * 3]; data[o + 5] = tg.normal[v * 3 + 1]; data[o + 6] = tg.normal[v * 3 + 2];
      }
    });
    this.morphTex = gl.createTexture()!;
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.morphTex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA32F, MORPH_TEX_WIDTH, rows);
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, MORPH_TEX_WIDTH, rows, gl.RGBA, gl.FLOAT, data);
  }

  draw(): void {
    const gl = this.gl;
    gl.bindVertexArray(this.vao);
    if (!this.hasUv) gl.vertexAttrib2f(2, 0, 0);
    gl.drawElements(gl.TRIANGLES, this.indexCount, this.indexType, 0);
  }

  dispose(): void {
    const gl = this.gl;
    gl.deleteVertexArray(this.vao);
    for (const b of this.buffers) gl.deleteBuffer(b);
    gl.deleteTexture(this.morphTex);
  }
}
