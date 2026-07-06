# Build complete Windows distribution + NekoSetup.exe installer.
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$WinDir = $PSScriptRoot

Write-Host "== Neko Windows full build =="
Write-Host ""

Set-Location $Root
Write-Host "[1/3] Building neko.exe and nm.exe (release)..."
cargo build --release --bin neko --bin nm
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host ""
Write-Host "[2/3] Staging payload (binaries + 15 libraries)..."
powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $WinDir "prepare-bundle.ps1")
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host ""
Write-Host "[3/3] Building NekoSetup.exe installer..."
Set-Location (Join-Path $WinDir "installer")
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$setupSrc = Join-Path $WinDir "installer\target\release\NekoSetup.exe"
$setupDest = Join-Path $WinDir "NekoSetup.exe"
Copy-Item $setupSrc $setupDest -Force

Set-Location $WinDir
Write-Host ""
Write-Host "== Build complete =="
Write-Host ""
Write-Host "  Installer:   $setupDest"
Write-Host "  Portable:    $(Join-Path $WinDir 'neko.cmd')"
Write-Host "  neko_home:   $(Join-Path $WinDir 'neko_home')"
Write-Host ""
Write-Host "Install (like Python):"
Write-Host "  Double-click NekoSetup.exe"
Write-Host ""
Write-Host "Or run portable (no install):"
Write-Host "  neko.cmd run examples\hello.neko"
