import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { rmSync, mkdirSync, existsSync, copyFileSync, readFileSync, writeFileSync } from 'node:fs';

if (process.platform !== 'win32') {
  console.log('SKIP: update e2e only covers the Windows file-lock failure');
  process.exit(0);
}

const HOST_PORT = 19878;
const BRAIN_PORT = 19877;
const TILE = 'e2e-update';
const BUILT = join(process.cwd(), 'sidecar-rs', 'target', 'debug');
const ROLES = ['panorama-host', 'panorama-brain', 'panorama-cli'];
const CONPTY = ['conpty.dll', 'OpenConsole.exe'];

const scratch = join(tmpdir(), `panorama-update-e2e-${process.pid}`);
const inst = join(scratch, 'inst');
const cfg = join(scratch, 'cfg');
const home = join(scratch, 'home');
const root = join(cfg, 'runtime');
const v1 = join(root, 'v1');
const pointer = join(root, `current-${HOST_PORT}.json`);

const dec = new TextDecoder();
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

const procsUnder = (prefix: string): { pid: number; path: string }[] => {
  const ps = `Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -like '${prefix}*' } | ForEach-Object { "$($_.ProcessId)|$($_.ExecutablePath)" }`;
  const out = spawnSync('powershell', ['-NoProfile', '-Command', ps], { encoding: 'utf8' }).stdout ?? '';
  return out
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => {
      const [pid, path] = line.split('|');
      return { pid: Number(pid), path };
    });
};

const killUnder = (prefix: string) => {
  for (const p of procsUnder(prefix)) spawnSync('taskkill', ['/PID', String(p.pid), '/T', '/F']);
};

const cleanup = () => {
  killUnder(scratch);
  for (let i = 0; i < 20; i++) {
    try {
      rmSync(scratch, { recursive: true, force: true });
      return;
    } catch {
      spawnSync('powershell', ['-NoProfile', '-Command', 'Start-Sleep -Milliseconds 250']);
    }
  }
};

const fail = (msg: string): never => {
  console.error(`FAIL: ${msg}`);
  cleanup();
  process.exit(1);
};

const waitFor = async (check: () => boolean, ms: number) => {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    if (check()) return true;
    await sleep(200);
  }
  return false;
};

const portOpen = async (port: number) => {
  try {
    const s = await Bun.connect({ hostname: '127.0.0.1', port, socket: { data() {} } });
    s.end();
    return true;
  } catch {
    return false;
  }
};

const waitPort = async (port: number, ms: number) => {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    if (await portOpen(port)) return true;
    await sleep(200);
  }
  return false;
};

const readPointer = (): { dir: string; source: string; previous: string | null } | null => {
  try {
    return JSON.parse(readFileSync(pointer, 'utf8'));
  } catch {
    return null;
  }
};

interface Pty {
  ws: WebSocket;
  ready: Promise<{ reused: boolean }>;
  waitText: (needle: string, ms: number) => Promise<boolean>;
}

const connectPty = (): Pty => {
  const ws = new WebSocket(`ws://127.0.0.1:${BRAIN_PORT}/pty?tileId=${TILE}&cols=100&rows=30&cwd=${encodeURIComponent(scratch)}`);
  ws.binaryType = 'arraybuffer';
  let text = '';
  ws.addEventListener('message', (e) => {
    if (typeof e.data === 'string') return;
    const buf = e.data as ArrayBuffer;
    const dv = new DataView(buf);
    if (dv.getUint8(0) !== 1) return;
    text = dec.decode(new Uint8Array(buf, 17, dv.getUint32(13, true)));
  });
  return {
    ws,
    ready: new Promise((res) => {
      ws.addEventListener('message', (e) => {
        if (typeof e.data !== 'string') return;
        const msg = JSON.parse(e.data);
        if (msg.t === 'ready') res({ reused: msg.reused });
      });
    }),
    waitText: (needle, ms) => waitFor(() => text.includes(needle), ms)
  };
};

const openPty = async () => {
  const pty = connectPty();
  await new Promise<void>((res, rej) => {
    pty.ws.addEventListener('open', () => res());
    pty.ws.addEventListener('error', () => rej(new Error('pty ws error')));
  });
  return pty;
};

for (const role of ROLES) {
  if (!existsSync(join(BUILT, `${role}.exe`))) fail(`missing ${role}.exe in ${BUILT}, run bun scripts/sidecar.ts first`);
}
if (await portOpen(BRAIN_PORT)) fail(`port ${BRAIN_PORT} already in use`);

