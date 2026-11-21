#version 300 es
precision highp float;
precision highp sampler2D;
uniform sampler2D uHead;   // head G-buffer: b > 0 inside the silhouette
uniform float uStrength;
in vec2 vUv;
out vec4 o;
void main() {
  float cov = smoothstep(0.0, 0.3, texture(uHead, vUv).b);
  o = vec4(0.03 * uStrength * cov, 0.0, 0.0, 0.0);
}
