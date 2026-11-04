#!/usr/bin/env bun
// Run with Bun on an isolated Linux/macOS acceptance host, never the owner's Mac.
import { mkdtemp, mkdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn, execFile } from 'node:child_process';
import { connect } from 'node:net';
import { createInterface } from 'node:readline';
import { once } from 'node:events';
import { promisify } from 'node:util';
import assert from 'node:assert/strict';

const binary = resolve(process.env.FERRYX_CLI ?? 'src-tauri/target/debug/ferryx-cli');
const root = await mkdtemp(join(tmpdir(), 'ferryx-detached-'));
const env = { ...process.env, FERRYX_RUNTIME_DIR: join(root, 'rt'), FERRYX_DATA_DIR: join(root, 'data') };
delete env.FERRYX_SESSION_ID;
const children = [];
const sockets = [];
const timeout = () => AbortSignal.timeout(90_000);
async function launch(legacy) {
  const child = spawn(binary, ['--daemon', ...(legacy ? ['--handover-from', legacy] : [])], { env, stdio: ['ignore', 'pipe', 'pipe'] });
  children.push(child);
  child.stderr.pipe(process.stderr);
  const lines = createInterface({ input: child.stdout });
  await new Promise((resolveReady, reject) => {
    const signal = timeout();
    const abort = () => reject(new Error('daemon readiness timeout'));
    signal.addEventListener('abort', abort, { once: true });
    child.once('error', reject);
    child.once('exit', code => reject(new Error(`daemon exited ${code}`)));
    lines.on('line', line => {
      if (line.includes('FERRYX_DAEMON_READY')) { signal.removeEventListener('abort', abort); resolveReady(); }
    });
  });
  return child;
}
async function client() {
  const socket = connect(join(env.FERRYX_RUNTIME_DIR, 'daemon.sock'));
  sockets.push(socket);
  await once(socket, 'connect', { signal: timeout() });
  const lines = createInterface({ input: socket });
  const pending = [];
  lines.on('line', line => pending.shift()?.(JSON.parse(line)));
  async function request(value) {
    const response = new Promise((resolveResponse, reject) => {
      pending.push(resolveResponse);
      const signal = timeout();
      signal.addEventListener('abort', () => reject(new Error(`request timeout: ${value.type}`)), { once: true });
    });
    socket.write(`${JSON.stringify(value)}\n`);
    return response;
  }
  assert.equal((await request({ type: 'Handshake', version: 5, token: null })).type, 'HandshakeOk');
  return { request, socket };
}
async function cli(...args) {
  const { stdout } = await promisify(execFile)(binary, ['panes', ...args], { env, timeout: 90_000 });
  return JSON.parse(stdout);
}
try {
  await mkdir(join(root, 'repo'));
  await promisify(execFile)('git', ['init', join(root, 'repo')]);
  const predecessor = await launch();
  const a = await client();
  assert.equal((await a.request({ type: 'RegisterWorkspace', workspaceId: 'detached-qa', repoRoot: join(root, 'repo') })).type, 'RegisterWorkspaceOk');
  const spawned = await a.request({ type: 'Spawn', clientRequestId: 'detached-qa', workspaceId: 'detached-qa', cwd: join(root, 'repo'), cols: 80, rows: 24, shell: '/bin/sh' });
  assert.equal(spawned.type, 'SpawnOk');
  const id = spawned.sessionId;
  const view = await a.request({ type: 'LayoutGet', window: 'main' });
  assert.equal((await a.request({ type: 'LayoutApply', window: 'main', baseRevision: view.view.revision, intent: { type: 'ADD_TAB_WITH_SESSION', tabId: 'cat', label: 'cat', content: { kind: 'terminal', backendSessionId: id }, activate: true } })).type, 'LayoutOk');
  const stream = await client();
  const output = createInterface({ input: stream.socket });
  const ready = new Promise((done, reject) => {
    const signal = timeout();
    signal.addEventListener('abort', () => reject(new Error('cat readiness timeout')), { once: true });
    output.on('line', line => { if (line.includes('CAT_READY')) done(); });
  });
  await stream.request({ type: 'Attach', sessionId: id });
  await a.request({ type: 'Write', sessionId: id, data: [...Buffer.from("stty -echo; printf 'CAT_%s\\n' READY; exec cat\n")] });
  await ready;
  await cli('detach', id);
  assert.deepEqual(await cli('list', '--json'), [id]);
  const before = await a.request({ type: 'DescribeSession', sessionId: id });
  await cli('attach', id);
  const b = await client();
  assert.equal((await b.request({ type: 'Write', sessionId: id, data: [...Buffer.from('x\n')] })).type, 'WriteOk');
  await cli('detach', id);
  await cli('attach', id);
  assert.deepEqual(await cli('list', '--json'), []);
  const hashA = await a.request({ type: 'DescribeScreenHash', sessionId: id });
  const hashB = await b.request({ type: 'DescribeScreenHash', sessionId: id });
  assert.deepEqual(hashA, hashB);
  const unknown = await a.request({ type: 'Panes', request: { action: 'attach', session: 'unknown', window: null, split: null } });
  assert.equal(unknown.code, 'SESSION_NOT_FOUND');
  await cli('detach', id);
  const prepared = await a.request({ type: 'PrepareHandover' });
  process.kill(predecessor.pid, 0);
  await launch(prepared.legacySocketPath);
  const successor = await client();
  assert.deepEqual(await cli('list', '--json'), [id]);
  const after = await successor.request({ type: 'DescribeSession', sessionId: id });
  assert.equal(after.details?.pid, before.details?.pid);
  assert.equal((await successor.request({ type: 'Write', sessionId: id, data: [...Buffer.from('alive\n')] })).type, 'WriteOk');
  console.log('DETACH_ROUNDTRIP_OK');
} finally {
  for (const socket of sockets) socket.destroy();
  for (const child of children) if (child.exitCode === null) { const exited = once(child, 'exit'); child.kill(); await exited; }
  await rm(root, { recursive: true, force: true });
}
