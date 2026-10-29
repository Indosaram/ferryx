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
import { accessSync, chmodSync, constants as fsConstants, existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, renameSync, statSync, watch, writeFileSync } from 'node:fs';
import { createConnection, createServer } from 'node:net';
import { delimiter, dirname, isAbsolute, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { awaitGone, HarnessError, isInside, POSIX, probeProcess, waitForPath } from './harness.mjs';
import { awaitEngine, prepareEngineEnv, startProviderStub } from './mhc.mjs';
import { runSlackLeg } from './slack.mjs';
import { MEMORY_REQUIRED_ASSERTIONS, USAGE_REQUIRED_ASSERTIONS } from './memory-usage.mjs';
import { LEGACY_REQUIRED_ASSERTIONS } from './legacy.mjs';
import { CORRECTION_REQUIRED_ASSERTIONS } from './correction.mjs';

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
export const DAEMON_PROTOCOL_VERSION = 5;
export const HELD_STATES = new Set(['degraded', 'awaiting_owner', 'delivery_uncertain', 'cancelled', 'paused', 'completed']);

export const shellQuote = (value) => `'${String(value).replace(/'/g, `'\\''`)}'`;

export function onPath(name) {
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

export function freeLoopbackPort() {
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
export function daemonExchange(socketPath, frames, deadlineMs) {
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

export async function openHubReader(dbPath) {
  let Database;
  try { ({ Database } = await import('bun:sqlite')); } catch {
    throw new HarnessError('SQLITE_UNAVAILABLE', 'the admission scenario reads the isolated hub database with bun:sqlite; run the runner with bun');
  }
  const all = (sql, ...params) => {
    const db = new Database(dbPath, { readonly: true });
    try { return db.query(sql).all(...params); } finally { db.close(); }
  };
  // Several reads from one snapshot: one connection inside one read transaction, so rows the
  // hub commits together (e.g. a started dispatch and its goal's move to running, written in
  // one transaction by supervisor.rs record_start) are never seen half applied. `all` opens a
  // connection per query, so two `all` calls can straddle a commit.
  const read = (fn) => {
    const db = new Database(dbPath, { readonly: true });
    try {
      db.exec('BEGIN');
      try {
        return fn((sql, ...params) => db.query(sql).all(...params));
      } finally {
        db.exec('COMMIT');
      }
    } finally { db.close(); }
  };
  return {
    all,
    read,
    one: (sql, ...params) => all(sql, ...params)[0] ?? null,
    count: (sql, ...params) => Number(Object.values(all(sql, ...params)[0] ?? { n: -1 })[0]),
  };
}

// After a change event whose check found not-done: one trailing re-check this long after the
// last event, re-armed by each new event. The watch can coalesce two writes a millisecond apart
// into one event that fires before the second is readable; the trailing check reads that state.
// It is not a poll: it runs once per quiet period after a change, never on its own.
const CHANGE_SETTLE_MS = 100;

// Subscribe to a directory (the hub directory: every SQLite commit writes the -wal file; the
// worker log directory: every worker line is an append), then reconcile: `check` runs once now
// and again after each change, until it returns { done: true } or the deadline passes. Each
// change also re-arms one re-check CHANGE_SETTLE_MS after the last event, so a write landing in
// or just after a check is never lost.
export function waitForChange(ctx, label, dir, check, deadlineMs) {
  return new Promise((resolveWait, rejectWait) => {
    let settled = false;
    let watcher = null;
    let last = null;
    let trailing = null;
    const finish = (fn, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      clearTimeout(trailing);
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
    // A change: check now, as each event always did, and re-arm the trailing re-check behind
    // the newest event.
    const onChange = () => {
      if (settled) return;
      clearTimeout(trailing);
      trailing = setTimeout(() => { trailing = null; evaluate(); }, CHANGE_SETTLE_MS);
      evaluate();
    };
    const timer = setTimeout(() => finish(rejectWait, new HarnessError('DEADLINE_EXCEEDED', `${label} was not observed within ${deadlineMs}ms`, { last })), deadlineMs);
    if (ctx.signal.aborted) { onAbort(); return; }
    ctx.signal.addEventListener('abort', onAbort, { once: true });
    try {
      watcher = watch(dir, onChange);
      watcher.on?.('error', (error) => finish(rejectWait, new HarnessError('WATCH_FAILED', `${label}: ${error?.message ?? error}`)));
    } catch (error) {
      finish(rejectWait, new HarnessError('WATCH_FAILED', `${label}: ${error?.message ?? error}`));
      return;
    }
    evaluate();
  });
}

export function readWorkerLogs(logDir) {
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
  const daemon = await ctx.startDaemon({ env, deadlineMs: 60_000 });
  const registered = await registerWorkspace(ctx, join(data, 'runtime', 'daemon.sock'), ADMISSION_WORKSPACE_ID, realpathSync(workspaceRoot));
  ctx.assert('workspace-registered', registered.at(-1)?.type === 'registerWorkspaceOk', { replies: registered });
  // FERRYX_DAEMON_READY precedes the hub: the first MCP submit waits until the hub answers.
  const hubWaitStarted = Date.now();
  const hubProbe = await hubOpen(ctx, env, 90_000);
  ctx.evidence.appendJsonl('requests.jsonl', { label: 'hub-open-wait', waitedMs: Date.now() - hubWaitStarted, code: hubProbe?.code, reply: hubProbe?.json ?? null, at: new Date().toISOString() });
  ctx.assert('hub-open-before-the-first-submit', hubProbe?.json?.code === 'NOT_FOUND', { code: hubProbe?.code, json: hubProbe?.json });
  if (hubProbe?.json?.code !== 'NOT_FOUND') throw new HarnessError('HUB_UNAVAILABLE', `the isolated daemon's hub did not open: ${JSON.stringify(hubProbe?.json ?? null)}`);

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

  // 10. Slack leg (plan task 11): the same admission reached from an owner's Slack DM through
  //     the production Slack ingress adapter of a fresh hub generation (slack.mjs). The MCP
  //     daemon is stopped first. The Slack owner gets its own installation (HOME, data dir,
  //     runtime dir, hub db) under <data>/slack-leg: an installation has one owner, and the
  //     main leg's `qa-owner` is no Slack principal.
  await ctx.terminate(mcp, 'slack leg');
  await ctx.terminate(daemon, 'slack leg');
  const slackRoot = join(data, 'slack-leg');
  const slackWorkspace = join(slackRoot, 'ws');
  const slackRepo = join(slackWorkspace, 'qa-slack');
  const slackDb = join(slackRoot, 'hub', 'hub.sqlite');
  mkdirSync(slackRepo, { recursive: true });
  mkdirSync(dirname(slackDb), { recursive: true });
  for (const [label, args] of [
    ['git-init-slack', [...gitArgs, 'init', '-q', slackRepo]],
    ['git-commit-slack', [...gitArgs, '-C', slackRepo, 'commit', '-q', '--allow-empty', '-m', 'qa fixture']],
  ]) {
    const result = await ctx.run(label, git, args, { deadlineMs: 30_000 });
    if (result.code !== 0) throw new HarnessError('FIXTURE_FAILED', `${label} exited ${result.code}: ${result.stderr}`);
  }
  await runSlackLeg(ctx, {
    root: slackRoot,
    setupCli: (owner, legEnv) => ctx.cli(['supervisor', 'setup', '--owner', owner, '--project', 'qa-slack', '--repo-identity', 'qa:qa-slack',
      '--host', ADMISSION_HOST, '--git-common-dir', realpathSync(join(slackRepo, '.git')), '--json'], { label: 'supervisor-setup-slack', env: legEnv }),
    hub: await openHubReader(slackDb), dbPath: slackDb, waitForChange,
    registerWorkspace: (legEnv) => registerWorkspace(ctx, join(legEnv.FERRYX_RUNTIME_DIR, 'daemon.sock'), ADMISSION_WORKSPACE_ID, realpathSync(slackWorkspace)),
    // The fixture pane shell, so a dispatched Slack goal never launches a real agent.
    extraEnv: { SHELL: shell, FERRYX_HUB_WORKSPACE: ADMISSION_WORKSPACE_ID, OMON_LANE_AUTO_WORKTREE: '1' },
  });
}

// ---------------------------------------------------------------------------
// autonomy (plan task 10): one submit through MCP `supervisor.request`; the fixture worker's
// first turn writes the wrong bytes, every later turn writes READY. The hub's own wakes grade
// each settled run on the retained mhc engine (a loopback provider stub) and run the
// requirement's verifier itself (`sig:req:r1`). The stub model claims completion every time
// and cites that check, so only the check Ferryx ran decides: run 1 fails it and becomes one
// corrective run without the owner, run 2 passes and completes the goal. The hub database is
// only read, read-only.

const AUTONOMY_WORKSPACE_ID = 'qa-autonomy';
const AUTONOMY_PROJECT = 'qa-autonomy';
const AUTONOMY_BODY = 'Create result.txt containing READY. Run the verifier. Correct failures until it passes.';
const AUTONOMY_WORK = Object.freeze({
  title: 'Write result.txt',
  instruction: 'Create result.txt containing READY. Run verify.sh. Correct failures until it passes.',
  requirements: [{ id: 'r1', text: 'result.txt contains exactly READY', verifier: 'verify.sh exits 0' }],
  operations: ['edit', 'test'],
});
export const AUTONOMY_VERIFY = 'test "$(cat result.txt)" = READY\n';
// The blind convergence prompt's opening (crates/ferryx-hub/src/goals/converge.rs build_prompt).
export const CONVERGENCE_MARKER = 'You independently check whether a coding lane';
export const AUTONOMY_CONVERGED = Object.freeze({ requirements: [{ req_id: 'r1', met: true, evidence_refs: ['sig:req:r1'] }] });

export function promptTextOf(json) {
  return (json?.messages ?? []).map((message) => (typeof message?.content === 'string' ? message.content
    : (Array.isArray(message?.content) ? message.content.map((part) => part?.text ?? '').join('') : ''))).join('\n');
}

