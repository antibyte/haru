# Haru heads – asset contract

Three busts: two sculptural porcelain busts (female, male) and the textured demon bust (`daemon`) that appear as "Ein Gesicht im
Nebel": Haru's face, large and frontal behind the fluid, revealed only where
ink flows over it and never fully visible. `settings.gender === 'male'` selects
the male head; otherwise (female or unset) the female head is used. The `daemon`
head belongs to the evil persona (see below).

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
  (JPEG, at most 1024²; 2048² for `daemon`) – optional for the renderer.
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
- It is the only **textured** subject: `build_head.py` packs the base colour at
  **2048²** (JPEG q85, aim for <= ~1.5 MB) instead of 1024²; `validate_heads.py`
  allows 2048² for `daemon` only. Triangle target stays 38,500 (limit 40,000).
- The renderer samples the embedded base colour only for heads listed in
  `TEXTURED_HEADS` in `app/src/heads/face.ts`; the porcelain heads stay untextured.
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
