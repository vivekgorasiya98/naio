import dns from 'node:dns';
import fs from 'node:fs/promises';
import path from 'node:path';
import { Client } from 'basic-ftp';
import { config } from '../config.js';

// Prefer IPv4 — avoids timeout when IPv6 route is broken
dns.setDefaultResultOrder('ipv4first');

async function walk(dir) {
  const entries = await fs.readdir(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await walk(full)));
    } else {
      files.push(full);
    }
  }
  return files;
}

export function ftpConfigured() {
  return Boolean(config.ftp.host && config.ftp.user && config.ftp.password);
}

/** Remove legacy per-package directories that shadow {name}.json rewrite rules on Apache. */
async function cleanupLegacyPackageDirs(client, remotePackagesDir) {
  try {
    await client.cd(remotePackagesDir);
  } catch {
    return;
  }
  const entries = await client.list();
  for (const entry of entries) {
    if (entry.isDirectory && /^[a-z][a-z0-9_-]*$/i.test(entry.name)) {
      const target = path.posix.join(remotePackagesDir, entry.name);
      try {
        await client.removeDir(target, true);
      } catch {
        // best effort
      }
    }
  }
}

function ftpHint(err) {
  const msg = err.message || String(err);
  if (msg.includes('530') || msg.includes('Login')) {
    return ' Check FTP_USER, FTP_PASSWORD, and unlock FTP in StackCP (Manage Hosting → Unlock FTP).';
  }
  if (msg.includes('Timeout') || msg.includes('control socket')) {
    return ' Use FTP_HOST=ftp.stackcp.com (not ftp.stackcp.risu.in). Unlock FTP in StackCP control panel.';
  }
  return '';
}

export async function syncToFtp() {
  if (!ftpConfigured()) {
    throw new Error('FTP not configured — set FTP_HOST, FTP_USER, FTP_PASSWORD in .env');
  }

  const client = new Client(config.ftp.timeoutMs);
  client.ftp.verbose = config.nodeEnv !== 'production';

  let remoteRoot = (config.ftp.remoteDir || '/').replace(/\/$/, '');
  if (!remoteRoot) remoteRoot = '/';

  try {
    await client.access({
      host: config.ftp.host,
      user: config.ftp.user,
      password: config.ftp.password,
      secure: config.ftp.secure,
    });

    const localRoot = config.dataDir;
    const remotePackages = remoteRoot === '/' ? '/v1/packages' : path.posix.join(remoteRoot, 'v1/packages');
    await cleanupLegacyPackageDirs(client, remotePackages);

    const files = await walk(localRoot);
    if (files.length === 0) {
      throw new Error('no files in DATA_DIR — run npm run seed first');
    }

    if (remoteRoot !== '/') {
      await client.ensureDir(remoteRoot);
    }

    for (const file of files) {
      const rel = path.relative(localRoot, file).split(path.sep).join('/');
      const remotePath =
        remoteRoot === '/' ? `/${rel}` : path.posix.join(remoteRoot, rel);
      const remoteDir = path.posix.dirname(remotePath);
      if (remoteDir && remoteDir !== '.') {
        await client.ensureDir(remoteDir);
      }
      await client.uploadFrom(file, remotePath);
    }

    // Ensure Apache rewrite rules are present (some hosts skip dotfiles in bulk upload)
    const htaccess = path.join(localRoot, '.htaccess');
    if (await fs.access(htaccess).then(() => true).catch(() => false)) {
      const remoteHtaccess = remoteRoot === '/' ? '/.htaccess' : path.posix.join(remoteRoot, '.htaccess');
      await client.uploadFrom(htaccess, remoteHtaccess);
    }

    const pwd = await client.pwd().catch(() => remoteRoot);

    return {
      uploaded: files.length,
      remote: `${config.ftp.host}:${pwd}`,
      host: config.ftp.host,
      remoteDir: pwd,
    };
  } catch (err) {
    throw new Error(`FTP sync failed: ${err.message}.${ftpHint(err)}`);
  } finally {
    client.close();
  }
}

export async function testFtpConnection() {
  if (!ftpConfigured()) {
    throw new Error('FTP not configured');
  }
  const client = new Client(config.ftp.timeoutMs);
  try {
    await client.access({
      host: config.ftp.host,
      user: config.ftp.user,
      password: config.ftp.password,
      secure: config.ftp.secure,
    });
    const pwd = await client.pwd();
    const list = await client.list();
    return {
      ok: true,
      host: config.ftp.host,
      pwd,
      entries: list.length,
      sample: list.slice(0, 8).map((e) => e.name),
    };
  } finally {
    client.close();
  }
}
