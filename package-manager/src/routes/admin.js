import fs from 'node:fs/promises';
import path from 'node:path';
import {
  deleteVersion,
  listPackages,
  listVersions,
  publishPackage,
  readPackageManifest,
  rebuildCatalog,
  updatePackageStatus,
  updateVersionStatus,
} from '../services/storage.js';
import { verifyLogin, requireAdmin } from '../services/auth.js';
import { syncToFtp, ftpConfigured } from '../services/ftp.js';
import { buildStaticApiMirror } from '../services/static-api.js';
import { config } from '../config.js';
import {
  deleteReleaseFromDb,
  getPackageFromDb,
  getReleaseFromDb,
  listReleasesFromDb,
  PKG_STATUS,
  RELEASE_STATUS,
  setReleaseStatus,
  updateReleaseVariant,
  upsertReleaseDoc,
  VERSION_STATUS,
  isMongoPrimary,
} from '../db/registry-db.js';
import { isDbReady } from '../db/mongo.js';

async function syncStaticFiles() {
  await buildStaticApiMirror();
  return syncToFtp();
}

async function maybeSync({ blocking = false } = {}) {
  if (!config.ftp.autoSync || !ftpConfigured()) return null;

  const run = async () => {
    try {
      return await syncStaticFiles();
    } catch (err) {
      return { error: err.message };
    }
  };

  if (blocking) return run();

  // Don't block publish/admin API — sync runs in background
  run().catch((err) => {
    console.warn('Background FTP sync failed:', err.message);
  });
  return { queued: true };
}

