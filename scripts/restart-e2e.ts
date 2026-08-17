import { join } from 'node:path';

const HOST_PORT = 9878;
const BRAIN_PORT = 9877;
const ext = process.platform === 'win32' ? '.exe' : '';
const EXE = join(process.cwd(), 'sidecar-rs', 'target', 'debug', `sidecar${ext}`);
const TILE = 'e2e-restart';
const CWD = process.cwd();

const dec = new TextDecoder();
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

const host = Bun.spawn([EXE, 'host', '--daemon'], {
  env: { ...process.env, PANORAMA_HOST_PORT: String(HOST_PORT), PANORAMA_SIDECAR_PORT: String(BRAIN_PORT) },
  stdout: 'pipe',
  stderr: 'pipe'
});

const waitPort = async (port: number) => {
  for (let i = 0; i < 100; i++) {
    try {
      const s = await Bun.connect({ hostname: '127.0.0.1', port, socket: { data() {} } });
      s.end();
      return true;
    } catch {
      await sleep(200);
    }
  }
  return false;
};

interface Conn {
  ws: WebSocket;
  ready: Promise<{ reused: boolean }>;
  closed: () => boolean;
  exits: number;
  saw: (needle: string) => boolean;
  waitText: (needle: string, ms: number) => Promise<boolean>;
}

const connect = (): Conn => {
  const q = `tileId=${TILE}&cols=100&rows=30&cwd=${encodeURIComponent(CWD)}`;
  const ws = new WebSocket(`ws://127.0.0.1:${BRAIN_PORT}/pty?${q}`);
  ws.binaryType = 'arraybuffer';
  let text = '';
  let closed = false;
  const conn: Conn = {
    ws,
    exits: 0,
    ready: new Promise((res) => {
      ws.addEventListener('message', (e) => {
        if (typeof e.data !== 'string') return;
        const msg = JSON.parse(e.data);
        if (msg.t === 'ready') res({ reused: msg.reused });
      });
    }),
    closed: () => closed,
    saw: (needle) => text.includes(needle),
    waitText: async (needle, ms) => {
      const until = Date.now() + ms;
      while (Date.now() < until) {
        if (text.includes(needle)) return true;
        await sleep(50);
      }
      return false;
    }
  };
  ws.addEventListener('message', (e) => {
    if (typeof e.data === 'string') {
      if (JSON.parse(e.data).t === 'exit') conn.exits++;
      return;
    }
    const buf = e.data as ArrayBuffer;
    const dv = new DataView(buf);
    if (dv.getUint8(0) !== 1) return;
    const textLen = dv.getUint32(13, true);
    text = dec.decode(new Uint8Array(buf, 17, textLen));
  });
  ws.addEventListener('close', () => {
    closed = true;
  });
  return conn;
};

const open = (ws: WebSocket) =>
  new Promise<void>((res, rej) => {
    if (ws.readyState === WebSocket.OPEN) return res();
    ws.addEventListener('open', () => res());
    ws.addEventListener('error', () => rej(new Error('ws error')));
  });

const fail = (msg: string) => {
  console.error(`FAIL: ${msg}`);
  host.kill();
  process.exit(1);
};

if (!(await waitPort(BRAIN_PORT))) fail('brain never listened');

let conn = connect();
await open(conn.ws);
await conn.ready;
conn.ws.send(JSON.stringify({ t: 'in', d: 'echo MARK0\r' }));
if (!(await conn.waitText('MARK0', 15000))) fail('first shell never echoed MARK0');
console.log('ok: initial shell types');

for (let round = 1; round <= 4; round++) {
  const t0 = Date.now();
  conn.ws.send(JSON.stringify({ t: 'kill' }));

  const until = Date.now() + 2000;
  while (!conn.closed() && Date.now() < until) await sleep(20);
  if (!conn.closed()) fail(`round ${round}: server did not close socket after kill`);
  const closeMs = Date.now() - t0;

  conn = connect();
  await open(conn.ws);
  const ready = await conn.ready;
  if (ready.reused) fail(`round ${round}: reconnect reused the killed session`);

  const mark = `MARK${round}`;
  conn.ws.send(JSON.stringify({ t: 'in', d: `echo ${mark}\r` }));
  if (!(await conn.waitText(mark, 20000))) fail(`round ${round}: new shell never echoed ${mark}`);
  await sleep(1200);
  if (conn.exits > 0) fail(`round ${round}: new session got a stray exit`);
  if (conn.closed()) fail(`round ${round}: new session socket closed`);
  console.log(`ok: round ${round} restart (close ${closeMs}ms, fresh shell types, no stray exit)`);
}

conn.ws.send(JSON.stringify({ t: 'kill' }));
await sleep(500);
host.kill();
console.log('PASS');
process.exit(0);
