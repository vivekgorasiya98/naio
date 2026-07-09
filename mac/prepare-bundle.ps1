# Build a self-contained mac/ folder (run from repo on Windows before copying to MacBook).
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$MacDir = $PSScriptRoot
$MacHome = Join-Path $MacDir "niao_home"
$LibsDest = Join-Path $MacHome "niao_libs"
$SrcLibs = Join-Path $Root "niao_libs"
$Engine = Join-Path $MacDir "engine"

$Version = "0.2.2"
$Ts = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds().ToString()

function Copy-LibManifest {
    param([string]$Name, [string]$Ver)
    $srcLib = Join-Path $SrcLibs $Name
    $destLib = Join-Path $LibsDest $Name
    $destVer = Join-Path $destLib $Ver
    New-Item -ItemType Directory -Force -Path $destVer | Out-Null

    $pkgSrc = Join-Path $srcLib "package.json"
    if (Test-Path $pkgSrc) {
        Copy-Item $pkgSrc $destLib -Force
        $pkg = Get-Content $pkgSrc -Raw | ConvertFrom-Json
        $Ver = $pkg.version
        $destVer = Join-Path $destLib $Ver
        New-Item -ItemType Directory -Force -Path $destVer | Out-Null
    }

    $libJsonSrc = Join-Path $srcLib "$Ver\lib.json"
    if (Test-Path $libJsonSrc) {
        Copy-Item $libJsonSrc (Join-Path $destVer "lib.json") -Force
    } else {
        $lib = @{
            name = $Name
            version = $Ver
            kind = "native"
            description = ""
            import_paths = @()
            builtin_count = 0
        }
        if (Test-Path $pkgSrc) {
            $pkg = Get-Content $pkgSrc -Raw | ConvertFrom-Json
            $lib.description = $pkg.description
            $lib.import_paths = @($pkg.import_paths)
            $lib.builtin_count = $pkg.builtin_count
        }
        $lib | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $destVer "lib.json") -Encoding UTF8
    }
}

function Write-AhiruLib {
    $srcPkg = Join-Path $SrcLibs "ahiru\package.json"
    if (Test-Path $srcPkg) {
        $ver = (Get-Content $srcPkg -Raw | ConvertFrom-Json).version
        Copy-LibManifest -Name "ahiru" -Ver $ver
        return
    }
    $name = "ahiru"
    $ver = "0.3.0"
    $destLib = Join-Path $LibsDest $name
    $destVer = Join-Path $destLib $ver
    New-Item -ItemType Directory -Force -Path $destVer | Out-Null
    $spec = @{
        name = $name
        version = $ver
        kind = "native"
        description = "ahiru-server 0.3.0: state, custom middleware, groups, cache, jobs, metrics, CLI toolkit"
        import_paths = @("ahiru", "std/ahiru")
        builtin_count = 36
    }
    $spec | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $destVer "lib.json") -Encoding UTF8
    $spec | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $destLib "package.json") -Encoding UTF8
}

function Copy-EngineSource {
    Write-Host "Copying compiler source into mac/engine/ ..."
    if (Test-Path $Engine) {
        Remove-Item $Engine -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $Engine | Out-Null

    Copy-Item (Join-Path $Root "Cargo.toml") $Engine -Force
    if (Test-Path (Join-Path $Root "Cargo.lock")) {
        Copy-Item (Join-Path $Root "Cargo.lock") $Engine -Force
    }

    $cratesSrc = Join-Path $Root "crates"
    $cratesDest = Join-Path $Engine "crates"
    robocopy $cratesSrc $cratesDest /E /XD target .git /NFL /NDL /NJH /NJS /nc /ns /np | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy crates failed: $LASTEXITCODE" }

    $libsDest = Join-Path $Engine "niao_libs"
    robocopy $SrcLibs $libsDest /E /NFL /NDL /NJH /NJS /nc /ns /np | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy niao_libs failed: $LASTEXITCODE" }
}

Write-Host "== Preparing self-contained mac/ bundle =="

if (Test-Path $LibsDest) { Remove-Item $LibsDest -Recurse -Force }
New-Item -ItemType Directory -Force -Path (Join-Path $MacHome "bin") | Out-Null
New-Item -ItemType Directory -Force -Path $LibsDest | Out-Null

$libNames = @(
    "core", "dsa", "json", "io", "re", "net", "parallel", "time",
    "nsqlite", "npg", "nmongo", "nos", "nenv", "ncl"
)
foreach ($lib in $libNames) {
    Copy-LibManifest -Name $lib -Ver $Version
}
Write-AhiruLib

$libs = @{}
foreach ($lib in ($libNames + @("ahiru"))) {
    $pkgPath = Join-Path $LibsDest "$lib\package.json"
    if (-not (Test-Path $pkgPath)) { continue }
    $pkg = Get-Content $pkgPath -Raw | ConvertFrom-Json
    $libs[$lib] = @{
        name = $pkg.name
        version = $pkg.version
        kind = $pkg.kind
        description = $pkg.description
        import_paths = @($pkg.import_paths)
        builtin_count = $pkg.builtin_count
        installed_at = $Ts
    }
}

$catalog = @{
    niao_version = $Version
    updated_at = $Ts
    libs = $libs
}
$catalog | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $LibsDest "catalog.json") -Encoding UTF8

$install = @{
    niao_version = $Version
    mode = "global"
    installed_at = $Ts
    root = "niao_home"
    source_root = "engine"
    libs = $libs
}
$install | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $MacHome "install.json") -Encoding UTF8

$ExDest = Join-Path $MacDir "examples"
New-Item -ItemType Directory -Force -Path $ExDest | Out-Null
Copy-Item (Join-Path $Root "examples\hello.niao") $ExDest -Force
Copy-Item (Join-Path $Root "examples\re_demo.niao") $ExDest -Force

Copy-EngineSource

Write-Host ""
Write-Host "Done."
Write-Host "  Libraries: $(($libs.Keys | Measure-Object).Count)"
Write-Host "  Engine:    $Engine"
Write-Host ""
Write-Host "Copy the whole mac/ folder to your MacBook, then run:"
Write-Host "  chmod +x setup.sh niao test.sh"
Write-Host "  ./setup.sh"
