#version 300 es
precision highp float;
precision highp sampler2D;
uniform sampler2D uSource;
uniform vec2 uTexel;    // source (smaller) texel
uniform float uWeight;  // contribution of this level when added to the next larger one
in vec2 vUv;
out vec4 o;
void main() {
  vec2 h = uTexel * 0.5;
  vec4 s = texture(uSource, vUv + vec2(-2.0 * h.x, 0.0));
  s += texture(uSource, vUv + vec2(-h.x,  h.y)) * 2.0;
  s += texture(uSource, vUv + vec2(0.0,  2.0 * h.y));
  s += texture(uSource, vUv + vec2( h.x,  h.y)) * 2.0;
  s += texture(uSource, vUv + vec2( 2.0 * h.x, 0.0));
  s += texture(uSource, vUv + vec2( h.x, -h.y)) * 2.0;
  s += texture(uSource, vUv + vec2(0.0, -2.0 * h.y));
  s += texture(uSource, vUv + vec2(-h.x, -h.y)) * 2.0;
  o = vec4(s.rgb * (uWeight / 12.0), 0.0);
}
