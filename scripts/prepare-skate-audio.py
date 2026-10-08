"""Decode board and skater effects from the player's extracted Skate 3 files."""
import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
import wave
from pathlib import Path

BANKS = {
    'grind': ['GRINDS.abk'],
    'pop': ['Sk8_Air_Flip_Tricks.abk'],
    'land': ['Skate_Collisions.bnk'],
    'land_metal': ['Skate_Metal.bnk'],
    'impact': ['Bodyslide.abk'],
    'brake': ['FOOT_DRAG.abk'],
    'slide': ['WHEEL_SKID_BANK.abk'],
    'scrape': ['board_scrapes.abk'],
    'rattle': ['Rolling_Rattles.abk'],
    'seams': ['Seams_Bank.abk'],
    'push': ['fstep_skateshoe1_sm.abk'],
    'cloth': ['Foley_Cloth.abk', 'clothes_flap.abk'],
}


def run_decoder(decoder, source, output, *options):
    result = subprocess.run([str(decoder), '-i', '-D', '2', '-o', str(output), *options, str(source)],
                            capture_output=True, text=True, timeout=180,
                            creationflags=subprocess.CREATE_NO_WINDOW if sys.platform == 'win32' else 0)
    if result.returncode:
        raise RuntimeError('Skate audio decoder failed: ' + source.name + '\n' + result.stderr[-1000:])
    if 'failed opening' in result.stdout.lower() or 'failed opening' in result.stderr.lower():
        raise RuntimeError('Skate audio decoder could not open ' + source.name + '\n' + result.stdout[-1000:] + result.stderr[-1000:])
    if not any(output.parent.glob(output.name.replace('?s', '*'))):
        raise RuntimeError('Skate audio decoder produced no wave files: ' + source.name + '\n' + result.stdout[-1600:] + result.stderr[-1600:])


def prepare(game, assets, decoder):
    from tools.owned_game.big import BigArchive
    if not decoder.is_file():
        raise RuntimeError('The bundled Skate audio decoder is missing.')
    output = assets / 'private/audio'
    output.mkdir(parents=True, exist_ok=True)
    catalog = {}
    archive = BigArchive(game / 'data/audio/audiofiles.big')
    entries = {Path(e.path).name.casefold(): e for e in archive.entries}
    with tempfile.TemporaryDirectory(prefix='iw4l-skate-audio-') as temporary:
        temporary = Path(temporary)
        for category, banks in BANKS.items():
            folder = temporary / category
            folder.mkdir(exist_ok=True)
            for name in banks:
                entry = entries.get(name.casefold())
                if entry is None:
                    raise RuntimeError('Missing Skate sound bank: ' + name)
                data = archive.read(entry)
                source = temporary / name
                source.write_bytes(data)
                if data[:4] == b'SPLC':
                    offsets = [m.start() for m in re.finditer(rb'\x03[\x00-\x07](?:\xbb\x80|\xac\x44)', data)]
                    for index, offset in enumerate(offsets):
                        snr = temporary / f'{source.stem}-{index + 1}.snr'
                        snr.write_bytes(data[offset:])
                        run_decoder(decoder, snr, folder / f'{source.stem}-{index + 1}.wav')
                else:
                    run_decoder(decoder, source, folder / f'{source.stem}-?s.wav', '-s', '1', '-S', '0')
            catalog[category] = validate(folder, temporary)
            if category in ('land', 'land_metal'):
                catalog[category] = [p for p in catalog[category] if transient(temporary / p)]
            if not catalog[category]:
                raise RuntimeError('No decoded Skate effects: ' + category)
            print('Skate audio:', category, len(catalog[category]), flush=True)
        grains = BigArchive(game / 'data/audio/grains.big')
        folder = temporary / 'rolling'
        folder.mkdir(exist_ok=True)
        for entry in grains.entries:
            data = grains.read(entry)
            offset = int.from_bytes(data[:4], 'big')
            if data[offset:offset + 1] != b'\x03':
                raise RuntimeError('Unsupported rolling grain: ' + entry.path)
            snr = temporary / (Path(entry.path).stem + '.snr')
            snr.write_bytes(data[offset:])
            run_decoder(decoder, snr, folder / (snr.stem + '.wav'))
        catalog['rolling'] = validate(folder, temporary)
        wheels = BigArchive(game / 'data/audio/wheels.big')
        folder = temporary / 'wheels'
        folder.mkdir(exist_ok=True)
        for entry in wheels.entries:
            if Path(entry.path).suffix.lower() != '.snr':
                continue
            snr = temporary / Path(entry.path).name
            snr.write_bytes(wheels.read(entry))
            run_decoder(decoder, snr, folder / (snr.stem + '.wav'))
        catalog['wheels'] = validate(folder, temporary)
        for paths in catalog.values():
            for relative in paths:
                target = output / relative
                target.parent.mkdir(exist_ok=True)
                shutil.copyfile(temporary / relative, target)
    target = output / 'sounds.json'
    stage = output / 'sounds.partial.json'
    stage.write_text(json.dumps({'version': 1, 'categories': catalog}, indent=2), encoding='utf-8')
    stage.replace(target)
    print('Original Skate board audio ready:', sum(map(len, catalog.values())), 'effects', flush=True)


def validate(folder, output):
    paths = []
    for path in sorted(folder.glob('*.wav')):
        with wave.open(str(path), 'rb') as sound:
            if sound.getnchannels() not in (1, 2) or sound.getsampwidth() != 2 or not sound.getnframes():
                raise RuntimeError('Invalid decoded Skate effect: ' + path.name)
        paths.append(path.relative_to(output).as_posix())
    return paths


def transient(path):
    with wave.open(str(path), 'rb') as sound:
        duration = sound.getnframes() / sound.getframerate()
        return 0.06 <= duration <= 0.8


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--engine', type=Path, required=True)
    parser.add_argument('--game', type=Path, required=True)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--decoder', type=Path, required=True)
    args = parser.parse_args()
    sys.path.insert(0, str(args.engine.resolve()))
    prepare(args.game.resolve(), args.assets.resolve(), args.decoder.resolve())
