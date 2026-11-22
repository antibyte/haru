import type { GLContext } from '../gl/context';
import { Program } from '../gl/program';
import {
  createDouble,
  createTarget,
  resizeDouble,
  resolution,
  type DoubleTarget,
  type Target,
} from '../gl/fbo';
import { drawQuad } from '../gl/quad';
import { DEFAULT_FLUID, type Box, type FluidParams, type RGB } from './params';

import baseVert from './shaders/base.vert?raw';
import copyFrag from './shaders/copy.frag?raw';
import advectFrag from './shaders/advect.frag?raw';
import divergenceFrag from './shaders/divergence.frag?raw';
import curlFrag from './shaders/curl.frag?raw';
import vorticityFrag from './shaders/vorticity.frag?raw';
import clearFrag from './shaders/clear.frag?raw';
import pressureFrag from './shaders/pressure.frag?raw';
import gradientFrag from './shaders/gradient.frag?raw';
import splatFrag from './shaders/splat.frag?raw';
import maskSplatFrag from './shaders/mask_splat.frag?raw';
import edgeSplatFrag from './shaders/edge_splat.frag?raw';
import restMaskFrag from './shaders/rest_mask.frag?raw';
import restHeadFrag from './shaders/rest_head.frag?raw';
import flowFrag from './shaders/flow.frag?raw';

/** Edge-splat swirl/push are expressed in grid-cells/s per second; applied per frame as `value * dt`. */
const MAX_DT = 1 / 30;

/**
 * Stable-fluids simulation (Dobryakov style) with an HDR RGBA16F dye field.
 *
 * Conventions
 *  - Coordinates are normalized canvas coordinates, y up, (0,0) bottom-left.
 *  - Velocity is in simulation-grid cells per second.
 *  - Mask textures are R8 and stored in canvas row order (first row = top of the glyph); shaders flip v.
 *  - Sim grids follow the canvas aspect (min side = simRes / dyeRes), so cells are square.
 */
export class FluidSim {
  params: FluidParams = { ...DEFAULT_FLUID };

  private readonly gl: WebGL2RenderingContext;
  private readonly glc: GLContext;

  private vel!: DoubleTarget;
  private dyeT!: DoubleTarget;
  private pressureT!: DoubleTarget;
  private divergenceT!: Target;
  private curlT!: Target;
  private restT!: Target;

  private simRes: number;
  private dyeRes: number;
  private aspect = 1;
  private lastDt = 1 / 60;

  private readonly pCopy: Program;
  private readonly pAdvect: Program;
  private readonly pDivergence: Program;
  private readonly pCurl: Program;
  private readonly pVorticity: Program;
  private readonly pClear: Program;
  private readonly pPressure: Program;
  private readonly pGradient: Program;
  private readonly pSplat: Program;
  private readonly pMaskSplat: Program;
  private readonly pEdgeSplat: Program;
  private readonly pRestMask: Program;
  private readonly pRestHead: Program;
  private readonly pFlow: Program;
  private flowTime = 0;
  private stepNo = 0;

  constructor(glc: GLContext, simRes: number, dyeRes: number) {
    this.glc = glc;
    this.gl = glc.gl;
    this.simRes = simRes;
    this.dyeRes = dyeRes;
    const gl = this.gl;
    this.pCopy = new Program(gl, baseVert, copyFrag);
    this.pAdvect = new Program(gl, baseVert, advectFrag);
    this.pDivergence = new Program(gl, baseVert, divergenceFrag);
    this.pCurl = new Program(gl, baseVert, curlFrag);
    this.pVorticity = new Program(gl, baseVert, vorticityFrag);
    this.pClear = new Program(gl, baseVert, clearFrag);
    this.pPressure = new Program(gl, baseVert, pressureFrag);
    this.pGradient = new Program(gl, baseVert, gradientFrag);
    this.pSplat = new Program(gl, baseVert, splatFrag);
    this.pMaskSplat = new Program(gl, baseVert, maskSplatFrag);
    this.pEdgeSplat = new Program(gl, baseVert, edgeSplatFrag);
    this.pRestMask = new Program(gl, baseVert, restMaskFrag);
    this.pRestHead = new Program(gl, baseVert, restHeadFrag);
    this.pFlow = new Program(gl, baseVert, flowFrag);
    this.allocate(true);
  }

  get dye(): Target {
    return this.dyeT.read;
  }
  get velocity(): Target {
    return this.vel.read;
  }
  /** width / height of the simulation domain (equals the canvas aspect at the last resize). */
  get domainAspect(): number {
    return this.aspect;
  }

