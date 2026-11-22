import type { GLContext } from '../gl/context';
import { Program } from '../gl/program';
import { createTarget, type Target } from '../gl/fbo';
import { drawQuad } from '../gl/quad';

import baseVert from '../fluid/shaders/base.vert?raw';
import prefilterFrag from './shaders/bloom_prefilter.frag?raw';
import downFrag from './shaders/kawase_down.frag?raw';
import upFrag from './shaders/kawase_up.frag?raw';
import godraysFrag from './shaders/godrays.frag?raw';
import compositeFrag from './shaders/composite.frag?raw';

/** Structural subset of render/quality.ts QualityParams used by the post pipeline. */
export interface PostQuality {
  bloomLevels: number;
  raySamples: number;
  rayScale: number;
}

export interface PostUniforms {
  timeSec: number;
  /** smoothed model RMS 0..1: drives chromatic aberration strength */
  rms: number;
  /** god-ray origin in normalized canvas coords (y up) */
  lightPos: { x: number; y: number };
  /** 1 = daemon ember look (dark red haze, ember grade), 0 = neutral (identical to before) */
  ember?: number;
}

/**
 * Skin grade of a textured head (structural copy of heads/face.ts HeadGrade):
 * skin = pow(albedo, gamma) * lift, then mix(lum(skin) * tint, skin, saturation).
 */
export interface SkinGrade {
  gamma: number;
  lift: number;
  tint: readonly [number, number, number];
  saturation: number;
}

/** Identity grade (skin = albedo); also what porcelain heads get (the composite never evaluates it for them). */
const NEUTRAL_SKIN: SkinGrade = { gamma: 1, lift: 1, tint: [1, 1, 1], saturation: 1 };

/** The face layer's inputs for the composite (null / undefined = no face this frame). */
export interface HeadPostInputs {
  texture: WebGLTexture;
  amount: number;
  light: [number, number, number];
  /** G-buffer texel size in UV units */
  texel: [number, number];
  /** face centre (eyes-nose-mouth) in UV (y up) and the chin-to-crown height in UV y units */
  focus: [number, number];
  size: number;
  /** per-head presence gain (normalises male / female) */
  gain?: number;
  /** 1 = textured head: shade with `albedo` instead of porcelain (0 / undefined = the unchanged porcelain look) */
  textured?: number;
  /** G-buffer albedo attachment (rgb linear, a = coverage of textured geometry) */
  albedo?: WebGLTexture | null;
  /** visibility cap for textured heads */
  visMax?: number;
  /** per-head skin grade for textured heads (undefined = neutral: skin = albedo) */
  grade?: SkinGrade;
  /** debug (?facedebug): show the face without ink gate and dissolve mask */
  debug?: boolean;
}

/** Weight with which each smaller bloom level is added to the next larger one. */
const UP_WEIGHT = 0.7;

/**
 * Post-processing: dye (RGBA16F HDR) → bloom prefilter (½ dye res) → dual-Kawase chain → god rays →
 * composite (glass shading, bloom, rays, CA, ACES, vignette, grain, dither) onto the default framebuffer.
 */
export class PostFX {
  private readonly gl: WebGL2RenderingContext;
  private readonly glc: GLContext;
  private q: PostQuality;
  private w: number;
  private h: number;

  private bloom: Target[] = [];
  private bloomForDye = '';
  /** threshold-free blurred copies of the dye at dye>>2 .. >>5 (local ink / fog glow for the face) */
  private ink: Target[] = [];
  private inkForDye = '';
  private rays: Target | null = null;

  private readonly pPrefilter: Program;
  private readonly pDown: Program;
  private readonly pUp: Program;
  private readonly pRays: Program;
  private readonly pComposite: Program;

  constructor(glc: GLContext, q: PostQuality) {
    this.glc = glc;
    this.gl = glc.gl;
    this.q = { ...q };
    this.w = glc.canvas.width;
    this.h = glc.canvas.height;
    const gl = this.gl;
    this.pPrefilter = new Program(gl, baseVert, prefilterFrag);
    this.pDown = new Program(gl, baseVert, downFrag);
    this.pUp = new Program(gl, baseVert, upFrag);
    this.pRays = new Program(gl, baseVert, godraysFrag);
    this.pComposite = new Program(gl, baseVert, compositeFrag);
    this.allocRays();
  }

