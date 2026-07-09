/**
 * Full Niao release pipeline — build all platforms, seed catalog, upload FTP.
 *
 * Windows: built locally (+ NiaoSetup.exe)
 * Linux / macOS: built via GitHub Actions (set GITHUB_TOKEN in .env)
 *
 * Usage (from package-manager/):
 *   npm run release
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
import { syncToFtp, testFtpConnection, ftpConfigured } from '../services/ftp.js';
import {
  githubCiConfigured,
  triggerReleaseWorkflow,
  waitForLatestRun,
  downloadRunArtifacts,
  copyCiArtifactsToReleases,
} from './github-ci.js';

dotenv.config();

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const pkgRoot = path.resolve(__dirname, '../..');
const repoRoot = path.resolve(pkgRoot, '..');
const version = config.niaoVersion;
const releasesDir = path.join(config.dataDir, 'releases');
const ciArtifactsDir = path.join(releasesDir, '.ci-artifacts');

const CARGO_ARGS = '--release --no-default-features -p niao_cli -p niao_nm';
const BUILD_ALL = process.env.NIAO_BUILD_ALL !== '0';
const SKIP_CI = process.env.NIAO_SKIP_CI === '1';
const SKIP_FTP = process.env.NIAO_SKIP_FTP === '1';
const SKIP_SEED = process.env.NIAO_SKIP_SEED === '1';

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

function installedTargets() {
  const out = execSync('rustup target list --installed', { encoding: 'utf8' });
  return new Set(out.split(/\r?\n/).map((l) => l.trim()).filter(Boolean));
}

function ensureRustTargets() {
  console.log('Ensuring Rust targets…');
  const installed = installedTargets();
  for (const p of DEFAULT_PLATFORMS) {
    if (installed.has(p.target)) continue;
    try {
      console.log(`  rustup target add ${p.target}`);
      execSync(`rustup target add ${p.target}`, { stdio: 'inherit' });
    } catch {
      console.log(`  ⊘ could not add ${p.target}`);
    }
  }
}

function cargoBuild(target) {
  const flag = target ? `--target ${target}` : '';
  console.log(`  cargo build ${CARGO_ARGS} ${flag}`.trim());
  execSync(`cargo build ${CARGO_ARGS} ${flag}`.trim(), {
    cwd: repoRoot,
    stdio: 'inherit',
  });
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

async function buildLocalPlatform(platform) {
  const host = hostTarget();
  if (platform.target !== host) {
    return null;
  }

  try {
    cargoBuild(null);
  } catch {
    console.log(`  ⊘ ${platform.id} local build failed`);
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
  } else if (platform.id !== 'windows-x64') {
    const unixInstaller = await buildUnixInstaller(platform, variant.url);
    Object.assign(variant, unixInstaller);
  }

  return variant;
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

async function fetchCiBuilds() {
  if (SKIP_CI || !githubCiConfigured()) {
    if (!SKIP_CI && !githubCiConfigured()) {
      console.log('\n⊘ GITHUB_TOKEN not set — Linux/macOS builds need GitHub Actions');
      console.log('  Add GITHUB_TOKEN to .env (PAT with repo + actions:read/write)');
      console.log('  Or place existing artifacts in data/releases/\n');
    }
    return;
  }

  console.log('\nFetching Linux + macOS builds from GitHub Actions…');
  try {
    const ref = process.env.GITHUB_REF || 'main';
    const triggered = await triggerReleaseWorkflow(config.githubRepo, ref);
    if (!triggered) return;

    const run = await waitForLatestRun(triggered.ownerRepo, triggered.workflowId, {
      minRunId: triggered.minRunId,
    });
    console.log(`\n  ✓ CI run ${run.id} completed`);

    await fs.rm(ciArtifactsDir, { recursive: true, force: true });
    await downloadRunArtifacts(run.id, triggered.ownerRepo, ciArtifactsDir);
    await copyCiArtifactsToReleases(ciArtifactsDir, releasesDir, version);
  } catch (err) {
    console.log(`\n⊘ CI build skipped: ${err.message}`);
    console.log('  Windows release will still publish.');
    console.log('  To enable all platforms: push repo to GitHub with .github/workflows/niao-release.yml\n');
  }
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
  const manifestPath = path.join(releasesDir, 'manifest.json');
  await fs.writeFile(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  return manifest;
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
    console.log('\n⊘ FTP not configured — set FTP_* in .env');
    return;
  }

  console.log('\nUploading to FTP…');
  const test = await testFtpConnection();
  console.log(`  ✓ Connected to ${test.host}`);

  const result = await syncToFtp();
  console.log(`  ✓ Uploaded ${result.uploaded} files → ${result.remote}`);
}

async function main() {
  console.log(`\n══ Niao ${version} release ══\n`);
  await ensureDir(releasesDir);

  if (BUILD_ALL) ensureRustTargets();

  console.log('\n[1/5] Local platform build…');
  const host = hostTarget();
  const localBuilt = new Map();
  for (const platform of DEFAULT_PLATFORMS) {
    if (platform.target === host) {
      const v = await buildLocalPlatform(platform);
      if (v) localBuilt.set(platform.id, v);
    }
  }

  console.log('\n[2/5] Remote platform builds (CI)…');
  const needCi = DEFAULT_PLATFORMS.some((p) => p.target !== host && !localBuilt.has(p.id));
  if (needCi) await fetchCiBuilds();

  console.log('\n[3/5] Collecting release artifacts…');
  const variants = [];
  for (const platform of DEFAULT_PLATFORMS) {
    const v = localBuilt.get(platform.id) || (await variantFromDisk(platform));
    if (v) {
      variants.push(v);
      console.log(`  ✓ ${platform.id}`);
    } else {
      console.log(`  ⊘ ${platform.id} (not built)`);
    }
  }

  if (!variants.length) {
    throw new Error('No platform artifacts — build failed or missing CI artifacts');
  }

  await buildToolchainTarball();
  await writeManifest(variants);

  console.log(`\n  ${variants.length}/${DEFAULT_PLATFORMS.length} platforms in manifest`);

  console.log('\n[4/5] Seed catalog…');
  runSeed();

  console.log('\n[5/5] FTP upload…');
  await uploadFtp();

  console.log('\n══ Release complete ══\n');
  for (const v of variants) {
    console.log(`  ${v.label}: ${v.installer_url || v.url}`);
  }
  console.log(`\n  manifest → ${path.join(releasesDir, 'manifest.json')}`);
  console.log(`  site     → ${config.filesUrl}\n`);
}

main().catch((err) => {
  console.error(`\n✗ ${err.message || err}\n`);
  process.exit(1);
});
