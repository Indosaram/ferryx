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
import { HarnessError, isInside, POSIX, probeProcess, waitForPath } from './harness.mjs';

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
  // A second real repository, outside the workspace: the target of the refused rebind.
  const elsewhere = join(data, 'elsewhere');
  mkdirSync(repo, { recursive: true });
  mkdirSync(elsewhere, { recursive: true });
  const gitArgs = ['-c', 'init.defaultBranch=main', '-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid'];
  for (const [label, args] of [
    ['git-init', [...gitArgs, 'init', '-q', repo]],
    ['git-commit', [...gitArgs, '-C', repo, 'commit', '-q', '--allow-empty', '-m', 'qa fixture']],
    ['git-init-elsewhere', [...gitArgs, 'init', '-q', elsewhere]],
  ]) {
    const result = await ctx.run(label, git, args, { deadlineMs: 30_000 });
    if (result.code !== 0) throw new HarnessError('FIXTURE_FAILED', `${label} exited ${result.code}: ${result.stderr}`);
  }
  const gitCommonDir = realpathSync(join(repo, '.git'));
  const elsewhereCommonDir = realpathSync(join(elsewhere, '.git'));
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

  // 1. Installation set-up through the product CLI: created, idempotent (also through another
  //    spelling of the same checkout, which the CLI canonicalizes), conflicts refused.
  const setupArgs = (dir, project = ADMISSION_PROJECT, repoIdentity = ADMISSION_REPO_IDENTITY) => ['supervisor', 'setup', '--owner', config.ownerPrincipal,
    '--project', project, '--repo-identity', repoIdentity, '--host', ADMISSION_HOST, '--git-common-dir', dir, '--json'];
  const setup = await ctx.cli(setupArgs(gitCommonDir), { label: 'supervisor-setup', env });
  ctx.assert('setup-created-identity-project-and-host', setup.code === 0 && setup.json?.ok === true && setup.json?.ownerPrincipal === config.ownerPrincipal
    && typeof setup.json?.supervisorId === 'string' && typeof setup.json?.projectId === 'string' && setup.json?.hostId === ADMISSION_HOST
    && setup.json?.gitCommonDir === gitCommonDir, { code: setup.code, json: setup.json, stderr: setup.stderr });
  // The rerun names the checkout root with a trailing separator, not the common dir.
  const rerun = await ctx.cli(setupArgs(`${realpathSync(repo)}/`), { label: 'supervisor-setup-rerun', env });
  ctx.assert('setup-rerun-returns-the-same-ids', rerun.code === 0 && rerun.json?.supervisorId === setup.json?.supervisorId
    && rerun.json?.projectId === setup.json?.projectId && rerun.json?.gitCommonDir === gitCommonDir, { code: rerun.code, json: rerun.json });
  // The project moved to another real repository, and another project on another spelling of
  // the mapped checkout: both are true SETUP_CONFLICTs, never a not-found refusal.
  const moved = await ctx.cli(setupArgs(elsewhereCommonDir), { label: 'supervisor-setup-conflict', env });
  const respelled = await ctx.cli(setupArgs(realpathSync(repo), 'qa-other', 'qa:qa-other'), { label: 'supervisor-setup-respelled-conflict', env });
  ctx.assert('setup-conflicting-rebind-refused', moved.code === 4 && moved.json?.code === 'SETUP_CONFLICT'
    && respelled.code === 4 && respelled.json?.code === 'SETUP_CONFLICT', { moved: { code: moved.code, json: moved.json }, respelled: { code: respelled.code, json: respelled.json } });
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
  // The start is confirmed by the worker's own agentState report (the pane's echo reads as
  // Unknown, not Working), but the dispatch row can commit before the worker has logged that
  // the report was delivered, so the worker log is awaited on its own.
  const reportedWorking = (entry) => entry.entries.some((line) => line.kind === 'reported' && line.state === 'working');
  const workerSettled = await waitForChange(ctx, 'worker working report', workerLogs, () => {
    const workers = readWorkerLogs(workerLogs);
    return { done: workers.length > 0 && workers.every((entry) => reportedWorking(entry) || entry.entries.some((line) => line.kind === 'report-failed' || line.kind === 'report-skipped')), workers: workers.length };
  }, 30_000).then(() => true, (error) => { ctx.log(`worker report wait: ${error?.message ?? error}`); return false; });
  const workersAfterStart = readWorkerLogs(workerLogs);
  const worker = workersAfterStart[0] ?? null;
  // The worker's first typed line is the dispatched instruction: it starts with the work's own
  // instruction (admission's worker_instruction), not just any non-empty line.
  const firstInput = worker?.entries.find((entry) => entry.kind === 'input' && entry.text.trim() !== '') ?? null;
  ctx.assert('exactly-one-worker-started', workerSettled && workersAfterStart.length === 1 && worker?.sessionId === dispatch?.provider_session_id
    && typeof firstInput?.text === 'string' && firstInput.text.includes(ADMISSION_WORK.instruction) && reportedWorking(worker),
  { firstInput, workers: workersAfterStart.map(({ name, sessionId, entries }) => ({ name, sessionId, entries })) });
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
    worker: {
      kind: 'fixture',
      note: 'The pane worker is fixture-worker.mjs launched through a runner-written SHELL wrapper; it reports agentState working on its first typed line, standing in for an agent extension. It is not a real provider worker and is no proof that a real worker was instructed.',
    },
    sourceId, goalId, receipt, setup: setup.json, binding: finalBinding, goal: finalGoal, dispatches: dispatched.dispatches,
    counts: { afterStart, afterReplay, afterEdited, afterQuestion, final: finalCounts }, toolsCalled, hubHttp: env.FERRYX_HUB_TRACKER_ADDR,
  });
}

// ---------------------------------------------------------------------------
// remote-delivery (plan task 8): a paired target pane accepts the dispatched bytes. With a
// start receipt the run starts; with the receipt suppressed the dispatch stays accepted and
// uncertain, never started or complete, and is never resent, also after the paired transport
// reconnects. Two isolated daemons on this machine are paired through a loopback ferryx-relay:
// the hub daemon (supervisor + dispatcher) and the target daemon (panes only, hub off). Every
// product effect goes through the product CLI (`supervisor setup|submit|dispatch status`,
// `agents read`) and the daemon protocol the GUI uses (pairing, workspace, spawn); the hub
// database is only read, read-only. A wrong (foreign) receipt is covered by the scoped Rust
// test crates/ferryx-hub/src/supervisor/tests/receipts.rs and listed as notCovered here.

const REMOTE_WORKSPACE_ID = 'qa-remote';
// The owner's body differs from the work's instruction, and every submit carries its own
// nonce in the instruction only: a delivery types it on exactly one line, so counting the
// nonce's lines counts deliveries (the body and scope lines never contain it).
const REMOTE_BODY = 'Please write result.txt with READY in it and check it with the verifier.';
const remoteWork = (nonce) => ({
  title: 'Write result.txt',
  instruction: `${nonce} Create result.txt containing READY. Run the verifier.`,
  requirements: [{ id: 'r1', text: 'result.txt contains exactly READY', verifier: 'verify.sh exits 0' }],
  operations: ['edit', 'test'],
});
// The product forces the gateway onto this loopback port (src-tauri/src/remote/state.rs); only
// the target daemon binds it (relay mode), the hub daemon's gateway stays off.
const GATEWAY_PORT = 43821;
const TURN_START_MS = 6_000;

// Free only when nothing answers a connect to 127.0.0.1:<port> (also catches a 0.0.0.0
// listener a SO_REUSEADDR bind probe misses on macOS) and a loopback bind succeeds.
function portIsFree(port) {
  const connects = new Promise((resolveConnect) => {
    const socket = createConnection({ host: '127.0.0.1', port });
    const done = (value) => { socket.destroy(); resolveConnect(value); };
    socket.setTimeout(2_000, () => done(false));
    socket.once('connect', () => done(true));
    socket.once('error', () => done(false));
  });
  const binds = () => new Promise((resolvePort) => {
    const server = createServer();
    server.unref();
    server.once('error', () => resolvePort(false));
    server.listen(port, '127.0.0.1', () => server.close(() => resolvePort(true)));
  });
  return connects.then((taken) => (taken ? false : binds()));
}

// True while `pid` exists (signal 0 probes without sending anything).
function pidAlive(pid) {
  try { process.kill(pid, 0); return true; } catch (error) { return error?.code === 'EPERM'; }
}

// One request on a fresh connection after a handshake; a protocolMismatch retries once with
// the daemon's own version. Logged to requests.jsonl / responses.jsonl.
async function daemonCall(ctx, socketPath, label, request, deadlineMs = 30_000) {
  let version = DAEMON_PROTOCOL_VERSION;
  for (let attempt = 0; attempt < 2; attempt += 1) {
    const frames = [{ type: 'handshake', version }, request];
    ctx.evidence.appendJsonl('requests.jsonl', { label, surface: 'daemon-socket', socket: socketPath, frames, at: new Date().toISOString() });
    const replies = await daemonExchange(socketPath, frames, deadlineMs);
    ctx.evidence.appendJsonl('responses.jsonl', { label, replies, at: new Date().toISOString() });
    const mismatch = replies.find((reply) => reply?.type === 'protocolMismatch');
    if (mismatch && attempt === 0 && Number.isInteger(mismatch.expectedVersion)) { version = mismatch.expectedVersion; continue; }
    return replies.at(-1) ?? null;
  }
  return null;
}

