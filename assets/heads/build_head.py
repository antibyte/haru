"""
build_head.py - build a Haru runtime head (Blender 5.3, headless).

    blender -b -P build_head.py -- --subject female|male|daemon [--stage STAGE] [--work DIR]

Stages
  renders   Import + clean + normalise the VARCO mesh (full resolution) and
            write orthographic front/side/close-up renders with a pixel grid
            into --work. The renders are LOOKED AT by a human/agent to pick
            landmark pixels; the picks are stored in <subject>/landmarks.json.
  landmarks Derive the 3D landmarks from the pixel picks in landmarks.json (raycast on the full-resolution
            normalised mesh), write them back to landmarks.json and render marker overlays for checking.
  build     (default) Full pipeline: clean, normalise, decimate (before any
            shape key), downsize texture, landmarks -> morph targets (shape
            keys), demo action, save <subject>/source.blend, export
            app/public/heads/<subject>.glb, review renders (<subject>/review/).
            Options: --review 0|1, --export 0|1, --target <max tris>.
  Validate the result afterwards with:  python validate_heads.py [--record]

Coordinate systems
  The Blender scene is Z-up and the face looks toward -Y; the glTF exporter
  converts that to +Y up / face toward +Z.  All landmarks (landmarks.json,
  scene.extras.haru) are expressed in the glTF frame (x = the head's own left,
  y up, z forward) after normalisation: chin-to-crown = 1.0, origin at the
  head centre between the ears at eye height.

landmarks.json
  "normalise": raw (VARCO) space numbers: chin_z, crown_z, eye_z, ear_y, ...
  "picks": pixel picks per view (provenance) used to derive the 3D
  "landmarks" (pixel -> world via the View mapping, depth by raycasting on the
  full-resolution normalised mesh).  Values are reviewed by eye on the renders
  and then frozen so builds are reproducible.
"""
import bpy, bmesh, sys, os, json, math, hashlib
import numpy as np
from mathutils import Vector, Matrix
from mathutils.bvhtree import BVHTree

HERE = os.path.dirname(os.path.abspath(__file__))
SUBJECTS = ('female', 'male', 'daemon')
TEXTURED_SUBJECTS = {'daemon': 2048}   # subject -> base-colour size; all others 1024
REPO = os.path.dirname(os.path.dirname(HERE))
# per-subject texture prep (only subjects listed here are touched; female/male stay identical)
DARK_STRIP = {'daemon': 0.08}     # fraction of the atlas height reserved at v=0 as a free strip for the inner mouth
TEX_GAIN = {'daemon': 1.3}        # linear brightness gain (VARCO base colour is darker than the reference)


# --------------------------------------------------------------------------
# args
# --------------------------------------------------------------------------
def parse_args():
    argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
    a = {'subject': 'female', 'stage': 'build',
         'work': os.path.join(os.environ.get('TEMP', '.'), 'haru_head_work')}
    i = 0
    while i < len(argv):
        k = argv[i]
        if k.startswith('--'):
            a[k[2:]] = argv[i + 1]
            i += 2
        else:
            i += 1
    assert a['subject'] in SUBJECTS, f"unknown subject {a['subject']!r}; expected one of {SUBJECTS}"
    return a


# --------------------------------------------------------------------------
# import / clean / normalise
# --------------------------------------------------------------------------
def import_source(path):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=path)
    meshes = [o for o in bpy.data.objects if o.type == 'MESH']
    bpy.ops.object.select_all(action='DESELECT')
    for o in meshes:
        o.select_set(True)
        o.data.transform(o.matrix_world)
        o.matrix_world = Matrix.Identity(4)
    bpy.context.view_layer.objects.active = meshes[0]
    if len(meshes) > 1:
        bpy.ops.object.join()
    ob = bpy.context.view_layer.objects.active
    for o in list(bpy.data.objects):
        if o is not ob:
            bpy.data.objects.remove(o)
    ob.name = 'Head'
    ob.data.name = 'Head'
    return ob


def clean_mesh(ob, min_part_frac=0.01, merge_dist=1e-5):
    """Remove tiny loose parts, merge by distance, recalc normals outward."""
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=merge_dist)
    bm.verts.ensure_lookup_table()
    seen, comps = set(), []
    for v in bm.verts:
        if v.index in seen:
            continue
        stack, comp = [v], []
        seen.add(v.index)
        while stack:
            c = stack.pop()
            comp.append(c)
            for e in c.link_edges:
                o = e.other_vert(c)
                if o.index not in seen:
                    seen.add(o.index)
                    stack.append(o)
        comps.append(comp)
    comps.sort(key=len, reverse=True)
    biggest = len(comps[0])
    removed = 0
    for comp in comps[1:]:
        if len(comp) < biggest * min_part_frac:
            bmesh.ops.delete(bm, geom=comp, context='VERTS')
            removed += 1
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(ob.data)
    bm.free()
    ob.data.update()
    print('clean: parts', len(comps), 'removed', removed)


def get_co(me):
    n = len(me.vertices)
    co = np.empty(n * 3, dtype=np.float64)
    me.vertices.foreach_get('co', co)
    return co.reshape(n, 3)


def set_co(me, co):
    me.vertices.foreach_set('co', np.asarray(co, dtype=np.float32).ravel())
    me.update()


def normalise(ob, nrm):
    """Scale/translate so chin-to-crown = 1, origin at head centre."""
    co = get_co(ob.data)
    # chin/crown come from the picked landmarks (normalise.chin_z / crown_z in landmarks.json), never from the
    # bounding box, so horns/ears cannot distort the scale. The bbox maximum is only a fallback for raw previews.
    crown = nrm.get('crown_z')
    if not crown:
        crown = co[:, 2].max()
        print('WARNING normalise: no crown_z picked, falling back to the highest vertex (horns would distort it)')
    s = 1.0 / (crown - nrm['chin_z'])
    origin = np.array([nrm.get('centre_x', 0.0), nrm['ear_y'], nrm['eye_z']])
    set_co(ob.data, (co - origin) * s)
    return s, origin


# --------------------------------------------------------------------------
# frame helpers (blender <-> glTF frame)
# --------------------------------------------------------------------------
def to_g(v):
    return Vector((v[0], v[2], -v[1]))


def to_b(v):
    return Vector((v[0], -v[2], v[1]))


def verts_g(me):
    co = get_co(me)
    return np.stack([co[:, 0], co[:, 2], -co[:, 1]], axis=1)


def g_to_b_arr(a):
    return np.stack([a[:, 0], -a[:, 2], a[:, 1]], axis=1)


# --------------------------------------------------------------------------
# rendering (workbench) with measurement grid
# --------------------------------------------------------------------------
FONT = {
    '0': '111101101101111', '1': '010110010010111', '2': '111001111100111', '3': '111001111001111',
    '4': '101101111001001', '5': '111100111001111', '6': '111100111101111', '7': '111001001001001',
    '8': '111101111101111', '9': '111101111001111'}


def setup_scene(res=1000, textured=False, clay=(0.72, 0.70, 0.67)):
    sc = bpy.context.scene
    sc.render.engine = 'BLENDER_WORKBENCH'
    sh = sc.display.shading
    sh.light = 'STUDIO'
    sh.show_cavity = False
    sh.show_shadows = False
    sh.color_type = 'TEXTURE' if textured else 'SINGLE'
    sh.single_color = clay
    sc.render.resolution_x = sc.render.resolution_y = res
    sc.render.resolution_percentage = 100
    sc.view_settings.view_transform = 'Standard'
    if sc.world is None:
        sc.world = bpy.data.worlds.new('w')
    sc.world.color = (0.93, 0.93, 0.95)
    sc.render.film_transparent = False
    sc.render.image_settings.file_format = 'PNG'
    sc.render.image_settings.color_mode = 'RGB'
    return sc


