import { getDb, mongoEnabled, isDbReady } from './mongo.js';
import { config } from '../config.js';

export const PKG_STATUS = {
  ACTIVE: 'active',
  DISCONTINUED: 'discontinued',
  HIDDEN: 'hidden',
};

export const VERSION_STATUS = {
  ACTIVE: 'active',
  DISCONTINUED: 'discontinued',
  YANKED: 'yanked',
};

export const RELEASE_STATUS = {
  DRAFT: 'draft',
  ACTIVE: 'active',
  DISCONTINUED: 'discontinued',
};

export const DEFAULT_PLATFORMS = [
  { id: 'windows-x64', label: 'Windows x64', platform: 'windows', arch: 'x64', ext: 'zip', target: 'x86_64-pc-windows-msvc', installer_ext: 'exe', installer_kind: 'setup', installer_label: 'NiaoSetup.exe' },
  { id: 'linux-x64', label: 'Linux x64', platform: 'linux', arch: 'x64', ext: 'tar.gz', target: 'x86_64-unknown-linux-gnu', installer_ext: 'sh', installer_kind: 'install', installer_label: 'install.sh' },
  { id: 'linux-arm64', label: 'Linux ARM64', platform: 'linux', arch: 'arm64', ext: 'tar.gz', target: 'aarch64-unknown-linux-gnu', installer_ext: 'sh', installer_kind: 'install', installer_label: 'install.sh' },
  { id: 'macos-x64', label: 'macOS Intel', platform: 'macos', arch: 'x64', ext: 'tar.gz', target: 'x86_64-apple-darwin', installer_ext: 'sh', installer_kind: 'install', installer_label: 'install.sh' },
  { id: 'macos-arm64', label: 'macOS Apple Silicon', platform: 'macos', arch: 'arm64', ext: 'tar.gz', target: 'aarch64-apple-darwin', installer_ext: 'sh', installer_kind: 'install', installer_label: 'install.sh' },
];

function now() {
  return new Date();
}

