"""Prepare selectable Skate 3 character parts from an extracted owned game."""
from pathlib import Path
import argparse
import copy
import json
import struct
import sys

import numpy as np
from PIL import Image

TARGETS = {
    'HIPS': 'j_mainroot', 'SPINE': 'j_spinelower', 'SPINE1': 'j_spineupper',
    'SPINE2': 'j_spineupper', 'SPINE3': 'j_spine4', 'NECK': 'j_neck',
    'NECK1': 'j_neck', 'HEAD': 'j_head',
    'LEFTSHOULDER': 'j_clavicle_le', 'LEFTARM': 'j_shoulder_le',
    'LEFTFOREARM': 'j_elbow_le', 'LEFTHAND': 'j_wrist_le',
    'RIGHTSHOULDER': 'j_clavicle_ri', 'RIGHTARM': 'j_shoulder_ri',
    'RIGHTFOREARM': 'j_elbow_ri', 'RIGHTHAND': 'j_wrist_ri',
    'LEFTUPLEG': 'j_hip_le', 'LEFTLEG': 'j_knee_le', 'LEFTFOOT': 'j_ankle_le',
    'LEFTTOEBASE': 'j_ball_le', 'RIGHTUPLEG': 'j_hip_ri', 'RIGHTLEG': 'j_knee_ri',
    'RIGHTFOOT': 'j_ankle_ri', 'RIGHTTOEBASE': 'j_ball_ri',
}

CHILDREN = {
    'HIPS': 'SPINE', 'SPINE': 'SPINE1', 'SPINE1': 'SPINE3',
    'SPINE2': 'SPINE3', 'SPINE3': 'NECK', 'NECK': 'HEAD', 'NECK1': 'HEAD',
    'LEFTSHOULDER': 'LEFTARM', 'LEFTARM': 'LEFTFOREARM', 'LEFTFOREARM': 'LEFTHAND',
    'RIGHTSHOULDER': 'RIGHTARM', 'RIGHTARM': 'RIGHTFOREARM', 'RIGHTFOREARM': 'RIGHTHAND',
    'LEFTUPLEG': 'LEFTLEG', 'LEFTLEG': 'LEFTFOOT', 'LEFTFOOT': 'LEFTTOEBASE',
    'RIGHTUPLEG': 'RIGHTLEG', 'RIGHTLEG': 'RIGHTFOOT', 'RIGHTFOOT': 'RIGHTTOEBASE',
}

# GLB is Y-up and faces +Z; MW2 is Z-up and faces +X.
WALK_BASIS = np.array([[0., 0., 1.], [1., 0., 0.], [0., 1., 0.]])


def read_glb(path):
    data = path.read_bytes()
    if data[:4] != b'glTF':
        raise ValueError('Invalid character GLB')
    length = struct.unpack_from('<I', data, 12)[0]
    document = json.loads(data[20:20 + length])
    binary = data[28 + length:]
    return document, binary


def accessor(document, binary, index):
    value = document['accessors'][index]
    view = document['bufferViews'][value['bufferView']]
    dtype = {5121: '<u1', 5123: '<u2', 5125: '<u4', 5126: '<f4'}[value['componentType']]
    width = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4, 'MAT4': 16}[value['type']]
    size = np.dtype(dtype).itemsize
    stride = view.get('byteStride', size * width)
    start = view.get('byteOffset', 0) + value.get('byteOffset', 0)
    return np.ndarray((value['count'], width), dtype=dtype, buffer=binary,
                      offset=start, strides=(stride, size)).copy()


