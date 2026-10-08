param([Parameter(Mandatory=$true)][string]$StatePath, [switch]$RemoveMapping, [ValidateRange(1,60)][int]$PollSeconds = 10)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'Internet-Multiplayer.psm1') -Force
$state = [IO.File]::ReadAllText($StatePath) | ConvertFrom-Json
function Save-State([string]$Status, [string]$Detail = '') {
    $current = [IO.File]::ReadAllText($StatePath) | ConvertFrom-Json
    if ($current.Description -eq $state.Description) {
        $current.Status = $Status
        $current.Detail = $Detail
        Write-Iw4lState $StatePath $current
    }
}
function Remove-OwnedMapping {
    $collection = Get-Iw4lUpnpMappings
    $mapping = Get-Iw4lUpnpMapping $collection $state.ExternalPort
    if ($null -eq $mapping) { Save-State 'closed'; return }
    if (!(Test-Iw4lOwnedMapping $mapping $state)) { Save-State 'closed' 'Mapping was replaced; left its replacement unchanged.'; return }
    $collection.Remove([int]$state.ExternalPort, 'UDP')
    Save-State 'closed' 'UPnP forwarding removed.'
}
if (!$RemoveMapping) {
    $refresh = [DateTime]::UtcNow.AddSeconds(180)
    while (Test-Iw4lRelayProcess $state) {
        Start-Sleep -Seconds $PollSeconds
        $current = [IO.File]::ReadAllText($StatePath) | ConvertFrom-Json
        if ($current.Description -ne $state.Description -or $current.Status -eq 'closed') { return }
        if ([DateTime]::UtcNow -ge $refresh -and (Test-Iw4lRelayProcess $state)) {
            try {
                $collection = Get-Iw4lUpnpMappings
                $mapping = Get-Iw4lUpnpMapping $collection $state.ExternalPort
                if ($mapping -and !(Test-Iw4lOwnedMapping $mapping $state)) { Save-State 'conflict' 'Another mapping occupies this port.'; return }
                if (!$mapping -or !$mapping.Enabled) {
                    $mapping = $collection.Add([int]$state.ExternalPort, 'UDP', [int]$state.InternalPort, $state.InternalClient, $true, $state.Description)
                }
                if ($mapping.ExternalIPAddress -ne $state.ExternalAddress) { Save-State 'address-changed' 'Run Host Internet again to create a new invitation.' }
                else { Save-State 'mapped' }
            } catch { Save-State 'mapping-error' $_.Exception.Message }
            $refresh = [DateTime]::UtcNow.AddSeconds(180)
        }
    }
}
for ($attempt = 0; $attempt -lt 3; $attempt++) {
    try { Remove-OwnedMapping; return } catch {
        if ($attempt -eq 2) { Save-State 'cleanup-failed' ('Run Stop Internet Relay again when the router is available. ' + $_.Exception.Message); throw }
        Start-Sleep -Seconds 2
    }
}