function countLines(path, needle) {
  if (!existsSync(path)) return 0;
  return readFileSync(path, 'utf8').split('\n').filter((line) => line.includes(needle)).length;
}

async function runRemoteDelivery(ctx) {
  if (!POSIX) {
    throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', 'remote-delivery runs POSIX fixture worker panes (the Windows pane shell is pwsh/powershell and ignores SHELL); this scenario runs on Linux or macOS');
  }
  const { config } = ctx;
  if (typeof config.ferryxRelay !== 'string' || config.ferryxRelay === '') {
    throw new HarnessError('CONFIG_INVALID', 'remote-delivery needs ferryxRelay: the absolute path of the ferryx-relay binary built from the same revision');
  }
  const git = onPath('git');
  if (!git) throw new HarnessError('PREREQUISITE_MISSING', 'git is not on PATH; the remote fixture project is a git repository');
  if (!(await portIsFree(GATEWAY_PORT))) {
    throw new HarnessError('PREREQUISITE_MISSING', `127.0.0.1:${GATEWAY_PORT} is in use; the target daemon's gateway must bind it (the product fixes the port) and nothing else is stopped to free it`);
  }
  const data = realpathSync(ctx.paths.data);
  const transcript = (line) => ctx.evidence.appendText('task-8-delivery.log', `${new Date().toISOString()} ${line}\n`);
  const hubDb = join(data, 'hub', 'hub.sqlite');
  // The target daemon's own isolated root inside the run root ("t" keeps its socket short).
  const target = join(data, 't');
  const targetSocket = join(target, 'runtime', 'daemon.sock');
  const hubSocket = join(data, 'runtime', 'daemon.sock');
  if (Buffer.byteLength(targetSocket) > 100) throw new HarnessError('CONFIG_INVALID', `target daemon socket ${targetSocket} exceeds 100 bytes; choose a shorter dataRoot`);
  for (const path of [hubDb, targetSocket]) {
    if (!isInside(path, data)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `${path} is outside the run root`);
  }

  // Fixture project and worker panes on the target. The pane shell is the fixture worker; a
  // mode file read at pane start makes the next pane report its turn start or stay silent.
  const workspaceRoot = join(data, 'rws');
  const repo = join(workspaceRoot, 'qa-supervisor');
  mkdirSync(repo, { recursive: true });
  const gitArgs = ['-c', 'init.defaultBranch=main', '-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid'];
  for (const [label, args] of [['git-init', [...gitArgs, 'init', '-q', repo]], ['git-commit', [...gitArgs, '-C', repo, 'commit', '-q', '--allow-empty', '-m', 'qa fixture']]]) {
    const result = await ctx.run(label, git, args, { deadlineMs: 30_000 });
    if (result.code !== 0) throw new HarnessError('FIXTURE_FAILED', `${label} exited ${result.code}: ${result.stderr}`);
  }
  const gitCommonDir = realpathSync(join(repo, '.git'));
  const workerLogs = join(data, 'workers');
  mkdirSync(workerLogs, { recursive: true });
  const modeFile = join(data, 'bin', 'pane-mode');
  const shell = join(data, 'bin', 'qa-remote-shell');
  mkdirSync(dirname(shell), { recursive: true });
  writeFileSync(shell, `#!/bin/sh\ncase "$1" in\n  -*c*) shift; exec /bin/sh -c "$1" ;;\nesac\nif [ "$(cat ${shellQuote(modeFile)} 2>/dev/null)" = silent ]; then\n  exec ${shellQuote(process.execPath)} ${shellQuote(WORKER)} ${shellQuote(workerLogs)} --no-report\nfi\nexec ${shellQuote(process.execPath)} ${shellQuote(WORKER)} ${shellQuote(workerLogs)}\n`, { mode: 0o755 });
  chmodSync(shell, 0o755);

  const relayPort = await freeLoopbackPort();
  const relayUrl = `http://127.0.0.1:${relayPort}`;
  const machineToken = `qa-${ctx.runId}-${relayPort}`;
  const trackerPort = await freeLoopbackPort();
  const isolated = (root) => ({
    HOME: join(root, 'home'), TMPDIR: join(root, 'tmp'), FERRYX_DATA_DIR: join(root, 'data'), FERRYX_RUNTIME_DIR: join(root, 'runtime'),
    FERRYX_HUB_DB: join(root, 'hub', 'hub.sqlite'),
  });
  for (const dir of [join(target, 'home'), join(target, 'tmp'), join(target, 'runtime')]) mkdirSync(dir, { recursive: true, mode: 0o700 });
  const relayEnv = ctx.productEnv({ FERRYX_DEPLOYMENT_MODE: 'selfhost', FERRYX_RELAY_MACHINE_TOKENS: machineToken, FERRYX_ACCOUNT_DATA_DIR: join(data, 'relay-account'), RUST_LOG: 'info' });
  const targetEnv = ctx.productEnv({
    ...isolated(target), FERRYX_HUB: 'off', SHELL: shell, FERRYX_ALLOW_INSECURE_RELAY: '1', FERRYX_MACHINE_TOKEN: machineToken,
    FERRYX_RELAY_URL: relayUrl, FERRYX_SUPERVISOR_TURN_START_MS: String(TURN_START_MS),
  });
  const hubEnv = ctx.productEnv({
    FERRYX_ALLOW_INSECURE_RELAY: '1', FERRYX_HUB_TRACKER_ADDR: `127.0.0.1:${trackerPort}`, FERRYX_HUB_WORKSPACE: REMOTE_WORKSPACE_ID,
    FERRYX_SUPERVISOR_TURN_START_MS: String(TURN_START_MS),
  });
  // The hub daemon's gateway stays at its default (off), so only the target binds the fixed
  // gateway port. The relay binds 0.0.0.0:<relayPort> (bin/relay.rs has no bind flag), so it
  // is reachable from this host's network for the run; recorded below.
  ctx.announce({ remoteDelivery: 'fixture', relayUrl, relayBind: `0.0.0.0:${relayPort}`, repo, gitCommonDir, hubDb, targetSocket });
  transcript(`fixture relay=${relayUrl} repo=${repo} hubDb=${hubDb} targetSocket=${targetSocket}`);

  // 1. Loopback relay, then the target daemon in relay mode with agent control on.
  const startRelay = async (label) => {
    const cursor = ctx.journal.cursor();
    const handle = await ctx.spawn(label, config.ferryxRelay, ['--port', String(relayPort)], { env: relayEnv, cwd: data, graceMs: 5_000 });
    await ctx.waitFor(`${label} listening`, (e) => e.source === label && (e.kind === 'stdout' || e.kind === 'stderr') && String(e.line ?? '').includes('ferryx-relay listening'), { after: cursor, deadlineMs: 30_000 });
    transcript(`${label} pid=${handle.pid} listening on ${relayPort}`);
    return handle;
  };
  let relay = await startRelay('relay');
  const targetLog = join(targetEnv.FERRYX_DATA_DIR, 'logs', 'daemon.log');
  const relayConnected = (min, label) => waitForChange(ctx, label, dirname(targetLog), () => ({ done: countLines(targetLog, 'relay control channel connected') >= min, connected: countLines(targetLog, 'relay control channel connected') }), 60_000);
  const targetReady = waitForPath(targetSocket, { deadlineMs: 60_000, signal: ctx.signal, label: 'target daemon socket' });
  const targetDaemon = await ctx.spawn('target-daemon', config.ferryxCli, ['--daemon'], { env: targetEnv, cwd: target, graceMs: 10_000 });
  await targetReady;
  transcript(`target daemon pid=${targetDaemon.pid} socket=${targetSocket}`);
  const control = await daemonCall(ctx, targetSocket, 'target-agent-control-on', { type: 'agentControlSettingsSet', enabled: true, excludedDeviceIds: [] });
  const configured = await daemonCall(ctx, targetSocket, 'target-relay-mode', { type: 'remoteConfigure', config: { mode: 'relay', port: GATEWAY_PORT, allowControl: true, relayUrl } });
  await relayConnected(1, 'target relay control channel');
  const code = await daemonCall(ctx, targetSocket, 'target-pairing-code', { type: 'remoteCreateMachinePairingCode' });
  ctx.assert('target-ready-for-pairing', control?.type === 'agentControlSettingsOk' && configured?.type === 'remoteConfigureOk'
    && code?.type === 'remotePairingCodeOk' && typeof code.code === 'string', { control, configured, code: code && { type: code.type, machineId: code.machineId, relayUrl: code.relayUrl } });
  if (code?.type !== 'remotePairingCodeOk') throw new HarnessError('PAIRING_FAILED', 'the target daemon issued no machine pairing code');

  // 2. The hub daemon pairs with the target through the relay.
  await ctx.startDaemon({ env: hubEnv, deadlineMs: 60_000 });
  const paired = await daemonCall(ctx, hubSocket, 'hub-pair-target', { type: 'pairedHostPair', request: { relayOrigin: relayUrl, pin: code.code, displayLabel: 'qa-target' } }, 60_000);
  const hostId = paired?.host?.hostId ?? null;
  ctx.assert('hub-paired-with-target', paired?.type === 'pairedHostPairOk' && typeof hostId === 'string' && paired.host.grantScope === 'machine',
    { paired: paired && { type: paired.type, host: paired.host ?? null, error: paired.error ?? paired.message ?? null } });
  if (!hostId) throw new HarnessError('PAIRING_FAILED', 'the hub daemon did not pair with the target');
  transcript(`paired hostId=${hostId}`);
  // The route the hub uses for that host: the relay must carry the traffic, so restarting
  // the relay below is a real transport interruption, not a bypassed one.
  const routeOf = async (label) => {
    const reply = await daemonCall(ctx, hubSocket, label, { type: 'pairedHostRouteList' });
    const entry = reply?.type === 'pairedHostRouteListOk' ? reply.entries.find((route) => route.hostId === hostId) ?? null : null;
    ctx.evidence.appendJsonl('routes.jsonl', { label, reply, entry, at: new Date().toISOString() });
    return entry?.state ?? null;
  };

  // 3. Owner set-up on the hub with the project mapped on the paired host (`--remote`: the
  //    target's realpath'd <repo>/.git, stored as given; the hub cannot resolve it).
  const setup = await ctx.cli(['supervisor', 'setup', '--owner', config.ownerPrincipal, '--project', 'qa-supervisor', '--repo-identity', 'qa:qa-supervisor',
    '--host', hostId, '--git-common-dir', gitCommonDir, '--remote', '--json'], { label: 'supervisor-setup-paired', env: hubEnv });
  ctx.assert('project-mapped-on-paired-host', setup.code === 0 && setup.json?.ok === true && setup.json?.hostId === hostId
    && setup.json?.gitCommonDir === gitCommonDir, { code: setup.code, json: setup.json, stderr: setup.stderr });
  if (setup.code !== 0) throw new HarnessError('SETUP_FAILED', `the project could not be mapped on paired host ${hostId}: ${setup.json?.code ?? setup.code}`);

  // 4. Worker panes on the target: reporting panes start their turn, the silent one never does.
  const registered = await daemonCall(ctx, targetSocket, 'target-register-workspace', { type: 'registerWorkspace', workspaceId: REMOTE_WORKSPACE_ID, repoRoot: realpathSync(workspaceRoot) });
  const spawnPane = async (mode, name = mode) => {
    writeFileSync(modeFile, `${mode}\n`);
    const reply = await daemonCall(ctx, targetSocket, `target-spawn-${name}`, { type: 'spawn', clientRequestId: `qa-${name}-${ctx.runId}`, workspaceId: REMOTE_WORKSPACE_ID, worktree: null, cwd: repo, cols: 100, rows: 30 }, 60_000);
    const sessionId = reply?.type === 'spawnOk' ? reply.sessionId : null;
    if (!sessionId) throw new HarnessError('FIXTURE_FAILED', `the ${name} pane was not spawned: ${JSON.stringify(reply)}`);
    await waitForChange(ctx, `${name} worker ready`, workerLogs, () => ({ done: readWorkerLogs(workerLogs).some((entry) => entry.sessionId === sessionId) }), 30_000);
    transcript(`pane ${name} (${mode}) sessionId=${sessionId}`);
    return sessionId;
  };
  ctx.assert('target-workspace-registered', registered?.type === 'registerWorkspaceOk', { registered });
  const reportingPane = await spawnPane('report');
  const silentPane = await spawnPane('silent');
  const bystanderPane = await spawnPane('report', 'bystander');
  const workerOf = (sessionId) => readWorkerLogs(workerLogs).find((entry) => entry.sessionId === sessionId) ?? null;
  // Lines a pane received carrying `nonce`: one per delivery of that submit.
  const inputsOf = (sessionId, nonce) => (workerOf(sessionId)?.entries ?? []).filter((entry) => entry.kind === 'input' && entry.text.includes(nonce)).length;
  const nonceOf = (label) => `[qa-${label}-${ctx.runId}]`;

  const hub = await openHubReader(hubDb);
  const submit = async (label, sessionId) => {
    const file = join(data, `${label}.request.json`);
    writeFileSync(file, `${JSON.stringify({ messageId: `qa-${label}-${ctx.runId}`, body: REMOTE_BODY, work: remoteWork(nonceOf(label)) })}\n`);
    return ctx.cli(['supervisor', 'submit', '--host', hostId, '--session', sessionId, '--request-file', file, '--json'], { label: `submit-${label}`, env: hubEnv });
  };
  const status = (label, goalId) => ctx.cli(['supervisor', 'dispatch', 'status', '--goal', goalId, '--json'], { label: `status-${label}`, env: hubEnv });
  const settledDispatch = (label, goalId, done) => waitForChange(ctx, label, dirname(hubDb), () => {
    const goal = hub.one('SELECT run_state, state_detail FROM supervisor_goals WHERE goal_id = ?', goalId);
    const dispatches = hub.all('SELECT request_id, state, run_id, uncertain_code, accepted_at, started_at FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id', goalId);
    return { done: done(goal, dispatches), goal, dispatches };
  }, 4 * TURN_START_MS + 60_000);

  const routeBefore = await routeOf('route-after-pairing');
  ctx.assert('paired-route-is-relay', routeBefore === 'relay', { route: routeBefore });

  // The daemon opens its hub after the socket is up and publishes no event for it, so ask the
  // read-only status request for a goal that cannot exist until the hub itself answers NOT_FOUND
  // (bounded backoff). Any other answer - unavailable, timeout, connect error - keeps polling,
  // and is the recorded failure if it is still the answer at the deadline.
  const hubReadyBy = Date.now() + 90_000;
  let hubProbe = null;
  for (let attempt = 0, pauseMs = 250; ; attempt += 1, pauseMs = Math.min(pauseMs * 2, 2_000)) {
    hubProbe = await ctx.cli(['supervisor', 'dispatch', 'status', '--goal', `qa-hub-ready-${attempt}`, '--json'], { label: `hub-ready-${attempt}`, env: hubEnv });
    if (hubProbe.json?.code === 'NOT_FOUND') break;
    if (Date.now() + pauseMs > hubReadyBy) break;
    await new Promise((resolvePause, rejectPause) => {
      if (ctx.signal.aborted) { rejectPause(ctx.signal.reason); return; }
      const onAbort = () => { clearTimeout(timer); rejectPause(ctx.signal.reason); };
      const timer = setTimeout(() => { ctx.signal.removeEventListener('abort', onAbort); resolvePause(); }, pauseMs);
      ctx.signal.addEventListener('abort', onAbort, { once: true });
    });
  }
  ctx.assert('hub-open-on-the-hub-daemon', hubProbe?.json?.code === 'NOT_FOUND', { code: hubProbe?.code, json: hubProbe?.json });

  // 5. Positive receipt: the reporting pane starts the run from its own report.
  const positive = await submit('positive', reportingPane);
  const positiveGoal = positive.json?.receipt?.goal_id ?? null;
  ctx.assert('positive-admitted-bound-to-paired-pane', positive.code === 0 && positive.json?.outcome === 'admitted' && typeof positiveGoal === 'string',
    { code: positive.code, json: positive.json, stderr: positive.stderr });
  if (!positiveGoal) throw new HarnessError('ADMISSION_FAILED', 'the positive submit returned no goal');
  await settledDispatch('positive dispatch', positiveGoal, (goal, rows) => rows.some((row) => row.state === 'started' || row.state === 'uncertain') || HELD_STATES.has(goal?.run_state));
  const positiveStatus = await status('positive', positiveGoal);
  const positiveRow = positiveStatus.json?.dispatches?.[0] ?? null;
  ctx.assert('positive-receipt-started-the-run', positiveStatus.code === 0 && positiveStatus.json?.binding?.hostId === hostId
    && positiveStatus.json?.binding?.providerSessionId === reportingPane && positiveStatus.json?.dispatches?.length === 1
    && positiveRow?.state === 'started' && typeof positiveRow?.runId === 'string' && typeof positiveRow?.startedAt === 'string'
    && positiveRow?.requestId === positive.json?.receipt?.dispatch_request_id && inputsOf(reportingPane, nonceOf('positive')) === 1,
  { status: positiveStatus.json, inputs: inputsOf(reportingPane, nonceOf('positive')) });
  transcript(`positive goal=${positiveGoal} dispatch=${JSON.stringify(positiveRow)}`);

  // 6. Missing receipt: the silent pane takes the bytes but never shows a turn start. While its
  //    start window is open, a person's input starts a turn in another (bystander) pane: that
  //    unrelated start must not count as the silent dispatch's receipt.
  const missingSubmit = submit('missing', silentPane);
  // Settles either way at once, so a timeout here cannot surface later as an unhandled rejection;
  // the outcome is asserted below.
  const bystanderTyped = waitForChange(ctx, 'silent pane received its dispatch', workerLogs, () => ({ done: inputsOf(silentPane, nonceOf('missing')) > 0 }), 60_000)
    .then(() => ctx.cli(['agents', 'send', bystanderPane, 'unrelated activity from a person', '--host', hostId, '--json'], { label: 'send-bystander', env: hubEnv }))
    .then((result) => result, (error) => {
      if (ctx.signal.aborted) throw error;
      return { code: null, json: null, error: String(error?.code ?? error?.message ?? error) };
    });
  bystanderTyped.catch(() => {});
  const missing = await missingSubmit;
  const missingGoal = missing.json?.receipt?.goal_id ?? null;
  ctx.assert('missing-admitted-bound-to-paired-pane', missing.code === 0 && missing.json?.outcome === 'admitted' && typeof missingGoal === 'string',
    { code: missing.code, json: missing.json, stderr: missing.stderr });
  if (!missingGoal) throw new HarnessError('ADMISSION_FAILED', 'the missing-receipt submit returned no goal');
  await settledDispatch('missing-receipt dispatch', missingGoal, (goal, rows) => rows.some((row) => row.state !== 'ready' && row.state !== 'claimed') && goal?.run_state !== 'dispatching');
  const missingStatus = await status('missing', missingGoal);
  const missingRow = missingStatus.json?.dispatches?.[0] ?? null;
  const acceptedNotStarted = (body, row) => body?.goal?.runState === 'delivery_uncertain' && body?.dispatches?.length === 1 && row?.state === 'uncertain'
    && typeof row?.acceptedAt === 'string' && row?.startedAt === null && row?.settledAt === null && row?.runId === null
    && ['TURN_NOT_OBSERVED', 'START_RECEIPT_MISSING'].includes(row?.uncertainCode) && body?.binding?.runId === null;
  ctx.assert('missing-receipt-accepted-never-started', missingStatus.code === 0 && acceptedNotStarted(missingStatus.json, missingRow)
    && missingStatus.json?.binding?.providerSessionId === silentPane && inputsOf(silentPane, nonceOf('missing')) === 1,
  { status: missingStatus.json, inputs: inputsOf(silentPane, nonceOf('missing')), worker: workerOf(silentPane)?.entries ?? null });
  const bystander = await bystanderTyped;
  const bystanderStarted = await waitForChange(ctx, 'bystander turn start report', workerLogs, () => ({
    done: (workerOf(bystanderPane)?.entries ?? []).some((entry) => entry.kind === 'reported' && entry.state === 'working'),
  }), 30_000).then(() => true, () => false);
  ctx.assert('unrelated-activity-is-not-a-receipt', bystander.code === 0 && bystanderStarted && acceptedNotStarted(missingStatus.json, missingRow),
    { send: { code: bystander.code, json: bystander.json, error: bystander.error ?? null }, bystanderStarted, missing: missingRow });
  const missingRead = await ctx.cli(['agents', 'read', silentPane, '--host', hostId, '--json'], { label: 'read-silent-pane', env: hubEnv });
  ctx.evidence.writeJson('silent-pane-read.json', missingRead.json);
  transcript(`missing goal=${missingGoal} dispatch=${JSON.stringify(missingRow)} paneRead=${missingRead.code}`);

  // 7. Double binding of the started pane is refused and writes nothing.
  const goalsBefore = hub.count('SELECT COUNT(*) AS n FROM supervisor_goals');
  const doubled = await submit('double', reportingPane);
  ctx.assert('double-binding-refused', doubled.code === 4 && doubled.json?.code === 'SESSION_ALREADY_BOUND'
    && hub.count('SELECT COUNT(*) AS n FROM supervisor_goals') === goalsBefore, { code: doubled.code, json: doubled.json, goalsBefore });

  // 8. Reconnect: the relay (recorded PID) is stopped and restarted on the same port; the
  //    target's control channel reconnects. A fresh goal on a fresh reporting pane must then
  //    start through the hub dispatcher and the paired transport: a full pass after the
  //    reconnect. Only after that are the earlier deliveries re-checked: nothing resent, the
  //    uncertain one still accepted and never started.
  const connectedBefore = countLines(targetLog, 'relay control channel connected');
  await ctx.terminate(relay, 'reconnect');
  transcript(`relay pid=${relay.pid} stopped for reconnect`);
  relay = await startRelay('relay-2');
  await relayConnected(connectedBefore + 1, 'target relay control channel reconnect');
  const reread = await ctx.cli(['agents', 'read', silentPane, '--host', hostId, '--json'], { label: 'read-silent-pane-after-reconnect', env: hubEnv, deadlineMs: 60_000 });
  const freshPane = await spawnPane('report', 'after-reconnect');
  const fresh = await submit('after-reconnect', freshPane);
  const freshGoal = fresh.json?.receipt?.goal_id ?? null;
  if (!freshGoal) throw new HarnessError('ADMISSION_FAILED', `the post-reconnect submit returned no goal: ${JSON.stringify(fresh.json)}`);
  const freshSettled = await settledDispatch('post-reconnect dispatch start', freshGoal, (goal, rows) => rows.some((row) => row.state === 'started') || HELD_STATES.has(goal?.run_state));
  const freshStatus = await status('after-reconnect', freshGoal);
  const routeAfter = await routeOf('route-after-reconnect');
  ctx.assert('post-reconnect-dispatch-started', fresh.code === 0 && freshStatus.json?.dispatches?.length === 1 && freshStatus.json.dispatches[0].state === 'started'
    && freshStatus.json?.binding?.providerSessionId === freshPane && inputsOf(freshPane, nonceOf('after-reconnect')) === 1 && routeAfter === 'relay',
  { admitted: fresh.json, status: freshStatus.json, settled: freshSettled, route: routeAfter });
  const afterStatus = await status('missing-after-reconnect', missingGoal);
  const afterPositive = await status('positive-after-reconnect', positiveGoal);
  ctx.assert('reconnected-without-resend', reread.code === 0 && afterStatus.code === 0 && acceptedNotStarted(afterStatus.json, afterStatus.json?.dispatches?.[0])
    && afterStatus.json?.dispatches?.[0]?.requestId === missingRow?.requestId && afterPositive.json?.dispatches?.length === 1
    && afterPositive.json?.dispatches?.[0]?.requestId === positiveRow?.requestId
    && inputsOf(silentPane, nonceOf('missing')) === 1 && inputsOf(reportingPane, nonceOf('positive')) === 1
    && hub.count("SELECT COUNT(*) AS n FROM supervisor_dispatches WHERE state = 'ready'") === 0,
  { reread: { code: reread.code, json: reread.json }, missing: afterStatus.json, positive: afterPositive.json,
    inputs: { silent: inputsOf(silentPane, nonceOf('missing')), reporting: inputsOf(reportingPane, nonceOf('positive')) } });
  transcript(`reconnect: relay-2 pid=${relay.pid} fresh=${freshStatus.json?.dispatches?.[0]?.state} silentInputs=${inputsOf(silentPane, nonceOf('missing'))} reportingInputs=${inputsOf(reportingPane, nonceOf('positive'))} missingState=${afterStatus.json?.dispatches?.[0]?.state}`);

  // 9. Fixture workers live in target panes, outside the process ledger: stop the target
  //    daemon (recorded PID); every worker must record a clean exit (input closed or terminal
  //    hang-up) and must already be gone when the harness looks for the daemon's descendants,
  //    so none of them needed reaping.
  const workerPids = readWorkerLogs(workerLogs).map((entry) => Number(entry.name.match(/^worker-(\d+)\.jsonl$/)?.[1])).filter(Number.isInteger);
  const cleanExit = new Set(['stdin-closed', 'hangup', 'terminated']);
  const targetReceipt = await ctx.terminate(targetDaemon, 'worker-cleanup');
  const closed = await waitForChange(ctx, 'fixture workers exit', workerLogs, () => {
    const open = readWorkerLogs(workerLogs).filter((entry) => !entry.entries.some((line) => cleanExit.has(line.kind)));
    return { done: open.length === 0, open: open.map((entry) => entry.name) };
  }, 30_000).then(() => true, (error) => { if (ctx.signal.aborted) throw error; return false; });
  // The harness reaps any descendant still alive after the daemon exits, so a later signal-0
  // probe cannot fail. The receipt's per-descendant initial verdict can: a worker that had to
  // be reaped shows up as 'match'; one that exited by itself is absent or 'gone'.
  const reaped = (targetReceipt?.descendants ?? []).filter((entry) => workerPids.includes(entry.pid) && entry.initial !== 'gone');
  const alive = workerPids.filter((pid) => pidAlive(pid));
  ctx.evidence.writeJson('worker-cleanup.json', {
    workerPids, allExitedCleanly: closed, reaped, alive, descendantsObservable: targetReceipt?.descendantsObservable ?? null,
    workerDescendants: (targetReceipt?.descendants ?? []).filter((entry) => workerPids.includes(entry.pid))
      .map(({ pid, initial, finalState }) => ({ pid, initial, finalState })),
    note: 'pids from worker-<pid>.jsonl; reaped = workers the harness still found alive after the target daemon exited',
  });
  // Four panes: reporting, silent, bystander, after-reconnect.
  ctx.assert('fixture-workers-gone', closed && workerPids.length === 4 && targetReceipt?.descendantsObservable === true
    && reaped.length === 0 && alive.length === 0, { workerPids, closed, reaped, alive });

  for (const entry of readWorkerLogs(workerLogs)) ctx.evidence.writeText(`workers/${entry.name}`, entry.text);
  if (existsSync(targetLog)) ctx.evidence.writeText('target-daemon.log', readFileSync(targetLog, 'utf8'));
  ctx.evidence.writeJson('remote-delivery.json', {
    worker: { kind: 'fixture', note: 'Target panes run fixture-worker.mjs; the reporting pane reports agentState working on its first typed line, the silent pane (--no-report) never does. Neither is a real provider worker.' },
    hostId, relayBind: `0.0.0.0:${relayPort}`, routes: { afterPairing: routeBefore, afterReconnect: routeAfter },
    panes: { reporting: reportingPane, silent: silentPane, bystander: bystanderPane, afterReconnect: freshPane },
    afterReconnect: { goalId: freshGoal, admitted: fresh.json, status: freshStatus.json },
    positive: { goalId: positiveGoal, admitted: positive.json, status: positiveStatus.json },
    missing: { goalId: missingGoal, admitted: missing.json, status: missingStatus.json, afterReconnect: afterStatus.json },
    doubleBinding: doubled.json,
    notCovered: ['wrong-receipt: crates/ferryx-hub/src/supervisor/tests/receipts.rs'],
  });
}