def part(path, slots, key):
    import io
    doc, binary = read_glb(path)
    skin = doc['skins'][0]
    names = [doc['nodes'][i]['name'] for i in skin['joints']]
    inverse = accessor(doc, binary, skin['inverseBindMatrices'])
    binds = [np.linalg.inv(m.reshape(4, 4).T) for m in inverse]
    bind_by_name = dict(zip(names, binds))
    native = dict(joints=[dict(target=n, origin=[0, 0, 0]) for n in names], surfaces=[], textures=[])
    walking = dict(joints=[], surfaces=[], textures=[])
    for name, bind in zip(names, binds):
        source = name.removesuffix('_REPARENTED')
        target = TARGETS.get(source, 'j_mainroot')
        joint = dict(target=target, origin=(WALK_BASIS @ bind[:3, 3] / .0254).tolist())
        child = CHILDREN.get(source)
        if child in bind_by_name and TARGETS.get(child) != target:
            joint.update(end=(WALK_BASIS @ bind_by_name[child][:3, 3] / .0254).tolist(),
                         target_child=TARGETS[child])
        walking['joints'].append(joint)
    for mesh in doc['meshes']:
        for primitive in mesh['primitives']:
            material = doc['materials'][primitive['material']]
            slot = material['name'].removeprefix('Retail_')
            if slot not in slots:
                continue
            tex = doc['textures'][material['pbrMetallicRoughness']['baseColorTexture']['index']]
            view = doc['bufferViews'][doc['images'][tex['source']]['bufferView']]
            start = view.get('byteOffset', 0)
            image = Image.open(io.BytesIO(binary[start:start + view['byteLength']])).convert('RGBA')
            image.thumbnail((256, 256), Image.Resampling.BOX)
            texture = dict(width=image.width, height=image.height, rgba=list(image.tobytes()))
            a = primitive['attributes']
            positions = accessor(doc, binary, a['POSITION'])
            normals = accessor(doc, binary, a['NORMAL'])
            uv = accessor(doc, binary, a['TEXCOORD_0']).astype('<f2').view('<u2')
            joints = accessor(doc, binary, a['JOINTS_0'])
            weights = accessor(doc, binary, a['WEIGHTS_0'])
            vertices = [dict(position=p.tolist(), normal=n.tolist(),
                             uv=(int(t[0]) << 16) | int(t[1]),
                             joints=j.tolist(), weights=w.tolist())
                        for p, n, t, j, w in zip(positions, normals, uv, joints, weights)]
            triangles = accessor(doc, binary, primitive['indices']).reshape(-1, 3)[:, [0, 2, 1]].flatten().tolist()
            surface = dict(material='iw4l_character/' + key + '/' + slot,
                           texture=len(native['textures']), vertices=vertices, indices=triangles)
            native['surfaces'].append(surface)
            native['textures'].append(texture)
            copied = copy.deepcopy(surface)
            for v in copied['vertices']:
                v['position'] = (WALK_BASIS @ v['position'] / .0254).tolist()
                v['normal'] = (WALK_BASIS @ v['normal']).tolist()
            walking['surfaces'].append(copied)
            walking['textures'].append(texture)
    if not native['surfaces']:
        raise ValueError('No character surfaces for ' + ', '.join(slots))
    return walking, native


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', type=Path, required=True)
    parser.add_argument('--game', type=Path, required=True)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--limit', type=int, default=8)
    parser.add_argument('--base-only', action='store_true')
    args = parser.parse_args()
    if not 1 <= args.limit <= 15:
        parser.error('--limit must be between 1 and 15')
    args.assets = args.assets.resolve()
    args.engine = args.engine.resolve()
    args.game = args.game.resolve()
    sys.path.insert(0, str(args.engine))
    from tools.asset_pipeline.customisation_catalog import prepare
    from tools.asset_pipeline.customisation_worker import default_profile, build, flag
    from tools.asset_pipeline.customisation_library import friendly
    base = args.assets / 'private/skater.glb'
    document, _ = read_glb(base)
    slots = sorted({m['name'].removeprefix('Retail_') for m in document['materials']
                    if 'Skate' not in m['name']})
    parts = []
    for slot in slots:
        walking, native = part(base, [slot], 'base/' + slot)
        parts.append(dict(category='base', label='Default ' + slot, index=0, slots=[slot],
                          walking=walking, native=native))
    failures = []
    if not args.base_only:
        cache = args.assets / 'private/customisation'
        cache.mkdir(exist_ok=True)
        if not (cache / 'catalog.json').is_file():
            print('Reading the Skate 3 character catalogue', flush=True)
            prepare(args.game, cache)
        catalog = json.loads((cache / 'catalog.json').read_text())
        config = dict(assets=str(args.assets), game_root=str(args.game))
        models = {c['slot']: c['models'] for c in catalog['components']}
        categories = [('shirt', 'OuterTorso', ['OuterTorso', 'InnerTorso', 'Arm']),
                      ('pants', 'Pants', ['Pants', 'Leg']), ('hair', 'Hair', ['Hair']),
                      ('board', 'SkateBoard', ['SkateBoard', 'SkateTruck', 'SkateWheel'])]
        for category, source, surfaces in categories:
            candidates = sorted(models[source], key=lambda m: (m['name'], m['id']))
            count = 0
            for model in candidates:
                if flag(model, 'Gender') not in {'', 'male', 'unisex'}:
                    continue
                lod = next((v for v in model['lods'] if v['index'] == 0), None)
                if not lod or len(lod['material_instances']) != 1:
                    continue
                variants = sorted(lod['material_instances'][0], key=lambda v: v['id'])
                if not variants:
                    continue
                profile = default_profile()
                profile['selections'][source] = dict(asset_id=model['id'], material_id=variants[0]['id'])
                try:
                    result = build(config, profile)
                    key = category + '/' + str(count + 1)
                    walking, native = part(args.assets / result['scene'], surfaces, key)
                except (ValueError, KeyError, OSError, RuntimeError, StopIteration) as error:
                    failures.append(dict(category=category, model=model['id'], error=str(error)))
                    print('Skipped', model['name'], str(error), flush=True)
                    continue
                count += 1
                label = friendly(model['name'])
                if category == 'board':
                    label += ' ' + str(count)
                parts.append(dict(category=category, index=count, label=label,
                                  slots=surfaces, walking=walking, native=native))
                print('Prepared', category, count, friendly(model['name']), flush=True)
                if count == args.limit:
                    break
        tones = sorted({flag(m, 'SkinTone') for m in catalog['materials'].values()} - {''})
        for i, tone in enumerate(tones[:15], 1):
            profile = default_profile()
            profile['skin'] = tone
            try:
                result = build(config, profile)
                surfaces = ['Rostral', 'Organ']
                walking, native = part(args.assets / result['scene'], surfaces, 'skin/' + str(i))
                parts.append(dict(category='skin', index=i, label=tone.title(), slots=surfaces,
                                  walking=walking, native=native))
            except (ValueError, KeyError, OSError, RuntimeError) as error:
                failures.append(dict(category='skin', error=str(error)))
    output = args.assets / 'characters.json'
    pending = output.with_suffix('.json.partial')
    pending.write_text(json.dumps(parts, separators=(',', ':')), encoding='utf-8')
    pending.replace(output)
    (args.assets / 'character-preparation.json').write_text(json.dumps(dict(parts=len(parts), failures=failures), indent=2))
    print('Characters ready:', len(parts), 'parts;', len(failures), 'unavailable choices', flush=True)


if __name__ == '__main__':
    main()
