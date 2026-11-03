#!/usr/bin/env bun
// F1 host agent: one per fixture computer. The F1 runner starts it directly on the hub computer
// and as `ssh -T <host> <bun> <repoDir>/scripts/qa/unified-supervisor/f1-host-agent.mjs` on every
// other computer. JSONL RPC on stdin/stdout: {id, op, ...} -> {id, ok, result | error}; it also
// streams unsolicited {event: 'pane', file, record} lines for every fixture pane log line, so the
// runner waits on pane events instead of polling.
//
// Ownership: it owns only what it starts here: the fixture provider proxy (in-process, loopback)
// and target daemons it spawns (recorded pid, executable, argv; signalled by that pid's process
// group only). Fixture panes are children of those daemons; a pane still alive after its daemon
// exited is signalled only when its pid comes from its own pane-<pid>.jsonl AND its argv names
// this run's pane script and log dir. Its stdin closing (runner gone, ssh dropped) tears down.
// All state lives under <dataRoot>/runs/<runId>; a dataRoot holding anything else is refused.
import { spawn, spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, readdirSync, readFileSync, statSync, watch, writeFileSync } from 'node:fs';
import { createConnection } from 'node:net';
import { homedir, hostname } from 'node:os';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';

const PANE = join(dirname(fileURLToPath(import.meta.url)), 'f1-pane.mjs');
const PROTOCOL = 5;
const out = (value) => process.stdout.write(`${JSON.stringify(value)}\n`);
const fail = (code, message) => Object.assign(new Error(message), { code });
const q = (value) => `'${String(value).replace(/'/g, `'\\''`)}'`;

const state = { runRoot: null, logDir: null, modeFile: null, shell: null, daemons: [], server: null, seen: new Map(), watcher: null, panes: 0, env: null };
const proxy = { outageStartedMs: 0, outageUntilMs: 0, quotaResetMs: 0, quotaUntil: new Map(), served: [] };

function exchange(socketPath, frames, deadlineMs) {
  return new Promise((resolveExchange, rejectExchange) => {
    const replies = [];
    let buffer = '';
    let next = 0;
    let settled = false;
    const connection = createConnection(socketPath);
    const finish = (fn, value) => { if (settled) return; settled = true; clearTimeout(timer); connection.destroy(); fn(value); };
    const timer = setTimeout(() => finish(rejectExchange, fail('DEADLINE_EXCEEDED', `daemon exchange exceeded ${deadlineMs}ms`)), deadlineMs);
    const send = () => {
      if (next >= frames.length) { finish(resolveExchange, replies); return; }
      connection.write(`${JSON.stringify(frames[next])}\n`);
      next += 1;
    };
    connection.setEncoding('utf8');
    connection.on('connect', send);
    connection.on('error', (error) => finish(rejectExchange, fail('DAEMON_IO', String(error?.message ?? error))));
    connection.on('close', () => finish(rejectExchange, fail('DAEMON_IO', 'the daemon closed the connection before every reply')));
    connection.on('data', (chunk) => {
      buffer += chunk;
      for (let index = buffer.indexOf('\n'); index >= 0 && !settled; index = buffer.indexOf('\n')) {
        const line = buffer.slice(0, index);
        buffer = buffer.slice(index + 1);
        let json = null;
        try { json = JSON.parse(line); } catch { /* kept raw */ }
        replies.push(json ?? { unparsed: line });
        if (json?.type === 'protocolMismatch' || json?.type === 'error') finish(resolveExchange, replies);
        else send();
      }
    });
  });
}

async function daemonCall(socketPath, request, deadlineMs = 60_000) {
  let version = PROTOCOL;
  for (let attempt = 0; attempt < 2; attempt += 1) {
    const replies = await exchange(socketPath, [{ type: 'handshake', version }, request], deadlineMs);
    const mismatch = replies.find((reply) => reply?.type === 'protocolMismatch');
    if (mismatch && attempt === 0 && Number.isInteger(mismatch.expectedVersion)) { version = mismatch.expectedVersion; continue; }
    return replies.at(-1) ?? null;
  }
  return null;
}

// Subscribe to the directory first, then reconcile; bounded by `deadlineMs`.
function waitUntil(dir, check, deadlineMs, label) {
  return new Promise((resolveWait, rejectWait) => {
    let watcher = null;
    let done = false;
    const finish = (fn, value) => { if (done) return; done = true; clearTimeout(timer); try { watcher?.close(); } catch { /* closed */ } fn(value); };
    const evaluate = () => { try { const value = check(); if (value) finish(resolveWait, value); } catch { /* the next change re-reads */ } };
    const timer = setTimeout(() => finish(rejectWait, fail('DEADLINE_EXCEEDED', `${label} was not observed within ${deadlineMs}ms`)), deadlineMs);
    mkdirSync(dir, { recursive: true });
    watcher = watch(dir, evaluate);
    evaluate();
  });
}

function scanPanes() {
  if (!state.logDir || !existsSync(state.logDir)) return;
  for (const name of readdirSync(state.logDir).filter((entry) => /^pane-\d+\.jsonl$/.test(entry)).sort()) {
    const text = readFileSync(join(state.logDir, name), 'utf8');
    const offset = state.seen.get(name) ?? 0;
    const complete = text.lastIndexOf('\n') + 1;
    if (complete <= offset) continue;
    for (const line of text.slice(offset, complete).split('\n').filter(Boolean)) {
      let record;
      try { record = JSON.parse(line); } catch { record = { unparsed: line }; }
      out({ event: 'pane', file: name, record });
    }
    state.seen.set(name, complete);
  }
}

function refuseForeign(dataRoot) {
  if (!isAbsolute(dataRoot) || dataRoot.split('/').includes('..')) throw fail('CONFIG_INVALID', `dataRoot ${dataRoot} must be absolute without ..`);
  const root = resolve(dataRoot);
  if (root === resolve(homedir()) || root === '/' || /(^|\/)\.ferryx(\/|$)/.test(root) || root.includes('hub.sqlite')) {
    throw fail('PRODUCTION_PATH_REFUSED', `dataRoot ${root} names a production location`);
  }
  if (existsSync(root)) {
    if (!statSync(root).isDirectory()) throw fail('FOREIGN_STATE_ROOT', `${root} is not a directory`);
    const foreign = readdirSync(root).filter((name) => name !== 'runs');
    if (foreign.length > 0) throw fail('FOREIGN_STATE_ROOT', `${root} holds state this agent did not create (${foreign.slice(0, 5).join(', ')})`);
  }
}

function daemonEnv(extra = {}) {
  const root = state.runRoot;
  return {
    PATH: process.env.PATH ?? '/usr/bin:/bin', LANG: process.env.LANG ?? 'C.UTF-8', TERM: 'xterm-256color', USER: process.env.USER ?? 'qa',
    HOME: join(root, 'home'), TMPDIR: join(root, 'tmp'), FERRYX_DATA_DIR: join(root, 'data'), FERRYX_RUNTIME_DIR: join(root, 'runtime'),
    FERRYX_HUB_DB: join(root, 'hub', 'hub.sqlite'), FERRYX_HUB: 'off', SHELL: state.shell, ...extra,
  };
}

const socketOf = (socket) => socket ?? join(state.runRoot, 'runtime', 'daemon.sock');

const ops = {
  async init({ runId, dataRoot, bun }) {
    if (state.runRoot) throw fail('ALREADY_INITIALIZED', 'init runs once');
    if (!/^[A-Za-z0-9._-]{1,32}$/.test(String(runId))) throw fail('CONFIG_INVALID', 'runId');
    refuseForeign(dataRoot);
    state.runRoot = join(resolve(dataRoot), 'runs', runId);
    if (existsSync(state.runRoot)) throw fail('FOREIGN_STATE_ROOT', `${state.runRoot} already exists`);
    state.logDir = join(state.runRoot, 'panes');
    state.modeFile = join(state.runRoot, 'bin', 'pane-mode');
    state.shell = join(state.runRoot, 'bin', 'f1-shell');
    for (const dir of [state.logDir, dirname(state.shell), join(state.runRoot, 'home'), join(state.runRoot, 'tmp'), join(state.runRoot, 'runtime')]) {
      mkdirSync(dir, { recursive: true, mode: 0o700 });
    }
    const interpreter = bun ?? process.execPath;
    writeFileSync(state.shell, `#!/bin/sh\ncase "$1" in\n  -*c*) shift; exec /bin/sh -c "$1" ;;\nesac\nexec ${q(interpreter)} ${q(PANE)} ${q(state.logDir)} ${q(state.modeFile)}\n`, { mode: 0o755 });
    chmodSync(state.shell, 0o755);
    state.watcher = watch(state.logDir, scanPanes);
    return { runRoot: state.runRoot, shell: state.shell, hostname: hostname(), agentPid: process.pid, platform: process.platform, nowMs: Date.now() };
  },

  async repo() {
    const repo = join(state.runRoot, 'ws', 'f1-project');
    mkdirSync(repo, { recursive: true });
    const git = (args) => spawnSync('git', ['-c', 'init.defaultBranch=main', '-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid', ...args], { encoding: 'utf8', timeout: 30_000 });
    for (const args of [['init', '-q', repo], ['-C', repo, 'commit', '-q', '--allow-empty', '-m', 'f1 fixture']]) {
      const result = git(args);
      if (result.status !== 0) throw fail('FIXTURE_FAILED', `git ${args.join(' ')}: ${result.stderr}`);
    }
    return { workspaceRoot: join(state.runRoot, 'ws'), repo, gitCommonDir: join(repo, '.git') };
  },

  // The computer's fixture provider proxy: every pane turn on this computer is one request here.
  async proxyStart({ quotaResetMs }) {
    if (state.server) throw fail('ALREADY_STARTED', 'proxy');
    proxy.quotaResetMs = quotaResetMs;
    state.server = Bun.serve({
      hostname: '127.0.0.1',
      port: 0,
      fetch(request) {
        const url = new URL(request.url);
        const label = url.searchParams.get('label') ?? '';
        const role = url.searchParams.get('role') ?? '';
        const now = Date.now();
        let status = 200;
        let body = { ok: true };
        if (role === 'outage' && now < proxy.outageUntilMs) {
          status = 502;
          body = { error: 'bad gateway', outageEndsInMs: proxy.outageUntilMs - now };
        } else if (role === 'quota') {
          if (!proxy.quotaUntil.has(label)) proxy.quotaUntil.set(label, now + proxy.quotaResetMs);
          const until = proxy.quotaUntil.get(label);
          if (now < until) { status = 429; body = { error: 'rate limited', resetMinutes: Math.ceil((until - now) / 60_000), resetAtMs: until }; }
        }
        proxy.served.push({ atMs: now, label, role, status });
        return Response.json(body, { status });
      },
    });
    return { url: `http://127.0.0.1:${state.server.port}` };
  },

  // Starts the outage on this computer's own clock; the proxy answers 502 to outage turns until
  // startedAtMs + durationMs. Time is the behavior under test: nothing shortens it.
  async outage({ durationMs }) {
    proxy.outageStartedMs = Date.now();
    proxy.outageUntilMs = proxy.outageStartedMs + durationMs;
    return { startedAtMs: proxy.outageStartedMs, untilMs: proxy.outageUntilMs };
  },

  async proxyLog() {
    return { served: proxy.served, quotaUntil: Object.fromEntries(proxy.quotaUntil), outage: { startedAtMs: proxy.outageStartedMs, untilMs: proxy.outageUntilMs }, nowMs: Date.now() };
  },

  // A target daemon with its hub off, every root inside this run root, SHELL = the pane wrapper.
  async daemonStart({ ferryxCli, env: extra = {}, deadlineMs = 60_000 }) {
    if (!isAbsolute(ferryxCli)) throw fail('CONFIG_INVALID', 'ferryxCli must be absolute');
    const env = daemonEnv(extra);
    state.env = env;
    const root = state.runRoot;
    const socketPath = join(root, 'runtime', 'daemon.sock');
    const args = ['--daemon'];
    const child = spawn(ferryxCli, args, { cwd: root, env, detached: true, stdio: ['ignore', 'ignore', 'ignore'] });
    const record = { pid: child.pid, executable: ferryxCli, args, startedAtMs: Date.now(), exited: null };
    child.on('exit', (code, signal) => { record.exited = { code, signal, atMs: Date.now() }; });
    state.daemons.push({ child, record });
    await waitUntil(dirname(socketPath), () => existsSync(socketPath), deadlineMs, 'target daemon socket');
    return { ...record, socket: socketPath, log: join(root, 'data', 'logs', 'daemon.log') };
  },

  // The fixture pane mode for the next pane this computer's daemon starts by any path (the
  // hub's machine API create, which the runner drives over HTTP).
  async setMode({ role, label, proxyUrl }) {
    writeFileSync(state.modeFile, `${role} ${label} ${proxyUrl}\n`);
    return { role, label };
  },

  // The last bytes of this computer's target daemon log (kept under the 64 KiB line cap).
  async daemonLogTail() {
    const logPath = join(state.runRoot, 'data', 'logs', 'daemon.log');
    return { text: existsSync(logPath) ? readFileSync(logPath, 'utf8').slice(-40_000) : null };
  },

  // The checkout this agent runs from: the evidence records each computer's revision.
  async rev() {
    const dir = dirname(PANE);
    const head = spawnSync('git', ['-C', dir, 'rev-parse', 'HEAD'], { encoding: 'utf8', timeout: 10_000 });
    const dirty = spawnSync('git', ['-C', dir, 'status', '--porcelain', '--', '.'], { encoding: 'utf8', timeout: 10_000 });
    return { head: head.stdout.trim() || null, dirty: dirty.stdout.trim() || null, script: PANE };
  },

  async call({ socket, request, deadlineMs }) {
    return daemonCall(socketOf(socket), request, deadlineMs);
  },

  async waitLog({ path, needle, min = 1, deadlineMs = 60_000 }) {
    const count = () => (existsSync(path) ? readFileSync(path, 'utf8').split('\n').filter((line) => line.includes(needle)).length : 0);
    await waitUntil(dirname(path), () => count() >= min, deadlineMs, `${needle} x${min}`);
    return { count: count() };
  },

  // One fixture pane through the daemon protocol the GUI uses. The runner serializes spawns per
  // computer, so the mode file names this pane's role while its shell starts.
  async spawnPane({ socket, workspaceId, cwd, role, label, proxyUrl, deadlineMs = 60_000 }) {
    writeFileSync(state.modeFile, `${role} ${label} ${proxyUrl}\n`);
    state.panes += 1;
    const reply = await daemonCall(socketOf(socket), { type: 'spawn', clientRequestId: `f1-${label}-${state.panes}`, workspaceId, worktree: null, cwd, cols: 120, rows: 40 }, deadlineMs);
    const sessionId = reply?.type === 'spawnOk' ? reply.sessionId : null;
    if (!sessionId) throw fail('FIXTURE_FAILED', `pane ${label} was not spawned: ${JSON.stringify(reply)}`);
    const ready = () => readdirSync(state.logDir).filter((name) => name.endsWith('.jsonl')).some((name) => {
      const text = readFileSync(join(state.logDir, name), 'utf8');
      return text.includes('"kind":"ready"') && text.includes(`"sessionId":"${sessionId}"`);
    });
    await waitUntil(state.logDir, ready, deadlineMs, `pane ${label} ready`);
    return { sessionId, label, role, spawnedAtMs: Date.now() };
  },

  // Free only when nothing answers a loopback connect: the product binds the gateway on a fixed
  // port, and nothing on this computer is stopped to free it.
  async portFree({ port }) {
    const taken = await new Promise((resolvePort) => {
      const probe = createConnection({ host: '127.0.0.1', port });
      const done = (value) => { probe.destroy(); resolvePort(value); };
      probe.setTimeout(2_000, () => done(false));
      probe.once('connect', () => done(true));
      probe.once('error', () => done(false));
    });
    return { port, free: !taken };
  },

  async shutdown() { return teardown('shutdown'); },
};

