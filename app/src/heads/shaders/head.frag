#version 300 es
precision highp float;
in vec3 vN;
in vec3 vObj;
in float vViewZ;
in vec2 vUv;
uniform sampler2D uTex;         // base-colour albedo (sRGB8, mipmapped), only sampled when uHasTex > 0.5
uniform float uHasTex;
uniform vec2 uDepth;            // (camera distance, half range)
uniform vec3 uMouth;            // mouth landmark (object space, unmorphed)
uniform float uJaw;             // jawOpen weight
layout(location = 0) out vec4 o;
layout(location = 1) out vec4 oAlb;   // rgb = albedo (linear), a = 1 on textured geometry; 0 otherwise
// G-buffer: rg = view-space normal xy, b = 0.35 + 0.65 * nearness (0 = no geometry), a = mouth-cavity mask
void main() {
  vec3 n = normalize(vN);
  float near = clamp(1.0 - (vViewZ - (uDepth.x - uDepth.y)) / (2.0 * uDepth.y), 0.0, 1.0);
  vec2 d = vec2((vObj.x - uMouth.x) / 0.07, (vObj.y - (uMouth.y - 0.03 * uJaw)) / (0.006 + 0.03 * uJaw));
  float front = smoothstep(-0.14, -0.02, vObj.z - uMouth.z);
  float mouth = uJaw * exp(-dot(d, d)) * front;
  o = vec4(n.xy, 0.35 + 0.65 * near, mouth);
  oAlb = uHasTex > 0.5 ? vec4(texture(uTex, vUv).rgb, 1.0) : vec4(0.0);
}