// Todo 12: recovery and capabilities.
//
// A hub restart is `ferryx-cli hub stop` + `hub start` against the isolated daemon: the hub
// generation (dispatcher, wake scheduler, observer loop, engine) ends and a new one reconciles,
// while the daemon and its worker PTYs keep running. Killing the daemon would kill the PTYs it
// owns, which is the very thing the scenario must show never happens, so no boundary needs a
// daemon kill or a product hook: each boundary is reached by observing the hub database (WAL
// changes) and the fixture worker's own log before the stop.

const RECOVERY_WORKSPACE_ID = 'qa-recovery';
const NO_START_MARK = 'QA_NO_START_REPORT';

/** Fixture repo + worker shell + owner setup + isolated daemon + workspace + MCP channel. */
async function supervisedHub(ctx, { workspaceId, extraEnv = {}, label }) {
  if (!POSIX) throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', `${label} needs the POSIX fixture worker shell`);
  const { config } = ctx;
  const git = onPath('git');
  if (!git) throw new HarnessError('PREREQUISITE_MISSING', 'git is not on PATH');
  const data = realpathSync(ctx.paths.data);
  const dbPath = join(data, 'hub', 'hub.sqlite');
  const workspaceRoot = join(data, 'ws');
  const repo = join(workspaceRoot, ADMISSION_PROJECT);
  mkdirSync(repo, { recursive: true });
  const gitArgs = ['-c', 'init.defaultBranch=main', '-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid'];
  for (const [step, args] of [['git-init', [...gitArgs, 'init', '-q', repo]], ['git-commit', [...gitArgs, '-C', repo, 'commit', '-q', '--allow-empty', '-m', 'qa fixture']]]) {
    const result = await ctx.run(`${label}-${step}`, git, args, { deadlineMs: 30_000 });
    if (result.code !== 0) throw new HarnessError('FIXTURE_FAILED', `${step} exited ${result.code}: ${result.stderr}`);
  }
  const workerLogs = join(data, 'workers');
  mkdirSync(workerLogs, { recursive: true });
  const shell = join(data, 'bin', 'qa-worker-shell');
  mkdirSync(dirname(shell), { recursive: true });
  writeFileSync(shell, `#!/bin/sh\ncase "$1" in\n  -*c*) shift; exec /bin/sh -c "$1" ;;\nesac\nexec ${shellQuote(process.execPath)} ${shellQuote(WORKER)} ${shellQuote(workerLogs)}\n`, { mode: 0o755 });
  const env = typeof extraEnv === 'function'
    ? await extraEnv({ SHELL: shell, FERRYX_HUB_WORKSPACE: workspaceId, OMON_LANE_AUTO_WORKTREE: '1' })
    : ctx.productEnv({ SHELL: shell, FERRYX_HUB_TRACKER_ADDR: `127.0.0.1:${await freeLoopbackPort()}`, FERRYX_HUB_WORKSPACE: workspaceId, OMON_LANE_AUTO_WORKTREE: '1', ...extraEnv });
  const setup = await ctx.cli(['supervisor', 'setup', '--owner', config.ownerPrincipal, '--project', ADMISSION_PROJECT, '--repo-identity', ADMISSION_REPO_IDENTITY,
    '--host', ADMISSION_HOST, '--git-common-dir', realpathSync(join(repo, '.git')), '--json'], { label: `${label}-setup`, env });
  ctx.assert('setup-created-identity-project-and-host', setup.code === 0 && setup.json?.ok === true, { code: setup.code, json: setup.json, stderr: setup.stderr });
  if (!setup.json?.projectId) throw new HarnessError('SETUP_FAILED', 'supervisor setup returned no project');
  const daemon = await ctx.startDaemon({ env, deadlineMs: 60_000 });
  const socket = join(data, 'runtime', 'daemon.sock');
  const registered = await registerWorkspace(ctx, socket, workspaceId, realpathSync(workspaceRoot));
  ctx.assert('workspace-registered', registered.at(-1)?.type === 'registerWorkspaceOk', { replies: registered });
  const mcp = await ctx.spawn(`${label}-mcp`, config.ferryxCli, ['mcp'], { env, cwd: data, stdin: 'pipe', graceMs: 3_000 });
  mcp.child.stdin.on('error', () => { /* the exit event decides */ });
  let rpcId = 0;
  const submit = async (step, sourceId, instruction) => {
    rpcId += 1;
    const id = rpcId;
    const work = { ...ADMISSION_WORK, instruction };
    const frame = { jsonrpc: '2.0', id, method: 'tools/call', params: { name: 'supervisor.request', arguments: { requestId: sourceId, action: 'submit', payload: { request: instruction, work } } } };
    const cursor = ctx.journal.cursor();
    ctx.evidence.appendJsonl('requests.jsonl', { label: step, surface: 'ferryx-cli mcp', frame, at: new Date().toISOString() });
    mcp.child.stdin.write(`${JSON.stringify(frame)}\n`);
    const reply = await ctx.waitFor(`mcp reply to ${step}`, (e) => e.source === `${label}-mcp` && e.kind === 'stdout' && e.json?.id === id, { after: cursor, deadlineMs: 30_000 });
    let body = null;
    try { body = JSON.parse(reply.json?.result?.content?.[0]?.text ?? 'null'); } catch { /* asserted by the caller */ }
    ctx.evidence.appendJsonl('responses.jsonl', { label: step, reply: reply.json, body, at: new Date().toISOString() });
    return body;
  };
  const hub = await openHubReader(dbPath);
  // Pane census through the product CLI. A failed list is never an empty census: `ok` must be
  // true and `sessions` an array, or the census step fails the scenario.
  const panes = async (step) => {
    const listed = await ctx.cli(['agents', 'list'], { label: `${label}-panes-${step}`, env });
    const ok = listed.code === 0 && listed.json?.ok === true && Array.isArray(listed.json?.sessions);
    const ids = ok ? listed.json.sessions.map((session) => session.sessionId).filter(Boolean).sort() : [];
    ctx.evidence.appendJsonl('pty-census.jsonl', { step, ok, code: listed.code, ids, at: new Date().toISOString() });
    ctx.assert(`pty-census-listed-${step}`, ok, { code: listed.code, json: listed.json, stderr: listed.stderr });
    return ids;
  };
  // Child census: the daemon and every fixture worker (one per pane, pid from its own log),
  // each with its start stamp, so a restarted or replaced process is told from a survivor.
  const children = async (step) => {
    const probe = async (pid) => {
      const seen = await probeProcess(pid);
      return { pid, state: seen.state, startStamp: seen.startStamp ?? null };
    };
    const workers = [];
    for (const worker of readWorkerLogs(workerLogs)) {
      const pid = worker.entries.find((entry) => entry.kind === 'ready')?.pid ?? null;
      if (pid) workers.push({ sessionId: worker.sessionId, ...(await probe(pid)) });
    }
    const record = { step, daemon: await probe(daemon.pid), workers, at: new Date().toISOString() };
    ctx.evidence.appendJsonl('child-census.jsonl', record);
    return record;
  };
  return { data, dbPath, env, daemon, hub, submit, panes, children, workerLogs };
}