def get_camera():
    sc = bpy.context.scene
    if sc.camera is None:
        cam = bpy.data.objects.new('cam', bpy.data.cameras.new('cam'))
        sc.collection.objects.link(cam)
        sc.camera = cam
    return sc.camera


class View:
    """Orthographic view with a known pixel <-> world mapping (glTF frame).

    front: camera on +Z_g looking toward -Z_g; image u -> +X, v -> -Y;
           centre = (x, y)
    side : camera on +X looking toward -X; image u -> -Z_g (face on the
           left), v -> -Y; centre = (z, y)
    A pixel (u, v) has origin top-left, the image is res x res px.
    """

    def __init__(self, kind, centre, scale, res=1000):
        self.kind, self.centre, self.scale, self.res = kind, tuple(centre), scale, res
        self.k = scale / res

    def px_to_world(self, u, v):
        h = (u - self.res / 2) * self.k
        y = (self.res / 2 - v) * self.k + self.centre[1]
        return (h + self.centre[0], y) if self.kind == 'front' else (-h + self.centre[0], y)

    def world_to_px(self, h, y):
        if self.kind == 'front':
            u = self.res / 2 + (h - self.centre[0]) / self.k
        else:
            u = self.res / 2 - (h - self.centre[0]) / self.k
        return (u, self.res / 2 - (y - self.centre[1]) / self.k)

    def place(self):
        cam = get_camera()
        cam.data.type = 'ORTHO'
        cam.data.ortho_scale = self.scale
        cam.data.clip_start = 0.01
        cam.data.clip_end = 100
        if self.kind == 'front':
            cb = Vector((self.centre[0], 0, self.centre[1]))
            cam.location = cb + Vector((0, -5, 0))
            cam.rotation_euler = (math.radians(90), 0, 0)
        else:
            cb = Vector((0, -self.centre[0], self.centre[1]))
            cam.location = cb + Vector((5, 0, 0))
            cam.rotation_euler = (math.radians(90), 0, math.radians(90))


def place_persp(az_deg, centre_g=(0, 0, 0), dist=3.2, lens=85, elev_deg=0.0):
    """Perspective camera at azimuth (deg, positive = toward the head's left/+X)."""
    cam = get_camera()
    cam.data.type = 'PERSP'
    cam.data.lens = lens
    cam.data.sensor_width = 36
    cam.data.clip_start = 0.05
    cam.data.clip_end = 100
    az, el = math.radians(az_deg), math.radians(elev_deg)
    cb = to_b(centre_g)
    cam.location = cb + Vector((dist * math.sin(az) * math.cos(el), -dist * math.cos(az) * math.cos(el),
                                dist * math.sin(el)))
    cam.rotation_euler = (math.radians(90) - el, 0, az)


def draw_grid(path, step=50, bold=100, ls=2):
    img = bpy.data.images.load(path)
    w, h = img.size
    a = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)[::-1].copy()  # top-left origin
    for p in range(0, w, step):
        if p % bold == 0:
            col, m = np.array([1, 0, 0, 1]), 0.5
        else:
            col, m = np.array([0, 0.6, 1, 1]), 0.2
        a[:, p] = a[:, p] * (1 - m) + col * m
        a[p, :] = a[p, :] * (1 - m) + col * m
    for p in range(0, w, bold):
        for (cx, cy) in [(p + 3, 3), (3, p + 3)]:
            x = cx
            for ch in str(p):
                bits = FONT[ch]
                for r in range(5):
                    for c in range(3):
                        if bits[r * 3 + c] == '1':
                            y0, x0 = cy + r * ls, x + c * ls
                            if y0 + ls < h and x0 + ls < w:
                                a[y0:y0 + ls, x0:x0 + ls] = [0.8, 0, 0, 1]
                x += 4 * ls
    img.pixels = a[::-1].ravel().tolist()
    img.filepath_raw = path
    img.file_format = 'PNG'
    img.save()
    bpy.data.images.remove(img)


def render(path, grid=False):
    sc = bpy.context.scene
    sc.render.filepath = path
    bpy.ops.render.render(write_still=True)
    if grid:
        draw_grid(path)


def load_json(p, default=None):
    if os.path.exists(p):
        with open(p, 'r', encoding='utf-8') as f:
            return json.load(f)
    return default


# --------------------------------------------------------------------------
# stage: renders (landmark picking aids)
# --------------------------------------------------------------------------
def stage_renders(args):
    subj = args['subject']
    sdir = os.path.join(HERE, subj)
    lm = load_json(os.path.join(sdir, 'landmarks.json'), {})
    work = args['work']
    os.makedirs(work, exist_ok=True)
    ob = import_source(os.path.join(sdir, 'varco-source.glb'))
    clean_mesh(ob)
    nrm = lm.get('normalise')
    if nrm is None:
        nrm = {'chin_z': -0.5, 'crown_z': 0.5, 'ear_y': 0.0, 'eye_z': 0.0}
        print('raw preview (no normalise block yet; raw units)')
    normalise(ob, nrm)
    setup_scene(1000)
    vs = args.get('views', '')
    views = json.load(open(vs[1:])) if vs.startswith('@') else (json.loads(vs) if vs else
             [['front', 0, 0, 1.6, 'front'], ['side', 0, 0, 1.6, 'side']])
    for kind, h, y, scl, tag in views:
        v = View(kind, (h, y), scl)
        v.place()
        p = os.path.join(work, f'{subj}_{tag}.png')
        render(p, grid=True)
        print('view', tag, kind, 'centre', (h, y), 'scale', scl, '->', p)


# --------------------------------------------------------------------------
# landmarks: pixel picks -> 3D (glTF frame)
# --------------------------------------------------------------------------
def make_bvh(ob):
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    bmesh.ops.triangulate(bm, faces=bm.faces)
    t = BVHTree.FromBMesh(bm)
    bm.free()
    return t


def ray_front(bvh, x, y, z_from=5.0):
    """First surface hit looking from +Z_g toward -Z_g at (x, y). Returns z_g or None."""
    hit = bvh.ray_cast(Vector((x, -z_from, y)), Vector((0, 1, 0)))
    return None if hit[0] is None else -hit[0].y