async function runAutonomy(ctx) {
  if (!POSIX) {
    throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', 'autonomy runs a POSIX fixture worker pane (the Windows pane shell ignores SHELL); this scenario runs on Linux or macOS');
  }
  const { config } = ctx;
  const git = onPath('git');
  if (!git) throw new HarnessError('PREREQUISITE_MISSING', 'git is not on PATH; the autonomy fixture project is a git repository');
  const data = realpathSync(ctx.paths.data);
  const dbPath = join(data, 'hub', 'hub.sqlite');
  if (!isInside(dbPath, data)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `hub database ${dbPath} is outside the run root`);

  // Fixture: a committed verify.sh (so every lane worktree has it) in a workspace checkout.
  const workspaceRoot = join(data, 'ws');
  const repo = join(workspaceRoot, AUTONOMY_PROJECT);
  mkdirSync(repo, { recursive: true });
  writeFileSync(join(repo, 'verify.sh'), AUTONOMY_VERIFY, { mode: 0o755 });
  const gitArgs = ['-c', 'init.defaultBranch=main', '-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid'];
  for (const [label, args] of [
    ['git-init', [...gitArgs, 'init', '-q', repo]],
    ['git-add', [...gitArgs, '-C', repo, 'add', 'verify.sh']],
    ['git-commit', [...gitArgs, '-C', repo, 'commit', '-q', '-m', 'qa fixture verifier']],
  ]) {
    const result = await ctx.run(label, git, args, { deadlineMs: 30_000 });
    if (result.code !== 0) throw new HarnessError('FIXTURE_FAILED', `${label} exited ${result.code}: ${result.stderr}`);
  }
  const gitCommonDir = realpathSync(join(repo, '.git'));
  const workerLogs = join(data, 'workers');
  mkdirSync(workerLogs, { recursive: true });
  const shell = join(data, 'bin', 'qa-worker-shell');
  mkdirSync(dirname(shell), { recursive: true });
  writeFileSync(shell, `#!/bin/sh\ncase "$1" in\n  -*c*) shift; exec /bin/sh -c "$1" ;;\nesac\nexec ${shellQuote(process.execPath)} ${shellQuote(WORKER)} ${shellQuote(workerLogs)} --multi-turn\n`, { mode: 0o755 });

  // The decision engine: the hub's retained mhc on a loopback provider stub.
  const stub = await startProviderStub(ctx, { answer: (json) => (promptTextOf(json).includes(CONVERGENCE_MARKER) ? AUTONOMY_CONVERGED : null) });
  try {
    const env = await prepareEngineEnv(ctx, { baseUrl: stub.baseUrl, mhcBin: config.mhcBin, label: 'autonomy', extra: {
      SHELL: shell,
      FERRYX_HUB_WORKSPACE: AUTONOMY_WORKSPACE_ID,
      OMON_LANE_AUTO_WORKTREE: '1',
    } });
    ctx.announce({ autonomy: 'fixture', workspaceRoot, repo, gitCommonDir, hubHttp: env.FERRYX_HUB_TRACKER_ADDR, dbPath, provider: stub.baseUrl });

    const setup = await ctx.cli(['supervisor', 'setup', '--owner', config.ownerPrincipal, '--project', AUTONOMY_PROJECT, '--repo-identity', `qa:${AUTONOMY_PROJECT}`,
      '--host', ADMISSION_HOST, '--git-common-dir', gitCommonDir, '--json'], { label: 'supervisor-setup', env });
    if (setup.code !== 0 || !setup.json?.projectId) throw new HarnessError('SETUP_FAILED', `supervisor setup failed: ${JSON.stringify(setup.json)} ${setup.stderr}`);

    const daemon = await ctx.startDaemon({ env, deadlineMs: 60_000 });
    const engine = await awaitEngine(ctx, env, 'autonomy-ready', (json) => json?.engine?.state === 'ready' && json?.hubOpen === true, 120_000);
    ctx.assert('engine-ready', engine?.engine?.state === 'ready', { engine });
    const registered = await registerWorkspace(ctx, join(data, 'runtime', 'daemon.sock'), AUTONOMY_WORKSPACE_ID, realpathSync(workspaceRoot));
    ctx.assert('workspace-registered', registered.at(-1)?.type === 'registerWorkspaceOk', { replies: registered });

    // The owner's one request, through the owned MCP channel.
    const mcp = await ctx.spawn('mcp', config.ferryxCli, ['mcp'], { env, cwd: data, stdin: 'pipe', graceMs: 3_000 });
    mcp.child.stdin.on('error', () => { /* the exit event decides */ });
    const sourceId = `qa-autonomy-${ctx.runId}`;
    // Every frame actually written to the MCP channel, as written.
    const sentFrames = [];
    const sendFrame = (value) => { sentFrames.push(JSON.parse(JSON.stringify(value))); mcp.child.stdin.write(`${JSON.stringify(value)}\n`); };
    const frame = { jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name: 'supervisor.request', arguments: { requestId: sourceId, action: 'submit', payload: { request: AUTONOMY_BODY, work: AUTONOMY_WORK } } } };
    const cursor = ctx.journal.cursor();
    ctx.evidence.appendJsonl('requests.jsonl', { label: 'submit', surface: 'ferryx-cli mcp', frame, at: new Date().toISOString() });
    sendFrame(frame);
    const reply = await ctx.waitFor('mcp reply to submit', (e) => e.source === 'mcp' && e.kind === 'stdout' && e.json?.id === 1, { after: cursor, deadlineMs: 30_000 });
    let body = null;
    try { body = JSON.parse(reply.json?.result?.content?.[0]?.text ?? 'null'); } catch { /* asserted below */ }
    ctx.evidence.appendJsonl('responses.jsonl', { label: 'submit', reply: reply.json, body, at: new Date().toISOString() });
    const goalId = body?.receipt?.goal_id ?? null;
    ctx.assert('submit-admitted', reply.json?.error == null && body?.outcome === 'admitted' && typeof goalId === 'string', { reply: reply.json, body });
    if (!goalId) throw new HarnessError('ADMISSION_FAILED', 'the submit returned no goal; nothing can run');

    // Nothing more is asked of anyone: the goal must end by itself.
    const hub = await openHubReader(dbPath);
    const observedDetails = [];
    const ended = await waitForChange(ctx, 'goal to complete or hold', dirname(dbPath), () => {
      const goal = hub.one('SELECT run_state, state_detail, revision, lane_key FROM supervisor_goals WHERE goal_id = ?', goalId);
      if (goal?.state_detail && observedDetails.at(-1) !== goal.state_detail) observedDetails.push(goal.state_detail);
      return { done: HELD_STATES.has(goal?.run_state), goal };
    }, 6 * 60_000);
    const goal = ended.goal;
    ctx.assert('goal-completed-without-owner', goal?.run_state === 'completed', { goal, observedDetails });

    const dispatches = hub.all('SELECT id, request_id, goal_revision, state, run_id, instruction, created_at, settled_at FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id', goalId);
    const [first, second] = dispatches;
    const admittedRevision = first?.goal_revision ?? null;
    ctx.assert('two-runs-at-admitted-revision', dispatches.length === 2 && dispatches.every((row) => row.state === 'settled' && row.goal_revision === admittedRevision && typeof row.run_id === 'string')
      && first.request_id !== second.request_id && first.run_id !== second.run_id, { dispatches: dispatches.map(({ instruction, ...row }) => row) });
    ctx.assert('correction-followed-first-settle-and-names-r1', Boolean(first?.settled_at && second?.created_at) && second.created_at >= first.settled_at
      && second.instruction.includes('r1') && second.instruction.includes('Allowed operations: edit, test'), { first: first && { settledAt: first.settled_at }, second });
    // state_detail is overwritten by later moves; the durable trace of the failed check is the
    // verifying -> ready transition that queued the correction.
    const events = hub.all("SELECT seq, from_state, to_state, from_revision, to_revision FROM supervisor_state_events WHERE goal_id = ? AND source = 'goal' ORDER BY seq", goalId);
    ctx.assert('verifier-failure-took-the-corrective-path', events.some((event) => event.from_state === 'verifying' && event.to_state === 'ready')
      && !events.some((event) => ['awaiting_owner', 'paused', 'degraded'].includes(event.to_state)), { events, observedDetails });
    const terms = hub.one('SELECT turns_used FROM lane_goal WHERE lane_key = ?', goal?.lane_key ?? '');
    ctx.assert('one-corrective-turn-counted', terms?.turns_used === 1, { terms });
    let verified = null;
    try { verified = JSON.parse((goal?.state_detail ?? '').replace(/^VERIFIED: /, '')); } catch { /* asserted below */ }
    ctx.assert('verified-names-second-run-and-current-revision', goal?.state_detail?.startsWith('VERIFIED: ') && verified?.runId === second?.run_id
      && verified?.requestId === second?.request_id && verified?.goalRevision === goal?.revision && verified?.evidenceRefs?.includes('sig:req:r1'), { detail: goal?.state_detail, verified });
    const requirement = hub.one("SELECT status, evidence_json FROM lane_goal_requirements WHERE lane_key = ? AND req_id = 'r1'", goal?.lane_key ?? '');
    let refs = [];
    try { refs = JSON.parse(requirement?.evidence_json ?? '[]'); } catch { /* asserted below */ }
    ctx.assert('r1-met-with-ferryx-check-and-run-provenance', requirement?.status === 'met' && refs.includes('sig:req:r1')
      && refs.some((ref) => typeof ref === 'string' && ref.startsWith('run:') && ref.includes(second?.run_id ?? '\u0000')), { requirement, refs });
    const calls = hub.all('SELECT purpose, kind, outcome, used_fallback FROM judge_calls ORDER BY rowid');
    const graded = calls.filter((call) => ['gap_assess', 'convergence_check'].includes(call.purpose));
    // Every grading turn ran on the retained mhc engine (kind mhc_*, outcome ok); none on omo.
    ctx.assert('grading-ran-on-mhc', graded.length >= 4 && graded.every((call) => call.kind.startsWith('mhc_') && call.outcome === 'ok')
      && stub.requests.some((request) => request.path?.endsWith('/chat/completions')), { calls, providerRequests: stub.requests.length });
    // What actually crossed the owner's channel: the frames written to MCP stdin, the
    // JSON-RPC replies the MCP process emitted, and the requests the hub admitted. Exactly one
    // supervisor.request submit, answered once, admitted once from MCP: no other tool call,
    // no correct/approve/resume nudge.
    const mcpCalls = sentFrames.map((sent) => ({ id: sent.id, method: sent.method, tool: sent.params?.name ?? null, action: sent.params?.arguments?.action ?? null }));
    const replyIds = ctx.journal.events.filter((e) => e.source === 'mcp' && e.kind === 'stdout' && e.json && e.json.id !== undefined && e.json.id !== null).map((e) => e.json.id);
    const requests = hub.all('SELECT source_adapter, source_message_id, goal_id FROM supervisor_requests ORDER BY id');
    ctx.assert('only-submit-was-requested', mcpCalls.length === 1 && mcpCalls[0].method === 'tools/call' && mcpCalls[0].tool === 'supervisor.request' && mcpCalls[0].action === 'submit'
      && JSON.stringify(replyIds) === JSON.stringify([mcpCalls[0].id])
      && requests.length === 1 && requests[0].source_message_id === sourceId && requests[0].goal_id === goalId,
    { mcpCalls, replyIds, requests });

    const workers = readWorkerLogs(workerLogs);
    const instructions = workers.flatMap((entry) => entry.entries.filter((line) => line.kind === 'instruction'));
    const turns = workers.flatMap((entry) => entry.entries.filter((line) => line.kind === 'turn'));
    ctx.assert('worker-received-exactly-two-instructions', workers.length === 1 && instructions.length === 2
      && turns.map((line) => line.verifyExit).join(',') === '1,0' && instructions[0].text.includes(AUTONOMY_WORK.instruction), { instructions, turns });

    // The runner checks the final bytes itself, in the lane's checkout.
    const lane = hub.one('SELECT cwd FROM lanes WHERE key = ?', goal?.lane_key ?? '');
    const cwd = lane?.cwd ?? null;
    const check = cwd ? await ctx.run('verify-final', '/bin/sh', ['verify.sh'], { cwd, deadlineMs: 20_000 }) : null;
    const bytes = cwd && existsSync(join(cwd, 'result.txt')) ? readFileSync(join(cwd, 'result.txt'), 'utf8') : null;
    ctx.assert('final-bytes-verify-on-disk', check?.code === 0 && bytes === 'READY', { cwd, code: check?.code, bytes });

    // Cleanup: stop the daemon; the fixture worker must exit by itself.
    const workerPids = workers.map((entry) => Number(entry.name.match(/^worker-(\d+)\.jsonl$/)?.[1])).filter(Number.isInteger);
    const cleanExit = new Set(['stdin-closed', 'hangup', 'terminated']);
    await ctx.terminate(daemon, 'worker-cleanup');
    const closed = await waitForChange(ctx, 'fixture worker exit', workerLogs, () => {
      const open = readWorkerLogs(workerLogs).filter((entry) => !entry.entries.some((line) => cleanExit.has(line.kind)));
      return { done: open.length === 0, open: open.map((entry) => entry.name) };
    }, 30_000).then(() => true, (error) => { if (ctx.signal.aborted) throw error; return false; });
    const alive = await stillAlive(workerPids);
    ctx.assert('fixture-workers-gone', workerPids.length === 1 && closed && alive.length === 0, { workerPids, closed, alive });

    for (const entry of readWorkerLogs(workerLogs)) ctx.evidence.writeText(`workers/${entry.name}`, entry.text);
    ctx.evidence.writeJson('autonomy.json', {
      worker: { kind: 'fixture', note: 'fixture-worker.mjs --multi-turn: wrong bytes on turn 1, READY after, runs verify.sh, reports working then idle. Not a real provider worker.' },
      model: { kind: 'loopback stub', note: 'The stub always answers complete and cites sig:req:r1; the check Ferryx ran decides. It is no proof of a real model\'s judgment.' },
      sourceId, goalId, goal, dispatches, events, observedDetails, terms, requirement, judgeCalls: calls, final: { cwd, code: check?.code, bytes },
    });
  } finally {
    await stub.close();
  }
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
export const GATEWAY_PORT = 43821;
const TURN_START_MS = 6_000;

// Free only when nothing answers a connect to 127.0.0.1:<port> (also catches a 0.0.0.0
// listener a SO_REUSEADDR bind probe misses on macOS) and a loopback bind succeeds.
export function portIsFree(port) {
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
export function pidAlive(pid) {
  try { process.kill(pid, 0); return true; } catch (error) { return error?.code === 'EPERM'; }
}

// The worker pids still alive once each had `ms` to finish exiting. A worker logs its exit
// line just before process.exit, so a single signal-0 probe right after that line can still see
// it; this waits (bounded) for the process itself to be gone.
async function stillAlive(pids, ms = 5_000) {
  const gone = await Promise.all(pids.map((pid) => awaitGone({ pid, startStamp: null }, ms)));
  return pids.filter((_, index) => !gone[index]);
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
  const hubProbe = await hubOpen(ctx, hubEnv, 90_000);
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
  const alive = await stillAlive(workerPids);
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

// authority (plan task 13): one owner goal is provisioned through the product, then every
// non-owner path is tried against it and must change nothing; finally the owner's own device
// changes it once. Real surfaces only: `ferryx-cli supervisor setup|request`, the daemon socket
// (also raw, from inside a real pane) and the isolated daemon's remote gateway over HTTP.
// http.jsonl holds HTTP exchanges only; socket and pane replies go to socket.jsonl.

const AUTHORITY_WORKSPACE_ID = 'qa-authority';
const AUTHORITY_PROJECT = 'qa-authority';
const AUTHORITY_REPO_IDENTITY = 'qa:qa-authority';
// Operations beyond edit/test/commit hold the goal for the owner, so nothing is ever dispatched
// and every dispatch row stays as admitted.
const AUTHORITY_WORK = Object.freeze({
  title: 'Publish result.txt',
  instruction: 'Create result.txt containing READY and publish it.',
  requirements: [{ id: 'r1', text: 'result.txt contains exactly READY', verifier: 'verify.sh exits 0' }],
  operations: ['edit', 'test', 'publish'],
});

function listTree(root) {
  const out = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir).sort()) {
      const path = join(dir, name);
      const stat = statSync(path);
      out.push(`${relative(root, path)}:${stat.isDirectory() ? 'dir' : stat.size}:${stat.mtimeMs}`);
      if (stat.isDirectory()) walk(path);
    }
  };
  if (existsSync(root)) walk(root);
  return out;
}

// The daemon opens its hub after the socket is up and publishes no event for it, so ask the
// read-only dispatch status for a goal that cannot exist until the hub itself answers NOT_FOUND
// (bounded backoff). Any other answer - unavailable, timeout, connect error - keeps polling, and
// is the recorded failure if it is still the answer at the deadline. Returns the last probe.
async function hubOpen(ctx, env, deadlineMs) {
  const readyBy = Date.now() + deadlineMs;
  let probe = null;
  for (let attempt = 0, pauseMs = 250; ; attempt += 1, pauseMs = Math.min(pauseMs * 2, 2_000)) {
    probe = await ctx.cli(['supervisor', 'dispatch', 'status', '--goal', `qa-hub-ready-${attempt}`, '--json'], { label: `hub-ready-${attempt}`, env });
    if (probe.json?.code === 'NOT_FOUND') break;
    if (Date.now() + pauseMs > readyBy) break;
    await new Promise((resolvePause, rejectPause) => {
      if (ctx.signal.aborted) { rejectPause(ctx.signal.reason); return; }
      const onAbort = () => { clearTimeout(timer); rejectPause(ctx.signal.reason); };
      const timer = setTimeout(() => { ctx.signal.removeEventListener('abort', onAbort); resolvePause(); }, pauseMs);
      ctx.signal.addEventListener('abort', onAbort, { once: true });
    });
  }
  return probe;
}

