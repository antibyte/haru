#version 300 es
precision highp float;
precision highp sampler2D;
uniform sampler2D uDye;
uniform sampler2D uBloom;
uniform sampler2D uRays;
uniform vec2 uDyeTexel;
uniform vec2 uResolution;   // drawing buffer size in px
uniform float uAspect;
uniform float uTime;
uniform float uRms;
uniform float uExposure;
uniform sampler2D uHead;    // head G-buffer: rg = view normal xy, b = coverage/depth, a = mouth cavity (textured: exact)
uniform float uHeadAmt;     // presence 0..1 (0 = no face drawn)
uniform vec3 uHeadLight;    // view-space light direction
uniform sampler2D uInk;     // blurred dye, coarse (wide fog glow)
uniform sampler2D uInkNear; // blurred dye, dye/8 (local ink)
uniform vec2 uHeadTexel;    // G-buffer texel in UV
uniform vec2 uFocus;        // face centre (eyes-nose-mouth) in UV
uniform float uFaceSize;    // head height in UV y
uniform float uHeadGain;    // per-head presence normalisation
uniform sampler2D uHeadAlb; // head G-buffer albedo (rgb linear, a = textured coverage); only read when uHeadTextured > 0.5
uniform float uHeadTextured; // 0 = porcelain (the original math, exactly), 1 = textured head (real albedo)
uniform float uHeadVisMax;  // visibility cap for textured heads (porcelain keeps 0.28)
// per-head skin grade of textured heads (HEAD_GRADE in heads/face.ts): skin = pow(albedo, gamma) * lift,
// then mix(lum(skin) * tint, skin, saturation). Never evaluated for porcelain heads.
uniform float uSkinGamma;
uniform float uSkinLift;
uniform vec3 uSkinTint;
uniform float uSkinSat;
uniform float uEmber;       // 0 = neutral look (exactly as before), 1 = daemon embers
uniform float uHeadDebug;   // ?facedebug: show the whole face (no ink gate / dissolve)
in vec2 vUv;
out vec4 o;

float lum(vec3 c) { return dot(c, vec3(0.299, 0.587, 0.114)); }
// compressed "height" of the liquid surface derived from ink brightness
float height(vec2 uv) {
  float l = lum(texture(uDye, uv).rgb);
  return l / (1.0 + 0.6 * l);
}
vec3 aces(vec3 x) {
  const float a = 2.51, b = 0.03, c = 2.43, d = 0.59, e = 0.14;
  return clamp((x * (a * x + b)) / (x * (c * x + d) + e), 0.0, 1.0);
}
float vnoise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  float a = fract(sin(dot(i, vec2(127.1, 311.7))) * 43758.5453);
  float b = fract(sin(dot(i + vec2(1.0, 0.0), vec2(127.1, 311.7))) * 43758.5453);
  float c = fract(sin(dot(i + vec2(0.0, 1.0), vec2(127.1, 311.7))) * 43758.5453);
  float d = fract(sin(dot(i + vec2(1.0, 1.0), vec2(127.1, 311.7))) * 43758.5453);
  return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}
