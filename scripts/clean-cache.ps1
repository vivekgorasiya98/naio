# Prune Niao caches: bytecode, old lib versions, and optional Cargo debug artifacts.
param(
    [int]$KeepBytecode = 16,
    [switch]$AllBytecode,
    [switch]$CargoDebug = $true,
    [switch]$OldLibVersions = $true
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

function Format-Mb($bytes) { [math]::Round($bytes / 1MB, 2) }

Write-Host "Niao cache cleanup (root: $Root)"

# 1. Bytecode cache via niao clean (rebuild if needed)
$Niao = Join-Path $Root "target\release\niao.exe"
if (-not (Test-Path $Niao)) {
    Write-Host "Building niao (release) for clean command..."
    & cargo build --release -p niao_cli
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
if ($AllBytecode) {
    & $Niao clean --all
} else {
    & $Niao clean --keep $KeepBytecode
}

# 2. Remove stale niao_libs version dirs (keep package.json version only)
if ($OldLibVersions) {
    $removed = 0
    $freed = 0
    $libsRoot = Join-Path $Root "niao_libs"
    Get-ChildItem $libsRoot -Directory | ForEach-Object {
        $pkg = $_.Name
        $pkgDir = $_.FullName
        $pkgJson = Join-Path $pkgDir "package.json"
        if (-not (Test-Path $pkgJson)) { return }
        $latest = (Get-Content $pkgJson -Raw | ConvertFrom-Json).version
        Get-ChildItem $pkgDir -Directory | Where-Object { $_.Name -ne $latest } | ForEach-Object {
            $size = (Get-ChildItem $_.FullName -Recurse -File -EA SilentlyContinue | Measure-Object Length -Sum).Sum
            Remove-Item $_.FullName -Recurse -Force
            $removed++
            $freed += $size
            Write-Host "  removed niao_libs/$pkg/$($_.Name)"
        }
    }
    Write-Host "niao_libs: removed $removed old version dir(s), freed $(Format-Mb $freed) MB"
}

# 3. Optional: drop Cargo debug build tree (largest disk use)
if ($CargoDebug) {
    $debugDir = Join-Path $Root "target\debug"
    if (Test-Path $debugDir) {
        $size = (Get-ChildItem $debugDir -Recurse -File -EA SilentlyContinue | Measure-Object Length -Sum).Sum
        Remove-Item $debugDir -Recurse -Force
        Write-Host "removed target/debug ($(Format-Mb $size) MB)"
    }
    foreach ($extra in @("target_bench", "target_bench2", "target-rel")) {
        $p = Join-Path $Root $extra
        if (Test-Path $p) {
            $size = (Get-ChildItem $p -Recurse -File -EA SilentlyContinue | Measure-Object Length -Sum).Sum
            Remove-Item $p -Recurse -Force
            Write-Host "removed $extra ($(Format-Mb $size) MB)"
        }
    }
}

Write-Host "Done."