async function runAuthority(ctx) {
  if (!POSIX) {
    throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', 'the pane peer check is POSIX (peer pid, session, terminal, parent chain); Windows uses the documented token fallback and is not asserted here');
  }
  const { config } = ctx;
  const git = onPath('git');
  if (!git) throw new HarnessError('PREREQUISITE_MISSING', 'git is not on PATH; the authority fixture project is a git repository');
  const data = realpathSync(ctx.paths.data);
  const dbPath = join(data, 'hub', 'hub.sqlite');
  if (!isInside(dbPath, data)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `hub database ${dbPath} is outside the run root`);
  const runtimeDir = join(data, 'runtime');
  const socket = join(runtimeDir, 'daemon.sock');
  const work = join(data, 'authority');
  const repo = join(data, 'ws', AUTHORITY_PROJECT);
  mkdirSync(work, { recursive: true });
  mkdirSync(repo, { recursive: true });
  const gitArgs = ['-c', 'init.defaultBranch=main', '-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid'];
  for (const [label, args] of [['git-init', [...gitArgs, 'init', '-q', repo]], ['git-commit', [...gitArgs, '-C', repo, 'commit', '-q', '--allow-empty', '-m', 'qa fixture']]]) {
    const result = await ctx.run(label, git, args, { deadlineMs: 30_000 });
    if (result.code !== 0) throw new HarnessError('FIXTURE_FAILED', `${label} exited ${result.code}: ${result.stderr}`);
  }
  const env = ctx.productEnv();
  const memoryBefore = listTree(ctx.paths.memory);
  const socketLog = (entry) => ctx.evidence.appendJsonl('socket.jsonl', { at: new Date().toISOString(), ...entry });
  const httpLog = (entry) => ctx.evidence.appendJsonl('http.jsonl', { at: new Date().toISOString(), ...entry });
  const exchange = async (label, request) => {
    const replies = await daemonExchange(socket, [{ type: 'handshake', version: DAEMON_PROTOCOL_VERSION }, request], 30_000);
    socketLog({ label, peer: 'runner', request, replies });
    return replies.at(-1) ?? null;
  };

  // 1. The owner: setup, isolated daemon, one held goal submitted through the owner CLI.
  const setup = await ctx.cli(['supervisor', 'setup', '--owner', config.ownerPrincipal, '--project', AUTHORITY_PROJECT,
    '--repo-identity', AUTHORITY_REPO_IDENTITY, '--host', 'local', '--git-common-dir', realpathSync(join(repo, '.git')), '--json'], { label: 'supervisor-setup', env });
  if (setup.code !== 0 || !setup.json?.projectId) throw new HarnessError('SETUP_FAILED', `supervisor setup failed: ${setup.stdout} ${setup.stderr}`);
  const projectId = setup.json.projectId;
  await ctx.startDaemon({ env, deadlineMs: 60_000 });
  // FERRYX_DAEMON_READY precedes the hub: the owner submit waits until the hub answers.
  const hubWaitStarted = Date.now();
  const hubProbe = await hubOpen(ctx, env, 90_000);
  socketLog({ label: 'hub-open-wait', waitedMs: Date.now() - hubWaitStarted, code: hubProbe?.code, reply: hubProbe?.json ?? null });
  ctx.assert('hub-open-before-the-owner-submit', hubProbe?.json?.code === 'NOT_FOUND', { code: hubProbe?.code, json: hubProbe?.json });
  if (hubProbe?.json?.code !== 'NOT_FOUND') throw new HarnessError('HUB_UNAVAILABLE', `the isolated daemon's hub did not open: ${JSON.stringify(hubProbe?.json ?? null)}`);
  const registered = await exchange('register-workspace', { type: 'registerWorkspace', workspaceId: AUTHORITY_WORKSPACE_ID, repoRoot: realpathSync(join(data, 'ws')) });
  ctx.assert('workspace-registered', registered?.type === 'registerWorkspaceOk', registered);
  const submitFile = join(work, 'owner-submit.json');
  writeFileSync(submitFile, JSON.stringify({ requestId: `qa-authority-${ctx.runId}`, projectId, action: 'submit',
    payload: { request: AUTHORITY_WORK.instruction, work: AUTHORITY_WORK } }));
  const submitted = await ctx.cli(['supervisor', 'request', '--json', submitFile], { label: 'owner-submit', env });
  const goalId = submitted.json?.receipt?.receipt?.goal_id ?? null;
  ctx.assert('owner-goal-held-for-the-owner', submitted.code === 0 && typeof goalId === 'string' && submitted.json?.receipt?.outcome === 'awaiting_owner',
    { code: submitted.code, json: submitted.json, stderr: submitted.stderr });
  if (!goalId) throw new HarnessError('ADMISSION_FAILED', 'the owner submit returned no goal; the refusals would prove nothing');
  const hub = await openHubReader(dbPath);
  const goalRow = () => hub.one('SELECT run_state, state_detail, revision FROM supervisor_goals WHERE goal_id = ?', goalId);
  const goalLane = hub.one('SELECT lane_key FROM supervisor_goals WHERE goal_id = ?', goalId)?.lane_key ?? null;
  const dispatchRows = () => hub.all('SELECT request_id, state FROM supervisor_dispatches ORDER BY id');
  const counts = () => ({
    goals: hub.count('SELECT COUNT(*) AS n FROM supervisor_goals'),
    requests: hub.count('SELECT COUNT(*) AS n FROM supervisor_requests'),
    waits: hub.count('SELECT COUNT(*) AS n FROM supervisor_waits'),
  });
  const before = { goal: goalRow(), dispatches: dispatchRows(), counts: counts() };
  ctx.announce({ authority: 'owner goal', goalId, goal: before.goal });

  // 2. The remote gateway of the isolated daemon (loopback listener) and three paired devices.
  const configured = await exchange('remote-configure', { type: 'remoteConfigure', config: { mode: 'localNetwork', port: 0, allowControl: true } });
  const status = await exchange('remote-status', { type: 'remoteGetStatus' });
  const bound = status?.status?.boundAddress ?? null;
  if (configured?.type !== 'remoteConfigureOk' || !bound) {
    throw new HarnessError('GATEWAY_UNAVAILABLE', `the isolated daemon's remote gateway did not start (its port is fixed; is another Ferryx daemon holding it?): ${JSON.stringify({ configured, status })}`);
  }
  const base = `http://${bound}`;
  const http = async (label, method, path, { token, body } = {}) => {
    const headers = { accept: 'application/json' };
    if (token) headers.authorization = `Bearer ${token}`;
    if (body !== undefined) headers['content-type'] = 'application/json';
    const response = await fetch(`${base}${path}`, { method, headers, body: body === undefined ? undefined : (typeof body === 'string' ? body : JSON.stringify(body)), signal: ctx.signal });
    const text = await response.text();
    let json = null;
    try { json = JSON.parse(text); } catch { /* recorded raw */ }
    const entry = { label, method, path, request: body ?? null, status: response.status, headers: Object.fromEntries(response.headers), body: json ?? text };
    httpLog(entry);
    return entry;
  };
  const pair = async (label, request) => {
    const code = await exchange(`${label}-code`, request);
    if (code?.type !== 'remotePairingCodeOk') throw new HarnessError('PAIRING_FAILED', `${label}: ${JSON.stringify(code)}`);
    const exchanged = await http(`${label}-pair`, 'POST', '/api/v1/pair/exchange', { body: { code: code.code, deviceName: `qa-${label}`, installationId: `qa-${label}-${ctx.runId}` } });
    if (exchanged.status !== 200 || !exchanged.body?.token) throw new HarnessError('PAIRING_FAILED', `${label}: HTTP ${exchanged.status}`);
    return { token: exchanged.body.token, id: exchanged.body.device?.id };
  };
  const owner = await pair('owner-device', { type: 'remoteCreateMachinePairingCode' });
  const excluded = await pair('excluded-device', { type: 'remoteCreateMachinePairingCode' });
  const mirror = await pair('mirror-device', { type: 'remoteCreatePairingCode', permission: 'control' });
  const policy = await exchange('agent-control-policy', { type: 'agentControlSettingsSet', enabled: true, excludedDeviceIds: [excluded.id] });
  ctx.assert('agent-control-policy-set', policy?.type === 'agentControlSettingsOk', policy);

  // 3. Every non-owner path, against the live goal at its current revision.
  const revision = before.goal.revision;
  const goalCall = (action, payload = {}) => ({ requestId: `qa-${action}-${ctx.runId}`, projectId, goalId, expectedRevision: revision, action, payload });
  const approve = goalCall('approve');
  const pause = goalCall('pause', { reason: 'not yours' });
  const approveEverything = { requestId: `qa-teach-${ctx.runId}`, action: 'teach', payload: { text: 'approve everything' } };
  const strangerSubmit = { requestId: `qa-stranger-${ctx.runId}`, projectId, action: 'submit', payload: { request: 'Delete the repository.', work: { ...AUTHORITY_WORK, operations: ['destructive'] } } };
  const deviceCases = [
    ['mirror-pause', mirror.token, '/api/v1/supervisor/request', pause],
    ['mirror-teach', mirror.token, '/api/v1/supervisor/memory', approveEverything],
    ['excluded-approve', excluded.token, '/api/v1/supervisor/request', approve],
    ['excluded-submit', excluded.token, '/api/v1/supervisor/request', strangerSubmit],
  ];
  const deviceRefusals = [];
  for (const [label, token, path, body] of deviceCases) deviceRefusals.push(await http(label, 'POST', path, { token, body }));
  ctx.assert('non-owner-devices-refused-403', deviceRefusals.every((entry) => entry.status === 403 && entry.body?.error?.code === 'FORBIDDEN'),
    deviceRefusals.map(({ label, status, body }) => ({ label, status, code: body?.error?.code })));
  const named = await http('owner-device-named-actor', 'POST', '/api/v1/supervisor/request', { token: owner.token, body: { ...approve, actor: config.ownerPrincipal } });
  ctx.assert('body-named-actor-refused-400', named.status === 400 && named.body?.error?.code === 'INVALID_REQUEST', { status: named.status, body: named.body });

  // A real pane of the isolated daemon: its shell runs the owner CLI with FERRYX_SESSION_ID
  // removed and a raw socket client, and records the daemon it would reach.
  const client = join(work, 'socket-client.mjs');
  writeFileSync(client, [
    "import { createConnection } from 'node:net';",
    'const [sock, version, ...lines] = process.argv.slice(2);',
    "const c = createConnection(sock); let buf = ''; const out = [];",
    'const frames = [JSON.stringify({ type: "handshake", version: Number(version) }), ...lines];',
    "c.on('connect', () => c.write(frames.map((f) => f + '\\n').join('')));",
    "c.on('data', (d) => { buf += d; let i; while ((i = buf.indexOf('\\n')) >= 0) { out.push(JSON.parse(buf.slice(0, i))); buf = buf.slice(i + 1); if (out.length === frames.length) { process.stdout.write(JSON.stringify(out)); c.end(); } } });",
    "c.on('error', (e) => { process.stdout.write(JSON.stringify({ error: String(e) })); process.exit(2); });",
  ].join('\n'));
  const approveFile = join(work, 'approve.json');
  writeFileSync(approveFile, JSON.stringify(approve));
  // T8's `supervisor submit` request file: owner work, which a pane may never submit.
  const dispatchFile = join(work, 'dispatch-submit.json');
  writeFileSync(dispatchFile, JSON.stringify({ messageId: `qa-pane-dispatch-${ctx.runId}`, body: 'Delete the repository.',
    work: { ...AUTHORITY_WORK, operations: ['destructive'] } }));
  // `ferryx-cli mcp` from the pane: a `supervisor.request` submit, one JSON-RPC line on stdin.
  const mcpFile = join(work, 'mcp-submit.jsonl');
  // Line 2 (`lane.list`) reads the MCP process's own hub pool: it proves which database it opened.
  writeFileSync(mcpFile, [
    { jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name: 'supervisor.request', arguments: { ...strangerSubmit, requestId: `qa-pane-mcp-${ctx.runId}` } } },
    { jsonrpc: '2.0', id: 2, method: 'tools/call', params: { name: 'lane.list', arguments: {} } },
  ].map((line) => `${JSON.stringify(line)}\n`).join(''));
  const wire = (envelope) => JSON.stringify({ type: 'supervisorRequestV1', envelope, cli: true });
  // The pane runs inside the registered workspace root (`ws/`); its results go to `work` by absolute path.
  const spawned = await exchange('spawn-pane', { type: 'spawn', clientRequestId: `qa-authority-pane-${ctx.runId}`, workspaceId: AUTHORITY_WORKSPACE_ID, worktree: null, cwd: repo, cols: 80, rows: 24, shell: '/bin/sh' });
  const sessionId = spawned?.sessionId ?? null;
  if (!sessionId) throw new HarnessError('PANE_SPAWN_FAILED', `could not spawn the pane: ${JSON.stringify(spawned)}`);
  const cliOut = join(work, 'pane-cli.json');
  const sockOut = join(work, 'pane-socket.json');
  const submitOut = join(work, 'pane-cli-submit.json');
  const mcpOut = join(work, 'pane-mcp.json');
  const envOut = join(work, 'pane-env.txt');
  const done = [cliOut, sockOut, submitOut, mcpOut].map((path) => ctx.waitForPath(path, { deadlineMs: 60_000, label: `pane result ${basenameOf(path)}` }));
  done.forEach((promise) => promise.catch(() => {}));
  const script = [
    `printf '%s\\n%s' "$FERRYX_RUNTIME_DIR" "$FERRYX_HUB_DB" > ${shellQuote(envOut)}`,
    `env -u FERRYX_SESSION_ID ${shellQuote(config.ferryxCli)} supervisor request --json ${shellQuote(approveFile)} > ${shellQuote(`${cliOut}.tmp`)} 2>&1; mv ${shellQuote(`${cliOut}.tmp`)} ${shellQuote(cliOut)}`,
    `env -u FERRYX_SESSION_ID ${shellQuote(config.ferryxCli)} supervisor submit --request-file ${shellQuote(dispatchFile)} --json > ${shellQuote(`${submitOut}.tmp`)} 2>&1; mv ${shellQuote(`${submitOut}.tmp`)} ${shellQuote(submitOut)}`,
    `env -u FERRYX_SESSION_ID ${shellQuote(config.ferryxCli)} mcp < ${shellQuote(mcpFile)} > ${shellQuote(`${mcpOut}.tmp`)} 2>/dev/null; mv ${shellQuote(`${mcpOut}.tmp`)} ${shellQuote(mcpOut)}`,
    `${shellQuote(process.execPath)} ${shellQuote(client)} ${shellQuote(socket)} ${DAEMON_PROTOCOL_VERSION} ${shellQuote(wire(approve))} ${shellQuote(wire(approveEverything))} ${shellQuote(wire(strangerSubmit))} > ${shellQuote(`${sockOut}.tmp`)} 2>&1; mv ${shellQuote(`${sockOut}.tmp`)} ${shellQuote(sockOut)}`,
  ].join('; ') + '\n';
  const typed = await exchange('pane-write', { type: 'write', sessionId, data: Buffer.from(script).toString('base64') });
  ctx.assert('pane-script-written', typed?.type !== 'error', typed);
  await Promise.all(done);
  const [paneRuntime = null, paneHubDb = null] = existsSync(envOut) ? readFileSync(envOut, 'utf8').split('\n') : [];
  ctx.assert('pane-inherits-the-isolated-daemon-env', paneRuntime === runtimeDir || (paneRuntime && realpathSync(paneRuntime) === realpathSync(runtimeDir)), { paneRuntime, runtimeDir });
  let paneCli = null; let paneSock = null; let paneSubmit = null; let paneMcp = null;
  let paneMcpLanes = null;
  try {
    const lines = readFileSync(mcpOut, 'utf8').trim().split('\n').map((line) => JSON.parse(line));
    paneMcp = lines.find((line) => line?.id === 1) ?? null;
    paneMcpLanes = lines.find((line) => line?.id === 2) ?? null;
  } catch { /* asserted below */ }
  try { paneCli = JSON.parse(readFileSync(cliOut, 'utf8')); } catch { /* asserted below */ }
  try { paneSubmit = JSON.parse(readFileSync(submitOut, 'utf8')); } catch { /* asserted below */ }
  try { paneSock = JSON.parse(readFileSync(sockOut, 'utf8')); } catch { /* asserted below */ }
  socketLog({ label: 'pane-cli-approve', peer: 'pane', reply: paneCli });
  socketLog({ label: 'pane-raw-socket', peer: 'pane', replies: paneSock });
  ctx.assert('pane-cli-approve-forbidden', paneCli?.ok === false && paneCli?.code === 'FORBIDDEN', paneCli);
  socketLog({ label: 'pane-cli-dispatch-submit', peer: 'pane', reply: paneSubmit });
  // FERRYX_SESSION_ID is removed, so the CLI's own speed bump does not fire: the daemon's peer gate refuses it.
  ctx.assert('pane-cli-dispatch-submit-forbidden', paneSubmit?.ok === false && paneSubmit?.code === 'FORBIDDEN', paneSubmit);
  socketLog({ label: 'pane-mcp-submit', peer: 'pane', reply: paneMcp });
  // The MCP server forwards every write to the daemon, whose peer gate sees the pane-run MCP process.
  ctx.assert('pane-mcp-submit-forbidden', paneMcp?.id === 1 && typeof paneMcp?.error?.message === 'string'
    && paneMcp.error.message.startsWith('FORBIDDEN'), paneMcp);
  // The pane-run MCP opened the isolated hub (inherited FERRYX_HUB_DB), never the real data dir:
  // its own lane list holds the lane of the goal admitted in this run's database.
  let mcpLaneKeys = null;
  try { mcpLaneKeys = JSON.parse(paneMcpLanes?.result?.content?.[0]?.text ?? 'null')?.map((lane) => lane.key) ?? null; } catch { /* asserted below */ }
  socketLog({ label: 'pane-mcp-hub-db', peer: 'pane', paneHubDb, dbPath, goalLane, mcpLaneKeys });
  const sameFile = (left, right) => left === right || (!!left && existsSync(left) && realpathSync(left) === right);
  ctx.assert('pane-mcp-opens-the-isolated-hub-db', sameFile(paneHubDb, dbPath) && typeof goalLane === 'string'
    && Array.isArray(mcpLaneKeys) && mcpLaneKeys.includes(goalLane), { paneHubDb, dbPath, goalLane, mcpLaneKeys, reply: paneMcpLanes });
  const paneReplies = Array.isArray(paneSock) ? paneSock.slice(1) : [];
  ctx.assert('pane-socket-approve-teach-submit-forbidden', paneReplies.length === 3 && paneReplies.every((reply) => reply?.type === 'error' && reply?.code === 'FORBIDDEN'), paneSock);

  // 4. Nothing changed: the goal, every dispatch, the request/wait counts and the memory root.
  const afterRefusals = { goal: goalRow(), dispatches: dispatchRows(), counts: counts() };
  ctx.assert('goal-unchanged-after-refusals', JSON.stringify(afterRefusals.goal) === JSON.stringify(before.goal), { before: before.goal, after: afterRefusals.goal });
  ctx.assert('zero-dispatch-after-refusals', JSON.stringify(afterRefusals.dispatches) === JSON.stringify(before.dispatches)
    && afterRefusals.dispatches.every((row) => row.state === 'ready' || row.state === 'superseded'), { before: before.dispatches, after: afterRefusals.dispatches });
  ctx.assert('no-request-or-wait-written', JSON.stringify(afterRefusals.counts) === JSON.stringify(before.counts), { before: before.counts, after: afterRefusals.counts });
  ctx.assert('memory-root-unchanged', JSON.stringify(listTree(ctx.paths.memory)) === JSON.stringify(memoryBefore), { entries: memoryBefore.length });

  // 5. The owner's device: authorized 200 (the same pause changes the goal once), the same
  //    decision replayed at the old revision 409, memory receipts 503.
  const snapshot = await http('owner-snapshot', 'GET', '/api/v1/supervisor', { token: owner.token });
  const listed = snapshot.body?.supervisor?.goals?.find?.((goal) => goal.goalId === goalId) ?? null;
  ctx.assert('authorized-snapshot-200-lists-the-goal', snapshot.status === 200 && listed?.revision === revision && listed?.runState === before.goal.run_state, { status: snapshot.status, listed });
  const paused = await http('owner-pause', 'POST', '/api/v1/supervisor/request', { token: owner.token, body: pause });
  ctx.assert('authorized-pause-200', paused.status === 200 && paused.headers['cache-control'] === 'no-store' && paused.body?.runState === 'paused'
    && paused.body?.revision === revision + 1, { status: paused.status, body: paused.body });
  const stale = await http('owner-pause-stale', 'POST', '/api/v1/supervisor/request', { token: owner.token, body: { ...pause, requestId: `${pause.requestId}-again` } });
  ctx.assert('stale-revision-409', stale.status === 409 && stale.body?.error?.code === 'REVISION_CONFLICT', { status: stale.status, body: stale.body });
  const receipts = await http('owner-memory-receipts', 'GET', '/api/v1/supervisor/memory', { token: owner.token });
  // No mhc engine runs in this daemon: the native receipt listing is unavailable, never empty.
  ctx.assert('memory-receipts-unavailable-503', receipts.status === 503 && receipts.body?.error?.code === 'MEMORY_UNAVAILABLE', { status: receipts.status, body: receipts.body });
  const final = { goal: goalRow(), dispatches: dispatchRows() };
  ctx.assert('only-the-owner-changed-the-goal', final.goal?.run_state === 'paused' && final.goal?.revision === revision + 1
    && final.dispatches.every((row) => row.state === 'ready' || row.state === 'superseded'), final);
  ctx.evidence.writeJson('authority.json', { goalId, projectId, before, afterRefusals, final, devices: { owner: owner.id, excluded: excluded.id, mirror: mirror.id }, paneSessionId: sessionId, paneRuntime });
}

