"""Fetch the pinned, asset-free Skate audio decoder and its source/licenses."""
import argparse
import hashlib
import json
from io import BytesIO
from pathlib import Path
from urllib.request import Request, urlopen
from zipfile import ZipFile, ZIP_DEFLATED

RELEASE = 'https://github.com/vgmstream/vgmstream/releases/download/r2117/vgmstream-win64.zip'
DIGEST = '6c4a8a3813864fefed081bbd337dbc0ad93bf88e0b92f5db98d7ab258b22dc6c'
SOURCES = {
    'vgmstream-r2117.zip': 'https://codeload.github.com/vgmstream/vgmstream/zip/refs/tags/r2117',
    'ffmpeg-n5.1.2.zip': 'https://codeload.github.com/FFmpeg/FFmpeg/zip/refs/tags/n5.1.2',
    'mpg123-source.zip': 'https://codeload.github.com/madebr/mpg123/zip/aec901b7a636b6eb61e03a87ff3547c787e8c693',
    'libg719-source.zip': 'https://codeload.github.com/kode54/libg719_decode/zip/da90ad8a676876c6c47889bcea6a753f9bbf7a73',
    'opus-1.3.1.zip': 'https://codeload.github.com/xiph/opus/zip/refs/tags/v1.3.1',
}


def download(url):
    with urlopen(Request(url, headers={'User-Agent': 'IW4L-Converter-Build'}), timeout=90) as response:
        data = response.read(96 * 1024 * 1024 + 1)
    if len(data) > 96 * 1024 * 1024:
        raise RuntimeError('Decoder download exceeds size limit.')
    return data


def main(out):
    out.mkdir(parents=True, exist_ok=True)
    binary = download(RELEASE)
    if hashlib.sha256(binary).hexdigest() != DIGEST:
        raise RuntimeError('Pinned audio decoder hash mismatch.')
    with ZipFile(BytesIO(binary)) as archive:
        for entry in archive.infolist():
            name = Path(entry.filename).name
            if entry.filename != name:
                raise RuntimeError('Unexpected audio decoder archive path.')
            (out / name).write_bytes(archive.read(entry))
    source_map = {}
    with ZipFile(out / 'audio-decoder-source.zip', 'w', ZIP_DEFLATED) as pack:
        for name, url in SOURCES.items():
            data = download(url)
            source_map[name] = {'url': url, 'sha256': hashlib.sha256(data).hexdigest()}
            pack.writestr(name, data)
            if name == 'vgmstream-r2117.zip':
                with ZipFile(BytesIO(data)) as source:
                    for entry in source.infolist():
                        if '/ext_libs/licenses/' in entry.filename and not entry.is_dir():
                            path = out / 'licenses' / Path(entry.filename).name
                            path.parent.mkdir(exist_ok=True)
                            path.write_bytes(source.read(entry))
        pack.writestr('SOURCES.json', json.dumps(source_map, indent=2))
        pack.writestr('README.txt',
            'vgmstream r2117 official Windows x64 decoder, unmodified.\n'
            'The supplied FFmpeg DLL reports n5.1.2 and LGPL version 2.1 or later.\n'
            'Original decoder build files, dependency configuration and patches are in vgmstream-r2117.zip.\n'
            'FFmpeg, mpg123, G.719 and statically linked Opus source is included.\n'
            'Other component licenses are in vgmstream ext_libs/licenses and the converter audio-decoder/licenses folder.\n'
            'Replace/rebuild decoder DLLs, then rebuild the converter with -AudioDecoder pointing to that folder.\n'
            'Sources and local changes to the converter are in skate/converter-source.zip.\n'
            'No game files are included.\n')
    print('Pinned Skate audio decoder and source prepared:', out.resolve(), flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    main(parser.parse_args().out)
