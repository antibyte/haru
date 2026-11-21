import type { Quality, QualityLevel } from '../ipc/types';

export interface QualityParams { sim: number; dye: number; pressureIterations: number; bloomLevels: number; raySamples: number; rayScale: number }

export const QUALITY_TABLE: Record<QualityLevel, QualityParams> = {
  low: { sim: 128, dye: 512, pressureIterations: 12, bloomLevels: 3, raySamples: 24, rayScale: 0.25 },
  medium: { sim: 192, dye: 1024, pressureIterations: 16, bloomLevels: 4, raySamples: 32, rayScale: 0.25 },
  high: { sim: 256, dye: 1440, pressureIterations: 20, bloomLevels: 5, raySamples: 48, rayScale: 0.5 },
  ultra: { sim: 384, dye: 2048, pressureIterations: 24, bloomLevels: 6, raySamples: 64, rayScale: 0.5 },
};

const ORDER: QualityLevel[] = ['low', 'medium', 'high', 'ultra'];
const EMA_ALPHA = 0.1;
const DOWN_MS = 20;
const UP_MS = 12;
const DOWN_AFTER = 2000;
const UP_AFTER = 5000;

/** Adaptive quality: steps down after sustained slow frames, up after sustained fast ones. */
export class QualityController {
  level: QualityLevel;
  private mode: Quality;
  private ema: number | null = null;
  private slowSince: number | null = null;
  private fastSince: number | null = null;

  constructor(mode: Quality, initial: QualityLevel = 'high') {
    this.mode = mode;
    this.level = mode === 'auto' ? initial : mode;
  }

  setMode(m: Quality): void {
    this.mode = m;
    if (m !== 'auto') this.level = m;
    this.resetTimers();
  }

  /** Feed one frame time. Returns the new level when it changed, otherwise null. */
  update(frameMs: number, nowMs: number): QualityLevel | null {
    if (this.mode !== 'auto') return null;
    this.ema = this.ema === null ? frameMs : this.ema + EMA_ALPHA * (frameMs - this.ema);

    if (this.ema > DOWN_MS) {
      this.fastSince = null;
      this.slowSince ??= nowMs;
      if (nowMs - this.slowSince >= DOWN_AFTER) return this.step(-1);
    } else if (this.ema < UP_MS) {
      this.slowSince = null;
      this.fastSince ??= nowMs;
      if (nowMs - this.fastSince >= UP_AFTER) return this.step(1);
    } else {
      this.resetTimers();
    }
    return null;
  }

  private step(dir: -1 | 1): QualityLevel | null {
    this.resetTimers();
    const i = ORDER.indexOf(this.level) + dir;
    if (i < 0 || i >= ORDER.length) return null;
    this.level = ORDER[i];
    return this.level;
  }

  private resetTimers(): void {
    this.slowSince = null;
    this.fastSince = null;
  }
}