  resize(simRes: number, dyeRes: number): void {
    this.simRes = simRes;
    this.dyeRes = dyeRes;
    this.allocate(false);
  }

  private canvasAspect(): number {
    const c = this.glc.canvas;
    return c.width > 0 && c.height > 0 ? c.width / c.height : 1;
  }

  private allocate(first: boolean): void {
    const gl = this.gl;
    this.aspect = this.canvasAspect();
    const max = this.glc.maxTextureSize;
    const s = resolution(this.simRes, this.aspect, max);
    const d = resolution(this.dyeRes, this.aspect, max);
    if (first) {
      this.vel = createDouble(gl, s.w, s.h, 'rg16f', 'linear');
      this.dyeT = createDouble(gl, d.w, d.h, 'rgba16f', 'linear');
    } else {
      this.vel = resizeDouble(gl, this.vel, s.w, s.h, this.pCopy);
      this.dyeT = resizeDouble(gl, this.dyeT, d.w, d.h, this.pCopy);
      this.divergenceT.dispose();
      this.curlT.dispose();
      this.restT.dispose();
      this.pressureT.dispose();
    }
    this.divergenceT = createTarget(gl, s.w, s.h, 'r16f', 'nearest');
    this.curlT = createTarget(gl, s.w, s.h, 'r16f', 'nearest');
    this.restT = createTarget(gl, s.w, s.h, 'r16f', 'linear');
    this.pressureT = createDouble(gl, s.w, s.h, 'r16f', 'nearest');
  }

  // ---------------------------------------------------------------- splats

  /** Velocity (dx, dy in grid cells/s) + dye gaussian. radius is the gaussian variance in height^2 units (0.0025 ≈ 5% of screen height). */
  splat(x: number, y: number, dx: number, dy: number, color: RGB, radius: number, velRadius: number = radius): void {
    const gl = this.gl;
    const p = this.pSplat;
    gl.enable(gl.BLEND);
    gl.blendEquation(gl.FUNC_ADD);
    gl.blendFunc(gl.ONE, gl.ONE);
    // the gaussian is below 1e-5 beyond sqrt(11.5 * radius): restrict the fill to that square
    const ext = Math.sqrt(11.5 * radius);
    const sciBox: Box = { x, y, w: (2 * ext) / this.aspect, h: 2 * ext };
    const vext = Math.sqrt(11.5 * velRadius);
    const velBox: Box = { x, y, w: (2 * vext) / this.aspect, h: 2 * vext };
    p.bind();
    gl.uniform1f(p.uniforms['uAspect'], this.aspect);
    gl.uniform2f(p.uniforms['uPoint'], x, y);
    gl.uniform1f(p.uniforms['uRadius'], velRadius);
    gl.uniform3f(p.uniforms['uColor'], dx, dy, 0);
    gl.uniform2f(p.uniforms['uTexel'], this.vel.read.texel[0], this.vel.read.texel[1]);
    this.scissorBox(this.vel.read, velBox);
    drawQuad(gl, this.vel.read);
    gl.uniform1f(p.uniforms['uRadius'], radius);
    gl.uniform3f(p.uniforms['uColor'], color[0], color[1], color[2]);
    gl.uniform2f(p.uniforms['uTexel'], this.dyeT.read.texel[0], this.dyeT.read.texel[1]);
    this.scissorBox(this.dyeT.read, sciBox);
    drawQuad(gl, this.dyeT.read);
    gl.disable(gl.SCISSOR_TEST);
    gl.disable(gl.BLEND);
  }

  private scissorBox(t: Target, box: Box): void {
    const gl = this.gl;
    const x0 = Math.max(0, Math.floor((box.x - box.w / 2) * t.width) - 1);
    const y0 = Math.max(0, Math.floor((box.y - box.h / 2) * t.height) - 1);
    const x1 = Math.min(t.width, Math.ceil((box.x + box.w / 2) * t.width) + 1);
    const y1 = Math.min(t.height, Math.ceil((box.y + box.h / 2) * t.height) + 1);
    gl.enable(gl.SCISSOR_TEST);
    gl.scissor(x0, y0, Math.max(0, x1 - x0), Math.max(0, y1 - y0));
  }

