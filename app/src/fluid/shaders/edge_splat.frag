#version 300 es
precision highp float;
precision highp sampler2D;
uniform sampler2D uMask;
uniform vec2 uMaskTexel;
uniform vec4 uBox;
uniform float uSwirl;
uniform float uPush;
in vec2 vUv;
out vec4 o;
float s(vec2 m) { return texture(uMask, vec2(m.x, 1.0 - m.y)).r; }
void main() {
  vec2 m = (vUv - (uBox.xy - 0.5 * uBox.zw)) / uBox.zw;
  if (m.x < 0.0 || m.x > 1.0 || m.y < 0.0 || m.y > 1.0) { o = vec4(0.0); return; }
  vec2 t = uMaskTexel * 1.5;
  // gradient in screen orientation (y up); points INTO the glyph, so outward normal = -g
  vec2 g = vec2(s(m + vec2(t.x, 0.0)) - s(m - vec2(t.x, 0.0)),
                -(s(m + vec2(0.0, t.y)) - s(m - vec2(0.0, t.y))));
  vec2 outward = -g;
  vec2 perp = vec2(-g.y, g.x);
  o = vec4(perp * uSwirl + outward * uPush, 0.0, 0.0);
}
