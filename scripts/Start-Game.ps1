param([string]$Directory = $PSScriptRoot, [switch]$HostGame, [string]$Map = 'mp_rust')
$ErrorActionPreference = 'Stop'
$directoryPath = [IO.Path]::GetFullPath($Directory)
if (Test-Path -LiteralPath (Join-Path $directoryPath '.updates/pending.json')) { throw 'An update is pending. Finish it or run Update-Multiplayer.ps1 -Mode Recover before playing.' }
$binary = Join-Path $directoryPath 'iw4l.exe'
if (!(Test-Path -LiteralPath $binary)) { throw 'iw4l.exe must be in this folder.' }
$config = Join-Path $directoryPath '.env'
$address = if (Test-Path -LiteralPath $config) {
    (Get-Content -LiteralPath $config | Where-Object { $_ -match '^IW4L_MASTER_ADDR=' } | Select-Object -First 1) -replace '^IW4L_MASTER_ADDR=', ''
}
if ($address -match '^(127\.0\.0\.1|localhost):([0-9]+)$') {
    & (Join-Path $PSScriptRoot 'Start-Local-Relay.ps1') -Directory $directoryPath -Port ([int]$Matches[2])
}
$env:IW4L_MASTER_JOIN = $null
$env:IW4L_MASTER_HOST_NAME = $null
$arguments = @('menu')
if ($HostGame) {
    if ($Map -notmatch '^[a-zA-Z0-9_]+$') { throw 'Map must be a map name such as mp_rust.' }
    $env:IW4L_CMDS = "wait 1s; set ui_mapname iw4:$Map; set ui_gametype dm; ui_create_lobby; ui_lobby_privacy; $env:IW4L_CMDS"
}
Start-Process -FilePath $binary -ArgumentList $arguments -WorkingDirectory $directoryPath -WindowStyle Hidden | Out-Null
