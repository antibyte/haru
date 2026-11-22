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
  /** 1 = ember ageing (amber to crimson to ash red, no violet), 0 = neutral. Set per persona. */
  ember: number;
}

/** CPU mirror of the dye ageing in advect.frag (per step, k = ageing * dt). Used by tests. */
export function ageInk(c: RGB, k: number, ember: number): RGB {
  const b = c[2] + c[1] * 0.55 * k * (1 - ember);
  const mix = (a: number, e: number) => a + (e - a) * ember;
  return [
    c[0] * Math.exp(-mix(0.75, 0.5) * k),
    c[1] * Math.exp(-mix(1.5, 1.7) * k),
    b * Math.exp(-mix(0.32, 2.5) * k),
  ];
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
  ember: 0,
};

export type RGB = [number, number, number];

/** Center + size in normalized canvas coordinates, y up. */
export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}
