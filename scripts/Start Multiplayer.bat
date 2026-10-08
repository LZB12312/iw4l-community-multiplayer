@echo off
powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -File "%~dp0Community-Multiplayer.ps1"
if errorlevel 1 pause
