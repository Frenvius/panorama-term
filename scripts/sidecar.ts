import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

const release = process.argv.includes('--release');
const profile = release ? 'release' : 'debug';
const ext = process.platform === 'win32' ? '.exe' : '';

const win = process.platform === 'win32';
for (const name of ['panorama-host', 'panorama-brain', 'sidecar']) {
  if (win) spawnSync('taskkill', ['/IM', `${name}.exe`, '/F'], { stdio: 'ignore' });
  else spawnSync('pkill', ['-x', name], { stdio: 'ignore' });
}

const args = ['build', '--manifest-path', 'sidecar-rs/Cargo.toml'];
if (release) args.push('--release');

const build = spawnSync('cargo', args, { stdio: 'inherit' });
if (build.status !== 0) process.exit(build.status ?? 1);

const info = spawnSync('rustc', ['-vV'], { encoding: 'utf8' }).stdout ?? '';
const triple = info.match(/host:\s*(\S+)/)?.[1];
if (!triple) {
  console.error('[sidecar] could not resolve host target triple from rustc -vV');
  process.exit(1);
}

const dir = join('src-tauri', 'binaries');
mkdirSync(dir, { recursive: true });
copyFileSync(join('sidecar-rs', 'target', profile, `sidecar${ext}`), join(dir, `sidecar-${triple}${ext}`));
