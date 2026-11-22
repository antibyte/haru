export interface PresenceStore { get(): string | null; set(v: string): void }
/**
 * "Quiet" floor on the linear block RMS of `VizFrame.model.rms` / `mic.rms` (float samples, full-scale sine = 0.707;
 * Rust `audio/features.rs`). Speech sits around 0.05 .. 0.2 (the test voices use 0.1, the mock uses 0.12 .. 0.22 for
 * speech and 0.01 for silence); 0.02 is about -34 dBFS, well above a silent stream (0) and a quiet room's noise floor.
 */
export const QUIET_RMS = 0.02;
export const QUIET_MS = 1200;
export const MIN_DELAY_MS = 30_000;
export const MAX_DELAY_MS = 180_000;
const KEY = 'haru.presence';

export function localPresenceStore(): PresenceStore {
  let mem: string | null = null;
  return {
    get: () => { try { return localStorage.getItem(KEY); } catch { return mem; } },
    set: (v) => { mem = v; try { localStorage.setItem(KEY, v); } catch { /* memory only */ } },
  };
}

export class Presence {
  private remaining: number;
  private armed: boolean;
  private active = false;
  private fireAt = 0;
  private quietSince: number | null = null;
  private forced = false;

  constructor(private readonly store: PresenceStore, private readonly rand: () => number = Math.random) {
    const s = parse(store.get());
    this.remaining = s?.remaining ?? this.draw();
    this.armed = s?.armed ?? false;
  }

  /** A daemon conversation starts. Returns whether the moment is armed for it. */
  begin(nowMs: number): boolean {
    if (!this.armed) {
      this.remaining -= 1;
      if (this.remaining <= 0) this.armed = true;
      this.save();
    }
    this.active = true;
    this.quietSince = null;
    this.fireAt = nowMs + MIN_DELAY_MS + this.rand() * (MAX_DELAY_MS - MIN_DELAY_MS);
    return this.armed;
  }

  /** Every viz frame of a daemon conversation; true exactly once when the moment fires. */
  tick(nowMs: number, modelRms: number, micRms: number): boolean {
    if (!this.active || (!this.armed && !this.forced)) return false;
    if (modelRms < QUIET_RMS && micRms < QUIET_RMS) this.quietSince ??= nowMs;
    else this.quietSince = null;
    const quiet = this.quietSince !== null && nowMs - this.quietSince >= QUIET_MS;
    if (!this.forced && !(nowMs >= this.fireAt && quiet)) return false;
    this.forced = false;
    this.active = false;
    this.armed = false;
    this.remaining = this.draw();
    this.save();
    return true;
  }

  /** The conversation ended; an armed moment carries over to the next daemon conversation. */
  end(): void { this.active = false; this.quietSince = null; }

  /** Debug: fire on the next tick of an active conversation. */
  force(): void { this.forced = true; }

  private draw(): number { return 1 + Math.min(4, Math.floor(this.rand() * 5)); }
  private save(): void { this.store.set(JSON.stringify({ remaining: this.remaining, armed: this.armed })); }
}

function parse(v: string | null): { remaining: number; armed: boolean } | null {
  if (!v) return null;
  try {
    const o = JSON.parse(v) as { remaining?: unknown; armed?: unknown };
    if (typeof o.remaining !== 'number' || !Number.isInteger(o.remaining) || o.remaining < 0 || o.remaining > 5) return null;
    return { remaining: o.remaining, armed: o.armed === true };
  } catch { return null; }
}
