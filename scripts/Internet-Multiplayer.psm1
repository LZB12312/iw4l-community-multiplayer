Set-StrictMode -Version 2.0

function Get-Iw4lEndpoint {
    param([string]$Address)
    if ($Address -notmatch '^(\[[0-9a-fA-F:]+\]|[a-zA-Z0-9][a-zA-Z0-9.-]*):([0-9]{1,5})$') { throw 'Use a hostname or IP followed by a port.' }
    $port = [int]$Matches[2]
    $hostName = $Matches[1].Trim('[', ']')
    if ($port -notin 1..65535) { throw 'Relay port must be between 1 and 65535.' }
    [pscustomobject]@{ Address = $Address; HostName = $hostName; Port = $port }
}

function Test-Iw4lPublicIpv4 {
    param([string]$Address)
    $ip = $null
    if (![Net.IPAddress]::TryParse($Address, [ref]$ip) -or $ip.AddressFamily -ne [Net.Sockets.AddressFamily]::InterNetwork) { return $false }
    $b = $ip.GetAddressBytes()
    return !($b[0] -eq 0 -or $b[0] -eq 10 -or $b[0] -eq 127 -or $b[0] -ge 224 -or
        ($b[0] -eq 100 -and $b[1] -ge 64 -and $b[1] -le 127) -or
        ($b[0] -eq 169 -and $b[1] -eq 254) -or
        ($b[0] -eq 172 -and $b[1] -ge 16 -and $b[1] -le 31) -or
        ($b[0] -eq 192 -and $b[1] -eq 168) -or
        ($b[0] -eq 192 -and $b[1] -eq 0 -and $b[2] -in 0,2) -or
        ($b[0] -eq 198 -and $b[1] -in 18,19) -or
        ($b[0] -eq 198 -and $b[1] -eq 51 -and $b[2] -eq 100) -or
        ($b[0] -eq 203 -and $b[1] -eq 0 -and $b[2] -eq 113))
}

function Get-Iw4lLanAddress {
    $routes = Get-NetRoute -AddressFamily IPv4 -DestinationPrefix '0.0.0.0/0' -ErrorAction Stop |
        Where-Object { $_.NextHop -ne '0.0.0.0' } | Sort-Object @{Expression={ $_.RouteMetric + $_.InterfaceMetric }}
    foreach ($route in $routes) {
        $ip = Get-NetIPAddress -InterfaceIndex $route.InterfaceIndex -AddressFamily IPv4 -ErrorAction SilentlyContinue |
            Where-Object { !$_.SkipAsSource -and $_.AddressState -eq 'Preferred' -and $_.IPAddress -notlike '169.254.*' } | Select-Object -First 1
        if ($ip) { return $ip.IPAddress }
    }
    throw 'No connected IPv4 Internet interface was found.'
}

function Get-Iw4lUpnpMappings {
    $nat = New-Object -ComObject HNetCfg.NATUPnP
    $collection = $nat.StaticPortMappingCollection
    if ($null -eq $collection) { throw 'The router did not provide UPnP. Enable UPnP on the router, forward UDP manually, or use a public relay.' }
    return ,$collection
}

function Get-Iw4lUpnpMapping {
    param($Mappings, [int]$ExternalPort)
    # Enumeration avoids interpreting a failed COM Item call as an unused port.
    foreach ($entry in $Mappings) {
        if ($entry.ExternalPort -eq $ExternalPort -and $entry.Protocol -eq 'UDP') { return $entry }
    }
    return $null
}

function Test-Iw4lOwnedMapping {
    param($Mapping, $State)
    return $null -ne $Mapping -and $Mapping.InternalClient -eq $State.InternalClient -and
        $Mapping.InternalPort -eq $State.InternalPort -and $Mapping.Description -eq $State.Description
}

function Test-Iw4lRelayProcess {
    param($State)
    $process = Get-Process -Id $State.RelayPid -ErrorAction SilentlyContinue
    if (!$process) { return $false }
    return $process.Path -eq $State.RelayPath -and $process.StartTime.ToUniversalTime().Ticks.ToString() -eq $State.RelayStarted
}

function Write-Iw4lState {
    param([string]$Path, $State)
    $temporary = $Path + '.' + [Guid]::NewGuid().ToString('N') + '.tmp'
    try {
        [IO.File]::WriteAllText($temporary, ($State | ConvertTo-Json -Depth 4))
        if ([IO.File]::Exists($Path)) { [IO.File]::Replace($temporary, $Path, $Path + '.previous') }
        else { [IO.File]::Move($temporary, $Path) }
    } finally { if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) } }
}

