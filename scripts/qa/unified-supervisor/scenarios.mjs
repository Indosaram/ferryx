// Scenario registry for scripts/qa/unified-supervisor.mjs.
//
// Every scenario named by the plan's "Scenario fixtures and exact observable
// contracts" table is registered here. A registered-but-unimplemented scenario
// reports NOT_IMPLEMENTED (exit 3) and can never PASS. Each later plan task
// replaces its `planned(...)` entry with `{ implemented: true, task, summary,
// deadlineMs, requiredAssertions, run(ctx) }` in the same increment that
// delivers the behaviour. ctx.spawn/ctx.run/ctx.cli default to the isolated
// product environment; pass `env: process.env` only to opt in to inheritance.
//
// The __selftest-* scenarios exist only for --self-test and are reachable only
// through --internal-self-test-child, which the CLI accepts only with the
// per-run nonce the self-test parent creates.
import { accessSync, chmodSync, constants as fsConstants, existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, statSync, watch, writeFileSync } from 'node:fs';
import { createConnection, createServer } from 'node:net';
import { delimiter, dirname, isAbsolute, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HarnessError, isInside, POSIX } from './harness.mjs';

export const SLEEPER = fileURLToPath(new URL('./fixture-sleeper.mjs', import.meta.url));
export const WORKER = fileURLToPath(new URL('./fixture-worker.mjs', import.meta.url));
const PLANNED_DEADLINE_MS = 15 * 60_000;

const planned = (task, summary) => Object.freeze({ implemented: false, task, summary, deadlineMs: PLANNED_DEADLINE_MS });

// ---------------------------------------------------------------------------
// admission (plan task 7): setup -> isolated daemon -> owned `ferryx-cli mcp` -> submit,
// same-id replay, edited replay and "What can you do?". Every product effect goes through
// `ferryx-cli supervisor setup`, the daemon protocol (RegisterWorkspace, the request the GUI
// issues) and MCP `supervisor.request`; the isolated hub database is only read, read-only.

const ADMISSION_WORKSPACE_ID = 'qa-admission';
const ADMISSION_PROJECT = 'qa-supervisor';
const ADMISSION_REPO_IDENTITY = 'qa:qa-supervisor';
const ADMISSION_HOST = 'local';
const ADMISSION_BODY = 'Create result.txt containing READY. Run the verifier. Correct failures until it passes.';
const ADMISSION_EDITED_BODY = 'Create result.txt containing DONE. Run the verifier. Correct failures until it passes.';
const ADMISSION_QUESTION = 'What can you do?';
const ADMISSION_WORK = Object.freeze({
  title: 'Write result.txt',
  instruction: 'Create result.txt containing READY. Run the verifier. Correct failures until it passes.',
  requirements: [{ id: 'r1', text: 'result.txt contains exactly READY', verifier: 'verify.sh exits 0' }],
  operations: ['edit', 'test'],
});
// Daemon protocol version of this source revision (src-tauri/src/daemon/protocol.rs); a
// daemon that speaks another one answers protocolMismatch with its own, which is used instead.
const DAEMON_PROTOCOL_VERSION = 5;
const HELD_STATES = new Set(['degraded', 'awaiting_owner', 'delivery_uncertain', 'cancelled', 'paused', 'completed']);

