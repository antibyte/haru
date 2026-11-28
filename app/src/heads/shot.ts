import type { Speaker } from '../ipc/types';

/** The hidden speaker must be clearly louder for this long before the picture cuts to them. */
export const SHOT_SWITCH_MS = 700;
/** "Clearly louder": at least this factor above the shown speaker. */
export const SHOT_LOUDER = 1.6;
/** Below this smoothed RMS a voice counts as silent (the presence.ts quiet floor). */
export const SHOT_SILENT_RMS = 0.02;
/** The dissolve through the fog: first half the old head fades out, second half the new one fades in. */
export const SHOT_DISSOLVE_MS = 600;

export interface ShotLevels { angel: number; daemon: number }
export interface ShotState { shown: Speaker; louderMs: number }
/** What the renderer draws: whose head, its visibility (0..1) and the ember grade (0 angel .. 1 daemon). */
export interface ShotView { speaker: Speaker; gain: number; ember: number }

/** Haru opens the battle. */
export const SHOT_START: ShotState = { shown: 'angel', louderMs: 0 };

const other = (s: Speaker): Speaker => (s === 'angel' ? 'daemon' : 'angel');
const smooth = (t: number): number => { const x = Math.min(1, Math.max(0, t)); return x * x * (3 - 2 * x); };
const emberOf = (s: Speaker): number => (s === 'daemon' ? 1 : 0);

/**
 * Hysteresis: time counts up while the hidden speaker is clearly louder than the shown one and drains twice as fast
 * otherwise; at SHOT_SWITCH_MS the shot changes. When both are silent nothing counts up, so the last speaker stays.
 */
export function nextShot(s: ShotState, lv: ShotLevels, dtMs: number): ShotState {
  const dt = Math.max(0, dtMs);
  const hidden = lv[other(s.shown)];
  const louder = hidden >= SHOT_SILENT_RMS && hidden >= SHOT_LOUDER * lv[s.shown];
  const louderMs = louder ? s.louderMs + dt : Math.max(0, s.louderMs - 2 * dt);
  return louderMs >= SHOT_SWITCH_MS ? { shown: other(s.shown), louderMs: 0 } : { shown: s.shown, louderMs };
}

/** The dissolve `elapsedMs` after a cut from `from` to `to`. */
export function shotView(from: Speaker, to: Speaker, elapsedMs: number): ShotView {
  const half = SHOT_DISSOLVE_MS / 2;
  const ember = emberOf(from) + (emberOf(to) - emberOf(from)) * smooth(elapsedMs / SHOT_DISSOLVE_MS);
  if (elapsedMs < half) return { speaker: from, gain: 1 - smooth(elapsedMs / half), ember };
  return { speaker: to, gain: smooth((elapsedMs - half) / half), ember };
}

/** Hysteresis plus dissolve, fed with the per-speaker smoothed levels every render frame. */
export class ShotDirector {
  private state: ShotState = SHOT_START;
  private from: Speaker = 'angel';
  private elapsed = SHOT_DISSOLVE_MS;

  update(lv: ShotLevels, dtMs: number): ShotView {
    const before = this.state.shown;
    this.state = nextShot(this.state, lv, dtMs);
    if (this.state.shown !== before) {
      this.from = before;
      this.elapsed = 0;
    } else {
      this.elapsed = Math.min(SHOT_DISSOLVE_MS, this.elapsed + Math.max(0, dtMs));
    }
    return shotView(this.from, this.state.shown, this.elapsed);
  }
}
