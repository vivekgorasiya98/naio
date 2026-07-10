import dns from 'node:dns';
import fs from 'node:fs/promises';
import path from 'node:path';
import { Client } from 'basic-ftp';
import { config } from '../config.js';

// Prefer IPv4 — avoids timeout when IPv6 route is broken
dns.setDefaultResultOrder('ipv4first');

export function formatBytes(bytes) {
  const n = Number(bytes) || 0;
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(2)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatEta(seconds) {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const r = s % 60;
  if (m < 60) return `${m}m ${r}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

function progressBar(pct, width = 24) {
  const filled = Math.round((pct / 100) * width);
  return `${'█'.repeat(filled)}${'░'.repeat(width - filled)}`;
}

/** Live console progress for CLI release / sync-ftp. */
export function createConsoleFtpProgress({ label = 'FTP upload' } = {}) {
  const started = Date.now();
  let lastLineLen = 0;
  let barStarted = false;

  function writeLine(line) {
    process.stdout.write(`\r${line}${' '.repeat(Math.max(0, lastLineLen - line.length))}`);
    lastLineLen = line.length;
  }

  return {
    onProgress(state) {
      switch (state.phase) {
        case 'connecting':
          console.log('  Connecting…');
          return;
        case 'scanning':
          console.log('  Scanning local files…');
          return;
        case 'cleanup':
          console.log('  Cleaning legacy FTP directories…');
          return;
        case 'uploading':
          if (!barStarted) {
            console.log(
              `  Uploading ${state.totalFiles} files (${formatBytes(state.totalBytes)})…`,
            );
            barStarted = true;
          }
          break;
        default:
          return;
      }

      const pct =
        state.totalBytes > 0
          ? Math.min(100, (state.uploadedBytes / state.totalBytes) * 100)
          : 0;
      const elapsedSec = (Date.now() - started) / 1000;
      const rate = state.uploadedBytes / (elapsedSec || 1);
      const leftBytes = Math.max(0, state.totalBytes - state.uploadedBytes);
      const eta = rate > 0 ? leftBytes / rate : 0;
      const leftFiles = Math.max(0, state.totalFiles - state.uploadedFiles);
      const shortFile = state.currentFile
        ? state.currentFile.length > 42
          ? `…${state.currentFile.slice(-41)}`
          : state.currentFile
        : '';

      writeLine(
        `  [${progressBar(pct)}] ${pct.toFixed(1)}%  ` +
          `${state.uploadedFiles}/${state.totalFiles} files (${leftFiles} left)  ` +
          `${formatBytes(state.uploadedBytes)} / ${formatBytes(state.totalBytes)}  ` +
          `${formatBytes(rate)}/s  ETA ${formatEta(eta)}  ${shortFile}`,
      );
    },
    done(result) {
      if (barStarted) process.stdout.write('\n');
      const elapsed = formatEta((Date.now() - started) / 1000);
      console.log(
        `  ✓ ${label}: ${result.uploaded} files, ${formatBytes(result.totalBytes)} in ${elapsed} → ${result.remote}`,
      );
    },
    fail(err) {
      if (barStarted) process.stdout.write('\n');
      console.error(`  ✗ ${label} failed: ${err.message}`);
    },
  };
}

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

export async function syncToFtp({ onProgress } = {}) {
  if (!ftpConfigured()) {
    throw new Error('FTP not configured — set FTP_HOST, FTP_USER, FTP_PASSWORD in .env');
  }

  const started = Date.now();
  const state = {
    phase: 'connecting',
    totalFiles: 0,
    uploadedFiles: 0,
    totalBytes: 0,
    uploadedBytes: 0,
    currentFile: '',
  };
  const report = (patch) => {
    Object.assign(state, patch);
    onProgress?.({ ...state });
  };

  const client = new Client(config.ftp.timeoutMs);
  client.ftp.verbose = config.nodeEnv !== 'production' && !onProgress;

  let remoteRoot = (config.ftp.remoteDir || '/').replace(/\/$/, '');
  if (!remoteRoot) remoteRoot = '/';

  try {
    report({ phase: 'connecting' });
    await client.access({
      host: config.ftp.host,
      user: config.ftp.user,
      password: config.ftp.password,
      secure: config.ftp.secure,
    });

    const localRoot = config.dataDir;
    const remotePackages = remoteRoot === '/' ? '/v1/packages' : path.posix.join(remoteRoot, 'v1/packages');
    report({ phase: 'cleanup' });
    await cleanupLegacyPackageDirs(client, remotePackages);

    report({ phase: 'scanning' });
    const files = await walk(localRoot);
    if (files.length === 0) {
      throw new Error('no files in DATA_DIR — run npm run seed first');
    }

    const fileEntries = [];
    let totalBytes = 0;
    for (const file of files) {
      const st = await fs.stat(file);
      const rel = path.relative(localRoot, file).split(path.sep).join('/');
      fileEntries.push({ file, rel, size: st.size });
      totalBytes += st.size;
    }

    if (remoteRoot !== '/') {
      await client.ensureDir(remoteRoot);
    }

    report({
      phase: 'uploading',
      totalFiles: fileEntries.length,
      totalBytes,
      uploadedFiles: 0,
      uploadedBytes: 0,
      currentFile: '',
    });

    let uploadedFiles = 0;
    let uploadedBytes = 0;
    for (const entry of fileEntries) {
      const remotePath =
        remoteRoot === '/' ? `/${entry.rel}` : path.posix.join(remoteRoot, entry.rel);
      const remoteDir = path.posix.dirname(remotePath);
      report({ currentFile: entry.rel });
      if (remoteDir && remoteDir !== '.') {
        await client.ensureDir(remoteDir);
      }
      await client.uploadFrom(entry.file, remotePath);
      uploadedFiles += 1;
      uploadedBytes += entry.size;
      report({ uploadedFiles, uploadedBytes, currentFile: entry.rel });
    }

    // Ensure Apache rewrite rules are present (some hosts skip dotfiles in bulk upload)
    const htaccess = path.join(localRoot, '.htaccess');
    if (await fs.access(htaccess).then(() => true).catch(() => false)) {
      const remoteHtaccess = remoteRoot === '/' ? '/.htaccess' : path.posix.join(remoteRoot, '.htaccess');
      const already = fileEntries.some((e) => e.rel === '.htaccess');
      if (!already) {
        const st = await fs.stat(htaccess);
        report({ currentFile: '.htaccess' });
        await client.uploadFrom(htaccess, remoteHtaccess);
        uploadedFiles += 1;
        uploadedBytes += st.size;
        report({ uploadedFiles, uploadedBytes, currentFile: '.htaccess' });
      }
    }

    const pwd = await client.pwd().catch(() => remoteRoot);
    report({ phase: 'done', currentFile: '' });

    return {
      uploaded: uploadedFiles,
      totalBytes: uploadedBytes,
      durationMs: Date.now() - started,
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
