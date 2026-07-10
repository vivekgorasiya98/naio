/** Package a single platform from existing target/ binaries. */
import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import { execSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import dotenv from 'dotenv';
import { config } from '../config.js';
import { DEFAULT_PLATFORMS } from '../db/registry-db.js';

dotenv.config();

const id = process.argv[2];
if (!id) {
  console.error('Usage: node package-one.js <platform-id>');
  process.exit(1);
}

const platform = DEFAULT_PLATFORMS.find((p) => p.id === id);
if (!platform?.target) {
  console.error(`Unknown platform: ${id}`);
  process.exit(1);
}

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const pkgRoot = path.resolve(__dirname, '../..');
const repoRoot = path.resolve(pkgRoot, '..');
const version = config.niaoVersion;
const releasesDir = path.join(config.dataDir, 'releases');
const base = path.join(repoRoot, 'target', platform.target, 'release');
const ext = platform.ext || 'zip';
const out = path.join(releasesDir, `niao-${version}-${platform.id}.${ext}`);
const staging = path.join(releasesDir, `.staging-${platform.id}`);
const binDir = path.join(staging, 'niao', 'bin');

await fs.mkdir(binDir, { recursive: true });
await fs.copyFile(path.join(base, 'niao.exe'), path.join(binDir, 'niao.exe'));
await fs.copyFile(path.join(base, 'nm.exe'), path.join(binDir, 'nm.exe'));
try {
  await fs.cp(path.join(repoRoot, 'niao_libs'), path.join(staging, 'niao', 'niao_libs'), { recursive: true });
} catch {
  /* optional */
}
await fs.writeFile(
  path.join(staging, 'niao', 'README.txt'),
  `Niao ${version} — ${platform.label}\n`,
);
const packRoot = path.join(staging, 'niao');
await fs.rm(out, { force: true });
const ps = `Compress-Archive -Path '${packRoot}\\*' -DestinationPath '${out}' -Force`;
execSync(`powershell -NoProfile -Command "${ps}"`, { stdio: 'inherit' });
await fs.rm(staging, { recursive: true, force: true });

const buf = await fs.readFile(out);
const shasum = crypto.createHash('sha256').update(buf).digest('hex');
const stat = await fs.stat(out);
console.log(`✓ ${path.basename(out)} (${(stat.size / 1024 / 1024).toFixed(2)} MB)`);
console.log(`  shasum: ${shasum}`);