def derive_landmarks(ob, lm):
    """Convert the pixel picks in landmarks.json to 3D landmarks (glTF frame)."""
    views = {k: View(v['kind'], v['centre'], v['scale']) for k, v in lm['views'].items()}
    bvh = make_bvh(ob)
    P = verts_g(ob.data)
    out = {}

    def pick(name):
        pk = lm['picks'][name]
        v = views[pk['view']]
        h, y = v.px_to_world(*pk['px'])
        if v.kind == 'front':
            z = ray_front(bvh, h, y)
            assert z is not None, name
            return [h, y, z]
        return [pk.get('x', 0.0), y, h]

    for n in ['mouth', 'mouthL', 'mouthR', 'eyeL', 'eyeR', 'eyeLouter', 'eyeLinner', 'eyeLtop', 'eyeLbottom',
              'browL', 'browLinner', 'chin', 'noseTip', 'jawPivot']:
        out[n] = pick(n)
    # symmetrise (the head is built symmetric: mirror the left picks)
    o = out
    mw = 0.5 * (abs(o['mouthL'][0]) + abs(o['mouthR'][0]))
    my = 0.5 * (o['mouthL'][1] + o['mouthR'][1])
    mz = 0.5 * (o['mouthL'][2] + o['mouthR'][2])
    o['mouthL'] = [mw, my, mz]
    o['mouthR'] = [-mw, my, mz]
    o['mouth'][0] = 0.0
    o['chin'][0] = 0.0
    ex = 0.5 * (abs(o['eyeL'][0]) + abs(o['eyeR'][0]))
    ey = 0.5 * (o['eyeL'][1] + o['eyeR'][1])
    width = math.hypot(o['eyeLouter'][0] - o['eyeLinner'][0], o['eyeLouter'][1] - o['eyeLinner'][1])
    R = 0.4 * width                       # eyeball radius ~ 0.4 x palpebral fissure width
    ez = 0.5 * (o['eyeL'][2] + o['eyeR'][2]) - R   # centre sits one radius behind the iris surface
    o['eyeL'] = [ex, ey, ez]
    o['eyeR'] = [-ex, ey, ez]
    eye = {'R': R, 'a': 0.5 * width,
           'hu': o['eyeLtop'][1] - ey, 'hl': ey - o['eyeLbottom'][1],
           'xInner': abs(o['eyeLinner'][0]), 'xOuter': abs(o['eyeLouter'][0])}
    for k in ('eyeLouter', 'eyeLinner', 'eyeLtop', 'eyeLbottom'):
        del o[k]
    brow = {'peakL': o.pop('browL'), 'innerL': o.pop('browLinner')}
    if 'crown' in lm['picks']:      # picked crown (daemon: top of the skull between the horns)
        c = pick('crown')
        o['crown'] = [0.0, c[1], c[2]]
    else:                            # legacy: highest vertex (fine for hornless heads)
        crown_i = int(np.argmax(P[:, 1]))
        o['crown'] = [0.0, float(P[crown_i, 1]), float(P[crown_i, 2])]
    o['jawPivot'][0] = 0.0
    o['noseTip'][0] = 0.0
    rnd = lambda v: [round(float(t), 5) for t in v]
    res = {k: rnd(v) for k, v in o.items()}
    res_eye = {k: round(float(v), 5) for k, v in eye.items()}
    res_brow = {k: rnd(v) for k, v in brow.items()}
    return res, res_eye, res_brow


def mark_image(path, items):
    """items: list of (u, v, (r, g, b), label_digit_or_None) crosses on a PNG."""
    img = bpy.data.images.load(path)
    w, h = img.size
    a = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)[::-1].copy()
    for u, v, col in items:
        u, v = int(round(u)), int(round(v))
        for d in range(-9, 10):
            for (xx, yy) in ((u + d, v), (u, v + d)):
                if 0 <= xx < w and 0 <= yy < h and abs(d) > 2:
                    a[yy, xx] = [*col, 1]
    img.pixels = a[::-1].ravel().tolist()
    img.filepath_raw = path
    img.save()
    bpy.data.images.remove(img)


def stage_landmarks(args):
    subj = args['subject']
    sdir = os.path.join(HERE, subj)
    lpath = os.path.join(sdir, 'landmarks.json')
    lm = load_json(lpath)
    work = args['work']
    os.makedirs(work, exist_ok=True)
    ob = import_source(os.path.join(sdir, 'varco-source.glb'))
    clean_mesh(ob)
    normalise(ob, lm['normalise'])
    marks, eye, brow = derive_landmarks(ob, lm)
    lm['landmarks'], lm['eye'], lm['brow'] = marks, eye, brow
    with open(lpath, 'w', encoding='utf-8') as f:
        json.dump(lm, f, indent=1)
    print(json.dumps(marks), json.dumps(eye), json.dumps(brow))
    setup_scene(1000)
    allp = dict(marks)
    allp['eyeL_'] = marks['eyeL']
    allp.update({'browL': brow['peakL'], 'browLinner': brow['innerL']})
    for name, vd in lm['views'].items():
        v = View(vd['kind'], vd['centre'], vd['scale'])
        v.place()
        path = os.path.join(work, f'{subj}_lm_{name}.png')
        render(path, grid=False)
        items = []
        for k, p in allp.items():
            h = p[0] if v.kind == 'front' else p[2]
            items.append((*v.world_to_px(h, p[1]), (1, 0, 0) if k.endswith('L') or k in ('mouth', 'chin') else (0, 0.4, 1)))
        mark_image(path, items)

# --------------------------------------------------------------------------
# decimation (BEFORE any shape key) + material
# --------------------------------------------------------------------------
def smooth(t):
    t = np.clip(t, 0.0, 1.0)
    return t * t * (3 - 2 * t)


def ramp(x, a, b):
    """0 below a, 1 above b (smoothstep); works for a > b (falling) too."""
    return smooth((np.asarray(x) - a) / (b - a))


def count_tris(ob_eval_mesh):
    return sum(len(p.vertices) - 2 for p in ob_eval_mesh.polygons)


def decimate(ob, L, target=38500, face_floor=0.10):
    """Collapse-decimate with a vertex-group bias that keeps the face dense.
    UVs are preserved by the collapse decimator (seams are kept)."""
    me = ob.data
    P = verts_g(me)
    lm = L['landmarks']
    chin_y = lm['chin'][1]
    face = (ramp(P[:, 2], -0.04, 0.12) * ramp(P[:, 1], chin_y - 0.16, chin_y - 0.04)
            * ramp(P[:, 1], 0.40, 0.28) * ramp(np.abs(P[:, 0]), 0.36, 0.26))
    wgt = face_floor + (1 - face_floor) * face
    vg = ob.vertex_groups.new(name='keep')
    for w in np.unique(np.round(wgt, 2)):
        idx = np.nonzero(np.round(wgt, 2) == w)[0].tolist()
        vg.add(idx, float(w), 'REPLACE')
    mod = ob.modifiers.new('decimate', 'DECIMATE')
    mod.decimate_type = 'COLLAPSE'
    mod.use_collapse_triangulate = True
    mod.vertex_group = 'keep'
    mod.vertex_group_factor = 1.0
    mod.invert_vertex_group = True   # high weight = keep (face), low weight = collapse first
    dg = bpy.context.evaluated_depsgraph_get()
    n0 = sum(len(p.vertices) - 2 for p in me.polygons)
    lo, hi = 0.05, 1.0
    ratio = target / n0
    best = None
    for _ in range(10):
        mod.ratio = ratio
        dg.update()
        ev = ob.evaluated_get(dg)
        m2 = ev.to_mesh()
        n = count_tris(m2)
        ev.to_mesh_clear()
        print('  decimate try ratio', round(ratio, 4), '->', n)
        if n > target:
            hi = ratio
        else:
            lo = ratio
            best = ratio
            if n > target - 600:
                break
        ratio = 0.5 * (lo + hi)
    mod.ratio = best if best is not None else lo
    bpy.context.view_layer.objects.active = ob
    ob.select_set(True)
    bpy.ops.object.modifier_apply(modifier='decimate')
    ob.vertex_groups.remove(ob.vertex_groups['keep'])
    n1 = sum(len(p.vertices) - 2 for p in ob.data.polygons)
    print('decimate', n0, '->', n1, 'tris; ratio', mod.ratio)
    return n1


def finish_mesh(ob):
    me = ob.data
    bpy.context.view_layer.objects.active = ob
    ob.select_set(True)
    try:
        bpy.ops.mesh.customdata_custom_splitnormals_clear()
    except Exception as e:  # pragma: no cover
        print('splitnormals clear:', e)
    bm = bmesh.new()
    bm.from_mesh(me)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(me)
    bm.free()
    bpy.ops.object.shade_smooth()
    me.update()


def find_base_image(ob):
    for mat in ob.data.materials:
        if mat and mat.node_tree:
            for n in mat.node_tree.nodes:
                if n.type == 'BSDF_PRINCIPLED':
                    inp = n.inputs['Base Color']
                    if inp.is_linked and inp.links[0].from_node.type == 'TEX_IMAGE':
                        return inp.links[0].from_node.image
    return None


