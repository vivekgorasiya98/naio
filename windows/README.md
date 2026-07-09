# Niao for Windows

Complete Windows distribution with a **single installer EXE** — like Python setup.

## Quick install (recommended)

1. Run the build (once, on a dev machine):

   ```powershell
   powershell -File windows\build.ps1
   ```

2. Double-click **`windows\NiaoSetup.exe`**

3. Open a **new** Command Prompt or PowerShell:

   ```cmd
   niao version
   niao run examples\hello.niao
   ```

Installs to `%USERPROFILE%\.niao` with `niao.exe`, `nm.exe`, and **all 15 standard libraries** pre-registered. No `nm install` needed.

## Portable mode (no install)

After running `build.ps1`:

```cmd
cd windows
niao.cmd run examples\hello.niao
test.cmd
```

## What's included

| Item | Location |
|------|----------|
| **NiaoSetup.exe** | One-click installer (embeds everything) |
| **niao.cmd** | Portable launcher |
| **niao_home/** | Portable runtime (niao, nm, all libs) |
| **examples/** | Demo programs |

## Pre-installed libraries

core, dsa, json, io, re, net, parallel, time, nsqlite, npg, nmongo, nos, nenv, ncl, ahiru

## Full guide

See [GUIDE.md](GUIDE.md) for CLI, language basics, and troubleshooting.

## Rebuild

```powershell
powershell -File windows\build.ps1
```

Output: `windows\NiaoSetup.exe`
