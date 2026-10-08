"""Converts the Skate 3 data IW4L's skate mode reads, from the player's own
extracted default.xex: animation banks, state graphs, input and physics
settings, and the skater model the board and rig are taken from.

Runs the converters of SK8-ENGINE/skate-3-rust-engine (the `tools` tree of the
same revision as `skate/crates`). Nothing is downloaded and nothing from the
game is bundled.

    iw4l-skate-convert --xex <path to default.xex> --out <folder>

Writes <folder>/assets on success; progress lines go to stdout.
"""
from pathlib import Path
import argparse, os, runpy, sys, tempfile, traceback, uuid

ROOT = Path(getattr(sys, '_MEIPASS', Path(__file__).resolve().parent))
sys.path.insert(0, str(ROOT))

REQUIRED = [
    'data/big/miscload.big',
    'data/big/miscboot.big',
    'data/big/db.big',
    'data/content/createacharacter.big',
    'data/content/marquee.big',
    'data/big/fedata.big',
    'data/big/fetexture.big',
    'data/big/fedynamic.big',
]


def run_task(script, args):
    # The converters start their own helper scripts through `--task`.
    script = Path(script)
    if not script.is_absolute():
        script = ROOT / script
    script = script.resolve()
    if not script.is_relative_to((ROOT / 'tools').resolve()):
        raise RuntimeError('Invalid conversion script')
    sys.path.insert(0, str(script.parent))
    sys.argv = [str(script), *args]
    runpy.run_path(str(script), run_name='__main__')
    return 0


def absolute_path(path):
    path = str(path.resolve())
    if os.name == 'nt' and not path.startswith('\\\\?\\'):
        path = '\\\\?\\UNC\\' + path[2:] if path.startswith('\\\\') else '\\\\?\\' + path
    return Path(path)


def convert(xex, out):
    xex = absolute_path(xex)
    if xex.suffix.lower() == '.iso':
        raise RuntimeError('ISO files are not supported. Extract the disc and select its default.xex.')
    if xex.name.lower() != 'default.xex' or not xex.is_file():
        raise RuntimeError(f'Select default.xex from an extracted Skate 3 (Xbox 360) game folder, not {xex.name}.')
    game = xex.parent
    missing = [path for path in REQUIRED if not (game / path).is_file()]
    if missing:
        raise RuntimeError('This folder is missing Skate 3 game data (' + ', '.join(missing) +
                           '). Keep the data folder beside default.xex.')

    from tools.asset_pipeline import asset_exports as exports

    out = absolute_path(out)
    stage = out.with_name(out.name + '.partial-' + uuid.uuid4().hex)
    stage.mkdir(parents=True)

    def report(text):
        print(text, flush=True)

    with tempfile.TemporaryDirectory(prefix='iw4l-skate-', dir=stage.parent) as work, \
            (stage / 'conversion.log').open('w', encoding='utf-8') as log:
        work = Path(work)
        converted = exports.core(game, stage, work, report, log)
        exports.character(game, stage, work, report, log, converted)

    assets = stage / 'assets'
    for needed in ('private/skater.glb', 'private/game.json', 'private/stock/physics-skeletons.json',
                   'private/stock/skater-collections.json'):
        if not (assets / needed).is_file():
            raise RuntimeError(f'Conversion finished without {needed}.')
    prepare_wardrobe(game, assets)
    prepare_creator(game, assets)
    prepare_hall_of_meat(game, assets)
    prepare_audio(game, assets)
    if out.exists():
        out.rename(out.with_name(out.name + '.previous-' + uuid.uuid4().hex))
    stage.rename(out)
    report('Skate 3 data ready')


def prepare_wardrobe(game, assets):
    print('Preparing selectable Skate 3 clothing and boards', flush=True)
    sys.argv = ['prepare-characters', '--engine', str(ROOT), '--game', str(game),
                '--assets', str(assets), '--limit', '8']
    runpy.run_path(str(ROOT / 'prepare-characters.py'), run_name='__main__')


def prepare_hall_of_meat(game, assets):
    print('Preparing original Hall of Meat HUD and skeleton', flush=True)
    sys.argv = ['prepare-hall-of-meat', '--engine', str(ROOT), '--game', str(game), '--assets', str(assets)]
    runpy.run_path(str(ROOT / 'prepare-hall-of-meat.py'), run_name='__main__')


def prepare_creator(game, assets):
    print('Preparing full Skate 3 characters and original creator UI', flush=True)
    sys.argv = ['prepare-creator-assets', '--engine', str(ROOT), '--game', str(game), '--assets', str(assets)]
    runpy.run_path(str(ROOT / 'prepare-creator-assets.py'), run_name='__main__')


def prepare_audio(game, assets):
    print('Preparing original Skate board sound effects', flush=True)
    decoder = Path(os.environ.get('IW4L_SKATE_AUDIO_DECODER', ROOT / 'audio-decoder/vgmstream-cli.exe'))
    sys.argv = ['prepare-skate-audio', '--engine', str(ROOT), '--game', str(game),
                '--assets', str(assets), '--decoder', str(decoder)]
    runpy.run_path(str(ROOT / 'prepare-skate-audio.py'), run_name='__main__')


def main():
    if len(sys.argv) > 2 and sys.argv[1] == '--task':
        return run_task(sys.argv[2], sys.argv[3:])
    parser = argparse.ArgumentParser()
    parser.add_argument('--xex', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--wardrobe-only', action='store_true')
    parser.add_argument('--hall-of-meat-only', action='store_true')
    parser.add_argument('--audio-only', action='store_true')
    parser.add_argument('--creator-only', action='store_true')
    parser.add_argument('--assets', type=Path)
    args = parser.parse_args()
    if args.assets is not None and not args.creator_only:
        parser.error('--assets is only supported with --creator-only')
    try:
        if args.creator_only:
            if not args.xex.is_file() or args.xex.name.lower() != 'default.xex':
                raise RuntimeError('Select an extracted Skate 3 default.xex.')
            assets = absolute_path(args.assets) if args.assets is not None else absolute_path(args.out) / 'assets'
            prepare_creator(absolute_path(args.xex).parent, assets)
        elif args.audio_only:
            if not args.xex.is_file() or args.xex.name.lower() != 'default.xex':
                raise RuntimeError('Select an extracted Skate 3 default.xex.')
            prepare_audio(absolute_path(args.xex).parent, absolute_path(args.out) / 'assets')
        elif args.hall_of_meat_only:
            if not args.xex.is_file() or args.xex.name.lower() != 'default.xex':
                raise RuntimeError('Select an extracted Skate 3 default.xex.')
            prepare_hall_of_meat(absolute_path(args.xex).parent, absolute_path(args.out) / 'assets')
        elif args.wardrobe_only:
            if not args.xex.is_file() or args.xex.name.lower() != 'default.xex':
                raise RuntimeError('Select an extracted Skate 3 default.xex.')
            prepare_wardrobe(absolute_path(args.xex).parent, absolute_path(args.out) / 'assets')
        else:
            convert(args.xex, args.out)
    except Exception as error:
        traceback.print_exc()
        print(f'ERROR: {error}', flush=True)
        return 2
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
