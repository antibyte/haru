import type { Program } from './program';
import { drawQuad } from './quad';

export type TexFormat = 'rgba16f' | 'rg16f' | 'r16f' | 'r8';

interface FormatInfo { internal: number; format: number; type: number }

function formatInfo(gl: WebGL2RenderingContext, fmt: TexFormat): FormatInfo {
  switch (fmt) {
    case 'rgba16f': return { internal: gl.RGBA16F, format: gl.RGBA, type: gl.HALF_FLOAT };
    case 'rg16f': return { internal: gl.RG16F, format: gl.RG, type: gl.HALF_FLOAT };
    case 'r16f': return { internal: gl.R16F, format: gl.RED, type: gl.HALF_FLOAT };
    case 'r8': return { internal: gl.R8, format: gl.RED, type: gl.UNSIGNED_BYTE };
  }
}

export class Target {
  readonly texel: [number, number];
  constructor(
    private readonly gl: WebGL2RenderingContext,
    readonly texture: WebGLTexture,
    readonly fbo: WebGLFramebuffer,
    readonly width: number,
    readonly height: number,
    readonly format: TexFormat,
  ) {
    this.texel = [1 / width, 1 / height];
  }
  /** Binds the texture to `unit` and returns the unit (for gl.uniform1i). */
  attach(unit: number): number {
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE0 + unit);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    return unit;
  }
  dispose(): void {
    this.gl.deleteFramebuffer(this.fbo);
    this.gl.deleteTexture(this.texture);
  }
}

export class DoubleTarget {
  constructor(public read: Target, public write: Target) {}
  swap(): void {
    const t = this.read;
    this.read = this.write;
    this.write = t;
  }
  dispose(): void {
    this.read.dispose();
    this.write.dispose();
  }
}

export function createTarget(
  gl: WebGL2RenderingContext,
  w: number,
  h: number,
  fmt: TexFormat,
  filter: 'linear' | 'nearest',
): Target {
  w = Math.max(1, Math.floor(w));
  h = Math.max(1, Math.floor(h));
  const f = formatInfo(gl, fmt);
  const tex = gl.createTexture()!;
  gl.activeTexture(gl.TEXTURE0);
  gl.bindTexture(gl.TEXTURE_2D, tex);
  const filt = filter === 'linear' ? gl.LINEAR : gl.NEAREST;
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filt);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filt);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  gl.texStorage2D(gl.TEXTURE_2D, 1, f.internal, w, h);
  const fbo = gl.createFramebuffer()!;
  gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
  const status = gl.checkFramebufferStatus(gl.FRAMEBUFFER);
  if (status !== gl.FRAMEBUFFER_COMPLETE) {
    gl.deleteFramebuffer(fbo);
    gl.deleteTexture(tex);
    throw new Error(`framebuffer incomplete (${fmt} ${w}x${h}): 0x${status.toString(16)}`);
  }
  gl.viewport(0, 0, w, h);
  gl.clearColor(0, 0, 0, 0);
  gl.clear(gl.COLOR_BUFFER_BIT);
  return new Target(gl, tex, fbo, w, h, fmt);
}

export function createDouble(
  gl: WebGL2RenderingContext,
  w: number,
  h: number,
  fmt: TexFormat,
  filter: 'linear' | 'nearest',
): DoubleTarget {
  return new DoubleTarget(createTarget(gl, w, h, fmt, filter), createTarget(gl, w, h, fmt, filter));
}

/** Allocates a new double buffer at (w,h) and blits the old content (stretched) into it. Disposes the old one. */
export function resizeDouble(
  gl: WebGL2RenderingContext,
  t: DoubleTarget,
  w: number,
  h: number,
  copy: Program,
): DoubleTarget {
  const fmt = t.read.format;
  const next = createDouble(gl, w, h, fmt, 'linear');
  gl.disable(gl.BLEND);
  gl.disable(gl.SCISSOR_TEST);
  copy.bind();
  gl.uniform1i(copy.uniforms['uTexture'], t.read.attach(0));
  gl.uniform2f(copy.uniforms['uTexel'], next.read.texel[0], next.read.texel[1]);
  drawQuad(gl, next.read);
  t.dispose();
  return next;
}

/** Min side = `res`, other side by aspect (w/h), both clamped to `max` (keeping the aspect). */
export function resolution(res: number, aspect: number, max: number): { w: number; h: number } {
  if (!(aspect > 0) || !isFinite(aspect)) aspect = 1;
  let w: number;
  let h: number;
  if (aspect >= 1) {
    h = res;
    w = res * aspect;
  } else {
    w = res;
    h = res / aspect;
  }
  const big = Math.max(w, h);
  if (big > max) {
    const s = max / big;
    w *= s;
    h *= s;
  }
  return { w: Math.max(2, Math.round(w)), h: Math.max(2, Math.round(h)) };
}