function Read-Iw4lCa {
    param([string]$Pem)
    if ($Pem.Length -gt 8192 -or $Pem -notmatch '\A\s*-----BEGIN CERTIFICATE-----\s*([A-Za-z0-9+/=\s]+)-----END CERTIFICATE-----\s*\z') { throw 'Invitation must contain one public CA certificate.' }
    try { $bytes = [Convert]::FromBase64String(($Matches[1] -replace '\s','')) } catch { throw 'Invalid CA certificate encoding.' }
    $certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new($bytes)
    try {
        $constraints = $certificate.Extensions | Where-Object { $_.Oid.Value -eq '2.5.29.19' } | Select-Object -First 1
        if (!$constraints -or !$constraints.CertificateAuthority -or $certificate.HasPrivateKey) { throw 'Invitation certificate must be a public CA.' }
        if ($certificate.NotBefore.ToUniversalTime() -gt [DateTime]::UtcNow -or $certificate.NotAfter.ToUniversalTime() -le [DateTime]::UtcNow) { throw 'Invitation CA is not currently valid.' }
        $hash = [Security.Cryptography.SHA256]::Create()
        try { $fingerprint = ([BitConverter]::ToString($hash.ComputeHash($bytes))).Replace('-','').ToLowerInvariant() } finally { $hash.Dispose() }
        return [pscustomobject]@{ Pem = $Pem; Fingerprint = $fingerprint }
    } finally { $certificate.Dispose() }
}

function New-Iw4lInvitation {
    param([string]$Address, [string]$ServerName, [string]$CaPem, [string]$LanAddress)
    $null = Get-Iw4lEndpoint $Address
    if ($ServerName -notmatch '^[a-zA-Z0-9][a-zA-Z0-9.-]{0,252}$') { throw 'Invalid certificate server name.' }
    $null = Read-Iw4lCa $CaPem
    $json = [ordered]@{ v=1; address=$Address; server_name=$ServerName; ca=$CaPem } | ConvertTo-Json -Compress
    if ($LanAddress) {
        $null = Get-Iw4lEndpoint $LanAddress
        $json = [ordered]@{ v=2; address=$Address; lan_address=$LanAddress; server_name=$ServerName; ca=$CaPem } | ConvertTo-Json -Compress
    }
    return 'IW4L1.' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($json)).TrimEnd('=').Replace('+','-').Replace('/','_')
}

function Read-Iw4lInvitation {
    param([string]$Code)
    if ($Code.Length -gt 16000) { throw 'Invitation is too long.' }
    if ($Code -notmatch '^IW4L1\.([A-Za-z0-9_-]+)$') { throw 'Paste an IW4L1 invitation code from the relay host.' }
    $encoded = $Matches[1].Replace('-','+').Replace('_','/')
    $encoded += '=' * ((4 - $encoded.Length % 4) % 4)
    try {
        $utf8 = [Text.UTF8Encoding]::new($false, $true)
        $invite = $utf8.GetString([Convert]::FromBase64String($encoded)) | ConvertFrom-Json -ErrorAction Stop
    } catch { throw 'Invalid invitation encoding.' }
    $version = if ($invite -and $invite.PSObject.Properties['v']) { $invite.v } else { 0 }
    $fields = if ($version -eq 2) { 5 } else { 4 }
    if ($null -eq $invite -or $invite -is [array] -or @($invite.PSObject.Properties.Name).Count -ne $fields -or
        !$invite.PSObject.Properties['v'] -or !$invite.PSObject.Properties['address'] -or
        !$invite.PSObject.Properties['server_name'] -or !$invite.PSObject.Properties['ca'] -or
        $invite.v -notin 1,2 -or ($invite.v -isnot [int] -and $invite.v -isnot [long]) -or $invite.address -isnot [string] -or
        $invite.server_name -isnot [string] -or $invite.ca -isnot [string]) { throw 'Unsupported invitation format.' }
    $null = Get-Iw4lEndpoint $invite.address
    $lan = $null
    if ($version -eq 2) {
        if (!$invite.PSObject.Properties['lan_address'] -or $invite.lan_address -isnot [string]) { throw 'Invalid invitation LAN address.' }
        $null = Get-Iw4lEndpoint $invite.lan_address
        $lan = $invite.lan_address
    }
    if ($invite.server_name -notmatch '^[a-zA-Z0-9][a-zA-Z0-9.-]{0,252}$') { throw 'Invalid invitation certificate name.' }
    $ca = Read-Iw4lCa $invite.ca
    return [pscustomobject]@{ Address=$invite.address; LanAddress=$lan; ServerName=$invite.server_name; CaPem=$ca.Pem; Fingerprint=$ca.Fingerprint }
}

