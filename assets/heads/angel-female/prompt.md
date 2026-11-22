# Reference prompt – female angel head (Haru, angel persona, female voice)

Purpose: single-image reference for VARCO image-to-3D (Generate3D, tPose=0), like ../daemon/prompt.md.
Requirements from the VARCO handoff: one complete subject, nothing cropped, plain background, no
base/platform, no strong cast shadows. Design goals: realistic, high-quality skin texture (the renderer
samples the base colour, like for the daemon), clean forms for the facial morph targets (jaw, lips,
brows, lids), a closed lip line with a dark gap so build_head.py finds the mouth slit, hair as one
compact volume clearly separated from the face so chin-to-crown normalisation stays meaningful, no
wings or halo (they would distort the bust), even light so the baked texture carries no strong shading.
Look: the same family as the angel in the persona chooser (assets/personas/angel): golden hair, white
robe with a gold neckline, warm and serene.

## Prompt (used verbatim)

Front view studio photograph of a single hyperrealistic bust of a female angel, made like a high-end
practical-effects sculpture with realistic painted silicone skin: an adult woman about 25 years old,
head, neck and the top of the shoulders, ending in a clean horizontal cut across the upper chest. No
pedestal, no base, no stand, nothing supporting it.

Pose: perfectly frontal and symmetrical, head level, looking straight ahead into the camera, shoulders
square to the camera.

Head and hair: soft golden-blond hair with gentle waves, parted in the middle and swept back from the
forehead and temples into a low bun at the nape, forming one compact volume close to the head, clearly
separated from the face and neck. Ears partly visible below the hairline. No loose strands falling
over the face, no flyaways.

Skin: realistic, luminous warm ivory skin with a soft rosy undertone on cheeks and lips, visible fine
pores, a few very faint freckles across the nose, subtle natural variation. Matte, velvety surface
with only a very soft natural sheen, no glossy highlights, no makeup.

Face: kind, serene and gentle, a relaxed neutral expression with relaxed facial muscles and the faint
warmth of a smile in the eyes only. Eyes open, light grey-blue irises with dark pupils, clearly
defined upper and lower eyelids, natural lashes. Softly arched brows a shade darker than the hair.
Straight, delicate nose. Soft natural pink lips, closed, with a narrow dark gap between upper and lower
lip along the whole mouth line, mouth corners relaxed, no teeth visible. Smooth cheeks, soft defined
jawline and chin.

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
