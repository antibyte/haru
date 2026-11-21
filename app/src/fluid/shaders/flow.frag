#version 300 es
precision highp float;
precision highp sampler2D;
// Persistent large-scale divergence-free flow: velocity = curl of a slowly evolving stream function.
uniform float uTime;
uniform float uAspect;
uniform float uGain;      // cells/s gained per second at unit field strength (already multiplied by dt)
in vec2 vUv;
out vec4 o;
float psi(vec2 p, float t) {
  float a = sin(1.7 * p.x + 0.9 * sin(1.3 * p.y + 0.13 * t) + 0.10 * t) * sin(2.1 * p.y + 0.8 * sin(1.1 * p.x - 0.11 * t) - 0.08 * t);
  float b = sin(3.3 * p.x - 0.16 * t + 1.2 * p.y) * sin(3.0 * p.y + 0.13 * t - 0.9 * p.x);
  return 0.62 * a + 0.20 * b;
}
void main() {
  vec2 p = vec2(vUv.x * uAspect, vUv.y);
  const float e = 0.01;
  float dpx = psi(p + vec2(e, 0.0), uTime) - psi(p - vec2(e, 0.0), uTime);
  float dpy = psi(p + vec2(0.0, e), uTime) - psi(p - vec2(0.0, e), uTime);
  vec2 v = vec2(dpy, -dpx) / (2.0 * e);
  o = vec4(v * uGain, 0.0, 0.0);
}