export function compareVersions(a, b) {
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

export function tarballPublicUrl(name, version) {
  const base = config.filesUrl.replace(/\/$/, '');
  return `${base}/v1/packages/${encodeURIComponent(name)}/${encodeURIComponent(version)}/tarball`;
}

export function releaseVariantUrl(version, variantId, ext = 'zip') {
  const base = config.filesUrl.replace(/\/$/, '');
  return `${base}/releases/niao-${version}-${variantId}.${ext}`;
}

export function releaseVariantFtpPath(version, variantId, ext = 'zip') {
  return `releases/niao-${version}-${variantId}.${ext}`;
}

export function releaseInstallerFileName(version, variantId, platform = {}) {
  const kind = platform.installer_kind || 'install';
  const ext = platform.installer_ext || 'sh';
  return `niao-${version}-${variantId}-${kind}.${ext}`;
}

export function releaseInstallerUrl(version, variantId, platform = {}) {
  const base = config.filesUrl.replace(/\/$/, '');
  return `${base}/releases/${releaseInstallerFileName(version, variantId, platform)}`;
}

export function releaseInstallerFtpPath(version, variantId, platform = {}) {
  return `releases/${releaseInstallerFileName(version, variantId, platform)}`;
}

function pickLatestVersion(versions, onlyActive = true) {
  const filtered = onlyActive
    ? versions.filter((v) => v.status === VERSION_STATUS.ACTIVE)
    : versions.filter((v) => v.status !== VERSION_STATUS.YANKED);
  if (!filtered.length) return null;
  return filtered.map((v) => v.version).sort(compareVersions).at(-1);
}

function variantTemplate(version) {
  return DEFAULT_PLATFORMS.map((p) => ({
    id: p.id,
    label: p.label,
    platform: p.platform,
    arch: p.arch,
    ext: p.ext || 'zip',
    installer_ext: p.installer_ext || 'sh',
    installer_label: p.installer_label || 'install.sh',
    status: VERSION_STATUS.ACTIVE,
    url: releaseVariantUrl(version, p.id, p.ext || 'zip'),
    ftp_path: releaseVariantFtpPath(version, p.id, p.ext || 'zip'),
    installer_url: releaseInstallerUrl(version, p.id, p),
    installer_ftp_path: releaseInstallerFtpPath(version, p.id, p),
    shasum: '',
    size: 0,
    installer_shasum: '',
    installer_size: 0,
  }));
}

export async function upsertPackageDoc(pkg) {
  const db = await getDb();
  if (!db) return null;
  const ts = now();
  await db.collection('packages').updateOne(
    { name: pkg.name },
    {
      $set: {
        name: pkg.name,
        kind: pkg.kind || 'native',
        description: pkg.description || '',
        import_paths: pkg.import_paths || [],
        builtin_count: pkg.builtin_count || 0,
        remote: pkg.remote === true,
        status: pkg.status || PKG_STATUS.ACTIVE,
        latest_version: pkg.latest_version || pkg.version,
        updated_at: ts,
      },
      $setOnInsert: { created_at: ts },
    },
    { upsert: true },
  );
}

export async function upsertVersionDoc({
  name,
  version,
  packageJson,
  libJson,
  dist,
  status = VERSION_STATUS.ACTIVE,
}) {
  const db = await getDb();
  if (!db) return null;
  const ts = now();
  const doc = {
    package_name: name,
    version,
    status,
    package_json: packageJson,
    lib_json: libJson,
    dist: {
      tarball_path: `tarballs/${name}-${version}.tgz`,
      tarball_url: dist?.tarball_url || tarballPublicUrl(name, version),
      shasum: dist?.shasum || dist?.sha256 || '',
      size: dist?.size || 0,
    },
    updated_at: ts,
  };
  await db.collection('package_versions').updateOne(
    { package_name: name, version },
    { $set: doc, $setOnInsert: { published_at: ts } },
    { upsert: true },
  );
  await refreshPackageLatest(name);
}

async function refreshPackageLatest(name) {
  const db = await getDb();
  if (!db) return;
  const versions = await db
    .collection('package_versions')
    .find({ package_name: name, status: { $ne: VERSION_STATUS.YANKED } })
    .toArray();
  const latest = pickLatestVersion(versions, true) || pickLatestVersion(versions, false);
  if (latest) {
    await db.collection('packages').updateOne(
      { name },
      { $set: { latest_version: latest, updated_at: now() } },
    );
  }
}

export async function buildCatalogFromDb({ includeHidden = false } = {}) {
  const db = await getDb();
  if (!db) return null;

  const pkgFilter = includeHidden ? {} : { status: { $ne: PKG_STATUS.HIDDEN } };
  const packages = await db.collection('packages').find(pkgFilter).sort({ name: 1 }).toArray();

  const libs = {};
  const remote = [];

  for (const pkg of packages) {
    const versionDocs = await db
      .collection('package_versions')
      .find({
        package_name: pkg.name,
        status: { $ne: VERSION_STATUS.YANKED },
        ...(includeHidden ? {} : {}),
      })
      .toArray();

    const publicVersions = versionDocs
      .filter((v) => v.status !== VERSION_STATUS.YANKED)
      .sort((a, b) => compareVersions(a.version, b.version));

    const installableVersions = publicVersions
      .filter((v) => v.status === VERSION_STATUS.ACTIVE)
      .map((v) => v.version);

    const allVersions = publicVersions.map((v) => v.version);
    const latest =
      pkg.status === PKG_STATUS.DISCONTINUED
        ? allVersions.at(-1)
        : pickLatestVersion(publicVersions, true) || allVersions.at(-1);

    if (!latest) continue;

    libs[pkg.name] = {
      name: pkg.name,
      version: latest,
      kind: pkg.kind || 'native',
      description: pkg.description || '',
      import_paths: pkg.import_paths || [],
      builtin_count: pkg.builtin_count || 0,
      versions: allVersions,
      installable_versions: installableVersions,
      version_details: publicVersions.map((v) => ({
        version: v.version,
        status: v.status,
        dist: v.dist,
      })),
      status: pkg.status,
      remote: pkg.remote === true,
    };
    if (pkg.remote) remote.push(pkg.name);
  }

  const latestRelease = await getLatestReleaseFromDb();

  return {
    niao_version: latestRelease?.version || config.niaoVersion,
    description: 'Niao online package registry',
    updated_at: String(Date.now()),
    remote_libs: remote,
    libs,
  };
}

export async function listPackagesFromDb({ admin = false } = {}) {
  const db = await getDb();
  if (!db) return null;
  const filter = admin ? {} : { status: { $ne: PKG_STATUS.HIDDEN } };
  const rows = await db.collection('packages').find(filter).sort({ name: 1 }).toArray();
  return rows.map((p) => p.name);
}

export async function getPackageFromDb(name, { admin = false } = {}) {
  const db = await getDb();
  if (!db) return null;
  const pkg = await db.collection('packages').findOne({ name });
  if (!pkg) return null;
  if (!admin && pkg.status === PKG_STATUS.HIDDEN) return null;

  const versions = await db
    .collection('package_versions')
    .find({ package_name: name })
    .sort({ version: 1 })
    .toArray();

  const visible = admin
    ? versions
    : versions.filter((v) => v.status !== VERSION_STATUS.YANKED);

  const latest =
    pkg.status === PKG_STATUS.DISCONTINUED
      ? visible.map((v) => v.version).sort(compareVersions).at(-1)
      : pickLatestVersion(visible, !admin) || visible.map((v) => v.version).sort(compareVersions).at(-1);

  const activeVersions = visible
    .filter((v) => v.status === VERSION_STATUS.ACTIVE)
    .map((v) => v.version)
    .sort(compareVersions);
  const allVersions = visible.map((v) => v.version).sort(compareVersions);

  return {
    name: pkg.name,
    version: latest,
    kind: pkg.kind,
    description: pkg.description,
    import_paths: pkg.import_paths,
    builtin_count: pkg.builtin_count,
    remote: pkg.remote,
    status: pkg.status,
    versions: admin ? allVersions : activeVersions,
    all_versions: allVersions,
    installable_versions: activeVersions,
    version_details: visible.map((v) => ({
      version: v.version,
      status: v.status,
      dist: v.dist,
      published_at: v.published_at,
    })),
    latest,
  };
}

export async function getVersionFromDb(name, version, { allowYanked = false } = {}) {
  const db = await getDb();
  if (!db) return null;

  const pkg = await db.collection('packages').findOne({ name });
  if (!pkg || pkg.status === PKG_STATUS.HIDDEN) return null;

  const ver = await db.collection('package_versions').findOne({ package_name: name, version });
  if (!ver) return null;
  if (!allowYanked && ver.status === VERSION_STATUS.YANKED) return null;

  return {
    name,
    version,
    package: ver.package_json,
    lib: ver.lib_json,
    status: ver.status,
    dist: ver.dist,
    published_at: ver.published_at,
  };
}

export async function setPackageStatus(name, status) {
  const db = await getDb();
  if (!db) throw new Error('MongoDB not configured');
  await db.collection('packages').updateOne({ name }, { $set: { status, updated_at: now() } });
}

export async function setVersionStatus(name, version, status) {
  const db = await getDb();
  if (!db) throw new Error('MongoDB not configured');
  await db.collection('package_versions').updateOne(
    { package_name: name, version },
    { $set: { status, updated_at: now() } },
  );
  await refreshPackageLatest(name);
}

export async function removeVersionFromDb(name, version) {
  const db = await getDb();
  if (!db) throw new Error('MongoDB not configured');
  await db.collection('package_versions').deleteOne({ package_name: name, version });
  const remaining = await db.collection('package_versions').countDocuments({ package_name: name });
  if (remaining === 0) {
    await db.collection('packages').deleteOne({ name });
  } else {
    await refreshPackageLatest(name);
  }
}

export async function listReleasesFromDb({ admin = false } = {}) {
  const db = await getDb();
  if (!db) return [];
  const filter = admin ? {} : { status: { $in: [RELEASE_STATUS.ACTIVE, RELEASE_STATUS.DISCONTINUED] } };
  const rows = await db.collection('niao_releases').find(filter).sort({ version: -1 }).toArray();
  return rows.map(normalizeRelease);
}

export async function getReleaseFromDb(version, { admin = false } = {}) {
  const db = await getDb();
  if (!db) return null;
  const row = await db.collection('niao_releases').findOne({ version });
  if (!row) return null;
  if (!admin && row.status === RELEASE_STATUS.DRAFT) return null;
  return normalizeRelease(row);
}

export async function getLatestReleaseFromDb() {
  const db = await getDb();
  if (!db) return null;
  let row = await db.collection('niao_releases').findOne({ is_latest: true, status: RELEASE_STATUS.ACTIVE });
  if (!row) {
    const rows = await db
      .collection('niao_releases')
      .find({ status: RELEASE_STATUS.ACTIVE })
      .toArray();
    if (!rows.length) return null;
    row = rows.sort((a, b) => compareVersions(a.version, b.version)).at(-1);
  }
  return normalizeRelease(row);
}

function normalizeRelease(row) {
  const github = config.githubRepo;
  const tag = `v${row.version}`;
  return {
    version: row.version,
    status: row.status,
    is_latest: row.is_latest === true,
    changelog: row.changelog || '',
    released_at: row.released_at,
    source: row.source || {
      zip_url: `${github}/archive/refs/tags/${tag}.zip`,
      tarball_url: `${github}/archive/refs/tags/${tag}.tar.gz`,
      hosted_url: `${config.filesUrl}/releases/niao-${row.version}-source.tgz`,
      ftp_path: `releases/niao-${row.version}-source.tgz`,
    },
    variants: (row.variants || []).map((v) => ({
      ...v,
      url: v.url || releaseVariantUrl(row.version, v.id),
      ftp_path: v.ftp_path || releaseVariantFtpPath(row.version, v.id),
    })),
  };
}

export async function upsertReleaseDoc(release) {
  const db = await getDb();
  if (!db) throw new Error('MongoDB not configured');
  const ts = now();
  const version = release.version;
  const variants = release.variants?.length ? release.variants : variantTemplate(version);

  const doc = {
    version,
    status: release.status || RELEASE_STATUS.ACTIVE,
    is_latest: release.is_latest === true,
    changelog: release.changelog || '',
    source: {
      zip_url: release.source?.zip_url || `${config.githubRepo}/archive/refs/tags/v${version}.zip`,
      tarball_url: release.source?.tarball_url || `${config.githubRepo}/archive/refs/tags/v${version}.tar.gz`,
      hosted_url: `${config.filesUrl}/releases/niao-${version}-source.tgz`,
      ftp_path: `releases/niao-${version}-source.tgz`,
    },
    variants: variants.map((v) => ({
      id: v.id,
      label: v.label,
      platform: v.platform,
      arch: v.arch,
      status: v.status || VERSION_STATUS.ACTIVE,
      url: v.url || releaseVariantUrl(version, v.id),
      ftp_path: v.ftp_path || releaseVariantFtpPath(version, v.id),
      shasum: v.shasum || '',
      size: v.size || 0,
    })),
    updated_at: ts,
  };

  if (doc.is_latest) {
    await db.collection('niao_releases').updateMany({}, { $set: { is_latest: false } });
  }

  await db.collection('niao_releases').updateOne(
    { version },
    { $set: doc, $setOnInsert: { released_at: ts } },
    { upsert: true },
  );
  return normalizeRelease(doc);
}

export async function setReleaseStatus(version, status) {
  const db = await getDb();
  if (!db) throw new Error('MongoDB not configured');
  await db.collection('niao_releases').updateOne(
    { version },
    { $set: { status, updated_at: now() } },
  );
  if (status === RELEASE_STATUS.ACTIVE) {
    await db.collection('niao_releases').updateMany(
      { version: { $ne: version } },
      { $set: { is_latest: false } },
    );
    await db.collection('niao_releases').updateOne(
      { version },
      { $set: { is_latest: true } },
    );
  }
}

export async function deleteReleaseFromDb(version) {
  const db = await getDb();
  if (!db) throw new Error('MongoDB not configured');
  await db.collection('niao_releases').deleteOne({ version });
}

export async function updateReleaseVariant(version, variantId, patch) {
  const db = await getDb();
  if (!db) throw new Error('MongoDB not configured');
  const release = await db.collection('niao_releases').findOne({ version });
  if (!release) throw new Error(`release not found: ${version}`);
  const variants = (release.variants || []).map((v) =>
    v.id === variantId ? { ...v, ...patch, updated_at: now() } : v,
  );
  await db.collection('niao_releases').updateOne(
    { version },
    { $set: { variants, updated_at: now() } },
  );
}

export function isMongoPrimary() {
  return isDbReady();
}
