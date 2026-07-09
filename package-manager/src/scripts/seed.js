import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import dotenv from 'dotenv';
import {
  publishPackage,
  rebuildCatalog,
  ensureDirs,
  buildTarball,
} from '../services/storage.js';
import { buildStaticApiMirror } from '../services/static-api.js';

dotenv.config();

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const niaoLibs = path.resolve(__dirname, '../../../niao_libs');

/** Optional network download — bundled with core Niao otherwise. */
const REMOTE_ONLY = new Set(['nllm', 'nrag']);

function compareVersions(a, b) {
  const pa = a.split('.').map((n) => parseInt(n, 10) || 0);
  const pb = b.split('.').map((n) => parseInt(n, 10) || 0);
  const len = Math.max(pa.length, pb.length);
  for (let i = 0; i < len; i++) {
    const va = pa[i] ?? 0;
    const vb = pb[i] ?? 0;
    if (va !== vb) return va - vb;
  }
  return 0;
}

function stripBom(text) {
  return text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
}

async function readJsonOptional(filePath) {
  try {
    const raw = stripBom(await fs.readFile(filePath, 'utf8'));
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

async function listVersionDirs(libRoot) {
  const entries = await fs.readdir(libRoot, { withFileTypes: true }).catch(() => []);
  return entries
    .filter((e) => e.isDirectory() && /^\d/.test(e.name))
    .map((e) => e.name)
    .sort(compareVersions);
}

async function loadBasePackage(name, libRoot) {
  const fromPkg = await readJsonOptional(path.join(libRoot, 'package.json'));
  if (fromPkg) return { ...fromPkg, name };

  const versions = await listVersionDirs(libRoot);
  const latest = versions.at(-1);
  if (!latest) return null;

  const fromLib = await readJsonOptional(path.join(libRoot, latest, 'lib.json'));
  if (fromLib) return { ...fromLib, name };

  return null;
}

async function collectVersionFiles(versionDir) {
  const versionFiles = {};
  try {
    const files = await fs.readdir(versionDir);
    for (const file of files) {
      if (file === 'lib.json') continue;
      versionFiles[file] = await fs.readFile(path.join(versionDir, file));
    }
  } catch {
    // metadata-only native libs
  }
  return versionFiles;
}

async function seedLib(name) {
  const libRoot = path.join(niaoLibs, name);
  const base = await loadBasePackage(name, libRoot);
  if (!base) {
    console.warn(`skip ${name}: no package.json or version lib.json`);
    return 0;
  }

  const versions = await listVersionDirs(libRoot);
  if (versions.length === 0) {
    console.warn(`skip ${name}: no version directories`);
    return 0;
  }

  let count = 0;
  for (const version of versions) {
    const versionSrc = path.join(libRoot, version);
    const libJson = await readJsonOptional(path.join(versionSrc, 'lib.json'));
    const pkg = {
      ...base,
      name,
      version,
      kind: libJson?.kind || base.kind || 'native',
      description: libJson?.description || base.description || '',
      import_paths: libJson?.import_paths || base.import_paths || [],
      builtin_count: libJson?.builtin_count ?? base.builtin_count ?? 0,
      remote: REMOTE_ONLY.has(name),
    };

    const versionFiles = await collectVersionFiles(versionSrc);
    if (libJson) {
      versionFiles['lib.json'] = Buffer.from(JSON.stringify(libJson, null, 2) + '\n');
    }

    await publishPackage({
      name,
      version,
      packageJson: pkg,
      versionFiles,
      skipCatalogRebuild: true,
    });
    count++;
  }

  console.log(`seeded ${name}: ${versions.join(', ')}`);
  return count;
}

async function rebuildAllTarballs() {
  const packagesRoot = path.resolve(__dirname, '../../data/packages');
  const names = await fs.readdir(packagesRoot, { withFileTypes: true });
  let built = 0;
  for (const entry of names) {
    if (!entry.isDirectory()) continue;
    const versions = await listVersionDirs(path.join(packagesRoot, entry.name));
    for (const version of versions) {
      await buildTarball(entry.name, version);
      built++;
    }
  }
  return built;
}

async function main() {
  await ensureDirs();

  const entries = await fs.readdir(niaoLibs, { withFileTypes: true });
  const libNames = entries
    .filter((e) => e.isDirectory() && e.name !== 'node_modules')
    .map((e) => e.name)
    .sort();

  let versionCount = 0;
  for (const name of libNames) {
    versionCount += await seedLib(name);
  }

  const tarballs = await rebuildAllTarballs();
  const catalog = await rebuildCatalog();
  await buildStaticApiMirror();

  console.log('');
  console.log(`catalog: ${Object.keys(catalog.libs || {}).length} libs`);
  console.log(`versions published: ${versionCount}`);
  console.log(`tarballs: ${tarballs}`);
  console.log(`remote-only: ${(catalog.remote_libs || []).join(', ') || '(none)'}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
