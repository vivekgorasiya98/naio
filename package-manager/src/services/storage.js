import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import * as tar from 'tar';
import { config } from '../config.js';
import {
  fetchCatalog,
  fetchPackageList,
  fetchPackageMeta,
  fetchVersionMeta,
  remoteTarballUrl,
} from './remote-registry.js';
import {
  buildCatalogFromDb,
  compareVersions,
  getPackageFromDb,
  getVersionFromDb,
  isMongoPrimary,
  listPackagesFromDb,
  removeVersionFromDb,
  setPackageStatus,
  setVersionStatus,
  upsertPackageDoc,
  upsertVersionDoc,
  VERSION_STATUS,
  PKG_STATUS,
} from '../db/registry-db.js';

const PKG_NAME_RE = /^[a-z][a-z0-9_-]*$/i;
const VERSION_RE = /^\d+\.\d+\.\d+([-.+][\w.-]+)?$/;

export function validatePackageName(name) {
  if (!PKG_NAME_RE.test(name)) {
    throw new Error(`invalid package name: ${name}`);
  }
}

export function validateVersion(version) {
  if (!VERSION_RE.test(version)) {
    throw new Error(`invalid version: ${version}`);
  }
}

async function pathExists(filePath) {
  try {
    await fs.access(filePath);
    return true;
  } catch {
    return false;
  }
}

export async function ensureDirs() {
  if (!config.isServerless && !(await pathExists(config.dataDir))) {
    await fs.mkdir(config.dataDir, { recursive: true });
  }
  await fs.mkdir(config.packagesDir, { recursive: true });
  await fs.mkdir(config.tarballsDir, { recursive: true });
}

export function packageDir(name) {
  validatePackageName(name);
  return path.join(config.packagesDir, name);
}

export function versionDir(name, version) {
  validatePackageName(name);
  validateVersion(version);
  return path.join(packageDir(name), version);
}

export function tarballPath(name, version) {
  validatePackageName(name);
  validateVersion(version);
  return path.join(config.tarballsDir, `${name}-${version}.tgz`);
}

export async function readJson(filePath) {
  let raw = await fs.readFile(filePath, 'utf8');
  if (raw.charCodeAt(0) === 0xfeff) raw = raw.slice(1);
  return JSON.parse(raw);
}

export async function writeJson(filePath, value) {
  await fs.mkdir(path.dirname(filePath), { recursive: true });
  await fs.writeFile(filePath, JSON.stringify(value, null, 2) + '\n', 'utf8');
}

async function readLocalCatalog() {
  if (!(await pathExists(config.catalogPath))) return null;
  return readJson(config.catalogPath);
}

export async function readCatalog() {
  if (isMongoPrimary()) {
    try {
      const catalog = await buildCatalogFromDb();
      if (catalog && Object.keys(catalog.libs || {}).length > 0) {
        await writeJson(config.catalogPath, catalog);
        return catalog;
      }
    } catch (err) {
      console.warn('MongoDB catalog read failed:', err.message);
    }
  }
  if (config.remoteReads) {
    try {
      const remote = await fetchCatalog();
      const remoteCount = Object.keys(remote?.libs || {}).length;
      const local = await readLocalCatalog();
      const localCount = Object.keys(local?.libs || {}).length;
      if (remoteCount > 0 && remoteCount >= localCount) {
        return remote;
      }
      if (local) return local;
      if (remoteCount > 0) return remote;
    } catch {
      // fall through to local
    }
  }
  const local = await readLocalCatalog();
  if (local) return local;
  throw new Error('catalog not found — run npm run seed');
}

export async function listPackages({ admin = false } = {}) {
  if (isMongoPrimary()) {
    const names = await listPackagesFromDb({ admin });
    if (names?.length) return names;
  }
  await ensureDirs();
  let local = [];
  try {
    const entries = await fs.readdir(config.packagesDir, { withFileTypes: true });
    local = entries.filter((e) => e.isDirectory()).map((e) => e.name);
  } catch {
    // empty or missing
  }
  if (admin || !config.remoteReads) {
    return local.sort();
  }
  if (config.remoteReads) {
    try {
      const remote = await fetchPackageList();
      if (remote.length > 0) {
        const merged = new Set([...local, ...remote]);
        return [...merged].sort();
      }
    } catch {
      // fall through
    }
  }
  return local.sort();
}

