import type { QualityLevel } from '../ipc/types';

/** How much of the post chain (bloom + god rays) is drawn. 'full' = whatever the quality level specifies. */
export type PostTier = 'full' | 'lite' | 'minimal';

export interface RenderBudget {
  tier: number;
  /** Upper bound for the quality level (the user's choice / auto may pick lower, never higher). */
  maxQuality: QualityLevel;
  /** Max rendered frames per second, null = display rate. */
  fpsCap: number | null;
  /** Canvas backing-store scale relative to DPR (CSS size is unchanged). */
  renderScale: number;
  post: PostTier;
}

/** Tier 0 changes nothing; the last tier is the most frugal. */
export const BUDGET_TIERS: readonly RenderBudget[] = [
  { tier: 0, maxQuality: 'ultra', fpsCap: null, renderScale: 1.0, post: 'full' },
  { tier: 1, maxQuality: 'high', fpsCap: null, renderScale: 0.75, post: 'full' },
  { tier: 2, maxQuality: 'medium', fpsCap: 45, renderScale: 0.75, post: 'lite' },
  { tier: 3, maxQuality: 'low', fpsCap: 30, renderScale: 0.5, post: 'minimal' },
];

export interface ModelFirstConfig {
  /** latency above this for highAfterMs steps down one tier */
  highMs: number;
  highAfterMs: number;
  /** latency above this for severeAfterMs jumps to the most frugal tier */
  severeMs: number;
  severeAfterMs: number;
  /** latency below this for lowAfterMs steps up one tier */
  lowMs: number;
  lowAfterMs: number;
  /** after a step down, wait this long before the next (non-severe) step so the backlog can react */
  settleMs: number;
}

export const DEFAULT_MODEL_FIRST: ModelFirstConfig = {
  highMs: 450,
  highAfterMs: 500,
  severeMs: 700,
  severeAfterMs: 200,
  lowMs: 350,
  lowAfterMs: 6000,
  settleMs: 1500,
};

/**
 * "Model first": the speech model shares the GPU with the renderer, so when the model's backlog (VizFrame.latencyMs)
 * grows, rendering gets cheaper. Escalates quickly, recovers slowly (hysteresis band lowMs..highMs holds the tier).
 * Pure: feed it latency samples with a monotonic clock.
 */
export class ModelFirstController {
  private t = 0;
  private highSince: number | null = null;
  private severeSince: number | null = null;
  private lowSince: number | null = null;
  private lastStepDown = -Infinity;
  private readonly cfg: ModelFirstConfig;

  constructor(cfg: Partial<ModelFirstConfig> = {}) {
    this.cfg = { ...DEFAULT_MODEL_FIRST, ...cfg };
  }

  get tier(): number { return this.t; }
  get budget(): RenderBudget { return BUDGET_TIERS[this.t]; }

  reset(): void {
    this.t = 0;
    this.highSince = this.severeSince = this.lowSince = null;
    this.lastStepDown = -Infinity;
  }

  /** Feed one latency sample (ms) at time nowMs; returns the budget to use now. */
  update(latencyMs: number, nowMs: number): RenderBudget {
    if (!Number.isFinite(latencyMs)) return this.budget;
    const c = this.cfg;
    const last = BUDGET_TIERS.length - 1;

    this.severeSince = latencyMs > c.severeMs ? (this.severeSince ?? nowMs) : null;
    this.highSince = latencyMs > c.highMs ? (this.highSince ?? nowMs) : null;
    this.lowSince = latencyMs < c.lowMs ? (this.lowSince ?? nowMs) : null;

    if (this.severeSince !== null && nowMs - this.severeSince >= c.severeAfterMs) {
      if (this.t < last) this.stepDownTo(last, nowMs);
    } else if (this.highSince !== null && nowMs - this.highSince >= c.highAfterMs && nowMs - this.lastStepDown >= c.settleMs) {
      if (this.t < last) this.stepDownTo(this.t + 1, nowMs);
    } else if (this.lowSince !== null && nowMs - this.lowSince >= c.lowAfterMs) {
      if (this.t > 0) this.t--;
      this.lowSince = null; // the next step up needs another full low period
    }
    return this.budget;
  }

  private stepDownTo(tier: number, nowMs: number): void {
    this.t = tier;
    this.lastStepDown = nowMs;
    this.highSince = null; // another sustained period is needed for the next step
    this.severeSince = null;
    this.lowSince = null;
  }
}

/** Lower of two quality levels. */
export function minQuality(a: QualityLevel, b: QualityLevel): QualityLevel {
  const order: QualityLevel[] = ['low', 'medium', 'high', 'ultra'];
  return order[Math.min(order.indexOf(a), order.indexOf(b))];
}
