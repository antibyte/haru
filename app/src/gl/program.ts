function compile(gl: WebGL2RenderingContext, type: number, src: string, label: string): WebGLShader {
  const sh = gl.createShader(type);
  if (!sh) throw new Error('createShader failed');
  gl.shaderSource(sh, src);
  gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(sh) ?? '';
    gl.deleteShader(sh);
    throw new Error(`${label} shader compile failed:\n${log}\n${src.split('\n').map((l, i) => `${i + 1}: ${l}`).join('\n')}`);
  }
  return sh;
}

export class Program {
  readonly program: WebGLProgram;
  /** Missing (optimised-out) uniforms are simply absent; passing `undefined` to gl.uniform* is a no-op. */
  readonly uniforms: Record<string, WebGLUniformLocation>;
  private readonly gl: WebGL2RenderingContext;

  constructor(gl: WebGL2RenderingContext, vert: string, frag: string) {
    this.gl = gl;
    const vs = compile(gl, gl.VERTEX_SHADER, vert, 'vertex');
    const fs = compile(gl, gl.FRAGMENT_SHADER, frag, 'fragment');
    const p = gl.createProgram();
    if (!p) throw new Error('createProgram failed');
    gl.attachShader(p, vs);
    gl.attachShader(p, fs);
    gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
      const log = gl.getProgramInfoLog(p) ?? '';
      gl.deleteProgram(p);
      throw new Error(`program link failed: ${log}`);
    }
    gl.deleteShader(vs);
    gl.deleteShader(fs);
    this.program = p;
    const uniforms: Record<string, WebGLUniformLocation> = {};
    const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS) as number;
    for (let i = 0; i < n; i++) {
      const info = gl.getActiveUniform(p, i);
      if (!info) continue;
      const name = info.name.replace(/\[0\]$/, '');
      const loc = gl.getUniformLocation(p, name);
      if (loc) uniforms[name] = loc;
    }
    this.uniforms = uniforms;
  }

  bind(): void {
    this.gl.useProgram(this.program);
  }
}
