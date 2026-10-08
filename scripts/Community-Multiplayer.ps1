param([string]$Directory = $PSScriptRoot, [switch]$CheckOnly, [string]$PreviewPath)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
[Windows.Forms.Application]::EnableVisualStyles()
$script:root = [IO.Path]::GetFullPath($Directory)
$script:job = $null
$script:buttons = @()
$form = New-Object Windows.Forms.Form
$form.Text = 'IW4L Multiplayer'
$form.ClientSize = New-Object Drawing.Size(660, 672)
$form.StartPosition = 'CenterScreen'
$form.FormBorderStyle = 'FixedDialog'
$form.MaximizeBox = $false
$form.Font = New-Object Drawing.Font('Segoe UI', 10)
$form.BackColor = [Drawing.Color]::FromArgb(245,247,250)

function Add-Label([string]$Text, [int]$X, [int]$Y, [int]$Width, [int]$Height) {
    $label = New-Object Windows.Forms.Label
    $label.Text = $Text
    $label.Location = New-Object Drawing.Point($X,$Y)
    $label.Size = New-Object Drawing.Size($Width,$Height)
    $form.Controls.Add($label)
    return $label
}
function Add-Button([string]$Text, [int]$X, [int]$Y, [int]$Width, [scriptblock]$Click) {
    $button = New-Object Windows.Forms.Button
    $button.Text = $Text
    $button.Location = New-Object Drawing.Point($X,$Y)
    $button.Size = New-Object Drawing.Size($Width,40)
    $button.Add_Click($Click)
    $form.Controls.Add($button)
    $script:buttons += $button
    return $button
}
$title = Add-Label 'Play together. Host or join from this folder.' 24 20 612 36
$title.Font = New-Object Drawing.Font('Segoe UI', 15, [Drawing.FontStyle]::Bold)
$null = Add-Label 'On first launch, select your MW2 folder and optionally your extracted Skate 3 default.xex. Setup and clothing conversion are included.' 24 65 612 52
$null = Add-Label 'Host a lobby' 24 123 612 25
$null = Add-Button 'Host Internet' 24 153 190 { Start-CommunityJob 'Host' }
$null = Add-Label 'Host UDP port' 230 158 128 25
$port = New-Object Windows.Forms.NumericUpDown
$port.Minimum = 1; $port.Maximum = 65535; $port.Value = 4433
try {
    $savedHost = Join-Path $script:root 'matchmaking/host-settings.json'
    if (Test-Path -LiteralPath $savedHost) {
        $saved = [IO.File]::ReadAllText($savedHost) | ConvertFrom-Json
        if ([int]$saved.Port -in 1..65535) { $port.Value = [int]$saved.Port }
    }
} catch { }
$port.Location = New-Object Drawing.Point(360,157)
$port.Size = New-Object Drawing.Size(95,28)
$form.Controls.Add($port)
$copy = Add-Button 'Copy invitation' 470 153 166 {
    $path = Join-Path $script:root 'matchmaking/Internet Invite.txt'
    if (Test-Path -LiteralPath $path) {
        [Windows.Forms.Clipboard]::SetText([IO.File]::ReadAllText($path).Trim())
        $status.Text = 'Invitation copied. Share it with friends using this same package.'
    } else { $status.Text = 'Host Internet first to create an invitation.' }
}
$null = Add-Label 'Manual public address (optional; leave blank to use UPnP)' 24 206 612 25
$address = New-Object Windows.Forms.TextBox
$address.Location = New-Object Drawing.Point(24,232)
$address.Size = New-Object Drawing.Size(612,28)
$address.MaxLength = 260
$form.Controls.Add($address)
$null = Add-Label 'Join a friend - paste their invitation code' 24 276 612 25
$invite = New-Object Windows.Forms.TextBox
$invite.Location = New-Object Drawing.Point(24,304)
$invite.Size = New-Object Drawing.Size(612,68)
$invite.Multiline = $true; $invite.ScrollBars = 'Vertical'; $invite.MaxLength = 16000
$form.Controls.Add($invite)
$null = Add-Button 'Join Internet' 24 383 190 { Start-CommunityJob 'Join' }
$null = Add-Button 'Play again' 230 383 190 { Start-CommunityJob 'Play' }
$null = Add-Button 'Stop my relay' 436 383 200 { Start-CommunityJob 'Stop' }
$status = Add-Label 'Ready. Host creates an invitation; Join connects to a friend. Join before the host starts the match.' 24 439 612 60
$status.ForeColor = [Drawing.Color]::FromArgb(25,54,95)
$null = Add-Button 'Open guide' 24 510 190 {
    Start-Process -FilePath (Join-Path $script:root 'README.md') | Out-Null
}
$null = Add-Label 'Hosting may ask for Windows firewall approval.' 230 519 406 25
$null = Add-Button 'Audio output' 24 566 190 {
    try { $result = & (Join-Path $PSScriptRoot 'Configure-Audio.ps1') -Directory $script:root; if ($result) { $status.Text = $result } }
    catch { $status.Text = $_.Exception.Message }
}
$null = Add-Label 'Choose headphones directly if game audio crackles.' 230 575 406 25
$null = Add-Button 'Update mod' 24 622 190 { Start-CommunityJob 'Update' }
$null = Add-Label 'Download and install the latest GitHub release.' 230 631 406 25

