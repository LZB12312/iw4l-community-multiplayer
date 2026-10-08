param([string]$Directory = $PSScriptRoot, [ValidateRange(1,65535)][int]$Port = 4433)
$ErrorActionPreference = 'Continue'
$root = [IO.Path]::GetFullPath($Directory)
$binary = Join-Path $root 'iw4l-master.exe'
$log = Join-Path $root 'matchmaking/relay.log'
& $binary serve --bind "0.0.0.0:$Port" --cert (Join-Path $root 'matchmaking/server-cert.pem') --key (Join-Path $root 'matchmaking/server-key.pem') *>&1 | ForEach-Object {
    [IO.File]::AppendAllText($log, $_.ToString() + [Environment]::NewLine)
}
exit $LASTEXITCODE
