/**
 * Write releases/manifest.json from built artifacts on disk (CI helper).
 */
import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import dotenv from 'dotenv';
import { config } from '../config.js';
import { DEFAULT_PLATFORMS, releaseInstallerFileName, releaseInstallerUrl } from '../db/registry-db.js';

dotenv.config();

const releasesDir = path.join(config.dataDir, 'releases');
const version = config.niaoVersion;

async function sha256File(filePath) {
  const buf = await fs.readFile(filePath);
  return crypto.createHash('sha256').update(buf).digest('hex');
}

async function main() {
  const variants = [];
  for (const p of DEFAULT_PLATFORMS) {
    const ext = p.ext || 'zip';
    const file = path.join(releasesDir, `niao-${version}-${p.id}.${ext}`);
    try {
      const stat = await fs.stat(file);
      const variant = {
        id: p.id,
        label: p.label,
        platform: p.platform,
        arch: p.arch,
        ext,
        url: `${config.filesUrl}/releases/niao-${version}-${p.id}.${ext}`,
        shasum: await sha256File(file),
        size: stat.size,
        status: 'active',
        installer_ext: p.installer_ext || 'sh',
        installer_label: p.installer_label || 'install.sh',
        installer_url: releaseInstallerUrl(version, p.id, p),
      };

      const installerFile = path.join(releasesDir, releaseInstallerFileName(version, p.id, p));
      try {
        const iStat = await fs.stat(installerFile);
        variant.installer_shasum = await sha256File(installerFile);
        variant.installer_size = iStat.size;
        console.log(`  ✓ ${p.id} (+ installer)`);
      } catch {
        console.log(`  ✓ ${p.id} (archive only)`);
      }

      variants.push(variant);
    } catch {
      console.log(`  ⊘ ${p.id} (not built)`);
    }
  }

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
    variants,
  };

  await fs.mkdir(releasesDir, { recursive: true });
  await fs.writeFile(
    path.join(releasesDir, 'manifest.json'),
    JSON.stringify(manifest, null, 2) + '\n',
  );
  console.log(`manifest: ${variants.length} variants`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
