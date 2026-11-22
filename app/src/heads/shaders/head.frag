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
uniform vec2 uCavityUv;         // UV of the inner-mouth pouch faces (heads/cavity.ts), x < 0 = none
layout(location = 0) out vec4 o;
layout(location = 1) out vec4 oAlb;   // rgb = albedo (linear), a = 1 on textured geometry; 0 otherwise
// G-buffer: rg = view-space normal xy, b = 0.35 + 0.65 * nearness (0 = no geometry), a = mouth-cavity mask
void main() {
  vec3 n = normalize(vN);
  float near = clamp(1.0 - (vViewZ - (uDepth.x - uDepth.y)) / (2.0 * uDepth.y), 0.0, 1.0);
  float mouth;
  if (uHasTex > 0.5) {
    // textured heads: the exact cavity. Every inner-mouth pouch face maps to the one point uCavityUv (a reserved
    // atlas cell no skin UV covers), so the interpolated UV equals it there and only there: 1 on the pouch visible
    // through the lips at any jaw opening (incl. the resting slit), 0 on lips and skin. The loose box around the
    // mouth landmark only guards against unrelated faces that might share that UV.
    vec2 e = abs(vUv - uCavityUv);
    vec3 m = (vObj - uMouth) / vec3(0.35, 0.25, 0.4);
    mouth = (uCavityUv.x >= 0.0 && max(e.x, e.y) < 1e-4 && dot(m, m) < 1.0) ? 1.0 : 0.0;
  } else {
    // porcelain heads (unchanged): a soft blob around the mouth landmark, proportional to the jaw opening
    vec2 d = vec2((vObj.x - uMouth.x) / 0.07, (vObj.y - (uMouth.y - 0.03 * uJaw)) / (0.006 + 0.03 * uJaw));
    float front = smoothstep(-0.14, -0.02, vObj.z - uMouth.z);
    mouth = uJaw * exp(-dot(d, d)) * front;
  }
  o = vec4(n.xy, 0.35 + 0.65 * near, mouth);
  oAlb = uHasTex > 0.5 ? vec4(texture(uTex, vUv).rgb, 1.0) : vec4(0.0);
}
