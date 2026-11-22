import type { GLContext } from '../gl/context';
import type { FluidSim } from '../fluid/sim';
import type { RGB } from '../fluid/params';
import type { SmoothedAudio } from '../audio/features';
import type { Persona } from '../ipc/types';
import { parseGlb } from './glb';
import { HeadPass, type Closeup, type HeadMotion } from './headpass';
import { LipsyncController, MORPH, type LipMode } from './lipsync';

export interface FaceFrame {
  frameMs: number;
  timeSec: number;
  audio: SmoothedAudio;
  mode: LipMode;
  /** model-first budget tier (0 = full, 3 = most frugal) */
  tier: number;
  /** fluid dye size in px (the G-buffer is a fraction of it) */
  dyeW: number;
  dyeH: number;
  aspect: number;
  lightPos: { x: number; y: number };
}

/** Inputs the composite shader needs while the face is visible. */
export interface FacePost {
  texture: WebGLTexture;
  /** presence 0..1 */
  amount: number;
  /** light direction in view space (normalized) */
  light: [number, number, number];
  texel: [number, number];
  focus: [number, number];
  size: number;
  /** per-head presence gain */
  gain: number;
  /** 1 = textured head: the composite uses `albedo` instead of porcelain (0 = exactly the porcelain look) */
  textured: number;
  /** albedo attachment of the G-buffer (only meaningful when textured) */
  albedo: WebGLTexture | null;
  /** upper bound of the face visibility for textured heads (porcelain keeps its own 0.28) */
  visMax: number;
  /** skin grade of the loaded head (only evaluated for textured heads) */
  grade: Readonly<HeadGrade>;
  /** moment only: eye positions in UV (y up): left x, y, right x, y */
  eyes?: [number, number, number, number];
}

/** Every runtime head: `app/public/heads/<key>.glb`. */
export const HEAD_KEYS = ['angel-female', 'angel-male', 'daemon'] as const;
export type HeadKey = (typeof HEAD_KEYS)[number];

/** Heads whose embedded base colour is used as albedo (the rest stays porcelain). */
export const TEXTURED_HEADS: ReadonlySet<HeadKey> = new Set<HeadKey>(['daemon', 'angel-female', 'angel-male']);
/** Per-head presence gain, tuned per asset. */
export const HEAD_GAIN: Readonly<Record<HeadKey, number>> = { daemon: 1.3, 'angel-female': 1.0, 'angel-male': 1.0 };

/** Per-head scale on the lipsync morph weights (index = MORPH order from lipsync.ts). Angels: the loudness-driven
 *  lipsync opens the jaw too far for a realistic face, so jaw / wide / round are damped. */
export const HEAD_MORPH_GAIN: Readonly<Record<HeadKey, readonly number[]>> = {
  daemon: [1, 1, 1, 1, 1, 1, 1, 1],
  'angel-female': [0.5, 0.7, 0.7, 1, 1, 1, 1, 1],
  'angel-male': [0.5, 0.7, 0.7, 1, 1, 1, 1, 1],
};
/** Visibility cap of textured heads in the composite (uHeadVisMax); porcelain heads stay at 0.28. */
export const HEAD_VIS_MAX = 0.42;

/**
 * Skin grade of a textured head, applied in composite.frag to the un-premultiplied albedo `a`
 * (uniforms uSkinGamma / uSkinLift / uSkinTint / uSkinSat):
 *   skin = pow(a, gamma) * lift
 *   skin = mix(lum(skin) * tint, skin, saturation)
 * Porcelain heads never evaluate it.
 */
export interface HeadGrade {
  /** exponent on the albedo (< 1 opens up the shadows) */
  gamma: number;
  /** linear gain after the gamma */
  lift: number;
  /** colour of the desaturated share (multiplies the luminance) */
  tint: readonly [number, number, number];
  /** 1 = the albedo's own colours, 0 = only the tinted luminance */
  saturation: number;
}

/** Identity grade: skin = albedo. */
export const NEUTRAL_GRADE: Readonly<HeadGrade> = Object.freeze({ gamma: 1, lift: 1, tint: [1, 1, 1] as const, saturation: 1 });

/** Per-head skin grade (tuned visually per asset). */
export const HEAD_GRADE: Readonly<Record<HeadKey, Readonly<HeadGrade>>> = {
  // dark VARCO creature skin (already x1.3 at build time) lifted so ink can reveal it, pulled toward red-grey: leathery, not human-pink
  daemon: { gamma: 0.9, lift: 1.3, tint: [1.1, 0.78, 0.7], saturation: 0.4 },
  // VARCO-baked angel skin is ~1.6-2x darker (linear) and more yellow than the reference; gamma 0.9 + lift 1.45 brings
  // the median skin back to the reference (~183/144/105 sRGB vs 190/137/107)
  'angel-female': { gamma: 0.9, lift: 1.45, tint: [1, 1, 1], saturation: 0.95 },
  'angel-male': { gamma: 0.9, lift: 1.45, tint: [1, 1, 1], saturation: 0.95 },
};

