#version 300 es
precision highp float;
precision highp sampler2D;
uniform sampler2D uBloom;
uniform vec2 uLight;       // light position in uv
uniform int uSamples;
uniform float uDensity;    // 0.8
uniform float uDecay;      // 0.96 per 48 taps-equivalent step
uniform float uWeight;     // 0.3
uniform float uExposure;   // 0.25
uniform float uAspect;
in vec2 vUv;
out vec4 o;
float hash(vec2 p) {
  vec3 p3 = fract(vec3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}
void main() {
  vec2 delta = (vUv - uLight) * (uDensity / float(uSamples));
  vec2 uv = vUv - delta * hash(gl_FragCoord.xy);   // jitter hides step banding
  float illum = 1.0;
  float decay = pow(uDecay, 48.0 / float(uSamples));
  vec3 acc = vec3(0.0);
  for (int i = 0; i < 128; i++) {
    if (i >= uSamples) break;
    uv -= delta;
    acc += texture(uBloom, uv).rgb * illum * uWeight;
    illum *= decay;
  }
  // normalise by the tap count so quality levels look alike
  acc *= uExposure * (48.0 / float(uSamples)) * (1.0 / 30.0);
  o = vec4(acc, 1.0);
}
