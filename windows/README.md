# Neko for Windows

Complete Windows distribution with a **single installer EXE** — like Python setup.

## Quick install (recommended)

1. Run the build (once, on a dev machine):

   ```powershell
   powershell -File windows\build.ps1
   ```

2. Double-click **`windows\NekoSetup.exe`**

3. Open a **new** Command Prompt or PowerShell:

   ```cmd
   neko version
   neko run examples\hello.neko
   ```

Installs to `%USERPROFILE%\.neko` with `neko.exe`, `nm.exe`, and **all 15 standard libraries** pre-registered. No `nm install` needed.

## Portable mode (no install)

After running `build.ps1`:

```cmd
cd windows
neko.cmd run examples\hello.neko
test.cmd
```

## What's included

| Item | Location |
|------|----------|
| **NekoSetup.exe** | One-click installer (embeds everything) |
| **neko.cmd** | Portable launcher |
| **neko_home/** | Portable runtime (neko, nm, all libs) |
| **examples/** | Demo programs |

## Pre-installed libraries

core, dsa, json, io, re, net, parallel, time, nsqlite, npg, nmongo, nos, nenv, ncl, ahiru

## Full guide

See [GUIDE.md](GUIDE.md) for CLI, language basics, and troubleshooting.

## Rebuild

```powershell
powershell -File windows\build.ps1
```

Output: `windows\NekoSetup.exe`
