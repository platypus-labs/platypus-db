@echo off
rem Double-click to build the PlatypusDB installers; see build-windows.ps1 beside it for options.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-windows.ps1" -Open %*
pause
