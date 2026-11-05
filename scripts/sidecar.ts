import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { rmSync, existsSync, mkdirSync, copyFileSync, writeFileSync } from 'node:fs';

const release = process.argv.includes('--release');
const profile = release ? 'release' : 'debug';
const ext = process.platform === 'win32' ? '.exe' : '';

const win = process.platform === 'win32';
const kill = (name: string) => {
  if (win) spawnSync('taskkill', ['/IM', `${name}.exe`, '/F'], { stdio: 'ignore' });
  else spawnSync('pkill', ['-x', name], { stdio: 'ignore' });
};
// the installed release app also runs as sidecar.exe, so only the binary cargo is about to overwrite may die
const killBuilt = () => {
  const bin = resolve('sidecar-rs', 'target', profile, `sidecar${ext}`);
  if (win) {
    const script = `Get-Process sidecar -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq '${bin}' } | Stop-Process -Force`;
    spawnSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', script], { stdio: 'ignore' });
  } else spawnSync('pkill', ['-f', bin], { stdio: 'ignore' });
};

killBuilt();

const info = spawnSync('rustc', ['-vV'], { encoding: 'utf8' }).stdout ?? '';
const host = info.match(/host:\s*(\S+)/)?.[1];
if (!host) {
  console.error('[sidecar] could not resolve host target triple from rustc -vV');
  process.exit(1);
}
const triple = process.env.TAURI_ENV_TARGET_TRIPLE || host;
const cross = triple !== host;

const args = ['build', '--manifest-path', 'sidecar-rs/Cargo.toml'];
if (release) args.push('--release');
if (cross) args.push('--target', triple);

const build = spawnSync('cargo', args, { stdio: 'inherit' });
if (build.status !== 0) process.exit(build.status ?? 1);

if (!release) kill('panorama-brain');

const out = cross ? join('sidecar-rs', 'target', triple, profile) : join('sidecar-rs', 'target', profile);
const dir = join('src-tauri', 'binaries');
mkdirSync(dir, { recursive: true });
copyFileSync(join(out, `sidecar${ext}`), join(dir, `sidecar-${triple}${ext}`));

// the ConPTY inside Windows 10 drops mouse-tracking escapes, so ship a current one beside the host
const CONPTY_VERSION = '1.24.261001001';
const CONPTY_FILES = ['conpty.dll', 'OpenConsole.exe'];

const conptyArch = (target: string) => {
  if (target.startsWith('aarch64')) return 'arm64';
  if (target.startsWith('i686')) return 'x86';
  return 'x64';
};

const fetchConpty = async (dest: string, arch: string) => {
  const stamp = join(dest, `${CONPTY_VERSION}-${arch}`);
  if (existsSync(stamp)) return;
  const url = `https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/${CONPTY_VERSION}/microsoft.windows.console.conpty.${CONPTY_VERSION}.nupkg`;
  const res = await fetch(url);
  if (!res.ok) throw new Error(`[sidecar] conpty download failed: ${res.status} ${url}`);
  const unpack = join(dest, 'pkg');
  rmSync(dest, { recursive: true, force: true });
  mkdirSync(unpack, { recursive: true });
  const nupkg = join(dest, 'conpty.zip');
  writeFileSync(nupkg, Buffer.from(await res.arrayBuffer()));
  const bsdtar = join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe');
  const tar = spawnSync(bsdtar, ['-xf', nupkg, '-C', unpack], { stdio: 'inherit' });
  if (tar.status !== 0) throw new Error('[sidecar] conpty unpack failed');
  copyFileSync(join(unpack, 'runtimes', `win-${arch}`, 'native', 'conpty.dll'), join(dest, 'conpty.dll'));
  copyFileSync(join(unpack, 'build', 'native', 'runtimes', arch, 'OpenConsole.exe'), join(dest, 'OpenConsole.exe'));
  rmSync(unpack, { recursive: true, force: true });
  rmSync(nupkg, { force: true });
  writeFileSync(stamp, '');
};

if (triple.includes('windows')) {
  const conptyDir = join(dir, 'conpty');
  await fetchConpty(conptyDir, conptyArch(triple));
  for (const file of CONPTY_FILES) copyFileSync(join(conptyDir, file), join(out, file));
}
