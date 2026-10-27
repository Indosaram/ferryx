// Fixtures and steps of the `runtime` scenario (plan todo 6): the hub's retained mhc supervisor
// engine, driven through the product surface `ferryx-cli supervisor engine status|assess`.
//
// Isolation: every file lives under the run's data root. The isolated HOME carries the hub env
// file (FERRYX_HUB_ENV_FILE), the mhc agent dir (MAHO_CODING_AGENT_DIR) with a models.json whose
// only provider is a loopback openai-completions stub served by this runner, and the mhc memory
// home (MAHO_MEMORY_HOME) under the run's memory root. No production credentials or state are read.
import { existsSync, lstatSync, mkdirSync, readFileSync, realpathSync, unlinkSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { createServer as createTcpServer } from 'node:net';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HarnessError, isInside, POSIX, probeProcess, sameStartStamp } from './harness.mjs';

export const MHC_FILTER = fileURLToPath(new URL('./fixture-mhc-filter.mjs', import.meta.url));
export const STUB_PROVIDER = 'qa-stub';
export const STUB_MODEL = 'qa-supervisor-1';
/// What the stub answers to every prompt: a valid gap assessment (goals/types.rs `Assessment`).
export const STUB_ASSESSMENT = Object.freeze({ gaps: [], decision: { decision: 'complete' } });
/// The capability the degraded case removes; the hub requires it (mhc_profile REQUIRED_CAPABILITIES).
export const DROPPED_CAPABILITY = 'memory_forget';

