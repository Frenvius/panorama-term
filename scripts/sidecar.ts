import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';

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
