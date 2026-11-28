/** A DOMRect-like box in CSS px (viewport coordinates). */
export interface RectLike { readonly left: number; readonly top: number; readonly width: number; readonly height: number }
/** The chooser's boxes the arrows are computed from. */
export interface VsRects { vs: RectLike; angel: RectLike; daemon: RectLike }
/** A point in CSS px relative to the canvas' top-left (y down). */
export type Pt = readonly [number, number];
/** One arrow: from the VS rim toward `target` (an image centre); `to` is the drawn tip; `width` the shaft width (CSS px). */
export interface ArrowGeom { from: Pt; to: Pt; target: Pt; width: number }
/** Both arrows plus the canvas size (CSS px) they are relative to. */
export interface ArrowPair { angel: ArrowGeom; daemon: ArrowGeom; canvasW: number; canvasH: number }

/** How far the tip reaches from the VS rim toward the target (1 = the target). */
export const ARROW_REACH = 0.8;
/** The arrows aim at the head: image centre x, this fraction of the image height below its top edge. */
export const ARROW_TARGET_Y = 0.2;
/** Side by side the images are opaque and lie above the canvas: the tip stays above this fraction of the image height
 *  (measured from its top edge), so the head of the arrow is in the open dark space and not hidden behind a figure. */
export const ARROW_TIP_CLEAR = 0.12;
/** Shortest shaft beyond the rim (fraction of the emblem radius), whatever the clearance says. */
export const ARROW_MIN_LEN = 0.7;
/** Shaft width as a fraction of the emblem size. */
export const ARROW_WIDTH = 0.14;
export const ARROW_BUILD_MS = 400;
export const ARROW_FADE_MS = 1100;

const centre = (r: RectLike, o: RectLike): [number, number] => [r.left + r.width / 2 - o.left, r.top + r.height / 2 - o.top];
const headPoint = (r: RectLike, o: RectLike): [number, number] => [r.left + r.width / 2 - o.left, r.top + ARROW_TARGET_Y * r.height - o.top];

/** Gold arrow toward the angel, red toward the daemon: both start at the VS rim and aim at the image centres. */
export function arrowGeometry(r: VsRects, canvas: RectLike): ArrowPair {
  const c = centre(r.vs, canvas);
  const radius = Math.min(r.vs.width, r.vs.height) / 2;
  const width = ARROW_WIDTH * 2 * radius;
  const stacked = r.daemon.top >= r.angel.top + r.angel.height / 2;
  const one = (img: RectLike): ArrowGeom => {
    const target = headPoint(img, canvas);
    const dx = target[0] - c[0];
    const dy = target[1] - c[1];
    const len = Math.hypot(dx, dy);
    if (len < 1e-6) return { from: target, to: target, target, width };
    const ux = dx / len;
    const uy = dy / len;
    const start = Math.min(radius, len);
    let tip = start + ARROW_REACH * (len - start);
    if (!stacked && dy > 0) {
      const yMax = img.top - canvas.top + ARROW_TIP_CLEAR * img.height; // the tip stays above the figure
      if (c[1] + uy * tip > yMax) tip = Math.max(start + ARROW_MIN_LEN * radius, (yMax - c[1]) / uy);
    }
    return { from: [c[0] + ux * start, c[1] + uy * start], to: [c[0] + ux * tip, c[1] + uy * tip], target, width };
  };
  return { angel: one(r.angel), daemon: one(r.daemon), canvasW: canvas.width, canvasH: canvas.height };
}

/** The arrows for the current layout, or null (nothing laid out yet). */
export function arrowsFor(r: VsRects | null, canvas: RectLike): ArrowPair | null {
  if (!r || canvas.width <= 0 || canvas.height <= 0 || r.vs.width <= 0 || r.vs.height <= 0) return null;
  return arrowGeometry(r, canvas);
}

/** Arrow outline (CSS px, y down): a tapered shaft (0.6 → 1 x width) and a head 2.6 x as wide, tip at `to`. */
export function arrowPolygon(from: Pt, to: Pt, width: number): Pt[] {
  const dx = to[0] - from[0];
  const dy = to[1] - from[1];
  const len = Math.hypot(dx, dy) || 1;
  const ux = dx / len;
  const uy = dy / len;
  const nx = -uy;
  const ny = ux;
  const head = Math.min(0.4 * len, 2.4 * width);
  const neck: Pt = [to[0] - ux * head, to[1] - uy * head];
  const at = (p: Pt, k: number): Pt => [p[0] + nx * k, p[1] + ny * k];
  return [at(from, 0.3 * width), at(neck, 0.5 * width), at(neck, 1.3 * width), to, at(neck, -1.3 * width), at(neck, -0.5 * width), at(from, -0.3 * width)];
}

/** 0..1: rises over ARROW_BUILD_MS while active, falls over ARROW_FADE_MS after release. */
export function arrowEnvelope(env: number, active: boolean, dtMs: number): number {
  const k = Math.max(0, dtMs);
  return active ? Math.min(1, env + k / ARROW_BUILD_MS) : Math.max(0, env - k / ARROW_FADE_MS);
}
