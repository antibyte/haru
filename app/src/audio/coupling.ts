import type { FluidSim } from '../fluid/sim';
import type { SmoothedAudio } from './features';

export type RGB = [number, number, number];

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));

/** Fluid curl strength driven by Haru's loudness. Low baseline keeps ink in silky filaments. */
export function vorticityFor(rms: number): number {
  return 3 + 4 * clamp(rms / 0.25, 0, 1);
}

/** Warm gold (low centroid) through red to magenta (high centroid), degrees. */
export function haruHue(c: number): number {
  return (42 - 82 * c + 360) % 360;
}

/** Ember: black-red to crimson to orange (hue 4..38 degrees), the daemon's warm ink. */
export function emberHue(c: number): number {
  return 4 + 34 * clamp(c, 0, 1);
}

/** Ink palette: `neutral` = gold / coral / magenta (angel, start screen), `ember` = the daemon's red-orange glow. */
export type InkPalette = 'neutral' | 'ember';

/** Cool cyan to blue, degrees. */
export function micHue(c: number): number {
  return 185 + 40 * c;
}

export function hsvToRgb(h: number, s: number, v: number): RGB {
  const hh = (((h % 360) + 360) % 360) / 60;
  const i = Math.floor(hh);
  const f = hh - i;
  const p = v * (1 - s);
  const q = v * (1 - s * f);
  const t = v * (1 - s * (1 - f));
  switch (i) {
    case 0: return [v, t, p];
    case 1: return [q, v, p];
    case 2: return [p, v, t];
    case 3: return [p, q, v];
    case 4: return [t, p, v];
    default: return [v, p, q];
  }
}

// ---------------------------------------------------------------------------------------------
// AudioCoupling: turns smoothed audio + app mode into fluid forcing (strokes of ink and velocity).
// ---------------------------------------------------------------------------------------------

export type CouplingMode = 'boot' | 'idle' | 'talking';

const BASE_FLOW = 9;
const BASE_ORBIT_POWER = 0.2;
const IDLE_AFTER_MS = 1500;
const STROKE_EVERY_MS = 1200;
const MIC_RATE_MAX = 14;
const TAU = Math.PI * 2;

interface Palette { stops: RGB[] }
const WARM: Palette = {
  stops: [[1.0, 0.62, 0.2], [1.0, 0.3, 0.45], [0.9, 0.2, 0.85], [1.0, 0.5, 0.18]],
};
const EMBER: Palette = {
  stops: [[0.55, 0.04, 0.02], [1.0, 0.18, 0.04], [1.0, 0.45, 0.08], [0.85, 0.1, 0.03]],
};
const COOL: Palette = {
  stops: [[0.2, 0.55, 1.0], [0.45, 0.3, 1.0], [0.15, 0.8, 0.95], [0.2, 0.55, 1.0]],
};

function sample(p: Palette, u: number): RGB {
  const n = p.stops.length - 1;
  const x = (((u % 1) + 1) % 1) * n;
  const i = Math.floor(x);
  const f = x - i;
  const s = f * f * (3 - 2 * f);
  const a = p.stops[i];
  const b = p.stops[i + 1];
  return [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, a[2] + (b[2] - a[2]) * s];
}

interface Orbiter { ax: number; ay: number; fx: number; fy: number; px: number; py: number; pal: Palette; hue: number; hueRate: number; phase: number }
interface Stroke {
  x: number; y: number; vx: number; vy: number; turn: number; life: number; age: number;
  color: RGB; power: number; radius: number; wobble: number; wobblePhase: number;
}

export class AudioCoupling {
  private readonly orbiters: Orbiter[];
  private strokes: Stroke[] = [];
  private t = 0;
  private strokeTimer = 0;
  private micAcc = 0;
  private quietMs = 0;
  private presence = 0; // 0 = talking with audio, 1 = ambient only
  private lastMode: CouplingMode = 'boot';
  private palette: InkPalette = 'neutral';
  private warm: Palette = WARM;

  constructor(private readonly sim: FluidSim, private readonly rng: () => number = Math.random) {
    this.orbiters = [
      { ax: 0.34, ay: 0.26, fx: 0.31, fy: 0.23, px: 0.0, py: 1.3, pal: WARM, hue: 0.0, hueRate: 0.012, phase: 0 },
      { ax: 0.30, ay: 0.30, fx: 0.19, fy: 0.37, px: 2.4, py: 0.4, pal: WARM, hue: 0.45, hueRate: 0.009, phase: 2.1 },
      { ax: 0.38, ay: 0.22, fx: 0.27, fy: 0.17, px: 4.1, py: 2.2, pal: COOL, hue: 0.2, hueRate: 0.015, phase: 4.2 },
    ];
  }

