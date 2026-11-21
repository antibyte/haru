/**
 * Lip-sync / expression controller: pure, deterministic given an injected rng. Turns the VizFrame stream
 * (model RMS, mic RMS, word pieces) into the 8 morph weights of the head contract plus presence and head motion.
 */

export const MORPH_NAMES = ['jawOpen', 'lipsWide', 'lipsRound', 'lipsPress', 'smile', 'browRaise', 'browFrown', 'blink'] as const;
export type MorphName = (typeof MORPH_NAMES)[number];
export const MORPH = { jawOpen: 0, lipsWide: 1, lipsRound: 2, lipsPress: 3, smile: 4, browRaise: 5, browFrown: 6, blink: 7 } as const;

export type LipMode = 'boot' | 'idle' | 'talking';

export interface LipsyncInput {
  dtMs: number;
  mode: LipMode;
  /** smoothed model RMS (0..1) */
  modelRms: number;
  /** smoothed mic RMS (0..1) */
  micRms: number;
  /** pieces that became audible since the previous update (text of VizFrame.words, '' = pause) */
  pieces: readonly string[];
}

export interface LipsyncOutput {
  /** morph weights in contract order, each 0..1 */
  weights: Float32Array;
  /** 0..1: how strongly the face may emerge from the ink */
  presence: number;
  /** speech activity 0..1 (smoothed), for ink emission */
  speech: number;
  /** head pitch offset in radians (positive = chin down) */
  nod: number;
  /** head roll in radians */
  tilt: number;
}

export const PRESENCE_FLOOR = 0.08;
export const PRESENCE_ATTACK_S = 0.4;
export const PRESENCE_RELEASE_S = 2.5;
const SPEAK_RMS = 0.02;
const SPEAK_HOLD_MS = 700;
const VISEME_HOLD_MS = 190;
const PRESS_MS = 95;
const PHRASE_GAP_MS = 900;

const clamp01 = (x: number) => Math.min(1, Math.max(0, x));
const lerpK = (dtMs: number, tauMs: number) => 1 - Math.exp(-dtMs / tauMs);

export type Vowel = 'a' | 'e' | 'o' | null;
export interface PieceInfo {
  vowel: Vowel;
  press: boolean;
  cjk: boolean;
  question: boolean;
  exclaim: boolean;
  /** piece contains at least one letter */
  voiced: boolean;
}

const CJK = /[぀-ヿ㐀-䶿一-鿿ｦ-ﾟ가-힯]/u;

/** Letter-level analysis of one WordPiece: the sustained vowel (the last one), a leading/trailing lip closure, punctuation. */
export function analysePiece(text: string): PieceInfo {
  const t = text.toLowerCase();
  const info: PieceInfo = { vowel: null, press: false, cjk: CJK.test(t), question: /[?？]/.test(t), exclaim: /[!！]/.test(t), voiced: false };
  const letters = [...t].filter((c) => /\p{L}/u.test(c));
  info.voiced = letters.length > 0;
  if (info.cjk) return info;
  for (const c of letters) {
    if ('aäàáâãå'.includes(c)) info.vowel = 'a';
    else if ('eiéèêëíìîïy'.includes(c)) info.vowel = 'e';
    else if ('ouöüóòôõúùû'.includes(c)) info.vowel = 'o';
  }
  const first = letters[0];
  const last = letters[letters.length - 1];
  info.press = (first !== undefined && 'mbp'.includes(first)) || (last !== undefined && 'mbp'.includes(last));
  return info;
}

/** Asymmetric one-pole follower. */
class Follower {
  value = 0;
  constructor(private readonly attackMs: number, private readonly releaseMs: number) {}
  step(target: number, dtMs: number): number {
    this.value += (target - this.value) * lerpK(dtMs, target > this.value ? this.attackMs : this.releaseMs);
    return this.value;
  }
}

export class LipsyncController {
  private readonly jaw = new Follower(38, 120);
  private readonly wide = new Follower(60, 140);
  private readonly round = new Follower(70, 150);
  private readonly press = new Follower(25, 80);
  private readonly smile = new Follower(180, 500);
  private readonly brow = new Follower(120, 450);
  private readonly frown = new Follower(250, 600);
  private readonly speechEnv = new Follower(60, 400);
  private readonly fast = new Follower(25, 120);
  private readonly slow = new Follower(900, 900);