function basenameOf(path) { return path.slice(path.lastIndexOf('/') + 1); }

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
  // OMON_INBOX_ENABLED=0 is the hub's own switch for its inbox watchers
  // (ferryx-hub inbox_watch.rs inbox_enabled, read in hub_runtime spawn_generation). The
  // isolated run has no iMessage or mail; a watcher that ran would fail and `hub status
  // --check` would count it (control.rs status_report), so health would mean nothing.
  const isolation = { SHELL: shell, FERRYX_HUB_WORKSPACE: workspaceId, OMON_LANE_AUTO_WORKTREE: '1', OMON_INBOX_ENABLED: '0' };
  const env = typeof extraEnv === 'function'
    ? await extraEnv(isolation)
    : ctx.productEnv({ ...isolation, FERRYX_HUB_TRACKER_ADDR: `127.0.0.1:${await freeLoopbackPort()}`, ...extraEnv });
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

// The goal and its dispatches from one snapshot (see openHubReader `read`).
const goalRows = (hub, goalId) => hub.read((query) => ({
  goal: query('SELECT run_state, state_detail, revision FROM supervisor_goals WHERE goal_id = ?', goalId)[0] ?? null,
  dispatches: query('SELECT request_id, state, uncertain_code, run_id, provider_session_id FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id', goalId),
}));

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
  // Every wait of this scenario: on a timeout (or any failure) the state of every goal seen so
  // far, its dispatches and the worker/pane counts are written to the evidence before the
  // failure propagates, so a hang carries a snapshot even after the data root is cleaned up.
  const snapshotOnFailure = async (label, wait) => {
    try {
      return await wait;
    } catch (error) {
      let snapshot;
      try {
        snapshot = {
          label,
          error: String(error?.message ?? error),
          goals: Object.fromEntries(Object.entries(transcript.goals).map(([boundary, goalId]) => [boundary, { goalId, ...goalRows(hub, goalId ?? '') }])),
          allDispatches: hub.all('SELECT goal_id, request_id, state, uncertain_code, run_id, provider_session_id, generation FROM supervisor_dispatches ORDER BY id'),
          bindings: hub.all('SELECT goal_id, kind, host_id, generation, provider_session_id, run_id FROM supervisor_execution_bindings ORDER BY goal_id'),
          workers: readWorkerLogs(workerLogs).map((worker) => ({ name: worker.name, sessionId: worker.sessionId, lines: worker.entries.length })),
          at: new Date().toISOString(),
        };
      } catch (readError) {
        snapshot = { label, error: String(error?.message ?? error), snapshotError: String(readError?.message ?? readError) };
      }
      ctx.evidence.appendJsonl('recovery/timeout-snapshots.jsonl', snapshot);
      throw error;
    }
  };
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
    const ran1 = await snapshotOnFailure('before-send dispatch after restart', waitForChange(ctx, 'before-send dispatch after restart', hubDir, () => {
      const rows = goalRows(hub, goal1);
      return { done: rows.dispatches.some((row) => row.state === 'started') || HELD_STATES.has(rows.goal?.run_state), ...rows };
    }, 120_000));
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
    const typed = await snapshotOnFailure('after-send instruction typed while claimed', waitForChange(ctx, 'after-send instruction typed while claimed', workerLogs, () => {
      const rows = goalRows(hub, goal2 ?? '');
      return { done: inputsWith(workerLogs, marker2).length >= 1 && ['claimed', 'accepted'].includes(rows.dispatches[0]?.state), ...rows };
    }, 120_000));
    transcript('after-send', 'typed');
    const typedState = typed.dispatches[0]?.state;
    ctx.assert('after-send-instruction-typed-while-claimed', typed.dispatches.length === 1 && ['claimed', 'accepted'].includes(typedState), typed);
    const panesMid = await take('after-send-restart');
    await restartHub('after-send');
    const held = await snapshotOnFailure('after-send reconciliation', waitForChange(ctx, 'after-send reconciliation', hubDir, () => {
      const rows = goalRows(hub, goal2);
      return { done: rows.goal?.run_state === 'delivery_uncertain', ...rows };
    }, 60_000));
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
    const started = await snapshotOnFailure('after-start run started', waitForChange(ctx, 'after-start run started', hubDir, () => {
      const rows = goalRows(hub, goal3 ?? '');
      return { done: rows.dispatches.some((row) => row.state === 'started') || HELD_STATES.has(rows.goal?.run_state), ...rows };
    }, 120_000));
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
    transcript.goals.sentinel = goalS;
    const sentinelRan = await snapshotOnFailure('sentinel dispatch after the after-start restart', waitForChange(ctx, 'sentinel dispatch after the after-start restart', hubDir, () => {
      const rows = goalRows(hub, goalS ?? '');
      return { done: rows.dispatches.some((row) => row.state === 'started') || HELD_STATES.has(rows.goal?.run_state), ...rows };
    }, 120_000));
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
    // Each goal launched its own pane: the hub never handed one goal's pane to another.
    const panesByGoal = hub.all('SELECT goal_id, provider_session_id FROM supervisor_execution_bindings WHERE provider_session_id IS NOT NULL ORDER BY goal_id');
    const boundPanes = panesByGoal.map((row) => row.provider_session_id);
    ctx.assert('each-goal-owns-its-pane', boundPanes.length >= 4 && new Set(boundPanes).size === boundPanes.length, { panesByGoal });
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

    // One fault leg: the stub fails every request of the turn, then answers normally again.
    // Before the next leg the engine must have ended this turn: a probe assessment (stub
    // succeeding) is repeated, bounded, until it returns an answer. A turn the engine still
    // runs keeps its decision session open (open_session refuses session_path_in_use), so
    // without this a slow leg cascades into the next. Per-leg stub request counts are kept.
    const legs = [];
    const fault = async (step, status, message, options = {}) => {
      const from = stub.requests.length;
      stub.failNext(status, message, options);
      const reply = await mhc.engineAssess(ctx, engineEnv, step, 'Assess: is the qa goal complete? Answer JSON.');
      stub.succeed();
      const faultRequests = stub.requests.length - from;
      const settle = [];
      const settleDeadline = performance.now() + 120_000;
      for (let attempt = 1; ; attempt += 1) {
        const probe = await mhc.engineAssess(ctx, engineEnv, `${step}-settled-${attempt}`, 'Assess: is the qa goal complete? Answer JSON.');
        settle.push({ attempt, code: probe.code, faultCode: probe.json?.code ?? null });
        if (probe.code === 0) break;
        if (performance.now() >= settleDeadline) {
          throw new HarnessError('DEADLINE_EXCEEDED', `the engine did not end the ${step} turn within 120s`, { settle });
        }
        if (ctx.signal.aborted) throw ctx.signal.reason;
        await new Promise((resolveWait) => setTimeout(resolveWait, Math.min(500 * attempt, 4_000)));
      }
      const leg = { step, status, options, code: reply.code, faultCode: reply.json?.code ?? null, message: reply.json?.message ?? null,
        faultRequests, settleRequests: stub.requests.length - from - faultRequests, settle, lastArrival: stub.arrivals() };
      legs.push(leg);
      ctx.evidence.writeJson('capabilities/fault-legs.json', legs);
      note(`${step}: code=${leg.faultCode ?? reply.code} faultRequests=${faultRequests} settleAttempts=${settle.length}`);
      return reply;
    };
    const auth = await fault('auth', 401, 'invalid x-api-key qa-loopback-fixture');
    ctx.assert('auth-fault-is-typed', auth.code !== 0 && auth.json?.code === 'MHC_AUTH', auth.json);
    // An exhausted quota, as OpenAI reports it: 429 with code insufficient_quota and
    // `x-should-retry: false`. mhc's transport retry stops on that header
    // (maho-ai utils/provider_retry.rs is_retryable) and its turn retry treats the
    // insufficient_quota text as non-retryable (utils/retry.rs NON_RETRYABLE_PROVIDER_ERROR_PATTERN),
    // so the turn fails at once instead of backing off until the assessment times out. A
    // plain 429 "rate limit" is a transient limit that mhc rightly retries.
    const quota = await fault('quota', 429, 'You exceeded your current quota: insufficient_quota (qa)',
      { code: 'insufficient_quota', headers: { 'x-should-retry': 'false' } });
    ctx.assert('quota-fault-is-typed', quota.code !== 0 && quota.json?.code === 'MHC_QUOTA'
      && legs.at(-1).faultRequests >= 1 && legs.at(-1).faultRequests <= 2, { reply: quota.json, leg: legs.at(-1) });
    const provider = await fault('provider', 400, 'qa stub refused the request');
    ctx.assert('provider-fault-is-typed', provider.code !== 0 && provider.json?.code === 'MHC_PROVIDER', provider.json);

    // Missing capability: the next engine launch hides one required capability; only the hub
    // generation restarts (hub stop/start), never the daemon, so every PTY stays alive.
    writeFileSync(dropFile, mhc.DROPPED_CAPABILITY);
    note(`engine wrapper now drops ${mhc.DROPPED_CAPABILITY}; restarting the hub generation only`);
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
    // Baseline by arrival, taken once the old engine process is gone. `hub stop` returns when
    // the generation's tasks are aborted; the engine's teardown is only spawned then
    // (mhc_supervisor.rs StopOnDrop::drop -> stop_engine: stdin close, SHUTDOWN_GRACE 5s, kill),
    // and during that graceful drain mhc's own shutdown work (e.g. the memory extension's
    // shutdown evaluators) may still call the provider. Such a request is the old engine's,
    // not a send of the degraded one, so the baseline waits for its exit (bounded) and every
    // request arriving after it can only come from the new generation's engine.
    const oldGoneDeadline = performance.now() + 30_000;
    let oldExit = oldEngine ? await probeProcess(oldEngine) : null;
    for (let attempt = 1; oldExit && oldExit.state === 'alive' && oldExit.startStamp === oldProbe?.startStamp; attempt += 1) {
      if (performance.now() >= oldGoneDeadline) {
        throw new HarnessError('DEADLINE_EXCEEDED', `the old engine ${oldEngine} did not exit within 30s of hub stop`, { oldProbe, oldExit });
      }
      if (ctx.signal.aborted) throw ctx.signal.reason;
      await new Promise((resolveWait) => setTimeout(resolveWait, Math.min(100 * attempt, 1_000)));
      oldExit = await probeProcess(oldEngine);
    }
    const arrivalsBefore = stub.arrivals();
    const shutdownRequests = stub.requests.filter((entry) => entry.arrival > (legs.at(-1)?.lastArrival ?? 0));
    ctx.evidence.writeJson('capabilities/old-engine-shutdown.json', { oldEngine, oldProbe, oldExit, arrivalsBefore, shutdownRequests });
    const degradedSends = () => stub.requests.filter((entry) => entry.arrival > arrivalsBefore);
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
    ctx.assert('degraded-engine-got-zero-sends', refused.json?.code === 'MHC_CAPABILITY_MISSING' && stub.arrivals() === arrivalsBefore
      && degradedSends().length === 0 && seen.length >= 1 && seen.every((frame) => frame.type === 'get_protocol_info'),
    { frames: seen, arrivalsBefore, arrivalsNow: stub.arrivals(), providerRequests: degradedSends() });
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

// ---------------------------------------------------------------------------
// waiting (plan task 9): five owner submits through `ferryx-cli supervisor request` (parent,
// fail, a, b, c), each run held open by its fixture worker until the runner writes its release
// file. While every run is live the owner links parent -> a, b and fail -> a, c (`depend`); a
// cycle and a self-dependency are refused 400 WAIT_CYCLE. The parents' runs then settle into
// `waiting_children`; a completes (graded by the hub on the retained mhc engine, a loopback
// stub), a's worker re-reports idle/working/idle after its end, c is cancelled (a failed
// dependency) and the cancel is replayed stale; only then does b complete. The hub's own wake
// scheduler decides everything: the parent makes no decision and no model call while b is
// incomplete and resumes exactly once; fail is released to the owner, never completed. The
// hub database is only read, read-only.

const WAITING_WORKSPACE_ID = 'qa-waiting';
const WAITING_PROJECT = 'qa-waiting';
const WAITING_VERIFY = 'test -s result.txt\n';
const WAITING_GOALS = Object.freeze(['parent', 'fail', 'a', 'b', 'c']);
// parent waits on a and b (both complete); fail waits on a and c (c is cancelled).
const WAITING_LINKS = Object.freeze([['parent', 'a'], ['parent', 'b'], ['fail', 'a'], ['fail', 'c']]);
// In each goal's instruction and requirement text, so every model prompt that grades the goal
// (gap assessment contract block, blind convergence requirements) names it.
const waitingMarker = (name) => `[qa-goal:${name}]`;
const waitingWork = (name) => Object.freeze({
  title: `Waiting fixture ${name}`,
  instruction: `Write result.txt and run verify.sh. ${waitingMarker(name)} QA_HOLD:hold-${name}${name === 'a' ? ' QA_ECHO:echo-a' : ''}`,
  requirements: [{ id: 'r1', text: `result.txt is not empty ${waitingMarker(name)}`, verifier: 'verify.sh exits 0' }],
  operations: ['edit', 'test'],
});

