#version 300 es
precision highp float;
precision highp sampler2D;
uniform sampler2D uMask;
uniform vec4 uBox;
uniform vec3 uColor;
uniform float uStrength;
in vec2 vUv;
out vec4 o;
void main() {
  vec2 m = (vUv - (uBox.xy - 0.5 * uBox.zw)) / uBox.zw;
  if (m.x < 0.0 || m.x > 1.0 || m.y < 0.0 || m.y > 1.0) { o = vec4(0.0); return; }
  float k = texture(uMask, vec2(m.x, 1.0 - m.y)).r;
  o = vec4(uColor * (uStrength * k), 0.0);
}
