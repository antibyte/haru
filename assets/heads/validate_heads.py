#!/usr/bin/env python3
"""
Validate the runtime head GLBs (app/public/heads/<subject>.glb) against the contract in README.md and
optionally record the outputs in <subject>/generation.json ("prepared" section).

    python validate_heads.py [--subjects female male] [--record]

Checks: GLB magic/version, one mesh named "Head" with one primitive (POSITION, NORMAL, TEXCOORD_0, indices),
<= 40,000 triangles, 8 morph targets with POSITION + NORMAL deltas, mesh.extras.targetNames in contract order,
scene.extras.haru landmarks, one embedded JPEG base-colour texture <= 1024^2, finite data, head height ~ 1.
Needs only Python 3 + numpy.
"""
import argparse, datetime, hashlib, json, os, struct, sys
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
TARGETS = ['jawOpen', 'lipsWide', 'lipsRound', 'lipsPress', 'smile', 'browRaise', 'browFrown', 'blink']
LANDMARKS = ['mouth', 'mouthL', 'mouthR', 'eyeL', 'eyeR', 'chin', 'crown', 'noseTip', 'jawPivot']
CT = {5120: np.int8, 5121: np.uint8, 5122: np.int16, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}
NC = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4}


def sha256(p):
    h = hashlib.sha256()
    with open(p, 'rb') as f:
        for ch in iter(lambda: f.read(1 << 20), b''):
            h.update(ch)
    return h.hexdigest()


def read_glb(path):
    data = open(path, 'rb').read()
    magic, version, length = struct.unpack_from('<4sII', data, 0)
    assert magic == b'glTF', 'bad magic'
    assert version == 2, f'bad version {version}'
    assert length == len(data), f'length mismatch {length} != {len(data)}'
    off, js, binc = 12, None, None
    while off < len(data):
        clen, ctype = struct.unpack_from('<II', data, off)
        body = data[off + 8:off + 8 + clen]
        if ctype == 0x4E4F534A:
            js = json.loads(body.decode('utf-8'))
        elif ctype == 0x004E4942:
            binc = body
        off += 8 + clen
    return js, binc


def _view(js, binc, a, bvi, extra_off, dt, n, count):
    bv = js['bufferViews'][bvi]
    stride = bv.get('byteStride', dt.itemsize * n)
    base = bv.get('byteOffset', 0) + extra_off
    if stride == dt.itemsize * n:
        return np.frombuffer(binc, dtype=dt, count=count * n, offset=base)
    raw = np.frombuffer(binc, dtype=np.uint8, count=stride * count, offset=base).reshape(count, stride)
    return raw[:, :dt.itemsize * n].copy().view(dt).reshape(-1)


def accessor(js, binc, idx):
    """Decode an accessor (including sparse ones, which Blender uses for morph targets)."""
    a = js['accessors'][idx]
    dt = np.dtype(CT[a['componentType']])
    n = NC[a['type']]
    if 'bufferView' in a:
        arr = _view(js, binc, a, a['bufferView'], a.get('byteOffset', 0), dt, n, a['count']).astype(dt).copy()
    else:
        arr = np.zeros(a['count'] * n, dtype=dt)
    arr = arr.reshape(a['count'], n)
    sp = a.get('sparse')
    if sp:
        ii = sp['indices']
        idt = np.dtype(CT[ii['componentType']])
        ind = _view(js, binc, a, ii['bufferView'], ii.get('byteOffset', 0), idt, 1, sp['count'])
        vals = _view(js, binc, a, sp['values']['bufferView'], sp['values'].get('byteOffset', 0), dt, n, sp['count'])
        arr[ind.astype(np.int64)] = vals.reshape(sp['count'], n)
    return arr if n > 1 else arr.reshape(-1)


def jpeg_size(b):
    assert b[:2] == b'\xff\xd8', 'not a JPEG'
    i = 2
    while i < len(b):
        assert b[i] == 0xFF
        m = b[i + 1]
        if m in (0xC0, 0xC1, 0xC2):
            h, w = struct.unpack_from('>HH', b, i + 5)
            return w, h
        seg = struct.unpack_from('>H', b, i + 2)[0]
        i += 2 + seg
    raise ValueError('no SOF')


