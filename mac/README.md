# Niao for macOS — copy this folder only

**Full documentation:** [GUIDE.md](GUIDE.md) — complete install, CLI, language, and library reference.

Everything you need is inside **`mac/`**. Copy this whole folder to your MacBook (USB, AirDrop, zip, etc.). You do **not** need the rest of the Niao repo.

## First time on Mac (3 steps)

### 1. Install Rust (once, like installing Python)

```bash
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

### 2. Build niao (once, ~5–10 min)

```bash
cd mac
chmod +x setup.sh niao test.sh
./setup.sh
```

### 3. Run

```bash
./niao version
./niao run examples/hello.niao
./niao run examples/re_demo.niao
./test.sh
```

After `./setup.sh`, use `./niao` for everything. No library install step — all 15 standard libs are already configured.

## Global `niao` command (optional)

Add to `~/.zshrc`:

```bash
export NIAO_HOME="/path/to/mac/niao_home"
export PATH="/path/to/mac/niao_home/bin:$PATH"
```

Then `niao run myfile.niao` works from any directory.

## What's inside

| Path | Purpose |
|------|---------|
| `engine/` | Compiler source (used only for first build) |
| `niao_home/bin/` | `niao` + `nm` binaries (created by setup) |
| `niao_home/niao_libs/` | All libraries pre-registered |
| `examples/` | Demo programs |
| `setup.sh` | One-time build script |
| `niao` | Launcher script |

## Pre-installed libraries

core, dsa, json, io, re, net, parallel, time, nsqlite, npg, nmongo, nos, nenv, ncl, ahiru

## Why isn't niao pre-built?

macOS apps must be compiled **on a Mac** (or Mac CI). A Windows PC cannot produce a working `niao` binary for Mac. This folder includes the source in `engine/` so **`./setup.sh` builds it on your MacBook** — one time only.

## Refresh this folder (on Windows)

From the main Niao repo:

```powershell
powershell -File mac/prepare-bundle.ps1
```

Then copy `mac/` to your MacBook again.
