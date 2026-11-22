#version 300 es
precision highp float;
precision highp sampler2D;
layout(location = 0) in vec3 aPos;
layout(location = 1) in vec3 aNormal;
layout(location = 2) in vec2 aUv;  // TEXCOORD_0 (glTF origin top-left), (0,0) when the file has none
uniform sampler2D uMorph;       // RGBA32F: texel (vertex * 8 + target) * 2 + {0: dPosition, 1: dNormal}
uniform float uW[8];            // morph weights in contract order
uniform mat4 uMVP;
uniform mat4 uMV;
out vec3 vN;                    // view-space normal
out vec3 vObj;                  // morphed object-space position (for the per-pixel mouth mask)
out float vViewZ;               // view-space distance along -z
out vec2 vUv;
void main() {
  vec3 p = aPos;
  vec3 n = aNormal;
  for (int t = 0; t < 8; t++) {
    float w = uW[t];
    if (w > 0.002) {
      int base = (gl_VertexID * 8 + t) * 2;
      p += w * texelFetch(uMorph, ivec2(base & 2047, base >> 11), 0).xyz;
      n += w * texelFetch(uMorph, ivec2((base + 1) & 2047, (base + 1) >> 11), 0).xyz;
    }
  }
  vObj = p;
  vUv = aUv;
  vN = mat3(uMV) * n;
  vec4 v = uMV * vec4(p, 1.0);
  vViewZ = -v.z;
  gl_Position = uMVP * vec4(p, 1.0);
}
