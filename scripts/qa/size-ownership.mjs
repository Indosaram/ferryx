// RUN@linux: bun scripts/qa/size-ownership.mjs [--binary path/to/ferryx-cli]
import net from 'node:net';
import { once } from 'node:events';
import { createInterface } from 'node:readline';
import { mkdtemp, mkdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';

const args = process.argv.slice(2);
const binary = resolve(args.includes('--binary') ? args[args.indexOf('--binary') + 1] : 'src-tauri/target/debug/ferryx-cli');
const root = await mkdtemp(join(tmpdir(), 'ferryx-size-owner-'));
const runtime = join(root, 'rt');
await mkdir(runtime);
const signal = AbortSignal.timeout(90_000);
const daemon = spawn(binary, ['--daemon'], { env: { ...process.env,
  FERRYX_RUNTIME_DIR: runtime, FERRYX_DATA_DIR: join(root, 'data'), FERRYX_ALLOW_INSECURE_DIRECT: '1' },
  stdio: ['ignore', 'pipe', 'pipe'] });
daemon.stderr.pipe(process.stderr);
const sockets = [];
const clients = [];
async function connect() {
  const socket = net.createConnection(join(runtime, 'daemon.sock'));
  clients.push(socket);
  await once(socket, 'connect', { signal });
  const lines = createInterface({ input: socket });
  const call = async request => {
    const answer = once(lines, 'line', { signal });
    socket.write(`${JSON.stringify(request)}\n`);
    const reply = JSON.parse((await answer)[0]);
    if (reply.type === 'error') throw new Error(JSON.stringify(reply));
    return reply;
  };
  await call({ type: 'handshake', version: 5 });
  return { call, lines };
}
let base;
async function post(path, body, token) {
  const response = await fetch(`${base}${path}`, { method: 'POST', signal,
    headers: { 'Content-Type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
    body: JSON.stringify(body) });
  if (!response.ok) throw new Error(`${response.status}: ${await response.text()}`);
  return response.json();
}
try {
  const stdout = createInterface({ input: daemon.stdout });
  signal.addEventListener('abort', () => stdout.close(), { once: true });
  let ready = false;
  for await (const line of stdout) { if (line === 'FERRYX_DAEMON_READY') { ready = true; break; } }
  assert(ready, 'daemon readiness');
  const client = await connect();
  await client.call({ type: 'registerWorkspace', workspaceId: 'size-qa', repoRoot: process.cwd() });
  const spawned = await client.call({ type: 'spawn', workspaceId: 'size-qa', clientRequestId: 'size-qa',
    cols: 120, rows: 40, cwd: process.cwd(), shell: '/bin/sh' });
  const sessionId = spawned.sessionId;
  await client.call({ type: 'resize', sessionId, cols: 120, rows: 40 });
  await client.call({ type: 'remoteConfigure', config: { mode: 'localNetwork', port: 0, allowControl: true } });
  const { status } = await client.call({ type: 'remoteGetStatus' });
  base = `http://127.0.0.1:${status.port}`;
  await client.call({ type: 'remoteSetActiveSelection', selection: { sessionId } });
  const events = await connect();
  await events.call({ type: 'subscribeRemoteEvents' });
  function ownerEvent(owner) {
    return new Promise((resolveEvent, reject) => {
      const abort = () => { events.lines.off('line', receive); reject(signal.reason); };
      const receive = line => {
        const event = JSON.parse(line);
        if (event.event !== 'SizeOwnerChanged' || event.payload.sessionId !== sessionId || !owner(event.payload.owner)) return;
        events.lines.off('line', receive); signal.removeEventListener('abort', abort); resolveEvent(event.payload);
      };
      events.lines.on('line', receive); signal.addEventListener('abort', abort, { once: true });
    });
  }
  async function attach(permission) {
    const pairing = await client.call({ type: 'remoteCreatePairingCode', permission });
    const { token } = await post('/api/v1/pair/exchange', { code: pairing.code, deviceName: 'Size QA' });
    const target = `/api/v1/terminal/${encodeURIComponent(sessionId)}`;
    const { ticket } = await post('/api/v1/socket-ticket', { target }, token);
    const socket = new WebSocket(`${base.replace('http:', 'ws:')}${target}?ticket=${encodeURIComponent(ticket)}&render=grid&cols=60&rows=30`);
    sockets.push(socket);
    await once(socket, 'open', { signal });
    return socket;
  }
  const taken = ownerEvent(owner => owner.startsWith('phone:'));
  const phone = await attach('control');
  await taken;
  let { session } = await client.call({ type: 'describeSession', sessionId });
  assert.deepEqual([session.cols, session.rows], [60, 30]);
  assert(session.sizeOwner.startsWith('phone:'));
  const restored = ownerEvent(owner => owner === 'desktop');
  phone.close();
  await restored;
  ({ session } = await client.call({ type: 'describeSession', sessionId }));
  assert.deepEqual([session.cols, session.rows, session.sizeOwner], [120, 40, 'desktop']);
  const viewer = await attach('view');
  const closed = once(viewer, 'close', { signal });
  viewer.send(JSON.stringify({ type: 'resize', cols: 10, rows: 10 }));
  viewer.close(); // Close is ordered after resize on the socket receive loop.
  await closed;
  ({ session } = await client.call({ type: 'describeSession', sessionId }));
  assert.deepEqual([session.cols, session.rows, session.sizeOwner], [120, 40, 'desktop']);
  console.log('OWNERSHIP_OK');
} finally {
  for (const socket of sockets) socket.close();
  for (const client of clients) client.destroy();
  const exited = once(daemon, 'exit');
  if (daemon.exitCode === null) { daemon.kill('SIGTERM'); await exited; }
  await rm(root, { recursive: true, force: true });
}
