import type { GLContext } from '../gl/context';
import { Program } from '../gl/program';
import { findCavityUv } from './cavity';
import type { GlbHead, Vec3 } from './glb';
import { HeadMesh } from './headmesh';
import { multiply, perspective, rotationX, rotationY, rotationZ, scaling, translation, transformPoint } from './mat4';

import headVert from './shaders/head.vert?raw';
import headFrag from './shaders/head.frag?raw';

const FOV = (30 * Math.PI) / 180;
/** The chin-to-crown height fills this fraction of the viewport height. */
const FACE_FILL = 0.8;
/** The face centre sits this fraction of the viewport height above the middle. */
const LIFT = 0.07;
const DEG = Math.PI / 180;

export interface HeadMotion {
  timeSec: number;
  /** extra pitch (rad, positive = chin down) and roll from the lipsync controller */
  nod: number;
  tilt: number;
  /** 0..1 speech activity: scales the micro-motion a little */
  speech: number;
  /** the close-up of the moment: camera distance factor and a shake offset in fractions of the visible height */
  closeup?: Closeup;
}

/** Reused every frame (no allocation): `zoom` scales the camera distance, `shakeX` / `shakeY` offset the view. */
export interface Closeup {
  zoom: number;
  shakeX: number;
  shakeY: number;
}

/**
 * Renders the head into a reduced-resolution G-buffer, attachment 0 (RGBA16F):
 *   r,g = view-space normal xy (z is reconstructed, front faces only)
 *   b   = 0.35 + 0.65 * nearness, 0 where there is no geometry (doubles as the soft silhouette / coverage)
 *   a   = mouth-cavity mask: textured heads = the exact inner-mouth pouch (1 on its pixels at any jaw opening, incl.
 *         the resting slit; see findCavityUv); porcelain heads = a soft blob proportional to jawOpen at the mouth
 * Attachment 1 (RGBA8) holds the albedo of textured heads (rgb linear, a = 1 on geometry; all 0 for porcelain heads).
 */
export class HeadPass {
  private readonly gl: WebGL2RenderingContext;
  private readonly prog: Program;
  private mesh: HeadMesh | null = null;
  private lm: Record<string, Vec3> | null = null;
  /** UV of the inner-mouth pouch (findCavityUv), null = none: no exact cavity mask */
  private cavityUv: [number, number] | null = null;
  private fbo: WebGLFramebuffer | null = null;
  private depth: WebGLRenderbuffer | null = null;
  private tex: WebGLTexture | null = null;
  private albTarget: WebGLTexture | null = null;
  private albedo: WebGLTexture | null = null;
  private w = 0;
  private h = 0;
  private empty = true;

  /** mouth position in normalized canvas coordinates (y up), valid after render() */
  readonly mouth = { x: 0.5, y: 0.35 };
  /** a few face points in normalized canvas coordinates (y up), valid after render(): brow, eyes, nose, cheeks, mouth, chin */
  readonly spots: Array<{ x: number; y: number }> = Array.from({ length: 9 }, () => ({ x: 0.5, y: 0.5 }));

  constructor(glc: GLContext) {
    this.gl = glc.gl;
    this.prog = new Program(this.gl, headVert, headFrag);
  }

  get texture(): WebGLTexture | null { return this.tex; }
  /** albedo attachment of the G-buffer (valid after render()) */
  get albedoTexture(): WebGLTexture | null { return this.albTarget; }
  /** true while a base-colour texture is set (the head is drawn textured) */
  get textured(): boolean { return this.albedo !== null; }
  get width(): number { return this.w; }
  get height(): number { return this.h; }
  get hasHead(): boolean { return this.mesh !== null; }
  get weights(): Float32Array | null { return this.mesh?.weights ?? null; }

  setHead(glb: GlbHead | null): void {
    this.mesh?.dispose();
    this.mesh = null;
    this.lm = null;
    this.cavityUv = null;
    this.empty = true;
    if (glb) {
      this.mesh = new HeadMesh(this.gl, glb);
      this.lm = glb.landmarks;
      this.cavityUv = findCavityUv(glb);
    }
  }