def make_material(ob, size=1024):
    """Single material: base colour only, texture downsized to size^2 (exported as JPEG)."""
    img = find_base_image(ob)
    img_new = None
    if img is not None:
        img.name = 'Head_basecolor_src'
        img.scale(size, size)
        img.pack()
        img_new = img
        img_new.name = 'Head_basecolor'
    mat = bpy.data.materials.new('HeadPorcelain')
    mat.use_nodes = True
    nt = mat.node_tree
    for n in list(nt.nodes):
        nt.nodes.remove(n)
    out = nt.nodes.new('ShaderNodeOutputMaterial')
    bsdf = nt.nodes.new('ShaderNodeBsdfPrincipled')
    bsdf.inputs['Roughness'].default_value = 0.55
    bsdf.inputs['Metallic'].default_value = 0.0
    nt.links.new(bsdf.outputs['BSDF'], out.inputs['Surface'])
    if img_new is not None:
        tex = nt.nodes.new('ShaderNodeTexImage')
        tex.image = img_new
        nt.links.new(tex.outputs['Color'], bsdf.inputs['Base Color'])
    ob.data.materials.clear()
    ob.data.materials.append(mat)
    for other in list(bpy.data.materials):
        if other is not mat and other.users == 0:
            bpy.data.materials.remove(other)
    return img_new

# --------------------------------------------------------------------------
# mouth: rip the lip slit open and add a closed, dark inner-mouth pouch
# --------------------------------------------------------------------------
def reserve_dark_strip(ob, img, frac, gain=1.0):
    """Dense VARCO atlases have no free block: squeeze the atlas content vertically into v in [frac, 1] and
    leave the bottom strip free (dark) for the inner-mouth faces. UVs are remapped accordingly."""
    w, h = img.size
    a = np.empty(w * h * 4, dtype=np.float32)
    img.pixels.foreach_get(a)
    a = a.reshape(h, w, 4)
    h0 = int(round(h * frac))
    src = np.linspace(0, h - 1, h - h0)
    i0 = np.floor(src).astype(int)
    i1 = np.minimum(i0 + 1, h - 1)
    t = (src - i0)[:, None, None]
    body = a[i0] * (1 - t) + a[i1] * t
    body[..., :3] = np.clip(body[..., :3] * gain, 0, 1)
    out = np.zeros_like(a)
    out[..., 3] = 1.0
    out[..., :3] = (0.045, 0.012, 0.014)
    out[h0:] = body
    img.pixels.foreach_set(out.ravel())
    img.pack()
    me = ob.data
    uvl = me.uv_layers.active
    uv = np.empty(len(me.loops) * 2, dtype=np.float32)
    uvl.data.foreach_get('uv', uv)
    uv = uv.reshape(-1, 2)
    uv[:, 1] = h0 / h + uv[:, 1] * (1 - h0 / h)
    uvl.data.foreach_set('uv', uv.ravel())
    me.update()
    print('texture: reserved dark strip', h0, 'rows, gain', gain)


def find_free_uv(me, cells=128, need=6):
    """Centre (u, v) of a free need x need block of the UV atlas (or None)."""
    uvl = me.uv_layers.active
    if uvl is None:
        return None
    me.calc_loop_triangles()
    uv = np.empty(len(me.loops) * 2)
    uvl.data.foreach_get('uv', uv)
    uv = uv.reshape(-1, 2)
    occ = np.zeros((cells, cells), dtype=bool)
    tri_loops = np.empty(len(me.loop_triangles) * 3, dtype=np.int32)
    me.loop_triangles.foreach_get('loops', tri_loops)
    t = uv[tri_loops].reshape(-1, 3, 2)
    for k in range(6):          # sample barycentric points so large triangles are covered
        for l in range(6 - k):
            a, b = k / 5.0, l / 5.0
            pt = t[:, 0] * (1 - a - b) + t[:, 1] * a + t[:, 2] * b
            iu = np.clip((pt[:, 0] * cells).astype(int), 0, cells - 1)
            iv = np.clip((pt[:, 1] * cells).astype(int), 0, cells - 1)
            occ[iv, iu] = True
    # dilate by one cell to keep clear of JPEG bleeding
    d = occ.copy()
    for dy in (-1, 0, 1):
        for dx in (-1, 0, 1):
            d |= np.roll(np.roll(occ, dy, 0), dx, 1)
    for iv in range(cells - need):
        for iu in range(cells - need):
            if not d[iv:iv + need, iu:iu + need].any():
                return ((iu + need / 2) / cells, (iv + need / 2) / cells)
    return None


def slit_curve(P, R, step=0.006):
    """Deepest point of the lip notch per x column -> smooth (x, y, z) curve."""
    w = R.w
    xs = np.arange(-w * 1.15, w * 1.15 + 1e-9, step)
    ys, zs, ok = [], [], []
    for bx in xs:
        m = (np.abs(R.x - bx) < step * 0.75) & (np.abs(R.y - (R.M[1] + (R.C[1] - R.M[1]) * min(abs(bx) / w, 1.25) ** 2)) < 0.02) & (R.z > R.M[2] - 0.15)
        if m.sum() < 2:
            ys.append(np.nan), zs.append(np.nan), ok.append(False)
            continue
        i = np.nonzero(m)[0]
        j = i[np.argmin(R.z[i])]
        ys.append(R.y[j]), zs.append(R.z[j]), ok.append(True)
    ys, zs = np.array(ys), np.array(zs)
    good = np.isfinite(ys)
    ys = np.interp(xs, xs[good], ys[good])
    zs = np.interp(xs, xs[good], zs[good])
    # smooth (median 3 then box 5)
    def sm(a):
        p = np.pad(a, 2, mode='edge')
        med = np.median(np.stack([p[:-2], p[1:-1], p[2:]]), axis=0)
        p2 = np.pad(med[1:-1], 2, mode='edge')
        return np.convolve(p2, np.ones(5) / 5, mode='valid')[:len(a)]
    ys, zs = sm(ys), sm(zs)
    return xs, ys, zs


def order_loop(edges):
    """Ordered vertex cycle of a closed edge loop."""
    adj = {}
    for e in edges:
        a_, b_ = e.verts
        adj.setdefault(a_, []).append(b_)
        adj.setdefault(b_, []).append(a_)
    start = edges[0].verts[0]
    ring, prev, cur = [start], None, start
    for _ in range(len(edges) + 1):
        cands = [v for v in adj[cur] if v is not prev]
        if not cands:
            break
        nxt = cands[0]
        if nxt is start:
            break
        ring.append(nxt)
        prev, cur = cur, nxt
    return ring


def split_rim(ring):
    """Split the closed lip-slit loop at its two corner vertices into (upper arc, lower arc) index lists."""
    xs = np.array([v.co.x for v in ring])
    i0, i1 = int(np.argmin(xs)), int(np.argmax(xs))
    n = len(ring)
    def arc(a_, b_):
        out = [a_]
        k = a_
        while k != b_:
            k = (k + 1) % n
            out.append(k)
        return out
    A, B_ = arc(i0, i1), arc(i1, i0)
    gy = lambda idx: np.mean([ring[k].co.z for k in idx])     # blender z == glTF y
    return (A, B_) if gy(A) > gy(B_) else (B_, A)


