export interface GLContext {
  gl: WebGL2RenderingContext;
  canvas: HTMLCanvasElement;
  maxTextureSize: number;
  /** EXT_disjoint_timer_query_webgl2 or null */
  timer: unknown | null;
}

/** Creates the WebGL2 context. Requires EXT_color_buffer_float (half-float render targets). */
export function createGL(canvas: HTMLCanvasElement): GLContext {
  const gl = canvas.getContext('webgl2', {
    alpha: false,
    antialias: false,
    depth: false,
    stencil: false,
    premultipliedAlpha: false,
    preserveDrawingBuffer: false,
    powerPreference: 'high-performance',
  });
  if (!gl) throw new Error('WEBGL2_UNAVAILABLE');
  if (!gl.getExtension('EXT_color_buffer_float')) throw new Error('COLOR_BUFFER_FLOAT_UNAVAILABLE');
  // Optional, harmless when missing (we only filter half floats, which is core).
  gl.getExtension('EXT_color_buffer_half_float');
  const timer = gl.getExtension('EXT_disjoint_timer_query_webgl2');
  gl.disable(gl.DEPTH_TEST);
  gl.disable(gl.CULL_FACE);
  gl.disable(gl.BLEND);
  gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
  return {
    gl,
    canvas,
    maxTextureSize: gl.getParameter(gl.MAX_TEXTURE_SIZE) as number,
    timer,
  };
}
