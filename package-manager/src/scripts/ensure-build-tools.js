/**
 * Auto-install Windows cross-build prerequisites:
 * - NASM (portable zip) for Windows x86 (32-bit) targets
 * - Visual Studio Build Tools ARM64 + x86 MSVC components
 */
import fs from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { execSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const pkgRoot = path.resolve(__dirname, '../..');
const toolsDir = path.join(pkgRoot, '.tools');
const nasmDir = path.join(toolsDir, 'nasm-3.01');
const nasmExe = path.join(nasmDir, 'nasm.exe');
const NASM_URL = 'https://www.nasm.us/pub/nasm/releasebuilds/3.01/win64/nasm-3.01-win64.zip';

const VS_SETUP = path.join(
  process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)',
  'Microsoft Visual Studio',
  'Installer',
  'setup.exe',
);
const VS_BUILDTOOLS_URL = 'https://aka.ms/vs/17/release/vs_buildtools.exe';
const VS_BUILDTOOLS_EXE = path.join(toolsDir, 'vs_buildtools.exe');

const VS_ARM64_COMPONENT = 'Microsoft.VisualStudio.Component.VC.Tools.ARM64';
const VS_LLVM_COMPONENTS = [
  'Microsoft.VisualStudio.Component.VC.Llvm.Clang',
  'Microsoft.VisualStudio.Component.VC.Llvm.ClangToolset',
];

export function vsInstallPath() {
  try {
    const vswhere = path.join(
      process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)',
      'Microsoft Visual Studio',
      'Installer',
      'vswhere.exe',
    );
    const out = execSync(
      `"${vswhere}" -latest -products * -property installationPath`,
      { encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] },
    ).trim();
    return out || null;
  } catch {
    return null;
  }
}