async function runWaiting(ctx) {
  if (!POSIX) {
    throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', 'waiting runs POSIX fixture worker panes (the Windows pane shell ignores SHELL); this scenario runs on Linux or macOS');
  }
  const { config } = ctx;
  const git = onPath('git');
  if (!git) throw new HarnessError('PREREQUISITE_MISSING', 'git is not on PATH; the waiting fixture project is a git repository');
  const data = realpathSync(ctx.paths.data);
  const dbPath = join(data, 'hub', 'hub.sqlite');
  if (!isInside(dbPath, data)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `hub database ${dbPath} is outside the run root`);
  const hubDir = dirname(dbPath);
  const log = [];
  const transcript = (line) => { log.push(`${new Date().toISOString()} ${line}`); ctx.log(line); };

  // Fixture: a committed verify.sh in a workspace checkout; one hold directory the workers watch.
  const workspaceRoot = join(data, 'ws');
  const repo = join(workspaceRoot, WAITING_PROJECT);
  mkdirSync(repo, { recursive: true });
  writeFileSync(join(repo, 'verify.sh'), WAITING_VERIFY, { mode: 0o755 });
  const gitArgs = ['-c', 'init.defaultBranch=main', '-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid'];
  for (const [label, args] of [
    ['git-init', [...gitArgs, 'init', '-q', repo]],
    ['git-add', [...gitArgs, '-C', repo, 'add', 'verify.sh']],
    ['git-commit', [...gitArgs, '-C', repo, 'commit', '-q', '-m', 'qa fixture verifier']],
  ]) {
    const result = await ctx.run(label, git, args, { deadlineMs: 30_000 });
    if (result.code !== 0) throw new HarnessError('FIXTURE_FAILED', `${label} exited ${result.code}: ${result.stderr}`);
  }
  const gitCommonDir = realpathSync(join(repo, '.git'));
  const workerLogs = join(data, 'workers');
  const holdDir = join(data, 'holds');
  const requestsDir = join(data, 'requests');
  for (const dir of [workerLogs, holdDir, requestsDir]) mkdirSync(dir, { recursive: true });
  const shell = join(data, 'bin', 'qa-worker-shell');
  mkdirSync(dirname(shell), { recursive: true });
  writeFileSync(shell, `#!/bin/sh\ncase "$1" in\n  -*c*) shift; exec /bin/sh -c "$1" ;;\nesac\nexec ${shellQuote(process.execPath)} ${shellQuote(WORKER)} ${shellQuote(workerLogs)} --multi-turn ${shellQuote(`--hold-dir=${holdDir}`)}\n`, { mode: 0o755 });
  const released = new Set();
  const release = (token) => {
    if (released.has(token)) return;
    released.add(token);
    writeFileSync(join(holdDir, token), `${new Date().toISOString()}\n`);
    transcript(`released ${token}`);
  };

  // Every model request the hub makes, attributed to the goals named by the request's own turn:
  // the user messages after its last assistant message. The decision session retains earlier
  // goals' prompts as history, and the engine appends reminder user messages (<omo-senpi-task>,
  // <memory_notice>) after the prompt, so neither the whole body nor the newest message names
  // the goal being graded.
  const modelCalls = [];
  const stub = await startProviderStub(ctx, { answer: (json) => {
    const messages = Array.isArray(json?.messages) ? json.messages : [];
    const lastAssistant = messages.map((message) => message?.role).lastIndexOf('assistant');
    const text = promptTextOf({ messages: messages.slice(lastAssistant + 1).filter((message) => message?.role === 'user') });
    const convergence = promptTextOf(json).includes(CONVERGENCE_MARKER);
    modelCalls.push({ seq: modelCalls.length + 1, at: new Date().toISOString(), goals: WAITING_GOALS.filter((name) => text.includes(waitingMarker(name))), convergence });
    return convergence ? AUTONOMY_CONVERGED : null;
  } });
  const callsNaming = (name) => modelCalls.filter((call) => call.goals.includes(name));
  try {
    const env = await prepareEngineEnv(ctx, { baseUrl: stub.baseUrl, mhcBin: config.mhcBin, label: 'waiting', extra: {
      SHELL: shell,
      FERRYX_HUB_WORKSPACE: WAITING_WORKSPACE_ID,
      OMON_LANE_AUTO_WORKTREE: '1',
      OMON_INBOX_ENABLED: '0',
    } });
    ctx.announce({ waiting: 'fixture', workspaceRoot, repo, gitCommonDir, holdDir, hubHttp: env.FERRYX_HUB_TRACKER_ADDR, dbPath, provider: stub.baseUrl });

    const setup = await ctx.cli(['supervisor', 'setup', '--owner', config.ownerPrincipal, '--project', WAITING_PROJECT, '--repo-identity', `qa:${WAITING_PROJECT}`,
      '--host', ADMISSION_HOST, '--git-common-dir', gitCommonDir, '--json'], { label: 'supervisor-setup', env });
    if (setup.code !== 0 || !setup.json?.projectId) throw new HarnessError('SETUP_FAILED', `supervisor setup failed: ${JSON.stringify(setup.json)} ${setup.stderr}`);
    const projectId = setup.json.projectId;
    const daemon = await ctx.startDaemon({ env, deadlineMs: 60_000 });
    const engine = await awaitEngine(ctx, env, 'waiting-ready', (json) => json?.engine?.state === 'ready' && json?.hubOpen === true, 120_000);
    ctx.assert('engine-ready', engine?.engine?.state === 'ready', { engine });
    const registered = await registerWorkspace(ctx, join(data, 'runtime', 'daemon.sock'), WAITING_WORKSPACE_ID, realpathSync(workspaceRoot));
    ctx.assert('workspace-registered', registered.at(-1)?.type === 'registerWorkspaceOk', { replies: registered });

    // The owner's channel: one envelope file per request through `ferryx-cli supervisor request`.
    let requestSeq = 0;
    const request = async (label, envelope) => {
      requestSeq += 1;
      const file = join(requestsDir, `${String(requestSeq).padStart(2, '0')}-${label}.json`);
      writeFileSync(file, JSON.stringify(envelope));
      ctx.evidence.appendJsonl('envelopes.jsonl', { seq: requestSeq, label, envelope, at: new Date().toISOString() });
      const result = await ctx.cli(['supervisor', 'request', '--json', file], { label, env });
      transcript(`${label}: exit ${result.code} ${JSON.stringify(result.json)}`);
      return result;
    };

    // 1. Five owner submits; every run starts and stays live (held by its worker).
    const goals = {};
    const submits = {};
    for (const name of WAITING_GOALS) {
      const work = waitingWork(name);
      const submitted = await request(`submit-${name}`, { requestId: `qa-waiting-${name}-${ctx.runId}`, projectId, action: 'submit', payload: { request: work.instruction, work } });
      submits[name] = { code: submitted.code, json: submitted.json };
      goals[name] = submitted.json?.receipt?.receipt?.goal_id ?? null;
    }
    ctx.assert('five-goals-admitted', WAITING_GOALS.every((name) => submits[name].code === 0 && submits[name].json?.receipt?.outcome === 'admitted' && typeof goals[name] === 'string')
      && new Set(Object.values(goals)).size === WAITING_GOALS.length, { goals, submits });
    if (!WAITING_GOALS.every((name) => typeof goals[name] === 'string')) throw new HarnessError('ADMISSION_FAILED', `not every waiting goal was admitted: ${JSON.stringify(goals)}`);
    ctx.announce({ waiting: 'goals', goals });

    const hub = await openHubReader(dbPath);
    const goalOf = (name) => hub.one('SELECT run_state, state_detail, revision, lane_key FROM supervisor_goals WHERE goal_id = ?', goals[name]);
    const snapshot = () => hub.read((query) => Object.fromEntries(WAITING_GOALS.map((name) => [name, {
      goal: query('SELECT run_state, state_detail, revision FROM supervisor_goals WHERE goal_id = ?', goals[name])[0] ?? null,
      dispatches: query('SELECT request_id, state, run_id, provider_session_id FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id', goals[name]),
    }])));
    // One goal's wait/wake bookkeeping, from one snapshot.
    const wakeState = (name) => hub.read((query) => ({
      goal: query('SELECT run_state, state_detail, revision FROM supervisor_goals WHERE goal_id = ?', goals[name])[0] ?? null,
      waits: query('SELECT wait_id, child_goal_id, state FROM supervisor_waits WHERE goal_id = ? ORDER BY rowid', goals[name]),
      waitEvents: query(`SELECT w.child_goal_id, e.event_id, e.outcome FROM supervisor_wait_events e JOIN supervisor_waits w ON w.wait_id = e.wait_id
        WHERE w.goal_id = ? ORDER BY e.received_at, e.event_id`, goals[name]),
      wakes: query('SELECT id, cause, event_id, consumed_at FROM supervisor_pending_wakes WHERE goal_id = ? ORDER BY id', goals[name]),
      turns: query('SELECT id, state, causes, urgent FROM supervisor_wake_turns WHERE goal_id = ? ORDER BY id', goals[name]),
      receipts: query(`SELECT r.wake_id, r.turn_id, r.disposition FROM supervisor_wake_receipts r JOIN supervisor_pending_wakes w ON w.id = r.wake_id
        WHERE w.goal_id = ? ORDER BY r.wake_id`, goals[name]),
      moves: query("SELECT seq, from_state, to_state, from_revision, to_revision FROM supervisor_state_events WHERE goal_id = ? AND source = 'goal' ORDER BY seq", goals[name]),
    }));
    const live = (rows, name) => rows[name].goal?.run_state === 'running' && rows[name].dispatches.length === 1 && rows[name].dispatches[0].state === 'started';
    const started = await waitForChange(ctx, 'every waiting goal running', hubDir, () => {
      const rows = snapshot();
      return { done: WAITING_GOALS.every((name) => live(rows, name) || HELD_STATES.has(rows[name].goal?.run_state)), rows };
    }, 180_000);
    ctx.assert('every-run-started-and-held-live', WAITING_GOALS.every((name) => live(started.rows, name)), started.rows);
    if (!WAITING_GOALS.every((name) => live(started.rows, name))) throw new HarnessError('DISPATCH_FAILED', 'not every waiting goal reached a live run; the dependency links would prove nothing');
    const sessionOf = Object.fromEntries(WAITING_GOALS.map((name) => [name, started.rows[name].dispatches[0].provider_session_id]));
    const workerOf = (name) => readWorkerLogs(workerLogs).find((entry) => entry.sessionId === sessionOf[name]) ?? null;

    // 2. Dependencies, linked by the owner while every run is live; a cycle and a self link are 400.
    const depend = (label, name, child) => request(label, { requestId: `qa-waiting-${label}-${ctx.runId}`, projectId, goalId: goals[name],
      expectedRevision: goalOf(name)?.revision ?? 0, action: 'depend', payload: { childGoalId: goals[child] } });
    const linked = [];
    for (const [name, child] of WAITING_LINKS) {
      const result = await depend(`depend-${name}-on-${child}`, name, child);
      linked.push({ name, child, code: result.code, json: result.json });
    }
    ctx.assert('dependencies-linked-while-running', linked.every(({ child, code, json }) => code === 0 && json?.ok === true && typeof json?.receipt?.waitId === 'string'
      && json.receipt.runState === 'running' && json.receipt.pendingWaits?.some((wait) => wait.childGoalId === goals[child])), linked);
    const waitCount = () => hub.count('SELECT COUNT(*) AS n FROM supervisor_waits');
    const waitsBefore = waitCount();
    const cycle = await depend('depend-a-on-parent-cycle', 'a', 'parent');
    const self = await depend('depend-parent-on-itself', 'parent', 'parent');
    const waitsAfter = waitCount();
    const refused400 = (result) => result.json?.ok === false && result.json?.code === 'WAIT_CYCLE' && result.json?.status === 400;
    ctx.assert('cycle-refused-400', refused400(cycle), { code: cycle.code, json: cycle.json, stderr: cycle.stderr });
    ctx.assert('self-dependency-refused-400', refused400(self), { code: self.code, json: self.json, stderr: self.stderr });
    ctx.assert('refused-links-wrote-nothing', waitsBefore === WAITING_LINKS.length && waitsAfter === waitsBefore, { waitsBefore, waitsAfter });

    // 3. The parents' own runs end: each parks in waiting_children, its settle kept, not decided.
    release('hold-parent');
    release('hold-fail');
    const parked = await waitForChange(ctx, 'parent runs to settle into waiting', hubDir, () => {
      const rows = snapshot();
      return { done: ['parent', 'fail'].every((name) => rows[name].dispatches[0]?.state === 'settled' && rows[name].goal?.run_state !== 'running'), rows };
    }, 120_000);
    ctx.assert('parents-park-in-waiting-children', ['parent', 'fail'].every((name) => parked.rows[name].goal?.run_state === 'waiting_children'
      && parked.rows[name].goal?.state_detail?.startsWith('WAITING_ON')), { parent: parked.rows.parent, fail: parked.rows.fail });

    // 4. The first child completes; both parents record it and still decide nothing.
    release('hold-a');
    const childEndRecorded = (state) => state.waits.find((wait) => wait.child_goal_id === goals.a)?.state === 'satisfied';
    const aEnded = await waitForChange(ctx, 'child a to end and reach its parents', hubDir, () => {
      const goal = goalOf('a');
      const done = HELD_STATES.has(goal?.run_state) && (goal.run_state !== 'completed' || (childEndRecorded(wakeState('parent')) && childEndRecorded(wakeState('fail'))));
      return { done, goal };
    }, 6 * 60_000);
    ctx.assert('child-a-completed', aEnded.goal?.run_state === 'completed', aEnded.goal);
    const undecided = (state) => state.goal?.run_state === 'waiting_children' && state.turns.length === 0
      && state.wakes.filter((wake) => wake.cause === 'turn_settled' && wake.consumed_at === null).length === 1
      && state.wakes.every((wake) => wake.consumed_at === null);
    const parentMid = wakeState('parent');
    const failMid = wakeState('fail');
    // A was graded meanwhile: the scheduler and the judge ran during the wait, so silence about
    // the parents is a decision withheld, not a stopped hub.
    ctx.assert('no-decision-while-a-dependency-is-incomplete', undecided(parentMid) && undecided(failMid) && callsNaming('parent').length === 0
      && callsNaming('fail').length === 0 && callsNaming('a').length > 0, { parent: parentMid, fail: failMid, modelCalls });
    const onlyEnd = (state, child, outcome, ended) => {
      const events = state.waitEvents.filter((event) => event.child_goal_id === goals[child]);
      return events.length === 1 && events[0].outcome === outcome && events[0].event_id === `goal:${goals[child]}:${ended}`;
    };
    ctx.assert('first-child-end-recorded-once', ['parent', 'fail'].every((name) => {
      const state = name === 'parent' ? parentMid : failMid;
      return onlyEnd(state, 'a', 'satisfied', 'completed') && state.waits.filter((wait) => wait.state === 'pending').length === 1;
    }), { parent: parentMid.waitEvents, fail: failMid.waitEvents });

    // 5. Duplicate and out-of-order reports of the ended child: its worker reports idle, working,
    //    idle again after its run settled and its goal completed.
    // Subscribe before the release: the worker's three reports and its `echoed` line land within
    // a millisecond of the release file, before a watch armed afterwards is live.
    const echoWait = waitForChange(ctx, 'child a echo reports', workerLogs, () => {
      const entry = workerOf('a')?.entries.find((line) => line.kind === 'echoed') ?? null;
      return { done: entry !== null, entry };
    }, 30_000);
    release('echo-a');
    const echoed = await echoWait.then((result) => result.entry, (error) => { if (ctx.signal.aborted) throw error; return null; });
    ctx.assert('child-a-reported-again-after-its-end', echoed?.delivered?.length === 3 && echoed.delivered.every(Boolean), { echoed });

    // 6. The second child of `fail` fails (owner cancel): fail is released to the owner, not completed.
    const cancelEnvelope = { requestId: `qa-waiting-cancel-c-${ctx.runId}`, projectId, goalId: goals.c, expectedRevision: goalOf('c')?.revision ?? 0,
      action: 'cancel', payload: { reason: 'qa: child c fails' } };
    const cancelled = await request('cancel-c', cancelEnvelope);
    ctx.assert('child-c-cancelled', cancelled.code === 0 && cancelled.json?.receipt?.runState === 'cancelled', { code: cancelled.code, json: cancelled.json });
    await waitForChange(ctx, 'fail released by its failed dependency', hubDir, () => {
      const state = wakeState('fail');
      return { done: state.goal?.run_state !== 'waiting_children' && state.turns.every((turn) => turn.state !== 'pending'), goal: state.goal };
    }, 120_000);
    const failReleased = wakeState('fail');
    const failTurn = failReleased.turns[0] ?? null;
    ctx.assert('failure-releases-parent-to-owner-not-completion', failReleased.goal?.run_state === 'awaiting_owner'
      && failReleased.goal?.state_detail?.startsWith('DEPENDENCY_FAILED') && failReleased.goal.state_detail.includes(goals.c)
      && !failReleased.moves.some((move) => ['verifying', 'completed'].includes(move.to_state)), { goal: failReleased.goal, moves: failReleased.moves });
    ctx.assert('failure-resumed-once', failReleased.turns.length === 1 && failTurn?.state === 'delivered' && failTurn?.urgent === 1
      && JSON.parse(failTurn.causes).includes('dependency_failed') && failReleased.wakes.filter((wake) => wake.cause === 'dependency_failed').length === 1
      && onlyEnd(failReleased, 'c', 'failed', 'cancelled') && failReleased.waits.find((wait) => wait.child_goal_id === goals.c)?.state === 'failed',
    { turns: failReleased.turns, wakes: failReleased.wakes, waits: failReleased.waits, waitEvents: failReleased.waitEvents });

    // 7. The failure reported again (the same cancel, now stale): 409, and nothing is unblocked twice.
    const replayed = await request('cancel-c-replay', cancelEnvelope);
    ctx.assert('duplicate-failure-report-refused-409', replayed.json?.ok === false && replayed.json?.code === 'REVISION_CONFLICT' && replayed.json?.status === 409,
      { code: replayed.code, json: replayed.json });
    const parentStill = wakeState('parent');
    ctx.assert('parent-still-waiting-after-duplicate-reports', undecided(parentStill) && callsNaming('parent').length === 0
      && onlyEnd(parentStill, 'a', 'satisfied', 'completed'), { parent: parentStill, modelCalls });

    // 8. The last dependency completes: the parent resumes exactly once and completes.
    const callsBeforeResume = modelCalls.length;
    release('hold-b');
    // The goal can reach completed inside the resume turn, before that turn settles: wait for
    // the turn to settle too, or a turn stuck pending fails on the deadline instead of a race.
    const resumed = await waitForChange(ctx, 'parent to complete or hold and its turn to settle', hubDir, () => {
      const goal = goalOf('parent');
      const turns = wakeState('parent').turns;
      return { done: HELD_STATES.has(goal?.run_state) && turns.every((turn) => turn.state !== 'pending'), goal, turns };
    }, 6 * 60_000);
    ctx.assert('child-b-completed', goalOf('b')?.run_state === 'completed', goalOf('b'));
    const parentFinal = wakeState('parent');
    const resumeTurn = parentFinal.turns[0] ?? null;
    const resumeCauses = resumeTurn ? JSON.parse(resumeTurn.causes) : [];
    ctx.assert('parent-resumed-once-and-completed', resumed.goal?.run_state === 'completed' && parentFinal.turns.length === 1 && resumeTurn?.state === 'delivered'
      && resumeTurn.urgent === 1 && resumeCauses.includes('dependency_ready') && resumeCauses.includes('turn_settled')
      && parentFinal.wakes.filter((wake) => wake.cause === 'dependency_ready').length === 1 && parentFinal.wakes.every((wake) => wake.consumed_at !== null)
      && parentFinal.receipts.length === parentFinal.wakes.length && parentFinal.receipts.every((receipt) => receipt.turn_id === resumeTurn.id && receipt.disposition === 'delivered'),
    { goal: resumed.goal, turns: parentFinal.turns, wakes: parentFinal.wakes, receipts: parentFinal.receipts });
    ctx.assert('each-child-end-unblocked-the-parent-once', onlyEnd(parentFinal, 'a', 'satisfied', 'completed') && onlyEnd(parentFinal, 'b', 'satisfied', 'completed')
      && parentFinal.waits.every((wait) => wait.state === 'satisfied') && !parentFinal.waitEvents.some((event) => event.outcome === 'failed'),
    { waits: parentFinal.waits, waitEvents: parentFinal.waitEvents });
    ctx.assert('parent-graded-only-after-it-resumed', callsNaming('parent').length > 0 && callsNaming('parent').every((call) => call.seq > callsBeforeResume),
      { callsBeforeResume, parentCalls: callsNaming('parent') });
    const failFinal = wakeState('fail');
    ctx.assert('failed-parent-unchanged-by-later-events', failFinal.goal?.run_state === 'awaiting_owner' && failFinal.goal.revision === failReleased.goal.revision
      && failFinal.turns.length === 1 && failFinal.wakes.filter((wake) => wake.cause === 'dependency_failed').length === 1 && callsNaming('fail').length === 0,
    { goal: failFinal.goal, turns: failFinal.turns, wakes: failFinal.wakes, failCalls: callsNaming('fail') });
    const finalRows = snapshot();
    ctx.assert('child-a-unchanged-by-its-echo', finalRows.a.goal?.run_state === 'completed' && finalRows.a.dispatches.length === 1
      && finalRows.a.dispatches[0].state === 'settled', finalRows.a);

    // Cleanup: every hold released, the daemon stopped; every fixture worker must be gone.
    release('hold-c');
    const workerPids = readWorkerLogs(workerLogs).map((entry) => Number(entry.name.match(/^worker-(\d+)\.jsonl$/)?.[1])).filter(Number.isInteger);
    const cleanExit = new Set(['stdin-closed', 'hangup', 'terminated']);
    await ctx.terminate(daemon, 'worker-cleanup');
    // The cancelled goal's worker may have been stopped by the cancel itself, without a log line.
    const closed = await waitForChange(ctx, 'fixture worker exit', workerLogs, () => {
      const open = readWorkerLogs(workerLogs).filter((entry) => !entry.entries.some((line) => cleanExit.has(line.kind))
        && pidAlive(Number(entry.name.match(/^worker-(\d+)\.jsonl$/)?.[1])));
      return { done: open.length === 0, open: open.map((entry) => entry.name) };
    }, 30_000).then(() => true, (error) => { if (ctx.signal.aborted) throw error; return false; });
    const alive = await stillAlive(workerPids);
    ctx.assert('fixture-workers-gone', workerPids.length === WAITING_GOALS.length && closed && alive.length === 0, { workerPids, closed, alive });

    for (const entry of readWorkerLogs(workerLogs)) ctx.evidence.writeText(`workers/${entry.name}`, entry.text);
    ctx.evidence.writeJson('waiting.json', {
      worker: { kind: 'fixture', note: 'fixture-worker.mjs --multi-turn --hold-dir: each run reports working and stays live until the runner writes its release file; a re-reports idle/working/idle after its end. Not a real provider worker.' },
      model: { kind: 'loopback stub', note: 'The stub always answers complete and cites sig:req:r1; the check Ferryx ran decides. Calls are attributed by the [qa-goal:NAME] marker in their prompt.' },
      goals, links: WAITING_LINKS, sessions: sessionOf, refusals: { cycle: cycle.json, self: self.json, replay: replayed.json },
      checkpoints: { parked: parked.rows, afterFirstChild: { parent: parentMid, fail: failMid }, afterFailure: failReleased, beforeResume: parentStill, final: { parent: parentFinal, fail: failFinal, rows: finalRows } },
      modelCalls, callsBeforeResume,
    });
  } finally {
    for (const name of WAITING_GOALS) {
      try { release(`hold-${name}`); } catch { /* the run root may be gone */ }
    }
    await stub.close();
    ctx.evidence.writeText('waiting-transcript.log', `${log.join('\n')}\n`);
  }
}

