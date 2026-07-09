# Build and install niao + nm globally with all standard libraries.
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Manifest = "$Root\Cargo.toml"
$CargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$NiaoHome = Join-Path $env:USERPROFILE ".niao"
$NiaoBinDir = Join-Path $NiaoHome "bin"
$LegacyNekoHome = Join-Path $env:USERPROFILE ".neko"
$LegacyNekoBin = Join-Path $LegacyNekoHome "bin"

function Update-UserPath {
    param([string]$BinDir)

    $current = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($null -eq $current) { $current = "" }

    $segments = @()
    foreach ($part in ($current -split ';')) {
        $trimmed = $part.Trim()
        if (-not $trimmed) { continue }
        if ($trimmed -ieq $LegacyNekoBin) { continue }
        if ($segments -notcontains $trimmed) { $segments += $trimmed }
    }

    if ($segments -notcontains $BinDir) {
        $segments = @($BinDir) + $segments
    }

    $newPath = ($segments -join ';')
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")

    # Refresh current session PATH
    $env:Path = ($segments + ($env:Path -split ';' | Where-Object { $_ })) -join ';'
}

function Remove-LegacyNekoShims {
    $legacy = @(
        (Join-Path $CargoBin "neko.exe"),
        (Join-Path $LegacyNekoBin "neko.exe")
    )
    foreach ($path in $legacy) {
        if (Test-Path $path) {
            Remove-Item -Force $path
            Write-Host "Removed legacy shim: $path"
        }
    }
}

Write-Host "Building niao and nm (release, no LLM backends)..."
& cargo build --release --no-default-features --manifest-path $Manifest -p niao_cli -p niao_nm
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$NiaoExe = "$Root\target\release\niao.exe"
$NmExe = "$Root\target\release\nm.exe"

New-Item -ItemType Directory -Force -Path $CargoBin | Out-Null
New-Item -ItemType Directory -Force -Path $NiaoBinDir | Out-Null

Copy-Item -Force $NiaoExe (Join-Path $CargoBin "niao.exe")
Copy-Item -Force $NmExe (Join-Path $CargoBin "nm.exe")
Copy-Item -Force $NiaoExe (Join-Path $NiaoBinDir "niao.exe")
Copy-Item -Force $NmExe (Join-Path $NiaoBinDir "nm.exe")
Write-Host "Installed binaries to $CargoBin and $NiaoBinDir"

Remove-LegacyNekoShims

Write-Host "Installing global niao_libs..."
& $NmExe install --global --force --niao-bin $NiaoExe --nm-bin $NmExe
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Update-UserPath -BinDir $NiaoBinDir
Write-Host "Updated user PATH (added $NiaoBinDir, removed legacy $LegacyNekoBin if present)"

Write-Host ""
Write-Host "Done. Open a new terminal, then try:"
Write-Host "  niao version"
Write-Host "  nm version"
Write-Host "  nm list"
