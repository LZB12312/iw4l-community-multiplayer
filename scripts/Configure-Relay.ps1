param([string]$Address, [string]$Directory = $PSScriptRoot, [string]$ServerName = 'iw4l-community')
$ErrorActionPreference = 'Stop'
if (!$Address) { $Address = Read-Host 'Relay hostname or IP and port (for example 192.168.1.10:4433)' }
$Address = $Address.Trim()
if ($Address -notmatch '^(\[[0-9a-fA-F:]+\]|[a-zA-Z0-9][a-zA-Z0-9.-]*):([0-9]{1,5})$' -or [int]$Matches[2] -notin 1..65535) {
    throw 'Enter a hostname or IP followed by a port from 1 to 65535.'
}
if ($ServerName -notmatch '^[a-zA-Z0-9][a-zA-Z0-9.-]*$') { throw 'Invalid certificate server name.' }
$directoryPath = [IO.Path]::GetFullPath($Directory)
$ca = Join-Path $directoryPath 'matchmaking\iw4l-ca.pem'
if (!(Test-Path -LiteralPath $ca)) { throw 'Place the relay owner''s iw4l-ca.pem in matchmaking first.' }
$config = Join-Path $directoryPath '.env'
$lines = if (Test-Path -LiteralPath $config) { @(Get-Content -LiteralPath $config) } else { @() }
$lines = @($lines | Where-Object { $_ -notmatch '^\s*IW4L_MASTER_(ADDR|SERVER_NAME|CA_CERT)\s*=' })
$lines += @("IW4L_MASTER_ADDR=$Address", "IW4L_MASTER_SERVER_NAME=$ServerName", 'IW4L_MASTER_CA_CERT=matchmaking/iw4l-ca.pem')
[IO.File]::WriteAllText($config, ($lines -join "`n") + "`n")
Write-Output "Relay set to $Address. Launch the game again to connect."