  private vowel: Vowel = null;
  private vowelAgeMs = Infinity;
  private pressMs = 0;
  private cjkPhase = 0;
  private cjkActiveMs = 0;
  private silentMs = Infinity;
  private speakingMs = 0;

  private browPulse = 0;
  private questionMs = 0;
  private smileMs = 0;
  private smilePower = 0;
  private frownMs = 0;
  private nextFrownMs: number;

  private blinkT = -1; // seconds since the blink started, -1 = none
  private nextBlinkMs: number;
  private pendingDouble = false;

  private presence = 0;
  private onset = 0;
  private nodPhase = 0;
  private nodV = 0;
  private nodValue = 0;
  private tiltValue = 0;
  private listenEnv = 0;

  private readonly out: LipsyncOutput = { weights: new Float32Array(8), presence: 0, speech: 0, nod: 0, tilt: 0 };

  constructor(private readonly rng: () => number = Math.random) {
    this.nextBlinkMs = this.randBlink();
    this.nextFrownMs = 22000 + this.rng() * 25000;
  }

  private randBlink(): number {
    return 2500 + this.rng() * 3500;
  }

  reset(): void {
    this.vowel = null;
    this.vowelAgeMs = Infinity;
    this.silentMs = Infinity;
    this.speakingMs = 0;
    this.questionMs = this.smileMs = this.frownMs = 0;
  }

