param([string]$JoinCode, [string]$InviteFile, [string]$Directory = $PSScriptRoot, [switch]$NoLaunch)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'Internet-Multiplayer.psm1') -Force
$directoryPath = [IO.Path]::GetFullPath($Directory)
if ($InviteFile) {
    if ((Get-Item -LiteralPath $InviteFile).Length -gt 16000) { throw 'Invitation file is too large.' }
    $JoinCode = [IO.File]::ReadAllText([IO.Path]::GetFullPath($InviteFile)).Trim()
}
if (!$JoinCode) {
    $JoinCode = (Read-Host 'Paste the IW4L1 invitation code, or the path to the invitation text file').Trim().Trim('"')
    if (Test-Path -LiteralPath $JoinCode -PathType Leaf) {
        if ((Get-Item -LiteralPath $JoinCode).Length -gt 16000) { throw 'Invitation file is too large.' }
        $JoinCode = [IO.File]::ReadAllText([IO.Path]::GetFullPath($JoinCode)).Trim()
    }
}
$invite = Read-Iw4lInvitation $JoinCode
$master = Join-Path $directoryPath 'iw4l-master.exe'
if (!(Test-Path -LiteralPath $master)) { throw 'iw4l-master.exe is missing from the client folder.' }
$caDirectory = Join-Path $directoryPath 'matchmaking'
[IO.Directory]::CreateDirectory($caDirectory) | Out-Null
$relativeCa = 'matchmaking/invite-ca-' + $invite.Fingerprint + '.pem'
$caPath = Join-Path $directoryPath $relativeCa
[IO.File]::WriteAllText($caPath, $invite.CaPem)
$candidates = @($invite.Address)
if ($invite.LanAddress -and (Test-Iw4lLocalEndpoint $invite.LanAddress)) { $candidates = @($invite.LanAddress, $invite.Address) | Select-Object -Unique }
$connected = $null
$details = @()
foreach ($candidate in $candidates) {
    Write-Host "Checking the relay at $candidate..."
    $result = Test-Iw4lRelayConnection $master $candidate $invite.ServerName $caPath
    if ($result.Success) { $connected = $candidate; break }
    $details += $result.Detail
}
if (!$connected) {
    throw ('The relay could not be reached and authenticated. Ask the host to start it and allow UDP through its firewall/router. Your game configuration was preserved. ' + ($details -join ' '))
}
Set-Iw4lRelayConfig $directoryPath $connected $invite.ServerName $relativeCa
Write-Host 'Relay connected and certificate verified. Select the host lobby in Find Lobbies.'
if (!$NoLaunch) {
    $env:IW4L_MASTER_ADDR = $connected
    $env:IW4L_MASTER_SERVER_NAME = $invite.ServerName
    $env:IW4L_MASTER_CA_CERT = $caPath
    $env:IW4L_CMDS = "wait 1s; ui_browser_open; $env:IW4L_CMDS"
    & (Join-Path $PSScriptRoot 'Start-Game.ps1') -Directory $directoryPath
}
