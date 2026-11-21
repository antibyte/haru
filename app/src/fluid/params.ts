export interface FluidParams {
  densityDissipation: number;
  velocityDissipation: number;
  pressure: number;
  pressureIterations: number;
  /** Vorticity confinement. Low values keep ink in silky filaments; high values curl it into blobs. */
  curl: number;
  restStrength: number;
  /** Strength of the persistent curl-noise flow (cells/s gained per second). 0 disables it. */
  flow: number;
  /** Colour ageing speed of the dye (gold -> magenta -> violet). 0 keeps the colours fixed. */
  ageing: number;
}

export const DEFAULT_FLUID: FluidParams = {
  densityDissipation: 0.8,
  velocityDissipation: 0.5,
  pressure: 0.8,
  pressureIterations: 20,
  curl: 3,
  restStrength: 0.9,
  flow: 10,
  ageing: 1,
};

export type RGB = [number, number, number];

/** Center + size in normalized canvas coordinates, y up. */
export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}
