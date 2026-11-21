/** Spectral centroid of a band vector, normalized to 0..1 (0.5 for silence). */
export function spectralCentroid(bands: readonly number[]): number {
  const n = bands.length;
  let sum = 0;
  let weighted = 0;
  for (let i = 0; i < n; i++) {
    sum += bands[i];
    weighted += i * bands[i];
  }
  if (sum <= 0 || n < 2) return 0.5;
  return weighted / sum / (n - 1);
}

/** Asymmetric one-pole follower: fast attack, slow release. */
export class Envelope {
  value = 0;
  constructor(private readonly attackMs = 30, private readonly releaseMs = 200) {}

  step(x: number, dtMs: number): number {
    const tau = x > this.value ? this.attackMs : this.releaseMs;
    const k = 1 - Math.exp(-dtMs / tau);
    this.value += (x - this.value) * k;
    return this.value;
  }
}

export interface SmoothedAudio { modelRms: number; modelCentroid: number; micRms: number; micCentroid: number }

const STALE_MS = 250;
const CENTROID_ALPHA = 0.2;
const SILENT_RMS = 0.01;

/** Smooths the 60 Hz viz frames: envelope followers for loudness, EMA for the spectral centroid. */
export class FeatureSmoother {
  private readonly model = new Envelope(30, 200);
  private readonly mic = new Envelope(30, 200);
  private modelCentroid = 0.5;
  private micCentroid = 0.5;
  private targetModel = 0;
  private targetMic = 0;
  private ageMs = Infinity;

  /** Pass the newest frame, or null when none arrived since the last call. Stale input decays to silence. */
  update(f: VizFrameLike | null, dtMs: number): SmoothedAudio {
    if (f) {
      this.ageMs = 0;
      this.targetModel = f.model.rms;
      this.targetMic = f.mic.rms;
      if (f.model.rms > SILENT_RMS) {
        this.modelCentroid += (spectralCentroid(f.model.bands) - this.modelCentroid) * CENTROID_ALPHA;
      }
      if (f.mic.rms > SILENT_RMS) {
        this.micCentroid += (spectralCentroid(f.mic.bands) - this.micCentroid) * CENTROID_ALPHA;
      }
    } else {
      this.ageMs += dtMs;
      if (this.ageMs > STALE_MS) {
        this.targetModel = 0;
        this.targetMic = 0;
      }
    }
    return {
      modelRms: this.model.step(this.targetModel, dtMs),
      modelCentroid: this.modelCentroid,
      micRms: this.mic.step(this.targetMic, dtMs),
      micCentroid: this.micCentroid,
    };
  }

  reset(): void {
    this.model.value = 0;
    this.mic.value = 0;
    this.targetModel = 0;
    this.targetMic = 0;
    this.ageMs = Infinity;
  }
}

/** The part of VizFrame the smoother reads (keeps this module free of IPC imports). */
export interface VizFrameLike {
  model: { rms: number; bands: readonly number[] };
  mic: { rms: number; bands: readonly number[] };
}