/** The head a persona shows: daemon its own head; angel (and the neutral start screen) the angel head of the voice's gender (unset = female). */
export function selectHead(persona: Persona | null, gender: 'female' | 'male' | null): HeadKey {
  if (persona === 'daemon') return 'daemon';
  return gender === 'male' ? 'angel-male' : 'angel-female';
}

/** Debug head override (`?debug&head=<key>`): pins the head regardless of persona and voice. */
export interface HeadOverride {
  key: HeadKey;
}

/** `<HeadKey>` pins that head (angel-female, angel-male, daemon). */
export function parseHeadOverride(value: string | null | undefined): HeadOverride | null {
  if (!value) return null;
  return (HEAD_KEYS as readonly string[]).includes(value) ? { key: value as HeadKey } : null;
}

const PRESENCE_OFF = 0.004;

/** Camera distance of the moment's close-up relative to the normal framing (the face fills the screen). */
export const MOMENT_ZOOM = 0.58;
/** Largest per-frame shake offset of the close-up, as a fraction of the visible height. */
export const MOMENT_SHAKE = 0.01;

/**
 * "Ein Gesicht im Nebel": owns the head asset, the lipsync controller and the G-buffer pass, and exposes what
 * the fluid (obstacle, mouth ink) and the composite (face shading) need. Fails soft: a missing GLB is logged once.
 */
export class FaceLayer {
  private readonly pass: HeadPass;
  private readonly lip = new LipsyncController();
  private enabled = true;
  private gender: 'female' | 'male' = 'female';
  private persona: Persona | null = null;
  private mode: LipMode = 'boot';
  private loaded: HeadKey | null = null;
  private loading: HeadKey | null = null;
  private readonly failed = new Set<HeadKey>();
  private pieces: string[] = [];
  private presence = 0;
  private speech = 0;
  private active = false;
  private frameNo = 0;
  private lastMist = -1;
  /** keep the head loadable while the face is switched off (a daemon conversation with a moment pending) */
  private keepReady = false;
  private momentShown = false;
  private readonly closeup: Closeup = { zoom: MOMENT_ZOOM, shakeX: 0, shakeY: 0 };
  private readonly momentMotion: HeadMotion = { timeSec: 0, nod: 0, tilt: 0, speech: 0, closeup: this.closeup };
  private post: FacePost = { texture: null as unknown as WebGLTexture, amount: 0, light: [0, 0, 1], texel: [1, 1], focus: [0.5, 0.5], size: 0.8, gain: 1, textured: 0, albedo: null, visMax: HEAD_VIS_MAX, grade: NEUTRAL_GRADE };

  private readonly gl: WebGL2RenderingContext;

  /**
   * `textured`: head keys whose embedded image is used as albedo (default TEXTURED_HEADS).
   * `forced`: debug override that pins the head regardless of persona and voice (null = selectHead()).
   */
  constructor(
    glc: GLContext,
    private readonly log: (msg: string) => void = (m) => console.warn(m),
    private readonly textured: ReadonlySet<HeadKey> = TEXTURED_HEADS,
    private readonly forced: HeadKey | null = null,
  ) {
    this.gl = glc.gl;
    this.pass = new HeadPass(glc);
  }

  setEnabled(on: boolean): void {
    this.enabled = on;
    this.maybeLoad();
  }

  /** Loads the head even while the face is switched off, so a moment can show it. */
  setKeepReady(on: boolean): void {
    this.keepReady = on;
    this.maybeLoad();
  }

  setGender(g: 'female' | 'male' | null): void {
    const next = g === 'male' ? 'male' : 'female';
    if (next === this.gender) return;
    this.gender = next;
    this.maybeLoad();
  }

  /** null / angel = textured angel head by voice gender (angel-female / angel-male); daemon = daemon.glb. */
  setPersona(p: Persona | null): void {
    if (p === this.persona) return;
    this.persona = p;
    this.maybeLoad();
  }

  /** The head that should be shown (a missing GLB simply means no face). */
  private get key(): HeadKey {
    return this.forced ?? selectHead(this.persona, this.gender);
  }

  /** The head currently loaded into the G-buffer pass (null = none yet); debug / capture only. */
  get head(): HeadKey | null {
    return this.loaded;
  }

  setMode(m: LipMode): void {
    this.mode = m;
    if (m !== 'talking') this.lip.reset();
    this.maybeLoad();
  }

  pushPieces(texts: readonly string[]): void {
    if (this.mode === 'talking') for (const t of texts) this.pieces.push(t);
  }