// burst (plan task 9): two owner submits (burst, other), each run held live by its fixture
// worker. While burst's run is live its own worker sends four ordinary reports through the
// product ingress (a `supervisorReport` line on the daemon's agent-state socket, the channel
// its `agentState` reports use) at real 0, 1, 2 and 3s, then a retry of the newest and a replay
// of the oldest; other's worker reports once; a session bound to no goal is refused. No clock
// seam: the runner holds burst's run until the hub's own clock is past the oldest report plus
// the 30s batch window, and asserts that until then no wake turn and no model call exists for
// burst. Then the run settles: exactly one delivered turn carries every report, each receipted
// once, at or after oldest + 30s. The hub database is only read, read-only.

const BURST_WORKSPACE_ID = 'qa-burst';
const BURST_PROJECT = 'qa-burst';
const BURST_GOALS = Object.freeze(['burst', 'other']);
// lanes/dispatch.rs BATCH_WINDOW_SECS.
const BURST_WINDOW_MS = 30_000;
// Real spacing of the four reports, from the first.
const BURST_OFFSETS_MS = Object.freeze([0, 1_000, 2_000, 3_000]);
// How far past oldest + window burst's run is held before it may settle.
const BURST_HOLD_PAST_MS = 3_000;
const burstWork = (name) => Object.freeze({
  title: `Burst fixture ${name}`,
  instruction: `Write result.txt and run verify.sh. ${waitingMarker(name)} QA_HOLD:hold-${name}`,
  requirements: [{ id: 'r1', text: `result.txt is not empty ${waitingMarker(name)}`, verifier: 'verify.sh exits 0' }],
  operations: ['edit', 'test'],
});

// Resolves after `ms` of real time; rejects when the run is aborted. Time is the behaviour
// under test in burst (the report spacing and the batch window), so these pauses are its input.
function pauseFor(ctx, ms) {
  return new Promise((resolvePause, rejectPause) => {
    if (ctx.signal.aborted) { rejectPause(ctx.signal.reason); return; }
    const onAbort = () => { clearTimeout(timer); rejectPause(ctx.signal.reason); };
    const timer = setTimeout(() => { ctx.signal.removeEventListener('abort', onAbort); resolvePause(); }, Math.max(0, ms));
    ctx.signal.addEventListener('abort', onAbort, { once: true });
  });
}

// One raw line on the agent-state socket, as a process that is not a goal's worker would send.
function agentStateExchange(socketPath, line, deadlineMs = 10_000) {
  return new Promise((resolveExchange) => {
    let buffer = '';
    let done = false;
    const connection = createConnection(socketPath);
    const finish = (value) => { if (done) return; done = true; clearTimeout(timer); connection.destroy(); resolveExchange(value); };
    const timer = setTimeout(() => finish({ error: `no reply within ${deadlineMs}ms` }), deadlineMs);
    connection.setEncoding('utf8');
    connection.on('error', (error) => finish({ error: String(error?.message ?? error) }));
    connection.on('connect', () => connection.write(`${line}\n`));
    connection.on('data', (chunk) => {
      buffer += chunk;
      const end = buffer.indexOf('\n');
      if (end < 0) return;
      try { finish({ reply: JSON.parse(buffer.slice(0, end)) }); } catch { finish({ error: `unparsed reply ${buffer.slice(0, end)}` }); }
    });
  });
}

