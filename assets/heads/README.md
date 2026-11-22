# Haru heads – asset contract

Busts that appear as "Ein Gesicht im Nebel": Haru's face, large and frontal behind the fluid, revealed only where
ink flows over it and never fully visible. The angel persona shows a textured angel bust chosen by the voice:
`settings.gender === 'male'` selects `angel-male`, otherwise (female or unset) `angel-female`. The evil persona shows
the textured demon bust `daemon` (see below). The shipped heads are `angel-female`, `angel-male` and `daemon`
(`app/public/heads/*.glb`). The two sculptural porcelain busts (`female`, `male`) remain as untextured sources only
(`build_head.py` still supports them) and are no longer shipped or used at runtime.

## Pipeline

1. `<subject>/prompt.md` – the exact image prompt (written by Claude).
2. `<subject>/reference.png` – generated with the Codex CLI image tool.
3. `<subject>/varco-source.glb` – untouched VARCO 3D output (Generate3D,
   flow "Haru Heads"); provenance in `<subject>/generation.json`
   (method: `AuraGo/documentation/handoff-varco-3d.md`).
4. `build_head.py` (Blender 5.3, headless) – cleans, normalises and decimates
   the mesh, places facial landmarks, builds the morph targets below, saves
   `<subject>/source.blend` and exports the runtime asset.
5. Runtime asset: `app/public/heads/<subject>.glb`.

## Runtime GLB contract (`app/public/heads/<subject>.glb`)

- glTF 2.0 binary, one mesh named `Head` with one primitive:
  `POSITION`, `NORMAL`, `TEXCOORD_0`, `indices` (uint16 or uint32).
- Budget: at most 40,000 triangles; one embedded base-colour texture
  (JPEG, at most 1024²; 2048² for the textured `daemon`, `angel-female`, `angel-male`) – optional for the renderer.
- Coordinates: glTF convention, +Y up, the face looks toward **+Z**, units in
  metres-like scale normalised so that **chin-to-crown height = 1.0**.
  Origin = centre of the head (midpoint between the ears, at eye height).
  Shoulders/neck remain below the origin.
- Morph targets (shape keys) on `Head`, names in `mesh.extras.targetNames`
  in exactly this order, each designed for weights 0..1 and combinable:

  | # | name        | meaning                                                         |
  |---|-------------|-----------------------------------------------------------------|
  | 0 | `jawOpen`   | jaw rotates open around the jaw hinge; lower lip/chin drop       |
  | 1 | `lipsWide`  | mouth corners move outward and slightly back (E / I)             |
  | 2 | `lipsRound` | lips purse and push forward, corners inward (O / U)              |
  | 3 | `lipsPress` | lips close and press together (M / B / P)                        |
  | 4 | `smile`     | corners up and out, cheeks lift slightly                         |
  | 5 | `browRaise` | both brows lift, forehead softens upward                         |
  | 6 | `browFrown` | inner brows down and together                                    |
  | 7 | `blink`     | both upper lids close over the eyes                              |

  Morph targets carry `POSITION` deltas and `NORMAL` deltas.
