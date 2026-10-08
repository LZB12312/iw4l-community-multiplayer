"""Prepare the authored Hall of Meat HUD and skeleton from an owned Skate 3 copy."""
import argparse
import hashlib
import json
import shutil
import sys
import tempfile
import xml.etree.ElementTree as ET
from pathlib import Path

from PIL import Image


def prepare_hud(game, assets):
    from tools.vendor.skate3_ui.project import extract_project
    from tools.vendor.skate3_ui.scene_graph import AssetCache, SceneFlattener
    from tools.vendor.skate3_ui.actions import Actions
    from tools.prepare_hud import font_mapping

    output = assets / 'private/hom'
    name = 'data/fe/source/screens/hud2/homscoring'
    manifest = extract_project(game, output, prefixes=(name,), update=True)
    if manifest['summary'].get('errors'):
        raise ValueError('Hall of Meat UI extraction failed')
    cache = AssetCache(output)
    mappings = font_mapping(assets / 'private/stock/skater-collections.json', cache)
    bundle = cache.load_bundle(name)
    if bundle['imports']:
        raise ValueError('Unexpected linked Hall of Meat UI assets')
    apt = output / 'raw' / (name + '.apt')
    actions = Actions(apt.read_bytes(), apt.with_suffix('.const').read_bytes())
    blocks, shapes, fonts = {}, {}, {}
    for character in bundle['characters'].values():
        for frame in character.get('frames', []):
            for control in frame['controls']:
                if control['type_name'] in ('do_action', 'do_init_action') and control.get('actions_offset'):
                    offset = control['actions_offset']
                    blocks[str(offset)] = actions.stream(offset)
        if character['type_name'] == 'shape':
            scene = SceneFlattener(cache, lambda *_: {}).flatten(name, character['id'])
            if scene['unresolved']:
                raise ValueError('Unresolved Hall of Meat shape')
            shapes[str(character['id'])] = scene['primitives']
        elif character['type_name'] == 'font':
            family = character['font']['name']
            fonts[family] = cache.font_asset(family)
            if fonts[family] is None:
                raise ValueError('Missing authored HUD font ' + family)
    language = json.loads((output / 'metadata/languages/english_global.json').read_text(encoding='utf-8'))
    runtime = {
        'format': 'iw4l-hall-of-meat', 'version': 1,
        'source': {'bundle': name, 'apt_sha256': hashlib.sha256(apt.read_bytes()).hexdigest()},
        'characters': list(bundle['characters'].values()), 'actions': blocks,
        'shapes': shapes, 'fonts': fonts, 'font_mappings': mappings,
        'language': {row['label'].strip(): row['value'] for row in language['entries']},
    }
    target = output / 'runtime/homscoring.json'
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(runtime, separators=(',', ':')), encoding='utf-8')
    print('Original Hall of Meat HUD ready:', len(shapes), 'shapes', flush=True)


def prepare_skeleton(engine, game, assets):
    from tools.owned_game.big import BigArchive
    from tools.extract_default_skater import import_rx2_parser, decode_texture
    from tools.asset_pipeline.character_glb import convert
    from tools.asset_pipeline.native_roster import finalize_glb
    from tools.asset_pipeline.retail_character import RX2, decode_dense_morphs
    from importlib.util import spec_from_file_location, module_from_spec

    spec = spec_from_file_location('iw4l_character_parts', Path(__file__).with_name('prepare-characters.py'))
    parts = module_from_spec(spec)
    spec.loader.exec_module(parts)
    archive = BigArchive(game / 'data/content/marquee.big')
    entries = {entry.path: entry for entry in archive.entries}
    parser = import_rx2_parser(engine / 'tools/vendor/utt')
    with tempfile.TemporaryDirectory(prefix='hom-', dir=assets / 'private') as temporary:
        work = Path(temporary)
        def extract(name):
            path = work / 'source' / name
            if not path.exists():
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(archive.read(entries[name]))
            return path
        xml = extract('data/content/recipe/marquee/dem_bones_hom.xml')
        root = ET.fromstring(xml.read_bytes())
        materials = {mat.attrib['id']: mat for mat in root.findall('mat')}
        models, maps = work / 'models', work / 'materials'
        models.mkdir()
        maps.mkdir()
        recipe = {'components': [], 'morph_assembly': {'expected_targets': {}, 'face_targets': []},
                  'preset': {'body_mods': {}}}
        for component in root.findall('comp'):
            slot = component.attrib['n']
            mods = component.findall('mod')
            if len(mods) != 1:
                raise ValueError('Ambiguous skeleton section ' + slot)
            lod = next(l for l in mods[0].findall('lod') if l.get('idx') == '0')
            raw = extract(f"data/content/marquee/model/dem_bones_hom/{slot}/{lod.attrib['arenaid']}.rx2")
            dest = models / slot / raw.name
            dest.parent.mkdir()
            shutil.copyfile(raw, dest)
            material = materials[lod.find('matinst/matvar').attrib['id']]
            textures = {s.attrib['chn']: s.attrib['id'].removeprefix('0x') for s in material.findall('sp')}
            image = work / (textures['diffuse'] + '.png')
            if not image.exists():
                decode_texture(parser, extract('data/content/marquee/texture/0x' + textures['diffuse'] + '.rx2'), image)
            Image.open(image).convert('RGBA').save(maps / (slot + '_base_color.png'))
            parsed = RX2.parse_rx2(str(dest))
            mesh = next(m for m in parsed['meshes'] if m.get('positions') and m.get('indices'))
            morphs = decode_dense_morphs(dest, parsed, len(mesh['positions']), RX2)
            recipe['morph_assembly']['expected_targets'][slot] = [m['name'] for m in morphs]
            recipe['components'].append({'slot': slot, 'tint': [1, 1, 1], 'textures': textures, 'alpha_mode': 'OPAQUE'})
        output = work / 'skeleton.glb'
        convert(models, assets / 'private', recipe, output=output, materials=maps)
        finalize_glb(output)
        _, native = parts.part(output, [c['slot'] for c in recipe['components']], 'hom')
        target = assets / 'skeleton.json'
        pending = target.with_suffix('.json.partial')
        pending.write_text(json.dumps(native, separators=(',', ':')), encoding='utf-8')
        pending.replace(target)
        print('Original Hall of Meat skeleton ready:', len(native['surfaces']), 'sections', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('engine', 'game', 'assets'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    engine, game, assets = args.engine.resolve(), args.game.resolve(), args.assets.resolve()
    sys.path[:0] = [str(engine), str(engine / 'tools')]
    prepare_hud(game, assets)
    prepare_skeleton(engine, game, assets)


if __name__ == '__main__':
    main()