def build_pouch(bm, ring, slit_y, depth, half_h=0.022, half_gap=0.0028):
    """Closed inner-mouth pouch behind the ripped lip slit: a tube from the rim to an inner ring
    (an ellipse hanging above/below the slit line), capped with a fan to a deeper centre vertex.
    Returns (new faces, upper rim coords, lower rim coords) in the glTF frame."""
    up_idx, lo_idx = split_rim(ring)
    xmax = max(abs(v.co.x) for v in ring)
    # regularise the ragged rims: lie on a smooth lens around the slit line (upper arc above, lower arc below)
    for idxs, sgn in ((up_idx, 1.0), (lo_idx, -1.0)):
        vs = [ring[k] for k in idxs]
        order = np.argsort([v.co.x for v in vs])
        zg = np.array([-vs[o].co.y for o in order])
        zs_s = np.convolve(np.pad(zg, 3, mode='edge'), np.ones(7) / 7, mode='valid')
        for j, o in enumerate(order):
            v = vs[o]
            x = v.co.x
            hgx = half_gap * math.sqrt(max(0.0, 1 - (x / xmax) ** 2)) + 0.0003
            v.co = Vector((x, -float(zs_s[j]), float(slit_y(x)) + sgn * hgx))
    side = {}
    for k in up_idx:
        side[k] = 1.0
    for k in lo_idx:
        side[k] = -1.0
    inner, zs = [], []
    for k, v in enumerate(ring):
        g = np.array([v.co.x, v.co.z, -v.co.y])
        sy = float(slit_y(g[0]))
        h = half_h * math.sqrt(max(0.0, 1 - (g[0] / xmax) ** 2))
        p = np.array([g[0] * 0.95, sy + side[k] * h, g[2] - depth])
        zs.append(p[2])
        inner.append(bm.verts.new(Vector((p[0], -p[2], p[1]))))
    cy = float(np.mean([slit_y(v.co.x) for v in ring]))
    centre = bm.verts.new(Vector((0.0, -(float(np.mean(zs)) - 0.035), cy)))
    out = []
    n = len(ring)
    for i in range(n):
        j = (i + 1) % n
        out.append(bm.faces.new((ring[i], ring[j], inner[j], inner[i])))
        out.append(bm.faces.new((inner[i], inner[j], centre)))
    G = lambda idx: np.array([[ring[k].co.x, ring[k].co.z, -ring[k].co.y] for k in idx])
    return out, G(up_idx), G(lo_idx)


def cut_mouth(ob, L, half_gap=0.0035, depth=0.05, width_frac=0.92, debug=None):
    """Delete the faces forming the lip slit, extrude the hole's boundary backwards and cap it:
    the closed mouth keeps a thin dark slit; an opened jaw reveals a dark inner pouch
    instead of stretched skin. Returns info dict."""
    me = ob.data
    P = verts_g(me)
    R = Rig(P, L)
    xs, ys, zs = slit_curve(P, R)
    slit_y = lambda x: np.interp(x, xs, ys)
    slit_z = lambda x: np.interp(x, xs, zs)
    if debug is not None:
        debug['slit'] = (xs, ys, zs)
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.faces.ensure_lookup_table()
    w = R.w
    kill = []
    for f in bm.faces:
        c = f.calc_center_median()
        cg = np.array([c.x, c.z, -c.y])
        if abs(cg[0]) < w * width_frac and abs(cg[1] - slit_y(cg[0])) < half_gap and cg[2] - slit_z(cg[0]) < 0.014 \
                and cg[2] > slit_z(cg[0]) - 0.03:
            kill.append(f)
    print('mouth cut: faces removed', len(kill))
    bmesh.ops.delete(bm, geom=kill, context='FACES_ONLY')

    def near_mouth(v):
        g = np.array([v.co.x, v.co.z, -v.co.y])
        return abs(g[0]) < w * 1.2 and abs(g[1] - slit_y(g[0])) < 0.03 and abs(g[2] - slit_z(g[0])) < 0.06

    # make the hole boundary a simple loop: grow the hole around pinch points until no vertex has > 2 boundary edges
    for it in range(12):
        bmesh.ops.delete(bm, geom=[v for v in bm.verts if not v.link_faces], context='VERTS')
        bedges = [e for e in bm.edges if e.is_boundary and all(near_mouth(v) for v in e.verts)]
        cnt = {}
        for e in bedges:
            for v in e.verts:
                cnt[v] = cnt.get(v, 0) + 1
        pinch = [v for v, c in cnt.items() if c > 2]
        if not pinch:
            break
        fs = set(f for v in pinch for f in v.link_faces)
        print('mouth cut: pinch vertices', len(pinch), '-> removing', len(fs), 'faces')
        bmesh.ops.delete(bm, geom=list(fs), context='FACES_ONLY')
        kill = kill + list(fs)
    # connected loops
    loops, seen = [], set()
    for e in bedges:
        if e in seen:
            continue
        comp, stack = [], [e]
        seen.add(e)
        while stack:
            cur = stack.pop()
            comp.append(cur)
            for v in cur.verts:
                for e2 in v.link_edges:
                    if e2 in bedges and e2 not in seen:
                        seen.add(e2)
                        stack.append(e2)
        loops.append(comp)
    loops.sort(key=len, reverse=True)
    print('mouth cut: boundary loops', [len(l) for l in loops])
    uvc = find_free_uv(me)
    uvlay = bm.loops.layers.uv.active
    inner_layer = bm.faces.layers.int.new('haru_inner')
    rims = None
    n_inner = 0
    for comp in loops:
        if len(comp) < 8:
            try:        # tiny leftover hole: just fill it
                bmesh.ops.triangle_fill(bm, edges=comp, use_beauty=True)
            except Exception as ex:
                print('fill small failed', ex)
            continue
        ring = order_loop(comp)
        if len(ring) != len(comp):
            print('WARNING: mouth loop not simple', len(ring), len(comp))
        newf, rim_up, rim_lo = build_pouch(bm, ring, slit_y, depth)
        rims = (rim_up, rim_lo)
        n_inner += len(newf)
        for f in newf:
            f.smooth = True
            f[inner_layer] = 1
            if uvc is not None and uvlay is not None:
                for lp in f.loops:
                    lp[uvlay].uv = uvc
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(me)
    bm.free()
    me.update()
    return {'removed': len(kill), 'loops': [len(l) for l in loops], 'uv': uvc, 'rims': rims, 'n_inner': n_inner}


def paint_inner_mouth(img, uvc, size=None, col=(0.045, 0.012, 0.014), half=0.014):
    """Paint the (free) atlas block used by the inner-mouth surfaces dark."""
    if img is None or uvc is None:
        return
    w, h = img.size
    a = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)
    u0, v0 = int(uvc[0] * w), int(uvc[1] * h)
    r = int(half * w)
    a[max(v0 - r, 0):v0 + r, max(u0 - r, 0):u0 + r, :3] = col
    img.pixels = a.ravel().tolist()
    img.pack()


# --------------------------------------------------------------------------
# morph targets: smooth symmetric displacement fields (glTF frame)
# --------------------------------------------------------------------------
MORPHS = ['jawOpen', 'lipsWide', 'lipsRound', 'lipsPress', 'smile', 'browRaise', 'browFrown', 'blink']
JAW_DEG = 13.0


def gauss(d2, sigma):
    return np.exp(-d2 / (sigma * sigma))


def rot_x(P, J, ang):
    """Rotate points about the X axis through J by per-vertex angle (rad).
    Positive angle moves the chin (forward, below J) down and back."""
    d = P - J
    c, s_ = np.cos(ang), np.sin(ang)
    y = d[:, 1] * c - d[:, 2] * s_
    z = d[:, 1] * s_ + d[:, 2] * c
    return np.stack([P[:, 0], J[1] + y, J[2] + z], axis=1)


