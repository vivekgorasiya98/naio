import path from 'node:path';
import fs from 'node:fs/promises';
import { config } from '../config.js';
import { getLatestReleaseFromDb, listReleasesFromDb, isMongoPrimary, DEFAULT_PLATFORMS, releaseVariantUrl, releaseInstallerUrl } from '../db/registry-db.js';

async function loadManifestFromDisk() {
  try {
    const raw = await fs.readFile(path.join(config.dataDir, 'releases', 'manifest.json'), 'utf8');
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

function releaseFromManifest(manifest) {
  if (!manifest) return null;
  return {
    version: manifest.version,
    status: manifest.status || 'active',
    is_latest: manifest.is_latest !== false,
    changelog: manifest.changelog || '',
    components: manifest.components || ['niao', 'nm', 'vm', 'nfe'],
    source: {
      zip_url: manifest.source?.zip_url,
      tarball_url: manifest.source?.tarball_url,
      hosted_url: manifest.toolchain?.url || null,
      github: manifest.source?.github || config.githubRepo,
    },
    variants: (manifest.variants || []).map((v) => ({
      ...v,
      url: v.url || releaseVariantUrl(manifest.version, v.id, v.ext || 'zip'),
    })),
  };
}

function releaseFromEnv(version) {
  const envMap = {
    'windows-x64': config.releaseBinaries.windows,
    'linux-x64': config.releaseBinaries.linux,
    'linux-arm64': config.releaseBinaries.linux_arm64,
    'macos-x64': config.releaseBinaries.macos,
    'macos-arm64': config.releaseBinaries.macos_arm64,
  };
  const tag = `v${version}`;
  const github = config.githubRepo;
  return {
    version,
    status: 'active',
    is_latest: true,
    components: ['niao', 'nm', 'vm', 'nfe'],
    source: {
      zip_url: `${github}/archive/refs/tags/${tag}.zip`,
      tarball_url: `${github}/archive/refs/tags/${tag}.tar.gz`,
      hosted_url: `${config.filesUrl}/releases/niao-${version}-toolchain.tgz`,
      github,
    },
    variants: DEFAULT_PLATFORMS.map((p) => ({
      id: p.id,
      label: p.label,
      platform: p.platform,
      arch: p.arch,
      ext: p.ext || 'zip',
      status: 'active',
      url: envMap[p.id] || releaseVariantUrl(version, p.id, p.ext || 'zip'),
      installer_ext: p.installer_ext || 'sh',
      installer_label: p.installer_label || 'install.sh',
      installer_url: releaseInstallerUrl(version, p.id, p),
    })),
  };
}

export async function buildSitePayload(catalog = null) {
  const version = catalog?.niao_version || config.niaoVersion;

  let releases = [];
  let latestRelease = null;

  if (isMongoPrimary()) {
    releases = await listReleasesFromDb();
    latestRelease = await getLatestReleaseFromDb();
  }

  if (!releases.length) {
    const manifest = await loadManifestFromDisk();
    const fromManifest = releaseFromManifest(manifest);
    if (fromManifest) {
      releases = [fromManifest];
      latestRelease = fromManifest;
    } else {
      const fallback = releaseFromEnv(version);
      releases = [fallback];
      latestRelease = fallback;
    }
  }

  return {
    name: 'niao',
    title: 'Niao',
    tagline: 'Fast programming language — VM runtime, NFE engine, and nm package manager',
    version: latestRelease?.version || version,
    registry: config.apiUrl,
    files: config.filesUrl,
    admin: `${config.apiUrl}/admin/`,
    mongo: isMongoPrimary(),
    docs: {
      catalog: `${config.apiUrl}/v1/catalog`,
      packages: `${config.apiUrl}/v1/packages`,
      releases: `${config.apiUrl}/v1/releases/niao`,
      health: `${config.apiUrl}/health`,
    },
    github: config.githubRepo,
    releases,
    latestRelease: latestRelease?.version || version,
    install: {
      global: 'nm install --global',
      lib: 'nm install <library>',
      libVersion: 'nm install <library>@<version>',
      examples: ['nm install nllm', 'nm install nrag@0.2.2', 'nm install nmongo'],
    },
    catalog: catalog
      ? {
          niao_version: catalog.niao_version || version,
          remote_libs: catalog.remote_libs || [],
          libs: catalog.libs || {},
        }
      : null,
  };
}

export async function hostedSourcePath(version = config.niaoVersion) {
  return path.join(config.dataDir, 'releases', `niao-${version}-toolchain.tgz`);
}

export async function hostedSourceExists(version = config.niaoVersion) {
  try {
    await fs.access(await hostedSourcePath(version));
    return true;
  } catch {
    return false;
  }
}
