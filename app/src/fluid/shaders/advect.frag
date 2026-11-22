#version 300 es
precision highp float;
precision highp sampler2D;
uniform sampler2D uVelocity;
uniform sampler2D uSource;
uniform sampler2D uRest;
uniform vec2 uTexel;          // velocity-grid texel (velocity is in grid cells / second)
uniform float uDt;
uniform float uDissipation;
uniform float uRestStrength;
uniform float uIsDye;
uniform float uAgeing;       // colour ageing speed multiplier
uniform float uEmber;        // 0 = neutral ageing (gold -> magenta -> violet), 1 = ember ageing (amber -> crimson -> ash red)
uniform float uSeed;         // per-step seed for the stochastic rounding below
in vec2 vUv;
out vec4 o;
float hash13(vec3 p3) {
  p3 = fract(p3 * 0.1031);
  p3 += dot(p3, p3.zyx + 31.32);
  return fract((p3.x + p3.y) * p3.z);
}
void main() {
  vec2 vel = texture(uVelocity, vUv).xy;
  float rest = texture(uRest, vUv).r;
  vec2 coord = vUv - uDt * vel * uTexel * (1.0 - uRestStrength * clamp(rest, 0.0, 1.0));
  vec4 r = texture(uSource, coord) / (1.0 + uDissipation * uDt);
  // words are calm islands: the rest field also damps the velocity inside the letterforms
  if (uIsDye < 0.5) r.xy *= 1.0 - 0.8 * uRestStrength * clamp(rest, 0.0, 1.0);
  if (any(isnan(r)) || any(isinf(r))) r = vec4(0.0);
  if (uIsDye > 0.5) {
    // Hue-preserving soft ceiling: keeps the HDR energy bounded so overlapping ink never runs away.
    float m = max(r.r, max(r.g, r.b));
    const float lim = 3.0;
    const float cap = 7.0;
    if (m > lim) {
      float mm = lim + (cap - lim) * tanh((m - lim) / (cap - lim));
      r.rgb *= mm / m;
    }
    r.rgb = max(r.rgb, 0.0);
    // Ageing: ink cools from hot gold/white through magenta into deep violet as it fades.
    // green is converted partly into blue and dies first, red follows, blue lives longest.
    float k = uAgeing * uDt;
    // ember: no green->blue conversion, blue dies fast, red lives longest (hue stays 0..40 degrees, never violet)
    r.b += r.g * 0.55 * k * (1.0 - uEmber);
    r.g *= exp(-mix(1.5, 1.7, uEmber) * k);
    r.r *= exp(-mix(0.75, 0.5, uEmber) * k);
    r.b *= exp(-mix(0.32, 2.5, uEmber) * k);
    // fp16 storage stalls: at very short steps (k ~ 1e-3, i.e. >500 fps) the per-step decay of a channel is below half
    // an ulp, so round-to-nearest returns the old value and that channel never fades (single-channel specks).
    // Adding +-0.5 ulp of noise before the store makes the rounding unbiased (stochastic), so the decay averages out.
    vec3 ulp = exp2(floor(log2(max(r.rgb, vec3(1e-6)))) - 10.0);
    vec3 h = vec3(hash13(vec3(gl_FragCoord.xy, uSeed)), hash13(vec3(gl_FragCoord.yx + 17.0, uSeed + 3.0)), hash13(vec3(gl_FragCoord.xy * 1.3 + 5.0, uSeed + 7.0)));
    r.rgb = max(r.rgb + (h - 0.5) * ulp, 0.0);
  }
  o = r;
}