/** Every process of `before` is alive in `after` with the same start stamp. */
const survived = (before, after) => {
  const same = (a, b) => b?.state === 'alive' && a.state === 'alive' && a.startStamp !== null && b.startStamp === a.startStamp;
  return same(before.daemon, after.daemon)
    && before.workers.every((worker) => same(worker, after.workers.find((later) => later.pid === worker.pid)));
};

/** Session ids of every started (or held-started) dispatch, which must each be a live pane. */
const startedSessions = (hub) => hub.all(
  "SELECT DISTINCT provider_session_id AS id FROM supervisor_dispatches WHERE state = 'started' AND provider_session_id IS NOT NULL",
).map((row) => row.id).sort();

const hubControl = async (ctx, env, op, step) => {
  const result = await ctx.cli(['hub', op], { label: `hub-${op}-${step}`, env });
  ctx.evidence.appendJsonl('boundaries.jsonl', { step, op, code: result.code, stdout: result.stdout, at: new Date().toISOString() });
  // `ferryx-cli hub stop|start` exits 0 only on a `"status":"ok"` reply (hub_runtime control_exit_code).
  return result.code === 0;
};

/**
 * `ferryx-cli hub status [--check]` until `accept(result)` holds, bounded by `deadlineMs`.
 * The daemon opens its hub only after it announced ready (src-tauri/src/cli.rs:3920-3925), and
 * binds control.sock after the hub's first generation (hub_runtime.rs serve_control), so right
 * after a daemon start (or a `hub start`, whose HTTP bind is async) the answer can still be
 * "no control socket" or "unhealthy". Each try is one bounded CLI round trip; there is no
 * event to subscribe to across the process boundary, so the pause only spaces the tries.
 */
