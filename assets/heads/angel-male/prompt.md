# Reference prompt – male angel head (Haru, angel persona, male voice)

Purpose: single-image reference for VARCO image-to-3D (Generate3D, tPose=0), like ../daemon/prompt.md.
Requirements from the VARCO handoff: one complete subject, nothing cropped, plain background, no
base/platform, no strong cast shadows. Design goals: realistic, high-quality skin texture (the renderer
samples the base colour, like for the daemon), clean forms for the facial morph targets (jaw, lips,
brows, lids), a closed lip line with a dark gap so build_head.py finds the mouth slit, hair as one
compact volume clearly separated from the face so chin-to-crown normalisation stays meaningful, no
wings or halo (they would distort the bust), even light so the baked texture carries no strong shading.
Look: matches the angel in the persona chooser (assets/personas/angel): a young man with golden-blond
curls, white robe with a gold neckline, warm and serene.

## Prompt (used verbatim)

Front view studio photograph of a single hyperrealistic bust of a male angel, made like a high-end
practical-effects sculpture with realistic painted silicone skin: a young adult man about 25 years old,
head, neck and the top of the shoulders, ending in a clean horizontal cut across the upper chest. No
pedestal, no base, no stand, nothing supporting it.

Pose: perfectly frontal and symmetrical, head level, looking straight ahead into the camera, shoulders
square to the camera.

Head and hair: short to medium golden-blond curly hair, soft defined curls forming one compact volume
close to the head, ending above the ears and at the nape, clearly separated from the face and neck.
Ears visible. No curls falling over the forehead or eyes, no flyaways. Clean-shaven.

Skin: realistic, luminous warm light skin with a soft golden undertone and a gentle rosy tint on
cheeks and lips, visible fine pores, subtle natural variation. Matte, velvety surface with only a very
soft natural sheen, no glossy highlights, no makeup.

Face: kind, serene and gentle, a relaxed neutral expression with relaxed facial muscles and the faint
warmth of a smile in the eyes only. Eyes open, light grey-blue irises with dark pupils, clearly
defined upper and lower eyelids, natural lashes. Straight brows a shade darker than the hair. Straight,
well-proportioned nose. Natural lips, closed, with a narrow dark gap between upper and lower lip along
the whole mouth line, mouth corners relaxed, no teeth visible. Smooth cheeks, defined jawline and chin.

Garment: the shoulders are draped in a simple white linen robe with a thin embroidered gold band along
the neckline. No jewellery.

Lighting: soft, even, diffuse studio light from the front and slightly above, gentle shading that
reveals the forms, no harsh or hard-edged cast shadows, no rim light, no glowing elements, no halo,
no wings.

Background: seamless plain light grey (#d4d4d4), evenly lit, no gradient banding, no shadow of the bust
on the background, nothing else in the frame.

Framing: the entire bust is fully visible and centred with a generous margin on all sides, nothing
cropped. Square 1:1 composition.

Style: cinematic, photorealistic, sharp focus, high detail. An original character that does not
resemble any real person. No text, no watermark, no logo, no other objects.
