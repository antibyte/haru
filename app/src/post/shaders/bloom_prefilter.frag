#version 300 es
precision highp float;
precision highp sampler2D;
uniform sampler2D uSource;
uniform vec2 uTexel;       // source (dye) texel
uniform float uThreshold;  // 0.6
uniform float uKnee;       // 0.3
in vec2 vUv;
out vec4 o;
vec3 knee(vec3 c) {
  float br = max(c.r, max(c.g, c.b));
  float rq = clamp(br - uThreshold + uKnee, 0.0, 2.0 * uKnee);
  rq = rq * rq / (4.0 * uKnee + 1e-4);
  float w = max(rq, br - uThreshold) / max(br, 1e-4);
  return c * w;
}
void main() {
  // 4 bilinear taps = 4x4 source footprint for the half-res output
  vec3 c = knee(texture(uSource, vUv + uTexel * vec2(-1.0, -1.0)).rgb)
         + knee(texture(uSource, vUv + uTexel * vec2( 1.0, -1.0)).rgb)
         + knee(texture(uSource, vUv + uTexel * vec2(-1.0,  1.0)).rgb)
         + knee(texture(uSource, vUv + uTexel * vec2( 1.0,  1.0)).rgb);
  o = vec4(max(c * 0.25, 0.0), 1.0);
}