  /** Adds `color * strength * mask` inside `box` to the dye field (additive). */
  maskSplatDye(mask: WebGLTexture, box: Box, color: RGB, strength: number): void {
    const gl = this.gl;
    const p = this.pMaskSplat;
    const t = this.dyeT.read;
    gl.enable(gl.BLEND);
    gl.blendEquation(gl.FUNC_ADD);
    gl.blendFunc(gl.ONE, gl.ONE);
    this.scissorBox(t, box);
    p.bind();
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, mask);
    gl.uniform1i(p.uniforms['uMask'], 0);
    gl.uniform4f(p.uniforms['uBox'], box.x, box.y, box.w, box.h);
    gl.uniform3f(p.uniforms['uColor'], color[0], color[1], color[2]);
    gl.uniform1f(p.uniforms['uStrength'], strength);
    gl.uniform2f(p.uniforms['uTexel'], t.texel[0], t.texel[1]);
    drawQuad(gl, t);
    gl.disable(gl.SCISSOR_TEST);
    gl.disable(gl.BLEND);
  }

  /**
   * Adds swirl (tangential) and push (outward) velocity along the glyph edges (mask gradient).
   * `swirl`/`push` are in grid-cells/s gained per second of simulated time at full edge contrast.
   */
  edgeSplatVelocity(mask: WebGLTexture, maskTexel: [number, number], box: Box, swirl: number, push: number): void {
    const gl = this.gl;
    const p = this.pEdgeSplat;
    const t = this.vel.read;
    gl.enable(gl.BLEND);
    gl.blendEquation(gl.FUNC_ADD);
    gl.blendFunc(gl.ONE, gl.ONE);
    this.scissorBox(t, box);
    p.bind();
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, mask);
    gl.uniform1i(p.uniforms['uMask'], 0);
    gl.uniform2f(p.uniforms['uMaskTexel'], maskTexel[0], maskTexel[1]);
    gl.uniform4f(p.uniforms['uBox'], box.x, box.y, box.w, box.h);
    const k = this.lastDt;
    gl.uniform1f(p.uniforms['uSwirl'], swirl * k);
    gl.uniform1f(p.uniforms['uPush'], push * k);
    gl.uniform2f(p.uniforms['uTexel'], t.texel[0], t.texel[1]);
    drawQuad(gl, t);
    gl.disable(gl.SCISSOR_TEST);
    gl.disable(gl.BLEND);
  }

  /** Clears the rest field. Call once per frame before the addRestMask calls. */
  beginRestMask(): void {
    const gl = this.gl;
    gl.disable(gl.SCISSOR_TEST);
    gl.disable(gl.BLEND);
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.restT.fbo);
    gl.viewport(0, 0, this.restT.width, this.restT.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
  }

  /** MAX-blends the mask into the rest field: advection inside is slowed by `restStrength * rest`. */
  addRestMask(mask: WebGLTexture, box: Box): void {
    const gl = this.gl;
    const p = this.pRestMask;
    const t = this.restT;
    gl.enable(gl.BLEND);
    gl.blendEquation(gl.MAX);
    gl.blendFunc(gl.ONE, gl.ONE);
    this.scissorBox(t, box);
    p.bind();
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, mask);
    gl.uniform1i(p.uniforms['uMask'], 0);
    gl.uniform4f(p.uniforms['uBox'], box.x, box.y, box.w, box.h);
    gl.uniform2f(p.uniforms['uTexel'], t.texel[0], t.texel[1]);
    drawQuad(gl, t);
    gl.disable(gl.SCISSOR_TEST);
    gl.blendEquation(gl.FUNC_ADD);
    gl.disable(gl.BLEND);
  }

  /**
   * Adds the head silhouette (G-buffer blue channel > 0 inside) to the rest field with strength `k` (0..1):
   * ink slows down and lingers on the face, so the flow piles up around it. Call after the word masks, before step().
   */
  addRestFromHead(headTex: WebGLTexture, k: number): void {
    const gl = this.gl;
    const p = this.pRestHead;
    const t = this.restT;
    gl.enable(gl.BLEND);
    gl.blendEquation(gl.MAX);
    gl.blendFunc(gl.ONE, gl.ONE);
    p.bind();
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, headTex);
    gl.uniform1i(p.uniforms['uHead'], 0);
    gl.uniform1f(p.uniforms['uStrength'], Math.max(0, Math.min(1, k)));
    drawQuad(gl, t);
    gl.blendEquation(gl.FUNC_ADD);
    gl.disable(gl.BLEND);
  }

  // ------------------------------------------------------------------ step

  step(dtIn: number): void {
    const gl = this.gl;
    const dt = Math.min(Math.max(dtIn, 0), MAX_DT);
    if (dt <= 0) return;
    this.lastDt = dt;
    const prm = this.params;
    const v = this.vel;
    const texel = v.read.texel;

    gl.disable(gl.BLEND);
    gl.disable(gl.SCISSOR_TEST);

    // persistent large-scale flow
    this.flowTime += dt;
    if (prm.flow > 0) {
      const fp = this.pFlow;
      gl.enable(gl.BLEND);
      gl.blendEquation(gl.FUNC_ADD);
      gl.blendFunc(gl.ONE, gl.ONE);
      fp.bind();
      gl.uniform1f(fp.uniforms['uTime'], this.flowTime);
      gl.uniform1f(fp.uniforms['uAspect'], this.aspect);
      gl.uniform1f(fp.uniforms['uGain'], prm.flow * dt);
      drawQuad(gl, v.read);
      gl.disable(gl.BLEND);
    }

    // curl
    let p: Program = this.pCurl;
    p.bind();
    gl.uniform2f(p.uniforms['uTexel'], texel[0], texel[1]);
    gl.uniform1i(p.uniforms['uVelocity'], v.read.attach(0));
    drawQuad(gl, this.curlT);

    // vorticity confinement
    p = this.pVorticity;
    p.bind();
    gl.uniform2f(p.uniforms['uTexel'], texel[0], texel[1]);
    gl.uniform1i(p.uniforms['uVelocity'], v.read.attach(0));
    gl.uniform1i(p.uniforms['uCurl'], this.curlT.attach(1));
    gl.uniform1f(p.uniforms['uCurlStrength'], prm.curl);
    gl.uniform1f(p.uniforms['uDt'], dt);
    drawQuad(gl, v.write);
    v.swap();

    // divergence
    p = this.pDivergence;
    p.bind();
    gl.uniform2f(p.uniforms['uTexel'], texel[0], texel[1]);
    gl.uniform1i(p.uniforms['uVelocity'], v.read.attach(0));
    drawQuad(gl, this.divergenceT);

    // pressure decay (warm start)
    p = this.pClear;
    p.bind();
    gl.uniform2f(p.uniforms['uTexel'], texel[0], texel[1]);
    gl.uniform1i(p.uniforms['uTexture'], this.pressureT.read.attach(0));
    gl.uniform1f(p.uniforms['uValue'], prm.pressure);
    drawQuad(gl, this.pressureT.write);
    this.pressureT.swap();

    // Jacobi
    p = this.pPressure;
    p.bind();
    gl.uniform2f(p.uniforms['uTexel'], texel[0], texel[1]);
    gl.uniform1i(p.uniforms['uDivergence'], this.divergenceT.attach(0));
    for (let i = 0; i < prm.pressureIterations; i++) {
      gl.uniform1i(p.uniforms['uPressure'], this.pressureT.read.attach(1));
      drawQuad(gl, this.pressureT.write);
      this.pressureT.swap();
    }

    // gradient subtract
    p = this.pGradient;
    p.bind();
    gl.uniform2f(p.uniforms['uTexel'], texel[0], texel[1]);
    gl.uniform1i(p.uniforms['uPressure'], this.pressureT.read.attach(0));
    gl.uniform1i(p.uniforms['uVelocity'], v.read.attach(1));
    drawQuad(gl, v.write);
    v.swap();

    // advect velocity
    p = this.pAdvect;
    p.bind();
    gl.uniform2f(p.uniforms['uTexel'], texel[0], texel[1]);
    gl.uniform1i(p.uniforms['uRest'], this.restT.attach(2));
    gl.uniform1f(p.uniforms['uDt'], dt);
    gl.uniform1f(p.uniforms['uRestStrength'], prm.restStrength);
    gl.uniform1i(p.uniforms['uVelocity'], v.read.attach(0));
    gl.uniform1i(p.uniforms['uSource'], v.read.attach(1));
    gl.uniform1f(p.uniforms['uDissipation'], prm.velocityDissipation);
    gl.uniform1f(p.uniforms['uIsDye'], 0);
    gl.uniform1f(p.uniforms['uAgeing'], 0);
    drawQuad(gl, v.write);
    v.swap();

    // advect dye (velocity texel is the grid unit of the velocity field)
    gl.uniform1i(p.uniforms['uVelocity'], v.read.attach(0));
    gl.uniform1i(p.uniforms['uSource'], this.dyeT.read.attach(1));
    gl.uniform1f(p.uniforms['uDissipation'], prm.densityDissipation);
    gl.uniform1f(p.uniforms['uIsDye'], 1);
    gl.uniform1f(p.uniforms['uAgeing'], prm.ageing);
    gl.uniform1f(p.uniforms['uEmber'], prm.ember);
    gl.uniform1f(p.uniforms['uSeed'], (this.stepNo++ % 9973) * 0.37);
    drawQuad(gl, this.dyeT.write);
    this.dyeT.swap();
  }
}
