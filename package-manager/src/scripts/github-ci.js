/**
 * Trigger niao-release GitHub Actions workflow and download platform artifacts.
 * Requires GITHUB_TOKEN in .env (classic PAT with repo + actions:read).
 */
import fs from 'node:fs/promises';
import path from 'node:path';
import { execSync } from 'node:child_process';

const WORKFLOW_FILE = 'niao-release.yml';

function parseRepo(githubUrl) {
  if (process.env.GITHUB_REPO?.includes('/')) {
    return process.env.GITHUB_REPO.trim();
  }
  const m = String(githubUrl || '').match(/github\.com[/:]([^/]+)\/([^/.]+)/);
  if (!m) throw new Error(`Invalid GitHub repo — set GITHUB_REPO=owner/name in .env`);
  return `${m[1]}/${m[2]}`;
}

async function ghFetch(url, token, opts = {}) {
  const res = await fetch(url, {
    ...opts,
    headers: {
      Accept: 'application/vnd.github+json',
      'X-GitHub-Api-Version': '2022-11-28',
      Authorization: `Bearer ${token}`,
      ...(opts.headers || {}),
    },
  });
  if (!res.ok) {
    const body = await res.text().catch(() => '');
    if (res.status === 404) {
      throw new Error(
        `GitHub repo not found (404). Push your code to GitHub first, then set in .env:\n` +
          `  GITHUB_REPO=your-username/niao\n` +
          `  NIAO_GITHUB_REPO=https://github.com/your-username/niao`,
      );
    }
    throw new Error(`GitHub API ${res.status}: ${body.slice(0, 300)}`);
  }
  return res;
}

export function githubCiConfigured() {
  return Boolean(process.env.GITHUB_TOKEN);
}

export async function triggerReleaseWorkflow(repo, ref = 'main') {
  const token = process.env.GITHUB_TOKEN;
  if (!token) return null;

  const ownerRepo = parseRepo(repo);
  const workflows = await (await ghFetch(
    `https://api.github.com/repos/${ownerRepo}/actions/workflows`,
    token,
  )).json();

  const wf = workflows.workflows?.find((w) => w.path?.endsWith(WORKFLOW_FILE));
  if (!wf) throw new Error(`Workflow ${WORKFLOW_FILE} not found on ${ownerRepo}`);

  const before = await (await ghFetch(
    `https://api.github.com/repos/${ownerRepo}/actions/workflows/${wf.id}/runs?per_page=1`,
    token,
  )).json();
  const minRunId = before.workflow_runs?.[0]?.id || 0;

  await ghFetch(`https://api.github.com/repos/${ownerRepo}/actions/workflows/${wf.id}/dispatches`, token, {
    method: 'POST',
    body: JSON.stringify({ ref }),
  });

  console.log(`  ✓ Triggered GitHub Actions: ${ownerRepo}@${ref}`);
  return { ownerRepo, workflowId: wf.id, minRunId };
}

export async function waitForLatestRun(ownerRepo, workflowId, { minRunId = 0, timeoutMs = 90 * 60_000, pollMs = 20_000 } = {}) {
  const token = process.env.GITHUB_TOKEN;
  const started = Date.now();

  while (Date.now() - started < timeoutMs) {
    const data = await (await ghFetch(
      `https://api.github.com/repos/${ownerRepo}/actions/workflows/${workflowId}/runs?per_page=5`,
      token,
    )).json();

    const run = data.workflow_runs?.find((r) => r.id > minRunId);
    if (!run) {
      process.stdout.write('  … waiting for CI to start\r');
      await sleep(pollMs);
      continue;
    }

    if (run.status !== 'completed') {
      process.stdout.write(`  … CI ${run.status} (run ${run.id})   \r`);
      await sleep(pollMs);
      continue;
    }

    if (run.conclusion !== 'success') {
      throw new Error(`GitHub Actions run ${run.id} failed: ${run.conclusion}`);
    }
    return run;
  }

  throw new Error('Timed out waiting for GitHub Actions release build');
}

export async function downloadRunArtifacts(runId, ownerRepo, destDir) {
  const token = process.env.GITHUB_TOKEN;
  await fs.mkdir(destDir, { recursive: true });

  const data = await (await ghFetch(
    `https://api.github.com/repos/${ownerRepo}/actions/runs/${runId}/artifacts`,
    token,
  )).json();

  const artifacts = data.artifacts || [];
  if (!artifacts.length) throw new Error('No CI artifacts found');

  let count = 0;
  for (const art of artifacts) {
    const zipPath = path.join(destDir, `${art.name}.zip`);
    const buf = Buffer.from(await (await ghFetch(art.archive_download_url, token)).arrayBuffer());
    await fs.writeFile(zipPath, buf);

    const extractDir = path.join(destDir, art.name);
    await fs.mkdir(extractDir, { recursive: true });
    if (process.platform === 'win32') {
      execSync(
        `powershell -NoProfile -Command "Expand-Archive -Path '${zipPath}' -DestinationPath '${extractDir}' -Force"`,
        { stdio: 'pipe' },
      );
    } else {
      execSync(`unzip -o "${zipPath}" -d "${extractDir}"`, { stdio: 'pipe' });
    }
    count += 1;
  }

  return count;
}

export async function copyCiArtifactsToReleases(ciDir, releasesDir, version) {
  const copied = [];
  const entries = await fs.readdir(ciDir, { withFileTypes: true });

  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const platformDir = path.join(ciDir, entry.name);
    const files = await fs.readdir(platformDir);
    for (const file of files) {
      if (!file.startsWith(`niao-${version}-`)) continue;
      const src = path.join(platformDir, file);
      const dest = path.join(releasesDir, file);
      await fs.copyFile(src, dest);
      copied.push(file);
      console.log(`  ✓ from CI: ${file}`);
    }
  }

  return copied;
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}