  update(inp: LipsyncInput): LipsyncOutput {
    const dt = Math.min(Math.max(inp.dtMs, 0), 100);
    const talking = inp.mode === 'talking';
    const rms = talking ? inp.modelRms : 0;

    // ---- speaking detection + phrase starts
    const gapBefore = this.silentMs;
    const speakingNow = rms > SPEAK_RMS;
    this.speakingMs = speakingNow ? SPEAK_HOLD_MS : Math.max(0, this.speakingMs - dt);
    const speaking = this.speakingMs > 0;
    this.silentMs = speakingNow ? 0 : this.silentMs + dt;
    const speech = this.speechEnv.step(speaking ? clamp01(rms / 0.15) : 0, dt);

    // ---- word pieces
    for (const text of inp.pieces) {
      if (!talking) break;
      const p = analysePiece(text);
      if (p.voiced || p.question || p.exclaim) {
        if (gapBefore > PHRASE_GAP_MS) this.browPulse = Math.max(this.browPulse, 0.5);
        this.onset = 1;
      }
      if (p.voiced) {
        this.vowel = p.cjk ? null : p.vowel;
        this.vowelAgeMs = 0;
        this.cjkActiveMs = p.cjk ? VISEME_HOLD_MS * 2 : 0;
        if (p.press) this.pressMs = PRESS_MS;
      }
      if (p.question) this.questionMs = 1400;
      if (p.exclaim) { this.smileMs = 1600; this.smilePower = Math.max(this.smilePower, 0.7); }
    }
    this.vowelAgeMs += dt;
    if (this.vowelAgeMs > VISEME_HOLD_MS * 2.2) this.vowel = null;
    if (this.cjkActiveMs > 0) this.cjkActiveMs = Math.max(0, this.cjkActiveMs - dt);
    this.cjkPhase += (dt / 1000) * 5.2;

    // ---- laughter-like spikes: a fast envelope far above the slow one
    const fast = this.fast.step(rms, dt);
    const slow = this.slow.step(rms, dt);
    if (fast > 0.16 && fast > 2.0 * slow + 0.04 && this.smileMs < 400) { this.smileMs = 1500; this.smilePower = Math.max(this.smilePower, 0.5); }

    // ---- mouth
    const level = 1 - Math.exp(-rms / 0.08); // 0.12 -> 0.78
    let jawT = level * 0.62;
    let wideT = 0;
    let roundT = 0;
    const live = this.vowelAgeMs < VISEME_HOLD_MS * 2.2 ? 1 : 0;
    if (this.vowel === 'a') jawT += 0.18 * level * live;
    else if (this.vowel === 'e') { jawT -= 0.12 * level * live; wideT = 0.85 * level * live; }
    else if (this.vowel === 'o') { jawT -= 0.05 * level * live; roundT = 0.9 * level * live; }
    else if (this.cjkActiveMs > 0 || (speaking && this.vowel === null && rms > SPEAK_RMS)) {
      roundT = (0.18 + 0.14 * Math.sin(this.cjkPhase)) * level;
      wideT = (0.1 + 0.08 * Math.sin(this.cjkPhase * 0.73 + 1)) * level;
    }
    const pressing = this.pressMs > 0 ? 1 : 0;
    this.pressMs = Math.max(0, this.pressMs - dt);
    jawT *= 1 - 0.75 * pressing;
    wideT *= 1 - pressing;
    roundT *= 1 - pressing;
    const w = this.out.weights;
    w[MORPH.jawOpen] = clamp01(this.jaw.step(clamp01(jawT), dt));
    w[MORPH.lipsWide] = clamp01(this.wide.step(clamp01(wideT), dt));
    w[MORPH.lipsRound] = clamp01(this.round.step(clamp01(roundT), dt));
    w[MORPH.lipsPress] = clamp01(this.press.step(0.95 * pressing, dt));

    // ---- expression
    this.browPulse *= Math.exp(-dt / 650);
    this.questionMs = Math.max(0, this.questionMs - dt);
    this.smileMs = Math.max(0, this.smileMs - dt);
    if (this.smileMs === 0) this.smilePower = 0;
    this.nextFrownMs -= talking && speaking ? dt : 0;
    if (this.nextFrownMs <= 0) { this.frownMs = 1300; this.nextFrownMs = 25000 + this.rng() * 30000; }
    this.frownMs = Math.max(0, this.frownMs - dt);

    // listening: the user speaks while Haru does not
    const listening = talking && !speaking && inp.micRms > 0.03;
    this.listenEnv += ((listening ? 1 : 0) - this.listenEnv) * lerpK(dt, listening ? 350 : 900);

    const breathSmile = talking ? 0.06 + 0.04 * speech : 0.03;
    const smileT = Math.max(breathSmile, this.smileMs > 0 ? this.smilePower : 0);
    const browT = Math.max(this.browPulse, this.questionMs > 0 ? 0.8 : 0, 0.28 * this.listenEnv);
    w[MORPH.smile] = clamp01(this.smile.step(smileT, dt));
    w[MORPH.browRaise] = clamp01(this.brow.step(browT, dt));
    w[MORPH.browFrown] = clamp01(this.frown.step(this.frownMs > 0 ? 0.4 : 0, dt));

    // ---- blinks (always, also while idle: the face is alive whenever it shows up)
    this.nextBlinkMs -= dt;
    if (this.blinkT < 0 && this.nextBlinkMs <= 0) {
      this.blinkT = 0;
      this.pendingDouble = this.rng() < 0.2;
      this.nextBlinkMs = this.randBlink();
    }
    let blink = 0;
    if (this.blinkT >= 0) {
      this.blinkT += dt / 1000;
      const t = this.blinkT;
      blink = t < 0.07 ? t / 0.07 : t < 0.11 ? 1 : t < 0.25 ? 1 - (t - 0.11) / 0.14 : 0;
      if (t >= 0.25) {
        this.blinkT = -1;
        if (this.pendingDouble) { this.pendingDouble = false; this.nextBlinkMs = 120; }
      }
    }
    w[MORPH.blink] = clamp01(blink);

    // ---- head motion: micro nods on word onsets and with the speech level; listening tilts the head
    this.onset *= Math.exp(-dt / 220);
    this.nodPhase += (dt / 1000) * 2 * Math.PI * 1.15;
    const nodTarget = speech * (0.018 * Math.sin(this.nodPhase) + 0.02 * this.onset) + 0.03 * this.listenEnv;
    // critically-damped-ish spring
    const k = 40;
    const c = 11;
    this.nodV += (k * (nodTarget - this.nodValue) - c * this.nodV) * (dt / 1000);
    this.nodValue += this.nodV * (dt / 1000);
    this.tiltValue += (0.065 * this.listenEnv - this.tiltValue) * lerpK(dt, 700);

    // ---- presence
    const target = !talking ? 0 : speaking ? 1 : PRESENCE_FLOOR;
    const tau = (target > this.presence ? PRESENCE_ATTACK_S : PRESENCE_RELEASE_S) * 1000;
    this.presence += (target - this.presence) * lerpK(dt, tau);

    this.out.presence = clamp01(this.presence);
    this.out.speech = speech;
    this.out.nod = this.nodValue;
    this.out.tilt = this.tiltValue;
    return this.out;
  }
}
