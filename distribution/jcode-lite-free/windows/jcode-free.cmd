@echo off
if /I "%CD%\"=="%~dp0" cd /d "%USERPROFILE%"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0jcode-free.ps1" %*
exit /b %ERRORLEVEL%