export async function listVersions(name, { admin = false } = {}) {
  if (isMongoPrimary()) {
    const pkg = await getPackageFromDb(name, { admin });
    if (pkg?.versions?.length) return pkg.versions;
  }
  const root = packageDir(name);
  try {
    const entries = await fs.readdir(root, { withFileTypes: true });
    const versions = [];
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      const vdir = path.join(root, entry.name);
      const hasLib = await fs.access(path.join(vdir, 'lib.json')).then(() => true).catch(() => false);
      if (hasLib || (await fs.readdir(vdir)).length > 0) {
        versions.push(entry.name);
      }
    }
    if (versions.length > 0) return versions.sort(compareVersions);
  } catch {
    // fall through to remote
  }
  if (config.remoteReads) {
    const meta = await fetchPackageMeta(name);
    return (meta.versions || []).sort(compareVersions);
  }
  return [];
}

export async function readPackageManifest(name, { admin = false } = {}) {
  if (isMongoPrimary()) {
    const pkg = await getPackageFromDb(name, { admin });
    if (pkg) {
      return {
        name: pkg.name,
        version: pkg.latest || pkg.version,
        kind: pkg.kind,
        description: pkg.description,
        import_paths: pkg.import_paths,
        builtin_count: pkg.builtin_count,
        remote: pkg.remote,
        status: pkg.status,
        versions: pkg.versions,
      };
    }
  }
  const pkgPath = path.join(packageDir(name), 'package.json');
  if (await pathExists(pkgPath)) {
    return readJson(pkgPath);
  }
  if (config.remoteReads) {
    const meta = await fetchPackageMeta(name);
    return meta.package || meta;
  }
  throw new Error(`package not found: ${name}`);
}

export async function readVersionManifest(name, version, { allowYanked = false } = {}) {
  if (isMongoPrimary()) {
    const ver = await getVersionFromDb(name, version, { allowYanked });
    if (ver?.lib_json) return ver.lib_json;
    if (ver?.lib) return ver.lib;
  }
  const localPath = path.join(versionDir(name, version), 'lib.json');
  if (await pathExists(localPath)) {
    return readJson(localPath);
  }
  if (config.remoteReads) {
    const meta = await fetchVersionMeta(name, version);
    return meta.lib || meta;
  }
  throw new Error(`version not found: ${name}@${version}`);
}

export async function buildTarball(name, version) {
  const out = tarballPath(name, version);
  const tarballUrl = remoteTarballUrl(name, version);

  if (await pathExists(out)) {
    const buf = await fs.readFile(out);
    const sha256 = crypto.createHash('sha256').update(buf).digest('hex');
    return { path: out, sha256, size: buf.length, tarballUrl };
  }

  const vdir = versionDir(name, version);
  const hasLocalVersion = await pathExists(path.join(vdir, 'lib.json'));
  if (hasLocalVersion) {
    await tar.c(
      {
        gzip: true,
        file: out,
        cwd: config.packagesDir,
        portable: true,
      },
      [name],
    );

    const buf = await fs.readFile(out);
    const sha256 = crypto.createHash('sha256').update(buf).digest('hex');
    return { path: out, sha256, size: buf.length, tarballUrl };
  }

  if (config.remoteReads) {
    const meta = await fetchVersionMeta(name, version);
    const dist = meta.dist || {};
    return {
      path: out,
      sha256: dist.shasum || dist.sha256 || '',
      size: dist.size || 0,
      tarballUrl: dist.tarball || tarballUrl,
    };
  }

  throw new Error(`version not found: ${name}@${version}`);
}

