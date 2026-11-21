#version 300 es
precision highp float;
precision highp sampler2D;
uniform vec2 uPoint;
uniform vec3 uColor;
uniform float uRadius;   // gaussian variance in (canvas-height)^2 units
uniform float uAspect;   // width / height
in vec2 vUv;
out vec4 o;
void main() {
  vec2 p = vUv - uPoint;
  p.x *= uAspect;
  float g = exp(-dot(p, p) / uRadius);
  o = vec4(g * uColor, 0.0);
}
