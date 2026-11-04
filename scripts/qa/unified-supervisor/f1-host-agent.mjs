#!/usr/bin/env bun
// F1 host agent: one per fixture computer. The F1 runner starts it directly on the hub computer
// and as `ssh -T <host> <bun> <repoDir>/scripts/qa/unified-supervisor/f1-host-agent.mjs` on every
// other computer. JSONL RPC on stdin/stdout: {id, op, ...} -> {id, ok, result | error}; it also
// streams unsolicited {event: 'pane', file, record} lines for every fixture pane log line, so the
// runner waits on pane events instead of polling.
//
// Transports (every output line carries a monotonically increasing `oseq`):
// - stdio (no flag; the hub computer's own agent): RPC on this process's stdin/stdout; its
//   stdin closing tears down.
// - --attach --mode start|reattach --data-root <abs> --run-id <id> --after <oseq> --orphan-ms <ms>
//   (what ssh runs on a target): a thin client. `start` launches the detached --serve process
//   (own session, survives the ssh session) and attaches to its unix socket
//   <runRoot>/agent/s; `reattach` only attaches. On attach the server writes
//   {event: 'agent-attached', serverAlive: true, ...} and then replays every line with
//   oseq > after, so lines emitted while no client was attached (a dropped ssh) are delivered
//   exactly once; a request id it has already received is never run twice. When no live server
//   answers, the client serves the server's on-disk record instead (orphan mode,
//   serverAlive: false): the persisted out.jsonl, pane lines the dead server never streamed,
//   proxyLog from disk, and shutdown by recorded daemon identity; nothing else is answered.
// - --serve: the durable agent. All output is also appended to <runRoot>/agent/out.jsonl; the
//   proxy log, daemon records and received request ids are persisted beside it. A client
//   disconnect does not tear down; `shutdown`, SIGTERM, or --orphan-ms with no client attached do.
//
// Ownership: it owns only what it starts here: the fixture provider proxy (in-process, loopback)
// and target daemons it spawns (recorded pid, executable, argv; signalled by that pid's process
// group only). Fixture panes are children of those daemons; a pane still alive after its daemon
// exited is signalled only when its pid comes from its own pane-<pid>.jsonl AND its argv names
// this run's pane script and log dir. Its stdin closing (runner gone, ssh dropped) tears down.
// All state lives under <dataRoot>/runs/<runId>; a dataRoot holding anything else is refused.
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { appendFileSync, chmodSync, existsSync, mkdirSync, openSync, readdirSync, readFileSync, realpathSync, statSync, unlinkSync, watch, writeFileSync } from 'node:fs';
import { createConnection, createServer } from 'node:net';
import { homedir, hostname } from 'node:os';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { paneAlive, readAppends } from './f1-agent-link.mjs';