  /** Switches the warm ink (two orbiters, 72 % of the ambient strokes, Haru's word ink) between neutral and ember. */
  setPalette(p: InkPalette): void {
    this.palette = p;
    this.warm = p === 'ember' ? EMBER : WARM;
    // ember: the cool orbiter becomes a dark ember too, so nothing blue/teal is left in the ambient ink
    this.orbiters[2].pal = p === 'ember' ? EMBER : COOL;
    this.orbiters[2].hue = 0.2;
    this.orbiters[0].pal = this.warm;
    this.orbiters[1].pal = this.warm;
  }

  haruColor(a: SmoothedAudio): RGB {
    const c = hsvToRgb(this.palette === 'ember' ? emberHue(a.modelCentroid) : haruHue(a.modelCentroid), 0.85, 1);
    const k = 1.5 + 2 * a.modelRms;
    return [c[0] * k, c[1] * k, c[2] * k];
  }

  /** Unscaled hue colour (max channel 1) for word ink. */
  haruTint(a: SmoothedAudio): RGB {
    if (this.palette === 'ember') return hsvToRgb(emberHue(a.modelCentroid), 0.85, 1);
    return hsvToRgb(haruHue(a.modelCentroid * 0.55), 0.85, 1); // warm half of the range: gold to coral; ageing supplies magenta/violet
  }

  update(
    a: SmoothedAudio, dtMs: number, nowMs: number, wordCenter: { x: number; y: number } | null,
    mode: CouplingMode, downloadBps = 0,
  ): void {
    void nowMs;
    const dt = Math.min(dtMs, 50) / 1000;
    const fk = dt * 60; // emission constants are tuned per 60 Hz frame
    this.t += dt;
    const sim = this.sim;
    if (mode !== this.lastMode) {
      this.lastMode = mode;
      this.quietMs = 0;
    }

    // how much ambient ink flows: always something, much more when nothing else is happening
    const quiet = a.modelRms < 0.01 && a.micRms < 0.01;
    this.quietMs = quiet ? this.quietMs + dtMs : 0;
    const wantIdle = mode !== 'talking' || this.quietMs > IDLE_AFTER_MS;
    this.presence += ((wantIdle ? 1 : 0) - this.presence) * (1 - Math.exp(-dt / 0.9));
    const dl = mode === 'boot' ? 1 + clamp(downloadBps / 50e6, 0, 3) : 1;
    const ambient = (0.5 + 0.5 * this.presence) * dl;

    // global dynamics: audio raises turbulence, not blob size
    sim.params.curl = vorticityFor(a.modelRms);
    sim.params.flow = (BASE_FLOW + 2 * clamp(a.modelRms / 0.25, 0, 1)) * (1 + 0.35 * (dl - 1));

    // orbiting emitters (continuous ribbons)
    for (const o of this.orbiters) {
      const w = this.t;
      const x = 0.5 + o.ax * Math.sin(o.fx * w + o.px);
      const y = 0.5 + o.ay * Math.sin(o.fy * w + o.py);
      const dx = o.ax * o.fx * Math.cos(o.fx * w + o.px);
      const dy = o.ay * o.fy * Math.cos(o.fy * w + o.py);
      const len = Math.hypot(dx, dy) + 1e-6;
      o.hue += o.hueRate * dt;
      const breath = 0.55 + 0.45 * Math.sin(TAU * (this.t / 11) + o.phase);
      const col = sample(o.pal, o.hue);
      const p = BASE_ORBIT_POWER * breath * (0.6 + 0.4 * ambient) * fk;
      const cells = 26 * (0.7 + 0.3 * dl);
      sim.splat(x, y, (dx / len) * cells * fk, (dy / len) * cells * fk, [col[0] * p, col[1] * p, col[2] * p], 0.00012, 0.0016);
    }

    // transient strokes from the idle / boot timer
    this.strokeTimer -= dtMs * ambient;
    if (this.strokeTimer <= 0) {
      this.strokeTimer += STROKE_EVERY_MS;
      this.spawnAmbientStroke(dl);
    }

    // Haru speaking: tangential velocity around the word, weak colour
    if (a.modelRms > 0.03 && wordCenter) {
      const col = this.haruColor(a);
      for (let i = 0; i < 2; i++) {
        const ang = this.t * 1.6 + i * Math.PI; // coherent counter-clockwise orbit: a swirl, not noise
        const rx = 0.16;
        const ry = 0.07;
        const px = wordCenter.x + Math.cos(ang) * rx;
        const py = wordCenter.y + Math.sin(ang) * ry;
        const tx = -Math.sin(ang) * rx;
        const ty = Math.cos(ang) * ry;
        const tl = Math.hypot(tx, ty) + 1e-6;
        const sp = 45 * a.modelRms * 6;
        sim.splat(px, py, (tx / tl) * sp * fk, (ty / tl) * sp * fk, [col[0] * 0.015 * fk, col[1] * 0.015 * fk, col[2] * 0.015 * fk], 0.0003, 0.003);
      }
    }

    // user voice: cool streams rising from the lower edge
    if (a.micRms > 0.02) {
      this.micAcc += MIC_RATE_MAX * clamp(a.micRms / 0.2, 0, 1) * dt;
      while (this.micAcc >= 1) {
        this.micAcc -= 1;
        const ember = this.palette === 'ember';
        const c = hsvToRgb(micHue(a.micCentroid), ember ? 0.5 : 0.8, ember ? 0.7 : 0.9); // ember: darker, desaturated steel
        const k = (0.6 + 2 * a.micRms) * (ember ? 0.55 : 1);
        this.strokes.push({
          x: 0.1 + this.rng() * 0.8, y: 0.02,
          vx: (this.rng() - 0.5) * 18, vy: 70 + 90 * a.micRms * 4,
          turn: (this.rng() - 0.5) * 0.9, life: 1.5 + this.rng() * 0.8, age: 0,
          color: [c[0] * k, c[1] * k, c[2] * k], power: 0.08, radius: 0.0001,
          wobble: 0.5 + this.rng(), wobblePhase: this.rng() * TAU,
        });
      }
    }

    this.stepStrokes(dt, fk);
  }

