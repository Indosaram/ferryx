#!/usr/bin/env bun
import { runGit } from './git-env.mjs';
// F4 status-truth driver (docs/plans/SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md, final
// verification F4): headless, against isolated live daemons built from this revision.
//
//   bun scripts/qa/unified-supervisor/f4-status-truth.mjs --ferryx-cli <abs debug ferryx-cli> \
//     --root <abs empty dir, short path> --out <abs empty evidence dir>
//
// Clauses and the product read paths their verdicts use (never pane text, never log text):
//   1 failed-within-5s: a fixture agent (`claude`, its own provider reports) is SIGKILLed while
//     its last report says working. Verdict: the first owner `panel.session` frame on
//     /api/v1/events whose session carries health `failed`, timed from the kill; the same session
//     on GET /api/v1/fleet/sessions; and the panel UI (ui/src PanelSurface rendered from that
//     snapshot) showing the row with data-state="failed". No engine is installed, so no
//     supervisor text exists to consult (converse answers MHC_NOT_RUNNING, recorded).
//   2 stop-with-hub-down: a working fixture session is stopped through
//     POST /api/v1/sessions/{ref}/stop (a) after `ferryx-cli hub stop` on the hub daemon and
//     (b) on a second daemon whose hub never opened (FERRYX_HUB=off). Verdict: the response's
//     receipt, then a `panel.session` frame and the fleet read showing the session out of
//     Working, with the PTY still alive (Stop ends the work, not the terminal).
//   3 unverified-claim: the recovery scheduler handles a fixture 502 on its own and writes a
//     claim row (`retried`/`resumed`/...) whose evidence holds no delivery event (receipt,
//     delivery, stateTransition, commit). Verdict: the row read from GET /api/v1/supervisor/panel
//     and its `panel.action` frame, and the panel UI rendering that row with "Unverified".
//
// Waits are event driven (owner socket frames, directory watches) with bounded deadlines. The
// only fixed waits are product constants under test: the foreground observer's full-listing
// refresh (2 s, waited twice so it has seen the agent before the kill) and the agent-control
// human-typing window (3 s after the driver's own keystrokes, before Stop).
// Exit: 0 all clauses PASS, 1 a clause FAILED, 4 harness error. Evidence is written either way.
import { spawn, spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { appendFileSync, copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, watch, writeFileSync } from 'node:fs';
import { createConnection } from 'node:net';
import { homedir } from 'node:os';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const PANE = join(HERE, 'f4-pane.mjs');
const PROTOCOL = 5;
const WORKSPACE = 'f4-ws';
const FAILED_WITHIN_MS = 5_000;
const OBSERVER_REFRESH_MS = 2_000;
const HUMAN_TYPING_WINDOW_MS = 3_000;
// A panel.session frame's state: the fleet entry when the owner frame carries one, else the
// observation's own state word.
const frameState = (frame) => frame?.session?.state ?? frame?.state ?? null;
const DELIVERY_KINDS = new Set(['receipt', 'delivery', 'stateTransition', 'commit']);

const args = Object.fromEntries(process.argv.slice(2).reduce((pairs, arg, index, all) => (arg.startsWith('--') ? [...pairs, [arg.slice(2), all[index + 1]]] : pairs), []));
for (const key of ['ferryx-cli', 'root', 'out']) {
  if (!args[key] || !isAbsolute(args[key])) { console.error(`--${key} <absolute path> is required`); process.exit(2); }
}
const root = resolve(args.root);
const out = resolve(args.out);
if (root === resolve(homedir()) || /(^|\/)\.ferryx(\/|$)/.test(root) || (existsSync(root) && readdirSync(root).length > 0)) {
  console.error(`--root ${root} must be a new or empty task-owned directory`); process.exit(2);
}
if (existsSync(out) && readdirSync(out).length > 0) { console.error(`--out ${out} must be empty`); process.exit(2); }
mkdirSync(root, { recursive: true, mode: 0o700 });
mkdirSync(out, { recursive: true });

const now = () => Date.now();
const fail = (code, message) => Object.assign(new Error(message), { code });
const jsonl = (name, value) => appendFileSync(join(out, name), `${JSON.stringify({ atMs: now(), at: new Date().toISOString(), ...value })}\n`);
const evidence = { schema: 'f4-status-truth/1', startedAtMs: now(), repo: null, binary: null, clauses: [], daemons: [], teardown: [] };
const clause = (name, pass, detail) => {
  const entry = { clause: name, pass: pass === true, atMs: now(), detail };
  evidence.clauses.push(entry);
  jsonl('clauses.jsonl', entry);
  console.log(`${entry.pass ? 'PASS' : 'FAIL'} ${name}`);
  return entry.pass;
};

// --- provenance -------------------------------------------------------------------------------
const git = (gitArgs) => runGit(['-C', REPO, ...gitArgs], { encoding: 'utf8' }).stdout.trim();
evidence.repo = { head: git(['rev-parse', 'HEAD']), dirty: git(['status', '--porcelain']) || null };
{
  const sha = spawnSync('sha256sum', [args['ferryx-cli']], { encoding: 'utf8' });
  evidence.binary = { path: args['ferryx-cli'], sha256: sha.stdout.split(/\s+/)[0] || null };
}

// --- daemon socket ----------------------------------------------------------------------------
function exchange(socketPath, frames, deadlineMs) {
  return new Promise((resolveExchange, rejectExchange) => {
    const replies = [];
    let buffer = '';
    let next = 0;
    let settled = false;
    const connection = createConnection(socketPath);
    const finish = (fn, value) => { if (settled) return; settled = true; clearTimeout(timer); connection.destroy(); fn(value); };
    const timer = setTimeout(() => finish(rejectExchange, fail('DEADLINE_EXCEEDED', `daemon exchange exceeded ${deadlineMs}ms`)), deadlineMs);
    const send = () => { if (next >= frames.length) { finish(resolveExchange, replies); return; } connection.write(`${JSON.stringify(frames[next])}\n`); next += 1; };
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

// Subscribe to the directory first, then reconcile; bounded by `deadlineMs`.
function waitUntil(dir, check, deadlineMs, label) {
  return new Promise((resolveWait, rejectWait) => {
    let watcher = null;
    let done = false;
    const finish = (fn, value) => { if (done) return; done = true; clearTimeout(timer); clearInterval(poll); try { watcher?.close(); } catch { /* closed */ } fn(value); };
    const evaluate = () => { try { const value = check(); if (value) finish(resolveWait, value); } catch { /* the next change re-reads */ } };
    const timer = setTimeout(() => finish(rejectWait, fail('DEADLINE_EXCEEDED', `${label} was not observed within ${deadlineMs}ms`)), deadlineMs);
    mkdirSync(dir, { recursive: true });
    watcher = watch(dir, evaluate);
    // fs.watch on macOS coalesces or drops appends to a file inside the watched directory, so a
    // change event is not guaranteed; the bounded poll keeps the same predicates authoritative.
    const poll = setInterval(evaluate, 250);
    evaluate();
  });
}

const paneRecords = (logDir) => (existsSync(logDir) ? readdirSync(logDir).filter((name) => name.endsWith('.jsonl')) : [])
  .flatMap((name) => readFileSync(join(logDir, name), 'utf8').split('\n').filter(Boolean).map((line) => { try { return JSON.parse(line); } catch { return null; } }))
  .filter(Boolean);
const waitPane = (logDir, predicate, deadlineMs, label) => waitUntil(logDir, () => paneRecords(logDir).find(predicate), deadlineMs, label);

// --- one isolated daemon with its owner device ------------------------------------------------
async function bringUp(label, hub) {
  const base = join(root, label);
  const env = {
    PATH: process.env.PATH ?? '/usr/bin:/bin', LANG: 'C.UTF-8', TERM: 'xterm-256color', USER: process.env.USER ?? 'qa',
    HOME: join(base, 'home'), TMPDIR: join(base, 'tmp'), FERRYX_DATA_DIR: join(base, 'data'), FERRYX_RUNTIME_DIR: join(base, 'runtime'),
    FERRYX_HUB_DB: join(base, 'hub', 'hub.sqlite'), FERRYX_HUB: hub ? 'on' : 'off', FERRYX_HUB_QUIET_HOURS: 'off', SHELL: '/bin/bash', RUST_LOG: 'info',
  };
  for (const dir of [env.HOME, env.TMPDIR, env.FERRYX_RUNTIME_DIR, join(base, 'ws', 'project'), join(base, 'panes'), join(base, 'bin')]) mkdirSync(dir, { recursive: true, mode: 0o700 });
  writeFileSync(join(env.HOME, '.bashrc'), '');
  // The pane program is named claude.js so the foreground observer recognizes agent `claude`.
  const entry = join(base, 'bin', 'claude.js');
  writeFileSync(entry, `await import(${JSON.stringify(PANE)});\n`);
  const repo = join(base, 'ws', 'project');
  for (const gitArgs of [['init', '-q', repo], ['-C', repo, 'commit', '-q', '--allow-empty', '-m', 'f4 fixture']]) {
    const result = runGit(['-c', 'init.defaultBranch=main', '-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid', ...gitArgs], { encoding: 'utf8' });
    if (result.status !== 0) throw fail('FIXTURE_FAILED', `git ${gitArgs.join(' ')}: ${result.stderr}`);
  }
  if (hub) {
    const free = await new Promise((resolvePort) => { const server = Bun.listen({ hostname: '127.0.0.1', port: 0, socket: { data() {} } }); const port = server.port; server.stop(true); resolvePort(port); });
    env.FERRYX_HUB_TRACKER_ADDR = `127.0.0.1:${free}`;
    const setup = spawnSync(args['ferryx-cli'], ['supervisor', 'setup', '--owner', 'f4-owner', '--project', 'f4', '--repo-identity', 'qa:f4', '--host', 'local', '--git-common-dir', join(realpathSync(repo), '.git'), '--json'], { env, encoding: 'utf8', timeout: 60_000 });
    jsonl('cli.jsonl', { daemon: label, argv: ['supervisor', 'setup'], code: setup.status, stdout: setup.stdout, stderr: setup.stderr });
    if (setup.status !== 0) throw fail('SETUP_FAILED', `supervisor setup: ${setup.stderr}`);
  }
  const socket = join(env.FERRYX_RUNTIME_DIR, 'daemon.sock');
  const child = spawn(args['ferryx-cli'], ['--daemon'], { cwd: base, env, detached: true, stdio: ['ignore', 'ignore', 'ignore'] });
  const record = { label, hub, pid: child.pid, executable: args['ferryx-cli'], args: ['--daemon'], startedAtMs: now(), exited: null, dataDir: base };
  child.on('exit', (code, signal) => { record.exited = { code, signal, atMs: now() }; });
  evidence.daemons.push(record);
  const daemon = { label, base, env, socket, child, record, repo, entry, logDir: join(base, 'panes'), frames: [], waiters: [], lastWriteMs: 0 };
  await waitUntil(env.FERRYX_RUNTIME_DIR, () => existsSync(socket), 60_000, `${label} daemon socket`);
  const log = join(env.FERRYX_DATA_DIR, 'logs', 'daemon.log');
  if (hub) await waitUntil(dirname(log), () => existsSync(log) && readFileSync(log, 'utf8').includes('ferryx hub is up'), 120_000, `${label} hub is up`);
  daemon.call = async (name, request, deadlineMs = 60_000) => {
    let version = PROTOCOL;
    let reply = null;
    for (let attempt = 0; attempt < 2; attempt += 1) {
      const replies = await exchange(socket, [{ type: 'handshake', version }, request], deadlineMs);
      const mismatch = replies.find((r) => r?.type === 'protocolMismatch');
      if (mismatch && attempt === 0 && Number.isInteger(mismatch.expectedVersion)) { version = mismatch.expectedVersion; continue; }
      reply = replies.at(-1) ?? null;
      break;
    }
    jsonl('daemon-socket.jsonl', { daemon: label, name, request, reply });
    return reply;
  };
  const ws = await daemon.call('register-workspace', { type: 'registerWorkspace', workspaceId: WORKSPACE, repoRoot: realpathSync(join(base, 'ws')) });
  const control = await daemon.call('agent-control-on', { type: 'agentControlSettingsSet', enabled: true, excludedDeviceIds: [] });
  // Port 0 means "the default port" to the daemon, which collides with any Ferryx desktop already
  // listening; claim a free ephemeral port first so this run owns its gateway.
  const gatewayPort = await new Promise((resolvePort) => { const server = Bun.listen({ hostname: '127.0.0.1', port: 0, socket: { data() {} } }); const port = server.port; server.stop(true); resolvePort(port); });
  const configured = await daemon.call('gateway-configure', { type: 'remoteConfigure', config: { mode: 'localNetwork', port: gatewayPort, allowControl: true } });
  const status = await daemon.call('gateway-status', { type: 'remoteGetStatus' });
  const bound = status?.status?.boundAddress ?? null;
  if (ws?.type !== 'registerWorkspaceOk' || control?.type !== 'agentControlSettingsOk' || configured?.type !== 'remoteConfigureOk' || !bound) {
    throw fail('GATEWAY_UNAVAILABLE', `${label}: ${JSON.stringify({ ws, control, configured, status })}`);
  }
  const url = `http://${bound.replace('0.0.0.0', '127.0.0.1')}`;
  daemon.http = async (name, method, path, body) => {
    const headers = { accept: 'application/json' };
    if (daemon.token) headers.authorization = `Bearer ${daemon.token}`;
    if (body !== undefined) headers['content-type'] = 'application/json';
    const sentAtMs = now();
    const response = await fetch(`${url}${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
    const text = await response.text();
    let json = null;
    try { json = JSON.parse(text); } catch { /* raw */ }
    const entry = { daemon: label, name, method, path, request: body ?? null, status: response.status, body: json ?? text, sentAtMs, receivedAtMs: now() };
    jsonl('http.jsonl', entry);
    return entry;
  };
  const code = await daemon.call('pairing-code', { type: 'remoteCreateMachinePairingCode' });
  const paired = await daemon.http('owner-pair', 'POST', '/api/v1/pair/exchange', { code: code?.code, deviceName: `f4-owner-${label}`, installationId: `f4-${label}-${randomUUID()}` });
  daemon.token = paired.body?.token ?? null;
  if (!daemon.token) throw fail('PAIRING_FAILED', `${label}: ${JSON.stringify(paired.body)}`);
  const ticket = await daemon.http('socket-ticket', 'POST', '/api/v1/socket-ticket', { target: '/api/v1/events' });
  daemon.socketWs = new WebSocket(`${url.replace('http', 'ws')}/api/v1/events?ticket=${encodeURIComponent(ticket.body?.ticket ?? '')}`);
  daemon.socketWs.addEventListener('message', (message) => {
    let frame = null;
    try { frame = JSON.parse(String(message.data)); } catch { return; }
    const entry = { daemon: label, atMs: now(), frame };
    jsonl('events.jsonl', entry);
    daemon.frames.push(entry);
    for (const waiter of [...daemon.waiters]) if (waiter.predicate(entry)) { daemon.waiters.splice(daemon.waiters.indexOf(waiter), 1); waiter.resolve(entry); }
  });
  daemon.socketWs.addEventListener('close', (event) => jsonl('events.jsonl', { daemon: label, kind: 'close', code: event.code }));
  const opened = await new Promise((resolveOpen) => {
    daemon.socketWs.addEventListener('open', () => resolveOpen(true), { once: true });
    daemon.socketWs.addEventListener('error', () => resolveOpen(false), { once: true });
    setTimeout(() => resolveOpen(false), 30_000);
  });
  if (!opened) throw fail('SOCKET_FAILED', `${label}: the owner event socket did not open`);
  // A frame already received counts, so a waiter registered late never misses one.
  daemon.frame = (name, predicate, deadlineMs, after = 0) => new Promise((resolveFrame, rejectFrame) => {
    const seen = daemon.frames.find((entry) => entry.atMs >= after && predicate(entry));
    if (seen) { resolveFrame(seen); return; }
    const waiter = { predicate: (entry) => entry.atMs >= after && predicate(entry), resolve: (entry) => { clearTimeout(timer); resolveFrame(entry); } };
    const timer = setTimeout(() => { daemon.waiters.splice(daemon.waiters.indexOf(waiter), 1); rejectFrame(fail('DEADLINE_EXCEEDED', `${label}: ${name} not seen within ${deadlineMs}ms`)); }, deadlineMs);
    daemon.waiters.push(waiter);
  });
  daemon.write = async (name, sessionId, text) => {
    daemon.lastWriteMs = now();
    return daemon.call(name, { type: 'write', sessionId, data: Buffer.from(text).toString('base64') });
  };
  // A bash pane, then the fixture agent typed into it, so the shell owns the pane again when it dies.
  daemon.agentPane = async (role) => {
    const spawned = await daemon.call(`spawn-${role}`, { type: 'spawn', clientRequestId: `f4-${role}-${randomUUID()}`, workspaceId: WORKSPACE, worktree: null, cwd: realpathSync(repo), cols: 120, rows: 40 });
    if (spawned?.type !== 'spawnOk') throw fail('FIXTURE_FAILED', `${label} ${role} pane: ${JSON.stringify(spawned)}`);
    const sessionId = spawned.sessionId;
    await daemon.write(`type-${role}`, sessionId, `${process.execPath} ${entry} ${role} ${daemon.logDir}\n`);
    const ready = await waitPane(daemon.logDir, (r) => r.sessionId === sessionId && r.role === role && r.kind === 'ready', 60_000, `${role} ready`);
    return { sessionId, pid: ready.pid };
  };
  daemon.fleetEntry = async (name, sessionId) => {
    const read = await daemon.http(name, 'GET', '/api/v1/fleet/sessions');
    return (read.body?.sessions ?? []).find((s) => String(s.sessionRef ?? '').endsWith(`:${sessionId}`)) ?? null;
  };
  return daemon;
}

// --- the panel UI, rendered headless from the product's own model and components ---------------
async function renderPanel(name, snapshot, nowMs) {
  const react = await import(join(REPO, 'ui', 'node_modules', 'react', 'index.js'));
  const server = await import(join(REPO, 'ui', 'node_modules', 'react-dom', 'server.node.js'));
  const { PanelSurface } = await import(join(REPO, 'ui', 'src', 'components', 'supervisorPanel', 'PanelSurface.tsx'));
  const { buildPanelView } = await import(join(REPO, 'ui', 'src', 'lib', 'supervisorPanel', 'model.ts'));
  const ok = async () => ({ ok: true });
  const actions = { answer: ok, setPaused: ok, setLanePaused: ok, converse: ok, stop: ok, undo: ok, turnOn: ok, openSession: () => {} };
  const html = server.renderToStaticMarkup(react.createElement(PanelSurface, { snapshot, actions, nowMs, hideComposer: true }));
  const view = buildPanelView(snapshot, { nowMs });
  writeFileSync(join(out, `render-${name}.html`), html);
  writeFileSync(join(out, `render-${name}.view.json`), `${JSON.stringify(view, null, 2)}\n`);
  return { html, view };
}

const sessionRowState = (html, title) => {
  for (const chunk of html.split('data-testid="session-row"').slice(1)) {
    if (chunk.includes(title)) return /data-state="([a-z]+)"/.exec(chunk)?.[1] ?? null;
  }
  return null;
};
const logLineChunk = (html, summary) => html.split('data-testid="action-log-line"').slice(1).find((chunk) => chunk.split('</li>')[0].includes(summary))?.split('</li>')[0] ?? null;

// --- teardown by recorded pid -----------------------------------------------------------------
// A zombie (exited, not yet reaped by its new parent) is not alive, and neither is a process
// mid-exit: the kernel empties /proc/<pid>/cmdline once its address space is released, so it
// can never run user code again.
const procState = (pid) => { try { return readFileSync(`/proc/${pid}/stat`, 'utf8').replace(/^.*\) /s, '')[0] ?? null; } catch { return null; } };
const exiting = (pid) => { try { return readFileSync(`/proc/${pid}/cmdline`).length === 0; } catch { return false; } };
const alive = (pid) => { try { process.kill(pid, 0); return procState(pid) !== 'Z' && !exiting(pid); } catch (error) { return error?.code === 'EPERM'; } };
const argvOf = (pid) => { try { return readFileSync(`/proc/${pid}/cmdline`, 'utf8').split('\0').join(' '); } catch { return spawnSync('ps', ['-o', 'args=', '-p', String(pid)], { encoding: 'utf8' }).stdout.trim() || null; } };
async function teardown(daemon) {
  try { daemon.socketWs?.close(); } catch { /* closed */ }
  const { record, child } = daemon;
  const receipt = { label: record.label, pid: record.pid, signals: [], panes: [] };
  for (const [signal, waitMs] of [['SIGTERM', 15_000], ['SIGKILL', 5_000]]) {
    if (record.exited) break;
    try { process.kill(-record.pid, signal); receipt.signals.push({ signal, target: `group ${record.pid}` }); } catch (error) { receipt.signals.push({ signal, error: String(error?.message ?? error) }); }
    await new Promise((resolveExit) => { if (record.exited) { resolveExit(); return; } const timer = setTimeout(resolveExit, waitMs); child.once('exit', () => { clearTimeout(timer); resolveExit(); }); });
  }
  receipt.exited = record.exited;
  receipt.aliveAfter = alive(record.pid);
  for (const pid of [...new Set(paneRecords(daemon.logDir).map((r) => r.pid))]) {
    const state = procState(pid);
    const sampledLive = alive(pid);
    const argv = sampledLive ? argvOf(pid) : null;
    // A cmdline that emptied (or a process that vanished) between the two reads is a pane that
    // was already exiting.
    const live = sampledLive && Boolean(argv);
    const ours = live && Boolean(argv?.includes(daemon.entry));
    if (ours) { try { process.kill(pid, 'SIGTERM'); } catch { /* raced its exit */ } }
    receipt.panes.push({ pid, aliveAtTeardown: live, procState: state, exiting: state !== null && exiting(pid), signalled: ours, argv });
  }
  for (const name of readdirSync(daemon.logDir)) copyFileSync(join(daemon.logDir, name), join(out, `${daemon.label}-${name}`));
  const log = join(daemon.env.FERRYX_DATA_DIR, 'logs', 'daemon.log');
  if (existsSync(log)) copyFileSync(log, join(out, `${daemon.label}-daemon.log`));
  evidence.teardown.push(receipt);
  return receipt;
}

// --- clause 2: Stop with the hub down ---------------------------------------------------------
async function stopClause(daemon, name, hubDown) {
  const pane = await daemon.agentPane('stoppable');
  const working = await daemon.frame(`${name} working`, (e) => e.frame?.type === 'panel.session' && String(e.frame.sessionRef).endsWith(`:${pane.sessionId}`) && frameState(e.frame) === 'working', 30_000);
  const before = await daemon.fleetEntry(`${name}-fleet-before`, pane.sessionId);
  // The product refuses supervisor input for 3 s after a person typed; the driver's own
  // keystrokes count as typing.
  const quiet = daemon.lastWriteMs + HUMAN_TYPING_WINDOW_MS + 250 - now();
  if (quiet > 0) await new Promise((resolveQuiet) => setTimeout(resolveQuiet, quiet));
  const ref = before?.sessionRef ?? working.frame.sessionRef;
  const requestId = randomUUID();
  const stopAt = now();
  const stop = await daemon.http(`${name}-stop`, 'POST', `/api/v1/sessions/${encodeURIComponent(ref)}/stop`, { requestId });
  const left = await daemon.frame(`${name} left working`, (e) => e.frame?.type === 'panel.session' && e.frame.sessionRef === ref && frameState(e.frame) !== null && frameState(e.frame) !== 'working', 30_000, stopAt).catch(() => null);
  const interrupted = await waitPane(daemon.logDir, (r) => r.sessionId === pane.sessionId && r.kind === 'interrupted', 30_000, 'interrupted').catch(() => null);
  const after = await daemon.fleetEntry(`${name}-fleet-after`, pane.sessionId);
  const listed = await daemon.call(`${name}-list`, { type: 'listSessions' }).catch((error) => ({ error: String(error?.message ?? error) }));
  const ptyAlive = JSON.stringify(listed).includes(pane.sessionId);
  return clause(`stop-with-hub-down:${name}`, hubDown.ok && stop.status === 200 && stop.body?.receipt?.stage === 'accepted' && before?.state === 'working' && left !== null && after?.state !== 'working' && after?.state !== undefined && interrupted !== null && ptyAlive, {
    hubDown, sessionRef: ref, requestId, stopSentAtMs: stopAt, stopResponse: { status: stop.status, body: stop.body, receivedAtMs: stop.receivedAtMs },
    stateBefore: before?.state ?? null, leftWorkingFrame: left, leftWorkingAfterMs: left ? left.atMs - stopAt : null, stateAfter: after?.state ?? null,
    paneInterruptedAtMs: interrupted?.atMs ?? null, ptyStillListed: ptyAlive,
  });
}

// --- main -------------------------------------------------------------------------------------
let exitCode = 4;
const daemons = [];
try {
  const hubDaemon = await bringUp('hub', true);
  daemons.push(hubDaemon);

  // Clause 3 starts first: the scheduler needs a few of its 15 s passes and a 60 s backoff.
  const upstream = await hubDaemon.agentPane('upstream');

  // Clause 1: kill an agent mid-turn.
  const killable = await hubDaemon.agentPane('killable');
  await waitPane(hubDaemon.logDir, (r) => r.sessionId === killable.sessionId && r.kind === 'reported' && r.state === 'working', 30_000, 'killable working');
  await hubDaemon.frame('killable working', (e) => e.frame?.type === 'panel.session' && String(e.frame.sessionRef).endsWith(`:${killable.sessionId}`) && frameState(e.frame) === 'working', 30_000);
  // Two full foreground listings (each at most OBSERVER_REFRESH_MS apart) have seen the agent.
  await new Promise((resolveSeen) => setTimeout(resolveSeen, 2 * OBSERVER_REFRESH_MS + 500));
  const argv = argvOf(killable.pid);
  if (!argv?.includes(hubDaemon.entry) || !argv.includes('killable')) throw fail('FIXTURE_FAILED', `pid ${killable.pid} is not the killable fixture: ${argv}`);
  const killAt = now();
  process.kill(killable.pid, 'SIGKILL');
  jsonl('kill.jsonl', { pid: killable.pid, argv, signal: 'SIGKILL', killAtMs: killAt, sessionId: killable.sessionId });
  const failedFrame = await hubDaemon.frame('failed panel.session', (e) => e.frame?.type === 'panel.session' && String(e.frame.sessionRef).endsWith(`:${killable.sessionId}`) && e.frame.session?.health?.type === 'failed', 15_000, killAt).catch(() => null);
  const fleetFailed = await hubDaemon.fleetEntry('killable-fleet-after-kill', killable.sessionId);
  const panelAfterKill = await hubDaemon.http('panel-after-kill', 'GET', '/api/v1/supervisor/panel');
  const converse = await hubDaemon.http('converse-no-engine', 'POST', '/api/v1/supervisor/converse', { text: 'how is the migration session doing?', mentions: [], requestId: randomUUID() });
  const killSnapshot = { ...panelAfterKill.body, sessions: panelAfterKill.body?.sessions ?? [] };
  const killRender = panelAfterKill.status === 200 ? await renderPanel('after-kill', killSnapshot, now()) : null;
  const killRow = killRender?.view.sessions.find((row) => String(row.sessionRef).endsWith(`:${killable.sessionId}`)) ?? null;
  const latency = failedFrame ? failedFrame.atMs - killAt : null;
  clause('failed-within-5s', latency !== null && latency <= FAILED_WITHIN_MS && fleetFailed?.health?.type === 'failed' && killRow?.glyph === 'failed'
    && sessionRowState(killRender.html, killRow.title) === 'failed' && converse.body?.error?.code === 'MHC_NOT_RUNNING', {
    killAtMs: killAt, failedFrameAtMs: failedFrame?.atMs ?? null, latencyMs: latency, limitMs: FAILED_WITHIN_MS, failedFrame: failedFrame?.frame ?? null,
    fleetEntry: fleetFailed, panelRow: killRow, renderedRowState: killRender ? sessionRowState(killRender.html, killRow?.title ?? '\u0000') : null,
    noSupervisorText: { converseStatus: converse.status, converseBody: converse.body, note: 'no engine is installed: no supervisor text exists to consult' },
  });

  // Clause 3: the recovery scheduler's own claim, with no delivery event, renders Unverified.
  // Once the incident is open the fixture stops drawing new errors, as Claude Code does at its
  // prompt after an API error, so the scheduler's resume is judged on the resume's own output.
  const opened = await hubDaemon.frame('recovery incident opened', (e) => e.frame?.type === 'panel.action' && e.frame.action?.live === true
    && JSON.stringify(e.frame.action?.evidence ?? []).includes(upstream.sessionId), 3 * 60_000);
  const upstreamArgv = argvOf(upstream.pid);
  if (!upstreamArgv?.includes(hubDaemon.entry) || !upstreamArgv.includes('upstream')) throw fail('FIXTURE_FAILED', `pid ${upstream.pid} is not the upstream fixture: ${upstreamArgv}`);
  process.kill(upstream.pid, 'SIGUSR2');
  jsonl('kill.jsonl', { pid: upstream.pid, argv: upstreamArgv, signal: 'SIGUSR2', note: 'stop drawing errors; incident opened', incidentFrameAtMs: opened.atMs });
  const resumedFrame = await hubDaemon.frame('recovery claim closed', (e) => e.frame?.type === 'panel.action' && e.frame.action?.live === false
    && JSON.stringify(e.frame.action?.evidence ?? []).includes(upstream.sessionId), 6 * 60_000).catch(() => null);
  const upstreamInputs = paneRecords(hubDaemon.logDir).filter((r) => r.sessionId === upstream.sessionId && r.kind === 'input');
  const panelAfterRecovery = await hubDaemon.http('panel-after-recovery', 'GET', '/api/v1/supervisor/panel');
  const logEntry = (panelAfterRecovery.body?.log ?? []).find((entry) => JSON.stringify(entry.evidence ?? []).includes(upstream.sessionId)) ?? null;
  const recoveryRender = panelAfterRecovery.status === 200 ? await renderPanel('after-recovery', panelAfterRecovery.body, now()) : null;
  const chunk = logEntry && recoveryRender ? logLineChunk(recoveryRender.html, logEntry.summary) : null;
  const noDelivery = logEntry !== null && !(logEntry.evidence ?? []).some((ref) => DELIVERY_KINDS.has(ref.kind));
  const allLogRows = (panelAfterRecovery.body?.log ?? []).map((entry) => ({ id: entry.id, kind: entry.kind, summary: entry.summary, evidence: entry.evidence }));
  clause('unverified-claim', resumedFrame !== null && logEntry !== null && noDelivery && chunk !== null && chunk.includes('Unverified')
    && recoveryRender.view.log.all.find((line) => line.id === logEntry.id)?.unverified === true, {
    claimFrame: resumedFrame, logEntry, deliveryKinds: [...DELIVERY_KINDS], evidenceHasDeliveryEvent: logEntry ? !noDelivery : null,
    incidentOpenedFrame: opened, renderedLogLine: chunk, resumeTypedByScheduler: upstreamInputs, allLogRows,
  });
  // The control in the same render: a verified line (if any) carries no chip.
  const verified = (panelAfterRecovery.body?.log ?? []).filter((entry) => (entry.evidence ?? []).some((ref) => DELIVERY_KINDS.has(ref.kind)));
  jsonl('clauses.jsonl', { note: 'verified log rows in the same render (no chip expected)', rows: verified.map((entry) => ({ id: entry.id, summary: entry.summary, chip: (logLineChunk(recoveryRender?.html ?? '', entry.summary) ?? '').includes('Unverified') })) });

  // Clause 2a: the hub stopped on the hub daemon.
  const stopHub = spawnSync(args['ferryx-cli'], ['hub', 'stop'], { env: hubDaemon.env, encoding: 'utf8', timeout: 60_000 });
  const hubStatus = spawnSync(args['ferryx-cli'], ['hub', 'status'], { env: hubDaemon.env, encoding: 'utf8', timeout: 60_000 });
  jsonl('cli.jsonl', { daemon: 'hub', argv: ['hub', 'stop'], code: stopHub.status, stdout: stopHub.stdout, stderr: stopHub.stderr });
  jsonl('cli.jsonl', { daemon: 'hub', argv: ['hub', 'status'], code: hubStatus.status, stdout: hubStatus.stdout, stderr: hubStatus.stderr });
  const hubStopped = stopHub.status === 0 && stopHub.stdout.includes('"op":"stop"') && stopHub.stdout.includes('"status":"ok"');
  await stopClause(hubDaemon, 'hub-stopped', { ok: hubStopped, how: 'ferryx-cli hub stop', stop: stopHub.stdout.trim(), status: hubStatus.stdout.trim(), statusExit: hubStatus.status });
  await teardown(hubDaemon);

  // Clause 2b: a daemon whose hub never opened (the gateway port is fixed, so it runs after).
  const bare = await bringUp('nohub', false);
  daemons.push(bare);
  const bareLog = readFileSync(join(bare.env.FERRYX_DATA_DIR, 'logs', 'daemon.log'), 'utf8');
  await stopClause(bare, 'hub-never-started', { ok: !bareLog.includes('ferryx hub is up'), how: 'FERRYX_HUB=off', hubUpLogged: bareLog.includes('ferryx hub is up') });
  await teardown(bare);

  const clean = evidence.teardown.every((r) => r.exited && !r.aliveAfter && r.panes.every((p) => !p.aliveAtTeardown || p.signalled));
  clause('teardown-clean', clean, { teardown: evidence.teardown });
  exitCode = evidence.clauses.every((c) => c.pass) ? 0 : 1;
} catch (error) {
  evidence.error = { code: error?.code ?? 'ERROR', message: String(error?.message ?? error), stack: error?.stack ?? null };
  console.error(`ERROR ${evidence.error.code}: ${evidence.error.message}`);
  for (const daemon of daemons) if (!evidence.teardown.some((r) => r.label === daemon.label)) await teardown(daemon).catch(() => {});
  exitCode = 4;
} finally {
  evidence.finishedAtMs = now();
  evidence.exitCode = exitCode;
  writeFileSync(join(out, 'f4-evidence.json'), `${JSON.stringify(evidence, null, 2)}\n`);
  writeFileSync(join(out, 'exit-code.txt'), `${exitCode}\n`);
}
process.exit(exitCode);
