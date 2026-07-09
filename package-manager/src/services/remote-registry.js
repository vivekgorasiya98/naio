import { config } from '../config.js';

function filesUrl(pathname) {
  const base = config.filesUrl.replace(/\/$/, '');
  const path = pathname.startsWith('/') ? pathname : `/${pathname}`;
  return `${base}${path}`;
}

export async function fetchRemoteJson(pathname) {
  const url = filesUrl(pathname);
  const res = await fetch(url, {
    headers: { accept: 'application/json' },
    signal: AbortSignal.timeout(30_000),
  });
  if (!res.ok) {
    throw new Error(`remote fetch failed (${res.status}): ${url}`);
  }
  return res.json();
}

export async function fetchCatalog() {
  try {
    return await fetchRemoteJson('/v1/catalog');
  } catch {
    return fetchRemoteJson('/catalog.json');
  }
}

export async function fetchPackageList() {
  try {
    const data = await fetchRemoteJson('/v1/packages/index.json');
    if (Array.isArray(data?.packages) && data.packages.length > 0) {
      return data.packages.map((p) => p.name).sort();
    }
  } catch {
    // fall through
  }
  const catalog = await fetchCatalog();
  return Object.keys(catalog.libs || {}).sort();
}

export async function fetchPackageMeta(name) {
  const encoded = encodeURIComponent(name);
  const paths = [
    `/v1/packages/${encoded}.json`,
    `/v1/packages/${encoded}`,
  ];
  for (const pathname of paths) {
    try {
      return await fetchRemoteJson(pathname);
    } catch {
      // try next path
    }
  }

  const catalog = await fetchCatalog();
  const entry = catalog.libs?.[name];
  if (!entry) {
    throw new Error(`package not found on files host: ${name}`);
  }
  return {
    name: entry.name || name,
    version: entry.version,
    kind: entry.kind,
    description: entry.description || '',
    import_paths: entry.import_paths || [],
    builtin_count: entry.builtin_count || 0,
    remote: entry.remote === true,
    versions: entry.versions || [entry.version],
    latest: entry.versions?.at(-1) || entry.version,
  };
}

export async function fetchVersionMeta(name, version) {
  const encName = encodeURIComponent(name);
  const encVer = encodeURIComponent(version);
  const paths = [
    `/v1/packages/${encName}-${encVer}.json`,
    `/v1/packages/${encName}/${encVer}.json`,
    `/v1/packages/${encName}/${encVer}`,
  ];
  for (const pathname of paths) {
    try {
      return await fetchRemoteJson(pathname);
    } catch {
      // try next path
    }
  }
  throw new Error(`version not found on files host: ${name}@${version}`);
}

export function remoteTarballUrl(name, version) {
  return filesUrl(`/v1/packages/${encodeURIComponent(name)}/${encodeURIComponent(version)}/tarball`);
}
