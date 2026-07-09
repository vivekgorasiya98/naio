# Niao nm Online Package Registry

| Domain | Purpose |
|--------|---------|
| **nms.taurus-tech.in** | Node API + admin (`npm start` or Vercel serverless) |
| **nm.c4compare.com** | Static packages (FTP sync) |

See **[DEPLOY.md](./DEPLOY.md)** for full deployment guide.

## Publish a new release (from your PC)

One command builds **all platforms**, seeds the catalog, and uploads to FTP:

```bash
cd package-manager
npm run release
```

### What it does

| Step | Action |
|------|--------|
| 1 | Build **Windows** locally (+ `NiaoSetup.exe`) |
| 2 | Build **Linux + macOS** via GitHub Actions (if `GITHUB_TOKEN` set) |
| 3 | Write `manifest.json` with all 5 platforms |
| 4 | Seed library catalog |
| 5 | Upload everything to FTP (`nm.c4compare.com`) |

### Setup

1. Bump version in `.env`: `NIAO_VERSION=0.2.3`
2. Unlock FTP in StackCP
3. For all platforms from Windows, add to `.env`:
   ```
   GITHUB_TOKEN=ghp_your_pat_here
   GITHUB_REF=main
   ```
   PAT needs `repo` + `actions:read` scopes.

### Skip flags (optional)

| Env | Effect |
|-----|--------|
| `NIAO_SKIP_CI=1` | Windows only, keep existing Linux/Mac files |
| `NIAO_SKIP_FTP=1` | Build only, no upload |
| `NIAO_SKIP_SEED=1` | Skip catalog rebuild |

## Quick start (local)

```bash
cd package-manager
cp .env.example .env
npm install
npm run seed
npm run dev
```

Admin: http://localhost:3000/admin/

## nm client

```bash
nm install nllm    # uses https://nms.taurus-tech.in by default
nm install nrag
```

## Commands

| Command | Description |
|---------|-------------|
| `npm run release` | **All-in-one:** build all platforms + seed + FTP upload |
| `npm run build-release` | Same as `release` |
| `npm run seed` | Build all libs into `data/` |
| `npm run sync-ftp` | Upload static files → nm.c4compare.com |
| `npm run deploy` | Same as `release` |
| `npm run test-ftp` | Test StackCP FTP connection |
| `npm start` | Run API server (deploy on nms.taurus-tech.in) |
