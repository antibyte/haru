export interface GlyphBitmap {
  texture: WebGLTexture;
  width: number;
  height: number;
  /** width / height of the bitmap (including padding) */
  aspect: number;
  texel: [number, number];
  text: string;
  /** Height of the bitmap in em (font px): multiply the wanted em height to get the box height. */
  heightEm: number;
}

const CACHE_SIZE = 64;
/** Refined display weight (Inter Variable supports the continuous range 100..900). */
const WEIGHT = 330;
const LETTER_SPACING_EM = 0.045;
export const PAD_EM = 0.38;
const LINE_EM = 1.4;
const FONT_STACK =
  '"Inter Variable", "Segoe UI", "Yu Gothic UI", "Meiryo", "Malgun Gothic", "Microsoft YaHei", "Noto Sans", "PingFang SC", "Hiragino Sans", sans-serif';

/** Renders words into soft-edged R8 glyph masks (white text with an outer glow) and caches them (LRU). */
export class GlyphRasterizer {
  private readonly cache = new Map<string, GlyphBitmap>();
  private readonly canvas = new OffscreenCanvas(8, 8);

  constructor(private readonly gl: WebGL2RenderingContext, private readonly fontPx = 128) {}

  async ready(): Promise<void> {
    try {
      await document.fonts.load(`${WEIGHT} ${this.fontPx}px "Inter Variable"`);
    } catch {
      /* fall back to system fonts */
    }
  }

  rasterize(text: string): GlyphBitmap {
    const hit = this.cache.get(text);
    if (hit) {
      this.cache.delete(text);
      this.cache.set(text, hit);
      return hit;
    }
    const bmp = this.render(text);
    this.cache.set(text, bmp);
    while (this.cache.size > CACHE_SIZE) {
      const oldest = this.cache.keys().next().value as string;
      this.gl.deleteTexture(this.cache.get(oldest)!.texture);
      this.cache.delete(oldest);
    }
    return bmp;
  }

  dispose(): void {
    for (const b of this.cache.values()) this.gl.deleteTexture(b.texture);
    this.cache.clear();
  }

  private render(text: string): GlyphBitmap {
    const gl = this.gl;
    const px = this.fontPx;
    const pad = Math.round(PAD_EM * px);
    const c = this.canvas;
    const ctx = c.getContext('2d', { willReadFrequently: true })!;
    const font = `${WEIGHT} ${px}px ${FONT_STACK}`;
    const ls = `${(LETTER_SPACING_EM * px).toFixed(2)}px`;
    const style = ctx as unknown as { letterSpacing: string };
    ctx.font = font;
    style.letterSpacing = ls;
    const w = Math.max(8, Math.ceil(ctx.measureText(text).width + 2 * pad));
    const h = Math.ceil(LINE_EM * px + 2 * pad);
    c.width = w;
    c.height = h;
    ctx.font = font;
    style.letterSpacing = ls;
    ctx.fillStyle = '#000';
    ctx.fillRect(0, 0, w, h);
    ctx.textBaseline = 'middle';
    // wide, faint glow: the ink condenses into the letterforms
    ctx.shadowColor = 'rgba(255,255,255,0.85)';
    ctx.shadowBlur = 0.22 * px;
    ctx.fillStyle = 'rgba(255,255,255,0.22)';
    ctx.fillText(text, pad + (LETTER_SPACING_EM * px) / 2, h / 2);
    // tight halo + crisp letterforms
    ctx.shadowBlur = 0.05 * px;
    ctx.fillStyle = '#fff';
    ctx.fillText(text, pad + (LETTER_SPACING_EM * px) / 2, h / 2);
    const img = ctx.getImageData(0, 0, w, h).data;
    const r = new Uint8Array(w * h);
    for (let i = 0; i < w * h; i++) r[i] = img[i * 4];

    const tex = gl.createTexture()!;
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, w, h, 0, gl.RED, gl.UNSIGNED_BYTE, r);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    return { texture: tex, width: w, height: h, aspect: w / h, texel: [1 / w, 1 / h], text, heightEm: h / px };
  }
}
