/**
 * Niao release — build locally, upload directly to FTP.
 * No GitHub required.
 *
 *   npm run release
 *
 * Builds niao + nm for your OS, creates installers, seeds catalog,
 * uploads everything to FTP (nm.c4compare.com).
 */
import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { execSync, spawnSync } from 'node:child_process';
import * as tar from 'tar';
import dotenv from 'dotenv';
import { config } from '../config.js';
import { DEFAULT_PLATFORMS, releaseInstallerFileName } from '../db/registry-db.js';
import { syncToFtp, testFtpConnection, ftpConfigured, createConsoleFtpProgress } from '../services/ftp.js';
import {
  ensureWindowsBuildTools,
  execWithMsvc,
  hasNasm,
  hasMsvcArm64,
  hasMsvcX86,
  hasClang,
} from './ensure-build-tools.js';

dotenv.config();

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const pkgRoot = path.resolve(__dirname, '../..');
const repoRoot = path.resolve(pkgRoot, '..');
const version = config.niaoVersion;
const releasesDir = path.join(config.dataDir, 'releases');

const CARGO_ARGS = '--release --no-default-features -p niao_cli -p niao_nm';
const SKIP_FTP = process.env.NIAO_SKIP_FTP === '1';
const SKIP_SEED = process.env.NIAO_SKIP_SEED === '1';
const MACOS_TARGETS = new Set(['x86_64-apple-darwin', 'aarch64-apple-darwin']);
const LINUX_TARGETS = new Set(['x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu']);
const WINDOWS_CROSS_TARGETS = new Set(['i686-pc-windows-msvc', 'aarch64-pc-windows-msvc']);

/** @type {{ pathExtra: string, vsPath: string | null }} */
let winBuildEnv = { pathExtra: '', vsPath: null };

async function sha256File(filePath) {
  const buf = await fs.readFile(filePath);
  return crypto.createHash('sha256').update(buf).digest('hex');
}

async function ensureDir(dir) {
  await fs.mkdir(dir, { recursive: true });
}

async function copyTree(src, dest) {
  await fs.mkdir(dest, { recursive: true });
  const entries = await fs.readdir(src, { withFileTypes: true });
  for (const entry of entries) {
    const from = path.join(src, entry.name);
    const to = path.join(dest, entry.name);
    if (entry.isDirectory()) await copyTree(from, to);
    else await fs.copyFile(from, to);
  }
}

function hostTarget() {
  return execSync('rustc -vV', { encoding: 'utf8' })
    .match(/^host: (.+)$/m)?.[1]
    ?.trim();
}

