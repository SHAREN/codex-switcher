@echo off
setlocal

set "ROOT=%~dp0.."
cd /d "%ROOT%"

set "POWERSHELL=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if not exist "%POWERSHELL%" (
  echo powershell.exe not found
  exit /b 1
)

call "%POWERSHELL%" -NoProfile -ExecutionPolicy Bypass -File "%ROOT%\scripts\start-switcher.ps1"
