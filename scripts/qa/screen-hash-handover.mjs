// RUN@linux: bun scripts/qa/screen-hash-handover.mjs --binary <first-build> --successor <second-build>
// Different vtVersion values legitimately compare textHash instead of the attribute grid hash.
import net from 'node:net';
import { once } from 'node:events';
import { createInterface } from 'node:readline';
import { mkdtemp, mkdir, rm } from 'node:fs/promises';
import { watch } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn } from 'node:child_process';

const args = process.argv.slice(2);
const binary = resolve(args[args.indexOf('--binary') + 1]);
const successor = resolve(args[args.indexOf('--successor') + 1]);
if (!args.includes('--binary') || !args.includes('--successor')) throw new Error('--binary and --successor required');
const root = await mkdtemp(join(tmpdir(), 'ferryx-screen-hash-'));
const runtime = join(root, 'rt');
await mkdir(runtime);
const signal = AbortSignal.timeout(120_000);
const pids = new Set();
const daemon = spawn(binary, ['--daemon'], {
  env: { ...process.env, FERRYX_RUNTIME_DIR: runtime, FERRYX_DATA_DIR: join(root, 'data') },
  stdio: ['ignore', 'pipe', 'pipe'],
});
pids.add(daemon.pid);
daemon.stderr.pipe(process.stderr);
const output = createInterface({ input: daemon.stdout });

async function connection() {
  const socket = net.createConnection(join(runtime, 'daemon.sock'));
  await once(socket, 'connect', { signal });
  const lines = createInterface({ input: socket });
  const send = async (request) => {
    const reply = once(lines, 'line', { signal });
    socket.write(`${JSON.stringify(request)}\n`);
    const answer = JSON.parse((await reply)[0]);
    if (answer.type === 'error') throw new Error(JSON.stringify(answer));
    return answer;
  };
  let tail = Promise.resolve();
  const call = (request) => {
    const reply = tail.then(() => send(request));
    tail = reply;
    return reply;
  };
  const handshake = await call({ type: 'handshake', version: 5 });
  pids.add(handshake.pid);
  return { socket, lines, call, pid: handshake.pid, close() { lines.close(); socket.destroy(); } };
}

async function fill(client, index) {
  const spawned = await client.call({ type: 'spawn', workspaceId: 'hash-qa',
    clientRequestId: `hash-${index}`, cols: 300, rows: 24, cwd: process.cwd(), shell: '/bin/sh' });
  const sessionId = spawned.sessionId;
  if (!sessionId) throw new Error(JSON.stringify(spawned));
  await client.call({ type: 'setScrollbackBudget', sessionId, lines: 10_000 });
  const attach = await connection();
  await attach.call({ type: 'attach', sessionId });
  // Subscribe before writing, and match executed output, not terminal echo of the command.
  const marker = `HASH_DONE_${index}`;
  const done = new Promise((resolveDone, reject) => {
    let text = '';
    const abort = () => reject(signal.reason);
    signal.addEventListener('abort', abort, { once: true });
    attach.lines.on('line', (line) => {
      const message = JSON.parse(line);
      if (message.type !== 'output') return;
      text += Buffer.from(message.data, 'base64').toString();
      if (text.includes(marker)) {
        signal.removeEventListener('abort', abort);
        resolveDone();
      }
    });
  });
  const command = `stty -echo; seq 1 12000; printf '\\e[1;31mRED\\e[0m'; printf 'HASH_%s_${index}' DONE\n`;
  await client.call({ type: 'write', sessionId, data: Buffer.from(command).toString('base64') });
  await done;
  attach.close();
  return sessionId;
}

async function handover(client, sessions) {
  const before = await Promise.all(sessions.map(sessionId => client.call({ type: 'describeScreenHash', sessionId })));
  // Subscribe to the socket replacement before triggering upgrade; no polling or fixed sleeps.
  const watcher = watch(runtime);
  const replacement = (async () => {
    for await (const [event, name] of (async function* () {
      while (true) yield await once(watcher, 'change', { signal });
    })()) {
      if (event !== 'rename' || String(name) !== 'daemon.sock') continue;
      const next = await connection();
      if (next.pid !== client.pid) return next;
      next.close();
    }
  })();
  try {
    const reply = await client.call({ type: 'upgradeBinary', newBinaryPath: successor });
    if (reply.type !== 'upgradeScheduled') throw new Error(JSON.stringify(reply));
    const next = await replacement;
    client.close();
    for (let i = 0; i < sessions.length; i++) {
      const after = await next.call({ type: 'describeScreenHash', sessionId: sessions[i] });
      const field = before[i].vtVersion === after.vtVersion ? 'hash' : 'textHash';
      if (!before[i][field] || before[i][field] !== after[field] || after.retainedLines !== 10_000) {
        throw new Error(`hash mismatch: ${JSON.stringify({ before: before[i], after })}`);
      }
    }
    console.log(`HASH_EQUAL=true RETAINED=10000 SESSIONS=${sessions.length}`);
    return next;
  } finally { watcher.close(); }
}

let client;
try {
  let ready = false;
  signal.addEventListener('abort', () => output.close(), { once: true });
  for await (const line of output) {
    if (line === 'FERRYX_DAEMON_READY') { ready = true; break; }
  }
  if (!ready) throw new Error('daemon exited or timed out before readiness');
  client = await connection();
  await client.call({ type: 'registerWorkspace', workspaceId: 'hash-qa', repoRoot: process.cwd() });
  const sessions = [await fill(client, 0)];
  client = await handover(client, sessions);
  sessions.push(...await Promise.all([1, 2, 3].map(index => fill(client, index))));
  client = await handover(client, sessions);
} finally {
  client?.close();
  for (const pid of pids) {
    try { process.kill(pid, 'SIGTERM'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
  }
  await rm(root, { recursive: true, force: true });
}