  /** Sets (or clears with null) the base-colour texture of the head. The pass takes ownership and deletes it. */
  setAlbedo(tex: WebGLTexture | null): void {
    if (this.albedo && this.albedo !== tex) this.gl.deleteTexture(this.albedo);
    this.albedo = tex;
  }

  private ensureTarget(w: number, h: number): void {
    w = Math.max(2, Math.round(w));
    h = Math.max(2, Math.round(h));
    if (this.fbo && w === this.w && h === this.h) return;
    const gl = this.gl;
    this.disposeTarget();
    this.tex = gl.createTexture()!;
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA16F, w, h);
    this.albTarget = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, this.albTarget);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA8, w, h);
    this.depth = gl.createRenderbuffer()!;
    gl.bindRenderbuffer(gl.RENDERBUFFER, this.depth);
    gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH_COMPONENT24, w, h);
    this.fbo = gl.createFramebuffer()!;
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.fbo);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, this.tex, 0);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT1, gl.TEXTURE_2D, this.albTarget, 0);
    gl.drawBuffers([gl.COLOR_ATTACHMENT0, gl.COLOR_ATTACHMENT1]); // clear() and the head draw both write the two targets
    gl.framebufferRenderbuffer(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, gl.RENDERBUFFER, this.depth);
    if (gl.checkFramebufferStatus(gl.FRAMEBUFFER) !== gl.FRAMEBUFFER_COMPLETE) {
      this.disposeTarget();
      throw new Error('head G-buffer framebuffer incomplete');
    }
    this.w = w;
    this.h = h;
    this.empty = false; // contents undefined until cleared by the next render
    this.clear();
  }

  private clear(): void {
    const gl = this.gl;
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.fbo);
    gl.viewport(0, 0, this.w, this.h);
    gl.clearColor(0, 0, 0, 0);
    gl.depthMask(true);
    gl.clearDepth(1);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    this.empty = true;
  }

  /** Clears the G-buffer once (no geometry anywhere), e.g. when the face is switched off. */
  blank(): void {
    if (this.fbo && !this.empty) this.clear();
  }

  /** Draws the head. `aspect` is the canvas aspect; w/h the G-buffer size. */
  render(w: number, h: number, aspect: number, m: HeadMotion): void {
    const mesh = this.mesh;
    const lm = this.lm;
    if (!mesh || !lm) return;
    const gl = this.gl;
    this.ensureTarget(w, h);

    const headH = Math.max(0.2, lm.crown[1] - lm.chin[1]);
    const cy = (lm.crown[1] + lm.chin[1]) / 2;
    const tanH = Math.tan(FOV / 2);
    let dist = headH / FACE_FILL / (2 * tanH);
    if (aspect < 0.8) dist *= 0.8 / aspect; // portrait: keep the face inside the width
    const close = m.closeup;
    if (close) dist *= close.zoom;
    const visH = 2 * dist * tanH;

    const t = m.timeSec;
    const yaw = close ? 0 : 8 * DEG * (0.62 * Math.sin((2 * Math.PI * t) / 17 + 0.3) + 0.38 * Math.sin((2 * Math.PI * t) / 9.3 + 1.7));
    const pitch = close ? 0 : 3 * DEG * Math.sin((2 * Math.PI * t) / 13 + 2.1) + m.nod;
    const roll = close ? 0 : 1.2 * DEG * Math.sin((2 * Math.PI * t) / 21 + 0.9) + m.tilt;
    const breath = close ? 0 : Math.sin((2 * Math.PI * t) / 4.6);
    const s = 1 + 0.007 * breath;
    const lift = visH * LIFT + 0.005 * breath;

    const pivot = lm.jawPivot;
    // model: view translation * breathing scale * rotation around the neck pivot
    let mv = translation(-pivot[0], -pivot[1], -pivot[2]);
    mv = multiply(multiply(rotationY(yaw), multiply(rotationX(pitch), rotationZ(roll))), mv);
    mv = multiply(translation(pivot[0], pivot[1], pivot[2]), mv);
    mv = multiply(scaling(s), mv);
    if (close) {
      // centre on the face between the eyes and the mouth; the shake moves the view by a fraction of the visible height
      const fy = (lm.eyeL[1] + lm.eyeR[1]) / 4 + lm.mouth[1] / 2 - 0.05;
      mv = multiply(translation(close.shakeX * visH, -fy + close.shakeY * visH, -dist), mv);
    } else {
      mv = multiply(translation(0, -cy + lift, -dist), mv);
    }
    const proj = perspective(FOV, aspect, Math.max(0.05, dist - 1.2), dist + 1.2);
    const mvp = multiply(proj, mv);

    // morph weights -> mouth mask + mouth screen position
    const jaw = mesh.weights[0];
    const mp: [number, number, number] = [lm.mouth[0], lm.mouth[1] - 0.025 * jaw, lm.mouth[2]];
    const c = transformPoint(mvp, mp);
    this.mouth.x = c[0] / c[3] * 0.5 + 0.5;
    this.mouth.y = c[1] / c[3] * 0.5 + 0.5;

    const lmv = lm;
    const eyeMid: Vec3 = [(lmv.eyeL[0] + lmv.eyeR[0]) / 2, (lmv.eyeL[1] + lmv.eyeR[1]) / 2 + 0.07, lmv.noseTip[2] * 0.9];
    const cheekL: Vec3 = [lmv.eyeL[0] * 1.15, (lmv.eyeL[1] + lmv.mouthL[1]) / 2, lmv.noseTip[2] * 0.8];
    const cheekR: Vec3 = [lmv.eyeR[0] * 1.15, cheekL[1], cheekL[2]];
    const pts: Vec3[] = [eyeMid, lmv.eyeL, lmv.eyeR, lmv.noseTip, cheekL, cheekR, mp, lmv.chin, [0, lmv.crown[1] - 0.1, lmv.noseTip[2] * 0.6]];
    pts.forEach((q, i) => {
      const cc = transformPoint(mvp, q);
      this.spots[i].x = (cc[0] / cc[3]) * 0.5 + 0.5;
      this.spots[i].y = (cc[1] / cc[3]) * 0.5 + 0.5;
    });

    const p = this.prog;
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.fbo);
    gl.viewport(0, 0, this.w, this.h);
    gl.clearColor(0, 0, 0, 0);
    gl.depthMask(true);
    gl.clearDepth(1);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.disable(gl.BLEND);
    gl.disable(gl.SCISSOR_TEST);
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.LEQUAL);
    gl.enable(gl.CULL_FACE);
    gl.cullFace(gl.BACK);

    p.bind();
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, mesh.morphTex);
    gl.uniform1i(p.uniforms['uMorph'], 0);
    // albedo on unit 1 (the morph texture stands in when there is none: never sampled, but keeps the unit valid)
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, this.albedo ?? mesh.morphTex);
    gl.uniform1i(p.uniforms['uTex'], 1);
    gl.uniform1f(p.uniforms['uHasTex'], this.albedo ? 1 : 0);
    gl.activeTexture(gl.TEXTURE0);
    gl.uniform1fv(p.uniforms['uW'], mesh.weights);
    gl.uniformMatrix4fv(p.uniforms['uMVP'], false, mvp);
    gl.uniformMatrix4fv(p.uniforms['uMV'], false, mv);
    gl.uniform2f(p.uniforms['uDepth'], dist, 0.75);
    gl.uniform3f(p.uniforms['uMouth'], lm.mouth[0], lm.mouth[1], lm.mouth[2]);
    gl.uniform1f(p.uniforms['uJaw'], jaw);
    gl.uniform2f(p.uniforms['uCavityUv'], this.cavityUv?.[0] ?? -1, this.cavityUv?.[1] ?? -1);
    mesh.draw();

    gl.bindVertexArray(null);
    gl.disable(gl.DEPTH_TEST);
    gl.disable(gl.CULL_FACE);
    this.empty = false;
  }

  private disposeTarget(): void {
    const gl = this.gl;
    if (this.fbo) gl.deleteFramebuffer(this.fbo);
    if (this.depth) gl.deleteRenderbuffer(this.depth);
    if (this.tex) gl.deleteTexture(this.tex);
    if (this.albTarget) gl.deleteTexture(this.albTarget);
    this.albTarget = null;
    this.fbo = null;
    this.depth = null;
    this.tex = null;
    this.w = this.h = 0;
  }

  dispose(): void {
    this.mesh?.dispose();
    this.mesh = null;
    this.setAlbedo(null);
    this.disposeTarget();
  }
}
