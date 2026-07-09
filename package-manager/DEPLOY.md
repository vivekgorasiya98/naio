# Deploy Niao nm Registry

Two domains, two roles:

| Domain | Role | How |
|--------|------|-----|
| **nm.c4compare.com** | Static package files | StackCP FTP (`npm run sync-ftp`) |
| **nms.taurus-tech.in** | API + admin (Node) | Your provider VPS / server |

```
nm install nllm
     │
     ▼
nms.taurus-tech.in/v1/catalog     ← API (Node server)
nms.taurus-tech.in/v1/packages/…
     │
     ▼ download tarball
nm.c4compare.com/tarballs/…      ← static files (FTP)
```

---

## 1. Static files — nm.c4compare.com (already done via FTP)

```bash
cd package-manager
npm run seed          # build catalog + packages + tarballs
npm run sync-ftp      # upload to StackCP FTP root /
```

Unlock FTP in StackCP before sync. Files land at `/`:
- `catalog.json`, `packages/`, `tarballs/`, `v1/`, `.htaccess`

---

## 2. API server — nms.taurus-tech.in

Choose **one** deployment target:

### Option A — VPS (recommended for always-on API)

**Requirements:** Node.js 20+, writable `data/` directory, port 3000 (or change `PORT` in `.env`)

```bash
# 1. Copy package-manager folder to server
scp -r package-manager/ user@your-server:/opt/niao-nms/

# 2. On the server
cd /opt/niao-nms
cp .env.example .env
# Edit .env — set API_URL, FILES_URL, admin password, JWT secret
# FTP_* only needed if you sync from the server; optional on VPS

npm install --production
npm run seed          # local data/ copy for API to serve metadata
npm start             # or use PM2 below
```

### Option B — Serverless (Vercel / AWS Lambda)

Serverless runtimes have a **read-only** filesystem (`/var/task`). The app auto-detects this and:

- Writes packages to **`/tmp/niao-nms/data`** (only writable path)
- Reads catalog/packages from **`FILES_URL`** (`nm.c4compare.com`) when local data is empty
- Redirects tarball downloads to the static CDN
- Admin publish writes to `/tmp` then **FTP syncs** to `nm.c4compare.com`

**Do not set `DATA_DIR=./data` on serverless** — leave it unset so `/tmp` is used automatically.

```bash
# Deploy to Vercel (from package-manager/)
npm install -g vercel
vercel

# Set env vars in Vercel dashboard (or vercel env add):
# API_URL, FILES_URL, MONGODB_URI, MONGODB_DB
# ADMIN_USERNAME, ADMIN_PASSWORD, JWT_SECRET
# FTP_HOST, FTP_USER, FTP_PASSWORD, FTP_REMOTE_DIR=/
# FTP_AUTO_SYNC=false   # use manual Sync FTP from admin on serverless
```

`vercel.json` routes all traffic to `api/index.js` (Fastify serverless handler).

Static files must already exist on **nm.c4compare.com** (`npm run deploy` from your dev machine).

### `.env` on the API server

```env
API_URL=https://nms.taurus-tech.in
FILES_URL=https://nm.c4compare.com
PORT=3000
HOST=0.0.0.0
NODE_ENV=production
ADMIN_USERNAME=admin
ADMIN_PASSWORD=<strong-password>
JWT_SECRET=<64-char-hex>
MONGODB_URI=mongodb+srv://...
MONGODB_DB=niao-nms
```

### MongoDB setup

```bash
# After .env has MONGODB_URI:
npm run migrate-mongo   # import existing data/packages → MongoDB
npm start
```

Collections: `packages`, `package_versions`, `niao_releases`. Admin panel manages discontinue/yank/delete; FTP serves tarballs and binary zips from `nm.c4compare.com`.

### PM2 (recommended)

```bash
npm install -g pm2
pm2 start ecosystem.config.cjs
pm2 save
pm2 startup
```

### Nginx reverse proxy (nms.taurus-tech.in → :3000)

```nginx
server {
    listen 80;
    server_name nms.taurus-tech.in;
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl http2;
    server_name nms.taurus-tech.in;

    ssl_certificate     /etc/letsencrypt/live/nms.taurus-tech.in/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/nms.taurus-tech.in/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

```bash
sudo certbot --nginx -d nms.taurus-tech.in
sudo nginx -t && sudo systemctl reload nginx
```

### DNS

| Record | Type | Value |
|--------|------|-------|
| `nms.taurus-tech.in` | A | Your VPS IP |
| `nm.c4compare.com` | A/CNAME | StackCP hosting |

---

## 3. Verify

```bash
# API (nms)
curl https://nms.taurus-tech.in/health
curl https://nms.taurus-tech.in/v1/catalog

# Static tarball (nm)
curl -I https://nm.c4compare.com/v1/packages/nllm/0.2.2/tarball

# nm client
nm install nllm
nm install nrag
```

Admin UI: **https://nms.taurus-tech.in/admin/**

---

## 4. Publish updates

From your dev machine (with FTP unlocked):

```bash
# Build Windows toolchain + seed libs + sync static files to nm.c4compare.com
npm run deploy
```

**What users download from the website:**
- `niao` + `nm` binaries (VM/NFE runtime) per platform — **not** libraries
- Full source from GitHub

**What `nm` installs via API** (not direct user download):
- Package libraries (`nm install nllm`, etc.) — tarballs at `/v1/packages/.../tarball`

### All platform binaries (Windows, Linux, macOS)

On Windows, `npm run deploy` builds **Windows x64** only. For Linux and macOS:

1. Push tag `v0.2.2` or run GitHub Actions workflow **Niao Release**
2. Download artifacts from each matrix job
3. Copy to `package-manager/data/releases/` as `niao-0.2.2-{platform}.{zip|tar.gz}`
4. Run `npm run write-manifest && npm run sync-ftp`

Or set `NIAO_BUILD_ALL=1` with cross-compilation targets installed locally.

On API server after catalog changes:

```bash
npm run seed      # refresh local data/
pm2 restart niao-nms
```

Or enable `FTP_AUTO_SYNC=true` on a machine with FTP access — admin publish syncs static files automatically.
