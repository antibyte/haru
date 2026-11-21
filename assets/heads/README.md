# Haru heads – asset contract

Two sculptural porcelain busts (female, male) that appear as "Ein Gesicht im
Nebel": Haru's face, large and frontal behind the fluid, revealed only where
ink flows over it and never fully visible. `settings.gender === 'male'` selects
the male head; otherwise (female or unset) the female head is used.

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
  (JPEG, at most 1024²) – optional for the renderer.
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