export function hasNasm() {
  if (existsSync(nasmExe)) return true;
  try {
    execSync('nasm -v', { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}

export async function nasmBinDir() {
  try {
    await fs.access(nasmExe);
    return nasmDir;
  } catch {
    return null;
  }
}

export function hasMsvcArm64(installPath = vsInstallPath()) {
  if (!installPath) return false;
  const glob = path.join(installPath, 'VC', 'Tools', 'MSVC', '*', 'bin', 'Hostx64', 'arm64', 'cl.exe');
  try {
    const matches = execSync(
      `powershell -NoProfile -Command "Get-Item -Path '${glob}' -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty FullName"`,
      { encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] },
    ).trim();
    return !!matches;
  } catch {
    return false;
  }
}

export function hasMsvcX86(installPath = vsInstallPath()) {
  if (!installPath) return false;
  const glob = path.join(installPath, 'VC', 'Tools', 'MSVC', '*', 'bin', 'Hostx64', 'x86', 'cl.exe');
  try {
    const matches = execSync(
      `powershell -NoProfile -Command "Get-Item -Path '${glob}' -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty FullName"`,
      { encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] },
    ).trim();
    return !!matches;
  } catch {
    return false;
  }
}

async function downloadNasm() {
  await fs.mkdir(toolsDir, { recursive: true });
  const zipPath = path.join(toolsDir, 'nasm-3.01-win64.zip');
  console.log('  Downloading NASM 3.01…');
  execSync(
    `powershell -NoProfile -Command "$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -Uri '${NASM_URL}' -OutFile '${zipPath}'"`,
    { stdio: 'inherit' },
  );
  console.log('  Extracting NASM…');
  execSync(
    `powershell -NoProfile -Command "Expand-Archive -Path '${zipPath}' -DestinationPath '${toolsDir}' -Force"`,
    { stdio: 'inherit' },
  );
  await fs.access(nasmExe);
  console.log(`  ✓ NASM → ${nasmDir}`);
}

export function clangBinDir(installPath = vsInstallPath()) {
  if (!installPath) return null;
  const candidates = [
    path.join(installPath, 'VC', 'Tools', 'Llvm', 'x64', 'bin'),
    path.join(installPath, 'VC', 'Tools', 'Llvm', 'ARM64', 'bin'),
    path.join(installPath, 'VC', 'Tools', 'Llvm', 'bin'),
  ];
  for (const dir of candidates) {
    if (existsSync(path.join(dir, 'clang-cl.exe'))) return dir;
  }
  return null;
}

export function hasClang(installPath = vsInstallPath()) {
  if (clangBinDir(installPath)) return true;
  try {
    execSync('clang-cl -? >nul 2>&1', { stdio: 'pipe', shell: true });
    return true;
  } catch {
    return false;
  }
}

export async function ensureNasm() {
  if (hasNasm()) {
    console.log('  ✓ NASM (system PATH)');
    return '';
  }
  const local = await nasmBinDir();
  if (local) {
    console.log(`  ✓ NASM (local ${local})`);
    return local;
  }
  await downloadNasm();
  return nasmDir;
}

function prependPath(dir, current) {
  const norm = dir.replace(/\//g, '\\');
  if (!current) return norm;
  if (current.toLowerCase().includes(norm.toLowerCase())) return current;
  return `${norm};${current}`;
}

async function ensureVsBuildtoolsBootstrapper() {
  try {
    await fs.access(VS_BUILDTOOLS_EXE);
    return VS_BUILDTOOLS_EXE;
  } catch {
    await fs.mkdir(toolsDir, { recursive: true });
    console.log('  Downloading Visual Studio Build Tools bootstrapper…');
    execSync(
      `powershell -NoProfile -Command "$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -Uri '${VS_BUILDTOOLS_URL}' -OutFile '${VS_BUILDTOOLS_EXE}'"`,
      { stdio: 'inherit' },
    );
    return VS_BUILDTOOLS_EXE;
  }
}

async function installVsComponents(installPath) {
  if (!installPath) {
    throw new Error('Visual Studio Build Tools not found — install from https://visualstudio.microsoft.com/downloads/');
  }

  const components = [];
  if (!hasMsvcArm64(installPath)) components.push(VS_ARM64_COMPONENT);
  if (!hasClang(installPath)) components.push(...VS_LLVM_COMPONENTS);
  if (!components.length) return;

  const add = components.map((c) => `--add ${c}`).join(' ');
  console.log(`  Installing VS components: ${components.length} (may take several minutes)…`);
  const bootstrapper = await ensureVsBuildtoolsBootstrapper();
  const cmd = `"${bootstrapper}" modify --installPath "${installPath}" ${add} --passive --wait --norestart`;
  const r = spawnSync(cmd, { shell: true, stdio: 'inherit', cwd: pkgRoot });
  if (r.status !== 0) {
    throw new Error(
      `VS component install failed (exit ${r.status ?? '?'}). Run PowerShell as Administrator, then: npm run release`,
    );
  }
}

export async function ensureVsMsvc() {
  const installPath = vsInstallPath();
  if (!installPath) {
    console.log('  ⊘ Visual Studio Build Tools not found');
    return installPath;
  }

  const needArm = !hasMsvcArm64(installPath);
  const needClang = !hasClang(installPath);

  if (!needArm && !needClang) {
    console.log('  ✓ MSVC ARM64 + LLVM clang');
    return installPath;
  }

  if (needArm) console.log('  ⊘ MSVC ARM64 tools missing');
  if (needClang) console.log('  ⊘ LLVM clang missing (needed for Windows ARM64)');

  try {
    await installVsComponents(installPath);
  } catch (err) {
    console.log(`  ⊘ ${err.message}`);
    return installPath;
  }

  if (hasMsvcArm64(installPath)) console.log('  ✓ MSVC ARM64 tools installed');
  else console.log('  ⊘ MSVC ARM64 tools still missing');

  if (hasClang(installPath)) console.log('  ✓ LLVM clang installed');
  else console.log('  ⊘ LLVM clang still missing');

  return installPath;
}

export function msvcArchForTarget(target) {
  if (target === 'aarch64-pc-windows-msvc') return 'x86_arm64';
  if (target === 'i686-pc-windows-msvc') return 'x86';
  return 'x64';
}

function cleanPathForMsvc(pathValue) {
  return (pathValue || '')
    .split(';')
    .filter((p) => p && !/\\Git\\usr\\bin/i.test(p) && !/\\Git\\mingw/i.test(p))
    .join(';');
}

export function execWithMsvc(command, { cwd, target, pathExtra = '' } = {}) {
  if (process.platform !== 'win32') {
    execSync(command, { cwd, stdio: 'inherit' });
    return;
  }

  const installPath = vsInstallPath();
  const childEnv = { ...process.env, PATH: cleanPathForMsvc(process.env.PATH || '') };

  if (!installPath) {
    const merged = pathExtra ? prependPath(pathExtra, childEnv.PATH) : childEnv.PATH;
    execSync(command, { cwd, stdio: 'inherit', env: { ...childEnv, PATH: merged } });
    return;
  }

  const vcvars = path.join(installPath, 'VC', 'Auxiliary', 'Build', 'vcvarsall.bat');
  const arch = msvcArchForTarget(target);
  let extraPath = pathExtra;
  if (target === 'aarch64-pc-windows-msvc') {
    const clang = clangBinDir(installPath);
    if (clang) extraPath = prependPath(clang, extraPath);
  }
  const pathSet = extraPath
    ? `set "PATH=${extraPath.replace(/"/g, '""')};%PATH%" && `
    : '';
  const clangSet =
    target === 'aarch64-pc-windows-msvc'
      ? 'set "CC_aarch64_pc_windows_msvc=clang-cl" && set "CXX_aarch64_pc_windows_msvc=clang-cl" && '
      : '';
  const wrapped = `"${vcvars}" ${arch} >nul && ${pathSet}${clangSet}${command}`;
  execSync(wrapped, { cwd, stdio: 'inherit', shell: true, env: childEnv });
}

export async function ensureWindowsBuildTools() {
  if (process.platform !== 'win32') return { pathExtra: '', vsPath: null };

  console.log('  Windows cross-build tools…');
  const pathExtra = await ensureNasm();
  const vsPath = await ensureVsMsvc();
  return { pathExtra, vsPath };
}