function argvOf(pid) {
  try { return readFileSync(`/proc/${pid}/cmdline`, 'utf8').split('\0').filter(Boolean).join(' '); } catch { /* not Linux */ }
  const ps = spawnSync('ps', ['-o', 'args=', '-p', String(pid)], { encoding: 'utf8', timeout: 5_000 });
  return ps.status === 0 ? ps.stdout.trim() : null;
}

const alive = (pid) => { try { process.kill(pid, 0); return true; } catch (error) { return error?.code === 'EPERM'; } };

async function teardown(reason) {
  try { state.watcher?.close(); } catch { /* closed */ }
  scanPanes();
  const daemons = [];
  for (const { child, record } of [...state.daemons].reverse()) {
    const receipt = { ...record, reason, signals: [] };
    for (const [signal, waitMs] of [['SIGTERM', 10_000], ['SIGKILL', 5_000]]) {
      if (record.exited) break;
      try { process.kill(-record.pid, signal); receipt.signals.push({ signal, target: 'group', pid: record.pid }); } catch (error) {
        receipt.signals.push({ signal, target: 'group', pid: record.pid, error: String(error?.message ?? error) });
      }
      await new Promise((resolveExit) => {
        if (record.exited) { resolveExit(); return; }
        const timer = setTimeout(resolveExit, waitMs);
        child.once('exit', () => { clearTimeout(timer); resolveExit(); });
      });
    }
    receipt.exited = record.exited;
    daemons.push(receipt);
  }
  const panes = [];
  for (const name of state.logDir && existsSync(state.logDir) ? readdirSync(state.logDir) : []) {
    const pid = Number(name.match(/^pane-(\d+)\.jsonl$/)?.[1]);
    if (!Number.isInteger(pid)) continue;
    const live = alive(pid);
    const argv = live ? argvOf(pid) : null;
    const ours = live && Boolean(argv?.includes(PANE) && argv?.includes(state.logDir));
    if (ours) { try { process.kill(pid, 'SIGTERM'); } catch { /* raced its exit */ } }
    panes.push({ pid, aliveAtTeardown: live, signalled: ours, argv });
  }
  try { state.server?.stop(true); } catch { /* stopped */ }
  // Small enough for one JSONL reply line: pane logs were already streamed as pane events.
  return { reason, runRoot: state.runRoot, daemons, panes, outage: { startedAtMs: proxy.outageStartedMs, untilMs: proxy.outageUntilMs }, served: proxy.served.length };
}

let tearing = null;
const input = createInterface({ input: process.stdin, terminal: false });
input.on('line', async (line) => {
  let request;
  try { request = JSON.parse(line); } catch { out({ id: null, ok: false, error: { code: 'BAD_JSON', message: line.slice(0, 200) } }); return; }
  const handler = ops[request.op];
  if (!handler) { out({ id: request.id, ok: false, error: { code: 'UNKNOWN_OP', message: String(request.op) } }); return; }
  try {
    const result = request.op === 'shutdown' ? await (tearing ??= handler(request)) : await handler(request);
    out({ id: request.id, ok: true, result });
    if (request.op === 'shutdown') process.exit(0);
  } catch (error) {
    out({ id: request.id, ok: false, error: { code: error?.code ?? 'ERROR', message: String(error?.message ?? error) } });
  }
});
input.on('close', async () => { await (tearing ??= teardown('stdin-closed')); process.exit(0); });
out({ event: 'agent-ready', pid: process.pid, hostname: hostname() });