  private url(key: HeadKey): string {
    return `${import.meta.env.BASE_URL}heads/${key}.glb`;
  }

  /** Decodes the embedded base colour into an sRGB mipmapped texture; null (porcelain) on any failure. */
  private async decodeAlbedo(blob: Blob | null, key: string): Promise<WebGLTexture | null> {
    if (!blob) {
      this.log(`face: ${key} has no embedded base colour; rendering it as porcelain`);
      return null;
    }
    try {
      // glTF UV origin is top-left = first image row = texture t 0, so the image is uploaded unflipped.
      const bmp = await createImageBitmap(blob, { imageOrientation: 'none', premultiplyAlpha: 'none', colorSpaceConversion: 'none' });
      const gl = this.gl;
      const tex = gl.createTexture()!;
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
      const levels = Math.floor(Math.log2(Math.max(bmp.width, bmp.height))) + 1;
      gl.texStorage2D(gl.TEXTURE_2D, levels, gl.SRGB8_ALPHA8, bmp.width, bmp.height);
      gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, gl.RGBA, gl.UNSIGNED_BYTE, bmp);
      gl.generateMipmap(gl.TEXTURE_2D);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);
      bmp.close();
      return tex;
    } catch (e) {
      this.log(`face: albedo of ${key} could not be decoded (${(e as Error).message}); rendering it as porcelain`);
      return null;
    }
  }

  /** Loads lazily: only once the conversation starts (and again when the head changes). */
  private maybeLoad(): void {
    if ((!this.enabled && !this.keepReady) || this.mode !== 'talking') return;
    const key = this.key;
    if (this.loaded === key || this.loading === key || this.failed.has(key)) return;
    const url = this.url(key);
    this.loading = key;
    void (async () => {
      try {
        const res = await fetch(url);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const glb = parseGlb(await res.arrayBuffer());
        const albedo = this.textured.has(key) ? await this.decodeAlbedo(glb.image, key) : null;
        if (this.loading !== key) { // superseded by a head change
          if (albedo) this.gl.deleteTexture(albedo);
          return;
        }
        this.pass.setHead(glb);
        this.pass.setAlbedo(albedo);
        this.loaded = key;
      } catch (e) {
        this.failed.add(key);
        this.log(`face: ${url} could not be loaded (${(e as Error).message}); rendering without a face`);
      } finally {
        if (this.loading === key) this.loading = null;
        // the head changed meanwhile (maybeLoad ignores loaded / failed heads)
        if (this.loading === null && this.loaded !== this.key) this.maybeLoad();
      }
    })();
  }

  /** Advances the lipsync, renders the G-buffer when the face is (nearly) visible. Call before the fluid step. */
  update(f: FaceFrame): void {
    const out = this.lip.update({
      dtMs: f.frameMs, mode: f.mode, modelRms: f.audio.modelRms, micRms: f.audio.micRms, pieces: this.pieces,
    });
    this.pieces.length = 0;
    this.presence = out.presence;
    this.speech = out.speech;

    const w = this.pass.weights;
    const usable = this.enabled && this.pass.hasHead && f.tier < 3;
    if (!usable || out.presence < PRESENCE_OFF) {
      if (this.active) this.pass.blank();
      this.active = false;
      return;
    }
    if (w) {
      w.set(out.weights);
      // damp per head; the pass (mouth mask / position / cavity) reads these weights, so it sees the damped value too
      const mg = HEAD_MORPH_GAIN[this.loaded ?? this.key];
      for (let i = 0; i < mg.length; i++) w[i] = Math.min(1, Math.max(0, w[i]! * mg[i]!));
    }
    const frugal = f.tier >= 2;
    this.frameNo++;
    if (!frugal || this.frameNo % 2 === 0 || !this.active) {
      const scale = frugal ? 0.33 : 0.5;
      this.pass.render(f.dyeW * scale, f.dyeH * scale, f.aspect, { timeSec: f.timeSec, nod: out.nod, tilt: out.tilt, speech: out.speech });
    }
    this.active = true;

    const lx = -0.3 + (f.lightPos.x - 0.5) * 1.6;
    const ly = 0.45 + (f.lightPos.y - 0.5) * 1.2;
    const lz = 0.8;
    const n = Math.hypot(lx, ly, lz);
    // face centre = mean of the eye / nose / mouth spots; size = chin-to-crown height (UV y)
    const sp = this.pass.spots;
    const fx = (sp[1].x + sp[2].x + sp[3].x + sp[6].x) / 4;
    const fy = (sp[1].y + sp[2].y + sp[3].y + sp[6].y) / 4;
    const size = Math.max(0.2, sp[8].y - sp[7].y + 0.1);
    const head = this.loaded ?? this.key; // tuning of the head actually in the pass (it lags a head change until loaded)
    this.post = {
      texture: this.pass.texture!, amount: out.presence, light: [lx / n, ly / n, lz / n],
      texel: [1 / this.pass.width, 1 / this.pass.height], focus: [fx, fy], size, gain: HEAD_GAIN[head],
      textured: this.pass.textured ? 1 : 0, albedo: this.pass.albedoTexture, visMax: HEAD_VIS_MAX, grade: HEAD_GRADE[head],
    };
  }

  /** True when the daemon head is loaded, i.e. a moment can show a picture. */
  get momentReady(): boolean {
    return this.pass.hasHead && this.loaded === 'daemon';
  }

  /**
   * One frame of the moment: the daemon head in close-up, jaw wide open, brows down, a new small shake offset.
   * `w` / `h` = G-buffer size. Returns the composite inputs, or null when the daemon head is not loaded.
   */
  renderMoment(w: number, h: number, aspect: number, timeSec: number): FacePost | null {
    if (!this.momentReady) return null;
    const wt = this.pass.weights;
    if (wt) {
      wt.fill(0);
      wt[MORPH.jawOpen] = 1; // fully open, bypassing HEAD_MORPH_GAIN
      wt[MORPH.lipsWide] = 0.5;
      wt[MORPH.browFrown] = 1;
    }
    this.closeup.shakeX = (Math.random() * 2 - 1) * MOMENT_SHAKE;
    this.closeup.shakeY = (Math.random() * 2 - 1) * MOMENT_SHAKE;
    this.momentMotion.timeSec = timeSec;
    this.pass.render(w, h, aspect, this.momentMotion);
    this.momentShown = true;
    this.active = false; // the normal frame renders its own G-buffer again
    const p = this.post;
    p.texture = this.pass.texture!;
    p.albedo = this.pass.albedoTexture;
    p.textured = this.pass.textured ? 1 : 0;
    p.grade = HEAD_GRADE.daemon;
    p.amount = 1;
    const sp = this.pass.spots;
    const eyes = (p.eyes ??= [0, 0, 0, 0]);
    eyes[0] = sp[1].x; eyes[1] = sp[1].y; eyes[2] = sp[2].x; eyes[3] = sp[2].y;
    return p;
  }

  /** Hard cut back: the close-up must not linger in the G-buffer. */
  endMoment(): void {
    if (!this.momentShown) return;
    this.momentShown = false;
    this.pass.blank();
  }

  /** Composite inputs, or null when the face is not drawn. */
  get postInputs(): FacePost | null {
    return this.active ? this.post : null;
  }

  /** Fluid obstacle (silhouette, via the rest field) while the face is drawn. */
  applyObstacle(sim: FluidSim): void {
    if (this.active && this.pass.texture) sim.addRestFromHead(this.pass.texture, Math.min(1, this.presence * 2));
  }

  /** "Breath of words": warm ink and a gentle velocity jet from the mouth, proportional to Haru's loudness. */
  emitMouth(sim: FluidSim, rms: number, timeSec: number, fk: number, tint: RGB): void {
    if (!this.active || this.presence < 0.15 || this.mode !== 'talking') return;
    const level = Math.min(1, rms / 0.18);
    if (level < 0.05) return;
    const m = this.pass.mouth;
    const k = 0.018 * level * fk;
    const wob = Math.sin(timeSec * 2.7) * 7 + Math.sin(timeSec * 5.1) * 3;
    const vy = -(10 + 26 * level);
    sim.splat(m.x, m.y - 0.012, wob * fk, vy * fk, [tint[0] * k, tint[1] * k, tint[2] * k], 0.00018, 0.0011);
  }

  /**
   * Slow drifting mist: tiny cool/warm ink puffs seeded on cheeks, brow, nose and chin while Haru speaks, so ink keeps
   * flowing over the face (it becomes visible only where ink is). Proportional to presence, far below the words.
   */
  seedMist(sim: FluidSim, timeSec: number, fk: number, warm: RGB): void {
    if (!this.active || this.mode !== 'talking') return;
    const a = this.presence;
    if (a < 0.1) return;
    const n = this.pass.spots.length;
    const k = Math.floor(timeSec * 3.2);
    if (k === this.lastMist) return; // ~3 puffs per second
    this.lastMist = k;
    const s = this.pass.spots[(k * 5 + 2) % n];
    const cool = ((k * 7) % 3) === 0;
    const c: RGB = cool ? [0.25, 0.5, 1.0] : warm;
    const amt = 0.06 * a * Math.max(1, fk);
    const ang = k * 2.399;
    sim.splat(s.x + Math.cos(ang) * 0.012, s.y + Math.sin(ang) * 0.02, Math.cos(ang) * 14, 6 + Math.sin(ang) * 10, [c[0] * amt, c[1] * amt, c[2] * amt], 0.0011, 0.0022);
  }

  dispose(): void {
    this.pass.dispose();
  }
}