  /** w,h = drawing-buffer size in pixels. */
  resize(w: number, h: number, q: PostQuality): void {
    this.w = Math.max(1, w);
    this.h = Math.max(1, h);
    this.q = { ...q };
    this.bloomForDye = '';
    this.inkForDye = ''; // re-create the chain lazily (level count may have changed)
    this.allocRays();
  }

  private allocRays(): void {
    this.rays?.dispose();
    const s = this.q.rayScale;
    this.rays = createTarget(
      this.gl,
      Math.max(2, Math.round(this.w * s)),
      Math.max(2, Math.round(this.h * s)),
      'rgba16f',
      'linear',
    );
  }

  private ensureBloom(dye: Target): void {
    const key = `${dye.width}x${dye.height}:${this.q.bloomLevels}`;
    if (key === this.bloomForDye) return;
    for (const t of this.bloom) t.dispose();
    this.bloom = [];
    let w = Math.max(2, dye.width >> 1);
    let h = Math.max(2, dye.height >> 1);
    const n = Math.max(2, this.q.bloomLevels);
    for (let i = 0; i < n; i++) {
      this.bloom.push(createTarget(this.gl, w, h, 'rgba16f', 'linear'));
      w = Math.max(2, w >> 1);
      h = Math.max(2, h >> 1);
    }
    this.bloomForDye = key;
  }

  private ensureInk(dye: Target): void {
    const key = `${dye.width}x${dye.height}`;
    if (key === this.inkForDye) return;
    for (const t of this.ink) t.dispose();
    this.ink = [];
    for (let i = 2; i <= 5; i++) {
      this.ink.push(createTarget(this.gl, Math.max(2, dye.width >> i), Math.max(2, dye.height >> i), 'rgba16f', 'linear'));
    }
    this.inkForDye = key;
  }