function Start-CommunityJob([string]$Action) {
    if ($script:job) { return }
    try {
        foreach ($name in @('iw4l.exe','iw4l-master.exe')) {
            if (!(Test-Path -LiteralPath (Join-Path $script:root $name) -PathType Leaf)) { throw 'Extract the whole ZIP before opening Multiplayer.' }
        }
        $work = Join-Path $script:root 'iw4l-artifacts/launcher'
        [IO.Directory]::CreateDirectory($work) | Out-Null
        $id = [Guid]::NewGuid().ToString('N')
        $log = Join-Path $work ($id + '.log')
        [IO.File]::WriteAllText($log, '')
        $codeFile = $null
        $extra = ''
        if ($Action -eq 'Update') { $extra = ' -LauncherPid ' + $PID }
        if ($Action -eq 'Join') {
            $code = $invite.Text.Trim()
            Import-Module (Join-Path $PSScriptRoot 'Internet-Multiplayer.psm1') -Force
            $null = Read-Iw4lInvitation $code
            $codeFile = Join-Path $work ($id + '.invite')
            [IO.File]::WriteAllText($codeFile, $code)
            $extra = ' -InviteFile "' + $codeFile + '"'
        }
        if ($Action -eq 'Host') {
            $null = $form.Validate()
            $extra = ' -Port ' + [int]$port.Value + ' -ExternalPort ' + [int]$port.Value
            $manual = $address.Text.Trim()
            if ($manual) {
                Import-Module (Join-Path $PSScriptRoot 'Internet-Multiplayer.psm1') -Force
                $null = Get-Iw4lEndpoint $manual
                $extra += ' -Address "' + $manual + '"'
            }
        }
        $start = New-Object Diagnostics.ProcessStartInfo
        $start.FileName = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
        $start.Arguments = '-NoProfile -STA -ExecutionPolicy Bypass -File "' + (Join-Path $PSScriptRoot 'Community-Action.ps1') + '" -Directory "' + $script:root + '" -Action ' + $Action + ' -LogFile "' + $log + '"' + $extra
        $start.WorkingDirectory = $script:root
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $process = [Diagnostics.Process]::Start($start)
        $script:job = [pscustomobject]@{ Process=$process; Log=$log; Action=$Action; CodeFile=$codeFile }
        foreach ($button in $script:buttons) { $button.Enabled = $false }
        $port.Enabled = $false; $address.Enabled = $false; $invite.Enabled = $false
        $status.Text = 'Working... Complete any game setup or Windows permission prompts that appear.'
        $timer.Start()
    } catch { $status.Text = $_.Exception.Message }
}
$timer = New-Object Windows.Forms.Timer
$timer.Interval = 500
$timer.Add_Tick({
    if (!$script:job) { return }
    $job = $script:job
    if ($job.Process.HasExited) {
        $exitCode = $job.Process.ExitCode
        $timer.Stop()
        $lines = @([IO.File]::ReadAllLines($job.Log))
        $problem = @($lines | Where-Object { $_ -match '^ERROR:|Windows Firewall may still block' })
        if ($job.Process.ExitCode -ne 0) {
            $status.Text = if ($problem.Count) { $problem[-1] } else { 'The action failed. See the launcher log: ' + $job.Log }
            [Windows.Forms.MessageBox]::Show($status.Text, 'IW4L Multiplayer', 'OK', 'Warning') | Out-Null
        } elseif ($problem.Count) {
            $status.Text = 'Hosting started, but firewall approval is still needed for Internet players. Run Host Internet again and approve the Windows prompt.'
        } else {
            $status.Text = switch ($job.Action) {
                'Host' { 'Game opened and invitation copied. Finish any setup, then wait for friends before Start Match.' }
                'Join' { 'Relay connected. Finish any game setup, then select your friend''s room in Find Lobbies.' }
                'Play' { 'Game opened using your saved relay.' }
                'Stop' { 'Your relay has stopped and its own router mapping was removed.' }
                'Update' { $lines[-1] }
            }
        }
        if ($job.CodeFile) { [IO.File]::Delete($job.CodeFile) }
        $job.Process.Dispose()
        $script:job = $null
        foreach ($button in $script:buttons) { $button.Enabled = $true }
        $port.Enabled = $true; $address.Enabled = $true; $invite.Enabled = $true
        if ($job.Action -eq 'Update' -and $exitCode -eq 0 -and @($lines | Where-Object { $_ -match '^UPDATE_READY:' }).Count) { $form.Close() }
    } else {
        $lines = @([IO.File]::ReadAllLines($job.Log))
        if ($lines.Count) { $status.Text = $lines[-1] }
    }
})
$form.Add_FormClosing({
    if ($script:job) {
        $_.Cancel = $true
        $status.Text = 'Wait for the current action and finish its prompts before closing.'
    }
})
if ($CheckOnly -or $PreviewPath) {
    if ($PreviewPath) {
        $form.ShowInTaskbar = $false
        $form.StartPosition = 'Manual'
        $form.Location = New-Object Drawing.Point(-30000,-30000)
        $form.Show()
        [Windows.Forms.Application]::DoEvents()
        $image = New-Object Drawing.Bitmap($form.Width,$form.Height)
        $form.DrawToBitmap($image, [Drawing.Rectangle]::new(0,0,$form.Width,$form.Height))
        $image.Save([IO.Path]::GetFullPath($PreviewPath), [Drawing.Imaging.ImageFormat]::Png)
        $image.Dispose()
        $form.Hide()
    }
    Write-Output ('Launcher ready: ' + (($script:buttons | ForEach-Object Text) -join ', '))
} else { $null = $form.ShowDialog() }
$timer.Dispose()
$form.Dispose()
