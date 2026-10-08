Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.Security

function Get-Iw4lUpdatePath([string]$Root, [string]$Relative) {
    if (!$Relative -or $Relative.Contains('\') -or $Relative.Contains(':') -or $Relative.StartsWith('/')) { throw 'Invalid update path.' }
    foreach ($part in $Relative.Split('/')) {
        if (!$part -or $part -in @('.','..') -or $part -match '[\x00-\x1f<>"|?*]' -or $part.EndsWith('.') -or $part.EndsWith(' ') -or $part -match '^(CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])(?:\.|$)') { throw 'Invalid update path.' }
    }
    $base = [IO.Path]::GetFullPath($Root).TrimEnd('\') + '\'
    $path = [IO.Path]::GetFullPath((Join-Path $base $Relative))
    if (!$path.StartsWith($base, [StringComparison]::OrdinalIgnoreCase)) { throw 'Update path leaves the installation.' }
    $cursor = $path
    while ($cursor.Length -ge $base.TrimEnd('\').Length) {
        if ((Test-Path -LiteralPath $cursor) -and ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Update paths cannot contain junctions or symbolic links.' }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
        if (!$cursor) { break }
    }
    return $path
}

function Test-Iw4lManagedUpdatePath([string]$Relative) {
    if ($Relative -eq '.env') { return $false }
    if ($Relative -match '^(matchmaking|iw4l-artifacts|skate-data|\.updates)(/|$)' -or $Relative -match '^iw4l-character(?:\.txt|-profile\.json(?:\.tmp)?)$' -or $Relative -match '\.(pem|key|pfx|xex|ff|iwd|iwi|bik)$') { throw 'The release contains user data or game assets.' }
    return $true
}

function Assert-Iw4lGameStopped([string]$Root) {
    foreach ($process in @(Get-CimInstance Win32_Process -Filter "Name='iw4l.exe' OR Name='iw4l-master.exe'")) {
        if ($process.ExecutablePath -and [IO.Path]::GetDirectoryName($process.ExecutablePath) -eq [IO.Path]::GetFullPath($Root).TrimEnd('\')) { throw 'Close the game and use Stop my relay before installing an update.' }
    }
}

function Read-Iw4lUpdateConfig([string]$Root) {
    $config = Get-Content -Raw -LiteralPath (Join-Path $Root 'UPDATE.json') | ConvertFrom-Json
    if ($config.schema -ne 1 -or $config.repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$' -or $config.version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+-demo\.[0-9]+$' -or $config.asset -ne 'IW4L-Community-Multiplayer.zip') { throw 'This package has unsupported update settings.' }
    return $config
}

function Get-Iw4lUpdateVersion([string]$Tag) {
    if ($Tag -notmatch '^v([0-9]+)\.([0-9]+)\.([0-9]+)-demo\.([0-9]+)$') { throw 'Unsupported release version.' }
    return [Version]::new([int]$Matches[1],[int]$Matches[2],[int]$Matches[3],[int]$Matches[4])
}

function Get-Iw4lGithubToken([string]$Root, [switch]$Prompt) {
    if ($env:IW4L_GITHUB_TOKEN) { return $env:IW4L_GITHUB_TOKEN }
    $path = Get-Iw4lUpdatePath $Root '.updates/github-token.dat'
    if (!$Prompt -and (Test-Path -LiteralPath $path)) {
        try { return [Text.Encoding]::UTF8.GetString([Security.Cryptography.ProtectedData]::Unprotect([IO.File]::ReadAllBytes($path), $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)) }
        catch { throw 'Saved GitHub authentication cannot be read. Use GitHub access to save a new token.' }
    }
    if (!$Prompt) { return $null }
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    $form = New-Object Windows.Forms.Form
    $form.Text = 'GitHub access (optional)'
    $form.ClientSize = New-Object Drawing.Size(570,225)
    $form.StartPosition = 'CenterScreen'
    $form.FormBorderStyle = 'FixedDialog'; $form.MaximizeBox = $false
    $label = New-Object Windows.Forms.Label
    $label.Text = 'Public updates need no token. For private forks or higher API limits, use a GitHub token with Contents: read access to the repository. The token is encrypted for your Windows account.'
    $label.Location = New-Object Drawing.Point(16,16); $label.Size = New-Object Drawing.Size(538,68)
    $form.Controls.Add($label)
    $text = New-Object Windows.Forms.TextBox
    $text.UseSystemPasswordChar = $true; $text.Location = New-Object Drawing.Point(16,96); $text.Size = New-Object Drawing.Size(538,28)
    $form.Controls.Add($text)
    $link = New-Object Windows.Forms.LinkLabel
    $link.Text = 'Create a fine-grained GitHub token'; $link.Location = New-Object Drawing.Point(16,145); $link.Size = New-Object Drawing.Size(300,30)
    $link.Add_LinkClicked({ Start-Process 'https://github.com/settings/personal-access-tokens/new' | Out-Null }); $form.Controls.Add($link)
    $save = New-Object Windows.Forms.Button
    $save.Text = 'Continue'; $save.DialogResult = 'OK'; $save.Location = New-Object Drawing.Point(344,175); $save.Size = New-Object Drawing.Size(100,34)
    $form.Controls.Add($save); $form.AcceptButton = $save
    $cancel = New-Object Windows.Forms.Button
    $cancel.Text = 'Cancel'; $cancel.DialogResult = 'Cancel'; $cancel.Location = New-Object Drawing.Point(454,175); $cancel.Size = New-Object Drawing.Size(100,34)
    $form.Controls.Add($cancel); $form.CancelButton = $cancel
    if ($form.ShowDialog() -ne 'OK') { $form.Dispose(); throw 'GitHub authentication cancelled.' }
    $token = $text.Text.Trim(); $text.Clear(); $form.Dispose()
    if (!$token -or $token -match '\s') { throw 'Enter a valid GitHub access token.' }
    return $token
}

function Save-Iw4lGithubToken([string]$Root, [string]$Token) {
    $path = Get-Iw4lUpdatePath $Root '.updates/github-token.dat'
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    $data = [Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes($Token), $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)
    [IO.File]::WriteAllBytes($path, $data)
}

function Get-Iw4lRelease([string]$Root, [string]$Token) {
    $config = Read-Iw4lUpdateConfig $Root
    $headers = @{ Accept='application/vnd.github+json'; 'X-GitHub-Api-Version'='2022-11-28' }
    if ($Token) { $headers.Authorization = 'Bearer ' + $Token }
    try { $releases = Invoke-RestMethod -Uri ('https://api.github.com/repos/' + $config.repository + '/releases?per_page=100') -Headers $headers -UserAgent 'IW4L-Community-Updater' -TimeoutSec 30 }
    catch { throw 'Cannot access GitHub releases. Check your connection and use GitHub access to renew repository permissions.' }
    $candidate = $null
    foreach ($release in $releases) {
        if ($release.draft -or !$release.published_at -or $release.tag_name -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+-demo\.[0-9]+$') { continue }
        $asset = @($release.assets | Where-Object { $_.name -eq $config.asset -and $_.state -eq 'uploaded' })
        if ($asset.Count -ne 1) { continue }
        $version = Get-Iw4lUpdateVersion $release.tag_name
        if (!$candidate -or $version -gt $candidate.Version) { $candidate = [pscustomobject]@{ Version=$version; Tag=$release.tag_name; Asset=$asset[0]; Repository=$config.repository } }
    }
    return $candidate
}

function Get-Iw4lReleaseArchive($Release, [string]$Token, [string]$Path) {
    if ($Release.Asset.digest -notmatch '^sha256:([0-9a-f]{64})$') { throw 'GitHub has not supplied a SHA-256 digest for this release.' }
    $expected = $Matches[1]
    if ($Release.Asset.size -lt 1 -or $Release.Asset.size -gt 512MB) { throw 'Release archive size is unsupported.' }
    $url = 'https://api.github.com/repos/' + $Release.Repository + '/releases/assets/' + [long]$Release.Asset.id
    for ($redirect = 0; $redirect -lt 5; $redirect++) {
        $uri = [Uri]$url
        if ($uri.Scheme -ne 'https' -or $uri.UserInfo -or $uri.Port -ne 443 -or $uri.Host -notin @('api.github.com','release-assets.githubusercontent.com','objects.githubusercontent.com','github.com')) { throw 'Unexpected download destination.' }
        $request = [Net.HttpWebRequest]::Create($uri)
        $request.AllowAutoRedirect = $false; $request.Timeout = 30000; $request.ReadWriteTimeout = 30000
        $request.UserAgent = 'IW4L-Community-Updater'; $request.Accept = 'application/octet-stream'
        if ($Token -and $uri.Host -eq 'api.github.com') { $request.Headers['Authorization'] = 'Bearer ' + $Token }
        $response = $request.GetResponse()
        try {
            if ([int]$response.StatusCode -in @(301,302,303,307,308)) { $url = [Uri]::new($uri, $response.Headers['Location']).AbsoluteUri; continue }
            if ([int]$response.StatusCode -ne 200) { throw 'Download failed.' }
            $input = $response.GetResponseStream(); $output = [IO.File]::Create($Path)
            try {
                $buffer = New-Object byte[] 65536; $total = 0L
                while (($read = $input.Read($buffer,0,$buffer.Length)) -gt 0) {
                    $total += $read
                    if ($total -gt $Release.Asset.size) { throw 'Download exceeds its declared size.' }
                    $output.Write($buffer,0,$read)
                }
            } finally { $output.Dispose(); $input.Dispose() }
            if ($total -ne $Release.Asset.size -or (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne $expected) { throw 'Downloaded release failed its size or SHA-256 check.' }
            return
        } finally { $response.Dispose() }
    }
    throw 'Too many download redirects.'
}

function Expand-Iw4lUpdateArchive([string]$Archive, [string]$Stage, [string]$Repository, [string]$Version) {
    [IO.Directory]::CreateDirectory($Stage) | Out-Null
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        $seen = New-Object 'Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
        $total = 0L
        foreach ($entry in $zip.Entries) {
            if ($entry.FullName -eq 'IW4L-Community-Multiplayer/') { continue }
            if (!$entry.FullName.StartsWith('IW4L-Community-Multiplayer/', [StringComparison]::Ordinal)) { throw 'Unexpected archive root.' }
            $relative = $entry.FullName.Substring('IW4L-Community-Multiplayer/'.Length)
            if ($entry.Name -eq '') { continue }
            if (!$seen.Add($relative)) { throw 'Duplicate archive path.' }
            if ((($entry.ExternalAttributes -shr 16) -band 61440) -eq 40960) { throw 'Archive contains a symbolic link.' }
            $null = Test-Iw4lManagedUpdatePath $relative
            $target = Get-Iw4lUpdatePath $Stage $relative
            $total += $entry.Length
            if ($total -gt 1GB) { throw 'Expanded archive is too large.' }
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry,$target,$false)
        }
        $build = Get-Content -Raw -LiteralPath (Join-Path $Stage 'BUILD.json') | ConvertFrom-Json
        $config = Read-Iw4lUpdateConfig $Stage
        if ($build.version -ne $Version -or $config.version -ne $Version -or $config.repository -ne $Repository) { throw 'Release identity does not match its package.' }
        $listed = New-Object 'Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
        $null = $listed.Add('BUILD.json')
        foreach ($file in $build.files.PSObject.Properties) {
            if (!$listed.Add($file.Name) -or $file.Value -notmatch '^[0-9a-f]{64}$') { throw 'Invalid build manifest.' }
            $target = Get-Iw4lUpdatePath $Stage $file.Name
            $null = Test-Iw4lManagedUpdatePath $file.Name
            if (!(Test-Path -LiteralPath $target -PathType Leaf) -or (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -ne $file.Value) { throw ('Package hash mismatch: ' + $file.Name) }
        }
        foreach ($file in $seen) { if (!$listed.Contains($file)) { throw 'Archive contains an unlisted file.' } }
        foreach ($required in @('iw4l.exe','iw4l-master.exe','Multiplayer.exe','Community-Multiplayer.ps1','Update-Multiplayer.ps1','Community-Update.psm1','UPDATE.json','LICENSE','NOTICE','GPL-3.0.txt','runtime-source.zip')) {
            if (!$listed.Contains($required)) { throw ('Missing package file: ' + $required) }
        }
        return $build
    } finally { $zip.Dispose() }
}

function Restore-Iw4lUpdate([string]$Root, [string]$Work) {
    Assert-Iw4lGameStopped $Root
    $journal = Join-Path $Work 'transaction.json'
    if (!(Test-Path -LiteralPath $journal)) { return }
    $state = Get-Content -Raw -LiteralPath $journal | ConvertFrom-Json
    if ($state.phase -ne 'applying') { return }
    $files = @($state.files); [Array]::Reverse($files)
    foreach ($file in $files) {
        if (!(Test-Iw4lManagedUpdatePath $file.relative)) { continue }
        $target = Get-Iw4lUpdatePath $Root $file.relative
        if ($file.existed) { [IO.File]::Copy((Get-Iw4lUpdatePath $Work ('backup/' + $file.relative)), $target, $true) }
        elseif (Test-Path -LiteralPath $target) { Remove-Item -LiteralPath $target -Force }
    }
    $state.phase = 'rolled_back'
    [IO.File]::WriteAllText($journal, ($state | ConvertTo-Json -Depth 6))
}

function Install-Iw4lUpdate([string]$Root, [string]$Work) {
    Assert-Iw4lGameStopped $Root
    $stage = Get-Iw4lUpdatePath $Work 'stage'
    $config = Read-Iw4lUpdateConfig $stage
    $installed = Read-Iw4lUpdateConfig $Root
    if ($config.repository -ne $installed.repository) { throw 'Update repository changed.' }
    $build = Get-Content -Raw -LiteralPath (Join-Path $stage 'BUILD.json') | ConvertFrom-Json
    $journal = Get-Iw4lUpdatePath $Work 'transaction.json'
    $state = [pscustomobject]@{ phase='applying'; files=@() }
    [IO.File]::WriteAllText($journal, ($state | ConvertTo-Json -Depth 6))
    $files = @($build.files.PSObject.Properties | Where-Object { $_.Name -ne 'UPDATE.json' }) + @($build.files.PSObject.Properties | Where-Object { $_.Name -eq 'UPDATE.json' })
    $files += [pscustomobject]@{ Name='BUILD.json'; Value=(Get-FileHash -LiteralPath (Join-Path $stage 'BUILD.json') -Algorithm SHA256).Hash }
    try {
        foreach ($file in $files) {
            if (!(Test-Iw4lManagedUpdatePath $file.Name)) { continue }
            $source = Get-Iw4lUpdatePath $stage $file.Name
            if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -ne $file.Value) { throw 'Staged package changed after validation.' }
            $target = Get-Iw4lUpdatePath $Root $file.Name
            $exists = Test-Path -LiteralPath $target -PathType Leaf
            if ($exists -and (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -eq $file.Value) { continue }
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
            if ($exists) {
                $backup = Get-Iw4lUpdatePath $Work ('backup/' + $file.Name)
                [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($backup)) | Out-Null
                [IO.File]::Copy($target,$backup,$false)
            }
            $state.files += [pscustomobject]@{ relative=$file.Name; existed=$exists }
            [IO.File]::WriteAllText($journal, ($state | ConvertTo-Json -Depth 6))
            $temporary = Get-Iw4lUpdatePath $Root ($file.Name + '.iw4l-update-' + [Guid]::NewGuid().ToString('N'))
            try {
                [IO.File]::Copy($source,$temporary,$false)
                if ($exists) { [IO.File]::Replace($temporary,$target,$backup) }
                else { [IO.File]::Move($temporary,$target) }
            } finally { if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force } }
        }
        $state.phase = 'complete'
        [IO.File]::WriteAllText($journal, ($state | ConvertTo-Json -Depth 6))
    } catch {
        $errorText = $_.Exception.Message
        try { Restore-Iw4lUpdate $Root $Work } catch { throw ('Update failed and rollback is incomplete. Run Update-Multiplayer.ps1 -Mode Recover. Details: ' + $errorText) }
        throw ('Update failed; previous files were restored. ' + $errorText)
    }
}
