#version 300 es
precision highp float;
precision highp sampler2D;
uniform sampler2D uTexture;
in vec2 vUv;
out vec4 o;
void main() { o = texture(uTexture, vUv); }
