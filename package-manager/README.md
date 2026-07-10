# Niao nm Online Package Registry

| Domain | Purpose |
|--------|---------|
| **nms.taurus-tech.in** | Node API + admin (`npm start` or Vercel serverless) |
| **nm.c4compare.com** | Static packages (FTP sync) |

See **[DEPLOY.md](./DEPLOY.md)** for full deployment guide.

## Publish a new release (from your PC)

One command — **build locally, upload directly to FTP**. No GitHub needed.

```bash
cd package-manager
npm run release
```

### What it does

| Step | Action |
|------|--------|
| 1 | Build `niao` + `nm` for your OS (+ `NiaoSetup.exe` on Windows) |
| 2 | Write `manifest.json` |
| 3 | Seed library catalog |
| 4 | Upload everything to FTP (`nm.c4compare.com`) |

### Setup

1. Set FTP credentials in `.env` (`FTP_HOST`, `FTP_USER`, `FTP_PASSWORD`)
2. Bump version: `NIAO_VERSION=0.2.3`
3. Unlock FTP in StackCP
4. Run `npm run release`

On Windows you get Windows builds. Run the same command on a Linux or Mac machine to add those platforms.

### Skip flags (optional)

| Env | Effect |
|-----|--------|
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