const SELF = fileURLToPath(import.meta.url);
const PANE = join(dirname(SELF), 'f1-pane.mjs');
const PROTOCOL = 5;
const MAX_SOCKET_PATH_BYTES = 100;
const fail = (code, message) => Object.assign(new Error(message), { code });
const q = (value) => `'${String(value).replace(/'/g, `'\\''`)}'`;

const state = { runRoot: null, agentDir: null, outFile: null, initialized: false, logDir: null, modeFile: null, shell: null, daemons: [], server: null, seen: new Map(), watcher: null, scanTimer: null, panes: 0, env: null };
const proxy = { outageStartedMs: 0, outageUntilMs: 0, quotaResetMs: 0, quotaUntil: new Map(), served: [] };

// Sequenced output: every line gets the next oseq, is kept for replay and (serve/orphan) appended
// to out.jsonl before it is written to the current sink. No sink (serve mode, nothing attached)
// loses nothing: the next attach replays it.
const outLog = [];
let oseq = 0;
let sink = (line) => { process.stdout.write(line); };
function out(value) {
  oseq += 1;
  const line = `${JSON.stringify({ oseq, ...value })}\n`;
  outLog.push({ oseq, line });
  if (state.outFile) appendFileSync(state.outFile, line);
  sink(line);
}
const persist = (name, value) => { if (state.agentDir) appendFileSync(join(state.agentDir, name), `${JSON.stringify(value)}\n`); };
const writeProxyState = () => {
  if (state.agentDir) writeFileSync(join(state.agentDir, 'proxy-state.json'), JSON.stringify({ outage: { startedAtMs: proxy.outageStartedMs, untilMs: proxy.outageUntilMs }, quotaResetMs: proxy.quotaResetMs, quotaUntil: Object.fromEntries(proxy.quotaUntil) }));
};
const readJsonl = (path) => (existsSync(path) ? readFileSync(path, 'utf8').split('\n').filter(Boolean).flatMap((line) => { try { return [JSON.parse(line)]; } catch { return []; } }) : []);
const runRootOf = (dataRoot, runId) => join(resolve(dataRoot), 'runs', runId);
const agentPaths = (runRoot) => ({ agentDir: join(runRoot, 'agent'), socket: join(runRoot, 'agent', 's'), outFile: join(runRoot, 'agent', 'out.jsonl') });

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
    let finish = (fn, value) => { if (done) return; done = true; clearTimeout(timer); try { watcher?.close(); } catch { /* closed */ } fn(value); };
    const evaluate = () => { try { const value = check(); if (value) finish(resolveWait, value); } catch { /* the next change re-reads */ } };
    const timer = setTimeout(() => finish(rejectWait, fail('DEADLINE_EXCEEDED', `${label} was not observed within ${deadlineMs}ms`)), deadlineMs);
    // fs.watch misses appends on some target filesystems (overlay/network homes): a 500 ms
    // re-check backs it up, so a wait never depends on a watch event arriving.
    const backstop = setInterval(evaluate, 500);
    const clear = finish;
    finish = (fn, value) => { clearInterval(backstop); clear(fn, value); };
    mkdirSync(dir, { recursive: true });
    watcher = watch(dir, evaluate);
    evaluate();
  });
}

function scanPanes() {
  if (!state.logDir || !existsSync(state.logDir)) return;
  let moved = false;
  for (const name of readdirSync(state.logDir).filter((entry) => /^pane-\d+\.jsonl$/.test(entry)).sort()) {
    const offset = state.seen.get(name) ?? 0;
    const { lines, end } = readAppends(readFileSync(join(state.logDir, name)), offset);
    for (const { text } of lines) {
      let record;
      try { record = JSON.parse(text); } catch { record = { unparsed: text }; }
      out({ event: 'pane', file: name, record });
    }
    if (end !== offset) { state.seen.set(name, end); moved = true; }
  }
  // The delivered byte offsets survive this process: an orphan reader resumes from them.
  if (moved && state.agentDir) writeFileSync(join(state.agentDir, 'pane-offsets.json'), JSON.stringify(Object.fromEntries(state.seen)));
}

// Pane delivery: fs.watch for promptness plus a bounded 1 s poll, because fs.watch misses
// appends on some target filesystems (run qa-mw1r528u-1bscr-1: lines up to 18 min late).
const PANE_POLL_MS = 1_000;
function watchPanes() {
  state.watcher = watch(state.logDir, scanPanes);
  state.scanTimer = setInterval(scanPanes, PANE_POLL_MS);
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
    if (state.initialized) throw fail('ALREADY_INITIALIZED', 'init runs once');
    if (!/^[A-Za-z0-9._-]{1,32}$/.test(String(runId))) throw fail('CONFIG_INVALID', 'runId');
    if (state.runRoot) {
      // Serve mode: the attach client created the run root (holding only agent/) for this run.
      if (state.runRoot !== runRootOf(dataRoot, runId)) throw fail('CONFIG_INVALID', `init names ${runRootOf(dataRoot, runId)}, this agent serves ${state.runRoot}`);
    } else {
      refuseForeign(dataRoot);
      state.runRoot = runRootOf(dataRoot, runId);
      if (existsSync(state.runRoot)) throw fail('FOREIGN_STATE_ROOT', `${state.runRoot} already exists`);
    }
    state.initialized = true;
    state.logDir = join(state.runRoot, 'panes');
    state.modeFile = join(state.runRoot, 'bin', 'pane-mode');
    state.shell = join(state.runRoot, 'bin', 'f1-shell');
    for (const dir of [state.logDir, dirname(state.shell), join(state.runRoot, 'home'), join(state.runRoot, 'tmp'), join(state.runRoot, 'runtime')]) {
      mkdirSync(dir, { recursive: true, mode: 0o700 });
    }
    const interpreter = bun ?? process.execPath;
    writeFileSync(state.shell, `#!/bin/sh\ncase "$1" in\n  -*c*) shift; exec /bin/sh -c "$1" ;;\nesac\nexec ${q(interpreter)} ${q(PANE)} ${q(state.logDir)} ${q(state.modeFile)}\n`, { mode: 0o755 });
    chmodSync(state.shell, 0o755);
    watchPanes();
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
    writeProxyState();
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
          if (!proxy.quotaUntil.has(label)) { proxy.quotaUntil.set(label, now + proxy.quotaResetMs); writeProxyState(); }
          const until = proxy.quotaUntil.get(label);
          if (now < until) { status = 429; body = { error: 'rate limited', resetMinutes: Math.ceil((until - now) / 60_000), resetAtMs: until }; }
        }
        proxy.served.push({ atMs: now, label, role, status });
        persist('served.jsonl', { atMs: now, label, role, status });
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
    writeProxyState();
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
    persist('daemons.jsonl', { pid: record.pid, executable: ferryxCli, args, startedAtMs: record.startedAtMs });
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

  // The checkout this agent runs from (whole repository, not only this directory) and the
  // ferryx-cli this computer runs: the evidence records each computer's provenance.
  async rev({ ferryxCli } = {}) {
    const dir = dirname(PANE);
    const git = (args) => spawnSync('git', ['-C', dir, ...args], { encoding: 'utf8', timeout: 20_000 });
    const top = git(['rev-parse', '--show-toplevel']);
    const repoRoot = top.status === 0 ? top.stdout.trim() : null;
    const head = git(['rev-parse', 'HEAD']);
    const status = repoRoot ? spawnSync('git', ['-C', repoRoot, 'status', '--porcelain'], { encoding: 'utf8', timeout: 20_000 }) : null;
    const dirty = status?.status === 0 ? (status.stdout.trim() || null) : 'git status failed';
    let binary = null;
    if (typeof ferryxCli === 'string' && isAbsolute(ferryxCli)) {
      try {
        const info = statSync(ferryxCli);
        binary = { path: ferryxCli, sha256: createHash('sha256').update(readFileSync(ferryxCli)).digest('hex'), bytes: info.size, mtimeMs: info.mtimeMs };
      } catch (error) {
        binary = { path: ferryxCli, error: String(error?.message ?? error) };
      }
    }
    return { repoRoot, head: head.stdout.trim() || null, dirty, script: PANE, ferryxCli: binary };
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
const procState = (pid) => { try { return readFileSync(`/proc/${pid}/stat`, 'utf8').replace(/^.*\) /s, '')[0] ?? null; } catch { return null; } };
const cmdlineBytes = (pid) => { try { return readFileSync(`/proc/${pid}/cmdline`).length; } catch { return null; } };
// A pane's teardown observation: liveness by paneAlive, with the raw proc facts recorded.
function paneProbe(pid) {
  const exists = alive(pid);
  const state = exists ? procState(pid) : null;
  const bytes = exists ? cmdlineBytes(pid) : null;
  return { exists, procState: state, cmdlineBytes: bytes, live: paneAlive({ exists, procState: state, cmdlineBytes: bytes }) };
}