const shellQuote = (value) => `'${String(value).replace(/'/g, `'\\''`)}'`;

function onPath(name) {
  for (const dir of (process.env.PATH ?? '').split(delimiter)) {
    if (!dir || !isAbsolute(dir)) continue;
    const candidate = join(dir, name);
    try {
      accessSync(candidate, fsConstants.X_OK);
      if (statSync(candidate).isFile()) return candidate;
    } catch { /* not here */ }
  }
  return null;
}

function freeLoopbackPort() {
  return new Promise((resolvePort, rejectPort) => {
    const server = createServer();
    server.unref();
    server.on('error', rejectPort);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => resolvePort(port));
    });
  });
}

// One connection, newline-delimited JSON frames, one reply per frame (the CLI's exchange).
function daemonExchange(socketPath, frames, deadlineMs) {
  return new Promise((resolveExchange, rejectExchange) => {
    const replies = [];
    let buffer = '';
    let next = 0;
    let settled = false;
    const connection = createConnection(socketPath);
    const finish = (fn, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      connection.destroy();
      fn(value);
    };
    const timer = setTimeout(() => finish(rejectExchange, new HarnessError('DEADLINE_EXCEEDED', `daemon exchange did not finish within ${deadlineMs}ms`, { replies })), deadlineMs);
    const send = () => {
      if (next >= frames.length) { finish(resolveExchange, replies); return; }
      connection.write(`${JSON.stringify(frames[next])}\n`);
      next += 1;
    };
    connection.setEncoding('utf8');
    connection.on('connect', send);
    connection.on('error', (error) => finish(rejectExchange, new HarnessError('DAEMON_IO', `daemon socket ${socketPath}: ${error?.message ?? error}`)));
    connection.on('close', () => finish(rejectExchange, new HarnessError('DAEMON_IO', 'the daemon closed the connection before every reply arrived', { replies })));
    connection.on('data', (chunk) => {
      buffer += chunk;
      for (let index = buffer.indexOf('\n'); index >= 0 && !settled; index = buffer.indexOf('\n')) {
        const line = buffer.slice(0, index);
        buffer = buffer.slice(index + 1);
        let json = null;
        try { json = JSON.parse(line); } catch { /* recorded raw */ }
        replies.push(json ?? { unparsed: line });
        if (json?.type === 'protocolMismatch' || json?.type === 'error') finish(resolveExchange, replies);
        else send();
      }
    });
  });
}

async function registerWorkspace(ctx, socketPath, workspaceId, repoRoot) {
  const request = { type: 'registerWorkspace', workspaceId, repoRoot };
  let version = DAEMON_PROTOCOL_VERSION;
  for (let attempt = 0; attempt < 2; attempt += 1) {
    const frames = [{ type: 'handshake', version }, request];
    ctx.evidence.appendJsonl('requests.jsonl', { label: 'register-workspace', surface: 'daemon-socket', frames, at: new Date().toISOString() });
    const replies = await daemonExchange(socketPath, frames, 30_000);
    ctx.evidence.appendJsonl('responses.jsonl', { label: 'register-workspace', replies, at: new Date().toISOString() });
    const mismatch = replies.find((reply) => reply?.type === 'protocolMismatch');
    if (mismatch && attempt === 0 && Number.isInteger(mismatch.expectedVersion)) { version = mismatch.expectedVersion; continue; }
    return replies;
  }
  return [];
}

async function openHubReader(dbPath) {
  let Database;
  try { ({ Database } = await import('bun:sqlite')); } catch {
    throw new HarnessError('SQLITE_UNAVAILABLE', 'the admission scenario reads the isolated hub database with bun:sqlite; run the runner with bun');
  }
  const all = (sql, ...params) => {
    const db = new Database(dbPath, { readonly: true });
    try { return db.query(sql).all(...params); } finally { db.close(); }
  };
  return {
    all,
    one: (sql, ...params) => all(sql, ...params)[0] ?? null,
    count: (sql, ...params) => Number(Object.values(all(sql, ...params)[0] ?? { n: -1 })[0]),
  };
}

// Subscribe to a directory (the hub directory: every SQLite commit writes the -wal file; the
// worker log directory: every worker line is an append), then reconcile: `check` runs once now
// and again after each change, until it returns { done: true } or the deadline passes.
function waitForChange(ctx, label, dir, check, deadlineMs) {
  return new Promise((resolveWait, rejectWait) => {
    let settled = false;
    let watcher = null;
    let last = null;
    const finish = (fn, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try { watcher?.close(); } catch { /* already closed */ }
      ctx.signal.removeEventListener('abort', onAbort);
      fn(value);
    };
    const onAbort = () => finish(rejectWait, ctx.signal.reason);
    const evaluate = () => {
      if (settled) return;
      try {
        const result = check();
        if (result?.done) finish(resolveWait, result);
        else last = result ?? last;
      } catch (error) {
        // A read racing a checkpoint can fail once; the next change re-reads.
        last = { error: String(error?.message ?? error) };
      }
    };
    const timer = setTimeout(() => finish(rejectWait, new HarnessError('DEADLINE_EXCEEDED', `${label} was not observed within ${deadlineMs}ms`, { last })), deadlineMs);
    if (ctx.signal.aborted) { onAbort(); return; }
    ctx.signal.addEventListener('abort', onAbort, { once: true });
    try {
      watcher = watch(dir, evaluate);
      watcher.on?.('error', (error) => finish(rejectWait, new HarnessError('WATCH_FAILED', `${label}: ${error?.message ?? error}`)));
    } catch (error) {
      finish(rejectWait, new HarnessError('WATCH_FAILED', `${label}: ${error?.message ?? error}`));
      return;
    }
    evaluate();
  });
}

function readWorkerLogs(logDir) {
  if (!existsSync(logDir)) return [];
  return readdirSync(logDir).filter((name) => /^worker-\d+\.jsonl$/.test(name)).sort().map((name) => {
    const text = readFileSync(join(logDir, name), 'utf8');
    const entries = text.split('\n').filter(Boolean).map((line) => { try { return JSON.parse(line); } catch { return { unparsed: line }; } });
    return { name, text, entries, sessionId: entries.find((entry) => entry.kind === 'ready')?.sessionId ?? null };
  });
}

async function runAdmission(ctx) {
  if (!POSIX) {
    throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', 'admission fixture worker needs a POSIX pane shell (the Windows pane shell is pwsh/powershell and ignores SHELL); this scenario runs on Linux or macOS');
  }
  const { config } = ctx;
  const git = onPath('git');
  if (!git) throw new HarnessError('PREREQUISITE_MISSING', 'git is not on PATH; the admission fixture project is a git repository');
  const data = realpathSync(ctx.paths.data);
  const dbPath = join(data, 'hub', 'hub.sqlite');
  if (!isInside(dbPath, data)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `hub database ${dbPath} is outside the run root`);

  // Fixture: a plain workspace root holding the project checkout and, beside it, the
  // task-owned lane worktrees the dispatcher creates (<repo>-lanes/<lane>).
  const workspaceRoot = join(data, 'ws');
  const repo = join(workspaceRoot, ADMISSION_PROJECT);
  mkdirSync(repo, { recursive: true });
  const gitArgs = ['-c', 'init.defaultBranch=main', '-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid'];
  for (const [label, args] of [['git-init', [...gitArgs, 'init', '-q', repo]], ['git-commit', [...gitArgs, '-C', repo, 'commit', '-q', '--allow-empty', '-m', 'qa fixture']]]) {
    const result = await ctx.run(label, git, args, { deadlineMs: 30_000 });
    if (result.code !== 0) throw new HarnessError('FIXTURE_FAILED', `${label} exited ${result.code}: ${result.stderr}`);
  }
  const gitCommonDir = realpathSync(join(repo, '.git'));
  const workerLogs = join(data, 'workers');
  mkdirSync(workerLogs, { recursive: true });
  const shell = join(data, 'bin', 'qa-worker-shell');
  mkdirSync(dirname(shell), { recursive: true });
  // `-c`/`-lc`/`-lic` probes (login PATH resolution) run a real /bin/sh; a pane (`-l`) runs the worker.
  writeFileSync(shell, `#!/bin/sh\ncase "$1" in\n  -*c*) shift; exec /bin/sh -c "$1" ;;\nesac\nexec ${shellQuote(process.execPath)} ${shellQuote(WORKER)} ${shellQuote(workerLogs)}\n`, { mode: 0o755 });
  const port = await freeLoopbackPort();
  const env = ctx.productEnv({
    SHELL: shell,
    FERRYX_HUB_TRACKER_ADDR: `127.0.0.1:${port}`,
    FERRYX_HUB_WORKSPACE: ADMISSION_WORKSPACE_ID,
    OMON_LANE_AUTO_WORKTREE: '1',
  });
  ctx.announce({ admission: 'fixture', workspaceRoot, repo, gitCommonDir, hubHttp: env.FERRYX_HUB_TRACKER_ADDR, dbPath });

  // 1. Installation set-up through the product CLI: created, idempotent, conflict refused.
  const setupArgs = (dir) => ['supervisor', 'setup', '--owner', config.ownerPrincipal, '--project', ADMISSION_PROJECT,
    '--repo-identity', ADMISSION_REPO_IDENTITY, '--host', ADMISSION_HOST, '--git-common-dir', dir, '--json'];
  const setup = await ctx.cli(setupArgs(gitCommonDir), { label: 'supervisor-setup', env });
  ctx.assert('setup-created-identity-project-and-host', setup.code === 0 && setup.json?.ok === true && setup.json?.ownerPrincipal === config.ownerPrincipal
    && typeof setup.json?.supervisorId === 'string' && typeof setup.json?.projectId === 'string' && setup.json?.hostId === ADMISSION_HOST
    && setup.json?.gitCommonDir === gitCommonDir, { code: setup.code, json: setup.json, stderr: setup.stderr });
  const rerun = await ctx.cli(setupArgs(gitCommonDir), { label: 'supervisor-setup-rerun', env });
  ctx.assert('setup-rerun-returns-the-same-ids', rerun.code === 0 && rerun.json?.supervisorId === setup.json?.supervisorId
    && rerun.json?.projectId === setup.json?.projectId, { code: rerun.code, json: rerun.json });
  const moved = await ctx.cli(setupArgs(join(data, 'elsewhere', '.git')), { label: 'supervisor-setup-conflict', env });
  ctx.assert('setup-conflicting-rebind-refused', moved.code === 4 && moved.json?.code === 'SETUP_CONFLICT', { code: moved.code, json: moved.json });
  if (!setup.json?.projectId) throw new HarnessError('SETUP_FAILED', 'supervisor setup did not return a project; nothing further can be admitted');

  // 2. Isolated daemon (its hub runs the dispatcher) and the fixture workspace.
  await ctx.startDaemon({ env, deadlineMs: 60_000 });
  const registered = await registerWorkspace(ctx, join(data, 'runtime', 'daemon.sock'), ADMISSION_WORKSPACE_ID, realpathSync(workspaceRoot));
  ctx.assert('workspace-registered', registered.at(-1)?.type === 'registerWorkspaceOk', { replies: registered });

  // 3. The owner's MCP channel, an owned stdio process.
  const mcp = await ctx.spawn('mcp', config.ferryxCli, ['mcp'], { env, cwd: data, stdin: 'pipe', graceMs: 3_000 });
  mcp.child.stdin.on('error', () => { /* the exit event decides */ });
  const toolsCalled = [];
  let rpcId = 0;
  const call = async (label, name, args) => {
    rpcId += 1;
    const id = rpcId;
    const frame = { jsonrpc: '2.0', id, method: 'tools/call', params: { name, arguments: args } };
    toolsCalled.push(name);
    const cursor = ctx.journal.cursor();
    ctx.evidence.appendJsonl('requests.jsonl', { label, surface: 'ferryx-cli mcp', frame, at: new Date().toISOString() });
    mcp.child.stdin.write(`${JSON.stringify(frame)}\n`);
    const reply = await ctx.waitFor(`mcp reply to ${label}`, (e) => e.source === 'mcp' && e.kind === 'stdout' && e.json?.id === id, { after: cursor, deadlineMs: 30_000 });
    let body = null;
    try { body = JSON.parse(reply.json?.result?.content?.[0]?.text ?? 'null'); } catch { /* asserted by the caller */ }
    const result = { error: reply.json?.error ?? null, body };
    ctx.evidence.appendJsonl('responses.jsonl', { label, reply: reply.json, body, at: new Date().toISOString() });
    return result;
  };
  const hub = await openHubReader(dbPath);
  const counts = () => ({
    goals: hub.count('SELECT COUNT(*) AS n FROM supervisor_goals'),
    requests: hub.count('SELECT COUNT(*) AS n FROM supervisor_requests'),
    dispatches: hub.count('SELECT COUNT(*) AS n FROM supervisor_dispatches'),
    readyIntents: hub.count("SELECT COUNT(*) AS n FROM supervisor_dispatches WHERE state = 'ready'"),
    workers: readWorkerLogs(workerLogs).length,
  });

  // 4. Submit the fixture request.
  const sourceId = `qa-admission-${ctx.runId}`;
  const submit = { requestId: sourceId, action: 'submit', payload: { request: ADMISSION_BODY, work: ADMISSION_WORK } };
  const first = await call('submit', 'supervisor.request', submit);
  const receipt = first.body?.receipt ?? null;
  const goalId = receipt?.goal_id ?? null;
  ctx.assert('submit-admitted-with-acknowledgment-naming-goal', first.error === null && first.body?.outcome === 'admitted' && first.body?.duplicate === false
    && typeof goalId === 'string' && typeof first.body?.acknowledgment === 'string' && first.body.acknowledgment.includes(goalId)
    && first.body?.mode === 'full' && receipt?.source_message_id === sourceId, { error: first.error, body: first.body });
  if (!goalId) throw new HarnessError('ADMISSION_FAILED', 'the submit returned no goal; nothing can be dispatched');

  // 5. The daemon's dispatcher starts the worker by itself: no drive call is ever made.
  const dispatched = await waitForChange(ctx, 'initial dispatch to start or hold', dirname(dbPath), () => {
    const goal = hub.one('SELECT run_state, state_detail, revision, lane_key FROM supervisor_goals WHERE goal_id = ?', goalId);
    const dispatches = hub.all('SELECT request_id, state, run_id, provider_session_id, uncertain_code, generation FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id', goalId);
    const started = dispatches.some((row) => row.state === 'started' || row.state === 'settled');
    const done = (started && goal?.run_state !== 'dispatching') || HELD_STATES.has(goal?.run_state);
    return { done, goal, dispatches };
  }, 120_000);
  const binding = hub.one('SELECT kind, host_id, provider_session_id, run_id, generation FROM supervisor_execution_bindings WHERE goal_id = ?', goalId);
  const dispatch = dispatched.dispatches[0] ?? null;
  ctx.assert('initial-dispatch-started-without-drive-call', dispatched.goal?.run_state === 'running' && dispatched.dispatches.length === 1
    && dispatch?.state === 'started' && dispatch?.request_id === receipt.dispatch_request_id && typeof dispatch?.provider_session_id === 'string'
    && binding?.provider_session_id === dispatch.provider_session_id && binding?.kind === 'pty' && toolsCalled.every((name) => name === 'supervisor.request'),
  { goal: dispatched.goal, dispatches: dispatched.dispatches, binding, toolsCalled });
  // The start may be confirmed from the pane's echo before the worker's own report is logged.
  const reportedWorking = (entry) => entry.entries.some((line) => line.kind === 'reported' && line.state === 'working');
  const workerSettled = await waitForChange(ctx, 'worker working report', workerLogs, () => {
    const workers = readWorkerLogs(workerLogs);
    return { done: workers.length > 0 && workers.every((entry) => reportedWorking(entry) || entry.entries.some((line) => line.kind === 'report-failed' || line.kind === 'report-skipped')), workers: workers.length };
  }, 30_000).then(() => true, (error) => { ctx.log(`worker report wait: ${error?.message ?? error}`); return false; });
  const workersAfterStart = readWorkerLogs(workerLogs);
  const worker = workersAfterStart[0] ?? null;
  ctx.assert('exactly-one-worker-started', workerSettled && workersAfterStart.length === 1 && worker?.sessionId === dispatch?.provider_session_id
    && worker.entries.some((entry) => entry.kind === 'input' && entry.text.trim() !== '') && reportedWorking(worker),
  { workers: workersAfterStart.map(({ name, sessionId, entries }) => ({ name, sessionId, entries })) });
  const afterStart = counts();

  // 6. Replay of the same source id and body: the same receipt, nothing new.
  const replay = await call('replay', 'supervisor.request', submit);
  const afterReplay = counts();
  ctx.assert('replay-returns-the-same-receipt', replay.error === null && replay.body?.duplicate === true
    && JSON.stringify(replay.body?.receipt) === JSON.stringify(receipt), { error: replay.error, body: replay.body, receipt });
  ctx.assert('replay-created-no-second-dispatch', JSON.stringify(afterReplay) === JSON.stringify(afterStart) && afterReplay.readyIntents === 0,
    { afterStart, afterReplay });

  // 7. The same source id with an edited body: a conflict naming the goal, nothing new.
  const edited = await call('edited-replay', 'supervisor.request', { ...submit, payload: { request: ADMISSION_EDITED_BODY, work: ADMISSION_WORK } });
  const afterEdited = counts();
  ctx.assert('edited-replay-returns-request-conflict', edited.body === null && typeof edited.error?.message === 'string'
    && edited.error.message.startsWith('REQUEST_CONFLICT') && edited.error.message.includes(goalId), { error: edited.error, body: edited.body });
  ctx.assert('edited-replay-created-no-second-worker', JSON.stringify(afterEdited) === JSON.stringify(afterStart) && afterEdited.readyIntents === 0,
    { afterStart, afterEdited });

  // 8. An ordinary question is conversation, also when a work proposal is wrongly attached.
  const question = await call('question', 'supervisor.request', { requestId: `${sourceId}-q1`, action: 'message', payload: { text: ADMISSION_QUESTION } });
  const questionWithWork = await call('question-with-work', 'supervisor.request', { requestId: `${sourceId}-q2`, action: 'message', payload: { text: ADMISSION_QUESTION, work: ADMISSION_WORK } });
  const afterQuestion = counts();
  ctx.assert('question-is-conversational', question.error === null && question.body?.outcome === 'conversational'
    && questionWithWork.error === null && questionWithWork.body?.outcome === 'conversational', { question: question.body ?? question.error, questionWithWork: questionWithWork.body ?? questionWithWork.error });
  ctx.assert('question-created-no-goal-or-dispatch', JSON.stringify(afterQuestion) === JSON.stringify(afterStart), { afterStart, afterQuestion });

  // 9. Final state, read once more: one goal, one request, one dispatch, one worker.
  const finalGoal = hub.one('SELECT run_state, revision FROM supervisor_goals WHERE goal_id = ?', goalId);
  const finalBinding = hub.one('SELECT provider_session_id, generation FROM supervisor_execution_bindings WHERE goal_id = ?', goalId);
  const finalCounts = counts();
  ctx.assert('final-one-goal-one-dispatch-one-worker', finalCounts.goals === 1 && finalCounts.requests === 1 && finalCounts.dispatches === 1
    && finalCounts.readyIntents === 0 && finalCounts.workers === 1 && finalBinding?.provider_session_id === dispatch?.provider_session_id,
  { finalCounts, finalGoal, finalBinding });

  const workers = readWorkerLogs(workerLogs);
  for (const entry of workers) ctx.evidence.writeText(`workers/${entry.name}`, entry.text);
  ctx.evidence.writeJson('admission.json', {
    sourceId, goalId, receipt, setup: setup.json, binding: finalBinding, goal: finalGoal, dispatches: dispatched.dispatches,
    counts: { afterStart, afterReplay, afterEdited, afterQuestion, final: finalCounts }, toolsCalled, hubHttp: env.FERRYX_HUB_TRACKER_ADDR,
  });
}

