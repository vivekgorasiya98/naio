@echo off
setlocal
cd /d "%~dp0"
echo == Neko Windows smoke tests ==
call neko.cmd version
if errorlevel 1 exit /b 1
echo.

call neko.cmd run examples\hello.neko
if errorlevel 1 exit /b 1
echo hello.neko OK

call neko.cmd run examples\re_demo.neko
if errorlevel 1 exit /b 1
echo re_demo.neko OK

call neko.cmd run examples\libs_smoke.neko
if errorlevel 1 exit /b 1
echo libs_smoke.neko OK

echo.
echo All smoke tests passed.