async function teardown(reason) {
  try { state.watcher?.close(); } catch { /* closed */ }
  clearInterval(state.scanTimer);
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
    const probe = paneProbe(pid);
    const live = probe.live;
    const argv = live ? argvOf(pid) : null;
    const ours = live && Boolean(argv?.includes(PANE) && argv?.includes(state.logDir));
    if (ours) { try { process.kill(pid, 'SIGTERM'); } catch { /* raced its exit */ } }
    panes.push({ pid, aliveAtTeardown: live, signalled: ours, argv, procState: probe.procState, exiting: probe.exists && !probe.live });
  }
  try { state.server?.stop(true); } catch { /* stopped */ }
  // Small enough for one JSONL reply line: pane logs were already streamed as pane events.
  return { reason, runRoot: state.runRoot, daemons, panes, outage: { startedAtMs: proxy.outageStartedMs, untilMs: proxy.outageUntilMs }, served: proxy.served.length };
}

// Identity of a recorded target daemon this process did not spawn (orphan mode): alive, its
// argv names the recorded executable with --daemon, and (Linux) its cwd is this run root.
function daemonIdentity(record) {
  if (!alive(record.pid)) return { verdict: 'gone' };
  const argv = argvOf(record.pid);
  let cwd = null;
  try { cwd = realpathSync(`/proc/${record.pid}/cwd`); } catch { /* not Linux */ }
  const argvOk = Boolean(argv?.includes(record.executable) && argv?.includes('--daemon'));
  const cwdOk = cwd !== null && cwd === realpathSync(state.runRoot);
  return { verdict: argvOk && cwdOk ? 'match' : 'unverified', argv, cwd };
}