const hubStatusUntil = async (ctx, env, step, { check = false, accept, deadlineMs = 60_000 }) => {
  const deadline = performance.now() + deadlineMs;
  const args = check ? ['hub', 'status', '--check'] : ['hub', 'status'];
  for (let attempt = 1; ; attempt += 1) {
    const result = await ctx.cli(args, { label: `hub-status-${step}-${attempt}`, env });
    if (accept(result)) {
      ctx.evidence.appendJsonl('boundaries.jsonl', { step, op: args.slice(1).join(' '), attempt, code: result.code, stdout: result.stdout, at: new Date().toISOString() });
      return { ok: true, attempt, result };
    }
    if (performance.now() >= deadline) return { ok: false, attempt, result };
    if (ctx.signal.aborted) throw ctx.signal.reason;
    await new Promise((resolveWait) => setTimeout(resolveWait, Math.min(250 * attempt, 2_000)));
  }
};
const hubRunning = (result) => result.code === 0 && result.json?.running === true;
const hubHealthy = (result) => result.code === 0 && result.json?.healthy === true;

// The prompt the dispatcher types (supervisor/admission.rs dispatch text) is several lines:
// the work instruction, then "Owner request (verbatim):" and the owner's body, then the scope.
// The fixture logs one `input` entry per line, so a marker in both instruction and body would
// count twice per line-count. One delivery is one header line followed by its body line.
const OWNER_REQUEST_HEADER = 'Owner request (verbatim):';
const deliveries = (workerLogs) => readWorkerLogs(workerLogs).flatMap((worker) => {
  const inputs = worker.entries.filter((entry) => entry.kind === 'input');
  return inputs.flatMap((entry, index) => (entry.text.trim() === OWNER_REQUEST_HEADER && inputs[index + 1]
    ? [{ worker: worker.name, body: inputs[index + 1].text }] : []));
});

