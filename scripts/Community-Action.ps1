param(
    [ValidateSet('Host','Join','Play','Stop','Update')][string]$Action,
    [string]$Directory = $PSScriptRoot,
    [string]$LogFile,
    [string]$InviteFile,
    [ValidateRange(1,65535)][int]$Port = 4433,
    [ValidateRange(1,65535)][int]$ExternalPort = 4433,
    [string]$Address,
    [int]$LauncherPid,
    [switch]$NoLaunch,
    [switch]$SkipFirewall
)
$ErrorActionPreference = 'Stop'
function Invoke-CommunityAction {
    switch ($Action) {
        'Host' { & (Join-Path $PSScriptRoot 'Host-Internet.ps1') -Directory $Directory -Port $Port -ExternalPort $ExternalPort -Address $Address -NoLaunch:$NoLaunch -SkipFirewall:$SkipFirewall }
        'Join' { & (Join-Path $PSScriptRoot 'Join-Internet.ps1') -Directory $Directory -InviteFile $InviteFile -NoLaunch:$NoLaunch }
        'Play' {
            $config = Join-Path $Directory '.env'
            if (!(Test-Path -LiteralPath $config) -or !(Get-Content -LiteralPath $config | Where-Object { $_ -match '^IW4L_MASTER_ADDR=' -and $_ -notmatch 'YOUR_RELAY' })) {
                throw 'Choose Host Internet or Join Internet first.'
            }
            if (!$NoLaunch) { & (Join-Path $PSScriptRoot 'Start-Game.ps1') -Directory $Directory }
        }
        'Stop' { & (Join-Path $PSScriptRoot 'Stop-Internet-Relay.ps1') -Directory $Directory }
        'Update' { & (Join-Path $PSScriptRoot 'Update-Multiplayer.ps1') -Directory $Directory -LauncherPid $LauncherPid }
    }
}
try {
    if ($LogFile) {
        Invoke-CommunityAction *>&1 | ForEach-Object {
            $line = $_.ToString()
            if ($line -notmatch '^IW4L1\.') { [IO.File]::AppendAllText($LogFile, $line + [Environment]::NewLine) }
        }
    } else { Invoke-CommunityAction }
    exit 0
} catch {
    if ($LogFile) { [IO.File]::AppendAllText($LogFile, 'ERROR: ' + $_.Exception.Message + [Environment]::NewLine) }
    else { Write-Error $_.Exception.Message }
    exit 1
}