class Rig:
    """Landmark-derived helper fields for one head."""

    def __init__(self, P, L, slit=None, rims=None):
        lm = L['landmarks']
        self.slit = slit
        self.rims = rims
        self.P = P
        self.M = np.array(lm['mouth'])
        self.C = np.array(lm['mouthL'])
        self.J = np.array(lm['jawPivot'])
        self.K = np.array(lm['chin'])
        self.E = np.array(lm['eyeL'])
        self.w = abs(self.C[0])
        self.eye = L['eye']
        self.brow = L['brow']
        x, y, z = P[:, 0], P[:, 1], P[:, 2]
        self.x, self.y, self.z = x, y, z
        self.ax = np.abs(x)
        # face-front mask: keeps hair/ears/neck back untouched
        self.FM = ramp(z, 0.04, 0.16) * (1 - ramp(self.ax, 0.23, 0.33))
        self.ylip = self.lip_y(x)

    def lip_y(self, x):
        if self.slit is not None:      # measured notch line of this head
            return np.interp(x, self.slit[0], self.slit[1])
        t = np.clip(np.abs(x) / self.w, 0, 1.25)
        return self.M[1] + (self.C[1] - self.M[1]) * t * t


def morph_jaw(R, deg=JAW_DEG):
    P, J, w = R.P, R.J, R.w
    A = ramp(R.ylip - R.y, -0.0015, 0.0025)
    B = (1 - 0.5 * ramp(R.ax, w * 0.45, w * 1.0)) * (1 - ramp(R.ax, w * 0.8, w * 3.0))
    C = ramp(R.z, J[2] + 0.03, J[2] + 0.24)
    ychin = R.K[1] - 0.012
    D = 1 - ramp(ychin - R.y, 0.0, 0.10)
    Wt = A * B * C * D
    Q = rot_x(P, J, np.radians(deg) * Wt)
    return Q - P


def lip_band(R, sy=0.045, x0=0.15, x1=1.0, fall=(1.15, 2.2)):
    """lateral h(x) * vertical v(y) mask around the lip line (corner-weighted)."""
    h = ramp(R.ax, x0 * R.w, x1 * R.w) * (1 - ramp(R.ax, fall[0] * R.w, fall[1] * R.w))
    v = gauss((R.y - R.ylip) ** 2, sy)
    return h, v


def morph_wide(R):
    d = np.zeros_like(R.P)
    h, v = lip_band(R, sy=0.04, x0=0.2, x1=0.95, fall=(1.1, 2.4))
    m = h * v * R.FM
    sgn = np.sign(R.x)
    d[:, 0] = sgn * 0.04 * m
    d[:, 2] = -0.028 * m * np.clip(R.ax / R.w, 0, 1.2)
    d[:, 1] = -(R.y - R.ylip) * 0.18 * m
    return d


def morph_round(R):
    d = np.zeros_like(R.P)
    w = R.w
    r2 = (R.x / (w * 2.0)) ** 2 + ((R.y - R.M[1]) / 0.13) ** 2
    H = smooth(np.clip(1 - r2, 0, 1)) * R.FM
    near = gauss((R.x / (w * 1.5)) ** 2 + ((R.y - R.M[1]) / 0.06) ** 2, 1.0) * R.FM
    d[:, 0] = -R.x * 0.40 * H
    d[:, 2] = 0.075 * near + 0.02 * H
    d[:, 1] = (R.y - R.ylip) * 0.16 * near
    return d


def morph_press(R):
    d = np.zeros_like(R.P)
    h, v = lip_band(R, sy=0.03, x0=0.0, x1=0.3, fall=(1.1, 1.9))
    m = h * v * R.FM
    d[:, 1] = -(R.y - R.ylip) * 0.55 * m
    d[:, 2] = -0.012 * m
    d[:, 0] = np.sign(R.x) * 0.012 * m * np.clip(R.ax / R.w, 0, 1)
    chin = gauss(((R.y - (R.M[1] - 0.075)) / 0.04) ** 2 + (R.x / 0.09) ** 2, 1.0) * R.FM
    d[:, 1] += 0.008 * chin
    return d


def morph_smile(R):
    d = np.zeros_like(R.P)
    w = R.w
    sgn = np.sign(R.x)
    g = gauss(((R.ax - w) / 0.07) ** 2 + ((R.y - R.C[1]) / 0.06) ** 2, 1.0) * R.FM
    d[:, 1] += 0.050 * g
    d[:, 0] += sgn * 0.038 * g
    d[:, 2] += -0.030 * g
    h, v = lip_band(R, sy=0.04, x0=0.3, x1=1.0, fall=(1.2, 2.2))
    m = h * v * R.FM
    d[:, 1] += 0.020 * m * np.clip(R.ax / w, 0, 1.2) ** 2
    cy = R.M[1] + 0.11
    cg = gauss(((R.ax - 0.165) / 0.075) ** 2 + ((R.y - cy) / 0.07) ** 2, 1.0) * R.FM
    d[:, 1] += 0.022 * cg
    d[:, 2] += 0.014 * cg
    d[:, 0] += sgn * 0.010 * cg
    return d


def brow_dist(R):
    """Distance in (x,y) to the brow polyline inner -> peak -> outer (mirrored by |x|)."""
    b = R.brow
    i, pk = np.array(b['innerL'][:2]), np.array(b['peakL'][:2])
    o = pk + (pk - i) * np.array([0.9, -0.35])
    pts = [i, pk, o]
    q = np.stack([R.ax, R.y], axis=1)
    dm = np.full(len(q), 9.0)
    for a_, b_ in zip(pts[:-1], pts[1:]):
        ab = b_ - a_
        t = np.clip(((q - a_) @ ab) / (ab @ ab), 0, 1)
        proj = a_ + t[:, None] * ab
        dm = np.minimum(dm, np.linalg.norm(q - proj, axis=1))
    return dm


def morph_brow_raise(R):
    d = np.zeros_like(R.P)
    dist = brow_dist(R)
    by = R.brow['peakL'][1]
    eye_margin = R.E[1] + R.eye['hu']
    above = 0.3 + 0.7 * ramp(R.y - eye_margin, 0.005, 0.035)
    bw = gauss(dist ** 2, 0.05) * above * (1 - ramp(R.ax, 0.15, 0.22))
    fore = ramp(R.y, by - 0.01, by + 0.02) * (1 - ramp(R.y, by + 0.09, by + 0.19)) * (1 - ramp(R.ax, 0.11, 0.18))
    m = np.maximum(bw, 0.8 * fore) * R.FM * ramp(R.z, 0.1, 0.25)
    d[:, 1] = 0.034 * m
    d[:, 2] = 0.006 * m
    return d


def morph_brow_frown(R):
    d = np.zeros_like(R.P)
    b = R.brow
    ix, iy = b['innerL'][0], b['innerL'][1]
    g = gauss(((R.ax - (ix + 0.015)) / 0.065) ** 2 + ((R.y - iy) / 0.045) ** 2, 1.0) * R.FM
    d[:, 1] = -0.032 * g
    d[:, 0] = -R.x * 0.30 * g
    d[:, 2] = 0.012 * g
    return d