export async function rebuildCatalog() {
  if (isMongoPrimary()) {
    const catalog = await buildCatalogFromDb();
    if (catalog) {
      await writeJson(config.catalogPath, catalog);
      return catalog;
    }
  }
  const names = await listPackages();
  const libs = {};
  const remote = [];

  for (const name of names) {
    let pkg;
    try {
      pkg = await readPackageManifest(name);
    } catch {
      continue;
    }
    const versions = await listVersions(name);
    const latest = versions.at(-1) || pkg.version;
    libs[name] = {
      name,
      version: latest,
      kind: pkg.kind || 'native',
      description: pkg.description || '',
      import_paths: pkg.import_paths || [],
      builtin_count: pkg.builtin_count || 0,
      versions,
      remote: pkg.remote === true,
    };
    if (pkg.remote === true) remote.push(name);
  }

  const catalog = {
    niao_version: config.niaoVersion,
    description: 'Niao online package registry',
    updated_at: String(Date.now()),
    remote_libs: remote,
    libs,
  };

  await writeJson(config.catalogPath, catalog);
  return catalog;
}

export async function publishPackage({
  name,
  version,
  packageJson,
  versionFiles,
  skipCatalogRebuild = false,
}) {
  validatePackageName(name);
  validateVersion(version);

  const root = packageDir(name);
  const vdir = versionDir(name, version);
  await fs.mkdir(vdir, { recursive: true });

  const pkg = typeof packageJson === 'string' ? JSON.parse(packageJson) : packageJson;
  if (pkg.name && pkg.name !== name) {
    throw new Error(`package name mismatch: expected ${name}, got ${pkg.name}`);
  }
  pkg.name = name;
  pkg.version = version;

  await writeJson(path.join(root, 'package.json'), pkg);

  const hasLibJson = versionFiles && Object.keys(versionFiles).includes('lib.json');
  if (!hasLibJson) {
    const libManifest = {
      name,
      version,
      kind: pkg.kind || 'native',
      description: pkg.description || '',
      import_paths: pkg.import_paths || [],
      builtin_count: pkg.builtin_count || 0,
    };
    await writeJson(path.join(vdir, 'lib.json'), libManifest);
  }

  for (const [relPath, content] of Object.entries(versionFiles || {})) {
    const safe = path.normalize(relPath).replace(/^(\.\.[/\\])+/, '');
    if (!safe || safe.startsWith('..')) continue;
    const dest = path.join(vdir, safe);
    await fs.mkdir(path.dirname(dest), { recursive: true });
    await fs.writeFile(dest, content);
  }

  const tarball = await buildTarball(name, version);
  const libJson = await readJson(path.join(vdir, 'lib.json'));

  if (isMongoPrimary()) {
    await upsertPackageDoc({
      name,
      ...pkg,
      latest_version: version,
      status: pkg.status || PKG_STATUS.ACTIVE,
    });
    await upsertVersionDoc({
      name,
      version,
      packageJson: pkg,
      libJson,
      dist: {
        tarball_url: tarball.tarballUrl,
        shasum: tarball.sha256,
        size: tarball.size,
      },
      status: VERSION_STATUS.ACTIVE,
    });
  }

  const catalog = skipCatalogRebuild ? null : await rebuildCatalog();
  return { package: pkg, tarball, catalog };
}

export async function updatePackageStatus(name, status) {
  validatePackageName(name);
  if (isMongoPrimary()) await setPackageStatus(name, status);
  return rebuildCatalog();
}

export async function updateVersionStatus(name, version, status) {
  validatePackageName(name);
  validateVersion(version);
  if (isMongoPrimary()) await setVersionStatus(name, version, status);
  return rebuildCatalog();
}

export async function deleteVersion(name, version, { hard = false } = {}) {
  const vdir = versionDir(name, version);
  await fs.rm(vdir, { recursive: true, force: true });
  const tgz = tarballPath(name, version);
  await fs.rm(tgz, { force: true });

  const versions = await listVersions(name);
  if (versions.length === 0) {
    await fs.rm(packageDir(name), { recursive: true, force: true });
  } else {
    const pkg = await readPackageManifest(name, { admin: true });
    pkg.version = versions.at(-1);
    await writeJson(path.join(packageDir(name), 'package.json'), pkg);
  }

  if (isMongoPrimary()) {
    if (hard) {
      await removeVersionFromDb(name, version);
    } else {
      await setVersionStatus(name, version, VERSION_STATUS.YANKED);
    }
  }

  return rebuildCatalog();
}

export { compareVersions };