export async function adminRoutes(app) {
  app.post('/admin/login', async (req, reply) => {
    const { username, password } = req.body || {};
    if (!username || !password) {
      reply.code(400);
      return { error: 'username and password required' };
    }
    const session = await verifyLogin(username, password);
    if (!session) {
      reply.code(401);
      return { error: 'invalid credentials' };
    }
    return session;
  });

  app.register(async (secured) => {
    secured.addHook('preHandler', requireAdmin);

    secured.get('/admin/status', async () => ({
      mongo: isDbReady(),
      ftp: ftpConfigured(),
      apiUrl: config.apiUrl,
      filesUrl: config.filesUrl,
      niaoVersion: config.niaoVersion,
    }));

    // ── Packages ──────────────────────────────────────────────

    secured.get('/admin/packages', async () => {
      const names = await listPackages({ admin: true });
      const packages = [];
      for (const name of names) {
        try {
          const pkg = isMongoPrimary()
            ? await getPackageFromDb(name, { admin: true })
            : { ...(await readPackageManifest(name, { admin: true })), versions: await listVersions(name, { admin: true }) };
          packages.push(pkg);
        } catch {
          // skip
        }
      }
      return { packages };
    });

    secured.post('/admin/packages/:name', async (req, reply) => {
      const { name } = req.params;
      const parts = req.parts();
      let version = null;
      let packageJson = null;
      const versionFiles = {};

      for await (const part of parts) {
        if (part.type === 'field') {
          if (part.fieldname === 'version') version = (await part.value).toString();
          if (part.fieldname === 'packageJson') packageJson = (await part.value).toString();
        } else if (part.type === 'file') {
          const rel = part.fieldname.replace(/^files\//, '');
          versionFiles[rel] = await part.toBuffer();
        }
      }

      if (!version) {
        reply.code(400);
        return { error: 'version field required' };
      }
      if (!packageJson) {
        try {
          packageJson = await fs.readFile(path.join(config.packagesDir, name, 'package.json'), 'utf8');
        } catch {
          reply.code(400);
          return { error: 'packageJson field or existing package.json required' };
        }
      }

      try {
        const result = await publishPackage({ name, version, packageJson, versionFiles });
        const ftp = await maybeSync();
        return { ok: true, ...result, ftp };
      } catch (err) {
        reply.code(400);
        return { error: err.message };
      }
    });

    secured.post('/admin/packages/:name/:version/publish-json', async (req, reply) => {
      const { name, version } = req.params;
      const body = req.body || {};
      try {
        const result = await publishPackage({
          name,
          version,
          packageJson: body.package || body,
          versionFiles: body.files || {},
        });
        const ftp = await maybeSync();
        return { ok: true, ...result, ftp };
      } catch (err) {
        reply.code(400);
        return { error: err.message };
      }
    });

    secured.patch('/admin/packages/:name/status', async (req, reply) => {
      const { name } = req.params;
      const { status } = req.body || {};
      if (!Object.values(PKG_STATUS).includes(status)) {
        reply.code(400);
        return { error: `invalid status — use: ${Object.values(PKG_STATUS).join(', ')}` };
      }
      try {
        const catalog = await updatePackageStatus(name, status);
        const ftp = await maybeSync();
        return { ok: true, catalog, ftp };
      } catch (err) {
        reply.code(400);
        return { error: err.message };
      }
    });

    secured.patch('/admin/packages/:name/:version/status', async (req, reply) => {
      const { name, version } = req.params;
      const { status } = req.body || {};
      if (!Object.values(VERSION_STATUS).includes(status)) {
        reply.code(400);
        return { error: `invalid status — use: ${Object.values(VERSION_STATUS).join(', ')}` };
      }
      try {
        const catalog = await updateVersionStatus(name, version, status);
        const ftp = await maybeSync();
        return { ok: true, catalog, ftp };
      } catch (err) {
        reply.code(400);
        return { error: err.message };
      }
    });

    secured.delete('/admin/packages/:name/:version', async (req, reply) => {
      const { name, version } = req.params;
      const hard = req.query?.hard === 'true';
      try {
        const catalog = await deleteVersion(name, version, { hard });
        const ftp = await maybeSync();
        return { ok: true, catalog, ftp };
      } catch (err) {
        reply.code(400);
        return { error: err.message };
      }
    });

    // ── Niao releases ─────────────────────────────────────────

    secured.get('/admin/releases', async () => {
      const releases = isMongoPrimary() ? await listReleasesFromDb({ admin: true }) : [];
      return { releases };
    });

    secured.get('/admin/releases/:version', async (req, reply) => {
      const release = await getReleaseFromDb(req.params.version, { admin: true });
      if (!release) {
        reply.code(404);
        return { error: 'release not found' };
      }
      return release;
    });

    secured.post('/admin/releases', async (req, reply) => {
      if (!isMongoPrimary()) {
        reply.code(400);
        return { error: 'MongoDB required for release management' };
      }
      try {
        const release = await upsertReleaseDoc(req.body || {});
        const ftp = await maybeSync();
        return { ok: true, release, ftp };
      } catch (err) {
        reply.code(400);
        return { error: err.message };
      }
    });

    secured.patch('/admin/releases/:version', async (req, reply) => {
      if (!isMongoPrimary()) {
        reply.code(400);
        return { error: 'MongoDB required' };
      }
      const { version } = req.params;
      const body = req.body || {};
      try {
        if (body.status) {
          await setReleaseStatus(version, body.status);
        }
        const existing = await getReleaseFromDb(version, { admin: true });
        const release = await upsertReleaseDoc({ ...existing, ...body, version });
        const ftp = await maybeSync();
        return { ok: true, release, ftp };
      } catch (err) {
        reply.code(400);
        return { error: err.message };
      }
    });

    secured.patch('/admin/releases/:version/variants/:variantId', async (req, reply) => {
      if (!isMongoPrimary()) {
        reply.code(400);
        return { error: 'MongoDB required' };
      }
      try {
        await updateReleaseVariant(req.params.version, req.params.variantId, req.body || {});
        const release = await getReleaseFromDb(req.params.version, { admin: true });
        const ftp = await maybeSync();
        return { ok: true, release, ftp };
      } catch (err) {
        reply.code(400);
        return { error: err.message };
      }
    });

    secured.delete('/admin/releases/:version', async (req, reply) => {
      if (!isMongoPrimary()) {
        reply.code(400);
        return { error: 'MongoDB required' };
      }
      try {
        await deleteReleaseFromDb(req.params.version);
        const ftp = await maybeSync();
        return { ok: true, ftp };
      } catch (err) {
        reply.code(400);
        return { error: err.message };
      }
    });

    // ── Catalog & FTP ─────────────────────────────────────────

    secured.post('/admin/catalog/rebuild', async () => {
      const catalog = await rebuildCatalog();
      const ftp = await maybeSync();
      return { ok: true, catalog, ftp };
    });

    secured.post('/admin/sync/ftp', async (_req, reply) => {
      try {
        const result = await syncStaticFiles();
        return { ok: true, ...result };
      } catch (err) {
        reply.code(500);
        return { error: err.message };
      }
    });
  });
}
