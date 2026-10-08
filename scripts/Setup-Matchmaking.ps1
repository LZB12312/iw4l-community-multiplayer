param(
    [string]$Directory = (Join-Path $PSScriptRoot 'matchmaking'),
    [string]$ServerName = 'iw4l-community',
    [string]$Binary = (Join-Path $PSScriptRoot 'iw4l-master.exe')
)
$ErrorActionPreference = 'Stop'
$directoryPath = [IO.Path]::GetFullPath($Directory)
$paths = @('iw4l-ca.pem', 'server-cert.pem', 'server-key.pem') | ForEach-Object { Join-Path $directoryPath $_ }
$found = @($paths | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf })
if ($found.Count -eq 3) { Write-Host 'Relay identity already exists.'; return }
if ($found.Count -ne 0) { throw 'Incomplete relay identity. Choose a new directory to preserve existing files.' }
if (!(Test-Path -LiteralPath $Binary -PathType Leaf)) { throw 'iw4l-master.exe is missing. Re-extract the multiplayer package.' }
& $Binary init --directory $directoryPath --server-name $ServerName
if ($LASTEXITCODE -ne 0) { throw 'Relay identity creation failed.' }