def validate(path):
    errs, info = [], {}
    js, binc = read_glb(path)
    info['glb_version'] = js['asset']['version']
    meshes = js.get('meshes', [])
    if len(meshes) != 1 or meshes[0].get('name') != 'Head':
        errs.append(f'expected exactly one mesh named Head, got {[m.get("name") for m in meshes]}')
    m = meshes[0]
    if len(m['primitives']) != 1:
        errs.append('expected one primitive')
    prim = m['primitives'][0]
    for k in ('POSITION', 'NORMAL', 'TEXCOORD_0'):
        if k not in prim['attributes']:
            errs.append(f'missing attribute {k}')
    if 'indices' not in prim:
        errs.append('missing indices')
    pos = accessor(js, binc, prim['attributes']['POSITION'])
    idx = accessor(js, binc, prim['indices'])
    tris = len(idx) // 3
    info.update(vertices=int(len(pos)), triangles=int(tris), index_type=int(js['accessors'][prim['indices']]['componentType']))
    if tris > 40000:
        errs.append(f'{tris} triangles > 40000')
    if idx.max() >= len(pos):
        errs.append('index out of range')
    bmin, bmax = pos.min(0), pos.max(0)
    info['bounds_min'] = [round(float(v), 4) for v in bmin]
    info['bounds_max'] = [round(float(v), 4) for v in bmax]
    nrm = accessor(js, binc, prim['attributes']['NORMAL'])
    ln = np.linalg.norm(nrm, axis=1)
    if not np.isfinite(pos).all() or not np.isfinite(nrm).all() or abs(ln.mean() - 1) > 0.02:
        errs.append('non-finite or non-unit normals')
    # morph targets
    targets = prim.get('targets', [])
    info['morph_targets'] = len(targets)
    if len(targets) != len(TARGETS):
        errs.append(f'{len(targets)} morph targets, expected {len(TARGETS)}')
    names = (m.get('extras') or {}).get('targetNames')
    info['targetNames'] = names
    if names != TARGETS:
        errs.append(f'targetNames {names} != contract order {TARGETS}')
    maxd = {}
    for i, t in enumerate(targets):
        if 'POSITION' not in t or 'NORMAL' not in t:
            errs.append(f'target {i} lacks POSITION/NORMAL')
            continue
        dp = accessor(js, binc, t['POSITION'])
        dn = accessor(js, binc, t['NORMAL'])
        if len(dp) != len(pos) or len(dn) != len(pos):
            errs.append(f'target {i} count mismatch')
        if not np.isfinite(dp).all() or not np.isfinite(dn).all():
            errs.append(f'target {i} non-finite')
        maxd[TARGETS[i] if i < len(TARGETS) else i] = round(float(np.abs(dp).max()), 4)
        if np.abs(dp).max() < 1e-4:
            errs.append(f'target {i} is empty')
    info['max_position_delta'] = maxd
    # landmarks
    scenes = js.get('scenes', [])
    haru = ((scenes[0].get('extras') or {}).get('haru')) if scenes else None
    if not haru:
        errs.append('scene.extras.haru missing')
        haru = {}
    for k in LANDMARKS:
        if k not in haru or len(haru[k]) != 3:
            errs.append(f'landmark {k} missing')
    info['landmarks'] = {k: [round(v, 4) for v in haru[k]] for k in LANDMARKS if k in haru}
    if 'crown' in haru and 'chin' in haru and 'mouth' in haru:
        h = float(bmax[1] - bmin[1])
        info['crown_minus_chin'] = round(haru['crown'][1] - haru['chin'][1], 3)
        if not 0.9 <= haru['crown'][1] - haru['chin'][1] <= 1.1:
            errs.append('crown-chin height not ~1')
        if not haru['jawPivot'][2] < haru['mouth'][2] and haru['eyeL'][0] > 0 > haru['eyeR'][0]:
            errs.append('landmark orientation looks wrong')
    # node / scene sanity: no non-identity transform on the mesh node
    nodes = js.get('nodes', [])
    for n in nodes:
        if 'mesh' in n and any(k in n for k in ('matrix', 'rotation', 'scale')):
            if n.get('rotation') not in (None, [0, 0, 0, 1]) or n.get('scale') not in (None, [1, 1, 1]):
                errs.append('mesh node has a non-identity transform')
    if js.get('animations'):
        errs.append('runtime file should not contain animations')
    # texture
    imgs = js.get('images', [])
    info['images'] = len(imgs)
    if len(imgs) > 1:
        errs.append('more than one image')
    for im in imgs:
        bv = js['bufferViews'][im['bufferView']]
        raw = binc[bv.get('byteOffset', 0):bv.get('byteOffset', 0) + bv['byteLength']]
        w, h = jpeg_size(raw)
        info['texture'] = {'mime': im.get('mimeType'), 'width': w, 'height': h, 'bytes': len(raw)}
        if im.get('mimeType') != 'image/jpeg':
            errs.append('texture is not JPEG')
        if w > 1024 or h > 1024:
            errs.append(f'texture {w}x{h} > 1024')
    return errs, info


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--subjects', nargs='*', default=['female', 'male'])
    ap.add_argument('--record', action='store_true', help='write the "prepared" section into generation.json')
    ap.add_argument('--blender', default='5.3.0')
    a = ap.parse_args()
    bad = 0
    for s in a.subjects:
        glb = os.path.join(REPO, 'app', 'public', 'heads', f'{s}.glb')
        blend = os.path.join(HERE, s, 'source.blend')
        errs, info = validate(glb)
        print(f'== {s}: {"OK" if not errs else "FAIL"}  ({glb})')
        print(json.dumps(info, indent=1))
        for e in errs:
            print('  ERROR:', e)
        bad += len(errs)
        if a.record and not errs:
            gp = os.path.join(HERE, s, 'generation.json')
            g = json.load(open(gp, encoding='utf-8'))
            g['prepared'] = {
                'date': datetime.date.today().isoformat(),
                'tool': f'Blender {a.blender} (headless)',
                'script': 'assets/heads/build_head.py',
                'validator': 'assets/heads/validate_heads.py',
                'landmarks_file': f'assets/heads/{s}/landmarks.json',
                'source_blend': {'path': f'assets/heads/{s}/source.blend', 'bytes': os.path.getsize(blend),
                                 'sha256': sha256(blend)},
                'runtime_glb': {'path': f'app/public/heads/{s}.glb', 'bytes': os.path.getsize(glb),
                                'sha256': sha256(glb)},
                'triangles': info['triangles'], 'vertices': info['vertices'],
                'morphTargets': info['targetNames'],
                'texture': info.get('texture'),
                'validation': 'ok',
            }
            json.dump(g, open(gp, 'w', encoding='utf-8'), indent=2)
            open(gp, 'a', encoding='utf-8').write('\n')
            print('  recorded prepared section ->', gp)
    sys.exit(1 if bad else 0)


if __name__ == '__main__':
    main()