  /** Renders the HDR dye field to the default framebuffer. */
  render(dye: Target, u: PostUniforms, head?: HeadPostInputs | null): void {
    const gl = this.gl;
    this.ensureBloom(dye);
    const bloom = this.bloom;
    gl.disable(gl.BLEND);
    gl.disable(gl.SCISSOR_TEST);

    // 1. prefilter
    let p = this.pPrefilter;
    p.bind();
    gl.uniform1i(p.uniforms['uSource'], dye.attach(0));
    gl.uniform2f(p.uniforms['uTexel'], dye.texel[0], dye.texel[1]);
    gl.uniform1f(p.uniforms['uThreshold'], 0.4);
    gl.uniform1f(p.uniforms['uKnee'], 0.3);
    drawQuad(gl, bloom[0]);

    // 2. dual-Kawase down
    p = this.pDown;
    p.bind();
    for (let i = 1; i < bloom.length; i++) {
      const src = bloom[i - 1];
      gl.uniform1i(p.uniforms['uSource'], src.attach(0));
      gl.uniform2f(p.uniforms['uTexel'], src.texel[0], src.texel[1]);
      drawQuad(gl, bloom[i]);
    }

    // 3. dual-Kawase up, additively accumulated into the next larger level
    p = this.pUp;
    p.bind();
    gl.enable(gl.BLEND);
    gl.blendEquation(gl.FUNC_ADD);
    gl.blendFunc(gl.ONE, gl.ONE);
    gl.uniform1f(p.uniforms['uWeight'], UP_WEIGHT);
    for (let i = bloom.length - 1; i >= 1; i--) {
      const src = bloom[i];
      gl.uniform1i(p.uniforms['uSource'], src.attach(0));
      gl.uniform2f(p.uniforms['uTexel'], src.texel[0], src.texel[1]);
      drawQuad(gl, bloom[i - 1]);
    }
    gl.disable(gl.BLEND);

    // 4. god rays from the accumulated bloom
    const rays = this.rays!;
    p = this.pRays;
    p.bind();
    gl.uniform1i(p.uniforms['uBloom'], bloom[0].attach(0));
    gl.uniform2f(p.uniforms['uTexel'], rays.texel[0], rays.texel[1]);
    gl.uniform2f(p.uniforms['uLight'], u.lightPos.x, u.lightPos.y);
    gl.uniform1i(p.uniforms['uSamples'], Math.max(4, Math.min(128, this.q.raySamples | 0)));
    gl.uniform1f(p.uniforms['uDensity'], 0.8);
    gl.uniform1f(p.uniforms['uDecay'], 0.96);
    gl.uniform1f(p.uniforms['uWeight'], 0.3);
    gl.uniform1f(p.uniforms['uExposure'], 0.5);
    gl.uniform1f(p.uniforms['uAspect'], this.w / this.h);
    drawQuad(gl, rays);

    // 4b. blurred ink chain for the face (only while it is drawn)
    const useHead = !!head && head.amount > 0;
    if (useHead) {
      this.ensureInk(dye);
      p = this.pDown;
      p.bind();
      for (let i = 0; i < this.ink.length; i++) {
        const src = i === 0 ? dye : this.ink[i - 1];
        gl.uniform1i(p.uniforms['uSource'], src.attach(0));
        gl.uniform2f(p.uniforms['uTexel'], src.texel[0], src.texel[1]);
        drawQuad(gl, this.ink[i]);
      }
    }

    // 5. composite to screen
    p = this.pComposite;
    p.bind();
    gl.uniform1i(p.uniforms['uDye'], dye.attach(0));
    gl.uniform1i(p.uniforms['uBloom'], bloom[0].attach(1));
    gl.uniform1i(p.uniforms['uRays'], rays.attach(2));
    gl.uniform2f(p.uniforms['uTexel'], 1 / this.w, 1 / this.h);
    gl.uniform2f(p.uniforms['uDyeTexel'], dye.texel[0], dye.texel[1]);
    gl.uniform2f(p.uniforms['uResolution'], this.w, this.h);
    gl.uniform1f(p.uniforms['uAspect'], this.w / this.h);
    gl.uniform1f(p.uniforms['uTime'], u.timeSec);
    gl.uniform1f(p.uniforms['uRms'], Math.max(0, Math.min(1, u.rms)));
    gl.uniform1f(p.uniforms['uExposure'], 1.0);
    gl.uniform1f(p.uniforms['uEmber'], u.ember ?? 0);
    if (head && useHead) {
      gl.uniform1i(p.uniforms['uInk'], this.ink[3].attach(4));
      gl.uniform1i(p.uniforms['uInkNear'], this.ink[1].attach(5));
      gl.uniform2f(p.uniforms['uHeadTexel'], head.texel[0], head.texel[1]);
      gl.uniform2f(p.uniforms['uFocus'], head.focus[0], head.focus[1]);
      gl.uniform1f(p.uniforms['uFaceSize'], head.size);
      gl.uniform1f(p.uniforms['uHeadGain'], head.gain ?? 1);
      const textured = !!(head.textured && head.albedo);
      if (textured) {
        gl.activeTexture(gl.TEXTURE6);
        gl.bindTexture(gl.TEXTURE_2D, head.albedo!);
        gl.uniform1i(p.uniforms['uHeadAlb'], 6);
        gl.uniform1f(p.uniforms['uHeadTextured'], 1);
        gl.uniform1f(p.uniforms['uHeadVisMax'], head.visMax ?? 0.42);
      } else {
        gl.uniform1i(p.uniforms['uHeadAlb'], 0);
        gl.uniform1f(p.uniforms['uHeadTextured'], 0);
        gl.uniform1f(p.uniforms['uHeadVisMax'], 0.28);
      }
      const g = textured ? head.grade ?? NEUTRAL_SKIN : NEUTRAL_SKIN;
      gl.uniform1f(p.uniforms['uSkinGamma'], g.gamma);
      gl.uniform1f(p.uniforms['uSkinLift'], g.lift);
      gl.uniform3f(p.uniforms['uSkinTint'], g.tint[0], g.tint[1], g.tint[2]);
      gl.uniform1f(p.uniforms['uSkinSat'], g.saturation);
      gl.activeTexture(gl.TEXTURE3);
      gl.bindTexture(gl.TEXTURE_2D, head.texture);
      gl.uniform1i(p.uniforms['uHead'], 3);
      gl.uniform1f(p.uniforms['uHeadAmt'], Math.min(1, head.amount));
      gl.uniform3f(p.uniforms['uHeadLight'], head.light[0], head.light[1], head.light[2]);
      gl.uniform1f(p.uniforms['uHeadDebug'], head.debug ? 1 : 0);
    } else {
      gl.uniform1i(p.uniforms['uHead'], 0);
      gl.uniform1f(p.uniforms['uHeadAmt'], 0);
    }
    drawQuad(gl, null, [this.w, this.h]);
  }
}