export const SCENARIOS = Object.freeze({
  runtime: planned('6', 'Real mhc: one structured assessment, restart the owned engine, resume the saved session from another cwd; missing capability refused'),
  admission: Object.freeze({
    implemented: true,
    task: '7',
    summary: 'Submit the fixture request, replay the same source id, ask "What can you do?": one goal, one initial dispatch',
    deadlineMs: 6 * 60_000,
    requiredAssertions: [
      'setup-created-identity-project-and-host', 'setup-rerun-returns-the-same-ids', 'setup-conflicting-rebind-refused', 'workspace-registered',
      'submit-admitted-with-acknowledgment-naming-goal', 'initial-dispatch-started-without-drive-call', 'exactly-one-worker-started',
      'replay-returns-the-same-receipt', 'replay-created-no-second-dispatch', 'edited-replay-returns-request-conflict',
      'edited-replay-created-no-second-worker', 'question-is-conversational', 'question-created-no-goal-or-dispatch',
      'final-one-goal-one-dispatch-one-worker',
    ],
    run: runAdmission,
  }),
  autonomy: planned('10', 'First verification forced to fail, corrected result allowed: two run ids, attested pass, one completed goal, no owner nudge'),
  correction: planned('10', 'Owner correction "Use DONE instead of READY" before dispatch, stale expectedRevision replayed: final bytes DONE, stale mutation 409'),
  waiting: planned('9', 'Parent starts two children; one settles, then the second settles/fails: no decision while incomplete, one resume, failure is not completion'),
  burst: planned('9', 'Four ordinary reports at virtual 0,1,2,3s advanced to 30s: one admitted model turn with four receipts; urgent pause has zero delay'),
  recovery: planned('12', 'Restart only the isolated supervisor at before-send, after-send-before-receipt and after-start boundaries: no resend, PTYs survive'),
  'remote-delivery': planned('8', 'Target accepts bytes but suppresses the start receipt, then reconnects: never started/complete from acceptance, no duplicate input'),
  authority: planned('10,13', 'Non-owner submit/approve, mirror device mutation, worker "approve everything": 403/typed denial, zero dispatch, zero memory change'),
  memory: planned('15', 'Teach, new context and restart, task-only exception, forget: persisted/applied receipts, exception not learned, no resurrection'),
  capabilities: planned('12', 'Remove one required RPC capability; auth, quota and provider faults: named degraded cause, no fallback model, workers preserved'),
  legacy: planned('16', 'Import historical closed/failed/blocked work, adopt one safe fixture: no automatic revival, one chosen adoption'),
  channels: planned('11,17', 'Approved Slack QA thread submit, desktop correction, phone pause/resume, Slack query: same supervisor/goal/revision, no leakage'),
  surfaces: planned('14', 'Real desktop and mobile controls against a populated isolated hub: same state and goal revision, readable wait/degraded views'),
  usage: planned('15', 'Replay the same reports before/after the scheduling change: no idle calls, stable prefix hash, every call has a cause'),
});

