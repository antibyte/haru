# Reference prompt – daemon head (Haru, evil persona)

Purpose: single-image reference for VARCO image-to-3D (Generate3D, tPose=0), like
../female/prompt.md. Requirements from the VARCO handoff: one complete subject, nothing cropped,
plain background, no base/platform, no strong cast shadows. Design goals: realistic skin texture
(the renderer samples the base colour for this head), clean forms for the facial morph targets
(jaw, lips, brows, lids), a closed lip line with a dark gap so build_head.py finds the mouth slit,
horns kept tight to the skull so the chin-to-crown normalisation stays meaningful, even light so
the baked texture carries no strong shading.

## Prompt (used verbatim)

Front view studio photograph of a single hyperrealistic demon bust, made like a high-end
practical-effects creature sculpture with realistic painted silicone skin: head, neck and the top
of the shoulders, ending in a clean horizontal cut across the upper chest. No pedestal, no base,
no stand, nothing supporting it.

Pose: perfectly frontal and symmetrical, head level, looking straight ahead into the camera,
shoulders square to the camera.

Head: bald, slightly elongated skull with a heavy brow ridge, gaunt cheeks, sharp cheekbones,
pointed ears close to the head. Two short thick ram-like horns grow from the upper forehead and
curve backward tightly along the skull, staying close to the head and not rising above the top of
the skull.

Skin: realistic, leathery and deeply wrinkled, charcoal grey with dark crimson and bruised purple
undertones, visible pores, fine cracks, raised veins on the temples and neck, a few old scars.
Matte to slightly moist surface, no glossy highlights.

Face: cold and menacing, but a relaxed neutral expression with relaxed facial muscles. Eyes open,
deep-set, amber-yellow irises with dark pupils, clearly defined upper and lower eyelids. Thin dark
lips, closed, with a narrow dark gap between upper and lower lip along the whole mouth line, mouth
corners relaxed, no teeth visible. Broad flat nose.

Lighting: soft, even, diffuse studio light from the front and slightly above, gentle shading that
reveals the forms, no harsh or hard-edged cast shadows, no rim light, no glowing elements.

Background: seamless plain light grey (#d4d4d4), evenly lit, no gradient banding, no shadow of
the bust on the background, nothing else in the frame.
