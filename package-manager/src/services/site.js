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

export async function buildSitePayload() {
  const version = config.niaoVersion;

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
    title: 'NIAO',
    tagline: 'Download the NIAO toolchain: niao runtime and nm package manager for every platform.',
    version: latestRelease?.version || version,
    registry: config.apiUrl,
    files: config.filesUrl,
    admin: `${config.apiUrl}/admin/`,
    mongo: isMongoPrimary(),
    links: {
      website: config.websiteUrl,
      github: config.githubRepo,
      instagram: config.social.instagram,
      linkedin: config.social.linkedin,
      x: config.social.x,
    },
    docs: {
      catalog: `${config.apiUrl}/v1/catalog`,
      packages: `${config.apiUrl}/v1/packages`,
      releases: `${config.apiUrl}/v1/releases/niao`,
      health: `${config.apiUrl}/health`,
    },
    github: config.githubRepo,
    platforms: DEFAULT_PLATFORMS.map((p) => ({
      id: p.id,
      label: p.label,
      platform: p.platform,
      arch: p.arch,
      ext: p.ext,
      installer_label: p.installer_label,
      installer_ext: p.installer_ext,
    })),
    releases,
    latestRelease: latestRelease?.version || version,
    install: {
      global: 'nm install --global',
      lib: 'nm install <library>',
      libVersion: 'nm install <library>@<version>',
      verify: 'nm version',
      examples: ['nm install --global', 'nm install nllm', 'nm install nrag@0.2.2'],
    },
    nm: {
      registry: config.apiUrl,
      home: '~/.niao',
      libsDir: '~/.niao/niao_libs',
      commandGroups: [
        {
          title: 'Install',
          commands: [
            { cmd: 'nm install --global', desc: 'Install the full standard library to ~/.niao' },
            { cmd: 'nm install <library>', desc: 'Install one or more libraries by name' },
            { cmd: 'nm install <library>@<version>', desc: 'Pin a specific package version' },
            { cmd: 'nm install', desc: 'Install dependencies listed in package.json' },
            { cmd: 'nm install --venv', desc: 'Install into the project .niao/ virtual env' },
          ],
        },
        {
          title: 'Browse',
          commands: [
            { cmd: 'nm list', desc: 'List all libraries with install status' },
            { cmd: 'nm list --installed', desc: 'Show only installed libraries' },
            { cmd: 'nm search <query>', desc: 'Search the catalog by name or keyword' },
            { cmd: 'nm info <library>', desc: 'Show details for one library' },
          ],
        },
        {
          title: 'Manage',
          commands: [
            { cmd: 'nm update', desc: 'Update all installed libraries' },
            { cmd: 'nm update <library>', desc: 'Update specific libraries' },
            { cmd: 'nm uninstall <library>', desc: 'Remove installed libraries' },
            { cmd: 'nm venv', desc: 'Initialize a project virtual environment' },
          ],
        },
        {
          title: 'Toolchain',
          commands: [
            { cmd: 'nm version', desc: 'Show nm and installed library versions' },
            { cmd: 'nm home', desc: 'Print the NIAO home directory path' },
          ],
        },
      ],
      featured: [
        { name: 'nllm', desc: 'LLM inference bindings', remote: true },
        { name: 'nrag', desc: 'Retrieval-augmented generation', remote: true },
        { name: 'nos', desc: 'Object storage helpers', remote: false },
        { name: 'nmongo', desc: 'MongoDB client', remote: false },
        { name: 'npg', desc: 'PostgreSQL client', remote: false },
        { name: 'ahiru', desc: 'HTTP server framework', remote: false },
      ],
    },
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