const goalRows = (hub, goalId) => ({
  goal: hub.one('SELECT run_state, state_detail, revision FROM supervisor_goals WHERE goal_id = ?', goalId),
  dispatches: hub.all('SELECT request_id, state, uncertain_code, run_id, provider_session_id FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id', goalId),
});

/** Workers (one entry per delivery) whose delivered owner request carries `needle`. */
const inputsWith = (workerLogs, needle) => deliveries(workerLogs)
  .filter((delivery) => delivery.body.includes(needle)).map((delivery) => delivery.worker);

async function runRecovery(ctx) {
  const log = [];
  const note = (line) => { log.push(`${new Date().toISOString()} ${line}`); ctx.log(line); };
  // The no-start dispatch must still be waiting for its start receipt when the hub stops.
  const h = await supervisedHub(ctx, { workspaceId: RECOVERY_WORKSPACE_ID, label: 'recovery', extraEnv: { FERRYX_SUPERVISOR_TURN_START_MS: '240000' } });
  const { hub, env, workerLogs } = h;
  const hubDir = dirname(h.dbPath);
  const transcript = (boundary, phase) => ctx.evidence.appendJsonl(`recovery/${boundary}.jsonl`, { phase, ...goalRows(hub, transcript.goals[boundary] ?? ''), workers: readWorkerLogs(workerLogs).length, at: new Date().toISOString() });
  transcript.goals = {};
  const census = [];
  const take = async (step) => { const ids = await h.panes(step); census.push({ step, ids }); return ids; };
  // One hub-generation restart: stop and start must each succeed, and the daemon and every
  // fixture worker must be the same processes (pid and start stamp) afterwards.
  const restartHub = async (step) => {
    const before = await h.children(`${step}-before`);
    ctx.assert(`hub-stopped-${step}`, await hubControl(ctx, env, 'stop', step), {});
    ctx.assert(`hub-started-${step}`, await hubControl(ctx, env, 'start', step), {});
    const after = await h.children(`${step}-after`);
    ctx.assert(`children-survived-${step}`, survived(before, after), { before, after });
  };
  try {
    // Boundary 1: before send. The hub is stopped, so the admission commits a ready intent that
    // no dispatcher can claim; after the restart it runs exactly once.
    // Precondition: the hub this daemon opened is running (control.sock bound, a generation
    // live), so the stop below is a real stop, and afterwards status reports it not running.
    const upFirst = await hubStatusUntil(ctx, env, 'before-send-up', { accept: hubRunning });
    ctx.assert('hub-running-before-send', upFirst.ok, { attempt: upFirst.attempt, code: upFirst.result.code, json: upFirst.result.json, stderr: upFirst.result.stderr });
    const childrenFirst = await h.children('before-send-before');
    const stoppedFirst = await hubControl(ctx, env, 'stop', 'before-send');
    const downFirst = await ctx.cli(['hub', 'status'], { label: 'hub-status-before-send-down', env });
    ctx.assert('hub-stopped-before-send', stoppedFirst && downFirst.code === 0 && downFirst.json?.running === false
      && downFirst.json?.stopped_flag === true, { stopped: stoppedFirst, status: downFirst.json, stderr: downFirst.stderr });
    const marker1 = `QA_RECOVERY_BEFORE_SEND_${ctx.runId}`;
    const first = await h.submit('before-send', `qa-recovery-1-${ctx.runId}`, `Create result.txt containing READY. ${marker1}`);
    const goal1 = first?.receipt?.goal_id ?? null;
    transcript.goals['before-send'] = goal1;
    const rows1 = goalRows(hub, goal1 ?? '');
    ctx.assert('before-send-admitted-while-stopped', first?.outcome === 'admitted' && rows1.goal?.run_state === 'ready'
      && rows1.dispatches.length === 1 && rows1.dispatches[0].state === 'ready' && inputsWith(workerLogs, marker1).length === 0, rows1);
    transcript('before-send', 'stopped');
    const panesBefore = await take('before-send-restart');
    ctx.assert('hub-restarted-before-send', await hubControl(ctx, env, 'start', 'before-send'), {});
    const childrenFirstAfter = await h.children('before-send-after');
    ctx.assert('children-survived-before-send', survived(childrenFirst, childrenFirstAfter), { before: childrenFirst, after: childrenFirstAfter });
    const ran1 = await waitForChange(ctx, 'before-send dispatch after restart', hubDir, () => {
      const rows = goalRows(hub, goal1);
      return { done: rows.dispatches.some((row) => row.state === 'started') || HELD_STATES.has(rows.goal?.run_state), ...rows };
    }, 120_000);
    transcript('before-send', 'restarted');
    ctx.assert('before-send-runs-once-after-restart', ran1.goal?.run_state === 'running' && ran1.dispatches.length === 1
      && ran1.dispatches[0].state === 'started' && inputsWith(workerLogs, marker1).length === 1, { ...ran1, inputs: inputsWith(workerLogs, marker1) });
    note(`before-send goal=${goal1} panes-before=${panesBefore.length}`);

    // Boundary 2: after send, before the start receipt. The worker logs the typed instruction
    // and withholds its start report, so the dispatch stays claimed while its delivery waits.
    const marker2 = `QA_RECOVERY_AFTER_SEND_${ctx.runId} ${NO_START_MARK}`;
    const second = await h.submit('after-send', `qa-recovery-2-${ctx.runId}`, `Create result.txt containing READY. ${marker2}`);
    const goal2 = second?.receipt?.goal_id ?? null;
    transcript.goals['after-send'] = goal2;
    const typed = await waitForChange(ctx, 'after-send instruction typed while claimed', workerLogs, () => {
      const rows = goalRows(hub, goal2 ?? '');
      return { done: inputsWith(workerLogs, marker2).length >= 1 && ['claimed', 'accepted'].includes(rows.dispatches[0]?.state), ...rows };
    }, 120_000);
    transcript('after-send', 'typed');
    const typedState = typed.dispatches[0]?.state;
    ctx.assert('after-send-instruction-typed-while-claimed', typed.dispatches.length === 1 && ['claimed', 'accepted'].includes(typedState), typed);
    const panesMid = await take('after-send-restart');
    await restartHub('after-send');
    const held = await waitForChange(ctx, 'after-send reconciliation', hubDir, () => {
      const rows = goalRows(hub, goal2);
      return { done: rows.goal?.run_state === 'delivery_uncertain', ...rows };
    }, 60_000);
    transcript('after-send', 'restarted');
    ctx.assert('after-send-held-uncertain-not-resent', held.dispatches.length === 1 && held.dispatches[0].state === 'uncertain'
      && held.dispatches[0].uncertain_code === (typedState === 'claimed' ? 'CLAIM_UNCONFIRMED_AT_RESTART' : 'START_UNCONFIRMED_AT_RESTART')
      && inputsWith(workerLogs, marker2).length === 1, { typedState, ...held, inputs: inputsWith(workerLogs, marker2) });
    note(`after-send goal=${goal2} panes-before=${panesMid.length}`);

    // Boundary 3: after start. The run is reattached (still running, same dispatch), never resent.
    const marker3 = `QA_RECOVERY_AFTER_START_${ctx.runId}`;
    const third = await h.submit('after-start', `qa-recovery-3-${ctx.runId}`, `Create result.txt containing READY. ${marker3}`);
    const goal3 = third?.receipt?.goal_id ?? null;
    transcript.goals['after-start'] = goal3;
    const started = await waitForChange(ctx, 'after-start run started', hubDir, () => {
      const rows = goalRows(hub, goal3 ?? '');
      return { done: rows.dispatches.some((row) => row.state === 'started') || HELD_STATES.has(rows.goal?.run_state), ...rows };
    }, 120_000);
    transcript('after-start', 'started');
    ctx.assert('after-start-run-started', started.goal?.run_state === 'running' && started.dispatches[0]?.state === 'started', started);
    const panesLate = await take('after-start-restart');
    await restartHub('after-start');
    const status = await hubStatusUntil(ctx, env, 'after-start-check', { check: true, accept: hubHealthy, deadlineMs: 30_000 });
    ctx.assert('hub-healthy-after-start-restart', status.ok, { attempt: status.attempt, code: status.result.code, stdout: status.result.stdout, stderr: status.result.stderr });
    // A live pane's run is left to the observer by the restart census, which writes nothing.
    // The new dispatcher runs its census before its first drain, so a sentinel goal submitted
    // now reaching `started` proves this generation's census already ran; goal3 is read after.
    const markerS = `QA_RECOVERY_SENTINEL_${ctx.runId}`;
    const sentinel = await h.submit('sentinel', `qa-recovery-s-${ctx.runId}`, `Create result.txt containing READY. ${markerS}`);
    const goalS = sentinel?.receipt?.goal_id ?? null;
    const sentinelRan = await waitForChange(ctx, 'sentinel dispatch after the after-start restart', hubDir, () => {
      const rows = goalRows(hub, goalS ?? '');
      return { done: rows.dispatches.some((row) => row.state === 'started') || HELD_STATES.has(rows.goal?.run_state), ...rows };
    }, 120_000);
    ctx.assert('census-ran-sentinel-started', sentinel?.outcome === 'admitted' && sentinelRan.dispatches[0]?.state === 'started', sentinelRan);
    const after = goalRows(hub, goal3);
    transcript('after-start', 'restarted');
    ctx.assert('after-start-reattached-not-resent', after.goal?.run_state === 'running' && after.dispatches.length === 1
      && after.dispatches[0].request_id === started.dispatches[0].request_id && after.dispatches[0].state === 'started'
      && inputsWith(workerLogs, marker3).length === 1, { ...after, inputs: inputsWith(workerLogs, marker3) });
    note(`after-start goal=${goal3} panes-before=${panesLate.length}`);

    const final = await take('final');
    const settledLate = goalRows(hub, goal3);
    ctx.evidence.appendJsonl('recovery/after-start.jsonl', { phase: 'final', ...settledLate, at: new Date().toISOString() });
    ctx.assert('after-start-still-running-at-end', settledLate.goal?.run_state === 'running' && settledLate.dispatches.length === 1
      && settledLate.dispatches[0].state === 'started' && inputsWith(workerLogs, marker3).length === 1, settledLate);
    const goals = hub.count('SELECT COUNT(*) AS n FROM supervisor_goals');
    ctx.assert('no-goal-lost', goals === 4 && [goal1, goal2, goal3, goalS].every((goal) => goal && hub.one('SELECT goal_id FROM supervisor_goals WHERE goal_id = ?', goal)), { goals });
    // Falsifiable census: every pane seen before any restart is still listed, and every started
    // dispatch's pane (the runs the restarts must not lose) is a live pane at the end.
    const liveRuns = startedSessions(hub);
    ctx.assert('pty-census-unchanged-across-restarts', census.every(({ ids }) => ids.every((id) => final.includes(id)))
      && liveRuns.length >= 2 && liveRuns.every((id) => final.includes(id)), { census, final, liveRuns });
    // One body per delivery; every submit's body carries its own marker, so a repeat is a resend.
    const allInputs = deliveries(workerLogs).map((delivery) => delivery.body).filter((body) => body.includes('QA_RECOVERY_'));
    ctx.assert('no-instruction-typed-twice', new Set(allInputs).size === allInputs.length, { allInputs });
  } finally {
    for (const worker of readWorkerLogs(workerLogs)) ctx.evidence.writeText(`workers/${worker.name}`, worker.text);
    ctx.evidence.writeText('task-12-recovery.log', `${log.join('\n')}\n`);
  }
}