  private spawnAmbientStroke(dl: number): void {
    const r = this.rng;
    const warm = r() < 0.72;
    const col = sample(warm || this.palette === 'ember' ? this.warm : COOL, r());
    const edge = r();
    // enter from the margin and travel across the frame
    const x = edge < 0.5 ? 0.05 + r() * 0.2 : 0.75 + r() * 0.2;
    const y = 0.15 + r() * 0.7;
    const ang = (edge < 0.5 ? 0 : Math.PI) + (r() - 0.5) * 1.2;
    const sp = (50 + r() * 50) * (0.8 + 0.2 * dl);
    this.strokes.push({
      x, y, vx: Math.cos(ang) * sp, vy: Math.sin(ang) * sp, turn: (r() - 0.5) * 0.7,
      life: 2.4 + r() * 1.6, age: 0, color: col, power: 0.22 * Math.min(1.5, dl), radius: 0.0001 + r() * 0.00012,
      wobble: 0.3 + r() * 0.6, wobblePhase: r() * TAU,
    });
  }

  private stepStrokes(dt: number, fk: number): void {
    const sim = this.sim;
    const vw = sim.velocity.width;
    const vh = sim.velocity.height;
    const keep: Stroke[] = [];
    for (const s of this.strokes) {
      s.age += dt;
      if (s.age >= s.life) continue;
      const u = s.age / s.life;
      const env = Math.sin(Math.PI * Math.min(1, u)) ** 0.8; // fade in and out along the stroke
      const ang = s.turn * dt + 0.9 * s.wobble * Math.sin(s.age * 2.2 + s.wobblePhase) * dt;
      const c = Math.cos(ang);
      const sn = Math.sin(ang);
      const nvx = s.vx * c - s.vy * sn;
      const nvy = s.vx * sn + s.vy * c;
      s.vx = nvx;
      s.vy = nvy;
      s.x += (s.vx * dt) / vw;
      s.y += (s.vy * dt) / vh;
      if (s.x < -0.1 || s.x > 1.1 || s.y < -0.1 || s.y > 1.1) continue;
      const p = s.power * env * fk;
      sim.splat(s.x, s.y, s.vx * 0.5 * fk, s.vy * 0.5 * fk, [s.color[0] * p, s.color[1] * p, s.color[2] * p], s.radius, 0.0008);
      keep.push(s);
    }
    this.strokes = keep;
  }
}
