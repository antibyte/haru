import { createGL, type GLContext } from '../gl/context';
import { Program } from '../gl/program';
import { drawQuad } from '../gl/quad';
import { FluidSim } from '../fluid/sim';
import type { Box, RGB } from '../fluid/params';
import { PostFX } from '../post/pipeline';
import { QUALITY_TABLE, QualityController, type QualityParams } from './quality';
import { BUDGET_TIERS, ModelFirstController, minQuality, type PostTier, type RenderBudget } from './modelfirst';
import { AudioCoupling, type CouplingMode } from '../audio/coupling';
import { FeatureSmoother, type SmoothedAudio } from '../audio/features';
import { GlyphRasterizer } from '../words/glyphs';
import { WordInjector } from '../words/injector';
import { WordPath } from '../words/path';
import { WordAssembler, type WordEvent } from '../words/assembler';
import { FaceLayer, TEXTURED_HEADS, parseHeadOverride } from '../heads/face';
import type { Persona } from '../ipc/types';
import type { Quality, QualityLevel, VizFrame } from '../ipc/types';

import baseVert from '../fluid/shaders/base.vert?raw';
import displayFrag from '../fluid/shaders/display.frag?raw';

const START_LEVEL: QualityLevel = 'high';
const DEFAULT_LIGHT = { x: 0.5, y: 0.6 };
const RESIZE_DEBOUNCE_MS = 140;
/** Tolerance so a 30 fps cap still renders every 2nd frame of a 60 Hz display despite rAF jitter. */
const CAP_SLACK_MS = 3;

function hsv(h: number, s: number, v: number): RGB {
  const i = Math.floor(h * 6);
  const f = h * 6 - i;
  const p = v * (1 - s);
  const q = v * (1 - f * s);
  const t = v * (1 - (1 - f) * s);
  switch (((i % 6) + 6) % 6) {
    case 0: return [v, t, p];
    case 1: return [q, v, p];
    case 2: return [p, v, t];
    case 3: return [p, q, v];
    case 4: return [t, p, v];
    default: return [v, p, q];
  }
}

/** Post chain cost per model-first tier; 'full' leaves the quality level's own values untouched. */
function postParams(q: QualityParams, tier: PostTier): QualityParams {
  switch (tier) {
    case 'full': return q;
    case 'lite': return { ...q, bloomLevels: Math.min(q.bloomLevels, 3), raySamples: Math.min(q.raySamples, 24), rayScale: Math.min(q.rayScale, 0.25) };
    case 'minimal': return { ...q, bloomLevels: 2, raySamples: 8, rayScale: 0.25 };
  }
}

export class Renderer {
  readonly glc: GLContext;
  readonly sim: FluidSim;
  readonly post: PostFX;

  /** Normalized light position for god rays (y up). T2.4 sets this to the active word centre. */
  lightPos = { x: 0.5, y: 0.6 };
  /** Smoothed model RMS (0..1) used for chromatic aberration. T2.6 drives it. */
  caRms = 0;

