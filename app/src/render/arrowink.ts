import type { FluidSim } from '../fluid/sim';
import type { Box, RGB } from '../fluid/params';
import { arrowEnvelope, arrowPolygon, type ArrowGeom, type ArrowPair } from '../ui/arrows';

/** Dye per 60 Hz frame while an arrow builds up, and while it is held (keeps it readable against dissipation). */
const BUILD_GAIN = 0.05;
const HOLD_GAIN = 0.012;
/** Velocity (grid cells/s) added per 60 Hz frame along the arrow, at three points of the shaft. */
const PUSH = 24;
/** Edge swirl and outward push while an arrow dissolves (like the words' unravelling). */
const SWIRL = 60;
const SPREAD = 28;
/** The mask is rasterized at this fraction of the arrow's CSS size; the dye field is coarser anyway. */
const MASK_SCALE = 0.5;
const SOFT = 4;
const GOLD: RGB = [1.0, 0.74, 0.3];
const RED: RGB = [1.0, 0.16, 0.05];
const NO_INK: RGB = [0, 0, 0];
const ALONG = [0.2, 0.5, 0.8] as const;

interface ArrowMask { tex: WebGLTexture; texel: [number, number]; box: Box; dir: [number, number]; points: [number, number][]; color: RGB }

/** The chooser's hover arrows as ink: an arrow-shaped mask splatted into the dye, pushed along its direction. */
export class ArrowInk {
  private masks: ArrowMask[] = [];
  private active = false;
  private env = 0;
  private lastNow = 0;
  private canvas: OffscreenCanvas | null = null;

  constructor(private readonly gl: WebGL2RenderingContext, private readonly sim: FluidSim) {}

  /** New arrows (they build up), or null (the current ones dissolve). */
  set(pair: ArrowPair | null): void {
    if (!pair || typeof OffscreenCanvas === 'undefined') { this.active = false; return; }
    this.release();
    const masks = [this.build(pair.angel, pair, GOLD), this.build(pair.daemon, pair, RED)];
    if (masks.some((m) => !m)) { for (const m of masks) if (m) this.gl.deleteTexture(m.tex); this.active = false; return; }
    this.masks = masks as ArrowMask[];
    this.active = true;
  }

  /** Every frame, after the word injector (it clears the rest field). */
  update(nowMs: number): void {
    const dt = this.lastNow > 0 ? Math.min(100, Math.max(0, nowMs - this.lastNow)) : 1000 / 60;
    this.lastNow = nowMs;
    this.env = arrowEnvelope(this.env, this.active, dt);
    if (this.env <= 0 || this.masks.length === 0) return;
    const fk = dt / (1000 / 60);
    for (const m of this.masks) {
      if (this.active) {
        this.sim.maskSplatDye(m.tex, m.box, m.color, (this.env < 1 ? BUILD_GAIN : HOLD_GAIN) * fk);
        if (this.env >= 1) this.sim.addRestMask(m.tex, m.box);
      } else {
        this.sim.edgeSplatVelocity(m.tex, m.texel, m.box, SWIRL * this.env, SPREAD * this.env);
      }
      const v = PUSH * this.env * fk;
      for (const [x, y] of m.points) this.sim.splat(x, y, m.dir[0] * v, m.dir[1] * v, NO_INK, 0.0002, 0.0008);
    }
  }

  dispose(): void { this.release(); }

  private release(): void {
    for (const m of this.masks) this.gl.deleteTexture(m.tex);
    this.masks = [];
  }

  private build(a: ArrowGeom, pair: ArrowPair, color: RGB): ArrowMask | null {
    const poly = arrowPolygon(a.from, a.to, a.width);
    let x0 = Infinity; let y0 = Infinity; let x1 = -Infinity; let y1 = -Infinity;
    for (const [x, y] of poly) { x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y); }
    const pad = Math.max(4, a.width);
    x0 -= pad; y0 -= pad; x1 += pad; y1 += pad;
    const bw = x1 - x0;
    const bh = y1 - y0;
    const w = Math.max(8, Math.ceil(bw * MASK_SCALE));
    const h = Math.max(8, Math.ceil(bh * MASK_SCALE));
    const c = (this.canvas ??= new OffscreenCanvas(8, 8));
    c.width = w;
    c.height = h;
    const ctx = c.getContext('2d', { willReadFrequently: true });
    if (!ctx) { this.active = false; return null; }
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = '#000';
    ctx.fillRect(0, 0, w, h);
    ctx.setTransform(w / bw, 0, 0, h / bh, (-x0 * w) / bw, (-y0 * h) / bh);
    ctx.shadowColor = 'rgba(255,255,255,0.9)';
    ctx.shadowBlur = SOFT;
    ctx.fillStyle = '#fff';
    ctx.beginPath();
    poly.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.closePath();
    ctx.fill();
    const img = ctx.getImageData(0, 0, w, h).data;
    const r = new Uint8Array(w * h);
    for (let i = 0; i < w * h; i++) r[i] = img[i * 4];
    const gl = this.gl;
    const tex = gl.createTexture()!;
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, w, h, 0, gl.RED, gl.UNSIGNED_BYTE, r);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    const W = pair.canvasW;
    const H = pair.canvasH;
    const dx = a.to[0] - a.from[0];
    const dy = a.to[1] - a.from[1];
    const len = Math.hypot(dx, dy) || 1;
    return {
      tex, texel: [1 / w, 1 / h], color,
      box: { x: (x0 + bw / 2) / W, y: 1 - (y0 + bh / 2) / H, w: bw / W, h: bh / H },
      dir: [dx / len, -dy / len], // the sim's y points up
      points: ALONG.map((t): [number, number] => [(a.from[0] + dx * t) / W, 1 - (a.from[1] + dy * t) / H]),
    };
  }
}
