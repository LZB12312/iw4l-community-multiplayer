# Builds iw4l-skate-convert.exe from a checkout of
# SK8-ENGINE/skate-3-rust-engine at the revision `skate/crates` was taken from.
#
#   pwsh skate/converter/build.ps1 -SkateEngine <checkout> -Out <folder>
#
# Needs Python 3.13 with pyinstaller, numpy and Pillow
# (the checkout's tools/requirements-setup.txt) and rustc.
param(
    [Parameter(Mandatory)] [string] $SkateEngine,
    [Parameter(Mandatory)] [string] $Out,
    [Parameter(Mandatory)] [string] $AudioDecoder
)
$ErrorActionPreference = 'Stop'
$SkateEngine = (Resolve-Path $SkateEngine).Path
$AudioDecoder = (Resolve-Path $AudioDecoder).Path
if (!(Test-Path -LiteralPath (Join-Path $AudioDecoder 'vgmstream-cli.exe'))) { throw 'AudioDecoder needs the vgmstream r2117 x64 decoder and its DLLs.' }
$revision = (& git -C $SkateEngine rev-parse --short HEAD).Trim()
if ($revision -ne 'cb79689') { Write-Warning "skate engine checkout is at $revision, skate/crates came from cb79689" }

$work = Join-Path ([IO.Path]::GetTempPath()) ('iw4l-skate-convert-build-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force "$work/tools" | Out-Null
New-Item -ItemType Directory -Force "$work/audio-decoder" | Out-Null
foreach ($source in Get-ChildItem -LiteralPath $AudioDecoder -File) {
    if ($source.Extension -in '.exe','.dll','.md' -or $source.Name -eq 'COPYING') { Copy-Item -LiteralPath $source.FullName -Destination "$work/audio-decoder" }
}
if (Test-Path -LiteralPath (Join-Path $AudioDecoder 'licenses')) { Copy-Item -LiteralPath (Join-Path $AudioDecoder 'licenses') -Destination "$work/audio-decoder/licenses" -Recurse }

& rustc --edition 2024 --crate-type cdylib -C opt-level=3 -C panic=abort -C target-feature=+crt-static `
    "$SkateEngine/tools/asset_pipeline/refpack_native.rs" -o "$work/refpack.dll"
if ($LASTEXITCODE -ne 0) { throw 'refpack.dll build failed' }

# The converters' sources, without editor-only authoring scripts or tests.
$tools = Join-Path $SkateEngine 'tools'
foreach ($source in Get-ChildItem -LiteralPath $tools -File -Recurse) {
    if ($source.FullName -match '[\\/]__pycache__[\\/]') { continue }
    if ($source.Extension -notin '.py','.json','.txt','.md','.toml' -and $source.Name -ne 'LICENSE') { continue }
    $relative = [IO.Path]::GetRelativePath($tools, $source.FullName).Replace('\', '/')
    if ($relative -like 'mixamo_to_skate/*.json' -or $relative -match '(^|/)test_[^/]*\.py$' -or
        $relative -match '(^|/)blender[^/]*(/|$)') { continue }
    $destination = Join-Path "$work/tools" $relative
    New-Item -ItemType Directory -Force (Split-Path -Parent $destination) | Out-Null
    Copy-Item -LiteralPath $source.FullName -Destination $destination
}

& python -m PyInstaller --noconfirm --clean --onefile --console --name iw4l-skate-convert `
    --paths $SkateEngine `
    --hidden-import numpy --hidden-import PIL.Image --hidden-import wave `
    --add-binary "$work/refpack.dll;tools/asset_pipeline" `
    --add-data "$work/tools;tools" `
    --add-data "$(Join-Path $PSScriptRoot '../../scripts/prepare-characters.py');." `
    --add-data "$(Join-Path $PSScriptRoot '../../scripts/prepare-creator-assets.py');." `
    --add-data "$(Join-Path $PSScriptRoot '../../scripts/prepare-hall-of-meat.py');." `
    --add-data "$(Join-Path $PSScriptRoot '../../scripts/prepare-skate-audio.py');." `
    --add-data "$work/audio-decoder;audio-decoder" `
    --exclude-module bpy --exclude-module mathutils --exclude-module tkinter `
    --copy-metadata numpy --copy-metadata Pillow `
    --distpath $Out --workpath "$work/build" --specpath "$work" `
    (Join-Path $PSScriptRoot 'iw4l_skate_convert.py')
if ($LASTEXITCODE -ne 0) { throw 'PyInstaller failed' }
