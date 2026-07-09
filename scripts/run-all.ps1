# Run every .niao example/test and measure execution time.
# Usage (from repo root):
#   .\scripts\run-all.ps1
#   .\scripts\run-all.ps1 -Mode vm

param(
    [ValidateSet("interp", "vm")]
    [string]$Mode = "vm"
)

$ErrorActionPreference = "Continue"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue

$Niao = Join-Path $Root "target\release\niao.exe"
if (-not (Test-Path $Niao)) {
    Write-Host "Building niao..." -ForegroundColor Yellow
    cargo build --release
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

$Skip = @(
    "examples\web_server.niao"   # long-running server - use: niao serve examples/web_server.niao
)

$Files = @(
    Get-ChildItem -Path "examples", "tests" -Filter "*.niao" -File -ErrorAction SilentlyContinue
) | Sort-Object FullName

if ($Files.Count -eq 0) {
    Write-Error "No .niao files found under examples/ or tests/"
    exit 1
}

Write-Host ""
Write-Host "Niao run-all  (mode: $Mode)" -ForegroundColor Cyan
Write-Host ("=" * 60)
Write-Host ""

$Results = @()
$TotalMs = 0.0
$Passed = 0
$Failed = 0
$Skipped = 0

foreach ($File in $Files) {
    $Rel = $File.FullName.Substring($Root.Length + 1)

    if ($Skip -contains $Rel) {
        Write-Host ("{0,-35} SKIP  (server - run separately)" -f $Rel) -ForegroundColor DarkYellow
        $Skipped++
        continue
    }

    $Sw = [System.Diagnostics.Stopwatch]::StartNew()
    & $Niao run $File.FullName --mode $Mode 2>&1 | Out-Null
    $Exit = $LASTEXITCODE
    $Sw.Stop()
    $Ms = $Sw.Elapsed.TotalMilliseconds
    $TotalMs += $Ms

    if ($Exit -eq 0) {
        Write-Host ("{0,-35} OK    {1,8:N1} ms" -f $Rel, $Ms) -ForegroundColor Green
        $Passed++
    } else {
        Write-Host ("{0,-35} FAIL  {1,8:N1} ms" -f $Rel, $Ms) -ForegroundColor Red
        $Failed++
    }

    $Results += [PSCustomObject]@{
        File = $Rel
        Ms   = [math]::Round($Ms, 1)
        Ok   = ($Exit -eq 0)
    }
}

Write-Host ""
Write-Host ("=" * 60)
Write-Host ("Passed: {0}  Failed: {1}  Skipped: {2}  Total: {3:N1} ms" -f $Passed, $Failed, $Skipped, $TotalMs)
Write-Host ""

if ($Failed -gt 0) { exit 1 }
