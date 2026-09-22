@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0remove.ps1" %*
if errorlevel 1 pause
exit /b %ERRORLEVEL%