const shellQuote = (value) => `'${String(value).replace(/'/g, `'\\''`)}'`;

// The daemon resolves its paths from HOME/FERRYX_* as given (not canonicalized), so paths it
// reports are compared against the same unresolved run roots; realpath only for containment.
const dataOf = (ctx) => ctx.paths.data;
const within = (path, root) => isInside(path, root) || (existsSync(path) && isInside(realpathSync(path), realpathSync(root)));

function freeLoopbackPort() {
  return new Promise((resolvePort, rejectPort) => {
    const server = createTcpServer();
    server.unref();
    server.on('error', rejectPort);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => resolvePort(port));
    });
  });
}

// ---------------------------------------------------------------------------
// Loopback openai-completions stub. Records every request (method, path, model, whether a
// tools list was offered and its names) and answers with one streamed assistant message, or
// with an HTTP error while `failWith` is set.

export async function startProviderStub(ctx) {
  const requests = [];
  const state = { failWith: null };
  const server = createServer((request, response) => {
    let body = '';
    request.setEncoding('utf8');
    request.on('data', (chunk) => { body += chunk; });
    request.on('end', () => {
      let json = null;
      try { json = JSON.parse(body); } catch { /* recorded raw */ }
      const tools = Array.isArray(json?.tools) ? json.tools.map((tool) => tool?.function?.name ?? tool?.name ?? null) : [];
      const entry = { at: new Date().toISOString(), method: request.method, path: request.url, model: json?.model ?? null, stream: json?.stream ?? null, tools, failWith: state.failWith };
      requests.push(entry);
      ctx.evidence.appendJsonl('runtime/provider-requests.jsonl', entry);
      ctx.journal.push({ source: 'provider-stub', kind: 'request', ...entry });
      if (state.failWith) {
        response.writeHead(state.failWith.status, { 'content-type': 'application/json' });
        response.end(JSON.stringify({ error: { message: state.failWith.message, type: 'qa_stub_error' } }));
        return;
      }
      if (request.method === 'GET' && request.url?.replace(/\?.*$/, '').endsWith('/models')) {
        response.writeHead(200, { 'content-type': 'application/json' });
        response.end(JSON.stringify({ object: 'list', data: [{ id: STUB_MODEL, object: 'model', owned_by: 'qa' }] }));
        return;
      }
      if (!request.url?.endsWith('/chat/completions')) {
        response.writeHead(404, { 'content-type': 'application/json' });
        response.end(JSON.stringify({ error: { message: `no route ${request.url}` } }));
        return;
      }
      const id = `qa-${requests.length}`;
      const chunk = (delta, finish = null) => `data: ${JSON.stringify({ id, object: 'chat.completion.chunk', created: 0, model: STUB_MODEL,
        choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`;
      response.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
      response.write(chunk({ role: 'assistant', content: JSON.stringify(STUB_ASSESSMENT) }));
      response.write(chunk({}, 'stop'));
      response.write(`data: ${JSON.stringify({ id, object: 'chat.completion.chunk', created: 0, model: STUB_MODEL, choices: [],
        usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 } })}\n\n`);
      response.end('data: [DONE]\n\n');
    });
  });
  await new Promise((resolveListen, rejectListen) => {
    server.once('error', rejectListen);
    server.listen(0, '127.0.0.1', resolveListen);
  });
  const { port } = server.address();
  const baseUrl = `http://127.0.0.1:${port}/v1`;
  ctx.journal.push({ source: 'provider-stub', kind: 'listening', baseUrl });
  return {
    baseUrl,
    requests,
    failNext(status, message) { state.failWith = { status, message }; },
    succeed() { state.failWith = null; },
    close: () => new Promise((resolveClose) => { server.closeAllConnections?.(); server.close(() => resolveClose()); }),
  };
}

// ---------------------------------------------------------------------------
// Isolated mhc profile: agent dir with the stub provider, memory home, hub env file.

/**
 * Writes the run-local mhc agent dir and the hub env file that turns the supervisor on with the
 * stub provider, and returns the product env for the daemon and CLI. `mhcBin` is what the hub
 * launches (the real mhc, or the capability-filter wrapper).
 */
export async function prepareEngineEnv(ctx, { baseUrl, mhcBin, label, extra = {} }) {
  const data = dataOf(ctx);
  const memory = ctx.paths.memory;
  const home = join(data, 'home');
  const agentDir = join(home, '.omo', 'agent');
  mkdirSync(agentDir, { recursive: true });
  // A fixture key only the loopback stub ever sees.
  const models = { providers: { [STUB_PROVIDER]: { api: 'openai-completions', baseUrl, apiKey: 'qa-loopback-fixture', models: [{ id: STUB_MODEL, contextWindow: 32000, maxTokens: 2048 }] } } };
  writeFileSync(join(agentDir, 'models.json'), `${JSON.stringify(models, null, 2)}\n`, { mode: 0o600 });
  const envFile = join(data, `hub-${label}.env`);
  writeFileSync(envFile, [
    'FERRYX_HUB_MHC_SUPERVISOR=1',
    `FERRYX_HUB_MHC_BIN=${mhcBin}`,
    `FERRYX_HUB_MHC_PROVIDER=${STUB_PROVIDER}`,
    `FERRYX_HUB_MHC_MODEL=${STUB_MODEL}`,
    '',
  ].join('\n'), { mode: 0o600 });
  for (const path of [agentDir, envFile]) {
    if (!within(path, data)) throw new HarnessError('HARNESS_ERROR', `${path} escapes the run data root ${data}`);
  }
  // The hub HTTP listener gets a run-local port instead of the production default :9119.
  const trackerAddr = `127.0.0.1:${await freeLoopbackPort()}`;
  ctx.evidence.writeJson(`runtime/engine-env-${label}.json`, { envFile, agentDir, models, mhcBin, memoryHome: join(memory, 'mhc'), trackerAddr });
  return ctx.productEnv({
    FERRYX_HUB_ENV_FILE: envFile,
    FERRYX_HUB_TRACKER_ADDR: trackerAddr,
    MAHO_CODING_AGENT_DIR: agentDir,
    MAHO_MEMORY_HOME: join(memory, 'mhc'),
    ...extra,
  });
}

/**
 * The POSIX capability-filter wrapper: a #! script that execs node on fixture-mhc-filter.mjs,
 * which runs the real mhc and hides `capability` from get_protocol_info. Windows has no #!
 * launch for the hub's child resolution here, so the degraded case fails closed there.
 */
export function writeCapabilityFilter(ctx, { realMhc, capability }) {
  if (!POSIX) {
    throw new HarnessError('FIXTURE_UNSUPPORTED_PLATFORM', 'the mhc capability-filter wrapper is a POSIX #! script; the runtime scenario\'s degraded case does not run on win32');
  }
  const data = dataOf(ctx);
  const wrapper = join(data, 'bin', 'mhc-without-capability');
  const frames = join(data, 'runtime-filter-frames.jsonl');
  mkdirSync(join(data, 'bin'), { recursive: true });
  writeFileSync(wrapper, [
    '#!/bin/sh',
    `UNIFIED_SUPERVISOR_MHC_REAL=${shellQuote(realMhc)}`,
    `UNIFIED_SUPERVISOR_MHC_DROP_CAPABILITY=${shellQuote(capability)}`,
    `UNIFIED_SUPERVISOR_MHC_FRAMES=${shellQuote(frames)}`,
    'export UNIFIED_SUPERVISOR_MHC_REAL UNIFIED_SUPERVISOR_MHC_DROP_CAPABILITY UNIFIED_SUPERVISOR_MHC_FRAMES',
    `exec ${shellQuote(process.execPath)} ${shellQuote(MHC_FILTER)} "$@"`,
    '',
  ].join('\n'), { mode: 0o755 });
  return { wrapper, frames };
}

export function readFrames(path) {
  if (!existsSync(path)) return [];
  return readFileSync(path, 'utf8').split('\n').filter(Boolean).map((line) => {
    try { return JSON.parse(line); } catch { return { unparsed: line }; }
  });
}

// ---------------------------------------------------------------------------
// Product surface.

/** `ferryx-cli supervisor engine status`, run from `cwd` (the CLI's own cwd never matters). */
export async function engineStatus(ctx, env, label) {
  const result = await ctx.cli(['supervisor', 'engine', 'status', '--json'], { label: `engine-status-${label}`, env, deadlineMs: 30_000 });
  ctx.evidence.writeJson(`runtime/status-${label}.json`, { code: result.code, json: result.json, stderr: result.stderr });
  return result;
}

export async function engineAssess(ctx, env, label, prompt) {
  const result = await ctx.cli(['supervisor', 'engine', 'assess', '--prompt', prompt, '--json'], { label: `engine-assess-${label}`, env, deadlineMs: 180_000 });
  ctx.evidence.writeJson(`runtime/assess-${label}.json`, { code: result.code, json: result.json, stderr: result.stderr });
  return result;
}

/**
 * Polls the engine status through the product CLI until `predicate(json)` holds. The daemon
 * answers MHC_NOT_RUNNING until its hub generation has installed the engine (FERRYX_HUB=on in
 * the product env opens the hub right after readiness); that and a reply matching nothing are
 * retried, any other failure ends the wait. Each poll is a bounded CLI call; the loop is bounded
 * by `deadlineMs` and ends on the first match. There is no event to subscribe to across the
 * process boundary, so the poll interval only spaces the CLI round trips.
 */
export async function awaitEngine(ctx, env, label, predicate, deadlineMs) {
  const deadline = performance.now() + deadlineMs;
  let attempt = 0;
  let last = null;
  for (;;) {
    attempt += 1;
    const result = await ctx.cli(['supervisor', 'engine', 'status', '--json'], { label: `engine-await-${label}-${attempt}`, env, deadlineMs: 30_000 });
    last = { code: result.code, timedOut: result.timedOut, json: result.json };
    if (result.code === 0 && predicate(result.json)) {
      ctx.evidence.writeJson(`runtime/status-${label}.json`, { attempt, ...last });
      return result.json;
    }
    // A status waits for the engine state lock, which a launch holds through its handshake.
    if (result.code !== 0 && !result.timedOut && result.json?.code !== 'MHC_NOT_RUNNING') {
      throw new HarnessError('ENGINE_STATUS_FAILED', `engine status failed while waiting for ${label}`, last);
    }
    if (performance.now() >= deadline) {
      throw new HarnessError('DEADLINE_EXCEEDED', `engine state ${label} was not observed within ${deadlineMs}ms`, { last, attempt });
    }
    // The next CLI round trip is the wait; a short pause keeps the poll from spinning.
    await new Promise((resolveWait) => setTimeout(resolveWait, Math.min(250 * attempt, 2_000)));
    if (ctx.signal.aborted) throw ctx.signal.reason;
  }
}

// ---------------------------------------------------------------------------
// Restart only the owned engine.

const ENGINE_ARGS = ['--mode', 'rpc', '--multi-session'];

function argvOf(probe) {
  if (Array.isArray(probe.argv)) return probe.argv;
  if (typeof probe.command === 'string') return probe.command.split(/\s+/);
  return null;
}

/**
 * Kills the engine the hub recorded in `<data>/mhc-supervisor.pid`, and nothing else. The pid
 * must match the engine status the daemon reported, the record must name the daemon as its
 * owner, the process argv must be `<mhc> --mode rpc --multi-session ...` with the configured
 * binary, and the start stamp read before signalling must equal the one re-read just before
 * the signal. Anything else is refused with a named HarnessError.
 */
export async function killOwnedEngine(ctx, { stateDir, reportedPid, daemonPid, mhcBin }) {
  if (!POSIX) throw new HarnessError('FIXTURE_UNSUPPORTED_PLATFORM', 'the engine restart signals by recorded pid with POSIX identity checks; it does not run on win32');
  const pidFile = join(stateDir, 'mhc-supervisor.pid');
  if (!within(pidFile, dataOf(ctx))) throw new HarnessError('PRODUCTION_PATH_REFUSED', `${pidFile} is outside the run data root`);
  let recorded;
  try { recorded = readFileSync(pidFile, 'utf8').trim().split(/\s+/).map(Number); } catch (error) {
    throw new HarnessError('ENGINE_IDENTITY', `no engine pid record at ${pidFile}: ${error?.message ?? error}`);
  }
  const [pid, owner] = recorded;
  const first = await probeProcess(pid);
  const argv = argvOf(first);
  const identity = {
    pidFile, recorded, reportedPid, daemonPid, state: first.state, startStamp: first.startStamp ?? null, argv,
    ownerMatches: owner === daemonPid, reportedMatches: pid === reportedPid,
    binaryMatches: Boolean(argv && (argv[0] === mhcBin || argv.slice(0, 3).some((part) => part === mhcBin))),
    argsMatch: Boolean(argv && ENGINE_ARGS.every((arg) => argv.includes(arg))),
  };
  ctx.evidence.writeJson('runtime/engine-identity.json', identity);
  if (first.state !== 'alive' || !identity.ownerMatches || !identity.reportedMatches || !identity.argsMatch || !identity.binaryMatches || !identity.startStamp) {
    throw new HarnessError('ENGINE_IDENTITY', 'the recorded engine pid does not match the owned engine; nothing was signalled', identity);
  }
  const second = await probeProcess(pid);
  if (!sameStartStamp({ startStamp: identity.startStamp }, second)) {
    throw new HarnessError('ENGINE_IDENTITY', `pid ${pid} changed between the identity check and the signal; nothing was signalled`, { identity, second });
  }
  process.kill(pid, 'SIGKILL');
  ctx.journal.push({ source: 'runner', kind: 'engine-killed', pid, startStamp: identity.startStamp });
  return identity;
}

/** The engine state dir (`<data>` of the hub) under the isolated HOME. */
export function engineStateDir(ctx) {
  return join(dataOf(ctx), 'home', '.ferryx', 'hub');
}

/**
 * Starts the isolated daemon from `cwd` (the parent directory the hub resolves nothing from),
 * with the socket barrier armed before the spawn, as ctx.startDaemon does for the data root.
 */
export async function startDaemonFrom(ctx, { id, env, cwd, deadlineMs = 60_000 }) {
  if (!POSIX) throw new HarnessError('DAEMON_READINESS_UNDEFINED', 'the runtime scenario waits on the POSIX daemon socket');
  const runtimeDir = join(dataOf(ctx), 'runtime');
  mkdirSync(runtimeDir, { recursive: true, mode: 0o700 });
  mkdirSync(cwd, { recursive: true });
  const socket = join(runtimeDir, 'daemon.sock');
  // A predecessor daemon of this run may leave its socket behind; the barrier must see the new one.
  try { if (lstatSync(socket).isSocket()) unlinkSync(socket); } catch (error) { if (error?.code !== 'ENOENT') throw error; }
  const ready = ctx.waitForPath(socket, { deadlineMs, label: `${id} socket` });
  ready.catch(() => { /* raced below */ });
  const cursor = ctx.journal.cursor();
  const handle = await ctx.spawn(id, ctx.config.ferryxCli, ['--daemon'], { env, cwd, graceMs: 10_000 });
  const local = new AbortController();
  const forward = () => local.abort(ctx.signal.reason);
  ctx.signal.addEventListener('abort', forward, { once: true });
  const died = ctx.waitFor(`${id} exit`, (e) => e.source === id && e.kind === 'exit' && e.pid === handle.pid, { after: cursor, deadlineMs, signal: local.signal })
    .then((e) => { throw new HarnessError('DAEMON_EXITED', `${id} exited before ready (code ${e.code}, signal ${e.signal})`); });
  died.catch(() => { /* raced below */ });
  try {
    await Promise.race([ready, died]);
  } finally {
    ctx.signal.removeEventListener('abort', forward);
    local.abort(new HarnessError('SUPERSEDED', `${id} readiness settled`));
  }
  return handle;
}

// ---------------------------------------------------------------------------
// The `runtime` scenario.

const ASSESS_PROMPT = 'QA runtime scenario: grade the empty fixture lane against its contract and answer with the assessment JSON only.';

const bindingOf = (status) => status?.engine?.sessions?.['decision-0'] ?? null;
const parseReply = (text) => { try { return JSON.parse(String(text ?? '').trim()); } catch { return null; } };
const noPaneInput = (json) => Array.isArray(json?.actions) && json.actions.length === 0;

async function runRuntime(ctx) {
  if (!POSIX) {
    throw new HarnessError('FIXTURE_UNSUPPORTED_PLATFORM', 'the runtime scenario waits on the POSIX daemon socket, restarts the engine by POSIX pid identity and uses a #! capability filter; it does not run on win32');
  }
  const { config } = ctx;
  const data = dataOf(ctx);
  const stateDir = engineStateDir(ctx);
  const log = [];
  const note = (line) => { log.push(`${new Date().toISOString()} ${line}`); ctx.log(line); };
  const stub = await startProviderStub(ctx);
  try {
    const env = await prepareEngineEnv(ctx, { baseUrl: stub.baseUrl, mhcBin: config.mhcBin, label: 'real' });

    // 1. Real mhc under daemon A, started from parent cwd A.
    const parentA = join(data, 'parents', 'a');
    const daemonA = await startDaemonFrom(ctx, { id: 'daemon-a', env, cwd: parentA });
    const ready = (json) => json?.engine?.state === 'ready' && json?.hubOpen === true;
    const readyA = await awaitEngine(ctx, env, 'a-ready', ready, 120_000);
    note(`daemon-a pid=${daemonA.pid} engine pid=${readyA.engine.pid} generation=${readyA.engine.generation}`);
    ctx.assert('engine-ready-with-required-capabilities', readyA.engine.faultCode === null && Number.isInteger(readyA.engine.pid)
      && ['multi_session', 'memory_write_deny', 'tools_exclude', 'memory_forget', 'supervisor_work_tools'].every((name) => readyA.engine.capabilities.includes(name)),
      readyA.engine);
    ctx.assert('profile-cwd-is-under-the-isolated-data-root', within(readyA.engine.profileCwd, data)
      && readyA.engine.profileCwd === join(stateDir, 'mhc-supervisor'), { profileCwd: readyA.engine.profileCwd, stateDir });

    // 2. One structured assessment through the product CLI.
    const providerBefore = stub.requests.length;
    const first = await engineAssess(ctx, env, 'a-first', ASSESS_PROMPT);
    const firstReply = parseReply(first.json?.assessment?.reply);
    ctx.assert('assessment-returns-a-structured-decision', first.code === 0 && first.json?.ok === true
      && JSON.stringify(firstReply) === JSON.stringify(STUB_ASSESSMENT), { code: first.code, json: first.json, stderr: first.stderr });
    const turnRequests = stub.requests.slice(providerBefore);
    ctx.assert('assessment-reached-only-the-loopback-stub-model', turnRequests.length >= 1 && turnRequests.every((entry) => entry.model === STUB_MODEL),
      { turnRequests });
    const offered = [...new Set(turnRequests.flatMap((entry) => entry.tools))];
    const decisionContext = first.json?.engine?.decisionContext ?? {};
    const excluded = String(decisionContext.tools_exclude ?? '').split(',').filter(Boolean);
    ctx.assert('decision-session-has-no-effect-tools', decisionContext.memory_write === 'deny'
      && ['bash', 'edit', 'write'].every((tool) => excluded.includes(tool))
      && offered.every((tool) => !excluded.includes(tool)), { decisionContext, offeredTools: offered });
    ctx.assert('assessment-sent-no-pane-input', noPaneInput(first.json), { actions: first.json?.actions ?? null });
    const binding = bindingOf(first.json);
    ctx.assert('decision-session-binding-persisted', typeof binding?.durableSessionId === 'string' && binding.role === 'decision'
      && typeof binding.sessionFile === 'string', { binding });
    note(`assessment ok; decision-0 durable=${binding?.durableSessionId} file=${binding?.sessionFile}`);

    // 3. Restart only the owned engine: kill the recorded pid after the identity checks; the
    //    daemon (and its hub) stays up and relaunches the engine on its next use.
    const generationA = first.json.engine.generation;
    const killed = await killOwnedEngine(ctx, { stateDir, reportedPid: first.json.engine.pid, daemonPid: daemonA.pid, mhcBin: config.mhcBin });
    note(`killed owned engine pid=${killed.recorded[0]} start=${killed.startStamp}`);
    ctx.assert('engine-identity-checked-before-signal', killed.ownerMatches && killed.reportedMatches && killed.argsMatch && killed.binaryMatches,
      killed);
    // An engine that ran under a minute backs off 5s before its relaunch; wait it out (or
    // see the hub's own supervision relaunch it first).
    await awaitEngine(ctx, env, 'a-exited', (json) => (json?.engine?.state !== 'ready' && (json?.engine?.retryInMs ?? 0) === 0)
      || (json?.engine?.state === 'ready' && json.engine.generation > generationA), 60_000);
    const second = await engineAssess(ctx, env, 'a-after-engine-restart', ASSESS_PROMPT);
    const restarted = second.json?.engine;
    ctx.assert('engine-relaunched-under-the-same-daemon', second.code === 0 && restarted?.state === 'ready'
      && restarted.generation > generationA && restarted.pid !== killed.recorded[0], { code: second.code, engine: restarted, stderr: second.stderr });
    ctx.assert('engine-restart-keeps-the-durable-session', JSON.stringify(bindingOf(second.json)) === JSON.stringify(binding),
      { before: binding, after: bindingOf(second.json) });
    ctx.assert('engine-restart-sent-no-pane-input', noPaneInput(second.json), { actions: second.json?.actions ?? null });

    // 4. Another daemon from another parent cwd resumes the same saved session.
    await ctx.terminate(daemonA, 'restart from another parent cwd');
    const parentB = join(data, 'parents', 'b');
    const daemonB = await startDaemonFrom(ctx, { id: 'daemon-b', env, cwd: parentB });
    await awaitEngine(ctx, env, 'b-ready', ready, 120_000);
    const third = await engineAssess(ctx, env, 'b-after-daemon-restart', ASSESS_PROMPT);
    note(`daemon-b pid=${daemonB.pid} cwd=${parentB} engine pid=${third.json?.engine?.pid}`);
    ctx.assert('resume-from-another-parent-cwd-keeps-identity-and-session', third.code === 0
      && JSON.stringify(parseReply(third.json?.assessment?.reply)) === JSON.stringify(STUB_ASSESSMENT)
      && third.json?.engine?.profileCwd === readyA.engine.profileCwd
      && JSON.stringify(bindingOf(third.json)) === JSON.stringify(binding), { code: third.code, engine: third.json?.engine, stderr: third.stderr });

    // 5. A provider error is a typed fault: no other model, no pane input.
    stub.failNext(400, 'qa stub refused the request');
    const providerFault = await engineAssess(ctx, env, 'b-provider-error', ASSESS_PROMPT);
    stub.succeed();
    const afterFault = await engineStatus(ctx, env, 'b-after-provider-error');
    ctx.assert('provider-error-is-a-typed-fault', providerFault.code !== 0 && providerFault.json?.code === 'MHC_PROVIDER',
      { code: providerFault.code, json: providerFault.json });
    ctx.assert('provider-error-sent-no-pane-input', afterFault.code === 0 && noPaneInput(afterFault.json), { actions: afterFault.json?.actions ?? null });
    ctx.assert('provider-error-used-no-other-model', stub.requests.every((entry) => entry.model === STUB_MODEL), { requests: stub.requests.length });

    // 6. An engine without a required capability is refused: degraded, zero sends.
    await ctx.terminate(daemonB, 'relaunch with a capability-filtered engine');
    const filter = writeCapabilityFilter(ctx, { realMhc: config.mhcBin, capability: DROPPED_CAPABILITY });
    const filteredEnv = await prepareEngineEnv(ctx, { baseUrl: stub.baseUrl, mhcBin: filter.wrapper, label: 'filtered' });
    const providerBeforeDegraded = stub.requests.length;
    const daemonC = await startDaemonFrom(ctx, { id: 'daemon-c', env: filteredEnv, cwd: join(data, 'parents', 'c') });
    const degraded = await awaitEngine(ctx, filteredEnv, 'c-degraded', (json) => json?.engine?.state === 'degraded' && json?.hubOpen === true, 120_000);
    const refused = await engineAssess(ctx, filteredEnv, 'c-refused', ASSESS_PROMPT);
    const frames = readFrames(filter.frames);
    ctx.evidence.writeJson('runtime/filtered-engine-frames.json', frames);
    note(`daemon-c pid=${daemonC.pid} degraded fault=${degraded.engine.faultCode}; filtered engine frames=${frames.map((frame) => frame.type).join(',')}`);
    ctx.assert('missing-capability-is-refused-as-degraded', degraded.engine.faultCode === 'MHC_CAPABILITY_MISSING'
      && JSON.stringify(degraded.engine.fault?.missing) === JSON.stringify([DROPPED_CAPABILITY]), degraded.engine);
    ctx.assert('degraded-engine-refuses-assessment', refused.code !== 0 && refused.json?.code === 'MHC_CAPABILITY_MISSING', { code: refused.code, json: refused.json });
    ctx.assert('degraded-engine-got-zero-sends', frames.length >= 1 && frames.every((frame) => frame.type === 'get_protocol_info')
      && stub.requests.length === providerBeforeDegraded && noPaneInput(degraded), { frames, providerRequests: stub.requests.length - providerBeforeDegraded });
    // Compared with the last binding the real engine left (a provider fault may rebind).
    const lastBinding = bindingOf(afterFault.json);
    ctx.assert('degraded-engine-keeps-the-saved-binding', lastBinding !== null && JSON.stringify(bindingOf(degraded)) === JSON.stringify(lastBinding),
      { before: lastBinding, after: bindingOf(degraded) });
  } finally {
    await stub.close();
    ctx.evidence.writeText('task-6-runtime.log', `${log.join('\n')}\n`);
  }
}

export const RUNTIME_SCENARIO = Object.freeze({
  implemented: true,
  task: '6',
  summary: 'Real mhc: one structured assessment, restart the owned engine, resume the saved session from another cwd; missing capability refused',
  deadlineMs: 12 * 60_000,
  requiredAssertions: [
    'assessment-returns-a-structured-decision',
    'decision-session-has-no-effect-tools',
    'assessment-sent-no-pane-input',
    'engine-identity-checked-before-signal',
    'engine-restart-keeps-the-durable-session',
    'resume-from-another-parent-cwd-keeps-identity-and-session',
    'provider-error-is-a-typed-fault',
    'missing-capability-is-refused-as-degraded',
    'degraded-engine-got-zero-sends',
  ],
  run: runRuntime,
});


