#Requires -RunAsAdministrator
param([string]$Directory = $PSScriptRoot, [ValidateRange(1,65535)][int]$Port = 4433)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'Internet-Multiplayer.psm1') -Force
$binary = Join-Path ([IO.Path]::GetFullPath($Directory)) 'iw4l-master.exe'
if (!(Test-Path -LiteralPath $binary)) { throw 'iw4l-master.exe is missing.' }
$name = Get-Iw4lFirewallRuleName $binary $Port
$blocked = @(Get-NetFirewallApplicationFilter -Program $binary | Get-NetFirewallRule | Where-Object {
    $_.Enabled -eq 'True' -and $_.Direction -eq 'Inbound' -and $_.Action -eq 'Block' -and $_.Name -like 'UDP Query User*'
})
foreach ($block in $blocked) { $block | Disable-NetFirewallRule | Out-Null }
$rule = Get-NetFirewallRule -Name $name -ErrorAction SilentlyContinue
if ($rule) {
    Set-NetFirewallRule -Name $name -Enabled True -Direction Inbound -Action Allow -Program $binary -Protocol UDP -LocalPort $Port -Profile Private,Public | Out-Null
} else {
    New-NetFirewallRule -Name $name -DisplayName "IW4L community relay UDP $Port" -Direction Inbound -Action Allow -Program $binary -Protocol UDP -LocalPort $Port -Profile Private,Public | Out-Null
}
Write-Output "Windows Firewall allows this relay executable on UDP $Port."