rmSync(scratch, { recursive: true, force: true });
for (const dir of [inst, home, v1]) mkdirSync(dir, { recursive: true });

const installed = [...ROLES.map((r) => `${r}.exe`), ...CONPTY.filter((f) => existsSync(join(BUILT, f)))];
for (const file of installed) copyFileSync(join(BUILT, file), join(inst, file));
for (const role of ROLES) copyFileSync(join(inst, `${role}.exe`), join(v1, `${role}-dev.exe`));
for (const file of CONPTY) if (existsSync(join(inst, file))) copyFileSync(join(inst, file), join(v1, file));
writeFileSync(pointer, JSON.stringify({ dir: v1, source: inst, previous: null }));

const env: Record<string, string> = {};
for (const [k, v] of Object.entries(process.env)) if (v !== undefined && !k.startsWith('PANORAMA_')) env[k] = v;
Object.assign(env, {
  PANORAMA_CONFIG_DIR: cfg,
  PANORAMA_HOST_PORT: String(HOST_PORT),
  PANORAMA_SIDECAR_PORT: String(BRAIN_PORT),
  USERPROFILE: home,
  HOME: home,
  KIMI_CODE_HOME: join(home, 'kimi')
});

const host = Bun.spawn([join(v1, 'panorama-host-dev.exe'), '--daemon'], { env, stdout: 'ignore', stderr: 'ignore' });

if (!(await waitPort(BRAIN_PORT, 20000))) fail('brain never listened');
let pty = await openPty();
await pty.ready;
pty.ws.send(JSON.stringify({ t: 'in', d: 'echo MARK0\r' }));
if (!(await pty.waitText('MARK0', 15000))) fail('shell never echoed MARK0');
console.log('ok: host + brain up from runtime v1, shell types');

const cli = Bun.spawn([join(v1, 'panorama-cli-dev.exe'), 'statusline'], { env, stdin: 'pipe', stdout: 'ignore', stderr: 'ignore' });
await sleep(500);
if (cli.exitCode !== null) fail('long-lived cli exited early');

const fromInstall = procsUnder(inst);
if (fromInstall.length) fail(`processes run from the install dir: ${fromInstall.map((p) => p.path).join(', ')}`);
console.log('ok: nothing runs from the install dir');

for (const file of installed) {
  const path = join(inst, file);
  const bytes = readFileSync(path);
  const next = file.endsWith('.exe') && ROLES.some((r) => file === `${r}.exe`) ? Buffer.concat([bytes, Buffer.alloc(64, 7)]) : bytes;
  try {
    writeFileSync(path, next);
  } catch (e) {
    fail(`could not overwrite installed ${file}: ${(e as Error).message}`);
  }
}
console.log(`ok: overwrote all ${installed.length} installed files while host, brain and cli run`);

const swapped = await waitFor(() => {
  const p = readPointer();
  return !!p && p.dir !== v1 && p.previous === v1;
}, 20000);
if (!swapped) fail(`pointer never moved to a new runtime dir: ${JSON.stringify(readPointer())}`);
const v2 = readPointer()!.dir;

const brainSwapped = await waitFor(() => procsUnder(v2).some((p) => p.path.endsWith('panorama-brain-dev.exe')), 20000);
if (!brainSwapped) fail(`brain never restarted from ${v2}`);
if (!(await waitPort(BRAIN_PORT, 20000))) fail('brain port did not come back after swap');
if (host.exitCode !== null) fail('host exited during the update');
console.log(`ok: brain swapped to ${v2}, host pid ${host.pid} unchanged`);

pty = await openPty();
const ready = await pty.ready;
if (!ready.reused) fail('PTY session was lost across the brain swap');
pty.ws.send(JSON.stringify({ t: 'in', d: 'echo MARK1\r' }));
if (!(await pty.waitText('MARK1', 15000))) fail('reattached shell never echoed MARK1');
console.log('ok: PTY session survived and still types');

if (!existsSync(join(v1, 'panorama-cli-dev.exe'))) fail('old runtime dir removed while its cli still runs');
cli.stdin.end();
await waitFor(() => cli.exitCode !== null, 5000);
console.log('ok: old runtime dir kept while its cli ran');

pty.ws.send(JSON.stringify({ t: 'kill' }));
await sleep(300);
cleanup();
console.log('PASS');
process.exit(0);
