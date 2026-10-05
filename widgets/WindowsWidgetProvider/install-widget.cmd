@echo off
REM Double-click launcher for the BiFlow Windows 11 widget.
REM The PowerShell script elevates itself; Windows will ask once, press Yes.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-widget.ps1"
if errorlevel 1 pause
