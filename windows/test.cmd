@echo off
setlocal
cd /d "%~dp0"
echo == Niao Windows smoke tests ==
call niao.cmd version
if errorlevel 1 exit /b 1
echo.

call niao.cmd run examples\hello.niao
if errorlevel 1 exit /b 1
echo hello.niao OK

call niao.cmd run examples\re_demo.niao
if errorlevel 1 exit /b 1
echo re_demo.niao OK

call niao.cmd run examples\libs_smoke.niao
if errorlevel 1 exit /b 1
echo libs_smoke.niao OK

echo.
echo All smoke tests passed.
