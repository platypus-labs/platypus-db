@echo off
rem Double-click to build the PlatypusDB installers; see scripts\build-windows.ps1 for options.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\build-windows.ps1" -Open %*
pause