// Orphan teardown: the serving agent is gone, its daemons may not be. Each recorded daemon is
// signalled by process group only when its identity matches; the wait for its exit is bounded.
async function orphanTeardown(reason) {
  try { state.watcher?.close(); } catch { /* closed */ }
  clearInterval(state.scanTimer);
  scanPanes();
  const daemons = [];
  for (const record of readJsonl(join(state.agentDir, 'daemons.jsonl')).reverse()) {
    const receipt = { ...record, reason, orphan: true, signals: [], identity: daemonIdentity(record), exited: null };
    if (receipt.identity.verdict === 'gone') receipt.exited = { observed: 'gone-before-teardown', atMs: Date.now() };
    if (receipt.identity.verdict === 'match') {
      for (const [signal, waitMs] of [['SIGTERM', 10_000], ['SIGKILL', 5_000]]) {
        try { process.kill(-record.pid, signal); receipt.signals.push({ signal, target: 'group', pid: record.pid }); } catch (error) {
          receipt.signals.push({ signal, target: 'group', pid: record.pid, error: String(error?.message ?? error) });
        }
        // Not our child: no exit event exists, so its existence is re-checked within the bound.
        const until = Date.now() + waitMs;
        while (alive(record.pid) && Date.now() < until) await new Promise((r) => setTimeout(r, 200));
        if (!alive(record.pid)) { receipt.exited = { observed: 'gone-after-signal', signal, atMs: Date.now() }; break; }
      }
    }
    daemons.push(receipt);
  }
  const panes = [];
  for (const name of state.logDir && existsSync(state.logDir) ? readdirSync(state.logDir) : []) {
    const pid = Number(name.match(/^pane-(\d+)\.jsonl$/)?.[1]);
    if (!Number.isInteger(pid)) continue;
    const probe = paneProbe(pid);
    const live = probe.live;
    const argv = live ? argvOf(pid) : null;
    const ours = live && Boolean(argv?.includes(PANE) && argv?.includes(state.logDir));
    if (ours) { try { process.kill(pid, 'SIGTERM'); } catch { /* raced its exit */ } }
    panes.push({ pid, aliveAtTeardown: live, signalled: ours, argv, procState: probe.procState, exiting: probe.exists && !probe.live });
  }
  return { reason, orphan: true, runRoot: state.runRoot, daemons, panes, outage: readProxyState().outage, served: readJsonl(join(state.agentDir, 'served.jsonl')).length };
}

const readProxyState = () => {
  const path = join(state.agentDir, 'proxy-state.json');
  return existsSync(path) ? JSON.parse(readFileSync(path, 'utf8')) : { outage: { startedAtMs: 0, untilMs: 0 }, quotaResetMs: 0, quotaUntil: {} };
};

// What an orphan reader answers from the dead server's on-disk record; nothing else.
const orphanOps = {
  async proxyLog() {
    const saved = readProxyState();
    return { served: readJsonl(join(state.agentDir, 'served.jsonl')), quotaUntil: saved.quotaUntil, outage: saved.outage, nowMs: Date.now(), fromDisk: true };
  },
  async shutdown() { return orphanTeardown('shutdown'); },
};

// Request ids already received (persisted in serve mode): a request re-sent after a reconnect
// is never run twice; its reply is in the replay or still to come.
const received = new Set();
let tearing = null;
async function handleRequest(line, table, finishShutdown) {
  let request;
  try { request = JSON.parse(line); } catch { out({ id: null, ok: false, error: { code: 'BAD_JSON', message: line.slice(0, 200) } }); return; }
  if (request.id !== undefined && request.id !== null) {
    if (received.has(request.id)) return;
    received.add(request.id);
    persist('requests.jsonl', { id: request.id, op: request.op, atMs: Date.now() });
  }
  const handler = table[request.op];
  if (!handler) { out({ id: request.id, ok: false, error: { code: table === ops ? 'UNKNOWN_OP' : 'AGENT_ORPHANED', message: table === ops ? String(request.op) : `${request.op}: the serving agent is gone; only proxyLog and shutdown are answered from its on-disk record` } }); return; }
  try {
    const result = request.op === 'shutdown' ? await (tearing ??= handler(request)) : await handler(request);
    out({ id: request.id, ok: true, result });
    if (request.op === 'shutdown') await finishShutdown();
  } catch (error) {
    out({ id: request.id, ok: false, error: { code: error?.code ?? 'ERROR', message: String(error?.message ?? error) } });
  }
}

