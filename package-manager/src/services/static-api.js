import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import { config } from '../config.js';
import {
  listPackages,
  listVersions,
  readPackageManifest,
  tarballPath,
} from './storage.js';

/** Static API mirror for Apache/StackCP when Node is not running. */
export async function buildStaticApiMirror() {
  const root = config.dataDir;
  const v1 = path.join(root, 'v1');
  const pkgApi = path.join(v1, 'packages');
  await fs.mkdir(pkgApi, { recursive: true });

  // catalog at /v1/catalog
  await fs.copyFile(config.catalogPath, path.join(v1, 'catalog'));

  const names = await listPackages();
  const packageSummaries = [];

  for (const name of names) {
    const pkg = await readPackageManifest(name);
    const versions = await listVersions(name);
    const summary = { ...pkg, versions, latest: versions.at(-1) || pkg.version };
    packageSummaries.push(summary);

    await fs.writeFile(
      path.join(pkgApi, `${name}.json`),
      JSON.stringify(summary, null, 2) + '\n',
    );

    const versionDetails = pkg.version_details || [];

    for (const version of versions) {
      const tgz = tarballPath(name, version);
      let buf;
      try {
        buf = await fs.readFile(tgz);
      } catch {
        continue;
      }
      const shasum = crypto.createHash('sha256').update(buf).digest('hex');
      const meta = {
        name,
        version,
        package: { ...pkg, version },
        dist: {
          tarball: `${config.filesUrl}/v1/packages/${name}/${version}/tarball`,
          shasum,
          size: buf.length,
        },
      };
      if (versionDetails) {
        const detail = versionDetails.find((d) => d.version === version);
        if (detail) meta.status = detail.status;
      }
      // Flat file avoids /v1/packages/{name}/ directory shadowing {name}.json rewrites
      await fs.writeFile(
        path.join(pkgApi, `${name}-${version}.json`),
        JSON.stringify(meta, null, 2) + '\n',
      );
    }
  }

  await fs.writeFile(
    path.join(pkgApi, 'index.json'),
    JSON.stringify({ packages: packageSummaries }, null, 2) + '\n',
  );

  const htaccess = `# Niao nm static files — nm.c4compare.com
RewriteEngine On
RewriteBase /

RewriteRule ^v1/catalog$ v1/catalog [L]
RewriteRule ^v1/packages$ v1/packages/index.json [L]
RewriteRule ^v1/packages/([a-zA-Z0-9_-]+)$ v1/packages/$1.json [L]
RewriteRule ^v1/packages/([a-zA-Z0-9_-]+)/([0-9.]+)$ v1/packages/$1-$2.json [L]
RewriteRule ^v1/packages/([a-zA-Z0-9_-]+)/([0-9.]+)/tarball$ tarballs/$1-$2.tgz [L]

# CORS for nm client
<IfModule mod_headers.c>
  Header set Access-Control-Allow-Origin "*"
  Header set Access-Control-Allow-Methods "GET, OPTIONS"
</IfModule>
`;
  await fs.writeFile(path.join(root, '.htaccess'), htaccess);
  // StackCP FTP often skips dotfiles — upload a copy operators can rename if needed
  await fs.writeFile(path.join(root, 'htaccess.txt'), htaccess);
}
