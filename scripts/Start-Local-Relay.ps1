param([string]$Directory = $PSScriptRoot, [ValidateRange(1,65535)][int]$Port = 4433, [switch]$PassThru)
$ErrorActionPreference = 'Stop'
$directoryPath = [IO.Path]::GetFullPath($Directory)
$binary = Join-Path $directoryPath 'iw4l-master.exe'
$cert = Join-Path $directoryPath 'matchmaking\server-cert.pem'
$key = Join-Path $directoryPath 'matchmaking\server-key.pem'
if (!(Test-Path -LiteralPath $binary) -or !(Test-Path -LiteralPath $cert) -or !(Test-Path -LiteralPath $key)) {
    throw 'The relay executable and certificates must be in this folder. Run Setup-Matchmaking.ps1 with -Directory matchmaking if certificates are missing.'
}
$existing = Get-NetUDPEndpoint -LocalPort $Port -ErrorAction SilentlyContinue
if ($existing) {
    $process = Get-Process -Id $existing[0].OwningProcess -ErrorAction SilentlyContinue
    if ($process.Path -eq $binary) {
        Write-Host 'The relay is already running.'
        if ($PassThru) { return $process }
        return
    }
    $owner = if ($process) { $process.ProcessName + ' (PID ' + $process.Id + ') at ' + $process.Path } else { 'PID ' + $existing[0].OwningProcess }
    throw "UDP port $Port is in use by $owner. Stop that copy's relay, or choose another Host UDP port."
}
$log = Join-Path $directoryPath 'matchmaking\relay.log'
[IO.File]::WriteAllText($log, '')
$start = [Diagnostics.ProcessStartInfo]::new()
$start.FileName = 'powershell.exe'
$start.Arguments = '-NoProfile -ExecutionPolicy Bypass -File "' + (Join-Path $PSScriptRoot 'Run-Relay.ps1') + '" -Directory "' + $directoryPath + '" -Port ' + $Port
$start.WorkingDirectory = $directoryPath
$start.UseShellExecute = $true
$start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
$helper = [Diagnostics.Process]::Start($start)
$process = $null
for ($attempt = 0; $attempt -lt 30; $attempt++) {
    if ($helper.HasExited) { throw "The relay exited during startup. See $log" }
    $endpoint = Get-NetUDPEndpoint -LocalPort $Port -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($endpoint) {
        $process = Get-Process -Id $endpoint.OwningProcess -ErrorAction Stop
        if ($process.Path -ne $binary) { throw "UDP $Port was bound by another process during startup." }
        break
    }
    if ($attempt -eq 29) { throw "The relay did not bind UDP $Port. See $log" }
    Start-Sleep -Milliseconds 100
}
$process.Id | Set-Content -LiteralPath (Join-Path $directoryPath 'matchmaking\relay.pid')
Write-Host "Local relay started on UDP $Port (process $($process.Id))."
if ($PassThru) { return $process }
