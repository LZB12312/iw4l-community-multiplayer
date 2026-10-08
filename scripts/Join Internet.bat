@echo off
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Join-Internet.ps1"
if errorlevel 1 pause