async function runCapabilities(ctx) {
  // T6's engine fixtures (provider stub, isolated mhc profile, capability filter, engine status
  // and assess through `ferryx-cli supervisor engine`). Loaded lazily: until T6 merges, only
  // this scenario fails, with a named prerequisite.
  let mhc;
  try { mhc = await import('./mhc.mjs'); } catch (error) {
    throw new HarnessError('PREREQUISITE_MISSING', `capabilities needs the T6 engine fixtures (scripts/qa/unified-supervisor/mhc.mjs, us/t6-qa): ${error?.message ?? error}`);
  }
  const { config } = ctx;
  const log = [];
  const note = (line) => { log.push(`${new Date().toISOString()} ${line}`); ctx.log(line); };
  const stub = await mhc.startProviderStub(ctx);
  try {
    // The hub reads its env file once per process (hub_env FILE_ENV), so the engine binary
    // cannot be swapped by a hub restart. The engine is instead a runner-owned wrapper that runs
    // the real mhc while `dropFile` is empty and T6's capability filter once it names a
    // capability; each engine launch reads it, and a hub restart relaunches the engine.
    const binDir = join(realpathSync(ctx.paths.data), 'bin');
    mkdirSync(binDir, { recursive: true });
    const dropFile = join(binDir, 'mhc-drop-capability');
    const frames = join(binDir, 'mhc-filter-frames.jsonl');
    const wrapper = join(binDir, 'mhc-switchable');
    writeFileSync(dropFile, '');
    writeFileSync(wrapper, [
      '#!/bin/sh',
      `drop=$(cat ${shellQuote(dropFile)} 2>/dev/null)`,
      `[ -z "$drop" ] && exec ${shellQuote(config.mhcBin)} "$@"`,
      `UNIFIED_SUPERVISOR_MHC_REAL=${shellQuote(config.mhcBin)}`,
      'UNIFIED_SUPERVISOR_MHC_DROP_CAPABILITY="$drop"',
      `UNIFIED_SUPERVISOR_MHC_FRAMES=${shellQuote(frames)}`,
      'export UNIFIED_SUPERVISOR_MHC_REAL UNIFIED_SUPERVISOR_MHC_DROP_CAPABILITY UNIFIED_SUPERVISOR_MHC_FRAMES',
      `exec ${shellQuote(process.execPath)} ${shellQuote(mhc.MHC_FILTER)} "$@"`,
      '',
    ].join('\n'), { mode: 0o755 });
    let engineEnv = null;
    const h = await supervisedHub(ctx, {
      workspaceId: 'qa-capabilities',
      label: 'capabilities',
      extraEnv: async (extra) => { engineEnv = await mhc.prepareEngineEnv(ctx, { baseUrl: stub.baseUrl, mhcBin: wrapper, label: 'switchable', extra }); return engineEnv; },
    });
    const ready = await mhc.awaitEngine(ctx, engineEnv, 'ready', (json) => json?.engine?.state === 'ready' && json?.hubOpen === true, 120_000);
    ctx.assert('engine-ready', ready?.engine?.state === 'ready', ready?.engine ?? null);
    // A worker pane the faults must leave alone.
    const marker = `QA_CAPABILITIES_${ctx.runId}`;
    const submitted = await h.submit('worker', `qa-capabilities-${ctx.runId}`, `Create result.txt containing READY. ${marker}`);
    const goalId = submitted?.receipt?.goal_id ?? '';
    const running = await waitForChange(ctx, 'worker pane started', dirname(h.dbPath), () => {
      const rows = goalRows(h.hub, goalId);
      return { done: rows.dispatches.some((row) => row.state === 'started') || HELD_STATES.has(rows.goal?.run_state), ...rows };
    }, 120_000);
    ctx.assert('worker-pane-started', running.goal?.run_state === 'running', running);
    const panesBefore = await h.panes('before-faults');

    const fault = async (step, status, message) => {
      stub.failNext(status, message);
      const reply = await mhc.engineAssess(ctx, engineEnv, step, 'Assess: is the qa goal complete? Answer JSON.');
      stub.succeed();
      note(`${step}: code=${reply.json?.code ?? reply.code}`);
      return reply;
    };
    const auth = await fault('auth', 401, 'invalid x-api-key qa-loopback-fixture');
    ctx.assert('auth-fault-is-typed', auth.code !== 0 && auth.json?.code === 'MHC_AUTH', auth.json);
    const quota = await fault('quota', 429, 'rate limit reached for qa');
    ctx.assert('quota-fault-is-typed', quota.code !== 0 && quota.json?.code === 'MHC_QUOTA', quota.json);
    const provider = await fault('provider', 400, 'qa stub refused the request');
    ctx.assert('provider-fault-is-typed', provider.code !== 0 && provider.json?.code === 'MHC_PROVIDER', provider.json);

    // Missing capability: the next engine launch hides one required capability; only the hub
    // generation restarts (hub stop/start), never the daemon, so every PTY stays alive.
    writeFileSync(dropFile, mhc.DROPPED_CAPABILITY);
    note(`engine wrapper now drops ${mhc.DROPPED_CAPABILITY}; restarting the hub generation only`);
    const providerBefore = stub.requests.length;
    // Child census around the hub-generation restart: the owned engine (profile pid record
    // `<pid> <hub pid>`) is replaced, while the daemon and every worker are the same processes.
    const pidFile = join(mhc.engineStateDir(ctx), 'mhc-supervisor.pid');
    const enginePid = () => {
      try {
        const [pid, owner] = readFileSync(pidFile, 'utf8').trim().split(/\s+/).map(Number);
        return owner === h.daemon.pid ? pid : null;
      } catch { return null; }
    };
    const oldEngine = enginePid();
    const oldProbe = oldEngine ? await probeProcess(oldEngine) : null;
    const childrenBefore = await h.children('capability-before');
    ctx.assert('hub-stopped-capability', await hubControl(ctx, engineEnv, 'stop', 'capability'), {});
    ctx.assert('hub-started-capability', await hubControl(ctx, engineEnv, 'start', 'capability'), {});
    const degraded = await mhc.awaitEngine(ctx, engineEnv, 'degraded', (json) => json?.engine?.state === 'degraded', 120_000);
    ctx.assert('missing-capability-is-degraded', degraded?.engine?.faultCode === 'MHC_CAPABILITY_MISSING'
      && JSON.stringify(degraded?.engine?.fault?.missing) === JSON.stringify([mhc.DROPPED_CAPABILITY]), degraded?.engine ?? null);
    // The refused engine is ended and its pid record forgotten (MhcSupervisor::launch), so a
    // degraded generation leaves no owned engine child. Its relaunch loop records a transient
    // pid per refused launch, so the record is re-read (bounded) until it is empty, or names a
    // pid other than the old engine that is gone on a second probe.
    const oldAfter = oldEngine ? await probeProcess(oldEngine) : null;
    // Ended only on positive evidence: gone or zombie, or alive with a defined, different start
    // stamp. An `unknown` probe is never counted as ended.
    const ended = (before, after) => Boolean(before) && before.state === 'alive' && typeof before.startStamp === 'string'
      && Boolean(after) && (after.state === 'gone' || after.state === 'zombie'
        || (after.state === 'alive' && typeof after.startStamp === 'string' && after.startStamp !== before.startStamp));
    const recordReads = [];
    const recordDeadline = performance.now() + 30_000;
    let noOwnedEngine = false;
    for (let attempt = 1; ; attempt += 1) {
      const recorded = enginePid();
      if (recorded === null) { recordReads.push({ attempt, recorded }); noOwnedEngine = true; break; }
      if (recorded !== oldEngine) {
        const first = await probeProcess(recorded);
        const second = first.state === 'gone' ? first : await probeProcess(recorded);
        recordReads.push({ attempt, recorded, first: first.state, second: second.state });
        if (second.state === 'gone') { noOwnedEngine = true; break; }
      } else {
        recordReads.push({ attempt, recorded, note: 'still the old engine' });
      }
      if (performance.now() >= recordDeadline) break;
      if (ctx.signal.aborted) throw ctx.signal.reason;
      await new Promise((resolveWait) => setTimeout(resolveWait, Math.min(250 * attempt, 2_000)));
    }
    const childrenAfter = await h.children('capability-after');
    const degradedCensus = { pidFile, oldEngine, oldProbe, oldAfter, recordReads, noOwnedEngine, childrenBefore, childrenAfter };
    ctx.evidence.writeJson('capabilities/engine-census-degraded.json', degradedCensus);
    ctx.assert('engine-child-ended-workers-kept', ended(oldProbe, oldAfter) && noOwnedEngine
      && survived(childrenBefore, childrenAfter), degradedCensus);
    const refused = await mhc.engineAssess(ctx, engineEnv, 'degraded-refused', 'Assess again.');
    const seen = mhc.readFrames(frames);
    ctx.assert('degraded-engine-got-zero-sends', refused.json?.code === 'MHC_CAPABILITY_MISSING' && stub.requests.length === providerBefore
      && seen.length >= 1 && seen.every((frame) => frame.type === 'get_protocol_info'), { frames: seen, providerRequests: stub.requests.length - providerBefore });
    ctx.assert('no-fallback-canary-requests', stub.requests.every((entry) => entry.model !== mhc.CANARY_MODEL), { requests: stub.requests.length });
    const panesAfter = await h.panes('after-faults');
    const rows = goalRows(h.hub, goalId);
    const started = startedSessions(h.hub);
    ctx.assert('worker-panes-preserved', panesBefore.length >= 1 && panesBefore.every((id) => panesAfter.includes(id))
      && started.length >= 1 && started.every((id) => panesAfter.includes(id))
      && inputsWith(h.workerLogs, marker).length === 1 && rows.dispatches.length === 1, { panesBefore, panesAfter, started, rows });

    // Recovery from degraded: the filter is cleared and only the hub generation restarts once
    // more. A new owned engine child exists; the daemon and every worker are unchanged.
    writeFileSync(dropFile, '');
    const childrenRecover = await h.children('recover-before');
    ctx.assert('hub-stopped-recover', await hubControl(ctx, engineEnv, 'stop', 'recover'), {});
    ctx.assert('hub-started-recover', await hubControl(ctx, engineEnv, 'start', 'recover'), {});
    const recovered = await mhc.awaitEngine(ctx, engineEnv, 'recovered', (json) => json?.engine?.state === 'ready', 120_000);
    const newEngine = enginePid();
    const newProbe = newEngine ? await probeProcess(newEngine) : null;
    const childrenRecovered = await h.children('recover-after');
    const recoverCensus = { oldEngine, newEngine, newProbe, reported: recovered?.engine?.pid ?? null, childrenRecover, childrenRecovered };
    ctx.evidence.writeJson('capabilities/engine-census-recovered.json', recoverCensus);
    ctx.assert('engine-child-replaced-workers-kept', newEngine !== null && newEngine !== oldEngine && newProbe?.state === 'alive'
      && recovered?.engine?.pid === newEngine && survived(childrenRecover, childrenRecovered), recoverCensus);
    const panesRecovered = await h.panes('recovered');
    ctx.assert('worker-panes-preserved-after-recovery', started.every((id) => panesRecovered.includes(id)), { started, panesRecovered });
  } finally {
    await stub.close();
    ctx.evidence.writeText('task-12-capabilities.log', `${log.join('\n')}\n`);
  }
}