function Test-Iw4lLocalEndpoint {
    param([string]$Address)
    $endpoint = Get-Iw4lEndpoint $Address
    $target = $null
    if (![Net.IPAddress]::TryParse($endpoint.HostName, [ref]$target) -or $target.AddressFamily -ne [Net.Sockets.AddressFamily]::InterNetwork) { return $false }
    if ([Net.IPAddress]::IsLoopback($target)) { return $true }
    $bytes = $target.GetAddressBytes()
    foreach ($local in @(Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object { $_.AddressState -eq 'Preferred' -and $_.PrefixLength -gt 0 })) {
        $own = [Net.IPAddress]::Parse($local.IPAddress).GetAddressBytes()
        $remaining = [int]$local.PrefixLength
        $same = $true
        for ($index = 0; $index -lt 4; $index++) {
            $bits = [Math]::Min(8, [Math]::Max(0, $remaining))
            $mask = if ($bits) { (255 -shl (8 - $bits)) -band 255 } else { 0 }
            if (($own[$index] -band $mask) -ne ($bytes[$index] -band $mask)) { $same = $false; break }
            $remaining -= 8
        }
        if ($same) { return $true }
    }
    return $false
}

function Set-Iw4lRelayConfig {
    param([string]$Directory, [string]$Address, [string]$ServerName, [string]$CaPath)
    $null = Get-Iw4lEndpoint $Address
    $config = Join-Path $Directory '.env'
    $lines = if (Test-Path -LiteralPath $config) { @([IO.File]::ReadAllLines($config)) } else { @() }
    $lines = @($lines | Where-Object { $_ -notmatch '^\s*IW4L_MASTER_(ADDR|SERVER_NAME|CA_CERT)\s*=' })
    $lines += @("IW4L_MASTER_ADDR=$Address", "IW4L_MASTER_SERVER_NAME=$ServerName", "IW4L_MASTER_CA_CERT=$CaPath")
    [IO.File]::WriteAllText($config, ($lines -join "`n") + "`n")
}

function Test-Iw4lRelayConnection {
    param([string]$Binary, [string]$Address, [string]$ServerName, [string]$CaPath)
    $endpoint = Get-Iw4lEndpoint $Address
    $addresses = @([Net.Dns]::GetHostAddresses($endpoint.HostName))
    $ip = $addresses | Where-Object { $_.AddressFamily -eq [Net.Sockets.AddressFamily]::InterNetwork } | Select-Object -First 1
    if (!$ip) { $ip = $addresses | Select-Object -First 1 }
    if (!$ip) { throw 'The relay hostname did not resolve.' }
    $target = if ($ip.AddressFamily -eq [Net.Sockets.AddressFamily]::InterNetworkV6) { '[' + $ip.ToString() + ']:' + $endpoint.Port } else { $ip.ToString() + ':' + $endpoint.Port }
    $ErrorActionPreference = 'Continue'
    $result = & $Binary status --connect $target --server-name $ServerName --ca-cert $CaPath 2>&1
    return [pscustomobject]@{ Success=($LASTEXITCODE -eq 0); Detail=($result -join ' ') }
}

function Get-Iw4lFirewallRuleName {
    param([string]$Binary, [int]$Port)
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $id = ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($Binary.ToLowerInvariant())))).Replace('-','').Substring(0,12) } finally { $sha.Dispose() }
    return "IW4L-Relay-UDP-$Port-$id"
}

function Test-Iw4lFirewallReady {
    param([string]$Binary, [int]$Port)
    try {
        $policy = New-Object -ComObject HNetCfg.FwPolicy2
        $allowed = $false
        foreach ($rule in $policy.Rules) {
            if (!$rule.Enabled -or $rule.Direction -ne 1 -or $rule.ApplicationName -ne $Binary -or $rule.Protocol -notin 17,256) { continue }
            if ($rule.Action -eq 0) { return $false }
            if ($rule.Action -eq 1 -and ($rule.Profiles -band 6) -eq 6 -and ($rule.LocalPorts -eq '*' -or $rule.LocalPorts -eq [string]$Port)) { $allowed = $true }
        }
        return $allowed
    } catch { return $false }
}

Export-ModuleMember -Function *-Iw4l*
