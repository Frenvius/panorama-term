import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';

const isWin = process.platform === 'win32';
const bin = join('sidecar-rs', 'target', 'debug', isWin ? 'sidecar.exe' : 'sidecar');

const build = spawnSync('bun', ['scripts/sidecar.ts'], { stdio: 'inherit', shell: isWin });
if (build.status !== 0 || !existsSync(bin)) {
  console.error('[dev] sidecar build failed');
  process.exit(1);
}

const vite = spawn('bunx', ['vite'], { stdio: 'inherit', shell: isWin });

const shutdown = () => {
  vite.kill();
  process.exit();
};
process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);
vite.on('exit', (code) => process.exit(code ?? 0));
