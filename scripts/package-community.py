"""Build a portable host/join archive from explicit compiled inputs."""
import argparse
import hashlib
import json
import re
import shutil
import subprocess
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile

ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = [
    'Community-Multiplayer.ps1', 'Community-Action.ps1', 'Internet-Multiplayer.psm1',
    'Host-Internet.ps1', 'Join-Internet.ps1', 'Start-Game.ps1', 'Start-Local-Relay.ps1',
    'Setup-Matchmaking.ps1', 'Run-Relay.ps1', 'Watch-Relay-UPnP.ps1', 'Stop-Internet-Relay.ps1',
    'Enable-Relay-Firewall.ps1', 'Configure-Relay.ps1', 'Configure-Audio.ps1', 'Start Multiplayer.bat',
    'Host Internet.bat', 'Join Internet.bat', 'Stop Internet Relay.bat',
    'Update-Multiplayer.ps1', 'Community-Update.psm1',
]


def game_protocol():
    source = (ROOT / 'crates/net/src/lib.rs').read_text(encoding='utf-8')
    values = re.findall(r'^pub const PROTOCOL_VERSION: u32 = ([0-9]+);$', source, re.MULTILINE)
    if len(values) != 1:
        raise RuntimeError('Cannot identify the packaged runtime source protocol')
    return int(values[0])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['game', 'relay', 'launcher', 'converter', 'engine', 'licenses', 'runtime', 'stage', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--audio-decoder-source', type=Path, required=True)
    args = parser.parse_args()
    protocol = game_protocol()
    source_commit = subprocess.check_output(
        ['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True,
    ).strip()
    source_dirty = bool(subprocess.check_output(
        ['git', 'status', '--porcelain', '--untracked-files=normal'], cwd=ROOT,
    ).strip())
    engine_revision = subprocess.check_output(
        ['git', '-C', str(args.engine), 'rev-parse', 'HEAD'], text=True,
    ).strip()
    if not re.fullmatch(r'[0-9a-f]{40}', engine_revision):
        raise RuntimeError('Cannot identify converter engine revision')
    stage = args.stage.resolve()
    stage.mkdir(parents=True, exist_ok=False)

    def copy(source, relative):
        target = stage / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)

    copy(args.game, 'iw4l.exe')
    copy(args.relay, 'iw4l-master.exe')
    copy(args.launcher, 'Multiplayer.exe')
    copy(args.converter, 'skate/iw4l-skate-convert.exe')
    copy(args.audio_decoder_source, 'skate/audio-decoder-source.zip')
    for name in SCRIPTS:
        copy(ROOT / 'scripts' / name, name)
    for name in ['LICENSE', 'NOTICE']:
        copy(ROOT / name, name)
    copy(ROOT / 'crates/ui/assets/OFL-Oxanium.txt', 'OFL-Oxanium.txt')
    copy(ROOT / 'crates/console/assets/COPYING-FreeFont.txt', 'COPYING-FreeFont.txt')
    copy(ROOT / 'crates/ui/src/skate_ui/LICENSE', 'GPL-3.0.txt')
    copy(ROOT / 'docs/MULTIPLAYER.md', 'README.md')
    for source in sorted(args.licenses.rglob('*')):
        if source.is_file():
            copy(source, 'skate/licenses/' + source.relative_to(args.licenses).as_posix())
    copy(args.engine / 'LICENSE', 'skate/licenses/SkateEngine-GPL-3.0.txt')
    for name in ['vcruntime140.dll', 'vcruntime140_1.dll']:
        copy(args.runtime / name, name)
    (stage / '.env').write_text('IW4L_MASTER_MAX_PLAYERS=18\n', encoding='utf-8')
    update = json.loads((ROOT / 'UPDATE.json').read_text(encoding='utf-8'))
    copy(ROOT / 'UPDATE.json', 'UPDATE.json')
    paths = subprocess.check_output(
        ['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'],
        cwd=ROOT,
    ).decode('utf-8').split('\0')
    with ZipFile(stage / 'runtime-source.zip', 'w', ZIP_DEFLATED, compresslevel=6) as source_zip:
        for relative in sorted(set(paths) - {''}):
            source = ROOT / relative
            if not source.is_file() or '__pycache__' in source.parts or source.suffix == '.pyc':
                continue
            if source.is_symlink() or relative == '.env' or relative.startswith(('context/', 'iw4l-artifacts/', 'skate-data/')):
                raise RuntimeError('Private or external file in runtime source: ' + relative)
            source_zip.write(source, relative)
        source_zip.writestr('BUILD-COMMUNITY.txt',
            'Corresponding source for this community build, including local modifications.\n'
            'Install Rust and the Windows MSVC build tools.\n'
            'Build: cargo build --profile play -p launcher -p iw4l-master --locked\n'
            'Game: target/play/iw4l.exe; relay: target/play/iw4l-master.exe.\n'
            'See README.md, NOTICE, and component licenses for provenance and licensing.\n'
            'Original game data must be supplied by each player.\n')
    with ZipFile(stage / 'skate/converter-source.zip', 'w', ZIP_DEFLATED, compresslevel=6) as source_zip:
        for source in sorted((args.engine / 'tools').rglob('*')):
            if not source.is_file():
                continue
            relative = source.relative_to(args.engine).as_posix()
            if '__pycache__' in source.parts or source.suffix not in {'.py', '.rs', '.json', '.txt', '.md', '.toml'} and source.name != 'LICENSE':
                continue
            source_zip.write(source, 'engine/' + relative)
        source_zip.write(args.engine / 'LICENSE', 'engine/LICENSE')
        for name in ['iw4l_skate_convert.py', 'build.ps1']:
            source_zip.write(ROOT / 'skate/converter' / name, 'skate/converter/' + name)
        source_zip.write(ROOT / 'scripts/prepare-characters.py', 'scripts/prepare-characters.py')
        source_zip.write(ROOT / 'scripts/prepare-creator-assets.py', 'scripts/prepare-creator-assets.py')
        source_zip.write(ROOT / 'scripts/prepare-hall-of-meat.py', 'scripts/prepare-hall-of-meat.py')
        source_zip.write(ROOT / 'scripts/prepare-skate-audio.py', 'scripts/prepare-skate-audio.py')
        source_zip.writestr('README.txt',
            'Skate converter corresponding source.\n'
            f'Engine tools: SK8-ENGINE/skate-3-rust-engine at {engine_revision}, including local modifications.\n'
            'Tools retain their upstream licenses; see engine/LICENSE and the license folder.\n'
            'Build: pwsh skate/converter/build.ps1 -SkateEngine engine -Out dist -AudioDecoder <vgmstream r2117 win64 folder>\n'
            'Audio decoder source and licenses: skate/audio-decoder-source.zip.\n'
            'Needs Rust and Python 3.13, PyInstaller 6.22.3, NumPy 2.4.6, Pillow 11.1.0.\n')
    files = sorted(p for p in stage.rglob('*') if p.is_file())
    manifest = {
        'version': update['version'],
        'source_commit': source_commit,
        'source_dirty': source_dirty,
        'upstream_commit': 'f608f85e407ff1b7689d54a9aafdd16e95711ac4',
        'game_protocol': protocol,
        'wardrobe_converter_commit': engine_revision,
        'files': {p.relative_to(stage).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in files},
    }
    (stage / 'BUILD.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with ZipFile(args.output, 'w', ZIP_DEFLATED, compresslevel=6) as archive:
        for source in sorted(p for p in stage.rglob('*') if p.is_file()):
            archive.write(source, 'IW4L-Community-Multiplayer/' + source.relative_to(stage).as_posix())
    with ZipFile(args.output) as archive:
        if archive.testzip() is not None:
            raise RuntimeError('Archive CRC check failed')
        for name in archive.namelist():
            if any(part in name for part in ['server-key', 'server-cert', 'skate-data/', 'iw4l-artifacts/', 'Invite.txt']):
                raise RuntimeError('User data entered the archive: ' + name)
        for name, digest in manifest['files'].items():
            if hashlib.sha256(archive.read('IW4L-Community-Multiplayer/' + name)).hexdigest() != digest:
                raise RuntimeError('Packaged hash mismatch: ' + name)
    print(str(args.output.resolve()), args.output.stat().st_size, 'bytes; CRC and hashes verified')


if __name__ == '__main__':
    main()
