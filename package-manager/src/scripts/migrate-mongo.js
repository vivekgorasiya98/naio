import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import dotenv from 'dotenv';
import { connectMongo, closeMongo } from '../db/mongo.js';
import {
  compareVersions,
  DEFAULT_PLATFORMS,
  RELEASE_STATUS,
  tarballPublicUrl,
  upsertPackageDoc,
  upsertReleaseDoc,
  upsertVersionDoc,
  VERSION_STATUS,
} from '../db/registry-db.js';
import { config } from '../config.js';
import { readJson, tarballPath } from '../services/storage.js';

dotenv.config();

async function sha256File(filePath) {
  const buf = await fs.readFile(filePath);
  return crypto.createHash('sha256').update(buf).digest('hex');
}

async function migratePackages() {
  const packagesRoot = config.packagesDir;
  let count = 0;
  try {
    const names = await fs.readdir(packagesRoot, { withFileTypes: true });
    for (const entry of names) {
      if (!entry.isDirectory()) continue;
      const name = entry.name;
      const pkgPath = path.join(packagesRoot, name, 'package.json');
      let pkg;
      try {
        pkg = await readJson(pkgPath);
      } catch {
        continue;
      }

      const versionDirs = await fs.readdir(path.join(packagesRoot, name), { withFileTypes: true });
      const versions = versionDirs.filter((e) => e.isDirectory()).map((e) => e.name).sort(compareVersions);

      await upsertPackageDoc({
        name,
        kind: pkg.kind,
        description: pkg.description,
        import_paths: pkg.import_paths,
        builtin_count: pkg.builtin_count,
        remote: pkg.remote === true,
        latest_version: versions.at(-1) || pkg.version,
      });

      for (const version of versions) {
        const libPath = path.join(packagesRoot, name, version, 'lib.json');
        let libJson;
        try {
          libJson = await readJson(libPath);
        } catch {
          continue;
        }
        const tgz = tarballPath(name, version);
        let dist = { tarball_url: tarballPublicUrl(name, version), shasum: '', size: 0 };
        try {
          const stat = await fs.stat(tgz);
          dist.shasum = await sha256File(tgz);
          dist.size = stat.size;
        } catch {
          // tarball may be missing locally
        }
        await upsertVersionDoc({
          name,
          version,
          packageJson: { ...pkg, name, version },
          libJson,
          dist,
          status: VERSION_STATUS.ACTIVE,
        });
        count++;
      }
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err;
  }
  return count;
}

async function migrateReleases() {
  const version = config.niaoVersion;
  const releasesDir = path.join(config.dataDir, 'releases');
  const variants = [];

  for (const p of DEFAULT_PLATFORMS) {
    const ext = p.ext || 'zip';
    const file = path.join(releasesDir, `niao-${version}-${p.id}.${ext}`);
    let shasum = '';
    let size = 0;
    let url = `${config.filesUrl}/releases/niao-${version}-${p.id}.${ext}`;
    try {
      const stat = await fs.stat(file);
      shasum = await sha256File(file);
      size = stat.size;
    } catch {
      // not built on this machine
    }
    variants.push({
      id: p.id,
      label: p.label,
      platform: p.platform,
      arch: p.arch,
      ext,
      status: VERSION_STATUS.ACTIVE,
      url,
      ftp_path: `releases/niao-${version}-${p.id}.${ext}`,
      shasum,
      size,
    });
  }

  await upsertReleaseDoc({
    version,
    status: RELEASE_STATUS.ACTIVE,
    is_latest: true,
    changelog: 'Initial registry release',
    variants,
  });
}

async function main() {
  console.log('\nMongoDB migration\n');
  if (!config.mongo.uri) {
    console.error('MONGODB_URI not set in .env');
    process.exit(1);
  }
  await connectMongo();
  const versions = await migratePackages();
  await migrateReleases();
  console.log(`  ✓ migrated ${versions} package versions`);
  console.log(`  ✓ ensured niao release ${config.niaoVersion}`);
  await closeMongo();
  console.log('\nDone.\n');
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