def morph_blink(R, overshoot=0.0025, lower_share=0.35):
    """Upper lids slide down over the eyeball sphere (centre E, radius R) by rotating about the X axis
    through E; the lower lids rise a little so the closing line is a smooth curve below the iris."""
    P = R.P
    E, eye = R.E, R.eye
    a, hu, hl, Rr = eye['a'], eye['hu'], eye['hl'], eye['R']
    dxl = R.ax - E[0]
    s_ = np.sqrt(np.clip(1 - (dxl / a) ** 2, 0, 1))
    py = R.y - E[1]
    Wx = 1 - ramp(np.abs(dxl), 0.85 * a, 1.3 * a)
    ym = hu * s_
    yl = -hl * s_
    yc = yl * lower_share                     # where both margins meet when closed
    th_u = np.arctan2(ym, Rr) - np.arctan2(yc, Rr)
    th_l = np.arctan2(yc, Rr) - np.arctan2(yl, Rr)
    Wu = ramp(py, ym - 0.005, ym + 0.002) * (1 - ramp(py, ym + 0.012, ym + 0.038))
    Wd = (1 - ramp(py, yl - 0.003, yl + 0.0015)) * ramp(py, yl - 0.026, yl - 0.008)
    FZ = ramp(P[:, 2], 0.12, 0.22)
    pz = P[:, 2] - E[2]
    r = np.hypot(py, pz)
    phi = np.arctan2(py, pz) - th_u * Wu * Wx * FZ + th_l * Wd * Wx * FZ
    r2 = r + overshoot * Wu * Wx * FZ
    d = np.zeros_like(P)
    d[:, 1] = E[1] + r2 * np.sin(phi) - P[:, 1]
    d[:, 2] = E[2] + r2 * np.cos(phi) - P[:, 2]
    d[(Wx * FZ * np.maximum(Wu, Wd)) <= 0] = 0
    return d


def compute_morphs(P, L, slit=None, rims=None):
    R = Rig(P, L, slit, rims)
    out = {
        'jawOpen': morph_jaw(R), 'lipsWide': morph_wide(R), 'lipsRound': morph_round(R),
        'lipsPress': morph_press(R), 'smile': morph_smile(R), 'browRaise': morph_brow_raise(R),
        'browFrown': morph_brow_frown(R), 'blink': morph_blink(R)}
    return [out[n] for n in MORPHS]


def add_shape_keys(ob, deltas):
    me = ob.data
    ob.shape_key_add(name='Basis', from_mix=False)
    base = verts_g(me)
    for name, d in zip(MORPHS, deltas):
        kb = ob.shape_key_add(name=name, from_mix=False)
        co = g_to_b_arr(base + d)
        kb.data.foreach_set('co', co.astype(np.float32).ravel())
        kb.slider_min, kb.slider_max = 0.0, 1.0
    me.shape_keys.use_relative = True
    return me.shape_keys


def set_weights(ob, weights):
    for kb in ob.data.shape_keys.key_blocks:
        if kb.name != 'Basis':
            kb.value = weights.get(kb.name, 0.0)


# --------------------------------------------------------------------------
# demo action, review renders, export
# --------------------------------------------------------------------------
def make_demo_action(ob, fps=24):
    """5 s of keyframed shape keys (talking + blink + brow); kept in the .blend only."""
    sc = bpy.context.scene
    sc.render.fps = fps
    sc.frame_start, sc.frame_end = 1, 120
    kbs = ob.data.shape_keys.key_blocks
    seq = {
        'jawOpen': [(1, 0), (6, .55), (10, .1), (15, .8), (20, .15), (26, .5), (31, .05), (37, .7), (43, .2),
                    (50, .6), (56, 0), (80, 0), (88, .35), (95, 0), (110, .0), (120, 0)],
        'lipsRound': [(1, 0), (13, 0), (16, .8), (22, 0), (36, 0), (38, .9), (45, 0), (120, 0)],
        'lipsWide': [(1, 0), (24, 0), (27, .8), (33, 0), (48, 0), (52, .6), (58, 0), (120, 0)],
        'lipsPress': [(1, 0), (59, 0), (62, 1), (72, 1), (76, 0), (120, 0)],
        'smile': [(1, 0), (80, 0), (96, .9), (120, .9)],
        'browRaise': [(1, 0), (64, 0), (72, 1), (84, 1), (92, 0), (120, 0)],
        'browFrown': [(1, 0), (98, 0), (106, .8), (116, 0), (120, 0)],
        'blink': [(1, 0), (32, 0), (34, 1), (37, 0), (92, 0), (94, 1), (97, 0), (112, 0), (114, 1), (117, 0),
                  (120, 0)],
    }
    key = ob.data.shape_keys
    key.animation_data_create()
    for name, pts in seq.items():
        kb = kbs[name]
        for f, v in pts:
            kb.value = v
            kb.keyframe_insert('value', frame=f)
    act = key.animation_data.action
    act.name = 'demo_talk'
    act.use_fake_user = True
    sc.frame_set(1)


def add_label(text='', size=0.045):
    cu = bpy.data.curves.new('lbl', 'FONT')
    cu.body = text
    cu.size = size
    lb = bpy.data.objects.new('lbl', cu)
    lb.color = (0.05, 0.05, 0.08, 1)
    bpy.context.scene.collection.objects.link(lb)
    return lb


def place_label(lb, kind, view, text):
    lb.data.body = text
    lb.hide_render = not text
    if text == '':
        return
    # top-left corner of the view, facing the camera
    k = view.scale * 0.46
    if kind == 'front':
        lb.location = Vector((view.centre[0] - k, -1.0, view.centre[1] + k))
        lb.rotation_euler = (math.radians(90), 0, 0)
    else:
        # side: image left = +Z_g  (= -Y_b), camera on +X looking -X
        lb.location = Vector((1.0, -(view.centre[0] + k), view.centre[1] + k))
        lb.rotation_euler = (math.radians(90), 0, math.radians(90))


def read_png(path):
    img = bpy.data.images.load(path)
    w, h = img.size
    a = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)
    bpy.data.images.remove(img)
    return a


def write_png(path, a):
    h, w = a.shape[:2]
    img = bpy.data.images.new('out_' + os.path.basename(path), w, h, alpha=False)
    img.pixels = a.astype(np.float32).ravel().tolist()
    img.filepath_raw = path
    img.file_format = 'PNG'
    img.save()
    bpy.data.images.remove(img)


def contact_sheet(tiles, cols, path):
    imgs = [read_png(t) for t in tiles]
    h, w = imgs[0].shape[:2]
    rows = (len(imgs) + cols - 1) // cols
    sheet = np.ones((rows * h, cols * w, 4), dtype=np.float32)
    for i, a in enumerate(imgs):
        r, c = divmod(i, cols)
        # image rows are bottom-up: row 0 of the sheet (top) is last in array order
        y0 = (rows - 1 - r) * h
        sheet[y0:y0 + h, c * w:(c + 1) * w] = a
    write_png(path, sheet)


REVIEW_STATES = [
    ('neutral', {}),
    ('jawOpen', {'jawOpen': 1}), ('lipsWide', {'lipsWide': 1}), ('lipsRound', {'lipsRound': 1}),
    ('lipsPress', {'lipsPress': 1}), ('smile', {'smile': 1}), ('browRaise', {'browRaise': 1}),
    ('browFrown', {'browFrown': 1}), ('blink', {'blink': 1}),
    ('jawOpen+lipsRound', {'jawOpen': 1, 'lipsRound': 1}), ('jawOpen+smile', {'jawOpen': 1, 'smile': 1}),
    ('jawOpen+lipsWide', {'jawOpen': 1, 'lipsWide': 1}),
]


