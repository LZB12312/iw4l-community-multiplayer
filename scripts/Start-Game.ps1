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
Write-Output 'Preparing game files. Skate 3 extraction can take several minutes on first launch; wait for it to finish.'
$setupLogs = Join-Path $directoryPath 'iw4l-artifacts/setup'
[IO.Directory]::CreateDirectory($setupLogs) | Out-Null
$setup = Start-Process -FilePath $binary -ArgumentList @('setup') -WorkingDirectory $directoryPath -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $setupLogs 'startup.log') -RedirectStandardError (Join-Path $setupLogs 'startup-errors.log')
# Windows PowerShell needs the retained handle to read the exit code after setup exits.
$setup.Handle | Out-Null
$lastProgress = ''
try {
    while (!$setup.HasExited) {
        $conversionLog = Join-Path $setupLogs 'conversion.log'
        if (Test-Path -LiteralPath $conversionLog) {
            $progress = Get-Content -LiteralPath $conversionLog -Tail 1 -ErrorAction SilentlyContinue
            if ($progress -and $progress -ne $lastProgress) { Write-Output $progress; $lastProgress = $progress }
        }
        Start-Sleep -Milliseconds 500
        $setup.Refresh()
    }
    $setup.WaitForExit()
    $setup.Refresh()
    if ($setup.ExitCode -ne 0) { throw ('Game setup failed. Details are in ' + $setupLogs + '. Your selected Skate folder is saved; retry after correcting the error.') }
} finally { $setup.Dispose() }
Write-Output 'Game files are ready. Opening the game.'
$arguments = @('menu')
if ($HostGame) {
    if ($Map -notmatch '^[a-zA-Z0-9_]+$') { throw 'Map must be a map name such as mp_rust.' }
    $env:IW4L_CMDS = "wait 1s; set ui_mapname iw4:$Map; set ui_gametype dm; ui_create_lobby; ui_lobby_privacy; $env:IW4L_CMDS"
}
Start-Process -FilePath $binary -ArgumentList $arguments -WorkingDirectory $directoryPath -WindowStyle Hidden | Out-Null
