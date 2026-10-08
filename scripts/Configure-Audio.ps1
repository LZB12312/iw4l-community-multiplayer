param([string]$Directory = $PSScriptRoot, [string]$DeviceName, [switch]$CheckOnly)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath($Directory)
$start = New-Object Diagnostics.ProcessStartInfo
$start.FileName = Join-Path $root 'iw4l.exe'
$start.Arguments = 'audio-devices'
$start.WorkingDirectory = $root
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
$child = [Diagnostics.Process]::Start($start)
$output = $child.StandardOutput.ReadToEnd()
$errorText = $child.StandardError.ReadToEnd()
$child.WaitForExit()
if ($child.ExitCode -ne 0) { throw ('Cannot list audio outputs: ' + $errorText.Trim()) }
$child.Dispose()
$devices = @($output -split '\r?\n' | Where-Object { $_.Trim() })
if ($CheckOnly) { $devices; return }
$config = Join-Path $root '.env'
$lines = if (Test-Path -LiteralPath $config) { @([IO.File]::ReadAllLines($config)) } else { @() }
$selected = $DeviceName
if (!$PSBoundParameters.ContainsKey('DeviceName')) {
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    $dialog = New-Object Windows.Forms.Form
    $dialog.Text = 'Game audio output'
    $dialog.ClientSize = New-Object Drawing.Size(550,190)
    $dialog.StartPosition = 'CenterParent'
    $dialog.FormBorderStyle = 'FixedDialog'
    $dialog.MaximizeBox = $false
    $dialog.MinimizeBox = $false
    $dialog.Font = New-Object Drawing.Font('Segoe UI',10)
    $label = New-Object Windows.Forms.Label
    $label.Text = 'Choose your headphones directly to bypass a virtual surround device. Restart the game after changing this setting.'
    $label.Location = New-Object Drawing.Point(16,16)
    $label.Size = New-Object Drawing.Size(518,54)
    $dialog.Controls.Add($label)
    $choice = New-Object Windows.Forms.ComboBox
    $choice.DropDownStyle = 'DropDownList'
    $choice.Location = New-Object Drawing.Point(16,80)
    $choice.Size = New-Object Drawing.Size(518,32)
    $null = $choice.Items.Add('Windows default output')
    foreach ($device in $devices) { $null = $choice.Items.Add($device) }
    $choice.SelectedIndex = 0
    $saved = ($lines | Where-Object { $_ -match '^\s*IW4L_AUDIO_DEVICE\s*=' } | Select-Object -Last 1) -replace '^\s*IW4L_AUDIO_DEVICE\s*=\s*', ''
    $saved = $saved.Trim('"', "'")
    if ($devices -contains $saved) { $choice.SelectedItem = $saved }
    $dialog.Controls.Add($choice)
    $save = New-Object Windows.Forms.Button
    $save.Text = 'Save'
    $save.Location = New-Object Drawing.Point(344,135)
    $save.Size = New-Object Drawing.Size(90,36)
    $save.DialogResult = 'OK'
    $dialog.Controls.Add($save)
    $cancel = New-Object Windows.Forms.Button
    $cancel.Text = 'Cancel'
    $cancel.Location = New-Object Drawing.Point(444,135)
    $cancel.Size = New-Object Drawing.Size(90,36)
    $cancel.DialogResult = 'Cancel'
    $dialog.Controls.Add($cancel)
    $dialog.AcceptButton = $save
    $dialog.CancelButton = $cancel
    if ($dialog.ShowDialog() -ne 'OK') { $dialog.Dispose(); return }
    $selected = if ($choice.SelectedIndex -eq 0) { '' } else { [string]$choice.SelectedItem }
    $dialog.Dispose()
}
if ($selected -and $devices -notcontains $selected) { throw 'The selected output is unavailable. Connect your headphones and try again.' }
$lines = @($lines | Where-Object { $_ -notmatch '^\s*IW4L_AUDIO_DEVICE\s*=' })
if ($selected) {
    $escaped = $selected.Replace('\','\\').Replace('"','\"').Replace('$','\$')
    $lines += 'IW4L_AUDIO_DEVICE="' + $escaped + '"'
}
[IO.File]::WriteAllText($config, ($lines -join "`n") + "`n")
Write-Output 'Audio output saved. Restart the game to apply it.'
