@echo off
setlocal
set "NIAO_HOME=%~dp0niao_home"
set "NIAO_BIN=%NIAO_HOME%\bin\niao.exe"
if not exist "%NIAO_BIN%" (
    echo Niao not built yet. From repo root run:
    echo   powershell -File windows\build.ps1
    exit /b 1
)
"%NIAO_BIN%" %*