async function runBurst(ctx) {
  if (!POSIX) {
    throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', 'burst runs POSIX fixture worker panes (the Windows pane shell ignores SHELL); this scenario runs on Linux or macOS');
  }
  const { config } = ctx;
  const git = onPath('git');
  if (!git) throw new HarnessError('PREREQUISITE_MISSING', 'git is not on PATH; the burst fixture project is a git repository');
  const data = realpathSync(ctx.paths.data);
  const dbPath = join(data, 'hub', 'hub.sqlite');
  if (!isInside(dbPath, data)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `hub database ${dbPath} is outside the run root`);
  const hubDir = dirname(dbPath);
  const log = [];
  const transcript = (line) => { log.push(`${new Date().toISOString()} ${line}`); ctx.log(line); };

  const workspaceRoot = join(data, 'ws');
  const repo = join(workspaceRoot, BURST_PROJECT);
  mkdirSync(repo, { recursive: true });
  writeFileSync(join(repo, 'verify.sh'), WAITING_VERIFY, { mode: 0o755 });
  const gitArgs = ['-c', 'init.defaultBranch=main', '-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid'];
  for (const [label, args] of [
    ['git-init', [...gitArgs, 'init', '-q', repo]],
    ['git-add', [...gitArgs, '-C', repo, 'add', 'verify.sh']],
    ['git-commit', [...gitArgs, '-C', repo, 'commit', '-q', '-m', 'qa fixture verifier']],
  ]) {
    const result = await ctx.run(label, git, args, { deadlineMs: 30_000 });
    if (result.code !== 0) throw new HarnessError('FIXTURE_FAILED', `${label} exited ${result.code}: ${result.stderr}`);
  }
  const gitCommonDir = realpathSync(join(repo, '.git'));
  const workerLogs = join(data, 'workers');
  const holdDir = join(data, 'holds');
  const requestsDir = join(data, 'requests');
  for (const dir of [workerLogs, holdDir, requestsDir]) mkdirSync(dir, { recursive: true });
  const shell = join(data, 'bin', 'qa-worker-shell');
  mkdirSync(dirname(shell), { recursive: true });
  writeFileSync(shell, `#!/bin/sh\ncase "$1" in\n  -*c*) shift; exec /bin/sh -c "$1" ;;\nesac\nexec ${shellQuote(process.execPath)} ${shellQuote(WORKER)} ${shellQuote(workerLogs)} --multi-turn ${shellQuote(`--hold-dir=${holdDir}`)}\n`, { mode: 0o755 });
  const released = new Set();
  const release = (token) => {
    if (released.has(token)) return;
    released.add(token);
    writeFileSync(join(holdDir, token), `${new Date().toISOString()}\n`);
    transcript(`released ${token}`);
  };

  // Model calls attributed to the goals named by the request's own turn (see runWaiting).
  const modelCalls = [];
  const stub = await startProviderStub(ctx, { answer: (json) => {
    const messages = Array.isArray(json?.messages) ? json.messages : [];
    const lastAssistant = messages.map((message) => message?.role).lastIndexOf('assistant');
    const text = promptTextOf({ messages: messages.slice(lastAssistant + 1).filter((message) => message?.role === 'user') });
    const convergence = promptTextOf(json).includes(CONVERGENCE_MARKER);
    modelCalls.push({ seq: modelCalls.length + 1, at: new Date().toISOString(), goals: BURST_GOALS.filter((name) => text.includes(waitingMarker(name))), convergence });
    return convergence ? AUTONOMY_CONVERGED : null;
  } });
  try {
    const env = await prepareEngineEnv(ctx, { baseUrl: stub.baseUrl, mhcBin: config.mhcBin, label: 'burst', extra: {
      SHELL: shell,
      FERRYX_HUB_WORKSPACE: BURST_WORKSPACE_ID,
      OMON_LANE_AUTO_WORKTREE: '1',
      OMON_INBOX_ENABLED: '0',
    } });
    ctx.announce({ burst: 'fixture', workspaceRoot, repo, gitCommonDir, holdDir, dbPath, provider: stub.baseUrl });

    const setup = await ctx.cli(['supervisor', 'setup', '--owner', config.ownerPrincipal, '--project', BURST_PROJECT, '--repo-identity', `qa:${BURST_PROJECT}`,
      '--host', ADMISSION_HOST, '--git-common-dir', gitCommonDir, '--json'], { label: 'supervisor-setup', env });
    if (setup.code !== 0 || !setup.json?.projectId) throw new HarnessError('SETUP_FAILED', `supervisor setup failed: ${JSON.stringify(setup.json)} ${setup.stderr}`);
    const projectId = setup.json.projectId;
    const daemon = await ctx.startDaemon({ env, deadlineMs: 60_000 });
    const engine = await awaitEngine(ctx, env, 'burst-ready', (json) => json?.engine?.state === 'ready' && json?.hubOpen === true, 120_000);
    ctx.assert('engine-ready', engine?.engine?.state === 'ready', { engine });
    const registered = await registerWorkspace(ctx, join(data, 'runtime', 'daemon.sock'), BURST_WORKSPACE_ID, realpathSync(workspaceRoot));
    ctx.assert('workspace-registered', registered.at(-1)?.type === 'registerWorkspaceOk', { replies: registered });

    // 1. Two owner submits; both runs start and stay live.
    const goals = {};
    const submits = {};
    for (const name of BURST_GOALS) {
      const work = burstWork(name);
      const file = join(requestsDir, `submit-${name}.json`);
      const envelope = { requestId: `qa-burst-${name}-${ctx.runId}`, projectId, action: 'submit', payload: { request: work.instruction, work } };
      writeFileSync(file, JSON.stringify(envelope));
      ctx.evidence.appendJsonl('envelopes.jsonl', { label: `submit-${name}`, envelope, at: new Date().toISOString() });
      const submitted = await ctx.cli(['supervisor', 'request', '--json', file], { label: `submit-${name}`, env });
      transcript(`submit-${name}: exit ${submitted.code} ${JSON.stringify(submitted.json)}`);
      submits[name] = { code: submitted.code, json: submitted.json };
      goals[name] = submitted.json?.receipt?.receipt?.goal_id ?? null;
    }
    ctx.assert('two-goals-admitted', BURST_GOALS.every((name) => submits[name].code === 0 && submits[name].json?.receipt?.outcome === 'admitted'
      && typeof goals[name] === 'string') && goals.burst !== goals.other, { goals, submits });
    if (!BURST_GOALS.every((name) => typeof goals[name] === 'string')) throw new HarnessError('ADMISSION_FAILED', `not every burst goal was admitted: ${JSON.stringify(goals)}`);
    ctx.announce({ burst: 'goals', goals });

    const hub = await openHubReader(dbPath);
    const snapshot = () => hub.read((query) => Object.fromEntries(BURST_GOALS.map((name) => [name, {
      goal: query('SELECT run_state, state_detail, revision, lane_key FROM supervisor_goals WHERE goal_id = ?', goals[name])[0] ?? null,
      dispatches: query('SELECT request_id, state, run_id, provider_session_id FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id', goals[name]),
    }])));
    const wakeState = (name) => hub.read((query) => ({
      goal: query('SELECT run_state, state_detail, revision, lane_key FROM supervisor_goals WHERE goal_id = ?', goals[name])[0] ?? null,
      wakes: query('SELECT id, cause, event_id, due_at, consumed_at, created_at FROM supervisor_pending_wakes WHERE goal_id = ? ORDER BY id', goals[name]),
      turns: query('SELECT id, state, causes, urgent, judge_score, created_at, settled_at FROM supervisor_wake_turns WHERE goal_id = ? ORDER BY id', goals[name]),
      receipts: query(`SELECT r.wake_id, r.turn_id, r.disposition FROM supervisor_wake_receipts r JOIN supervisor_pending_wakes w ON w.id = r.wake_id
        WHERE w.goal_id = ? ORDER BY r.wake_id, r.turn_id`, goals[name]),
      laneEvents: query(`SELECT e.id, e.kind, e.source_event_id, e.created_at, e.delivered_at, e.delivery_reason FROM lane_events e
        JOIN supervisor_goals g ON g.lane_key = e.lane_key WHERE g.goal_id = ? ORDER BY e.id`, goals[name]),
    }));
    const live = (rows, name) => rows[name].goal?.run_state === 'running' && rows[name].dispatches.length === 1 && rows[name].dispatches[0].state === 'started';
    const started = await waitForChange(ctx, 'both burst goals running', hubDir, () => {
      const rows = snapshot();
      return { done: BURST_GOALS.every((name) => live(rows, name) || HELD_STATES.has(rows[name].goal?.run_state)), rows };
    }, 180_000);
    ctx.assert('both-runs-started-and-held-live', BURST_GOALS.every((name) => live(started.rows, name)), started.rows);
    if (!BURST_GOALS.every((name) => live(started.rows, name))) throw new HarnessError('DISPATCH_FAILED', 'not every burst goal reached a live run; its worker could not report');
    const sessionOf = Object.fromEntries(BURST_GOALS.map((name) => [name, started.rows[name].dispatches[0].provider_session_id]));
    const workerOf = (name) => readWorkerLogs(workerLogs).find((entry) => entry.sessionId === sessionOf[name]) ?? null;
    const repliesOf = (name) => (workerOf(name)?.entries ?? []).filter((line) => line.kind === 'supervisor-report');

    // 2. Reports through the worker's own pane: the runner drops a report file; the worker sends
    //    it. Written by rename, so the worker never reads half a file.
    let reportSeq = 0;
    const sendReport = (name, eventId) => {
      reportSeq += 1;
      const temp = join(holdDir, `.tmp-report-${reportSeq}`);
      writeFileSync(temp, `${eventId}\n`);
      renameSync(temp, join(holdDir, `report-${sessionOf[name]}-${String(reportSeq).padStart(3, '0')}`));
      transcript(`report ${name} ${eventId} (#${reportSeq})`);
    };
    const awaitReplies = (name, count, label) => waitForChange(ctx, label, workerLogs, () => {
      const replies = repliesOf(name);
      return { done: replies.length >= count, replies };
    }, 30_000);

    const firstFour = awaitReplies('burst', 4, 'four burst report replies');
    const t0 = Date.now();
    for (const [index, offset] of BURST_OFFSETS_MS.entries()) {
      await pauseFor(ctx, t0 + offset - Date.now());
      sendReport('burst', `r-${index}`);
    }
    const originals = (await firstFour).replies.slice(0, 4);
    ctx.assert('four-reports-accepted-for-their-own-goal', originals.length === 4 && originals.every((entry, index) => entry.eventId === `r-${index}`
      && entry.reply?.ok === true && entry.reply.goalId === goals.burst && entry.reply.laneKey === started.rows.burst.goal?.lane_key && entry.reply.duplicate === false)
      && new Set(originals.map((entry) => entry.reply?.laneEventId)).size === 4, { originals });

    // A retry of the newest and an out-of-order replay of the oldest.
    const repeats = awaitReplies('burst', 6, 'duplicate burst report replies');
    sendReport('burst', 'r-3');
    sendReport('burst', 'r-0');
    const duplicates = (await repeats).replies.slice(4, 6);
    const laneEventOf = (eventId) => originals.find((entry) => entry.eventId === eventId)?.reply?.laneEventId ?? null;
    ctx.assert('duplicate-and-out-of-order-reports-are-the-same-events', duplicates.length === 2 && duplicates.every((entry) => entry.reply?.ok === true
      && entry.reply.duplicate === true && entry.reply.goalId === goals.burst && entry.reply.laneEventId === laneEventOf(entry.eventId))
      && duplicates.map((entry) => entry.eventId).join(',') === 'r-3,r-0', { duplicates, originals });

    // An unrelated goal's worker reports once; a session bound to no goal is refused.
    const otherReply = awaitReplies('other', 1, 'other goal report reply');
    sendReport('other', 'r-0');
    const otherReport = (await otherReply).replies[0] ?? null;
    ctx.assert('unrelated-report-lands-on-its-own-goal', otherReport?.reply?.ok === true && otherReport.reply.goalId === goals.other
      && otherReport.reply.duplicate === false, { otherReport });
    const laneEventsBefore = hub.count('SELECT COUNT(*) AS n FROM lane_events');
    const stranger = await agentStateExchange(join(env.FERRYX_RUNTIME_DIR, 'agent-state.sock'),
      JSON.stringify({ type: 'supervisorReport', sessionId: `qa-not-a-worker-${ctx.runId}`, eventId: 'r-0', body: 'not a worker' }));
    const laneEventsAfter = hub.count('SELECT COUNT(*) AS n FROM lane_events');
    ctx.assert('report-from-an-unbound-session-refused', stranger.reply?.ok === false && stranger.reply.code === 'NOT_FOUND' && laneEventsAfter === laneEventsBefore,
      { stranger, laneEventsBefore, laneEventsAfter });

    // 2b. Urgent owner pause inside an open report window, on the other goal: its report is
    //     pending and its window (oldest report + 30s, hub clock) has not elapsed. The owner
    //     pauses through `ferryx-cli supervisor request`; the scheduler delivers the owner wake
    //     at once, alone and unjudged (wakes.rs tick step 3), before that window ends, while the
    //     report wake stays pending behind the pause (a paused goal is Deferral::Held).
    const otherReported = wakeState('other');
    const otherReportEvent = otherReported.laneEvents.find((event) => event.kind === 'worker_report') ?? null;
    const otherWindowEndMs = Date.parse(otherReportEvent?.created_at ?? '') + BURST_WINDOW_MS;
    const pauseEnvelope = { requestId: `qa-burst-pause-other-${ctx.runId}`, projectId, goalId: goals.other, expectedRevision: otherReported.goal?.revision,
      action: 'pause', payload: { reason: 'qa urgent pause inside the report window' } };
    const pauseFile = join(requestsDir, 'pause-other.json');
    writeFileSync(pauseFile, JSON.stringify(pauseEnvelope));
    ctx.evidence.appendJsonl('envelopes.jsonl', { label: 'pause-other', envelope: pauseEnvelope, at: new Date().toISOString() });
    const pauseSentMs = Date.now();
    const pauseRequest = await ctx.cli(['supervisor', 'request', '--json', pauseFile], { label: 'pause-other', env });
    transcript(`pause-other: exit ${pauseRequest.code} ${JSON.stringify(pauseRequest.json)}`);
    const ownerTurnsOf = (state) => state.turns.filter((turn) => JSON.parse(turn.causes).includes('owner_control'));
    // Bounded by the window end itself: a pause turn later than that is the defect asserted below.
    const pauseSettled = pauseRequest.code !== 0 ? null : await waitForChange(ctx, 'urgent pause turn on the other goal', hubDir, () => {
      const state = wakeState('other');
      const turn = ownerTurnsOf(state)[0] ?? null;
      return { done: turn !== null && turn.state !== 'pending', state, observedMs: Date.now() };
    }, Math.max(1_000, otherWindowEndMs - Date.now())).then((value) => value, (error) => { if (ctx.signal.aborted) throw error; return null; });
    const atPause = pauseSettled?.state ?? wakeState('other');
    const pauseEvent = hub.one(`SELECT seq, from_state, to_state, from_revision, to_revision, recorded_at FROM supervisor_state_events
      WHERE goal_id = ? AND source = 'goal' AND to_state = 'paused' ORDER BY seq LIMIT 1`, goals.other);
    const pauseTurn = ownerTurnsOf(atPause)[0] ?? null;
    const ownerWakes = atPause.wakes.filter((wake) => wake.cause === 'owner_control');
    const heldReport = atPause.wakes.find((wake) => wake.cause === 'report') ?? null;
    const pauseDelayMs = pauseTurn && pauseEvent ? Date.parse(pauseTurn.created_at) - Date.parse(pauseEvent.recorded_at) : null;
    ctx.assert('owner-pause-applied-inside-the-open-report-window', pauseRequest.code === 0 && Number.isFinite(otherWindowEndMs)
      && pauseSentMs < otherWindowEndMs && pauseEvent?.from_state === 'running' && Date.parse(pauseEvent.recorded_at) < otherWindowEndMs
      && pauseEvent.to_revision === (otherReported.goal?.revision ?? -1) + 1,
    { pause: { code: pauseRequest.code, json: pauseRequest.json }, pauseEvent, otherReportEvent, windowEnd: new Date(otherWindowEndMs || 0).toISOString() });
    ctx.assert('urgent-pause-delivered-before-the-window-ends', pauseTurn?.state === 'delivered' && Number(pauseTurn.urgent) === 1
      && JSON.stringify(JSON.parse(pauseTurn.causes)) === JSON.stringify(['owner_control']) && pauseTurn.judge_score === null
      && Date.parse(pauseTurn.created_at) >= Date.parse(pauseEvent?.recorded_at ?? '') && Date.parse(pauseTurn.settled_at ?? '') < otherWindowEndMs
      && ownerWakes.length === 1 && ownerWakes[0].consumed_at !== null
      && JSON.stringify(atPause.receipts.filter((receipt) => receipt.turn_id === pauseTurn.id).map((receipt) => [receipt.wake_id, receipt.disposition])) === JSON.stringify([[ownerWakes[0].id, 'delivered']]),
    { pauseTurn, pauseDelayMs, ownerWakes, receipts: atPause.receipts, windowEnd: new Date(otherWindowEndMs || 0).toISOString(), observedAt: pauseSettled && new Date(pauseSettled.observedMs).toISOString() });
    ctx.assert('pending-report-held-behind-the-pause', heldReport !== null && heldReport.event_id === `lane_event:${otherReport?.reply?.laneEventId}`
      && heldReport.consumed_at === null && !atPause.receipts.some((receipt) => receipt.wake_id === heldReport.id)
      && atPause.goal?.run_state === 'paused' && ownerTurnsOf(atPause).length === atPause.turns.length,
    { heldReport, goal: atPause.goal, turns: atPause.turns, receipts: atPause.receipts });

    // 3. The window, on the hub's own clock: the oldest report's lane event time.
    const afterReports = wakeState('burst');
    const reportEvents = afterReports.laneEvents.filter((event) => event.kind === 'worker_report');
    const oldestMs = Math.min(...reportEvents.map((event) => Date.parse(event.created_at)));
    const windowEndMs = oldestMs + BURST_WINDOW_MS;
    const spanMs = Math.max(...reportEvents.map((event) => Date.parse(event.created_at))) - oldestMs;
    ctx.assert('reports-spread-over-three-seconds-inside-the-window', reportEvents.length === 4 && Number.isFinite(oldestMs)
      && spanMs >= 2_500 && spanMs < BURST_WINDOW_MS, { reportEvents, spanMs });
    const expectedWakeIds = reportEvents.map((event) => `lane_event:${event.id}`).sort();
    ctx.assert('each-report-handed-to-the-goal-once', reportEvents.every((event) => event.delivery_reason === 'supervisor_wake' && event.delivered_at !== null)
      && JSON.stringify(afterReports.wakes.filter((wake) => wake.cause === 'report').map((wake) => wake.event_id).sort()) === JSON.stringify(expectedWakeIds),
    { laneEvents: afterReports.laneEvents, wakes: afterReports.wakes });
    ctx.announce({ burst: 'window', oldest: new Date(oldestMs).toISOString(), windowEnd: new Date(windowEndMs).toISOString() });

    // Hold the run past the window end; then nothing may have been decided or asked for burst.
    await pauseFor(ctx, windowEndMs + BURST_HOLD_PAST_MS - Date.now());
    const atWindowEnd = wakeState('burst');
    const callsInWindow = modelCalls.filter((call) => Date.parse(call.at) >= oldestMs && Date.parse(call.at) < windowEndMs);
    ctx.assert('no-wake-turn-before-the-window-ends', atWindowEnd.turns.length === 0 && atWindowEnd.receipts.length === 0
      && atWindowEnd.wakes.every((wake) => wake.consumed_at === null), { now: new Date().toISOString(), turns: atWindowEnd.turns, wakes: atWindowEnd.wakes });
    ctx.assert('no-model-call-between-the-reports-and-the-window-end', callsInWindow.length === 0
      && modelCalls.every((call) => !call.goals.includes('burst')), { callsInWindow, modelCalls });

    // 4. The run settles: one turn takes every report with the settle.
    const callsBeforeRelease = modelCalls.length;
    release('hold-burst');
    const ended = await waitForChange(ctx, 'burst to complete or hold', hubDir, () => {
      const state = wakeState('burst');
      return { done: HELD_STATES.has(state.goal?.run_state) && state.turns.every((turn) => turn.state !== 'pending'), goal: state.goal };
    }, 6 * 60_000);
    const final = wakeState('burst');
    ctx.assert('burst-goal-completed', ended.goal?.run_state === 'completed', ended.goal);
    const reportWakes = final.wakes.filter((wake) => wake.cause === 'report');
    const turn = final.turns[0] ?? null;
    const causes = turn ? JSON.parse(turn.causes) : [];
    ctx.assert('exactly-one-wake-turn-delivered-once', final.turns.length === 1 && turn?.state === 'delivered' && causes.includes('report')
      && final.turns.filter((row) => row.state === 'delivered').length === 1, { turns: final.turns });
    ctx.assert('turn-at-or-after-oldest-report-plus-window', turn !== null && Date.parse(turn.created_at) >= windowEndMs,
      { oldest: new Date(oldestMs).toISOString(), windowEnd: new Date(windowEndMs).toISOString(), turn });
    ctx.assert('each-report-receipted-once', reportWakes.length === 4 && JSON.stringify(reportWakes.map((wake) => wake.event_id).sort()) === JSON.stringify(expectedWakeIds)
      && reportWakes.every((wake) => wake.consumed_at !== null && final.receipts.filter((receipt) => receipt.wake_id === wake.id).length === 1
        && final.receipts.find((receipt) => receipt.wake_id === wake.id)?.turn_id === turn?.id
        && final.receipts.find((receipt) => receipt.wake_id === wake.id)?.disposition === 'delivered'),
    { reportWakes, receipts: final.receipts, turn });
    ctx.assert('duplicates-added-no-event-wake-or-turn', final.laneEvents.filter((event) => event.kind === 'worker_report').length === 4
      && reportWakes.length === 4 && final.turns.length === 1, { laneEvents: final.laneEvents, wakes: final.wakes, turns: final.turns });
    const otherState = wakeState('other');
    const otherWakeIds = otherState.wakes.filter((wake) => wake.cause === 'report').map((wake) => wake.event_id);
    // The other goal's only turn so far is the owner pause: its report is still held.
    ctx.assert('unrelated-report-does-not-wake-this-goal', otherWakeIds.length === 1 && otherWakeIds[0] === `lane_event:${otherReport?.reply?.laneEventId}`
      && !final.wakes.some((wake) => otherWakeIds.includes(wake.event_id)) && otherState.turns.every((row) => !JSON.parse(row.causes).includes('report')),
    { otherWakes: otherState.wakes, otherTurns: otherState.turns, burstWakes: final.wakes });
    ctx.assert('burst-graded-only-after-the-window', modelCalls.filter((call) => call.goals.includes('burst')).every((call) => call.seq > callsBeforeRelease
      && Date.parse(call.at) >= windowEndMs), { callsBeforeRelease, burstCalls: modelCalls.filter((call) => call.goals.includes('burst')) });

    // 5. The owner resumes the other goal and its run settles: the report held behind the pause
    //    is delivered in a later turn, never lost.
    const otherPaused = wakeState('other');
    const resumeEnvelope = { requestId: `qa-burst-resume-other-${ctx.runId}`, projectId, goalId: goals.other, expectedRevision: otherPaused.goal?.revision, action: 'resume', payload: {} };
    const resumeFile = join(requestsDir, 'resume-other.json');
    writeFileSync(resumeFile, JSON.stringify(resumeEnvelope));
    ctx.evidence.appendJsonl('envelopes.jsonl', { label: 'resume-other', envelope: resumeEnvelope, at: new Date().toISOString() });
    const resumeRequest = await ctx.cli(['supervisor', 'request', '--json', resumeFile], { label: 'resume-other', env });
    transcript(`resume-other: exit ${resumeRequest.code} ${JSON.stringify(resumeRequest.json)}`);
    release('hold-other');
    const otherEnded = resumeRequest.code !== 0 ? null : await waitForChange(ctx, 'other to complete or hold after its resume', hubDir, () => {
      const state = wakeState('other');
      return { done: HELD_STATES.has(state.goal?.run_state) && state.goal?.run_state !== 'paused' && state.turns.every((row) => row.state !== 'pending'), state };
    }, 6 * 60_000);
    const otherFinal = otherEnded?.state ?? wakeState('other');
    const reportReceipt = heldReport ? otherFinal.receipts.find((receipt) => receipt.wake_id === heldReport.id) ?? null : null;
    const reportTurn = reportReceipt ? otherFinal.turns.find((row) => row.id === reportReceipt.turn_id) ?? null : null;
    ctx.assert('held-report-delivered-after-the-resume', resumeRequest.code === 0 && reportReceipt?.disposition === 'delivered'
      && reportTurn?.state === 'delivered' && pauseTurn !== null && reportTurn.id > pauseTurn.id && JSON.parse(reportTurn.causes).includes('report')
      && otherFinal.wakes.find((wake) => wake.id === heldReport.id)?.consumed_at !== null,
    { resume: { code: resumeRequest.code, json: resumeRequest.json }, goal: otherFinal.goal, turns: otherFinal.turns, receipts: otherFinal.receipts, wakes: otherFinal.wakes });

    // Cleanup: every hold released, the daemon stopped; every fixture worker must be gone.
    release('hold-other');
    const workerPids = readWorkerLogs(workerLogs).map((entry) => Number(entry.name.match(/^worker-(\d+)\.jsonl$/)?.[1])).filter(Number.isInteger);
    const cleanExit = new Set(['stdin-closed', 'hangup', 'terminated']);
    await ctx.terminate(daemon, 'worker-cleanup');
    const closed = await waitForChange(ctx, 'fixture worker exit', workerLogs, () => {
      const open = readWorkerLogs(workerLogs).filter((entry) => !entry.entries.some((line) => cleanExit.has(line.kind))
        && pidAlive(Number(entry.name.match(/^worker-(\d+)\.jsonl$/)?.[1])));
      return { done: open.length === 0, open: open.map((entry) => entry.name) };
    }, 30_000).then(() => true, (error) => { if (ctx.signal.aborted) throw error; return false; });
    const alive = await stillAlive(workerPids);
    ctx.assert('fixture-workers-gone', workerPids.length === BURST_GOALS.length && closed && alive.length === 0, { workerPids, closed, alive });

    for (const entry of readWorkerLogs(workerLogs)) ctx.evidence.writeText(`workers/${entry.name}`, entry.text);
    ctx.evidence.writeJson('burst.json', {
      worker: { kind: 'fixture', note: 'fixture-worker.mjs --multi-turn --hold-dir: each run stays live until its release file; its reports are supervisorReport lines on the agent-state socket, one per runner-written report file. Not a real provider worker.' },
      model: { kind: 'loopback stub', note: 'The stub always answers complete and cites sig:req:r1. Calls are attributed by the [qa-goal:NAME] marker in their own turn.' },
      ingress: 'agent-state socket supervisorReport -> hub_runtime::supervisor_worker_report -> lanes::dispatch::report_from_worker -> hand_to_supervisor -> wakes::report',
      window: { oldest: new Date(oldestMs).toISOString(), windowEnd: new Date(windowEndMs).toISOString(), spanMs, heldPastMs: BURST_HOLD_PAST_MS },
      goals, sessions: sessionOf, replies: { originals, duplicates, other: otherReport, stranger },
      urgentPause: { otherWindowEnd: new Date(otherWindowEndMs || 0).toISOString(), pauseEvent, pauseTurn, pauseDelayMs, heldReport, atPause, otherFinal },
      checkpoints: { afterReports, atWindowEnd, final, other: otherState },
      modelCalls, callsBeforeRelease,
    });
  } finally {
    for (const name of BURST_GOALS) {
      try { release(`hold-${name}`); } catch { /* the run root may be gone */ }
    }
    await stub.close();
    ctx.evidence.writeText('burst-transcript.log', `${log.join('\n')}\n`);
  }
}

