"""Prepare character choices and the linked creator UI from an owned Skate 3 copy."""
import argparse
import copy
import hashlib
import json
import shutil
import struct
import sys
from pathlib import Path


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    pending = path.with_suffix(path.suffix + '.partial')
    pending.write_text(json.dumps(value, separators=(',', ':')), encoding='utf-8')
    pending.replace(path)


def prepare_library(game, assets):
    from tools.asset_pipeline.customisation_catalog import prepare as catalogue
    from tools.asset_pipeline.customisation_library import prepare as library

    directory = assets / 'private/customisation'
    catalogue(game, directory)
    result = library({'game_root': str(game), 'assets': str(assets),
                      'directory': str(directory), 'library_index': 'library.json'})
    for preset in result['defaults'].values():
        for selected in preset['selections'].values():
            model = result['models'].get(selected['asset_id'])
            if model is None or not any(selected['material_id'] in group for group in model['groups']):
                raise ValueError('Character default is missing its owned model or material')
    if result['errors']:
        print('Unavailable character resources:', len(result['errors']), flush=True)
    print('Character library ready:', len(result['models']), 'models,',
          len(result['materials']), 'materials', flush=True)


def key_hash(name):
    from tools.asset_pipeline.vlt import hash64
    return int(name[5:], 16) if name.startswith('Hash_') else hash64(name)


def configured_fonts(collections, cache, characters, fonts, mappings):
    from tools.asset_pipeline.vlt import hash64

    data = json.loads(collections.read_text(encoding='utf-8'))
    font_class = hash64('fe_fonts')
    rows = {key_hash(row['key']): row for row in data['collections']
            if key_hash(row['class']) == font_class}
    groups = [row for row in data['collections']
              if key_hash(row['class']) == hash64('fe_fonts_group')
              and key_hash(row['key']) == hash64('region_europe')]
    if len(groups) != 1:
        raise ValueError('Missing English creator font configuration')
    arrays = [field['array']['items'] for field in groups[0]['fields'].values()
              if field.get('array', {}).get('element_size') == 24
              and field['array']['items']
              and all(int(item[:16], 16) == font_class for item in field['array']['items'])]
    if len(arrays) != 1:
        raise ValueError('Ambiguous creator font configuration')
    order = []
    for index, item in enumerate(arrays[0]):
        row = rows[int(item[16:32], 16)]
        fields = {key_hash(key): value for key, value in row['fields'].items()}
        apt_name = fields[hash64('AptName')]['data']
        filename = fields[hash64('FileName')]['data']
        font = cache.font_asset(apt_name)
        if font is None or apt_name not in mappings:
            raise ValueError('Missing configured creator font ' + apt_name)
        font_id = -100 - index
        characters.append({'id': font_id, 'type_name': 'font', 'font': {'name': apt_name}})
        fonts[apt_name] = font
        order.append({'font_id': font_id, 'apt_name': apt_name, 'file_name': filename})
    return order


def relocate(code, base):
    result = copy.deepcopy(code)
    for instruction in result:
        for key in ('offset', 'next', 'target'):
            if key in instruction:
                instruction[key] += base
        if 'body' in instruction:
            instruction['body'] = relocate(instruction['body'], base)
    return result


