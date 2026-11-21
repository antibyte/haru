#version 300 es
precision highp float;
precision highp sampler2D;
uniform sampler2D uDye;
in vec2 vUv;
out vec4 o;
void main() {
  vec3 c = texture(uDye, vUv).rgb;
  c = c / (1.0 + c);
  o = vec4(pow(c, vec3(1.0 / 2.2)), 1.0);
}