function hasZigbuild() {
  try {
    execSync('cargo zigbuild --help', { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}

function ensureRustTargets() {
  const host = hostTarget();
  const needed = new Set(
    DEFAULT_PLATFORMS.map((p) => p.target).filter((t) => t && t !== host),
  );
  for (const target of needed) {
    try {
      execSync(`rustup target add ${target}`, { stdio: 'pipe' });
      console.log(`  ✓ rust target ${target}`);
    } catch {
      console.log(`  ⊘ could not add rust target ${target}`);
    }
  }
}

function crossBuildMethod(target) {
  if (LINUX_TARGETS.has(target)) return hasZigbuild() ? 'zig' : null;
  if (process.platform !== 'win32' || !WINDOWS_CROSS_TARGETS.has(target)) return null;
  if (target === 'i686-pc-windows-msvc' && (!hasNasm() || !hasMsvcX86(winBuildEnv.vsPath))) return null;
  if (target === 'aarch64-pc-windows-msvc' && (!hasMsvcArm64(winBuildEnv.vsPath) || !hasClang(winBuildEnv.vsPath))) return null;
  return 'cargo';
}

function cargoBuild(target) {
  const flag = target ? `--target ${target}` : '';
  const cmd = `cargo build ${CARGO_ARGS} ${flag}`.trim();
  console.log(`  ${cmd}`);
  execWithMsvc(cmd, {
    cwd: repoRoot,
    target: target || 'x86_64-pc-windows-msvc',
    pathExtra: winBuildEnv.pathExtra,
  });
}

function cargoZigbuild(target) {
  console.log(`  cargo zigbuild ${CARGO_ARGS} --target ${target}`);
  execSync(`cargo zigbuild ${CARGO_ARGS} --target ${target}`, {
    cwd: repoRoot,
    stdio: 'inherit',
  });
}

function canCrossBuild(target) {
  if (MACOS_TARGETS.has(target)) return false;
  return crossBuildMethod(target) != null;
}

function skipReason(platform) {
  if (MACOS_TARGETS.has(platform.target)) {
    return 'macOS requires a Mac (Apple SDK not available on Windows)';
  }
  if (LINUX_TARGETS.has(platform.target) && !hasZigbuild()) {
    return 'install cargo-zigbuild + zig for Linux cross-compile';
  }
  if (platform.target === 'i686-pc-windows-msvc' && !hasNasm()) {
    return 'NASM install failed — retry or install manually from nasm.us';
  }
  if (platform.target === 'aarch64-pc-windows-msvc') {
    if (!hasMsvcArm64(winBuildEnv.vsPath)) return 'MSVC ARM64 tools install failed — run as Administrator';
    if (!hasClang(winBuildEnv.vsPath)) return 'LLVM clang install failed — run as Administrator';
  }
  if (platform.target === 'i686-pc-windows-msvc' && !hasMsvcX86(winBuildEnv.vsPath)) {
    return 'MSVC x86 tools install failed — run terminal as Administrator';
  }
  return 'unsupported cross-compile target';
}

function binPaths(target) {
  const base = target
    ? path.join(repoRoot, 'target', target, 'release')
    : path.join(repoRoot, 'target', 'release');
  const isWindows = target?.includes('windows') || (!target && process.platform === 'win32');
  const ext = isWindows ? '.exe' : '';
  return {
    niao: path.join(base, `niao${ext}`),
    nm: path.join(base, `nm${ext}`),
  };
}

async function zipDir(srcDir, outZip) {
  await fs.rm(outZip, { force: true });
  if (process.platform === 'win32') {
    const ps = `Compress-Archive -Path '${srcDir}\\*' -DestinationPath '${outZip}' -Force`;
    execSync(`powershell -NoProfile -Command "${ps}"`, { stdio: 'inherit' });
  } else {
    execSync(`cd "${srcDir}" && zip -r "${outZip}" .`, { stdio: 'inherit' });
  }
}

async function tarGzDir(srcDir, outTar) {
  await fs.rm(outTar, { force: true });
  await tar.c({ gzip: true, file: outTar, cwd: srcDir }, ['.']);
}

async function packageArchive(platform, bins) {
  const staging = path.join(releasesDir, `.staging-${platform.id}`);
  const binDir = path.join(staging, 'niao', 'bin');
  await fs.rm(staging, { recursive: true, force: true });
  await ensureDir(binDir);

  await fs.copyFile(bins.niao, path.join(binDir, path.basename(bins.niao)));
  await fs.copyFile(bins.nm, path.join(binDir, path.basename(bins.nm)));

  const libsSrc = path.join(repoRoot, 'niao_libs');
  try {
    await fs.access(libsSrc);
    await copyTree(libsSrc, path.join(staging, 'niao', 'niao_libs'));
  } catch {
    /* optional */
  }

  await fs.writeFile(
    path.join(staging, 'niao', 'README.txt'),
    `Niao ${version} — ${platform.label}\n\n` +
      `  bin/niao  — language runtime (VM / NFE)\n  bin/nm    — package manager\n\n` +
      `Install: use the one-click installer from the download page.\n`,
  );

  const ext = platform.ext || 'zip';
  const outName = `niao-${version}-${platform.id}.${ext}`;
  const outPath = path.join(releasesDir, outName);
  const packRoot = path.join(staging, 'niao');

  if (ext === 'zip') await zipDir(packRoot, outPath);
  else await tarGzDir(packRoot, outPath);

  await fs.rm(staging, { recursive: true, force: true });
  const stat = await fs.stat(outPath);
  return { outName, outPath, stat, shasum: await sha256File(outPath) };
}

async function buildWindowsInstaller() {
  const platform = DEFAULT_PLATFORMS.find((p) => p.id === 'windows-x64');
  const winDir = path.join(repoRoot, 'windows');
  const prepareScript = path.join(winDir, 'prepare-bundle.ps1');
  const installerDir = path.join(winDir, 'installer');

  console.log('  Building NiaoSetup.exe…');
  execSync(`powershell -NoProfile -ExecutionPolicy Bypass -File "${prepareScript}"`, {
    cwd: repoRoot,
    stdio: 'inherit',
  });
  execSync('cargo build --release', { cwd: installerDir, stdio: 'inherit' });

  const setupSrc = path.join(installerDir, 'target', 'release', 'NiaoSetup.exe');
  const outName = releaseInstallerFileName(version, 'windows-x64', platform);
  const outPath = path.join(releasesDir, outName);
  await fs.copyFile(setupSrc, outPath);

  const stat = await fs.stat(outPath);
  const shasum = await sha256File(outPath);
  console.log(`  ✓ ${outName} (${(stat.size / 1024 / 1024).toFixed(2)} MB)`);

  return {
    installer_url: `${config.filesUrl}/releases/${outName}`,
    installer_shasum: shasum,
    installer_size: stat.size,
  };
}

async function buildUnixInstaller(platform, archiveUrl) {
  const templatePath = path.join(pkgRoot, 'scripts/templates/install-unix.sh');
  let script = await fs.readFile(templatePath, 'utf8');
  script = script
    .replace(/\{\{VERSION\}\}/g, version)
    .replace(/\{\{LABEL\}\}/g, platform.label)
    .replace(/\{\{PLATFORM\}\}/g, platform.id)
    .replace(/\{\{ARCHIVE_URL\}\}/g, archiveUrl);

  const outName = releaseInstallerFileName(version, platform.id, platform);
  const outPath = path.join(releasesDir, outName);
  await fs.writeFile(outPath, script, { mode: 0o755 });

  const stat = await fs.stat(outPath);
  const shasum = await sha256File(outPath);
  console.log(`  ✓ ${outName} (${(stat.size / 1024).toFixed(1)} KB)`);

  return {
    installer_url: `${config.filesUrl}/releases/${outName}`,
    installer_shasum: shasum,
    installer_size: stat.size,
  };
}

async function packagePlatformVariant(platform, bins) {
  const { outName, stat, shasum } = await packageArchive(platform, bins);
  console.log(`  ✓ ${outName} (${(stat.size / 1024 / 1024).toFixed(2)} MB)`);

  const variant = {
    id: platform.id,
    label: platform.label,
    platform: platform.platform,
    arch: platform.arch,
    ext: platform.ext || 'zip',
    url: `${config.filesUrl}/releases/${outName}`,
    shasum,
    size: stat.size,
    status: 'active',
    installer_ext: platform.installer_ext || 'sh',
    installer_label: platform.installer_label || 'install.sh',
  };

  if (platform.id === 'windows-x64' && process.platform === 'win32') {
    const winInstaller = await buildWindowsInstaller();
    if (winInstaller) Object.assign(variant, winInstaller);
  } else if (platform.platform !== 'windows') {
    const unixInstaller = await buildUnixInstaller(platform, variant.url);
    Object.assign(variant, unixInstaller);
  }

  return variant;
}

async function buildLocalPlatform(platform) {
  const host = hostTarget();
  if (platform.target !== host) return null;

  try {
    cargoBuild(null);
  } catch {
    console.log(`  ⊘ ${platform.id} build failed`);
    return null;
  }

  const bins = binPaths(null);
  for (const p of [bins.niao, bins.nm]) {
    try {
      await fs.access(p);
    } catch {
      console.log(`  ⊘ ${platform.id} missing ${path.basename(p)}`);
      return null;
    }
  }

  return packagePlatformVariant(platform, bins);
}

async function buildCrossPlatform(platform) {
  const method = crossBuildMethod(platform.target);
  if (!method) {
    console.log(`  ⊘ ${platform.id} (${skipReason(platform)})`);
    return null;
  }

  try {
    if (method === 'zig') cargoZigbuild(platform.target);
    else cargoBuild(platform.target);
  } catch {
    console.log(`  ⊘ ${platform.id} cross-build failed (${skipReason(platform)})`);
    return null;
  }

  const bins = binPaths(platform.target);
  for (const p of [bins.niao, bins.nm]) {
    try {
      await fs.access(p);
    } catch {
      console.log(`  ⊘ ${platform.id} missing ${path.basename(p)}`);
      return null;
    }
  }

  return packagePlatformVariant(platform, bins);
}

async function variantFromDisk(platform) {
  const ext = platform.ext || 'zip';
  const archiveName = `niao-${version}-${platform.id}.${ext}`;
  const archivePath = path.join(releasesDir, archiveName);

  try {
    await fs.access(archivePath);
  } catch {
    return null;
  }

  const stat = await fs.stat(archivePath);
  const url = `${config.filesUrl}/releases/${archiveName}`;
  const variant = {
    id: platform.id,
    label: platform.label,
    platform: platform.platform,
    arch: platform.arch,
    ext,
    url,
    shasum: await sha256File(archivePath),
    size: stat.size,
    status: 'active',
    installer_ext: platform.installer_ext || 'sh',
    installer_label: platform.installer_label || 'install.sh',
  };

  if (platform.id === 'windows-x64') {
    const setupName = releaseInstallerFileName(version, platform.id, platform);
    const setupPath = path.join(releasesDir, setupName);
    try {
      const iStat = await fs.stat(setupPath);
      variant.installer_url = `${config.filesUrl}/releases/${setupName}`;
      variant.installer_shasum = await sha256File(setupPath);
      variant.installer_size = iStat.size;
    } catch {
      /* no installer */
    }
  } else {
    const inst = await buildUnixInstaller(platform, url);
    Object.assign(variant, inst);
  }

  return variant;
}

async function buildToolchainTarball() {
  const staging = path.join(releasesDir, `.staging-toolchain-${version}`);
  const out = path.join(releasesDir, `niao-${version}-toolchain.tgz`);

  await fs.rm(staging, { recursive: true, force: true });
  await ensureDir(staging);
  await fs.copyFile(path.join(repoRoot, 'Cargo.toml'), path.join(staging, 'Cargo.toml'));
  await copyTree(path.join(repoRoot, 'niao_libs'), path.join(staging, 'niao_libs'));

  await fs.rm(out, { force: true });
  await tar.c({ gzip: true, file: out, cwd: staging }, ['Cargo.toml', 'niao_libs']);
  await fs.rm(staging, { recursive: true, force: true });

  const stat = await fs.stat(out);
  console.log(`  ✓ toolchain: ${path.basename(out)} (${(stat.size / 1024).toFixed(1)} KB)`);
}

async function writeManifest(variants) {
  const manifest = {
    version,
    status: 'active',
    is_latest: true,
    updated_at: new Date().toISOString(),
    components: ['niao', 'nm', 'vm', 'nfe'],
    description: 'Niao language runtime — VM, NFE engine, and nm package manager',
    source: {
      github: config.githubRepo,
      tag: `v${version}`,
      zip_url: `${config.githubRepo}/archive/refs/tags/v${version}.zip`,
      tarball_url: `${config.githubRepo}/archive/refs/tags/v${version}.tar.gz`,
    },
    toolchain: {
      url: `${config.filesUrl}/releases/niao-${version}-toolchain.tgz`,
      note: 'For nm install --global only',
    },
    variants,
  };
  await fs.writeFile(
    path.join(releasesDir, 'manifest.json'),
    JSON.stringify(manifest, null, 2) + '\n',
  );
}

function runSeed() {
  if (SKIP_SEED) {
    console.log('\n⊘ Skipping seed (NIAO_SKIP_SEED=1)');
    return;
  }
  console.log('\nSeeding package catalog…');
  const r = spawnSync(process.execPath, ['src/scripts/seed.js'], {
    cwd: pkgRoot,
    stdio: 'inherit',
  });
  if (r.status !== 0) throw new Error('seed failed');
}

async function uploadFtp() {
  if (SKIP_FTP) {
    console.log('\n⊘ Skipping FTP (NIAO_SKIP_FTP=1)');
    return;
  }
  if (!ftpConfigured()) {
    throw new Error('FTP not configured — set FTP_HOST, FTP_USER, FTP_PASSWORD in .env');
  }

  console.log('\nUploading to FTP…');
  const test = await testFtpConnection();
  console.log(`  ✓ Connected to ${test.host} (${config.filesUrl})`);

  const progress = createConsoleFtpProgress({ label: 'Release upload' });
  try {
    const result = await syncToFtp({ onProgress: progress.onProgress });
    progress.done(result);
  } catch (err) {
    progress.fail(err);
    throw err;
  }
}

async function main() {
  console.log(`\n══ Niao ${version} release → FTP ══\n`);

  await ensureDir(releasesDir);

  if (process.platform === 'win32') {
    console.log('[0/5] Windows build tools…');
    winBuildEnv = await ensureWindowsBuildTools();
    console.log('');
  }

  console.log(`[${process.platform === 'win32' ? '1' : '0'}/5] Rust targets…`);
  ensureRustTargets();

  console.log(`\n[${process.platform === 'win32' ? '2' : '1'}/5] Build binaries…`);
  const host = hostTarget();
  const localBuilt = new Map();
  for (const platform of DEFAULT_PLATFORMS) {
    let v = null;
    if (platform.target === host) {
      v = await buildLocalPlatform(platform);
    } else {
      v = await buildCrossPlatform(platform);
    }
    if (v) localBuilt.set(platform.id, v);
  }

  console.log(`\n[${process.platform === 'win32' ? '3' : '2'}/5] Package + manifest…`);
  const variants = [];
  for (const platform of DEFAULT_PLATFORMS) {
    const v = localBuilt.get(platform.id) || (await variantFromDisk(platform));
    if (v) {
      variants.push(v);
      console.log(`  ✓ ${platform.id}`);
    } else {
      console.log(`  ⊘ ${platform.id} (${skipReason(platform)})`);
    }
  }

  if (!variants.length) {
    throw new Error('No release artifacts built');
  }

  await buildToolchainTarball();
  await writeManifest(variants);
  console.log(`\n  ${variants.length}/${DEFAULT_PLATFORMS.length} platforms ready`);

  console.log(`\n[${process.platform === 'win32' ? '4' : '3'}/5] Seed catalog…`);
  runSeed();

  console.log(`\n[${process.platform === 'win32' ? '5' : '4'}/5] FTP upload…`);
  await uploadFtp();

  console.log('\n══ Done — live on CDN ══\n');
  for (const v of variants) {
    console.log(`  ${v.label}: ${v.installer_url || v.url}`);
  }
  console.log(`\n  ${config.filesUrl}/releases/manifest.json\n`);
}

main().catch((err) => {
  console.error(`\n✗ ${err.message || err}\n`);
  process.exit(1);
});