def prepare_movie(game, assets):
    from tools.vendor.skate3_ui.project import extract_project
    from tools.vendor.skate3_ui.scene_graph import AssetCache, SceneFlattener
    from tools.vendor.skate3_ui.actions import Actions
    from tools.prepare_hud import font_mapping

    output = assets / 'private/creator'
    button_bundle = 'data/fe/source/images/buttons/xbox360/buttons'
    manifest = extract_project(game, output,
                               prefixes=('data/fe/source/screens/cas/', 'data/fe/source/controls/',
                                         button_bundle), include_dynamic=True, update=True)
    if manifest['summary'].get('errors'):
        raise ValueError('Creator UI extraction failed')
    cache = AssetCache(output)
    collections = assets / 'private/stock/skater-collections.json'
    mappings = font_mapping(collections, cache)
    name = 'data/fe/source/screens/cas/createskater'
    ordered, visited = [], set()

    def visit(bundle_name):
        bundle = cache.load_bundle(bundle_name)
        if bundle['key'] in visited:
            return
        visited.add(bundle['key'])
        for imported in bundle['imports'].values():
            visit(imported['file'])
        ordered.append(bundle)

    visit(name)
    main = cache.load_bundle(name)
    ids = {(main['key'], 0): 0}
    for bundle in [main] + [b for b in ordered if b is not main]:
        for character_id in bundle['characters']:
            ids.setdefault((bundle['key'], character_id), len(ids))

    def resolve_id(bundle, character_id):
        resolved, character = cache.resolve_character(bundle['key'], character_id)
        return ids[(resolved['key'], character['id'])]

    characters, shapes, fonts, blocks = [], {}, {}, {}
    initial, exports, bundle_exports = [], {}, {}
    sources = []
    for index, bundle in enumerate(ordered):
        action_base = index * 0x1000000
        apt = output / 'raw' / (bundle['key'] + '.apt')
        raw = apt.read_bytes()
        if len(raw) >= 0x1000000:
            raise ValueError('Creator action namespace exceeded')
        actions = Actions(raw, apt.with_suffix('.const').read_bytes())
        sources.append({'bundle': bundle['key'], 'apt_sha256': hashlib.sha256(raw).hexdigest()})

        def add_action(offset):
            relocated = offset + action_base
            blocks.setdefault(str(relocated), relocate(actions.stream(offset), action_base))
            return relocated

        for source in bundle['characters'].values():
            character = copy.deepcopy(source)
            character_id = character['id'] = ids[(bundle['key'], source['id'])]
            character['source_bundle'] = bundle['key']
            if character['type_name'] == 'shape':
                scene = SceneFlattener(cache, lambda *_: {}).flatten(bundle['key'], source['id'])
                if scene['unresolved']:
                    raise ValueError('Unresolved creator shape')
                shapes[str(character_id)] = scene['primitives']
            elif character['type_name'] == 'font':
                family = character['font']['name']
                fonts[family] = cache.font_asset(family)
                if fonts[family] is None:
                    raise ValueError('Missing creator font ' + family)
                character['font']['glyph_character_ids'] = [resolve_id(bundle, c) for c in
                                                             character['font'].get('glyph_character_ids', [])]
            if character.get('text'):
                character['text']['font_id'] = resolve_id(bundle, character['text']['font_id'])
            for frame in character.get('frames', []):
                for control in frame['controls']:
                    if control.get('character_id', -1) is not None and control.get('character_id', -1) >= 0:
                        control['character_id'] = resolve_id(bundle, control['character_id'])
                    offset = control.get('actions_offset', 0)
                    if not offset:
                        continue
                    if control['type_name'] in ('do_action', 'do_init_action'):
                        control['actions_offset'] = add_action(offset)
                        if control['type_name'] == 'do_init_action':
                            initial.append(control['actions_offset'])
                    elif control['type_name'].startswith('place_object') and control.get('flags', 0) & 0x80:
                        count, pointer = struct.unpack_from('>II', raw, offset)
                        if count > 256 or pointer + count * 12 > len(raw):
                            raise ValueError('Invalid creator clip events')
                        control['clip_events'] = []
                        for event in range(count):
                            flags, key, code = struct.unpack_from('>III', raw, pointer + event * 12)
                            control['clip_events'].append({'flags': flags, 'key': key,
                                                          'actions_offset': add_action(code)})
                        control['actions_offset'] += action_base
                    else:
                        raise ValueError('Unsupported creator action control ' + control['type_name'])
            characters.append(character)
        for label, character_id in bundle['exports'].items():
            resolved = resolve_id(bundle, character_id)
            exports.setdefault(label, resolved)
            bundle_exports.setdefault(bundle['key'], {})[label] = resolved

    font_order = configured_fonts(collections, cache, characters, fonts, mappings)
    buttons = [row for row in manifest['bundles'] if row['name'] == button_bundle]
    if len(buttons) != 1:
        raise ValueError('Missing original Xbox 360 controller images')
    texture_manifest = Path(buttons[0]['textures']['manifest'])
    textures = json.loads((output / texture_manifest).read_text(encoding='utf-8'))['textures']
    images = {}
    for texture in textures:
        if not texture.get('rgba_file') or texture.get('preview_error'):
            raise ValueError('Undecoded creator controller image')
        if texture['name'] in images:
            raise ValueError('Duplicate creator controller image')
        images[texture['name']] = {'width': texture['width'], 'height': texture['height'],
                                   'rgba': (texture_manifest.parent / texture['rgba_file']).as_posix()}
    language = json.loads((output / 'metadata/languages/english_global.json').read_text(encoding='utf-8'))
    for font in fonts.values():
        font['definition'].pop('source', None)
    runtime = {'format': 'iw4l-character-creator', 'version': 1, 'sources': sources,
               'characters': characters, 'shapes': shapes, 'fonts': fonts, 'font_mappings': mappings,
               'actions': blocks, 'initial_actions': initial, 'exports': exports,
               'bundle_exports': bundle_exports, 'native_font_order': font_order,
               'custom_images': images,
               'language': {row['label'].strip(): row['value'] for row in language['entries']}}
    write_json(output / 'runtime/creator.json', runtime)
    print('Original creator UI ready:', len(characters), 'characters,', len(blocks),
          'action blocks,', len(images), 'controller images', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('engine', 'game', 'assets'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    engine, game, assets = args.engine.resolve(), args.game.resolve(), args.assets.resolve()
    if game == assets or game.is_relative_to(assets) or assets.is_relative_to(game):
        raise ValueError('Creator cache must be separate from the owned game')
    sys.path[:0] = [str(engine), str(engine / 'tools')]
    prepare_library(game, assets)
    prepare_movie(game, assets)
    for gender in ('male', 'female'):
        name = 'cac_edit_' + gender + '.abin'
        source = game / 'data/anim' / name
        destination = assets / 'private/creator/animations' / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        pending = destination.with_suffix('.partial')
        shutil.copyfile(source, pending)
        pending.replace(destination)


if __name__ == '__main__':
    main()