  private readonly canvas: HTMLCanvasElement;
  private readonly params = new URLSearchParams(location.search);
  private readonly debug: boolean;
  private readonly noPost: boolean;
  /** Debug-only (?fpscap=60): emulates a 60 Hz display when the browser runs rAF unthrottled. */
  private readonly fpsCap: number;
  /** Debug-only: ?quality=<level> pins the level; settings and auto must not override it. */
  private readonly forcedLevel: QualityLevel | null;
  /** ?modelfirst=off (debug) disables the model-first render policy for A/B comparisons. */
  private readonly modelFirstOff: boolean;
  /** ?debug&facedebug shows the whole face (no ink gate / dissolve) to inspect shading and lipsync. */
  private readonly faceDebug: boolean;
  /** ?debug&momentshow keeps the close-up on screen (capture / shading work); the HUD is hidden like in the real thing. */
  private readonly momentShow: boolean;
  /** ms of the close-up still to show (0 = normal frames); Infinity with ?momentshow */
  private momentLeft = 0;
  private readonly modelFirst = new ModelFirstController();
  private budget: RenderBudget = BUDGET_TIERS[0];
  private latencyMs: number | null = null;
  private readonly pDisplay: Program;
  private raf = 0;
  private running = false;
  private lastNow = 0;
  private timeSec = 0;
  private emberAmt = 0;
  private resizeAt = 0;
  private fpsEl: HTMLDivElement | null = null;
  private frameEma = 16.7;
  private fpsFrames = 0;
  private fpsT0 = 0;
  private fpsValue = 0;
  private level: QualityLevel = START_LEVEL;
  private readonly quality: QualityController;
  private readonly smoother = new FeatureSmoother();
  private readonly coupling: AudioCoupling;
  private readonly glyphs: GlyphRasterizer;
  private readonly injector: WordInjector;
  private readonly assembler = new WordAssembler();
  private readonly face: FaceLayer;
  private readonly wordCbs: Array<(e: WordEvent) => void> = [];
  private readonly path = new WordPath({ candidates: 48 });
  private mode: CouplingMode = 'boot';
  private downloadBps = 0;
  private pendingViz: VizFrame | null = null;
  private lastViz: VizFrame | null = null;
  private audio: SmoothedAudio = { modelRms: 0, modelCentroid: 0.5, micRms: 0, micCentroid: 0.5 };
  private readonly disposers: Array<() => void> = [];

  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;
    this.debug = this.params.has('debug');
    this.noPost = this.params.has('nopost');
    this.fpsCap = this.debug ? Number(this.params.get('fpscap') ?? 0) || 0 : 0;
    this.applyCanvasSize();
    this.glc = createGL(canvas);
    const qp = this.params.get('quality');
    const forced = this.debug && qp && qp in QUALITY_TABLE ? (qp as QualityLevel) : null;
    this.forcedLevel = forced;
    this.faceDebug = this.debug && this.params.has('facedebug');
    this.modelFirstOff = this.debug && this.params.get('modelfirst') === 'off';
    this.momentShow = this.debug && this.params.has('momentshow');
    this.quality = new QualityController(forced ?? 'auto', START_LEVEL);
    this.level = this.quality.level;
    const q = QUALITY_TABLE[this.level];
    this.sim = new FluidSim(this.glc, q.sim, q.dye);
    this.sim.params.pressureIterations = q.pressureIterations;
    this.post = new PostFX(this.glc, q);
    this.pDisplay = new Program(this.glc.gl, baseVert, displayFrag);
    this.coupling = new AudioCoupling(this.sim);
    this.glyphs = new GlyphRasterizer(this.glc.gl);
    this.injector = new WordInjector(this.sim, this.glyphs, this.path);
    // debug only: ?head=<angel-female|angel-male|daemon> pins a head
    const head = this.debug ? parseHeadOverride(this.params.get('head')) : null;
    this.face = new FaceLayer(this.glc, undefined, TEXTURED_HEADS, head?.key ?? null);
    if (this.debug) this.setupDebug();
    if (this.momentShow) {
      this.momentLeft = Infinity;
      document.documentElement.classList.add('is-moment');
    }
  }

  start(): void {
    if (this.running) return;
    this.running = true;
    void this.glyphs.ready();
    this.lastNow = performance.now();
    this.fpsT0 = this.lastNow;
    const loop = (now: number): void => {
      if (!this.running) return;
      const cap = this.effectiveFpsCap();
      if (cap > 0 && now - this.lastNow < 1000 / cap - (this.budget.fpsCap ? CAP_SLACK_MS : 1)) {
        this.raf = requestAnimationFrame(loop);
        return;
      }
      this.frame(now);
      this.raf = requestAnimationFrame(loop);
    };
    this.raf = requestAnimationFrame(loop);
  }

  stop(): void {
    this.running = false;
    cancelAnimationFrame(this.raf);
    for (const d of this.disposers.splice(0)) d();
  }

  // ------------------------------------------------------------------ frame

  private effectiveFpsCap(): number {
    const caps = [this.fpsCap, this.budget.fpsCap ?? 0].filter((c) => c > 0);
    return caps.length ? Math.min(...caps) : 0;
  }

  private applyCanvasSize(): boolean {
    const dpr = Math.min(window.devicePixelRatio || 1, 2) * this.budget.renderScale;
    const w = Math.max(1, Math.floor(this.canvas.clientWidth * dpr));
    const h = Math.max(1, Math.floor(this.canvas.clientHeight * dpr));
    if (w === this.canvas.width && h === this.canvas.height) return false;
    this.canvas.width = w;
    this.canvas.height = h;
    return true;
  }

  private frame(now: number): void {
    const frameMs = now - this.lastNow;
    // a capped frame rate has longer frames by design; the sim step must not be clamped to slow motion
    const dt = Math.min(Math.max(frameMs / 1000, 0), this.effectiveFpsCap() > 0 && this.effectiveFpsCap() < 50 ? 1 / 20 : 1 / 30);
    this.lastNow = now;
    this.timeSec += frameMs / 1000;
    this.frameEma += (Math.min(frameMs, 100) - this.frameEma) * 0.05;

    const budgetChanged = this.updateBudget(now);
    if (budgetChanged) {
      // render scale / post tier / level changed: reallocate once, right now
      this.applyCanvasSize();
      this.resizeAt = 0;
      this.applyLevel(this.effectiveLevel(), true);
    } else if (this.applyCanvasSize()) this.resizeAt = now + RESIZE_DEBOUNCE_MS;
    if (this.resizeAt && now >= this.resizeAt) {
      this.resizeAt = 0;
      this.resizeAll();
    }

    if (this.momentLeft > 0 && this.drawMoment(frameMs, now)) return;

    const f = this.pendingViz;
    this.pendingViz = null;
    this.audio = this.smoother.update(this.mode === 'talking' ? f : null, frameMs);
    const center = this.injector.activeCenter();
    this.coupling.update(this.audio, frameMs, now, center, this.mode, this.downloadBps);
    this.injector.update(now, this.coupling.haruTint(this.audio));
    const target = center ?? DEFAULT_LIGHT;
    const k = 1 - Math.exp(-dt / 0.25);
    this.lightPos.x += (target.x - this.lightPos.x) * k;
    this.lightPos.y += (target.y - this.lightPos.y) * k;
    this.caRms += (this.audio.modelRms - this.caRms) * 0.2;

    // the face: lipsync + G-buffer first, so the fluid step sees this frame's silhouette
    this.face.update({
      frameMs, timeSec: this.timeSec, audio: this.audio, mode: this.mode, tier: this.budget.tier,
      dyeW: this.sim.dye.width, dyeH: this.sim.dye.height, aspect: this.canvas.width / Math.max(1, this.canvas.height),
      lightPos: this.lightPos,
    });
    this.face.applyObstacle(this.sim);
    this.face.emitMouth(this.sim, this.audio.modelRms, this.timeSec, Math.min(3, frameMs / (1000 / 60)), this.coupling.haruTint(this.audio));

    this.face.seedMist(this.sim, this.timeSec, Math.min(3, frameMs / (1000 / 60)), this.coupling.haruTint(this.audio));
    this.sim.step(dt);
    this.drawScreen();
    this.updateQuality(frameMs, now);
    this.updateFps(now);
  }

  /** One close-up frame (hard cut, nothing else is simulated or drawn). False = no picture possible, draw normally. */
  private drawMoment(frameMs: number, now: number): boolean {
    const post = this.face.renderMoment(this.canvas.width, this.canvas.height, this.canvas.width / Math.max(1, this.canvas.height), this.timeSec);
    if (!post) {
      if (!this.momentShow) this.momentLeft = 0; // the head is gone: give up (?momentshow waits for it)
      return false;
    }
    this.post.renderMoment(post, this.timeSec);
    this.momentLeft -= frameMs;
    if (this.momentLeft <= 0) {
      this.momentLeft = 0;
      this.face.endMoment();
    }
    this.updateFps(now);
    return true;
  }

  // ------------------------------------------------------------------ public control

  /**
   * The moment: for `ms` milliseconds only the daemon head is drawn, in close-up (hard cut in and out).
   * Returns false when the daemon head is not loaded (nothing is shown then).
   */
  startMoment(ms: number): boolean {
    if (this.noPost || !this.face.momentReady) return false;
    this.momentLeft = Math.max(1, ms);
    return true;
  }
  /** Keeps the daemon head loaded while a moment is pending, even with the face switched off. */
  keepFaceReady(on: boolean): void { this.face.setKeepReady(on); }

  setMode(m: CouplingMode): void {
    if (m !== this.mode) this.modelFirst.reset(); // the policy is only active while talking
    this.mode = m; if (m !== 'boot') this.downloadBps = 0;
    this.face.setMode(m);
  }
  /** Settings: "Gesicht im Nebel" on/off and the voice gender that picks the angel head (male, otherwise female). */
  setFace(show: boolean, gender: 'female' | 'male' | null): void {
    this.face.setGender(gender);
    this.face.setEnabled(show);
  }
  /**
   * null = start screen / neutral. angel = textured angel head by voice gender (angel-female / angel-male) + neutral
   * ink; daemon = daemon.glb + ember ink. (Debug: `?head=<angel-female|angel-male|daemon>` pins a head.)
   */
  setPersona(p: Persona | null): void {
    this.face.setPersona(p);
    const ember = p === 'daemon';
    this.coupling.setPalette(ember ? 'ember' : 'neutral');
    this.sim.params.ember = ember ? 1 : 0;
    this.injector.ember = ember;
    this.emberAmt = ember ? 1 : 0;
  }
  setDownloadRate(bps: number): void { this.downloadBps = Math.max(0, bps); }
  pushViz(f: VizFrame): void {
    this.pendingViz = f;
    this.lastViz = f;
    this.latencyMs = f.latencyMs;
    const now = performance.now();
    for (const p of f.words) this.dispatchWords(this.assembler.push(p), now);
    this.face.pushPieces(f.words.map((p) => p.text));
  }
  setQualityMode(q: Quality): void {
    if (this.forcedLevel) return; // an explicit debug override (?quality=) wins over settings
    this.quality.setMode(q);
    this.applyLevel(this.effectiveLevel());
  }
  onWordEvent(cb: (e: WordEvent) => void): void { this.wordCbs.push(cb); }
  /** Ends the open word (conversation end) and lets everything dissolve. */
  resetWords(): void {
    const now = performance.now();
    this.dispatchWords(this.assembler.flush(), now);
    this.assembler.reset();
    this.smoother.reset();
    this.pendingViz = null;
  }

  private dispatchWords(evs: WordEvent[], now: number): void {
    for (const ev of evs) {
      this.injector.handle(ev, now);
      for (const cb of this.wordCbs) cb(ev);
    }
  }

  // ------------------------------------------------------------------ quality + GPU timing

  /** The level the user/auto wants, capped by the model-first budget. */
  private effectiveLevel(): QualityLevel {
    return minQuality(this.quality.level, this.budget.maxQuality);
  }

  private applyLevel(level: QualityLevel, force = false): void {
    if (level === this.level && !force) return;
    this.level = level;
    this.resizeAll();
  }

  /** Feeds the latency to the model-first controller (talking only). Returns true when the budget changed. */
  private updateBudget(now: number): boolean {
    let next = BUDGET_TIERS[0];
    if (!this.modelFirstOff && this.mode === 'talking' && this.latencyMs !== null) {
      next = this.modelFirst.update(this.latencyMs, now);
    }
    if (next === this.budget) return false;
    const scaleChanged = next.renderScale !== this.budget.renderScale;
    const postChanged = next.post !== this.budget.post;
    this.budget = next;
    // the fps cap alone needs no reallocation
    return scaleChanged || postChanged || this.effectiveLevel() !== this.level;
  }

  private updateQuality(frameMs: number, now: number): void {
    if (frameMs > 250) return; // tab was hidden or the window stalled
    if (this.budget.tier > 0 || this.forcedLevel) return; // capped frame times would only teach auto the wrong lesson
    const ms = frameMs; // rAF delta (the EXT_disjoint_timer_query reading under-reports on ANGLE/D3D11: 0.6 ms for a 20 ms frame)
    const before = this.quality.level;
    const next = this.quality.update(ms, now);
    if (next === 'ultra') {
      // ultra costs ~20 ms on the reference GPU; auto never raises there (it would flap), it is opt-in via settings
      this.quality.level = before;
    } else if (next) {
      this.applyLevel(this.effectiveLevel());
    }
  }

  private resizeAll(): void {
    const q = QUALITY_TABLE[this.level];
    this.sim.params.pressureIterations = q.pressureIterations;
    this.sim.resize(q.sim, q.dye);
    this.post.resize(this.canvas.width, this.canvas.height, postParams(q, this.budget.post));
  }

  private drawScreen(): void {
    const gl = this.glc.gl;
    if (this.noPost) {
      const p = this.pDisplay;
      p.bind();
      gl.uniform1i(p.uniforms['uDye'], this.sim.dye.attach(0));
      drawQuad(gl, null, [this.canvas.width, this.canvas.height]);
      return;
    }
    this.post.render(this.sim.dye, {
      timeSec: this.timeSec,
      rms: this.caRms,
      lightPos: this.lightPos,
      ember: this.emberAmt,
    }, this.faceDebug && this.face.postInputs ? { ...this.face.postInputs, debug: true } : this.face.postInputs);
  }

  // ------------------------------------------------------------------ debug

  private setupDebug(): void {
    const el = document.createElement('div');
    el.style.cssText =
      'position:fixed;left:8px;top:8px;font:12px/1.3 ui-monospace,Consolas,monospace;color:#9fd;' +
      'background:rgba(0,0,0,.45);padding:4px 8px;border-radius:4px;pointer-events:none;z-index:9999';
    document.body.appendChild(el);
    this.fpsEl = el;
    this.disposers.push(() => el.remove());

    let last: { x: number; y: number } | null = null;
    let down = false;
    const pos = (e: PointerEvent): { x: number; y: number } => {
      const r = this.canvas.getBoundingClientRect();
      return { x: (e.clientX - r.left) / r.width, y: 1 - (e.clientY - r.top) / r.height };
    };
    const onDown = (e: PointerEvent): void => {
      down = true;
      last = pos(e);
    };
    const onUp = (): void => {
      down = false;
      last = null;
    };
    const onMove = (e: PointerEvent): void => {
      if (!down) return;
      const p = pos(e);
      if (last) {
        const color = hsv(Math.random(), 0.85, 1).map((c) => c * 1.2) as RGB;
        this.sim.splat(p.x, p.y, (p.x - last.x) * 1000 * 6, (p.y - last.y) * 1000 * 6, color, 0.0025);
      }
      last = p;
    };
    window.addEventListener('pointerdown', onDown);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointermove', onMove);
    this.disposers.push(() => {
      window.removeEventListener('pointerdown', onDown);
      window.removeEventListener('pointerup', onUp);
      window.removeEventListener('pointermove', onMove);
    });
    (window as unknown as { __haru: unknown }).__haru = this;
  }

  private updateFps(now: number): void {
    this.fpsFrames++;
    if (now - this.fpsT0 >= 500) {
      this.fpsValue = (this.fpsFrames * 1000) / (now - this.fpsT0);
      this.fpsFrames = 0;
      this.fpsT0 = now;
      if (this.fpsEl) {
        const c = this.canvas;
        this.fpsEl.textContent =
          `${this.fpsValue.toFixed(1)} fps  ${this.frameEma.toFixed(1)} ms  ${c.width}x${c.height}  ` +
          `dye ${this.sim.dye.width}x${this.sim.dye.height}  sim ${this.sim.velocity.width}x${this.sim.velocity.height}
` +
          `quality ${this.level}  mf-tier ${this.budget.tier}${this.modelFirstOff ? '(off)' : ''} x${this.budget.renderScale}${this.budget.fpsCap ? ' cap' + this.budget.fpsCap : ''}  latency ${this.lastViz ? Math.round(this.lastViz.latencyMs) : '-'} ms  words ${this.injector.count}  mode ${this.mode}`;
      }
    }
  }
}
