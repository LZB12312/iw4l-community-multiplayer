param(
    [string]$Directory = $PSScriptRoot,
    [ValidateSet('Stage','Check','Apply','Recover','Authenticate')][string]$Mode = 'Stage',
    [string]$Work,
    [int]$LauncherPid,
    [string]$ArchivePath,
    [switch]$NoRestart
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Import-Module (Join-Path $PSScriptRoot 'Community-Update.psm1') -Force
$root = [IO.Path]::GetFullPath($Directory).TrimEnd('\')
$pending = Get-Iw4lUpdatePath $root '.updates/pending.json'
$key = [Security.Cryptography.SHA256]::Create()
try { $lockName = ([BitConverter]::ToString($key.ComputeHash([Text.Encoding]::UTF8.GetBytes($root.ToLowerInvariant())))).Replace('-','') }
finally { $key.Dispose() }
$mutex = New-Object Threading.Mutex($false, ('Local\IW4LUpdate_' + $lockName))
$owns = $false
try {
    try { $owns = $mutex.WaitOne($(if ($Mode -in @('Apply','Recover')) { 10000 } else { 0 })) } catch [Threading.AbandonedMutexException] { $owns = $true }
    if (!$owns) { throw 'Another update is already running for this folder.' }
    if ($Mode -in @('Apply','Recover')) {
        if (!$Work -and (Test-Path -LiteralPath $pending)) { $Work = (Get-Content -Raw -LiteralPath $pending | ConvertFrom-Json).work }
        if ($Work -notmatch '^\.updates/[a-f0-9]{32}$') { throw 'Invalid update work folder.' }
        $workPath = Get-Iw4lUpdatePath $root $Work
        $deadline = [DateTime]::UtcNow.AddSeconds(45)
        do {
            $launchers = @(Get-CimInstance Win32_Process -Filter "Name='Multiplayer.exe'" | Where-Object { $_.ExecutablePath -eq (Join-Path $root 'Multiplayer.exe') })
            $ui = if ($LauncherPid) { Get-Process -Id $LauncherPid -ErrorAction SilentlyContinue } else { $null }
            if (!$launchers.Count -and !$ui) { break }
            if ([DateTime]::UtcNow -ge $deadline) { throw 'Close all Multiplayer launcher windows, then retry the update.' }
            Start-Sleep -Milliseconds 200
        } while ($true)
        if ($Mode -eq 'Recover') { Restore-Iw4lUpdate $root $workPath }
        else { Install-Iw4lUpdate $root $workPath }
        if (Test-Path -LiteralPath $pending) { Remove-Item -LiteralPath $pending -Force }
        Write-Output 'Update installed successfully.'
        if (!$NoRestart) { Start-Process -FilePath (Join-Path $root 'Multiplayer.exe') -WorkingDirectory $root -WindowStyle Hidden | Out-Null }
        return
    }
    if (Test-Path -LiteralPath $pending) { throw 'An update is pending or was interrupted. Close the launcher and run Update-Multiplayer.ps1 -Mode Recover.' }
    $config = Read-Iw4lUpdateConfig $root
    $token = Get-Iw4lGithubToken $root -Prompt:($Mode -eq 'Authenticate')
    if (!$ArchivePath) {
        Write-Output 'Checking GitHub releases...'
        $release = Get-Iw4lRelease $root $token
        if ($Mode -eq 'Authenticate') { Save-Iw4lGithubToken $root $token; Write-Output 'GitHub access saved for this Windows account.'; return }
        if (!$release) { Write-Output 'No compatible release has been published yet.'; return }
        if ($release.Version -le (Get-Iw4lUpdateVersion $config.version)) { Write-Output ('Already up to date: ' + $config.version); return }
        if ($Mode -eq 'Check') { Write-Output ('Update available: ' + $release.Tag); return }
        $version = $release.Tag
    } else {
        $zip = [IO.Compression.ZipFile]::OpenRead([IO.Path]::GetFullPath($ArchivePath))
        try {
            $entry = $zip.GetEntry('IW4L-Community-Multiplayer/UPDATE.json')
            if (!$entry -or $entry.Length -gt 16384) { throw 'Invalid local update archive.' }
            $reader = New-Object IO.StreamReader($entry.Open())
            try { $version = ($reader.ReadToEnd() | ConvertFrom-Json).version } finally { $reader.Dispose() }
        } finally { $zip.Dispose() }
        if ((Get-Iw4lUpdateVersion $version) -le (Get-Iw4lUpdateVersion $config.version)) { throw 'Local archive is not newer than this installation.' }
    }
    Assert-Iw4lGameStopped $root
    $Work = '.updates/' + [Guid]::NewGuid().ToString('N')
    $workPath = Get-Iw4lUpdatePath $root $Work
    [IO.Directory]::CreateDirectory($workPath) | Out-Null
    $archive = Get-Iw4lUpdatePath $workPath 'release.zip'
    if ($ArchivePath) { Copy-Item -LiteralPath ([IO.Path]::GetFullPath($ArchivePath)) -Destination $archive }
    else { Write-Output ('Downloading ' + $version + '...'); Get-Iw4lReleaseArchive $release $token $archive }
    Write-Output 'Verifying package contents...'
    $stage = Get-Iw4lUpdatePath $workPath 'stage'
    $null = Expand-Iw4lUpdateArchive $archive $stage $config.repository $version
    Copy-Item -LiteralPath $PSCommandPath -Destination (Join-Path $workPath 'Update-Multiplayer.ps1')
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Community-Update.psm1') -Destination (Join-Path $workPath 'Community-Update.psm1')
    [IO.File]::WriteAllText($pending, (@{work=$Work} | ConvertTo-Json))
    if (!$NoRestart) {
        $start = New-Object Diagnostics.ProcessStartInfo
        $start.FileName = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
        $start.Arguments = '-NoProfile -STA -ExecutionPolicy Bypass -File "' + (Join-Path $workPath 'Update-Multiplayer.ps1') + '" -Mode Apply -Directory "' + $root + '" -Work "' + $Work + '" -LauncherPid ' + $LauncherPid
        $start.UseShellExecute = $true; $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
        $null = [Diagnostics.Process]::Start($start)
    }
    Write-Output ('UPDATE_READY:' + $version)
} catch {
    if ($Mode -in @('Apply','Recover')) {
        $message = 'Update could not finish: ' + $_.Exception.Message
        [IO.File]::WriteAllText((Get-Iw4lUpdatePath $root '.updates/last-error.log'),$message)
        if (!$NoRestart) { Add-Type -AssemblyName System.Windows.Forms; [Windows.Forms.MessageBox]::Show($message,'IW4L Update','OK','Warning') | Out-Null }
    }
    throw
} finally {
    if ($owns) { $mutex.ReleaseMutex() }
    $mutex.Dispose()
}