// The memory and usage scenarios (task 15, memory-usage.mjs) reuse these helpers.
const memoryUsageDeps = Object.freeze({
  supervisedHub,
  hubControl,
  work: (instruction) => ({ ...ADMISSION_WORK, instruction }),
});
const loadMemoryUsage = async () => import('./memory-usage.mjs');

// The legacy scenario (task 16, legacy.mjs) reuses these helpers.
const legacyDeps = Object.freeze({
  supervisedHub,
  hubControl,
  hubStatusUntil,
  goalRows,
  inputsWith,
  readWorkerLogs,
  waitForChange,
  survived,
  shellQuote,
  heldStates: HELD_STATES,
  work: ADMISSION_WORK,
  project: ADMISSION_PROJECT,
});
const loadLegacy = async () => import('./legacy.mjs');

// The correction scenario (task 10, correction.mjs) reuses these helpers.
const correctionDeps = Object.freeze({
  onPath,
  shellQuote,
  worker: WORKER,
  promptTextOf,
  convergenceMarker: CONVERGENCE_MARKER,
  converged: AUTONOMY_CONVERGED,
  registerWorkspace,
  openHubReader,
  hubStatusUntil,
  hubRunning,
  hubControl,
  waitForChange,
  readWorkerLogs,
  stillAlive,
  heldStates: HELD_STATES,
});
const loadCorrection = async () => import('./correction.mjs');

export const SCENARIOS = Object.freeze({
  // Task 6, implemented in mhc.mjs.
  runtime: RUNTIME_SCENARIO,
  admission: Object.freeze({
    implemented: true,
    task: '7',
    summary: 'Submit the fixture request, replay the same source id, ask "What can you do?": one goal, one initial dispatch; then the owner\'s Slack DM through the production Slack adapter: one goal, foreign user and redelivery add nothing (task 11)',
    deadlineMs: 14 * 60_000,
    requiredAssertions: [
      'setup-created-identity-project-and-host', 'setup-rerun-returns-the-same-ids', 'setup-conflicting-rebind-refused', 'workspace-registered', 'hub-open-before-the-first-submit',
      'submit-admitted-with-acknowledgment-naming-goal', 'initial-dispatch-started-without-drive-call', 'exactly-one-worker-started',
      'replay-returns-the-same-receipt', 'replay-created-no-second-dispatch', 'edited-replay-returns-request-conflict',
      'edited-replay-created-no-second-worker', 'question-is-conversational', 'question-created-no-goal-or-dispatch',
      'final-one-goal-one-dispatch-one-worker',
      // Slack leg (task 11, slack.mjs).
      'slack-owner-identity-provisioned', 'slack-adapter-connected-through-loopback-socket-mode',
      'owner-dm-created-exactly-one-goal-through-common-admission', 'owner-reply-carries-the-persisted-acknowledgment',
      'foreign-user-yields-no-goal-and-no-learned-fact', 'owner-text-learned-once', 'duplicate-delivery-yields-no-new-goal-and-no-learned-fact',
      'foreign-text-never-reached-the-engine',
    ],
    run: runAdmission,
  }),
  autonomy: Object.freeze({
    implemented: true,
    task: '10',
    summary: 'First verification forced to fail, corrected result allowed: two run ids, attested pass, one completed goal, no owner nudge',
    deadlineMs: 10 * 60_000,
    requiredAssertions: [
      'engine-ready', 'workspace-registered', 'submit-admitted', 'goal-completed-without-owner', 'two-runs-at-admitted-revision',
      'correction-followed-first-settle-and-names-r1', 'verifier-failure-took-the-corrective-path', 'one-corrective-turn-counted',
      'verified-names-second-run-and-current-revision', 'r1-met-with-ferryx-check-and-run-provenance', 'grading-ran-on-mhc',
      'only-submit-was-requested', 'worker-received-exactly-two-instructions', 'final-bytes-verify-on-disk', 'fixture-workers-gone',
    ],
    notCovered: [
      { clause: 'model-judgment', reason: 'the provider is a loopback stub that always claims completion; only the Ferryx-run check decides', coveredBy: 'crates/ferryx-hub/src/supervisor/tests/completion.rs' },
      { clause: 'remote-run', reason: 'a run on a paired host is held REMOTE_CHECK_UNAVAILABLE, not graded', coveredBy: 'crates/ferryx-hub/src/supervisor/tests/completion.rs a_remote_run_is_never_checked_against_the_local_checkout' },
    ],
    run: runAutonomy,
  }),
  correction: Object.freeze({
    implemented: true,
    task: '10',
    summary: 'Submit with the hub generation stopped, inject owner correction "Use DONE instead of READY" before dispatch, replay it at the stale expectedRevision: old dispatch superseded and never typed, final bytes DONE verified, goal completed, stale mutation 409 changes nothing',
    deadlineMs: 12 * 60_000,
    requiredAssertions: CORRECTION_REQUIRED_ASSERTIONS,
    notCovered: [
      { clause: 'correction-while-claimed', reason: 'the runner holds the first dispatch by stopping the hub before it is claimed; a correction racing a claimed send (the typing fence) has no reachable window without a product hook', coveredBy: 'crates/ferryx-hub/src/supervisor/tests/request.rs a_correction_fences_a_claimed_send_and_the_claim_is_released_untransmitted' },
      { clause: 'stale-decision-after-correction', reason: 'a model decision computed at the pre-correction revision needs a decision in flight across the correction', coveredBy: 'crates/ferryx-hub/src/supervisor/tests/completion.rs a_stale_decision_cannot_overwrite_an_owner_correction' },
    ],
    run: (ctx) => loadCorrection().then((module) => module.runCorrection(ctx, correctionDeps)),
  }),
  waiting: Object.freeze({
    implemented: true,
    task: '9',
    summary: 'Parent waits on two live children, one completes, the other completes (or, for a second parent, fails): no decision or model call while incomplete, one resume, failure is not completion; duplicate/out-of-order child reports unblock nothing twice; a cycle is 400',
    deadlineMs: 15 * 60_000,
    requiredAssertions: [
      'engine-ready', 'workspace-registered', 'five-goals-admitted', 'every-run-started-and-held-live', 'dependencies-linked-while-running',
      'cycle-refused-400', 'self-dependency-refused-400', 'refused-links-wrote-nothing', 'parents-park-in-waiting-children', 'child-a-completed',
      'no-decision-while-a-dependency-is-incomplete', 'first-child-end-recorded-once', 'child-a-reported-again-after-its-end', 'child-c-cancelled',
      'failure-releases-parent-to-owner-not-completion', 'failure-resumed-once', 'duplicate-failure-report-refused-409',
      'parent-still-waiting-after-duplicate-reports', 'child-b-completed', 'parent-resumed-once-and-completed', 'each-child-end-unblocked-the-parent-once',
      'parent-graded-only-after-it-resumed', 'failed-parent-unchanged-by-later-events', 'child-a-unchanged-by-its-echo', 'fixture-workers-gone',
    ],
    notCovered: [
      { clause: 'cross-owner-link', reason: 'an isolated installation has exactly one supervisor owner, so a second owner\'s goal cannot be created through the product', coveredBy: 'crates/ferryx-hub/src/supervisor/tests/wakes.rs a_cycle_a_self_dependency_and_a_cross_owner_link_are_refused_as_400' },
      { clause: 'restart-replay', reason: 'replaying the state log from a rewound cursor needs a database write; the runner only reads the hub database', coveredBy: 'crates/ferryx-hub/src/supervisor/tests/wakes.rs a_restart_recovers_pending_ready_events_from_the_durable_cursor' },
    ],
    run: runWaiting,
  }),
  burst: Object.freeze({
    implemented: true,
    task: '9',
    summary: 'Four ordinary reports from a live run\'s own worker at real 0,1,2,3s, a retry and an out-of-order replay, another goal\'s report: no turn and no model call before oldest+30s, then exactly one delivered turn with each report receipted once; an owner pause of the other goal inside its open report window is delivered at once, its report held and delivered after the resume',
    deadlineMs: 16 * 60_000,
    requiredAssertions: [
      'engine-ready', 'workspace-registered', 'two-goals-admitted', 'both-runs-started-and-held-live', 'four-reports-accepted-for-their-own-goal',
      'duplicate-and-out-of-order-reports-are-the-same-events', 'unrelated-report-lands-on-its-own-goal', 'report-from-an-unbound-session-refused',
      'reports-spread-over-three-seconds-inside-the-window', 'each-report-handed-to-the-goal-once', 'no-wake-turn-before-the-window-ends',
      'no-model-call-between-the-reports-and-the-window-end', 'burst-goal-completed', 'exactly-one-wake-turn-delivered-once',
      'turn-at-or-after-oldest-report-plus-window', 'each-report-receipted-once', 'duplicates-added-no-event-wake-or-turn',
      'unrelated-report-does-not-wake-this-goal', 'burst-graded-only-after-the-window', 'owner-pause-applied-inside-the-open-report-window',
      'urgent-pause-delivered-before-the-window-ends', 'pending-report-held-behind-the-pause', 'held-report-delivered-after-the-resume', 'fixture-workers-gone',
    ],
    notCovered: [
      { clause: 'report-only-window-on-an-idle-goal', reason: 'a worker reports only while its run is live, so the goal is busy and its reports wait for the settle; the runner holds the run past oldest+30s on the real clock. A report-only batch admitted by the window itself (and its judge call) is never reached through a live worker', coveredBy: 'crates/ferryx-hub/src/supervisor/tests/wakes.rs reports_batch_on_the_oldest_and_an_owner_pause_skips_the_window' },
      { clause: 'ended-goal-report', reason: 'a worker of an ended goal is gone with its pane, so a report for it cannot be sent through the product', coveredBy: 'crates/ferryx-hub/src/lanes/dispatch.rs a_worker_report_becomes_one_wake_on_its_own_goal' },
    ],
    run: runBurst,
  }),
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
      'pty-census-unchanged-across-restarts', 'each-goal-owns-its-pane', 'no-instruction-typed-twice',
    ],
    // No product surface advances an execution generation for a live goal (advance_generation is
    // called only from tests), so a predecessor's late completion cannot be provoked end to end.
    notCovered: ['predecessor-completion-after-new-generation: crates/ferryx-hub/src/supervisor/tests/recovery.rs a_predecessor_completion_after_a_new_generation_is_ignored'],
    run: runRecovery,
  }),
  authority: Object.freeze({
    implemented: true,
    task: '13',
    summary: 'Owner goal provisioned; mirror device, excluded device, body-named actor, and a real pane (CLI request, `supervisor submit` and a `ferryx-cli mcp` `supervisor.request` submit with FERRYX_SESSION_ID removed, raw socket approve/"approve everything"/submit) are refused: goal, dispatches, requests and memory root unchanged; then the owner device pauses it (200), the replay is 409, memory receipts 503 without an engine',
    deadlineMs: 5 * 60_000,
    requiredAssertions: ['hub-open-before-the-owner-submit', 'owner-goal-held-for-the-owner', 'non-owner-devices-refused-403', 'body-named-actor-refused-400', 'pane-inherits-the-isolated-daemon-env',
      'pane-cli-approve-forbidden', 'pane-cli-dispatch-submit-forbidden', 'pane-mcp-submit-forbidden', 'pane-mcp-opens-the-isolated-hub-db', 'pane-socket-approve-teach-submit-forbidden', 'goal-unchanged-after-refusals', 'zero-dispatch-after-refusals',
      'no-request-or-wait-written', 'memory-root-unchanged', 'authorized-snapshot-200-lists-the-goal', 'authorized-pause-200', 'stale-revision-409',
      'memory-receipts-unavailable-503', 'only-the-owner-changed-the-goal'],
    run: runAuthority,
  }),
  memory: Object.freeze({
    implemented: true,
    task: '15',
    summary: 'Teach through the desktop adapter (receipt + provenance), recall in a new context, worker-injected MCP text never teaches, a one-time exception reaches only its goal and is absent next run, forget then daemon-hub restart and replay of the old message: the item never reaches the model again',
    deadlineMs: 20 * 60_000,
    requiredAssertions: MEMORY_REQUIRED_ASSERTIONS,
    run: (ctx) => loadMemoryUsage().then((module) => module.runMemory(ctx, memoryUsageDeps)),
  }),
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
  legacy: Object.freeze({
    implemented: true,
    task: '16',
    summary: 'Upgrade from an omo snapshot with historical closed/failed/blocked lanes, a legacy Slack session and cron jobs: nothing runs and new work is unavailable; cutover without an mhc capability is refused while the old conversation stays readable; cutover adopts one safe lane once (retry is duplicate); a rollback stops new scheduling and keeps receipts and panes; a second cutover recovers; cron jobs never change',
    deadlineMs: 15 * 60_000,
    requiredAssertions: LEGACY_REQUIRED_ASSERTIONS,
    notCovered: [
      { clause: 'session-adoption-untrusted-context', reason: 'the fixture worker does not show what it was told beyond the owner request line, so the fenced history is asserted on the stored dispatch instruction in the hub test', coveredBy: 'crates/ferryx-hub/src/supervisor/tests/cutover.rs adopting_a_legacy_lane_or_session_happens_once_and_a_retry_is_a_duplicate' },
      { clause: 'legacy-goal-loop-after-cutover', reason: 'a legacy goal turn needs a live legacy pane and a judge answer; the runner has no product surface that starts a legacy lane', coveredBy: 'crates/ferryx-hub/src/supervisor/tests/cutover.rs a_cutover_stops_the_legacy_intake_before_the_unified_intake_opens' },
    ],
    run: (ctx) => loadLegacy().then((module) => module.runLegacy(ctx, legacyDeps)),
  }),
  channels: planned('11,17', 'Approved Slack QA thread submit, desktop correction, phone pause/resume, Slack query: same supervisor/goal/revision, no leakage'),
  // Task 14, implemented in surfaces.mjs.
  surfaces: SURFACES_SCENARIO,
  usage: Object.freeze({
    implemented: true,
    task: '15',
    summary: 'Two differing goals assessed through the engine share one prefix hash; every engine call has a cause and unreported usage is null; a goal waiting on a subscribed dependency makes zero provider requests and zero model-call rows across delivered owner wakes',
    deadlineMs: 20 * 60_000,
    requiredAssertions: USAGE_REQUIRED_ASSERTIONS,
    run: (ctx) => loadMemoryUsage().then((module) => module.runUsage(ctx, memoryUsageDeps)),
  }),
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
import { SURFACES_SCENARIO } from './surfaces.mjs';