- Landmarks in `scene.extras.haru` (same coordinate space as the mesh, unmorphed):
  `mouth` (centre of the lip line), `mouthL`, `mouthR` (corners), `eyeL`, `eyeR`
  (eyeball centres; L = the head's own left, i.e. +X), `chin`, `crown`,
  `noseTip`, `jawPivot` (point on the hinge axis, axis = X).
- Inner mouth: the lip slit is cut open and closed behind by a pouch (`cut_mouth` / `build_pouch` in
  `build_head.py`). Every pouch face maps to one single UV point inside a reserved dark atlas cell that no skin UV
  covers. The renderer finds that point (`findCavityUv` in `app/src/heads/cavity.ts`: the UV shared by most
  point-mapped triangles around the `mouth` landmark) and treats exactly those pixels as the mouth cavity: on
  textured heads it is drawn near-black at any jaw opening, including the resting slit, so no fluid shows through.
  `validate_heads.py` checks it (>= 8 such triangles, texel dark after JPEG).

## Review

`<subject>/review/` holds renders: front/side/three-quarter views of the
neutral head and one contact sheet with every morph target at weight 1.0
(and `jawOpen`+`lipsRound` combined). A successful export is not approval –
check the contact sheet.

## Subject `daemon` (textured, evil persona)

- Realistic demon bust: bald skull, ram horns curling tightly at the sides of the
  skull (they do not rise above the crown), pointed ears, closed lips with a dark
  lip line, frontal. Reference: `daemon/reference.png`, source:
  `daemon/varco-source.glb` (VARCO image-to-3D, textured, ~100k tris).
- It is a **textured** subject (like the angels below): `build_head.py` packs the base colour at
  **2048²** (JPEG q85, aim for <= ~1.5 MB) instead of 1024²; `validate_heads.py`
  allows 2048² for the textured subjects only. Triangle target stays 38,500 (limit 40,000).
- The renderer samples the embedded base colour only for heads listed in
  `TEXTURED_HEADS` in `app/src/heads/face.ts`; the composite grades the skin
  per head (`HEAD_GRADE` in `face.ts`: gamma, lift, tint, saturation); `daemon` = gamma 0.9, lift 1.3,
  tint (1.1, 0.78, 0.7), saturation 0.4 (leathery red-grey).
- Normalisation uses the picked `normalise.chin_z` / `normalise.crown_z` (and the
  `crown` pick, if present, for the `crown` landmark) – never the bounding box, so
  horns and ears do not distort scale or crown. For `daemon` the crown is the top
  of the skull between the horns.
- Build: `blender -b -P build_head.py -- --subject daemon --stage renders|landmarks|build`,
  then `python validate_heads.py daemon`.
- Daemon-specific build parameters (per-subject dicts in `build_head.py`, female/male unaffected):
  `DARK_STRIP['daemon'] = 0.08` - the VARCO atlas is fully packed, so the texture content is squeezed
  vertically into v in [0.08, 1] (UVs remapped) and the free bottom strip is painted dark for the inner
  mouth; `TEX_GAIN['daemon'] = 1.3` - linear brightness lift of the (dark) base colour.
  Landmarks are picked in `daemon/landmarks.json` incl. a `crown` pick (top of the skull between the horns).

## Subjects `angel-female`, `angel-male` (textured, angel persona)

- Realistic angel busts (shipped, replacing the former porcelain heads) for the angel persona: golden hair as one compact volume
  (female: low bun; no halo, no wings), white robe with a gold neckline, closed lips with a dark lip line, frontal.
  References and prompts: `angel-*/reference.png`, `angel-*/prompt.md`; source: `angel-*/varco-source.glb`
  (VARCO image-to-3D, textured, ~100k tris).
- Built exactly like `daemon`: textured, base colour at **2048²**, triangle target 38,500. Per-subject parameters
  in `build_head.py`: `DARK_STRIP = 0.08` (dark inner-mouth strip, as the VARCO atlas is fully packed) and
  `TEX_GAIN = 1.0`; both can be overridden per build with `--dark-strip <frac>` (0 = none) / `--tex-gain <g>`.
- Landmarks: `angel-*/landmarks.json` must be picked by hand from the raw renders (`--stage renders`), incl. the
  `normalise` block (`chin_z`, `crown_z`, `eye_z`, `ear_y`, `centre_x`). The crown is the top of the hair; a `crown`
  pick is optional (without it the highest vertex is used, fine for heads without horns or a halo).
- Build: `blender -b -P build_head.py -- --subject angel-female --stage renders|landmarks|build` (same for
  `angel-male`), then `python validate_heads.py angel-female angel-male`.
- Runtime (`app/src/heads/face.ts`): both are in `TEXTURED_HEADS`, `HEAD_GAIN` 1.0 and the grade gamma 0.9, lift 1.45,
  tint (1, 1, 1), saturation 0.95: the VARCO-baked skin is ~1.6-2x darker (linear) and more yellow than the reference,
  and this brings the median skin back to the reference (~183/144/105 sRGB vs 190/137/107).
- The porcelain `female` / `male` sources stay in the repo but their GLBs are no longer shipped. A missing angel GLB
  simply means no face (as for the daemon). `?debug&head=<angel-female|angel-male|daemon>` pins a head.
