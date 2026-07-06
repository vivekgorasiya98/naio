# Build and install neko + nm globally with all standard libraries.
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Manifest = "$Root\Cargo.toml"

Write-Host "Building neko and nm (release)..."
& cargo build --release --manifest-path $Manifest -p neko_cli -p neko_nm
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$NekoExe = "$Root\target\release\neko.exe"
$NmExe = "$Root\target\release\nm.exe"
$CargoBin = "$env:USERPROFILE\.cargo\bin"

Copy-Item -Force $NekoExe "$CargoBin\neko.exe"
Copy-Item -Force $NmExe "$CargoBin\nm.exe"
Write-Host "Installed binaries to $CargoBin"

Write-Host "Installing global neko_libs..."
& $NmExe install --global --force --neko-bin $NekoExe --nm-bin $NmExe
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host ""
Write-Host "Done. Try:"
Write-Host "  neko version"
Write-Host "  nm version"
Write-Host "  nm list"