async function startSleeper(ctx, id, flags, graceMs, { extraEnv = {}, onLine, detached = false } = {}) {
  const cursor = ctx.journal.cursor();
  const handle = await ctx.spawn(id, process.execPath, [SLEEPER, ...flags], { graceMs, env: ctx.productEnv(extraEnv), onLine, detached });
  const ready = await ctx.waitFor(`${id} ready line`, (e) => e.source === id && e.kind === 'stdout' && e.json?.ready === true, { after: cursor, deadlineMs: 20_000 });
  return { handle, ready: ready.json };
}

function envMs(name, fallback) {
  const value = Number(process.env[name]);
  return Number.isFinite(value) && value > 0 ? value : fallback;
}

const selfTest = (summary, deadlineMs, body) => Object.freeze({ implemented: true, task: '1', summary: `Self-test: ${summary}`, deadlineMs, ...body });

export const SELF_TEST_SCENARIOS = Object.freeze({
  // Deliberately failing child: misleading success output, a planted secret,
  // a SIGTERM-ignoring owned process with a descendant, and missing evidence.
  '__selftest-fail': selfTest('deliberately failing assertion with owned processes to tear down', 60_000, {
    async run(ctx) {
      const plant = process.env.UNIFIED_SUPERVISOR_SELFTEST_PLANT_SLACK;
      const { handle, ready } = await startSleeper(ctx, 'sleeper', ['--ignore-sigterm', '--grandchild'], 1_000,
        { extraEnv: plant ? { UNIFIED_SUPERVISOR_SELFTEST_PLANT_SLACK: plant } : {} });
      ctx.announce({ selftest: 'armed', case: 'fail', sleeperPid: ready.pid, grandchildPid: ready.grandchildPid });
      ctx.log(`diagnostic line carrying a planted credential: ${plant ?? 'unset'}`);
      ctx.log(JSON.stringify({ apiKey: process.env.UNIFIED_SUPERVISOR_SELFTEST_PLANT_KEYED ?? 'unset' }));
      ctx.announce('PASS: all assertions passed (deliberately misleading output; assertions.json decides)');
      ctx.assert('sleeper-alive-before-teardown', handle.exitInfo === null, { pid: ready.pid });
      const required = join(ctx.paths.data, 'worker-result.txt');
      ctx.assert('worker-result-evidence-present', existsSync(required), { required, note: 'deliberately absent: missing evidence must fail the run' });
    },
  }),
  // Hangs on an event that never arrives: ends by step deadline or by SIGINT.
  '__selftest-hang': selfTest('wait on a never-emitted event (deadline or interrupt)', 180_000, {
    async run(ctx) {
      const ignoreTerm = process.env.UNIFIED_SUPERVISOR_SELFTEST_SLEEPER_IGNORE_TERM === '1';
      const { ready } = await startSleeper(ctx, 'sleeper', ignoreTerm ? ['--ignore-sigterm'] : [], ignoreTerm ? 5_000 : 3_000);
      const hangMs = envMs('UNIFIED_SUPERVISOR_SELFTEST_HANG_MS', 120_000);
      const cursor = ctx.journal.cursor();
      ctx.announce({ selftest: 'armed', case: 'hang', sleeperPid: ready.pid, hangMs });
      await ctx.waitFor('an event that is never emitted', (e) => e.kind === 'never-emitted', { after: cursor, deadlineMs: hangMs });
      ctx.assert('unreachable-after-hang', false);
    },
  }),
  // Positive control: proves the runner can PASS, that a hung command is
  // killed at its deadline, that a path barrier armed before its trigger
  // resolves, and that ctx.run/ctx.cli default to an isolated environment.
  '__selftest-hung-command': selfTest('positive control: bounded hung command, path barrier, isolated env', 60_000, {
    requiredAssertions: ['hung-command-timed-out', 'path-barrier-armed-before-trigger-resolves', 'run-default-env-is-isolated', 'cli-env-is-isolated', 'spawn-default-env-is-isolated'],
    async run(ctx) {
      const hung = await ctx.run('hung-command', process.execPath, [SLEEPER], { deadlineMs: 1_500, graceMs: 1_000 });
      ctx.assert('hung-command-timed-out', hung.timedOut === true, { timedOut: hung.timedOut, durationMs: hung.durationMs });
      ctx.assert('hung-command-terminated', hung.receipt?.exited === true && hung.receipt?.ok === true, hung.receipt);
      ctx.assert('hung-command-not-success', hung.code !== 0, { code: hung.code, signal: hung.signal });

      const marker = join(ctx.paths.data, 'barrier', 'ready.flag');
      mkdirSync(dirname(marker), { recursive: true });
      const barrier = ctx.waitForPath(marker, { deadlineMs: 15_000, label: 'barrier marker' });
      barrier.catch(() => { /* awaited below */ });
      const trigger = await ctx.run('barrier-trigger', process.execPath, ['-e', `require('node:fs').writeFileSync(${JSON.stringify(marker)}, 'ok')`], { deadlineMs: 15_000 });
      const observed = await barrier.then(() => true, () => false);
      ctx.assert('path-barrier-armed-before-trigger-resolves', trigger.code === 0 && observed, { code: trigger.code, stderr: trigger.stderr });

      const inside = (path) => typeof path === 'string' && isAbsolute(path) && !relative(ctx.paths.data, path).startsWith('..');
      const probe = 'process.stdout.write(JSON.stringify({ home: process.env.HOME, tmp: process.env.TMPDIR, dataDir: process.env.FERRYX_DATA_DIR, inherited: Object.keys(process.env).filter((k) => k.startsWith("UNIFIED_SUPERVISOR_")) }))';
      const ran = await ctx.run('run-env-echo', process.execPath, ['-e', probe], { deadlineMs: 15_000 });
      let ranJson = null;
      try { ranJson = JSON.parse(ran.stdout); } catch { /* asserted below */ }
      ctx.assert('run-default-env-is-isolated', ran.code === 0 && inside(ranJson?.home) && inside(ranJson?.tmp) && ranJson?.inherited?.length === 0,
        { code: ran.code, json: ranJson, stderr: ran.stderr });
      const echo = await ctx.cli(['-e', probe], { label: 'echo-env', deadlineMs: 15_000 });
      ctx.assert('cli-env-is-isolated', echo.code === 0 && inside(echo.json?.home) && inside(echo.json?.dataDir) && echo.json?.inherited?.length === 0,
        { code: echo.code, json: echo.json, stderr: echo.stderr });
      // ctx.spawn with no env must also get the isolated product environment.
      const spawnCursor = ctx.journal.cursor();
      const spawned = await ctx.spawn('spawn-env-echo', process.execPath, ['-e', probe]);
      const output = await ctx.waitFor('spawn-env-echo output', (e) => e.source === 'spawn-env-echo' && e.kind === 'stdout' && e.json !== null, { after: spawnCursor, deadlineMs: 15_000 });
      const exited = await ctx.waitFor('spawn-env-echo exit', (e) => e.source === 'spawn-env-echo' && e.kind === 'exit', { after: spawnCursor, deadlineMs: 15_000 });
      ctx.assert('spawn-default-env-is-isolated', exited.code === 0 && inside(output.json?.home) && inside(output.json?.tmp) && inside(output.json?.dataDir)
        && output.json?.inherited?.length === 0, { pid: spawned.pid, code: exited.code, json: output.json });
    },
  }),
  // Waits until the self-test parent SIGKILLs this runner, leaving orphans for
  // --cleanup to reclaim by recorded identity: a sleeper and (POSIX) a #! script
  // whose kernel argv starts with the interpreter rather than the launched path.
  // The sleeper is detached: on Windows a non-detached child is in libuv's
  // kill-on-close job and would die with the runner instead of being orphaned.
  '__selftest-orphan': selfTest('leave a ledgered orphan when the runner is SIGKILLed', 180_000, {
    async run(ctx) {
      const { ready } = await startSleeper(ctx, 'sleeper', [], 3_000, { detached: true });
      let scriptPid = null;
      if (process.platform !== 'win32') {
        const script = join(ctx.paths.data, 'worker.sh');
        writeFileSync(script, '#!/bin/sh\nprintf \'{"ready":true,"pid":%s}\\n\' "$$"\nsleep 240\n', { mode: 0o755 });
        const cursor = ctx.journal.cursor();
        const handle = await ctx.spawn('script', script, [], { graceMs: 3_000 });
        await ctx.waitFor('script ready line', (e) => e.source === 'script' && e.kind === 'stdout' && e.json?.ready === true, { after: cursor, deadlineMs: 20_000 });
        scriptPid = handle.pid;
      }
      ctx.announce({ selftest: 'armed', case: 'orphan', sleeperPid: ready.pid, scriptPid });
      await ctx.waitFor('the self-test parent to SIGKILL this runner', () => false, { deadlineMs: 120_000 });
      ctx.assert('unreachable-after-orphan-wait', false);
    },
  }),
  // PASS gating mutations: each of these must end nonzero with a named cause.
  '__selftest-no-assertions': selfTest('settles without recording any assertion', 30_000, {
    async run() { /* deliberately records nothing */ },
  }),
  '__selftest-throw': selfTest('throws an ordinary error', 30_000, {
    async run(ctx) {
      ctx.assert('before-throw', true);
      throw new Error('deliberate ordinary error (self-test)');
    },
  }),
  '__selftest-missing-required': selfTest('omits one declared required assertion', 30_000, {
    requiredAssertions: ['first-required', 'second-required'],
    async run(ctx) { ctx.assert('first-required', true); },
  }),
  // An assertion that arrives after the scenario settled (during teardown). Two
  // triggers: the ignoredSignal line (POSIX only; Windows never delivers SIGTERM)
  // and the sleeper's exit event, which can only happen during teardown.
  '__selftest-late-assertion': selfTest('records an assertion during teardown', 60_000, {
    async run(ctx) {
      const lateAssert = () => {
        try { ctx.assert('late-assertion-after-settle', false); } catch { /* LATE_ASSERTION is the expected refusal */ }
      };
      const onLine = (_kind, _line, json) => { if (json?.ignoredSignal) lateAssert(); };
      const cursor = ctx.journal.cursor();
      ctx.waitFor('sleeper exit during teardown', (e) => e.source === 'sleeper' && e.kind === 'exit', { after: cursor, deadlineMs: 60_000 })
        .then(lateAssert, () => { /* aborted or never exited: the run already fails another way */ });
      await startSleeper(ctx, 'sleeper', ['--ignore-sigterm'], 2_000, { onLine });
      ctx.assert('sleeper-ready', true);
    },
  }),
  // An unhandled rejection or uncaught exception during the run must end it as HARNESS_ERROR.
  '__selftest-fault': selfTest('unhandled rejection or uncaught exception mid-run', 60_000, {
    async run(ctx) {
      const { ready } = await startSleeper(ctx, 'sleeper', [], 3_000);
      ctx.announce({ selftest: 'armed', case: 'fault', sleeperPid: ready.pid });
      const cursor = ctx.journal.cursor();
      ctx.assert('fault-armed', true);
      if (process.env.UNIFIED_SUPERVISOR_SELFTEST_FAULT === 'exception') {
        setTimeout(() => { throw new Error('deliberate uncaught exception (self-test)'); }, 0);
      } else {
        Promise.reject(new Error('deliberate unhandled rejection (self-test)'));
      }
      await ctx.waitFor('the harness fault to be recorded', (e) => e.kind === 'harness-fault', { after: cursor, deadlineMs: 15_000 });
    },
  }),
  // All assertions pass but the run root cannot be removed (read-only subdirectory).
  '__selftest-cleanup-fail': selfTest('assertions pass but run-root removal fails', 30_000, {
    async run(ctx) {
      const locked = join(ctx.paths.data, 'locked');
      mkdirSync(locked, { recursive: true });
      writeFileSync(join(locked, 'keep.txt'), 'held by a read-only directory\n');
      chmodSync(locked, 0o500);
      ctx.announce({ selftest: 'armed', case: 'cleanup-fail', locked });
      ctx.assert('locked-directory-created', existsSync(join(locked, 'keep.txt')), { locked });
    },
  }),
});

export function resolveScenario(name, internal = false) {
  if (Object.hasOwn(SCENARIOS, name)) return SCENARIOS[name];
  if (internal && Object.hasOwn(SELF_TEST_SCENARIOS, name)) return SELF_TEST_SCENARIOS[name];
  throw new HarnessError('UNKNOWN_SCENARIO', `unknown scenario ${name}; registered: ${Object.keys(SCENARIOS).join(', ')}`);
}