const argv = process.argv.slice(2);
const flag = (name) => { const index = argv.indexOf(name); return index >= 0 ? argv[index + 1] : undefined; };
const linesOf = (stream, onLine) => {
  let buffer = '';
  stream.setEncoding('utf8');
  stream.on('data', (chunk) => {
    buffer += chunk;
    for (let index = buffer.indexOf('\n'); index >= 0; index = buffer.indexOf('\n')) { const line = buffer.slice(0, index); buffer = buffer.slice(index + 1); if (line.trim()) onLine(line); }
  });
};

function attachArgs() {
  const dataRoot = flag('--data-root');
  const runId = flag('--run-id');
  const after = Number(flag('--after') ?? 0);
  const orphanMs = Number(flag('--orphan-ms') ?? 15 * 60_000);
  if (!/^[A-Za-z0-9._-]{1,32}$/.test(String(runId))) throw fail('CONFIG_INVALID', 'runId');
  if (typeof dataRoot !== 'string' || !isAbsolute(dataRoot) || dataRoot.split('/').includes('..')) throw fail('CONFIG_INVALID', 'dataRoot must be absolute without ..');
  if (!Number.isSafeInteger(after) || after < 0 || !Number.isSafeInteger(orphanMs) || orphanMs <= 0) throw fail('CONFIG_INVALID', '--after / --orphan-ms');
  const runRoot = runRootOf(dataRoot, runId);
  const paths = agentPaths(runRoot);
  if (Buffer.byteLength(paths.socket) > MAX_SOCKET_PATH_BYTES) throw fail('CONFIG_INVALID', `agent socket ${paths.socket} exceeds ${MAX_SOCKET_PATH_BYTES} bytes`);
  return { dataRoot, runId, after, orphanMs, runRoot, paths };
}

// The durable agent: detached, its output on disk, attachable over its unix socket.
function serve() {
  const { runRoot, paths, orphanMs } = attachArgs();
  state.runRoot = runRoot;
  state.agentDir = paths.agentDir;
  state.outFile = paths.outFile;
  for (const record of readJsonl(join(paths.agentDir, 'requests.jsonl'))) received.add(record.id);
  sink = () => {};
  let client = null;
  let orphanTimer = null;
  const exitAfterTeardown = async () => {
    try { unlinkSync(paths.socket); } catch { /* gone */ }
    await new Promise((resolveEnd) => (client ? client.end(resolveEnd) : resolveEnd()));
    process.exit(0);
  };
  const shutdownBy = async (reason) => { out({ event: 'agent-teardown', reason, result: await (tearing ??= teardown(reason)) }); await exitAfterTeardown(); };
  const armOrphan = () => { clearTimeout(orphanTimer); orphanTimer = setTimeout(() => void shutdownBy('orphaned'), orphanMs); };
  const server = createServer((connection) => {
    // One client: a new attach replaces a half-dead link from a dropped ssh session.
    if (client) { try { client.destroy(); } catch { /* gone */ } }
    client = connection;
    clearTimeout(orphanTimer);
    let hello = false;
    linesOf(connection, (line) => {
      if (!hello) {
        hello = true;
        let after = 0;
        try { after = Number(JSON.parse(line).after ?? 0); } catch { /* 0 */ }
        // Synchronous: the attach line, then every line after `after`, then live output.
        connection.write(`${JSON.stringify({ event: 'agent-attached', serverAlive: true, pid: process.pid, hostname: hostname(), after, lastOseq: oseq, atMs: Date.now() })}\n`);
        for (const entry of outLog) if (entry.oseq > after) connection.write(entry.line);
        sink = (text) => { if (client === connection) connection.write(text); };
        return;
      }
      void handleRequest(line, ops, exitAfterTeardown);
    });
    connection.on('error', () => { /* close decides */ });
    connection.on('close', () => { if (client === connection) { client = null; sink = () => {}; armOrphan(); } });
  });
  try { unlinkSync(paths.socket); } catch { /* absent */ }
  server.listen(paths.socket, () => {
    writeFileSync(join(paths.agentDir, 'server.json'), JSON.stringify({ pid: process.pid, hostname: hostname(), startedAtMs: Date.now(), orphanMs }));
    out({ event: 'agent-ready', pid: process.pid, hostname: hostname(), transport: 'serve' });
    armOrphan();
  });
  process.on('SIGHUP', () => { /* detached: a hangup is not a shutdown */ });
  process.on('SIGTERM', () => void shutdownBy('SIGTERM'));
}

