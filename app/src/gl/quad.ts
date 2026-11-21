const vaos = new WeakMap<WebGL2RenderingContext, WebGLVertexArrayObject>();

interface Sized {
  readonly fbo: WebGLFramebuffer;
  readonly width: number;
  readonly height: number;
}

/**
 * Draws one fullscreen triangle (vertices come from gl_VertexID, no buffers).
 * `target` null = default framebuffer; `viewport` then defaults to the drawing buffer size.
 */
export function drawQuad(gl: WebGL2RenderingContext, target: Sized | null, viewport?: [number, number]): void {
  let vao = vaos.get(gl);
  if (!vao) {
    vao = gl.createVertexArray()!;
    vaos.set(gl, vao);
  }
  gl.bindFramebuffer(gl.FRAMEBUFFER, target ? target.fbo : null);
  const w = target ? target.width : viewport ? viewport[0] : gl.drawingBufferWidth;
  const h = target ? target.height : viewport ? viewport[1] : gl.drawingBufferHeight;
  gl.viewport(0, 0, w, h);
  gl.bindVertexArray(vao);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
}