float fbm(vec2 p) {
  float v = 0.0, a = 0.5;
  for (int i = 0; i < 4; i++) { v += a * vnoise(p); p = p * 2.03 + vec2(17.1, 9.7); a *= 0.5; }
  return v;
}
float hash12(vec2 p) {
  vec3 p3 = fract(vec3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}
// smooth, non-boxy fbm for the dissolve mask: rotated octaves, 3 octaves
float fbmR(vec2 p) {
  const mat2 R = mat2(0.8, 0.6, -0.6, 0.8);
  float v = 0.0, a = 0.55;
  for (int i = 0; i < 3; i++) { v += a * vnoise(p); p = R * p * 2.07 + vec2(3.1, 7.3); a *= 0.5; }
  return v;
}
// ink glow around a point at a large scale (coarsest blurred dye level): the fog around the face
vec3 inkWide(vec2 uv) { return texture(uInk, uv).rgb; }
// local ink around a point (dye/8, linear): lights and gates the face
vec3 inkBlur(vec2 uv) {
  // 4 diagonal bilinear taps (+ centre) hide the coarse texel grid of the low-res level
  vec2 e = 0.9 / vec2(textureSize(uInkNear, 0));
  return (texture(uInkNear, uv).rgb * 2.0
    + texture(uInkNear, uv + vec2(e.x, e.y)).rgb + texture(uInkNear, uv + vec2(-e.x, e.y)).rgb
    + texture(uInkNear, uv + vec2(e.x, -e.y)).rgb + texture(uInkNear, uv + vec2(-e.x, -e.y)).rgb) / 6.0;
}

void main() {
  vec2 uv = vUv;
  // --- head G-buffer (glass head in fog): coverage, normal and refraction of the dye lookup inside the silhouette
  vec4 hg = vec4(0.0);
  float cov = 0.0;
  float cavity = 0.0;          // textured heads: how much the open mouth hides the light behind it (0 elsewhere)
  vec3 hn = vec3(0.0, 0.0, 1.0);
  if (uHeadAmt > 0.002) {
    hg = texture(uHead, uv);
    // 3x3 tent over ~2.5 G-texel steps: a soft, anti-aliased silhouette; normals averaged where there is geometry
    vec2 st = uHeadTexel * 2.5;
    float wsum = 0.0, csum = 0.0;
    vec2 nsum = vec2(0.0);
    for (int j = -1; j <= 1; j++) {
      for (int i = -1; i <= 1; i++) {
        float w = (i == 0 ? 2.0 : 1.0) * (j == 0 ? 2.0 : 1.0);
        vec4 s = texture(uHead, uv + vec2(float(i), float(j)) * st);
        float c = smoothstep(0.0, 0.25, s.b);
        csum += w * c;
        wsum += w;
        nsum += w * c * s.rg;
        if (i == 0 && j == 0) hg = s;
      }
    }
    cov = csum / wsum;
    vec2 nxy = nsum / max(csum, 1e-4);
    hn = vec3(nxy, sqrt(max(1.0 - dot(nxy, nxy), 0.0)));
  }
  vec2 ca = (uv - 0.5) * (0.0015 + 0.004 * uRms);

  // --- liquid-glass normal from the ink luminance gradient
  vec2 t = uDyeTexel * 3.0;
  float hl = height(uv - vec2(t.x, 0.0));
  float hr = height(uv + vec2(t.x, 0.0));
  float ht = height(uv + vec2(0.0, t.y));
  float hb = height(uv - vec2(0.0, t.y));
  vec2 grad = vec2(hr - hl, ht - hb);
  vec3 n = normalize(vec3(-grad * 1.5, 1.0));

  // glyph protection: 0 in free fog, 1 on bright ink and its immediate surroundings (sharp + two blurred rings of the
  // dye luminance). The face's refraction and light fade out here, so words stay as crisp as outside the head.
  float inkMask = 0.0;
  if (cov > 0.002) {
    float lS = lum(texture(uDye, uv).rgb);
    float l1 = 0.0, l2 = 0.0;
    for (int i = 0; i < 8; i++) {
      float a = float(i) * 0.7853982 + 0.2;
      vec2 d = vec2(cos(a), sin(a)) * uDyeTexel;
      l1 += lum(texture(uDye, uv + d * 6.0).rgb);
      l2 += lum(texture(uDye, uv + d * 15.0).rgb);
    }
    inkMask = smoothstep(0.015, 0.2, max(lS, max(l1 * 0.2, l2 * 0.2)));
  }
  // slight refraction through the surface + chromatic aberration on R / B
  vec2 refr = grad * 0.007 + hn.xy * hn.z * (0.006 * cov * uHeadAmt * (1.0 - inkMask)) * vec2(1.0 / uAspect, 1.0);
  vec3 dye;
  dye.r = texture(uDye, uv + refr + ca).r;
  dye.g = texture(uDye, uv + refr).g;
  dye.b = texture(uDye, uv + refr - ca).b;
  float l = lum(dye);

  vec3 L = normalize(vec3(-0.4, 0.6, 0.7));
  float ndotl = max(dot(n, L), 0.0);
  vec3 col = dye * (0.85 + 0.15 * ndotl);

  vec3 hv = normalize(L + vec3(0.0, 0.0, 1.0));
  float spec = pow(max(dot(n, hv), 0.0), 40.0) * 0.28;
  float presence = smoothstep(0.03, 0.35, l);
  vec3 tint = mix(vec3(1.0), dye / max(max(dye.r, max(dye.g, dye.b)), 1e-3), 0.25);
  col += spec * presence * tint * 3.0;
  // fresnel-like rim on steep slopes
  float rim = pow(1.0 - n.z, 2.0);
  col += dye * rim * 0.22;

  // --- Ein Gesicht im Nebel: the porcelain face emerges only where ink flows over it
  if (cov > 0.002) {
    vec3 L = normalize(uHeadLight);
    float ndl = dot(hn, L);
    vec4 alb = vec4(0.0);
    if (uHeadTextured > 0.5) alb = texture(uHeadAlb, uv);         // same uv as the G-buffer lookup (hg)
    float texd = uHeadTextured * alb.a;
    float wrapP = clamp((ndl + 0.55) / 1.55, 0.0, 1.0);           // soft wrapped Lambert
    float wrapT = clamp((ndl + 0.15) / 1.15, 0.0, 1.0);           // harder terminator for skin
    float wrap = mix(wrapP, wrapT, texd);
    float sss = exp(-(ndl - 0.1) * (ndl - 0.1) * 9.0);                  // warm subsurface glow around the terminator
    float fres = pow(max(1.0 - hn.z, 0.0), 2.4) * smoothstep(0.0, 0.35, hn.z); // rim, faded at grazing angles
    vec3 ink = inkBlur(uv);                                       // local ink: the only thing that reveals the face
    vec3 fog = inkWide(uv);                                       // wide glow: faint, never gates
    float inkR = min(lum(ink), 0.8);
    float inkS = 1.0 - exp(-inkR * 16.0);                        // saturating ink presence 0..1 (dim ink counts)
    float fogS = 1.0 - exp(-lum(fog) * 16.0);
    vec3 inkF = ink + 0.5 * fog;
    vec3 tint = mix(vec3(1.0), inkF / max(max(inkF.r, max(inkF.g, inkF.b)), 1e-3), 0.55);
    // slow fbm dissolve: only patches show, drifting independently of the head
    vec2 fp = (uv - 0.5) * vec2(uAspect, 1.0);
    float dn = fbmR(fp * 1.7 + vec2(uTime * 0.021, -uTime * 0.015));
    float dn2 = fbmR(fp * 5.5 + vec2(-uTime * 0.03, uTime * 0.02) + 4.7);
    // the outline dissolves into the fog: broken by noise, thinner toward grazing angles
    cov *= smoothstep(0.02, 0.6, hn.z);
    cov = smoothstep(0.1, 0.9, cov + (dn2 - 0.5) * 0.45);
    float dissolve = smoothstep(0.38 - 0.06 * uHeadAmt, 0.60 - 0.06 * uHeadAmt, dn);
    float gate = smoothstep(0.03, 0.6, inkS + 0.7 * fogS);                  // needs local ink to be lit
    float words = 1.0 - smoothstep(0.15, 0.6, lum(dye));           // words stay the brightest thing
    // emphasis on eyes - nose - mouth; crown, back of the head, neck and shoulders fade into the dark
    vec2 q = vec2((uv.x - uFocus.x) * uAspect, uv.y - uFocus.y) / uFaceSize;
    float qy = q.y > 0.0 ? q.y / 0.30 : q.y / 0.26;
    float region = exp(-(q.x * q.x / 0.075 + qy * qy));
    float visCap = mix(0.28, uHeadVisMax, uHeadTextured);
    float vis = min(visCap, visCap * uHeadAmt * cov * region * mix(0.35, 1.0, smoothstep(0.12, 0.55, hn.z)) * mix(0.25, 1.0, dissolve) * mix(0.12, 1.0, gate) * words * (1.0 - inkMask) * uHeadGain);
    if (uHeadDebug > 0.5) vis = 0.4 * cov;
    vec3 porcelain = vec3(0.96, 0.92, 0.88);
    // textured heads: the real albedo (un-premultiplied against the filtered G-buffer edge), graded per head
    // (daemon: dark creature skin lifted so ink can reveal it, pulled toward red-grey; angels: natural skin).
    // texd == 0 (porcelain heads, outside textured geometry) skips it: the porcelain colour stays exactly as it was.
    if (texd > 0.0) {
      vec3 skin = pow(max(alb.rgb / max(alb.a, 1e-3), 0.0), vec3(uSkinGamma)) * uSkinLift;
      skin = mix(vec3(dot(skin, vec3(0.299, 0.587, 0.114))) * uSkinTint, skin, uSkinSat);
      porcelain = mix(porcelain, skin, texd);
    }
    float shade = 0.1 + 0.9 * wrap;
    // relief: the ink itself is shaped by the face (lit side brighter, shadow side darker)
    col *= 1.0 + vis * 2.4 * max(shade - 0.3, 0.0);
    // porcelain lit by the ink
    vec3 lit = porcelain * tint * pow(max(0.08 + 0.92 * wrap, 0.0), 1.25) * (0.14 + 1.6 * inkS + 2.0 * inkR + 0.35 * uHeadDebug);
    lit += porcelain * fog * 1.2 * wrap;
    lit += vec3(1.0, 0.55, 0.4) * sss * 0.12 * tint * inkS * (1.0 - texd);
    // moist low specular for skin
    lit += texd * pow(max(dot(reflect(-L, hn), vec3(0.0, 0.0, 1.0)), 0.0), 18.0) * 0.25 * inkS * tint;
    lit += ink * fres * 3.0 * (0.3 + inkS);
    // additive but bounded: soft-saturate the face light so abundant ink can never turn the face into a bright solid
    vec3 add = lit * vis * 8.0;
    col += 0.4 * (1.0 - exp(-add / 0.4));
    // mouth cavity
    if (uHeadTextured > 0.5) {
      // textured heads: the G-buffer alpha is the exact inner mouth (head.frag: the pouch behind the lips at any jaw
      // opening, incl. the resting slit). Wherever the face shows, the cavity hides the fluid behind it: near-black
      // with a faint warm glow from the (smooth) local ink, never brighter than what it covers, no fluid structure.
      // 2x2 taps half a G-texel apart soften the stair-stepped outline of the low-res G-buffer (no halo on the lips).
      vec2 ht = 0.5 * uHeadTexel;
      float hm = 0.25 * (texture(uHead, uv + ht).a + texture(uHead, uv - ht).a
        + texture(uHead, uv + vec2(ht.x, -ht.y)).a + texture(uHead, uv + vec2(-ht.x, ht.y)).a);
      cavity = hm * smoothstep(0.0, 0.04, vis);
      vec3 deep = vec3(0.016, 0.006, 0.005) * (0.25 + inkS);
      col = mix(col, min(col, deep), cavity);
    } else {
      col *= 1.0 - 0.55 * hg.a * cov * smoothstep(0.0, 0.04, vis);
    }
  }

  // --- ambient nebula: a very faint deep-blue / teal haze so the frame is never dead black
  {
    vec2 hp = (uv - 0.5) * vec2(uAspect, 1.0) * 1.5;
    float t = uTime * 0.012;
    float n1 = fbm(hp * 1.3 + vec2(t, -0.7 * t));
    float n2 = fbm(hp * 2.1 + vec2(-0.8 * t, 0.5 * t) + 11.3);
    float a = smoothstep(0.38, 0.8, n1);
    float b = smoothstep(0.42, 0.85, n2);
    vec3 hazeA = mix(vec3(0.10, 0.28, 0.75) * 0.012, vec3(0.55, 0.10, 0.04) * 0.010, uEmber);
    vec3 hazeB = mix(vec3(0.05, 0.60, 0.55) * 0.006, vec3(0.45, 0.14, 0.03) * 0.005, uEmber);
    col += hazeA * a + hazeB * b;
  }

  // --- light: bloom + rays
  vec3 bloom = vec3(
    texture(uBloom, uv + ca).r,
    texture(uBloom, uv).g,
    texture(uBloom, uv - ca).b);
  // (the open mouth of a textured head also keeps the fluid's core and most of its glow and rays out; cavity = 0 for
  // porcelain heads and everywhere outside the mouth, so these lines are unchanged there)
  col += dye * dye * 0.26 * (1.0 - cavity); // luminous core: highlights rise faster than the tails
  col += bloom * 1.4 * (1.0 - 0.9 * cavity);
  col += texture(uRays, uv).rgb * (1.0 - 0.9 * cavity);

  // --- grade
  col *= uExposure;
  float g = lum(col);
  col = mix(vec3(g), col, 1.18);          // richer saturation before the shoulder
  col = aces(max(col, 0.0));

  vec2 d = (uv - 0.5) * vec2(uAspect, 1.0);
  col *= mix(1.0, smoothstep(0.85, 0.25, length(d)), 0.35);

  // film grain (scaled so deep blacks stay deep)
  float n1 = hash12(gl_FragCoord.xy + fract(uTime) * 91.7);
  float lg = lum(col);
  col += (n1 - 0.5) * 0.015 * (0.1 + smoothstep(0.0, 0.25, lg));

  col = pow(max(col, 0.0), vec3(1.0 / 2.2));
  col += (hash12(gl_FragCoord.xy * 1.37 + fract(uTime * 0.7) * 53.1) - 0.5) / 255.0;
  o = vec4(col, 1.0);
}
