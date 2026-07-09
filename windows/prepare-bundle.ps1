# Stage Windows payload: niao.exe, nm.exe, all libraries, examples.
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$WinDir = $PSScriptRoot
$Payload = Join-Path $WinDir "payload"
$PortableHome = Join-Path $WinDir "niao_home"
$SrcLibs = Join-Path $Root "niao_libs"
$ReleaseBin = Join-Path $Root "target\release"

$Version = "0.2.2"
$Ts = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds().ToString()

function Stage-Libs {
    param([string]$LibsDest)

    New-Item -ItemType Directory -Force -Path $LibsDest | Out-Null

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
        }
    }

    $libNames = @(
        "core", "dsa", "json", "io", "re", "net", "parallel", "time",
        "nsqlite", "npg", "nmongo", "nos", "nenv", "ncl"
    )
    foreach ($lib in $libNames) { Copy-LibManifest -Name $lib -Ver $Version }

    $ahiruPkg = Join-Path $SrcLibs "ahiru\package.json"
    if (Test-Path $ahiruPkg) {
        $ahiruVer = (Get-Content $ahiruPkg -Raw | ConvertFrom-Json).version
        Copy-LibManifest -Name "ahiru" -Ver $ahiruVer
    } else {
        $ahiruLib = Join-Path $LibsDest "ahiru"
        $ahiruVerDir = Join-Path $ahiruLib "0.3.0"
        New-Item -ItemType Directory -Force -Path $ahiruVerDir | Out-Null
        $ahiruSpec = @{
            name = "ahiru"
            version = "0.3.0"
            kind = "native"
            description = "ahiru-server 0.3.0: state, custom middleware, groups, cache, jobs, metrics, CLI toolkit"
            import_paths = @("ahiru", "std/ahiru")
            builtin_count = 36
        }
        $ahiruJson = $ahiruSpec | ConvertTo-Json -Depth 5
        Set-Content (Join-Path $ahiruVerDir "lib.json") $ahiruJson -Encoding UTF8
        Set-Content (Join-Path $ahiruLib "package.json") $ahiruJson -Encoding UTF8
    }

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

    $catalog = @{ niao_version = $Version; updated_at = $Ts; libs = $libs }
    $catalog | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $LibsDest "catalog.json") -Encoding UTF8
    return $libs
}

function Write-InstallJson {
    param([string]$HomeRoot, [hashtable]$Libs)
    $install = @{
        niao_version = $Version
        mode = "global"
        installed_at = $Ts
        root = $HomeRoot
        source_root = ""
        libs = $Libs
    }
    $install | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $HomeRoot "install.json") -Encoding UTF8
}

Write-Host "== Preparing Windows bundle =="

$niaoExe = Join-Path $ReleaseBin "niao.exe"
$nmExe = Join-Path $ReleaseBin "nm.exe"
if (-not (Test-Path $niaoExe)) {
    throw "Missing $niaoExe - run: cargo build --release --no-default-features --bin niao --bin nm"
}

if (Test-Path $Payload) { Remove-Item $Payload -Recurse -Force }
New-Item -ItemType Directory -Force -Path (Join-Path $Payload "bin") | Out-Null

$libs = Stage-Libs -LibsDest (Join-Path $Payload "niao_libs")
Copy-Item $niaoExe (Join-Path $Payload "bin\niao.exe") -Force
Copy-Item $nmExe (Join-Path $Payload "bin\nm.exe") -Force

$installPayload = @{
    niao_version = $Version
    mode = "global"
    installed_at = $Ts
    root = "%USERPROFILE%\.niao"
    source_root = ""
    libs = $libs
}
$installPayload | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $Payload "install.json") -Encoding UTF8

$exPayload = Join-Path $Payload "examples"
New-Item -ItemType Directory -Force -Path $exPayload | Out-Null
Copy-Item (Join-Path $Root "examples\hello.niao") $exPayload -Force
Copy-Item (Join-Path $Root "examples\re_demo.niao") $exPayload -Force
if (Test-Path (Join-Path $Root "mac\examples\libs_smoke.niao")) {
    Copy-Item (Join-Path $Root "mac\examples\libs_smoke.niao") $exPayload -Force
}

if (Test-Path $PortableHome) { Remove-Item $PortableHome -Recurse -Force }
robocopy $Payload $PortableHome /E /NFL /NDL /NJH /NJS /nc /ns /np | Out-Null
if ($LASTEXITCODE -ge 8) { throw "robocopy portable home failed: $LASTEXITCODE" }
$portableRoot = (Resolve-Path $PortableHome).Path
Write-InstallJson -HomeRoot $portableRoot -Libs $libs

$exPortable = Join-Path $WinDir "examples"
New-Item -ItemType Directory -Force -Path $exPortable | Out-Null
Copy-Item (Join-Path $exPayload "*") $exPortable -Force

Write-Host "Done."
Write-Host "  payload:     $Payload"
Write-Host "  niao_home:   $PortableHome"
Write-Host "  libraries:   $(($libs.Keys | Measure-Object).Count)"