export const SCENARIOS = Object.freeze({
  // Task 6, implemented in mhc.mjs.
  runtime: RUNTIME_SCENARIO,
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
  'remote-delivery': Object.freeze({
    implemented: true,
    task: '8',
    summary: 'Paired target pane: start receipt starts the run; suppressed receipt stays accepted/uncertain, never started, never resent across a relay reconnect',
    deadlineMs: 8 * 60_000,
    requiredAssertions: [
      'target-ready-for-pairing', 'hub-paired-with-target', 'project-mapped-on-paired-host', 'target-workspace-registered', 'hub-open-on-the-hub-daemon',
      'positive-admitted-bound-to-paired-pane', 'positive-receipt-started-the-run', 'missing-admitted-bound-to-paired-pane',
      'paired-route-is-relay', 'missing-receipt-accepted-never-started', 'unrelated-activity-is-not-a-receipt', 'double-binding-refused',
      'post-reconnect-dispatch-started', 'reconnected-without-resend', 'fixture-workers-gone',
    ],
    // Listed in assertions.json, never counted toward PASS.
    notCovered: [
      { clause: 'wrong-receipt', reason: 'a start receipt naming another request or session is produced only by a scripted target', coveredBy: 'crates/ferryx-hub/src/supervisor/tests/receipts.rs' },
    ],
    run: runRemoteDelivery,
  }),
  recovery: Object.freeze({
    implemented: true,
    task: '12',
    summary: 'Restart only the isolated hub generation at before-send, after-send-before-receipt and after-start boundaries: one execution when reconcilable, otherwise delivery_uncertain with no resend; every PTY survives',
    deadlineMs: 10 * 60_000,
    requiredAssertions: [
      'setup-created-identity-project-and-host', 'workspace-registered', 'hub-running-before-send', 'hub-stopped-before-send',
      'before-send-admitted-while-stopped',
      'hub-restarted-before-send', 'children-survived-before-send', 'before-send-runs-once-after-restart',
      'after-send-instruction-typed-while-claimed', 'hub-stopped-after-send', 'hub-started-after-send', 'children-survived-after-send',
      'after-send-held-uncertain-not-resent', 'after-start-run-started', 'hub-stopped-after-start', 'hub-started-after-start',
      'children-survived-after-start', 'hub-healthy-after-start-restart', 'census-ran-sentinel-started',
      'after-start-reattached-not-resent', 'after-start-still-running-at-end', 'no-goal-lost', 'pty-census-listed-final',
      'pty-census-unchanged-across-restarts', 'no-instruction-typed-twice',
    ],
    // No product surface advances an execution generation for a live goal (advance_generation is
    // called only from tests), so a predecessor's late completion cannot be provoked end to end.
    notCovered: ['predecessor-completion-after-new-generation: crates/ferryx-hub/src/supervisor/tests/recovery.rs a_predecessor_completion_after_a_new_generation_is_ignored'],
    run: runRecovery,
  }),
  authority: planned('10,13', 'Non-owner submit/approve, mirror device mutation, worker "approve everything": 403/typed denial, zero dispatch, zero memory change'),
  memory: planned('15', 'Teach, new context and restart, task-only exception, forget: persisted/applied receipts, exception not learned, no resurrection'),
  capabilities: Object.freeze({
    implemented: true,
    task: '12',
    summary: 'Remove one required RPC capability; auth, quota and provider faults: named degraded cause, no fallback model, workers preserved',
    deadlineMs: 10 * 60_000,
    requiredAssertions: [
      'engine-ready', 'worker-pane-started', 'auth-fault-is-typed', 'quota-fault-is-typed', 'provider-fault-is-typed',
      'hub-stopped-capability', 'hub-started-capability', 'engine-child-ended-workers-kept',
      'missing-capability-is-degraded', 'degraded-engine-got-zero-sends', 'no-fallback-canary-requests',
      'pty-census-listed-before-faults', 'pty-census-listed-after-faults', 'worker-panes-preserved',
      'hub-stopped-recover', 'hub-started-recover', 'engine-child-replaced-workers-kept', 'pty-census-listed-recovered',
      'worker-panes-preserved-after-recovery',
    ],
    run: runCapabilities,
  }),
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

// ES imports are hoisted: RUNTIME_SCENARIO is bound before SCENARIOS is built.
import { RUNTIME_SCENARIO } from './mhc.mjs';