def render_review(ob, outdir, work, has_tex, lmk, n_inner=0):
    os.makedirs(outdir, exist_ok=True)
    os.makedirs(work, exist_ok=True)
    sc = setup_scene(900)
    sh = sc.display.shading
    me = ob.data
    clay = me.materials[0]
    clay.diffuse_color = (0.74, 0.72, 0.69, 1)
    lb = add_label()
    lmat = bpy.data.materials.new('lbl')
    lmat.diffuse_color = (0.05, 0.05, 0.08, 1)
    lb.data.materials.append(lmat)
    set_weights(ob, {})
    lb.hide_render = True
    # textured views first (real material: inner mouth uses the painted dark texel block)
    if has_tex:
        sh.color_type = 'TEXTURE'
        place_persp(-25, (0, -0.1, 0), dist=3.0, lens=80)
        render(os.path.join(outdir, 'neutral_textured.png'))
        v = View('front', (0, -0.18), 1.25, 900)
        v.place()
        render(os.path.join(outdir, 'neutral_textured_front.png'))
        set_weights(ob, {'jawOpen': 1})
        place_persp(-25, (0, -0.2, 0.1), dist=1.6, lens=85)
        render(os.path.join(outdir, 'jawOpen_textured.png'))
        set_weights(ob, {'jawOpen': 1, 'smile': 1})
        v = View('front', (0, -0.14), 0.7, 900)
        v.place()
        render(os.path.join(outdir, 'jawOpen_smile_textured_front.png'))
        set_weights(ob, {})
    # clay: inner mouth gets a dark second material (review only; removed again before export)
    dark = bpy.data.materials.new('InnerDark')
    dark.diffuse_color = (0.03, 0.012, 0.014, 1)
    me.materials.append(dark)
    if 'haru_inner' in me.attributes:
        idx = np.zeros(len(me.polygons), dtype=np.int32)
        me.attributes['haru_inner'].data.foreach_get('value', idx)
        me.polygons.foreach_set('material_index', idx)
    sh.color_type = 'MATERIAL'
    v = View('front', (0, -0.18), 1.25, 900)
    v.place()
    render(os.path.join(outdir, 'neutral_front.png'))
    v = View('side', (0.12, -0.18), 1.25, 900)
    v.place()
    render(os.path.join(outdir, 'neutral_side.png'))
    place_persp(-35, (0, -0.1, 0), dist=3.0, lens=80)
    render(os.path.join(outdir, 'neutral_three_quarter.png'))
    # contact sheets
    sc.render.resolution_x = sc.render.resolution_y = 440
    tiles = {'front': [], 'side': [], 'three': [], 'mouth': [], 'eyes': []}
    my_, ey_ = lmk['mouth'][1], lmk['eyeL'][1]
    for name, wts in REVIEW_STATES:
        set_weights(ob, wts)
        for kind in tiles:
            if kind == 'front':
                vw = View('front', (0, -0.12), 0.82, 440)
                vw.place()
                place_label(lb, 'front', vw, name)
            elif kind == 'side':
                vw = View('side', (0.2, -0.12), 0.82, 440)
                vw.place()
                place_label(lb, 'side', vw, name)
            elif kind == 'mouth':
                vw = View('front', (0, my_ - 0.04), 0.42, 440)
                vw.place()
                place_label(lb, 'front', vw, name)
            elif kind == 'eyes':
                vw = View('front', (0, ey_ + 0.03), 0.56, 440)
                vw.place()
                place_label(lb, 'front', vw, name)
            else:
                place_persp(-35, (0, -0.12, 0.1), dist=2.2, lens=85)
                lb.hide_render = True
            p = os.path.join(work, f'tile_{kind}_{name}.png')
            render(p)
            tiles[kind].append(p)
    contact_sheet(tiles['front'], 4, os.path.join(outdir, 'contact_sheet.png'))
    contact_sheet(tiles['side'], 4, os.path.join(outdir, 'contact_sheet_side.png'))
    contact_sheet(tiles['three'], 4, os.path.join(outdir, 'contact_sheet_three_quarter.png'))
    contact_sheet(tiles['mouth'], 4, os.path.join(outdir, 'contact_sheet_mouth.png'))
    contact_sheet(tiles['eyes'], 4, os.path.join(outdir, 'contact_sheet_eyes.png'))
    set_weights(ob, {})
    bpy.data.objects.remove(lb)
    # restore single material
    me.polygons.foreach_set('material_index', np.zeros(len(me.polygons), dtype=np.int32))
    me.materials.pop(index=1)
    if 'haru_inner' in me.attributes:
        me.attributes.remove(me.attributes['haru_inner'])
    bpy.data.materials.remove(dark)
    bpy.data.materials.remove(lmat)


def sha256(p):
    h = hashlib.sha256()
    with open(p, 'rb') as f:
        for ch in iter(lambda: f.read(1 << 20), b''):
            h.update(ch)
    return h.hexdigest()


def export_glb(ob, path):
    bpy.ops.object.select_all(action='DESELECT')
    ob.select_set(True)
    bpy.context.view_layer.objects.active = ob
    set_weights(ob, {})
    os.makedirs(os.path.dirname(path), exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=path, export_format='GLB', use_selection=True, export_apply=False,
        export_animations=False, export_morph=True, export_morph_normal=True, export_morph_tangent=False,
        export_extras=True, export_normals=True, export_texcoords=True, export_materials='EXPORT',
        export_image_format='JPEG', export_jpeg_quality=85, export_yup=True, export_cameras=False,
        export_lights=False, export_skins=False, export_tangents=False, export_vertex_color='NONE')
    print('exported', path, os.path.getsize(path))
    if os.path.getsize(path) > 6_000_000:
        print('WARNING: GLB larger than expected; check the texture JPEG size (target <= ~1.5 MB)')


def stage_build(args):
    subj = args['subject']
    sdir = os.path.join(HERE, subj)
    L = load_json(os.path.join(sdir, 'landmarks.json'))
    assert 'landmarks' in L, 'run --stage landmarks first'
    work = os.path.join(args['work'], subj)
    os.makedirs(work, exist_ok=True)
    ob = import_source(os.path.join(sdir, 'varco-source.glb'))
    clean_mesh(ob)
    normalise(ob, L['normalise'])
    ntri = decimate(ob, L, target=int(args.get('target', 38500)))
    finish_mesh(ob)
    tex_size = TEXTURED_SUBJECTS.get(subj, 1024)
    img = make_material(ob, tex_size)
    if subj in DARK_STRIP and img is not None:
        reserve_dark_strip(ob, img, DARK_STRIP[subj], TEX_GAIN.get(subj, 1.0))
    dbg = {}
    cut = cut_mouth(ob, L, debug=dbg)
    print('mouth', {k: v for k, v in cut.items() if k != 'rims'})
    paint_inner_mouth(img, cut['uv'])
    ntri = sum(len(p.vertices) - 2 for p in ob.data.polygons)
    P = verts_g(ob.data)
    print('head bounds', P.min(0).round(3), P.max(0).round(3))
    deltas = compute_morphs(P, L, (dbg['slit'][0], dbg['slit'][1]), cut['rims'])
    for n, d in zip(MORPHS, deltas):
        print(f'  {n:10s} max |d| {np.abs(d).max():.4f}  moved {(np.abs(d).max(axis=1) > 1e-5).sum()}')
    add_shape_keys(ob, deltas)
    sc = bpy.context.scene
    sc['haru'] = {k: [float(t) for t in v] for k, v in L['landmarks'].items()}
    if args.get('review', '1') == '1':   # (before the action exists: it would override slider values)
        render_review(ob, os.path.join(sdir, 'review'), work, img is not None, L['landmarks'], cut['n_inner'])
    if 'haru_inner' in ob.data.attributes:
        ob.data.attributes.remove(ob.data.attributes['haru_inner'])
    make_demo_action(ob)
    set_weights(ob, {})
    blend = os.path.join(sdir, 'source.blend')
    bpy.context.preferences.filepaths.save_version = 0      # no source.blend1 backup file
    bpy.ops.wm.save_as_mainfile(filepath=blend, compress=True)
    glb = os.path.join(REPO, 'app', 'public', 'heads', f'{subj}.glb')
    if args.get('export', '1') == '1':
        export_glb(ob, glb)
    info = {'triangles': ntri, 'vertices': len(ob.data.vertices)}
    print('BUILD DONE', json.dumps(info))


def main():
    args = parse_args()
    {'renders': stage_renders, 'landmarks': stage_landmarks, 'build': stage_build}.get(args['stage'], stage_renders)(args)


if __name__ == '__main__':
    main()
