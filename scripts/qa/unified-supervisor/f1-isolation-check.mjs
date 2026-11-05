// F1 PID-isolation integration self-test (Linux only). It runs the real f1-pid-isolate.sh with
// a real bwrap PID namespace, a staged Bun and staged copies of the current fixture sources, then
// proves: (1) the initial invocation enters a new PID namespace, (2) a daemon-like env rebuild ->
// SHELL wrapper re-entry stays in the SAME namespace, (3) the host helper maps the recorded
// namespace-local identity to the real host pid/start stamp, (4) a live matching identity fails
// fleetTeardownClean even with an end record and only a real observed exit makes it clean, and
// (5) a daemon group whose leader exited while a member remains is still seen alive.
// No fixed sleeps: every wait subscribes to a stdout line, file event or process exit first.
//
// The module doubles as the fixture the sandbox runs (argv[2]):
//   inner-parent   daemon stand-in; line-JSON control on stdin, JSON events on stdout
//   pane-child     pane stand-in started through SHELL -> wrapper; writes pane-<pid>.jsonl
//   group-leader   spawns group-member in its own process group, exits on request
//   group-member   stays alive until SIGTERM
import { spawn } from 'node:child_process';
import { spawnSync } from 'node:child_process';
import { appendFileSync, chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, readlinkSync, realpathSync, writeFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { gitObjectsSource } from './f1-git-objects.mjs';
import { fleetTeardownClean, linuxProcess, paneTeardownState, sameProcess } from './f1-process-identity.mjs';
const SELF = fileURLToPath(import.meta.url);
const QA_DIR = dirname(SELF);
const WRAPPER = join(QA_DIR, 'f1-pid-isolate.sh');
const HELPER = join(QA_DIR, 'f1-host-helper.mjs');
const BWRAP = '/usr/bin/bwrap';
const STAGED_WRAPPER = 'f1-pid-isolate.sh';
const WAIT_MS = 60_000;
// Same key list the host agent's daemonEnv forwards into a rebuilt daemon environment.
const FORWARDED = ['F1_PID_ISOLATED', 'F1_HOST_HELPER', 'F1_ISOLATION_ROOT', 'F1_BUN'];

const emit = (value) => process.stdout.write(`${JSON.stringify(value)}\n`);
const readJsonl = (path) => (existsSync(path) ? readFileSync(path, 'utf8').split('\n').filter(Boolean).map((line) => JSON.parse(line)) : []);

// Namespace-local facts only, as the pane itself would record them.
function localIdentity() {
  const p = linuxProcess(process.pid);
  return p && { pid: p.pid, namespacePid: p.namespacePid, pidNamespace: p.pidNamespace, startStamp: p.startStamp, pgrp: p.pgrp, args: p.args };
}

function rebuiltEnv(shell) {
  const root = process.env.F1_ISOLATION_ROOT;
  return {
    PATH: process.env.PATH ?? '/usr/bin:/bin', LANG: 'C.UTF-8', TERM: 'xterm-256color', USER: process.env.USER ?? 'qa',
    HOME: join(root, 'home'), TMPDIR: join(root, 'tmp'), SHELL: shell,
    ...Object.fromEntries(FORWARDED.filter((key) => process.env[key] !== undefined).map((key) => [key, process.env[key]])),
  };
}

function runInnerParent() {
  let pane = null;
  let leader = null;
  emit({ event: 'parent-ready', identity: localIdentity(), isolated: process.env.F1_PID_ISOLATED ?? null, socket: process.env.F1_HOST_HELPER ?? null });
  const forward = (child, tag) => createInterface({ input: child.stdout, terminal: false }).on('line', (text) => {
    let line; try { line = JSON.parse(text); } catch { line = { unparsed: text }; }
    emit({ event: tag, line });
  });
  createInterface({ input: process.stdin, terminal: false }).on('line', (text) => {
    const request = JSON.parse(text);
    if (request.op === 'spawn-pane') {
      // Daemon-like: environment rebuilt from scratch, then the pane is the SHELL.
      pane = spawn(request.shell, [], { env: rebuiltEnv(request.shell), stdio: ['pipe', 'pipe', 'inherit'] });
      forward(pane, 'pane');
      pane.on('exit', (code, signal) => emit({ event: 'pane-exit', code, signal }));
    } else if (request.op === 'pane-stdin-close') {
      pane.stdin.end();
    } else if (request.op === 'spawn-group') {
      leader = spawn(process.execPath, [SELF, 'group-leader'], { env: rebuiltEnv(process.env.SHELL ?? '/bin/sh'), detached: true, stdio: ['pipe', 'pipe', 'inherit'] });
      // stdout closes only when the leader AND the member holding the inherited pipe are gone.
      createInterface({ input: leader.stdout, terminal: false }).on('line', (line) => emit({ event: 'group', line: JSON.parse(line) })).on('close', () => emit({ event: 'group-closed' }));
      leader.on('exit', (code, signal) => emit({ event: 'leader-exit', code, signal }));
    } else if (request.op === 'leader-exit') {
      leader.stdin.write('exit\n');
    } else if (request.op === 'quit') {
      process.exit(0);
    }
  }).on('close', () => process.exit(0));
}

function runPaneChild(logDir) {
  mkdirSync(logDir, { recursive: true });
  const identity = localIdentity();
  const file = join(logDir, `pane-${process.pid}.jsonl`);
  const record = (value) => appendFileSync(file, `${JSON.stringify({ pid: process.pid, identity, ...value })}\n`);
  record({ kind: 'ready' });
  process.on('SIGTERM', () => process.exit(0));
  createInterface({ input: process.stdin, terminal: false }).on('close', () => {
    // The end record is written while the process stays alive: it must not hide a live identity.
    record({ kind: 'stdin-closed' });
    emit({ event: 'end-recorded', identity });
  });
  setInterval(() => {}, 2 ** 30);
  emit({ event: 'pane-ready', identity, isolated: process.env.F1_PID_ISOLATED ?? null, socket: process.env.F1_HOST_HELPER ?? null });
}

function runGroupLeader() {
  const member = spawn(process.execPath, [SELF, 'group-member'], { env: process.env, stdio: ['ignore', 'inherit', 'inherit'] });
  member.unref();
  emit({ event: 'leader-ready', identity: localIdentity() });
  createInterface({ input: process.stdin, terminal: false }).on('line', () => process.exit(0));
}

function runGroupMember() {
  process.on('SIGTERM', () => process.exit(0));
  setInterval(() => {}, 2 ** 30);
  emit({ event: 'member-ready', identity: localIdentity() });
}

// ---- runner side -------------------------------------------------------------------------

// JSON-line stream with a history, so an event that arrives before a wait is never lost.
class Events {
  constructor(proc, label) {
    this.label = label; this.history = []; this.waiters = new Set(); this.closed = null; this.stderr = '';
    createInterface({ input: proc.stdout, terminal: false }).on('line', (text) => {
      let value; try { value = JSON.parse(text); } catch { value = { unparsed: text }; }
      this.history.push(value);
      for (const waiter of [...this.waiters]) if (waiter.match(value)) waiter.done(value);
    });
    proc.stderr?.on('data', (chunk) => { this.stderr += chunk; });
    proc.on('close', (code, signal) => {
      this.closed = { code, signal };
      for (const waiter of [...this.waiters]) waiter.fail(new Error(`${label} exited (code ${code}, signal ${signal}) before the awaited event: ${this.stderr.slice(-1500)}`));
    });
  }

  next(match, timeoutMs = WAIT_MS) {
    return new Promise((resolve, reject) => {
      const hit = this.history.find(match);
      if (hit) { resolve(hit); return; }
      if (this.closed) { reject(new Error(`${this.label} already exited (code ${this.closed.code}): ${this.stderr.slice(-1500)}`)); return; }
      const waiter = {
        match,
        done: (value) => { clearTimeout(timer); this.waiters.delete(waiter); resolve(value); },
        fail: (error) => { clearTimeout(timer); this.waiters.delete(waiter); reject(error); },
      };
      const timer = setTimeout(() => waiter.fail(new Error(`${this.label}: no awaited event within ${timeoutMs}ms`)), timeoutMs);
      this.waiters.add(waiter);
    });
  }
}

const tagged = (event, inner) => (value) => value.event === event && (!inner || value.line?.event === inner);
const isReply = (value) => value.ok !== undefined;

// Copies a fixture module and every relative module it imports, transitively.
function stageModules(entry, targetDir, staged = new Set()) {
  const name = basename(entry);
  if (staged.has(name)) return staged;
  staged.add(name);
  copyFileSync(entry, join(targetDir, name));
  for (const match of readFileSync(entry, 'utf8').matchAll(/from '(\.\/[^']+\.mjs)'/g)) stageModules(join(dirname(entry), match[1]), targetDir, staged);
  return staged;
}

// Independent host-side cross-check: the single host process whose namespace identity matches.
function scanHost(local) {
  const found = [];
  for (const name of readdirSync('/proc').filter((entry) => /^\d+$/.test(entry))) {
    let candidate;
    try { candidate = linuxProcess(Number(name)); } catch (error) { if (['EACCES', 'EPERM'].includes(error?.code)) continue; throw error; }
    if (candidate && sameProcess(local, candidate)) found.push(candidate);
  }
  return found.length === 1 ? found[0] : null;
}

// One helper RPC over the wrapper-owned socket (`--call`); returns the stream so a caller can
// subscribe to the `armed` frame of a `wait` before it triggers the signal.
function helperCall(socket, request) {
  const proc = spawn(process.execPath, [HELPER, '--call', socket], { stdio: ['pipe', 'pipe', 'pipe'] });
  const stream = new Events(proc, `helper ${request.op}`);
  proc.stdin.end(JSON.stringify(request));
  return stream;
}

async function helperReply(socket, request) {
  const reply = await helperCall(socket, request).next(isReply);
  if (!reply.ok) throw new Error(`helper ${request.op} refused: ${JSON.stringify(reply.error)}`);
  return reply.result;
}

// Waits for the helper's kernel pidfd exit evidence. The helper arms its watch before it replies
// `armed`; only then is the signal sent, so a fast exit cannot be missed.
async function signalAndAwaitExit(socket, identity, hostPid, signal) {
  const stream = helperCall(socket, { op: 'wait', identity, deadlineMs: 20_000 });
  await stream.next((value) => value.armed === true);
  process.kill(hostPid, signal);
  const reply = await stream.next(isReply);
  if (!reply.ok) throw new Error(`helper wait refused: ${JSON.stringify(reply.error)}`);
  return reply.result;
}

function exitWatchSupport() {
  const probe = spawnSync('python3', ['-c', "import time; s=open('/proc/self/stat').read().rsplit(') ',1)[1].split(); assert s[19].isdigit(); time.monotonic()"], { timeout: 10_000 });
  return probe.status === 0;
}

export async function f1IsolationChecks(check, stateRoot) {
  if (process.platform !== 'linux') {
    // Explicit, never silent: the PID-namespace integration only exists on Linux. No bwrap here.
    check('f1-isolation-integration-linux-only', true, { skipped: true, reason: 'PID-namespace isolation integration needs Linux bwrap', platform: process.platform });
    return;
  }
  const gitObjects = gitObjectsSource();
  const preconditions = {
    bwrap: existsSync(BWRAP), helperSource: existsSync(HELPER), wrapperSource: existsSync(WRAPPER),
    gitObjects: gitObjects.path !== null, pythonExitWatch: exitWatchSupport(),
  };
  const ready = Object.values(preconditions).every(Boolean);
  check('f1-isolation-requirements-available', ready, { required: preconditions, gitObjects, bwrap: BWRAP, note: 'Linux requires bwrap, the wrapper git-objects bind source, the host helper and python3 with readable proc identity stamps; none is skipped silently' });
  if (!ready) return;

  mkdirSync(stateRoot, { recursive: true });
  const root = realpathSync(mkdtempSync(join(stateRoot, 'f1-isolation-')));
  const logDir = join(root, 'panes');
  // The helper refuses an existing host-helper dir, so it is deliberately not created here.
  for (const dir of ['base-home', 'base-tmp', 'base-data', 'base-runtime', 'home', 'tmp', 'fixture']) mkdirSync(join(root, dir), { recursive: true });
  const fixtureDir = join(root, 'fixture');
  // The wrapper finds f1-host-helper.mjs next to itself, so it is staged with every module.
  const staged = stageModules(SELF, fixtureDir);
  stageModules(HELPER, fixtureDir, staged);
  const wrapper = join(fixtureDir, STAGED_WRAPPER);
  copyFileSync(WRAPPER, wrapper);
  chmodSync(wrapper, 0o755);
  const module = join(fixtureDir, basename(SELF));
  const bun = join(root, 'bun');
  copyFileSync(realpathSync(process.execPath), bun);
  chmodSync(bun, 0o755);
  const shell = join(root, 'shell.sh');
  writeFileSync(shell, `#!/bin/sh\nexec '${wrapper}' '${module}' pane-child '${logDir}'\n`, { mode: 0o755 });
  const socket = join(root, 'host-helper', 'helper.sock');
  check('f1-isolation-staged-current-source', readFileSync(wrapper, 'utf8') === readFileSync(WRAPPER, 'utf8') && ['f1-host-helper.mjs', 'f1-process-identity.mjs', basename(SELF)].every((name) => staged.has(name) && readFileSync(join(fixtureDir, name), 'utf8') === readFileSync(join(QA_DIR, name), 'utf8')),
    { root, bun, staged: [...staged], note: 'byte-identical copies of the current sources' });

  // Initial invocation: not yet isolated, so the wrapper must enter bwrap --unshare-pid.
  const env = { PATH: process.env.PATH ?? '/usr/bin:/bin', LANG: 'C.UTF-8', USER: process.env.USER ?? 'qa', F1_GIT_OBJECTS: gitObjects.path, F1_ISOLATION_ROOT: root, F1_BUN: bun };
  const hostNamespace = readlinkSync('/proc/self/ns/pid');
  let initial = null; let events = null; let parentLocal = null;
  try {
    initial = spawn('/bin/sh', [wrapper, module, 'inner-parent'], { env, stdio: ['pipe', 'pipe', 'pipe'] });
    events = new Events(initial, 'initial wrapper');
    const control = (request) => initial.stdin.write(`${JSON.stringify(request)}\n`);
    const parent = await events.next(tagged('parent-ready'));
    parentLocal = parent.identity;
    const parentHost = scanHost(parentLocal);
    check('f1-isolation-initial-bwrap-new-pid-namespace', Boolean(parentHost) && parentLocal.pidNamespace !== hostNamespace && parent.isolated === '1' && parent.socket === socket
      && parentLocal.args[0] === bun && parentLocal.args.includes(module),
      { hostNamespace, parent: parentLocal, hostPid: parentHost?.pid ?? null, isolated: parent.isolated, socket: parent.socket });

    const mapVia = async (local) => {
      const record = await helperReply(socket, { op: 'register', identity: { namespacePid: local.namespacePid, pidNamespace: local.pidNamespace, startStamp: local.startStamp } });
      const host = scanHost(local);
      const file = join(root, 'host-helper', 'map', `map-${local.pidNamespace.match(/\d+/)[0]}-${local.namespacePid}-${local.startStamp}.json`);
      return { host, record, file: existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : null };
    };
    const mappingOk = ({ host, record, file }) => Boolean(host && file) && record.hostPid === host.pid && record.startStamp === host.startStamp
      && record.pidNamespace === host.pidNamespace && record.namespacePid === host.namespacePid && record.hostPgrp === host.pgrp
      && file.hostPid === host.pid && file.startStamp === host.startStamp;

    // Pane through a daemon-like env rebuild -> SHELL wrapper -> wrapper re-entry.
    const paneReady = events.next(tagged('pane', 'pane-ready'));
    const endRecorded = events.next(tagged('pane', 'end-recorded'));
    const paneExit = events.next(tagged('pane-exit'));
    control({ op: 'spawn-pane', shell });
    const pane = (await paneReady).line;
    const paneLocal = pane.identity;
    check('f1-isolation-reentry-same-outer-pid-namespace', paneLocal.pidNamespace === parentLocal.pidNamespace && paneLocal.pidNamespace !== hostNamespace
      && paneLocal.pid !== parentLocal.pid && pane.isolated === '1' && pane.socket === socket && paneLocal.args[0] === bun && paneLocal.args.includes(module) && paneLocal.args.includes(logDir),
      { parent: parentLocal, pane: paneLocal, isolated: pane.isolated, socket: pane.socket });

    const paneMap = await mapVia(paneLocal);
    check('f1-isolation-helper-maps-recorded-identity-to-host-pid', mappingOk(paneMap) && paneMap.record.hostPid !== paneLocal.pid,
      { local: paneLocal, host: paneMap.host && { pid: paneMap.host.pid, startStamp: paneMap.host.startStamp, pidNamespace: paneMap.host.pidNamespace }, record: paneMap.record, file: paneMap.file });
    const identity = paneMap.record;
    const paneLog = join(logDir, `pane-${paneLocal.pid}.jsonl`);
    // Observation and group liveness go through the helper RPC (no process.env mutation here).
    const observe = (value) => helperReply(socket, { op: 'observe', identity: value });
    const groupAlive = (value) => helperReply(socket, { op: 'groupAlive', identity: value });
    const state = async (records) => paneTeardownState(records, await observe(identity), module, logDir);

    // signalPid is the helper-observed namespace-local pid. It is only an ownership token here:
    // on the host it could collide with an unrelated process, so it is never passed to kill.
    const live = await state(readJsonl(paneLog));
    check('f1-isolation-live-pane-not-clean-without-end', live.owned && live.alive && !live.clean && live.signalPid === paneLocal.namespacePid && live.signalPid !== identity.hostPid, live);
    // End record present, process still alive: the record must not make the fleet clean.
    control({ op: 'pane-stdin-close' });
    await endRecorded;
    const withEnd = await state(readJsonl(paneLog));
    check('f1-isolation-live-pane-with-end-record-fails-fleet-teardown', withEnd.ended && withEnd.owned && withEnd.alive && !withEnd.clean && !fleetTeardownClean([{ daemons: [], panes: [withEnd] }]), withEnd);
    // Signal the independently verified host pid (same start stamp) after the helper armed its
    // pidfd, then await the real exit. withEnd.signalPid is namespace-local and never signalled.
    check('f1-isolation-signal-target-is-verified-host-pid', withEnd.signalPid === paneLocal.namespacePid && paneMap.host.pid === identity.hostPid && paneMap.host.startStamp === identity.startStamp, { signalPid: withEnd.signalPid, hostPid: identity.hostPid, verifiedHostPid: paneMap.host.pid });
    if (paneMap.host.pid !== identity.hostPid || paneMap.host.startStamp !== identity.startStamp) throw new Error('refusing to signal an unverified host pid');
    const waited = await signalAndAwaitExit(socket, identity, paneMap.host.pid, 'SIGTERM');
    await paneExit;
    const afterExit = await state(readJsonl(paneLog));
    const afterObservation = await observe(identity);
    check('f1-isolation-pane-clean-only-after-observed-exit', waited.exited === true && afterExit.ended && !afterExit.alive && afterExit.clean && afterObservation.verdict === 'gone' && fleetTeardownClean([{ daemons: [], panes: [afterExit] }]),
      { waited, afterExit, observation: afterObservation });

    // Group whose leader exits while a member remains.
    const leaderReady = events.next(tagged('group', 'leader-ready'));
    const memberReady = events.next(tagged('group', 'member-ready'));
    const leaderExit = events.next(tagged('leader-exit'));
    const groupClosed = events.next(tagged('group-closed'));
    control({ op: 'spawn-group' });
    const leaderLocal = (await leaderReady).line.identity;
    const memberLocal = (await memberReady).line.identity;
    const leaderMap = await mapVia(leaderLocal);
    const memberMap = await mapVia(memberLocal);
    const leaderIdentity = leaderMap.record;
    // Captured while the leader is alive: the helper remembers the member once the leader is gone.
    const beforeExit = await groupAlive(leaderIdentity);
    check('f1-isolation-group-members-share-leader-pgrp', mappingOk(leaderMap) && mappingOk(memberMap) && memberLocal.pgrp === leaderLocal.pgrp && leaderMap.host.pgrp === memberMap.host.pgrp
      && beforeExit === true,
      { leaderLocal, memberLocal, leaderHostPgrp: leaderMap.host?.pgrp, memberHostPgrp: memberMap.host?.pgrp, helperGroupAlive: beforeExit });
    control({ op: 'leader-exit' });
    await leaderExit;
    const leaderGone = await observe(leaderIdentity);
    const remaining = await groupAlive(leaderIdentity);
    const stuck = { daemons: [{ exited: { code: 0 }, groupAlive: remaining }], panes: [] };
    check('f1-isolation-leader-exited-member-remains-fails-fleet-teardown', leaderGone.verdict === 'gone' && remaining === true && fleetTeardownClean([stuck]) === false,
      { leaderObservation: leaderGone, groupAlive: remaining, memberHostPid: memberMap.record.hostPid });
    const memberWaited = await signalAndAwaitExit(socket, memberMap.record, memberMap.record.hostPid, 'SIGTERM');
    await groupClosed;
    const gone = await groupAlive(leaderIdentity);
    check('f1-isolation-group-clean-after-member-exit', memberWaited.exited === true && gone === false && fleetTeardownClean([{ daemons: [{ exited: { code: 0 }, groupAlive: gone }], panes: [] }]),
      { memberWaited, groupAlive: gone });

    // Quitting the namespace's first process ends bwrap, then the wrapper-owned helper. Its
    // socket is gone afterwards, so the end state is read from the host /proc scan only.
    const closed = new Promise((resolve) => (events.closed ? resolve(events.closed) : initial.once('close', (code, signal) => resolve({ code, signal }))));
    control({ op: 'quit' });
    const finalExit = await closed;
    check('f1-isolation-namespace-torn-down', finalExit.code === 0 && !scanHost(parentLocal) && !scanHost(paneLocal) && !scanHost(leaderLocal) && !scanHost(memberLocal) && !existsSync(socket), { finalExit, socketPresent: existsSync(socket) });
  } catch (error) {
    check('f1-isolation-integration-run', false, { error: String(error?.stack ?? error), stderr: events?.stderr?.slice(-2000) });
  } finally {
    if (initial && !events.closed) {
      // `initial` is the wrapper-owned helper, not bwrap: SIGKILL on it would leak the namespace.
      // SIGTERM is forwarded to bwrap; await the real close. If that stalls, kill only the
      // verified namespace init (the inner parent), which tears the whole PID namespace down.
      const closed = new Promise((resolve) => initial.once('close', resolve));
      initial.kill('SIGTERM');
      const timer = setTimeout(() => {
        const init = parentLocal && scanHost(parentLocal);
        if (init) { try { process.kill(init.pid, 'SIGKILL'); } catch (error) { if (error?.code !== 'ESRCH') throw error; } }
      }, 15_000);
      await closed;
      clearTimeout(timer);
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const mode = process.argv[2];
  if (mode === 'inner-parent') runInnerParent();
  else if (mode === 'pane-child') runPaneChild(process.argv[3]);
  else if (mode === 'group-leader') runGroupLeader();
  else if (mode === 'group-member') runGroupMember();
  else process.exit(2);
}
