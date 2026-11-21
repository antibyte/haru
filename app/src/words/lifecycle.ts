export const INK_MS = 250;
export const RELEASE_MS = 400;
const HOLD_STRENGTH = 0.15;
const END_GRACE_MS = 300;

/** Minimum time a word stays readable after the ink phase. */
export function holdMs(len: number): number {
  return Math.min(1200, Math.max(600, 600 + 60 * len));
}

export interface PhaseState {
  phase: 'ink' | 'hold' | 'release' | 'done';
  inkStrength: number;
  holdStrength: number;
  rest: boolean;
  releaseT: number;
}

export function wordPhase(nowMs: number, startMs: number, endedAtMs: number | null, len: number): PhaseState {
  const age = nowMs - startMs;
  if (age < INK_MS) return { phase: 'ink', inkStrength: 1, holdStrength: 0, rest: false, releaseT: 0 };
  const releaseStart = Math.max(startMs + INK_MS + holdMs(len), (endedAtMs ?? Infinity) + END_GRACE_MS);
  if (nowMs < releaseStart) return { phase: 'hold', inkStrength: 0, holdStrength: HOLD_STRENGTH, rest: true, releaseT: 0 };
  const dt = nowMs - releaseStart;
  if (dt < RELEASE_MS) return { phase: 'release', inkStrength: 0, holdStrength: 0, rest: false, releaseT: dt / RELEASE_MS };
  return { phase: 'done', inkStrength: 0, holdStrength: 0, rest: false, releaseT: 1 };
}