// Orphan reader: the serving agent is gone; serve its on-disk record on this ssh session.
function orphan({ runRoot, paths, after }, why) {
  state.runRoot = runRoot;
  state.agentDir = paths.agentDir;
  const persisted = readJsonl(paths.outFile);
  // Only requests the dead server answered are skipped; one it received but never answered is
  // answered here (from disk, or AGENT_ORPHANED), so no caller waits on a reply that cannot come.
  for (const line of persisted) if (line.id !== undefined && line.id !== null && 'ok' in line) received.add(line.id);
  oseq = persisted.reduce((max, line) => Math.max(max, Number(line.oseq) || 0), 0);
  process.stdout.write(`${JSON.stringify({ event: 'agent-attached', serverAlive: false, why, server: existsSync(join(paths.agentDir, 'server.json')) ? JSON.parse(readFileSync(join(paths.agentDir, 'server.json'), 'utf8')) : null, after, lastOseq: oseq, atMs: Date.now() })}\n`);
  for (const line of persisted) if (line.oseq > after) process.stdout.write(`${JSON.stringify(line)}\n`);
  state.outFile = paths.outFile;
  // Pane lines the dead server never streamed, from its last persisted byte offsets on.
  state.logDir = join(runRoot, 'panes');
  const offsets = join(paths.agentDir, 'pane-offsets.json');
  if (existsSync(offsets)) for (const [name, offset] of Object.entries(JSON.parse(readFileSync(offsets, 'utf8')))) state.seen.set(name, offset);
  if (existsSync(state.logDir)) watchPanes();
  const input = createInterface({ input: process.stdin, terminal: false });
  input.on('line', (line) => void handleRequest(line, orphanOps, async () => process.exit(0)));
  input.on('close', () => process.exit(0));
}

async function attach() {
  const args = attachArgs();
  const { runRoot, paths, dataRoot, runId, orphanMs } = args;
  const mode = flag('--mode');
  if (mode === 'start') {
    refuseForeign(dataRoot);
    if (existsSync(runRoot)) throw fail('FOREIGN_STATE_ROOT', `${runRoot} already exists`);
    mkdirSync(paths.agentDir, { recursive: true, mode: 0o700 });
    const log = openSync(join(paths.agentDir, 'serve.log'), 'a');
    const child = spawn(process.execPath, [SELF, '--serve', '--data-root', dataRoot, '--run-id', runId, '--orphan-ms', String(orphanMs)], { cwd: runRoot, detached: true, stdio: ['ignore', log, log] });
    child.unref();
    await waitUntil(paths.agentDir, () => existsSync(paths.socket), 30_000, 'agent socket');
  } else if (mode !== 'reattach') {
    throw fail('CONFIG_INVALID', '--mode start|reattach');
  } else if (!existsSync(paths.agentDir)) {
    throw fail('AGENT_STATE_MISSING', `${paths.agentDir} does not exist; nothing to reattach to`);
  }
  const connection = createConnection(paths.socket);
  let connected = false;
  connection.on('error', (error) => {
    if (connected) return; // close decides
    if (mode === 'start') { process.stderr.write(`attach: ${error?.message ?? error}\n`); process.exit(3); }
    orphan(args, String(error?.code ?? error?.message ?? error));
  });
  connection.once('connect', () => {
    connected = true;
    connection.write(`${JSON.stringify({ after: args.after })}\n`);
    connection.pipe(process.stdout);
    process.stdin.pipe(connection);
    process.stdin.on('end', () => connection.end());
    // Exit only once every relayed line has left this process's stdout.
    connection.on('close', () => process.stdout.write('', () => process.exit(0)));
  });
}

if (argv.includes('--serve')) {
  serve();
} else if (argv.includes('--attach')) {
  attach().catch((error) => { process.stderr.write(`f1-host-agent attach: ${error?.code ?? ''} ${error?.message ?? error}\n`); process.exit(3); });
} else {
  const input = createInterface({ input: process.stdin, terminal: false });
  input.on('line', (line) => void handleRequest(line, ops, async () => process.exit(0)));
  input.on('close', async () => { await (tearing ??= teardown('stdin-closed')); process.exit(0); });
  out({ event: 'agent-ready', pid: process.pid, hostname: hostname(), transport: 'stdio' });
}
