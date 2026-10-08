param([string]$Directory = $PSScriptRoot, [ValidateRange(1,65535)][int]$Port)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'Internet-Multiplayer.psm1') -Force
$directoryPath = [IO.Path]::GetFullPath($Directory)
$binary = Join-Path $directoryPath 'iw4l-master.exe'
$filter = if ($PSBoundParameters.ContainsKey('Port')) { "upnp-$Port-*.json" } else { 'upnp-*.json' }
$states = @(Get-ChildItem -LiteralPath (Join-Path $directoryPath 'matchmaking') -Filter $filter -ErrorAction SilentlyContinue)
foreach ($file in $states) {
    $state = [IO.File]::ReadAllText($file.FullName) | ConvertFrom-Json
    if ($state.RelayPath -ne $binary -or ($PSBoundParameters.ContainsKey('Port') -and $state.InternalPort -ne $Port)) { throw 'Relay state does not belong to this folder/port.' }
    if (Test-Iw4lRelayProcess $state) { Stop-Process -Id $state.RelayPid }
    & (Join-Path $PSScriptRoot 'Watch-Relay-UPnP.ps1') -StatePath $file.FullName -RemoveMapping
}
$endpoints = if ($PSBoundParameters.ContainsKey('Port')) { @(Get-NetUDPEndpoint -LocalPort $Port -ErrorAction SilentlyContinue) } else { @(Get-NetUDPEndpoint -ErrorAction SilentlyContinue) }
foreach ($endpoint in $endpoints) {
    $process = Get-Process -Id $endpoint.OwningProcess -ErrorAction SilentlyContinue
    if ($process -and $process.Path -eq $binary) { Stop-Process -Id $process.Id }
}
Write-Output 'Local Internet relay stopped; its UPnP mappings were removed.'
