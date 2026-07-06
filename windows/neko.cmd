@echo off
setlocal
set "NEKO_HOME=%~dp0neko_home"
set "NEKO_BIN=%NEKO_HOME%\bin\neko.exe"
if not exist "%NEKO_BIN%" (
    echo Neko not built yet. From repo root run:
    echo   powershell -File windows\build.ps1
    exit /b 1
)
"%NEKO_BIN%" %*
