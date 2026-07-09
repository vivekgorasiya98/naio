/**
 * End-to-end test: registry API, admin publish, tarball download, checksum.
 * Usage: node src/scripts/test-all.js [baseUrl]
 *
 * Env: ADMIN_USERNAME, ADMIN_PASSWORD (defaults: admin / admin)
 */
import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import * as tar from 'tar';
import dotenv from 'dotenv';

dotenv.config();

const base = (process.argv[2] || process.env.API_URL || 'http://127.0.0.1:3000').replace(/\/$/, '');
const adminUser = process.env.ADMIN_USERNAME || 'admin';
const adminPass = process.env.ADMIN_PASSWORD || 'admin';
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const tmpDir = path.join(__dirname, '../../.test-tmp');

let passed = 0;
let failed = 0;

function ok(name) {
  passed++;
  console.log(`  ✓ ${name}`);
}

function fail(name, err) {
  failed++;
  console.error(`  ✗ ${name}: ${err}`);
}

async function json(method, urlPath, body, token) {
  const res = await fetch(`${base}${urlPath}`, {
    method,
    headers: {
      ...(body ? { 'Content-Type': 'application/json' } : {}),
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  let data;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = text;
  }
  return { res, data };
}

async function main() {
  console.log(`\nNiao registry E2E tests → ${base}\n`);

  // 1. Health
  try {
    const { res, data } = await json('GET', '/health');
    if (!res.ok || !data?.ok) throw new Error(JSON.stringify(data));
    ok('GET /health');
  } catch (e) {
    fail('GET /health', e.message);
    console.error('\nServer not running? Start with: npm run dev\n');
    process.exit(1);
  }

  // 1b. Root metadata uses correct registry URL (JSON API mode)
  try {
    const { res, data } = await json('GET', '/');
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    if (typeof data === 'object' && data?.registry) {
      if (!data.registry.includes('taurus-tech.in')) {
        throw new Error(`unexpected registry URL: ${data.registry}`);
      }
      ok(`GET / (registry=${data.registry})`);
    } else {
      ok('GET / (landing page — set API_URL on server for JSON metadata)');
    }
  } catch (e) {
    fail('GET /', e.message);
  }

  // 2. Catalog
  try {
    const { res, data } = await json('GET', '/v1/catalog');
    if (!res.ok || !data?.libs?.nllm) throw new Error('catalog missing nllm');
    ok(`GET /v1/catalog (${Object.keys(data.libs).length} libs)`);
  } catch (e) {
    fail('GET /v1/catalog', e.message);
  }

  // 3. Package list
  try {
    const { res, data } = await json('GET', '/v1/packages');
    if (!res.ok || !data.packages?.length) throw new Error('no packages');
    ok(`GET /v1/packages (${data.packages.length} packages)`);
  } catch (e) {
    fail('GET /v1/packages', e.message);
  }

  // 4. Package metadata
  try {
    const { res, data } = await json('GET', '/v1/packages/nllm');
    if (!res.ok || data.version !== '0.2.2') throw new Error(JSON.stringify(data));
    ok('GET /v1/packages/nllm');
  } catch (e) {
    fail('GET /v1/packages/nllm', e.message);
  }

  // 5. Version metadata + dist
  let nllmShasum = '';
  let nllmTarballUrl = '';
  try {
    const { res, data } = await json('GET', '/v1/packages/nllm/0.2.2');
    if (!res.ok || !data.dist?.tarball || !data.dist?.shasum) throw new Error(JSON.stringify(data));
    nllmShasum = data.dist.shasum;
    nllmTarballUrl = data.dist.tarball;
    if (!nllmTarballUrl.includes('taurus-tech.in') && !nllmTarballUrl.includes('c4compare.com')) {
      throw new Error(`unexpected tarball host: ${nllmTarballUrl}`);
    }
    ok(`GET /v1/packages/nllm/0.2.2 (sha256 ${nllmShasum.slice(0, 12)}…)`);
  } catch (e) {
    fail('GET /v1/packages/nllm/0.2.2', e.message);
  }

  // 6. Tarball download + checksum
  try {
    const res = await fetch(`${base}/v1/packages/nllm/0.2.2/tarball`);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const buf = Buffer.from(await res.arrayBuffer());
    const hash = crypto.createHash('sha256').update(buf).digest('hex');
    if (nllmShasum && hash !== nllmShasum) throw new Error(`checksum mismatch: ${hash}`);
    await fs.mkdir(tmpDir, { recursive: true });
    const tgz = path.join(tmpDir, 'nllm-0.2.2.tgz');
    await fs.writeFile(tgz, buf);
    ok(`GET tarball nllm@0.2.2 (${buf.length} bytes, checksum OK)`);

    // 7. Extract tarball
    const extractDir = path.join(tmpDir, 'extract-nllm');
    await fs.rm(extractDir, { recursive: true, force: true });
    await fs.mkdir(extractDir, { recursive: true });
    await tar.x({ file: tgz, cwd: extractDir });
    const libJson = path.join(extractDir, 'nllm', '0.2.2', 'lib.json');
    const lib = JSON.parse(await fs.readFile(libJson, 'utf8'));
    if (lib.name !== 'nllm') throw new Error('bad lib.json in tarball');
    ok('tarball extracts valid nllm/0.2.2/lib.json');
  } catch (e) {
    fail('tarball download/extract', e.message);
  }

  // 8. Admin login
  let token = '';
  try {
    const { res, data } = await json('POST', '/admin/login', {
      username: adminUser,
      password: adminPass,
    });
    if (!res.ok || !data.token) throw new Error(JSON.stringify(data));
    token = data.token;
    ok('POST /admin/login');
  } catch (e) {
    fail('POST /admin/login', e.message);
  }

  // 9. Admin publish (JSON)
  const testName = 'testpkg';
  const testVersion = '0.0.1';
  try {
    const { res, data } = await json(
      'POST',
      `/admin/packages/${testName}/${testVersion}/publish-json`,
      {
        package: {
          name: testName,
          version: testVersion,
          kind: 'native',
          description: 'E2E test package',
          import_paths: ['testpkg'],
          builtin_count: 1,
        },
      },
      token,
    );
    if (!res.ok || !data.ok) throw new Error(JSON.stringify(data));
    ok(`POST admin publish ${testName}@${testVersion}`);
  } catch (e) {
    fail('admin publish', e.message);
  }

  // 10. Verify published package downloadable
  try {
    const { res, data } = await json('GET', `/v1/packages/${testName}/${testVersion}`);
    if (!res.ok) throw new Error(JSON.stringify(data));
    const tgzRes = await fetch(`${base}/v1/packages/${testName}/${testVersion}/tarball`);
    if (!tgzRes.ok) throw new Error(`tarball HTTP ${tgzRes.status}`);
    const buf = Buffer.from(await tgzRes.arrayBuffer());
    if (buf.length < 100) throw new Error('tarball too small');
    ok(`download published ${testName}@${testVersion} (${buf.length} bytes)`);
  } catch (e) {
    fail('download published package', e.message);
  }

  // 11. Admin list packages
  try {
    const { res, data } = await json('GET', '/admin/packages', null, token);
    if (!res.ok || !data.packages?.find((p) => p.name === testName)) {
      throw new Error('testpkg not in admin list');
    }
    ok('GET /admin/packages');
  } catch (e) {
    fail('GET /admin/packages', e.message);
  }

  // 12. Admin delete test package
  try {
    const { res, data } = await json(
      'DELETE',
      `/admin/packages/${testName}/${testVersion}`,
      null,
      token,
    );
    if (!res.ok) throw new Error(JSON.stringify(data));
    ok(`DELETE ${testName}@${testVersion}`);
  } catch (e) {
    fail('admin delete', e.message);
  }

  // 13. FTP sync (optional — only if configured)
  try {
    const { res, data } = await json('POST', '/admin/sync/ftp', null, token);
    if (res.ok) {
      ok(`FTP sync (${data.uploaded} files → ${data.remote})`);
    } else if (data?.error?.includes('not configured')) {
      ok('FTP sync skipped (not configured in .env)');
    } else {
      throw new Error(data?.error || `HTTP ${res.status}`);
    }
  } catch (e) {
    fail('FTP sync', e.message);
  }

  // Cleanup
  await fs.rm(tmpDir, { recursive: true, force: true }).catch(() => {});

  console.log(`\n${'─'.repeat(40)}`);
  console.log(`Results: ${passed} passed, ${failed} failed\n`);
  process.exit(failed > 0 ? 1 : 0);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
