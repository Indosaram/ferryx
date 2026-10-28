// Fixtures and steps of the `runtime` scenario (plan todo 6): the hub's retained mhc supervisor
// engine, driven through the product surface `ferryx-cli supervisor engine status|assess`.
//
// Isolation: every file lives under the run's data root. The isolated HOME carries the hub env
// file (FERRYX_HUB_ENV_FILE), the mhc agent dir (MAHO_CODING_AGENT_DIR, mhc 4a160c1f
// maho-core/src/config.rs:41-43,84-88 + brand.rs:69-71,209-224) with a models.json whose only
// provider is a loopback openai-completions stub served by this runner, and the mhc memory home
// (MAHO_MEMORY_HOME, memory-core/src/identity/layout.rs:11) under the run's memory root. No
// production credentials or state are read.
import { accessSync, constants as fsConstants, existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, realpathSync, statSync, unlinkSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { createServer } from 'node:http';
import { createServer as createTcpServer } from 'node:net';
import { delimiter, isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HarnessError, isInside, listProcesses, POSIX, probeProcess, sameStartStamp } from './harness.mjs';

export const MHC_FILTER = fileURLToPath(new URL('./fixture-mhc-filter.mjs', import.meta.url));
export const STUB_PROVIDER = 'qa-stub';
export const STUB_MODEL = 'qa-supervisor-1';
/// A second model of the stub provider that is the configured fallback of STUB_MODEL. The hub
/// launches mhc with --no-model-fallback, so it must never receive a request.
export const CANARY_MODEL = 'qa-fallback-canary';
/// Both stub models: large enough for mhc's session prompt (a 32000-token window was refused
/// on omarchy as 5185 tokens short of the 37185-token requirement).
const STUB_CONTEXT_WINDOW = 200_000;
/// What the stub answers to every prompt: a valid gap assessment (goals/types.rs `Assessment`).
export const STUB_ASSESSMENT = Object.freeze({ gaps: [], decision: { decision: 'complete' } });
/// The capability the degraded case removes; the hub requires it (mhc_profile REQUIRED_CAPABILITIES).
export const DROPPED_CAPABILITY = 'memory_forget';
/// The hub's memory name of the supervisor (ferryx-hub supervisor.rs MEMORY_NAME).
export const MEMORY_NAME = 'ferryx-supervisor';

// Tools a decision session must never be offered, from the mhc 4a160c1f sources:
// maho-omo-memory write_policy.rs:34 (memory write tools), :42-46 (effect set), :48 (task_/team_
// families), the hub's own tools_exclude (mhc_profile DEFAULT_EFFECT_TOOLS bash/edit/write),
// maho-cli supervisor_work_tools.rs:22-24 (conversation-only work tools), and the other memory
// writer tools (tool_metadata.rs:2-7 MCP spellings, facts_record_tool.rs:19). memory_forget and
// memory_correct are owner extension request names (owner_memory.rs:83-84), not tools; they are
// listed only so a future tool of that name would fail the check.
// create_goal/update_goal (goal mutation, maho-ext-goal extension.rs:24-37) and
// omo_browser_bridge (publishes browser state to the app, maho-omo-browser-bridge index.rs:97-124)
// are effects too; the hub withholds them from decision sessions (mhc_profile
// DECISION_EXCLUDED_TOOLS).
export const FORBIDDEN_DECISION_TOOLS = Object.freeze([
  'memory', 'memory_apply_patch', 'mcp_omo-memory_memory', 'mcp_omo-memory_memory_apply_patch', 'record_fact',
  'memory_forget', 'memory_correct',
  'bash', 'bash_input', 'bash_output', 'bash_resize', 'kill_bash', 'monitor', 'powershell',
  'eval', 'apply_patch', 'lsp_rename', 'lsp_format', 'ast_grep_replace', 'generate_image',
  'computer', 'computer_actions', 'task', 'dag', 'workflow', 'workpool',
  'edit', 'write',
  'supervisor_submit_work', 'supervisor_work_status',
  'create_goal', 'update_goal', 'omo_browser_bridge',
]);
// task_/team_ families (write_policy.rs:48); process* steering (the review's withheld set).
export const FORBIDDEN_DECISION_TOOL_PREFIXES = Object.freeze(['task_', 'team_', 'process']);
/// Read-only tools a decision session may be offered. A tool outside both lists is reported in
/// evidence as unclassified (not a failure: a new read-only builtin must not fail the run).
export const READ_ONLY_DECISION_TOOLS = Object.freeze([
  'read', 'grep', 'glob', 'find', 'ls', 'lsp_diagnostics', 'lsp_goto_definition', 'lsp_find_references', 'lsp_symbols',
  'lsp_hover', 'lsp_prepare_rename', 'ast_grep_search', 'web_search', 'webfetch', 'web_fetch', 'tool_search', 'x_search',
  'todo', 'todowrite', 'ask_user_question', 'session_entries', 'search', 'scan', 'look_at', 'read_video',
  'mcp_list_resources', 'mcp_read_resource', 'get_goal',
]);
export const forbiddenDecisionTool = (name) => FORBIDDEN_DECISION_TOOLS.includes(name)
  || FORBIDDEN_DECISION_TOOL_PREFIXES.some((prefix) => String(name).startsWith(prefix));
/// Positive control: tools mhc always offers a decision session. With no `defaultTools` setting a
/// shared-host session selects read/bash/edit/write/grep (maho-core sdk.rs:229-232; the host
/// passes no tools list, maho-cli host_runtime.rs:767-783), and the decision exclusions remove
/// only bash/edit/write, so read and grep remain.
export const ALWAYS_OFFERED_DECISION_TOOLS = Object.freeze(['read', 'grep']);

/// Tool names of one chat request: `tools[].function.name` or `tools[].custom.name`
/// (maho-ai api/openai_completions.rs:1103-1140), plus deferred tool lists carried on system
/// messages (`messages[].tools`, :1078-1080). An entry without a name is kept as null so the
/// scenario can fail on a shape it could not parse.
/// What a chat request asked, for attributing it: the last user message's text (start and end,
/// bounded) and the system prompt's first line. Fixture data only (the loopback stub's own
/// requests); nothing here reaches a real provider.
function promptOf(json) {
  const messages = Array.isArray(json?.messages) ? json.messages : [];
  const text = (message) => (typeof message?.content === 'string' ? message.content
    : Array.isArray(message?.content) ? message.content.map((part) => part?.text ?? '').join('') : '');
  const user = [...messages].reverse().find((message) => message?.role === 'user');
  const userText = text(user);
  const system = text(messages.find((message) => message?.role === 'system' || message?.role === 'developer'));
  return {
    messages: messages.length,
    user: userText.length > 600 ? `${userText.slice(0, 400)} ... ${userText.slice(-200)}` : userText,
    systemFirstLine: system.split('\n', 1)[0].slice(0, 200),
  };
}

function toolNamesOf(json) {
  const nameOf = (tool) => {
    const name = tool?.function?.name ?? tool?.custom?.name ?? tool?.name;
    return typeof name === 'string' ? name : null;
  };
  const lists = [json?.tools, ...(Array.isArray(json?.messages) ? json.messages.map((message) => message?.tools) : [])];
  return lists.filter(Array.isArray).flatMap((list) => list.map(nameOf));
}

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
// with an HTTP error while `failWith` is set. The message is STUB_ASSESSMENT unless `answer`
// (called with the parsed request body) returns another object for that request.

export async function startProviderStub(ctx, { answer = null } = {}) {
  const requests = [];
  // Raw request bodies, index-aligned with `requests`, kept in memory only (never written to
  // evidence): a scenario asks which requests carried a marker anywhere in what the model saw.
  const bodies = [];
  // `arrivals` counts requests when they start (before their body is read and their reply
  // streams); each recorded entry carries its arrival number, so a caller can count only the
  // requests that arrived after a point it observed, whatever order they finish in.
  const state = { failWith: null, arrivals: 0 };
  const server = createServer((request, response) => {
    state.arrivals += 1;
    const arrival = state.arrivals;
    const arrivedAt = new Date().toISOString();
    let body = '';
    request.setEncoding('utf8');
    request.on('data', (chunk) => { body += chunk; });
    request.on('end', () => {
      let json = null;
      try { json = JSON.parse(body); } catch { /* recorded raw */ }
      const tools = toolNamesOf(json);
      const entry = { at: new Date().toISOString(), arrival, arrivedAt, method: request.method, path: request.url, model: json?.model ?? null, stream: json?.stream ?? null, tools,
        prompt: promptOf(json), failWith: state.failWith };
      requests.push(entry);
      bodies.push(body);
      ctx.evidence.appendJsonl('runtime/provider-requests.jsonl', entry);
      ctx.journal.push({ source: 'provider-stub', kind: 'request', ...entry });
      if (state.failWith) {
        const { status, message, code = null, headers = {} } = state.failWith;
        response.writeHead(status, { 'content-type': 'application/json', ...headers });
        response.end(JSON.stringify({ error: { message, type: 'qa_stub_error', ...(code ? { code } : {}) } }));
        return;
      }
      if (request.method === 'GET' && request.url?.replace(/\?.*$/, '').endsWith('/models')) {
        response.writeHead(200, { 'content-type': 'application/json' });
        response.end(JSON.stringify({ object: 'list', data: [STUB_MODEL, CANARY_MODEL].map((id) => ({ id, object: 'model', owned_by: 'qa' })) }));
        return;
      }
      if (!request.url?.endsWith('/chat/completions')) {
        response.writeHead(404, { 'content-type': 'application/json' });
        response.end(JSON.stringify({ error: { message: `no route ${request.url}` } }));
        return;
      }
      const id = `qa-${requests.length}`;
      const model = json?.model ?? STUB_MODEL;
      const reply = answer?.(json) ?? STUB_ASSESSMENT;
      const chunk = (delta, finish = null) => `data: ${JSON.stringify({ id, object: 'chat.completion.chunk', created: 0, model,
        choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`;
      response.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
      response.write(chunk({ role: 'assistant', content: JSON.stringify(reply) }));
      response.write(chunk({}, 'stop'));
      response.write(`data: ${JSON.stringify({ id, object: 'chat.completion.chunk', created: 0, model, choices: [],
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
    arrivals: () => state.arrivals,
    /// Arrival numbers of the recorded requests from index `from` whose raw body contains every
    /// one of `needles` (a string or an array of strings).
    containing: (needles, from = 0) => {
      const all = Array.isArray(needles) ? needles : [needles];
      return requests.slice(from).filter((_, index) => all.every((needle) => bodies[from + index].includes(needle))).map((entry) => entry.arrival);
    },
    // Every request fails with `status` until succeed(). `code` is the provider error code in
    // the body (e.g. insufficient_quota) and `headers` extra response headers (e.g.
    // x-should-retry: false, which mhc's transport retry honours before any backoff).
    failNext(status, message, { code = null, headers = {} } = {}) { state.failWith = { status, message, code, headers }; },
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
  const models = { providers: { [STUB_PROVIDER]: { api: 'openai-completions', baseUrl, apiKey: 'qa-loopback-fixture',
    models: [STUB_MODEL, CANARY_MODEL].map((id) => ({ id, contextWindow: STUB_CONTEXT_WINDOW, maxTokens: 8192 })) } } };
  writeFileSync(join(agentDir, 'models.json'), `${JSON.stringify(models, null, 2)}\n`, { mode: 0o600 });
  // Global settings (agent dir settings.json, maho-core settings_manager.rs:280-305): model
  // fallback on, with the canary as STUB_MODEL's fallback chain (retry_fallback/controller.rs:
  // 84-100). Only the hub's --no-model-fallback flag (agent_session.rs:1669-1671) keeps a
  // failed turn off the canary, so a canary request proves a fallback happened.
  const settings = { retry: { modelFallback: true, fallbackChains: { [`${STUB_PROVIDER}/${STUB_MODEL}`]: [`${STUB_PROVIDER}/${CANARY_MODEL}`] } } };
  writeFileSync(join(agentDir, 'settings.json'), `${JSON.stringify(settings, null, 2)}\n`, { mode: 0o600 });
  // Loopback only: mhc's built-in MCP servers (maho-omo-builtin-mcps index.rs:19-23,57-62:
  // context7 -> mcp.context7.com, grep_app -> mcp.grep.app) have no env switch; a global
  // mcp.json entry with enabled:false wins over an extension declaration (maho-ext-mcp
  // config.rs:175 reads <agentDir>/mcp.json, :217-218 marks it Disabled, :232-241 skips the
  // extension's server, :229-231 spawns Enabled servers only).
  const mcp = { mcpServers: { context7: { enabled: false }, grep_app: { enabled: false } } };
  writeFileSync(join(agentDir, 'mcp.json'), `${JSON.stringify(mcp, null, 2)}\n`, { mode: 0o600 });
  const envFile = join(data, `hub-${label}.env`);
  writeFileSync(envFile, [
    'FERRYX_HUB_MHC_SUPERVISOR=1',
    `FERRYX_HUB_MHC_BIN=${mhcBin}`,
    `FERRYX_HUB_MHC_PROVIDER=${STUB_PROVIDER}`,
    `FERRYX_HUB_MHC_MODEL=${STUB_MODEL}`,
    '',
  ].join('\n'), { mode: 0o600 });
  for (const path of [agentDir, join(agentDir, 'mcp.json'), envFile]) {
    if (!within(path, data)) throw new HarnessError('HARNESS_ERROR', `${path} escapes the run data root ${data}`);
  }
  // The hub HTTP listener gets a run-local port instead of the production default :9119.
  const trackerAddr = `127.0.0.1:${await freeLoopbackPort()}`;
  // The daemon's environment reaches the hub's mhc engine and its session workers (both inherit
  // it: ferryx-hub child_env.rs:60-65, mhc host_runtime.rs:683-693).
  // - PostHog telemetry (us.i.posthog.com): DO_NOT_TRACK=1 and OMO_DISABLE_POSTHOG=1
  //   (telemetry-core env.rs:6,28-47, gate of posthog_client.rs:53-58 used by
  //   maho-omo-telemetry omo_native_session.rs:27 and record_daily_active.rs:40).
  // - Model catalog refresh, version and package checks: MAHO_OFFLINE=1 (maho-core
  //   brand.rs:173,209-236; maho-cli runtime.rs:340, version_check.rs:37; maho-core
  //   model_runtime.rs:382, package_manager.rs:698) and MAHO_SKIP_VERSION_CHECK=1
  //   (version_check.rs:57).
  // - The hub's inbox watchers: OMON_INBOX_ENABLED=0 (ferryx-hub inbox_watch.rs inbox_enabled,
  //   src-tauri hub_runtime.rs:1725). Their kakao source defaults to a non-loopback
  //   OMON_PIRATETALK_URL (inbox_watch.rs fetch_kakao), and their triage is an engine
  //   decision turn (judge.rs inbox_class) no scenario drives.
  const offline = { DO_NOT_TRACK: '1', OMO_DISABLE_POSTHOG: '1', MAHO_OFFLINE: '1', MAHO_SKIP_VERSION_CHECK: '1', OMON_INBOX_ENABLED: '0' };
  ctx.evidence.writeJson(`runtime/engine-env-${label}.json`, { envFile, agentDir, models, settings, mcp, offline, mhcBin, memoryHome: join(memory, 'mhc'), trackerAddr });
  return ctx.productEnv({
    FERRYX_HUB_ENV_FILE: envFile,
    FERRYX_HUB_TRACKER_ADDR: trackerAddr,
    MAHO_CODING_AGENT_DIR: agentDir,
    MAHO_MEMORY_HOME: join(memory, 'mhc'),
    ...offline,
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
export async function engineStatus(ctx, env, label, deadlineMs = 120_000) {
  // A status waits for the engine state lock, which a slow launch holds through its handshake:
  // a timed-out call is retried within the deadline.
  const deadline = performance.now() + deadlineMs;
  for (let attempt = 1; ; attempt += 1) {
    const result = await ctx.cli(['supervisor', 'engine', 'status', '--json'], { label: `engine-status-${label}-${attempt}`, env, deadlineMs: 30_000 });
    if (!result.timedOut || performance.now() >= deadline) {
      ctx.evidence.writeJson(`runtime/status-${label}.json`, { attempt, code: result.code, timedOut: result.timedOut, json: result.json, stderr: result.stderr });
      return result;
    }
    if (ctx.signal.aborted) throw ctx.signal.reason;
  }
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

const ENGINE_ARGS = ['--mode', 'rpc', '--multi-session', '--no-model-fallback', '--ttsr-rules-disabled=repetitive-turns', '--omo-senpi-onboarding-disabled'];

function argvOf(probe) {
  if (Array.isArray(probe.argv)) return probe.argv;
  if (typeof probe.command === 'string') return probe.command.split(/\s+/);
  return null;
}

/**
 * Kills the engine the hub recorded in `<data>/mhc-supervisor.pid`, and nothing else. The pid
 * must match the engine status the daemon reported, the record must name the daemon as its
 * owner, the process's parent must be the daemon, the process argv must be `<mhc> --mode rpc
 * --multi-session --no-model-fallback` with the configured binary, and the start stamp read
 * before signalling must equal the one re-read just before the signal. Anything else is
 * refused with a named HarnessError.
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
  // The darwin probe carries no ppid; the process table does.
  const ppid = Number.isInteger(first.ppid) ? first.ppid : ((await listProcesses()) ?? []).find((row) => row.pid === pid)?.ppid ?? null;
  const identity = {
    pidFile, recorded, reportedPid, daemonPid, ppid, state: first.state, startStamp: first.startStamp ?? null, argv,
    ownerMatches: owner === daemonPid, reportedMatches: pid === reportedPid, parentMatches: ppid === daemonPid,
    binaryMatches: Boolean(argv && (argv[0] === mhcBin || argv.slice(0, 3).some((part) => part === mhcBin))),
    argsMatch: Boolean(argv && ENGINE_ARGS.every((arg) => argv.includes(arg))),
    noModelFallback: Boolean(argv?.includes('--no-model-fallback')),
  };
  ctx.evidence.writeJson('runtime/engine-identity.json', identity);
  if (first.state !== 'alive' || !identity.ownerMatches || !identity.reportedMatches || !identity.parentMatches || !identity.argsMatch
    || !identity.binaryMatches || !identity.startStamp) {
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
const RUNTIME_PROJECT = 'qa-runtime';
const RUNTIME_HOST = 'local';
/// mhc 4a160c1f custom session entries (maho-omo-memory binding.rs:4, write_policy.rs:124-128;
/// maho-cli session_policy.rs:17,125-135).
const MEMORY_BINDING_CUSTOM_TYPE = 'senpi-memory.session-binding';
const SESSION_POLICY_CUSTOM_TYPE = 'maho.session-policy';
/// TTSR rule activation entry (maho-ext-ttsr index.rs:30) and the rule the hub disables at launch.
const RULE_ACTIVATION_CUSTOM_TYPE = 'rule-activation';
const REPETITION_GUARD_RULE = 'repetitive-turns';
/// Unit tests (ferryx-hub meta/mhc_supervisor/tests.rs) that cover what this scenario does not drive.
const UNIT_COVERED = Object.freeze({
  command: 'cargo test --manifest-path crates/ferryx-hub/Cargo.toml mhc_supervisor',
  tests: {
    occupied_slots_do_not_stall_other_slots_or_owner_cancellation: 'an occupied decision slot does not stall other slots or owner cancellation (todo 6 acceptance)',
    decision_slots_have_no_effect_tools_and_ignore_injected_policy: 'decision sessions open with the effect-tool exclusion and memory_write deny whatever the prompt asks',
    a_provider_error_is_a_typed_fault_with_no_fallback: 'a provider error is MHC_PROVIDER and no other model answers',
  },
});

const bindingOf = (status) => status?.engine?.sessions?.['decision-0'] ?? null;
const parseReply = (text) => { try { return JSON.parse(String(text ?? '').trim()); } catch { return null; } };
const noPaneInput = (json) => Array.isArray(json?.actions) && json.actions.length === 0;
const replied = (result) => result?.code === 0 && result?.json?.ok === true && typeof result?.json?.engine === 'object' && result.json.engine !== null;
const replyDetail = (result) => ({ code: result?.code ?? null, timedOut: result?.timedOut ?? null, json: result?.json ?? null, stderr: result?.stderr ?? null });
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

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

/** Read-only row counts of the isolated hub database (bun:sqlite, readonly connection per read). */
async function openHubCounter(dbPath) {
  let Database;
  try { ({ Database } = await import('bun:sqlite')); } catch {
    throw new HarnessError('SQLITE_UNAVAILABLE', 'the runtime scenario reads the isolated hub database with bun:sqlite; run the runner with bun');
  }
  const queries = {
    goals: 'SELECT COUNT(*) AS n FROM supervisor_goals',
    requests: 'SELECT COUNT(*) AS n FROM supervisor_requests',
    dispatches: 'SELECT COUNT(*) AS n FROM supervisor_dispatches',
    readyIntents: "SELECT COUNT(*) AS n FROM supervisor_dispatches WHERE state = 'ready'",
    wakeReceipts: 'SELECT COUNT(*) AS n FROM supervisor_wake_receipts',
    // judge.rs SUPERVISOR_STRUCTURED_KIND / Purpose::GapAssess.
    structuredJudgeCalls: "SELECT COUNT(*) AS n FROM judge_calls WHERE kind = 'mhc_structured' AND purpose = 'gap_assess'",
  };
  return () => {
    try {
      const db = new Database(dbPath, { readonly: true });
      try { return Object.fromEntries(Object.entries(queries).map(([key, sql]) => [key, Number(db.query(sql).get()?.n ?? -1)])); } finally { db.close(); }
    } catch (error) {
      return { error: String(error?.message ?? error) };
    }
  };
}

/**
 * The engine's own record of a decision session: the session file mhc writes (read-only, and
 * only inside the run data root) carries the memory binding (identity + memoryWrite deny) and
 * the session policy entries.
 */
function sessionRecordOf(ctx, binding, profileCwd) {
  const file = binding?.sessionFile;
  if (typeof file !== 'string' || !file) return { error: 'no session file in the binding' };
  const path = isAbsolute(file) ? file : join(profileCwd ?? '', file);
  if (!within(path, dataOf(ctx))) return { path, error: 'session file is outside the run data root; not read' };
  let text;
  try { text = readFileSync(path, 'utf8'); } catch (error) { return { path, error: String(error?.message ?? error) }; }
  const entries = text.split('\n').filter(Boolean).flatMap((line) => { try { return [JSON.parse(line)]; } catch { return []; } });
  const custom = (type) => entries.filter((entry) => entry?.type === 'custom' && entry?.customType === type).map((entry) => entry.data ?? {});
  const bindings = custom(MEMORY_BINDING_CUSTOM_TYPE);
  const policies = custom(SESSION_POLICY_CUSTOM_TYPE);
  return {
    path,
    memoryIdentity: bindings.at(-1)?.identity ?? null,
    memoryRepoPathHash: bindings.at(-1)?.repoPathHash ?? null,
    bindingMemoryWrite: bindings.at(-1)?.memoryWrite ?? null,
    policyMemoryWrite: policies.some((policy) => policy?.memoryWrite === 'deny') ? 'deny' : null,
    policyToolsExclude: [...new Set(policies.flatMap((policy) => (Array.isArray(policy?.toolsExclude) ? policy.toolsExclude : [])))],
  };
}

/**
 * Whether the binding mhc recorded points at the identity's memory repo under the run's
 * MAHO_MEMORY_HOME. The binding's `repoPathHash` is sha256 of that repo path (maho-omo-memory
 * binding.rs:13-15; repo = <home>/agents/<id>/repo, memory-core layout.rs:69-73). The directory
 * itself is not observed: a memory-write-denied session never creates it (only the memory write
 * path does, engine_session.rs:15-18), so its absence is by design.
 */
function memoryRepoUnderRunHome(ctx, session) {
  const identity = session?.memoryIdentity;
  if (typeof identity !== 'string' || typeof session?.memoryRepoPathHash !== 'string') return false;
  const repo = join(ctx.paths.memory, 'mhc', 'agents', identity, 'repo');
  return createHash('sha256').update(repo).digest('hex') === session.memoryRepoPathHash;
}

/**
 * Supervisor identity as observed from one engine reply. `supervisors` are the hub's rows;
 * `hubProfileMemoryAgent` is the hub's own profile config (`.omo/omo.jsonc`, written by the hub,
 * not an engine observation); the engine observations are `sessionMemoryIdentity` (the memory
 * binding mhc wrote into the decision session) and `memoryRepoUnderRunHome` (that binding's repo
 * path hash names the identity's repo under the run's MAHO_MEMORY_HOME).
 */
function identityOf(ctx, json, session) {
  return {
    supervisors: Array.isArray(json?.supervisorIdentities)
      ? json.supervisorIdentities.map(({ supervisor_id: id, memory_name: memoryName, owner_principal: owner }) => ({ id, memoryName, owner }))
      : null,
    hubProfileMemoryAgent: json?.engine?.memoryAgent ?? null,
    sessionMemoryIdentity: session?.memoryIdentity ?? null,
    memoryRepoUnderRunHome: memoryRepoUnderRunHome(ctx, session),
  };
}

async function argvOfPid(pid) {
  if (!Number.isInteger(pid)) return null;
  const probe = await probeProcess(pid);
  return probe.state === 'alive' ? argvOf(probe) : null;
}

async function runRuntime(ctx) {
  if (!POSIX) {
    throw new HarnessError('FIXTURE_UNSUPPORTED_PLATFORM', 'the runtime scenario waits on the POSIX daemon socket, restarts the engine by POSIX pid identity and uses a #! capability filter; it does not run on win32');
  }
  const { config } = ctx;
  const data = dataOf(ctx);
  const stateDir = engineStateDir(ctx);
  const dbPath = join(data, 'hub', 'hub.sqlite');
  const log = [];
  const note = (line) => { log.push(`${new Date().toISOString()} ${line}`); ctx.log(line); };
  ctx.evidence.writeJson('runtime/unit-test-coverage.json', UNIT_COVERED);
  // Awaits an engine state; a failed or missing state is a recorded failed assertion, not a crash.
  const expectEngine = async (assertion, env, label, predicate, deadlineMs) => {
    try {
      return await awaitEngine(ctx, env, label, predicate, deadlineMs);
    } catch (error) {
      if (ctx.signal.aborted || !(error instanceof HarnessError) || !['DEADLINE_EXCEEDED', 'ENGINE_STATUS_FAILED'].includes(error.code)) throw error;
      ctx.assert(assertion, false, { label, error: error.message, detail: error.detail ?? null });
      return null;
    }
  };
  // Every reply is checked before it is read: a refused reply is a failed assertion with the reply.
  const expectReply = (assertion, result) => {
    if (replied(result)) return true;
    ctx.assert(assertion, false, replyDetail(result));
    return false;
  };
  const stub = await startProviderStub(ctx);
  let live = null;
  // Observed by the real-engine legs, compared by the degraded leg.
  let lastBinding = null;
  const identities = {};
  const stop = async (reason) => { if (live) { const handle = live; live = null; await ctx.terminate(handle, reason); } };
  try {
    const env = await prepareEngineEnv(ctx, { baseUrl: stub.baseUrl, mhcBin: config.mhcBin, label: 'real' });

    // 0. The owner's supervisor identity, through the product setup CLI (a fixture repository).
    const git = onPath('git');
    if (!git) throw new HarnessError('PREREQUISITE_MISSING', 'git is not on PATH; the runtime scenario provisions its supervisor identity against a fixture git repository');
    const repo = join(data, 'runtime-repo');
    mkdirSync(repo, { recursive: true });
    const gitInit = await ctx.run('git-init-runtime', git, ['-c', 'init.defaultBranch=main', 'init', '-q', repo], { deadlineMs: 30_000 });
    if (gitInit.code !== 0) throw new HarnessError('FIXTURE_FAILED', `git init exited ${gitInit.code}: ${gitInit.stderr}`);
    const setup = await ctx.cli(['supervisor', 'setup', '--owner', config.ownerPrincipal, '--project', RUNTIME_PROJECT, '--repo-identity', `qa:${RUNTIME_PROJECT}`,
      '--host', RUNTIME_HOST, '--git-common-dir', realpathSync(join(repo, '.git')), '--json'], { label: 'supervisor-setup-runtime', env });
    const supervisorId = setup.code === 0 && setup.json?.ok === true ? setup.json.supervisorId ?? null : null;
    ctx.assert('supervisor-identity-provisioned', typeof supervisorId === 'string', replyDetail(setup));
    const counter = await openHubCounter(dbPath);

    // 1. Real mhc under daemon A, started from parent cwd A.
    const ready = (json) => json?.engine?.state === 'ready' && json?.hubOpen === true;
    const realLegs = async () => {
      const parentA = join(data, 'parents', 'a');
      const daemonA = await startDaemonFrom(ctx, { id: 'daemon-a', env, cwd: parentA });
      live = daemonA;
      const readyA = await expectEngine('engine-ready-with-required-capabilities', env, 'a-ready', ready, 120_000);
      if (!readyA) return;
      note(`daemon-a pid=${daemonA.pid} engine pid=${readyA.engine.pid} generation=${readyA.engine.generation}`);
      ctx.assert('engine-ready-with-required-capabilities', readyA.engine.faultCode === null && Number.isInteger(readyA.engine.pid)
        && ['multi_session', 'memory_write_deny', 'memory_write_deny_effects', 'tools_exclude', 'memory_forget', 'supervisor_work_tools'].every((name) => readyA.engine.capabilities?.includes(name)),
        readyA.engine);
      ctx.assert('profile-cwd-is-under-the-isolated-data-root', within(readyA.engine.profileCwd, data)
        && readyA.engine.profileCwd === join(stateDir, 'mhc-supervisor'), { profileCwd: readyA.engine.profileCwd, stateDir });

      // 2. One structured assessment through the product CLI, with read-only row counts around it.
      const rowsBefore = counter();
      const providerBefore = stub.requests.length;
      const first = await engineAssess(ctx, env, 'a-first', ASSESS_PROMPT);
      const rowsAfter = counter();
      const turnRequests = stub.requests.slice(providerBefore).filter((entry) => entry.path?.endsWith('/chat/completions'));
      ctx.evidence.writeJson('runtime/assess-rows.json', { before: rowsBefore, after: rowsAfter });
      // judge.rs structured(): every assess reaching the judge records exactly one row (ok or fault).
      const unchanged = ['goals', 'requests', 'dispatches', 'readyIntents', 'wakeReceipts'];
      ctx.assert('assess-writes-no-goals-dispatches-or-receipts', !rowsBefore.error && !rowsAfter.error
        && unchanged.every((key) => rowsAfter[key] === rowsBefore[key])
        && rowsAfter.structuredJudgeCalls === rowsBefore.structuredJudgeCalls + 1, { before: rowsBefore, after: rowsAfter });
      if (!expectReply('assessment-returns-a-structured-decision', first)) return;
      ctx.assert('assessment-returns-a-structured-decision', same(parseReply(first.json.assessment?.reply), STUB_ASSESSMENT), replyDetail(first));
      ctx.assert('assessment-reached-only-the-loopback-stub-model', turnRequests.length >= 1 && turnRequests.every((entry) => entry.model === STUB_MODEL), { turnRequests });

      // Effect tools, measured from what the engine offered the model and what it recorded.
      const binding = bindingOf(first.json);
      lastBinding = binding;
      const session = sessionRecordOf(ctx, binding, first.json.engine.profileCwd);
      const parsedNames = turnRequests.flatMap((entry) => entry.tools);
      const unparsed = parsedNames.filter((name) => typeof name !== 'string' || name === '').length;
      const offered = [...new Set(parsedNames.filter((name) => typeof name === 'string' && name !== ''))].sort();
      const forbidden = offered.filter(forbiddenDecisionTool);
      const unclassified = offered.filter((tool) => !forbiddenDecisionTool(tool) && !READ_ONLY_DECISION_TOOLS.includes(tool));
      const missingAlwaysOffered = ALWAYS_OFFERED_DECISION_TOOLS.filter((tool) => !offered.includes(tool));
      const perRequest = turnRequests.map((entry) => ({ at: entry.at, model: entry.model, tools: entry.tools }));
      const decisionTools = { offered, forbidden, unclassified, unparsed, missingAlwaysOffered, perRequest, session,
        hubDecisionContext: first.json.engine.decisionContext ?? null };
      ctx.evidence.writeJson('runtime/decision-tools.json', decisionTools);
      // Positive control: every decision-turn request carries a tool list whose every entry
      // parsed to a name, and the always-offered read-only tools are among them; only then does
      // "no forbidden tool" mean something.
      ctx.assert('decision-session-has-no-effect-tools', turnRequests.length >= 1
        && turnRequests.every((entry) => entry.tools.length > 0 && entry.tools.every((name) => typeof name === 'string' && name !== ''))
        && offered.length > 0 && missingAlwaysOffered.length === 0
        && forbidden.length === 0 && session.bindingMemoryWrite === 'deny', decisionTools);
      ctx.assert('assessment-sent-no-pane-input', noPaneInput(first.json), { actions: first.json.actions ?? null });
      ctx.assert('decision-session-binding-persisted', typeof binding?.durableSessionId === 'string' && binding.role === 'decision'
        && typeof binding.sessionFile === 'string', { binding });
      const identityA = identityOf(ctx, first.json, session);
      identities.a = identityA;
      ctx.evidence.writeJson('runtime/identity-a.json', identityA);
      // mhc names the identity `<slug>-<sha256-8>` of the configured agent (memory-core
      // identity/resolve.rs:112-114) and binds the session to <MAHO_MEMORY_HOME>/agents/<id>/repo
      // (layout.rs:69-73), here under the run's memory home.
      ctx.assert('supervisor-identity-observed', identityA.hubProfileMemoryAgent === MEMORY_NAME && typeof supervisorId === 'string'
        && same(identityA.supervisors?.map((row) => [row.id, row.memoryName]), [[supervisorId, MEMORY_NAME]])
        && typeof identityA.sessionMemoryIdentity === 'string' && /^ferryx-supervisor-[0-9a-f]{8}$/.test(identityA.sessionMemoryIdentity)
        && identityA.memoryRepoUnderRunHome === true, identityA);
      note(`assessment ok; decision-0 durable=${binding?.durableSessionId} file=${binding?.sessionFile} identity=${identityA.sessionMemoryIdentity}`);

      // 3. Restart only the owned engine: kill the recorded pid after the identity checks; the
      //    daemon (and its hub) stays up and relaunches the engine on its next use.
      const generationA = first.json.engine.generation;
      let killed;
      try {
        killed = await killOwnedEngine(ctx, { stateDir, reportedPid: first.json.engine.pid, daemonPid: daemonA.pid, mhcBin: config.mhcBin });
      } catch (error) {
        if (!(error instanceof HarnessError) || error.code !== 'ENGINE_IDENTITY') throw error;
        ctx.assert('engine-identity-checked-before-signal', false, { error: error.message, detail: error.detail ?? null });
        return;
      }
      note(`killed owned engine pid=${killed.recorded[0]} start=${killed.startStamp}`);
      ctx.assert('engine-identity-checked-before-signal', killed.ownerMatches && killed.reportedMatches && killed.parentMatches && killed.argsMatch && killed.binaryMatches, killed);
      // An engine that ran under a minute backs off 5s before its relaunch; wait it out (or
      // see the hub's own supervision relaunch it first).
      const exited = await expectEngine('engine-relaunched-under-the-same-daemon', env, 'a-exited', (json) => (json?.engine?.state !== 'ready' && (json?.engine?.retryInMs ?? 0) === 0)
        || (json?.engine?.state === 'ready' && json.engine.generation > generationA), 60_000);
      if (!exited) return;
      const second = await engineAssess(ctx, env, 'a-after-engine-restart', ASSESS_PROMPT);
      if (!expectReply('engine-relaunched-under-the-same-daemon', second)) return;
      const restarted = second.json.engine;
      lastBinding = bindingOf(second.json);
      ctx.assert('engine-relaunched-under-the-same-daemon', restarted.state === 'ready' && restarted.generation > generationA && restarted.pid !== killed.recorded[0],
        replyDetail(second));
      ctx.assert('engine-restart-keeps-the-durable-session', same(bindingOf(second.json), binding), { before: binding, after: bindingOf(second.json) });
      ctx.assert('engine-restart-sent-no-pane-input', noPaneInput(second.json), { actions: second.json.actions ?? null });
      const identityRestart = identityOf(ctx, second.json, sessionRecordOf(ctx, bindingOf(second.json), restarted.profileCwd));
      identities.aRestarted = identityRestart;
      ctx.evidence.writeJson('runtime/identity-a-restarted.json', identityRestart);

      // 4. Another daemon from another parent cwd resumes the same saved session.
      await stop('restart from another parent cwd');
      const parentB = join(data, 'parents', 'b');
      const daemonB = await startDaemonFrom(ctx, { id: 'daemon-b', env, cwd: parentB });
      live = daemonB;
      if (!await expectEngine('resume-from-another-parent-cwd-keeps-identity-and-session', env, 'b-ready', ready, 120_000)) return;
      const third = await engineAssess(ctx, env, 'b-after-daemon-restart', ASSESS_PROMPT);
      if (!expectReply('resume-from-another-parent-cwd-keeps-identity-and-session', third)) return;
      lastBinding = bindingOf(third.json);
      note(`daemon-b pid=${daemonB.pid} cwd=${parentB} engine pid=${third.json.engine.pid}`);
      const identityB = identityOf(ctx, third.json, sessionRecordOf(ctx, bindingOf(third.json), third.json.engine.profileCwd));
      identities.b = identityB;
      ctx.evidence.writeJson('runtime/identity-b.json', identityB);
      ctx.assert('resume-from-another-parent-cwd-keeps-identity-and-session', same(parseReply(third.json.assessment?.reply), STUB_ASSESSMENT)
        && third.json.engine.profileCwd === readyA.engine.profileCwd
        && same(bindingOf(third.json), binding) && same(identityB, identityA), { engine: third.json.engine, identityA, identityB });

      // 5. A provider error is a typed fault from the 400, with no fallback model.
      const engineArgv = await argvOfPid(third.json.engine.pid);
      const faultFrom = stub.requests.length;
      stub.failNext(400, 'qa stub refused the request');
      const providerFault = await engineAssess(ctx, env, 'b-provider-error', ASSESS_PROMPT);
      stub.succeed();
      const failedRequests = stub.requests.slice(faultFrom).filter((entry) => entry.failWith?.status === 400);
      const afterFault = await engineStatus(ctx, env, 'b-after-provider-error');
      const faultMessage = String(providerFault.json?.message ?? '');
      ctx.evidence.writeJson('runtime/provider-fault.json', { reply: replyDetail(providerFault), failedRequests, engineArgv });
      ctx.assert('provider-error-reached-the-stub', failedRequests.length >= 1 && failedRequests.every((entry) => entry.model === STUB_MODEL), { failedRequests });
      ctx.assert('provider-error-is-a-typed-fault', providerFault.code !== 0 && providerFault.json?.code === 'MHC_PROVIDER'
        && !/fell back/i.test(faultMessage), replyDetail(providerFault));
      ctx.assert('engine-runs-without-model-fallback', Boolean(engineArgv?.includes('--no-model-fallback')) && killed.noModelFallback, { engineArgv, killedArgv: killed.argv });
      ctx.assert('provider-error-sent-no-pane-input', replied(afterFault) && noPaneInput(afterFault.json), replyDetail(afterFault));
      if (replied(afterFault)) lastBinding = bindingOf(afterFault.json);
    };
    await realLegs();
    ctx.assert('no-fallback-canary-got-zero-requests', stub.requests.every((entry) => entry.model !== CANARY_MODEL),
      { canaryRequests: stub.requests.filter((entry) => entry.model === CANARY_MODEL) });
    // The hub launches mhc with --ttsr-rules-disabled=repetitive-turns; an mhc that does not
    // know the flag drops it silently in multi-session RPC mode (host_runtime.rs:785-786), so the
    // engine's own session files must show that the guard never fired. Every session file under
    // the run's agent dir is scanned, and the decision session the hub bound must be among them.
    const sessionsDir = join(data, 'home', '.omo', 'agent', 'sessions');
    let sessionFiles = [];
    try {
      sessionFiles = readdirSync(sessionsDir, { recursive: true }).map(String).filter((name) => name.endsWith('.jsonl')).map((name) => join(sessionsDir, name)).sort();
    } catch { /* none written */ }
    const activations = sessionFiles.flatMap((path) => readFileSync(path, 'utf8').split('\n').filter(Boolean).flatMap((line) => {
      try { return [JSON.parse(line)]; } catch { return []; }
    }).filter((entry) => entry?.type === 'custom' && entry?.customType === RULE_ACTIVATION_CUSTOM_TYPE
      && entry?.data?.owner === REPETITION_GUARD_RULE).map((entry) => ({ path, at: entry.timestamp ?? null, data: entry.data })));
    const boundFile = typeof lastBinding?.sessionFile === 'string' ? lastBinding.sessionFile : null;
    ctx.assert('repetition-guard-never-fired', boundFile !== null && sessionFiles.includes(boundFile) && activations.length === 0,
      { sessionsDir, sessionFiles, boundFile, activations });

    // 6. An engine without a required capability is refused: degraded, zero sends.
    await stop('relaunch with a capability-filtered engine');
    const filter = writeCapabilityFilter(ctx, { realMhc: config.mhcBin, capability: DROPPED_CAPABILITY });
    const filteredEnv = await prepareEngineEnv(ctx, { baseUrl: stub.baseUrl, mhcBin: filter.wrapper, label: 'filtered' });
    const providerBeforeDegraded = stub.requests.length;
    const daemonC = await startDaemonFrom(ctx, { id: 'daemon-c', env: filteredEnv, cwd: join(data, 'parents', 'c') });
    live = daemonC;
    const degraded = await expectEngine('missing-capability-is-refused-as-degraded', filteredEnv, 'c-degraded',
      (json) => json?.engine?.state === 'degraded' && json?.hubOpen === true, 120_000);
    if (degraded) {
      const refused = await engineAssess(ctx, filteredEnv, 'c-refused', ASSESS_PROMPT);
      const frames = readFrames(filter.frames);
      ctx.evidence.writeJson('runtime/filtered-engine-frames.json', frames);
      note(`daemon-c pid=${daemonC.pid} degraded fault=${degraded.engine.faultCode}; filtered engine frames=${frames.map((frame) => frame.type ?? 'unparsed').join(',')}`);
      ctx.assert('missing-capability-is-refused-as-degraded', degraded.engine.faultCode === 'MHC_CAPABILITY_MISSING'
        && same(degraded.engine.fault?.missing, [DROPPED_CAPABILITY]), degraded.engine);
      ctx.assert('degraded-engine-refuses-assessment', refused.code !== 0 && refused.json?.code === 'MHC_CAPABILITY_MISSING', replyDetail(refused));
      ctx.assert('degraded-engine-got-zero-sends', frames.length >= 1 && frames.every((frame) => frame.type === 'get_protocol_info')
        && stub.requests.length === providerBeforeDegraded && noPaneInput(degraded), { frames, providerRequests: stub.requests.length - providerBeforeDegraded });
      // Compared with the last binding the real engine left (a provider fault may rebind).
      ctx.assert('degraded-engine-keeps-the-saved-binding', lastBinding !== null && same(bindingOf(degraded), lastBinding),
        { before: lastBinding, after: bindingOf(degraded) });
      const identityC = identityOf(ctx, degraded, sessionRecordOf(ctx, bindingOf(degraded), degraded.engine.profileCwd));
      ctx.evidence.writeJson('runtime/identity-c.json', identityC);
      identities.c = identityC;
      ctx.assert('supervisor-identity-unchanged-across-restarts', ['a', 'aRestarted', 'b', 'c'].every((key) => identities[key] !== undefined)
        && [identities.aRestarted, identities.b, identities.c].every((observed) => same(observed, identities.a)), identities);
    }
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
    'supervisor-identity-provisioned',
    'engine-ready-with-required-capabilities',
    'assess-writes-no-goals-dispatches-or-receipts',
    'assessment-returns-a-structured-decision',
    'decision-session-has-no-effect-tools',
    'assessment-sent-no-pane-input',
    'supervisor-identity-observed',
    'engine-identity-checked-before-signal',
    'engine-relaunched-under-the-same-daemon',
    'engine-restart-keeps-the-durable-session',
    'resume-from-another-parent-cwd-keeps-identity-and-session',
    'provider-error-reached-the-stub',
    'provider-error-is-a-typed-fault',
    'engine-runs-without-model-fallback',
    'no-fallback-canary-got-zero-requests',
    'repetition-guard-never-fired',
    'missing-capability-is-refused-as-degraded',
    'degraded-engine-got-zero-sends',
    'supervisor-identity-unchanged-across-restarts',
  ],
  run: runRuntime,
});
