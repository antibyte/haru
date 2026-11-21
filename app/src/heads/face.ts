import type { GLContext } from '../gl/context';
import type { FluidSim } from '../fluid/sim';
import type { RGB } from '../fluid/params';
import type { SmoothedAudio } from '../audio/features';
import { parseGlb } from './glb';
import { HeadPass } from './headpass';
import { LipsyncController, type LipMode } from './lipsync';

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
}

const PRESENCE_OFF = 0.004;

/**
 * "Ein Gesicht im Nebel": owns the head asset, the lipsync controller and the G-buffer pass, and exposes what
 * the fluid (obstacle, mouth ink) and the composite (face shading) need. Fails soft: a missing GLB is logged once.
 */
export class FaceLayer {
  private readonly pass: HeadPass;
  private readonly lip = new LipsyncController();
  private enabled = true;
  private gender: 'female' | 'male' = 'female';
  private mode: LipMode = 'boot';
  private loaded: string | null = null;
  private loading: string | null = null;
  private readonly failed = new Set<string>();
  private pieces: string[] = [];
  private presence = 0;
  private speech = 0;
  private active = false;
  private frameNo = 0;
  private lastMist = -1;
  private post: FacePost = { texture: null as unknown as WebGLTexture, amount: 0, light: [0, 0, 1], texel: [1, 1], focus: [0.5, 0.5], size: 0.8, gain: 1 };

  constructor(glc: GLContext, private readonly log: (msg: string) => void = (m) => console.warn(m)) {
    this.pass = new HeadPass(glc);
  }

  setEnabled(on: boolean): void {
    this.enabled = on;
    this.maybeLoad();
  }

  setGender(g: 'female' | 'male' | null): void {
    const next = g === 'male' ? 'male' : 'female';
    if (next === this.gender) return;
    this.gender = next;
    this.maybeLoad();
  }

  setMode(m: LipMode): void {
    this.mode = m;
    if (m !== 'talking') this.lip.reset();
    this.maybeLoad();
  }

  pushPieces(texts: readonly string[]): void {
    if (this.mode === 'talking') for (const t of texts) this.pieces.push(t);
  }

  private url(): string {
    return `${import.meta.env.BASE_URL}heads/${this.gender}.glb`;
  }

  /** Loads lazily: only once the conversation starts (and again when the gender changes). */
  private maybeLoad(): void {
    if (!this.enabled || this.mode !== 'talking') return;
    const url = this.url();
    if (this.loaded === url || this.loading === url || this.failed.has(url)) return;
    this.loading = url;
    void (async () => {
      try {
        const res = await fetch(url);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const glb = parseGlb(await res.arrayBuffer());
        if (this.loading !== url) return; // superseded by a gender change
        this.pass.setHead(glb);
        this.loaded = url;
      } catch (e) {
        this.failed.add(url);
        this.log(`face: ${url} could not be loaded (${(e as Error).message}); rendering without a face`);
      } finally {
        if (this.loading === url) this.loading = null;
        if (this.loaded !== null && this.loaded !== this.url()) this.maybeLoad(); // gender changed meanwhile
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
    w?.set(out.weights);
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
    this.post = {
      texture: this.pass.texture!, amount: out.presence, light: [lx / n, ly / n, lz / n],
      texel: [1 / this.pass.width, 1 / this.pass.height], focus: [fx, fy], size, gain: this.gender === 'male' ? 1.0 : 1.3,
    };
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
