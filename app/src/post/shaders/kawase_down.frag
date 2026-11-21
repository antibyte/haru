#version 300 es
precision highp float;
precision highp sampler2D;
uniform sampler2D uSource;
uniform vec2 uTexel;   // source texel
in vec2 vUv;
out vec4 o;
void main() {
  vec4 s = texture(uSource, vUv) * 4.0;
  s += texture(uSource, vUv + uTexel * vec2(-1.0, -1.0));
  s += texture(uSource, vUv + uTexel * vec2( 1.0, -1.0));
  s += texture(uSource, vUv + uTexel * vec2(-1.0,  1.0));
  s += texture(uSource, vUv + uTexel * vec2( 1.0,  1.0));
  o = s * 0.125;
}
