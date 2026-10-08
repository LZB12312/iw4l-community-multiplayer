param(
    [string]$Directory = $PSScriptRoot,
    [ValidateRange(1,65535)][int]$Port = 4433,
    [ValidateRange(1,65535)][int]$ExternalPort = 4433,
    [string]$ServerName = 'iw4l-community',
    [string]$Address,
    [switch]$NoLaunch,
    [switch]$SkipFirewall
)
$ErrorActionPreference = 'Stop'
$usingUpnp = !$Address
Import-Module (Join-Path $PSScriptRoot 'Internet-Multiplayer.psm1') -Force
$directoryPath = [IO.Path]::GetFullPath($Directory)
$ca = Join-Path $directoryPath 'matchmaking/iw4l-ca.pem'
if (!(Test-Path -LiteralPath (Join-Path $directoryPath 'matchmaking/server-cert.pem')) -or !(Test-Path -LiteralPath (Join-Path $directoryPath 'matchmaking/server-key.pem'))) {
    $generated = Join-Path $directoryPath ('matchmaking/generated-' + [Guid]::NewGuid().ToString('N'))
    & (Join-Path $PSScriptRoot 'Setup-Matchmaking.ps1') -Directory $generated -ServerName $ServerName -Binary (Join-Path $directoryPath 'iw4l-master.exe')
    if (Test-Path -LiteralPath $ca) { Copy-Item -LiteralPath $ca -Destination (Join-Path $directoryPath ('matchmaking/received-ca-' + [Guid]::NewGuid().ToString('N') + '.pem')) }
    foreach ($name in @('iw4l-ca.pem','server-cert.pem','server-key.pem')) { Copy-Item -LiteralPath (Join-Path $generated $name) -Destination (Join-Path $directoryPath ('matchmaking/' + $name)) }
}
$null = New-Iw4lInvitation "127.0.0.1:$Port" $ServerName ([IO.File]::ReadAllText($ca))
if (!$SkipFirewall -and !(Test-Iw4lFirewallReady (Join-Path $directoryPath 'iw4l-master.exe') $Port)) {
    try {
        $firewallScript = Join-Path $PSScriptRoot 'Enable-Relay-Firewall.ps1'
        $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
        $principal = [Security.Principal.WindowsPrincipal]::new($identity)
        if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
            & $firewallScript -Directory $directoryPath -Port $Port
        } else {
            Write-Host 'Windows will ask for administrator permission to allow the relay through its firewall.'
            $firewall = Start-Process -FilePath 'powershell.exe' -Verb RunAs -WindowStyle Hidden -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',('"' + $firewallScript + '"'),'-Directory',('"' + $directoryPath + '"'),'-Port',$Port) -Wait -PassThru
            if ($firewall.ExitCode -ne 0) { throw 'Firewall setup did not complete.' }
        }
    } catch { Write-Warning ('Windows Firewall may still block Internet players. Run Host Internet again and approve its Windows permission prompt. ' + $_.Exception.Message) }
}
$relay = & (Join-Path $PSScriptRoot 'Start-Local-Relay.ps1') -Directory $directoryPath -Port $Port -PassThru
$connection = Test-Iw4lRelayConnection (Join-Path $directoryPath 'iw4l-master.exe') "127.0.0.1:$Port" $ServerName $ca
if (!$connection.Success) { throw ('The local relay failed its certificate/protocol check. ' + $connection.Detail) }
if (!$Address) {
    $internal = Get-Iw4lLanAddress
    $collection = Get-Iw4lUpnpMappings
    $statePath = Join-Path $directoryPath "matchmaking/upnp-$Port-$ExternalPort.json"
    $previous = if (Test-Path -LiteralPath $statePath) { [IO.File]::ReadAllText($statePath) | ConvertFrom-Json }
    $mapping = Get-Iw4lUpnpMapping $collection $ExternalPort
    if ($mapping) {
        if (!$previous -or $previous.RelayPath -ne (Join-Path $directoryPath 'iw4l-master.exe') -or !(Test-Iw4lOwnedMapping $mapping $previous)) {
            throw "UDP $ExternalPort already has another mapping. Use -ExternalPort with an unused port; the existing rule was preserved."
        }
        $state = $previous
        if (!(Test-Iw4lRelayProcess $previous)) {
            $oldWatcher = Get-Process -Id $previous.WatcherPid -ErrorAction SilentlyContinue
            if ($oldWatcher -and $oldWatcher.StartTime.ToUniversalTime().Ticks.ToString() -eq $previous.WatcherStarted) { Stop-Process -Id $oldWatcher.Id }
            $state.RelayPid = $relay.Id
            $state.RelayStarted = $relay.StartTime.ToUniversalTime().Ticks.ToString()
            $state.WatcherPid = 0
            $state.WatcherStarted = ''
        }
    } else {
        $state = [pscustomobject]@{
            Description = 'IW4L relay ' + [Guid]::NewGuid().ToString('N').Substring(0,12)
            InternalClient = $internal; InternalPort = $Port; ExternalPort = $ExternalPort
            ExternalAddress = ''; RelayPid = $relay.Id; RelayPath = $relay.Path
            RelayStarted = $relay.StartTime.ToUniversalTime().Ticks.ToString()
            WatcherPid = 0; WatcherStarted = ''; Status = 'starting'; Detail = ''
        }
        $mapping = $collection.Add($ExternalPort, 'UDP', $Port, $internal, $true, $state.Description)
    }
    if (!(Test-Iw4lOwnedMapping $mapping $state) -or !$mapping.Enabled) { throw 'The router did not confirm the requested mapping.' }
    $externalIp = $mapping.ExternalIPAddress
    if (!(Test-Iw4lPublicIpv4 $externalIp)) {
        if (Test-Iw4lOwnedMapping (Get-Iw4lUpnpMapping $collection $ExternalPort) $state) { $collection.Remove($ExternalPort, 'UDP') }
        throw 'The router reports a private/shared WAN address (double NAT or CGNAT). UPnP on this router cannot make it publicly reachable. Use a public relay or ask the ISP for a public IPv4 address.'
    }
    $state.ExternalAddress = $externalIp
    $state.Status = 'mapped'
    $state.Detail = ''
    Write-Iw4lState $statePath $state
    $watcher = if ($state.WatcherPid -gt 0) { Get-Process -Id $state.WatcherPid -ErrorAction SilentlyContinue }
    $watcherRunning = $watcher -and $watcher.StartTime -and $watcher.StartTime.ToUniversalTime().Ticks.ToString() -eq $state.WatcherStarted
    if (!$watcherRunning) {
        try {
            $script = Join-Path $PSScriptRoot 'Watch-Relay-UPnP.ps1'
            # Shell execution gives the watcher its own hidden console instead
            # of inheriting a launcher pipe that keeps the launcher waiting.
            $start = [Diagnostics.ProcessStartInfo]::new()
            $start.FileName = 'powershell.exe'
            $start.Arguments = '-NoProfile -ExecutionPolicy Bypass -File "' + $script + '" -StatePath "' + $statePath + '"'
            $start.UseShellExecute = $true
            $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
            $watcher = [Diagnostics.Process]::Start($start)
            $state.WatcherPid = $watcher.Id
            $state.WatcherStarted = $watcher.StartTime.ToUniversalTime().Ticks.ToString()
            Write-Iw4lState $statePath $state
        } catch {
            if (Test-Iw4lOwnedMapping (Get-Iw4lUpnpMapping $collection $ExternalPort) $state) { $collection.Remove($ExternalPort, 'UDP') }
            throw
        }
    }
    $Address = "$($externalIp):$ExternalPort"
    Write-Host "Router confirmed UDP $ExternalPort forwarding to $($internal):$Port."
} else {
    $null = Get-Iw4lEndpoint $Address
    Write-Host 'Using the supplied public address. UPnP was not requested; configure UDP forwarding yourself.'
}
$lanAddress = $null
try { $lanAddress = (Get-Iw4lLanAddress) + ':' + $Port } catch { }
$code = New-Iw4lInvitation $Address $ServerName ([IO.File]::ReadAllText($ca)) -LanAddress $lanAddress
[IO.File]::WriteAllText((Join-Path $directoryPath 'matchmaking/host-settings.json'), ([ordered]@{Port=$Port; ExternalPort=$ExternalPort; ServerName=$ServerName} | ConvertTo-Json))
$invitePath = Join-Path $directoryPath 'matchmaking/Internet Invite.txt'
[IO.File]::WriteAllText($invitePath, $code + "`n")
Set-Iw4lRelayConfig $directoryPath "127.0.0.1:$Port" $ServerName 'matchmaking/iw4l-ca.pem'
Write-Host "Invitation saved to $invitePath"
Write-Host 'Friends open Join Internet and paste this code (or use the invitation text file):'
Write-Output $code
if ($usingUpnp) { Write-Host 'Router mapping is confirmed; an outside player must still verify Internet reachability. The host connects locally, so NAT loopback is not required.' }
else { Write-Host 'Invitation created for the supplied address. Forwarding and Internet reachability still need verification.' }
if (!$NoLaunch) {
    try { Set-Clipboard -Value $code; Write-Host 'Invitation copied to the clipboard.' } catch { Write-Warning 'Copy the invitation from Internet Invite.txt instead.' }
    $env:IW4L_MASTER_ADDR = "127.0.0.1:$Port"
    $env:IW4L_MASTER_SERVER_NAME = $ServerName
    $env:IW4L_MASTER_CA_CERT = $ca
    & (Join-Path $PSScriptRoot 'Start-Game.ps1') -Directory $directoryPath -HostGame
}
