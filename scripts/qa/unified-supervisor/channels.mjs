// Scenario `channels` (plan todo 17, scenario table "channels"): one owner, one supervisor, one
// goal across three channels. The owner submits in a thread of the approved, dedicated Slack QA
// channel (the real Slack workspace, posted with the owner's own user token), corrects from the
// desktop surface (`ferryx-cli supervisor request`, the daemon's SupervisorRequestV1), pauses and
// resumes from a paired phone (the remote HTTP gateway, as authority does), and finally asks in
// the Slack thread for the goal's state. Every view (hub database, desktop snapshot, phone
// snapshot, Slack reply) must name the same goal and revision; no stale action applies and no
// text of the owner's private DM ever appears in the QA thread.
//
// Opt-in: it runs only with config.realSlack; without it the scenario refuses with
// PREREQUISITE_MISSING (never PASS) and `--all` leaves it out. Missing or rejected Slack
// credentials are a named release blocker (PREREQUISITE_MISSING / SLACK_AUTH_FAILED), never a
// stubbed pass. Only env var NAMES are configured; every one contains TOKEN, so the evidence
// redactor masks the values.
//
// Real Slack from an isolated hub: crates/ferryx-hub/src/chat/slack.rs api_base() falls back to
// https://slack.com/api when FERRYX_HUB_SLACK_API is unset (an override is honoured only for
// loopback), so the runner leaves it unset; Socket Mode dials the URL apps.connections.open
// returns (wss-*.slack.com).
//
// Channel binding: the owner's request mentions the bot as a thread root, so the hub binds that
// thread (and only that thread) to the owner's supervisor (owner_led_thread / bind_owner_thread in
// crates/ferryx-hub/src/chat/slack_supervisor.rs). The runner only reads the hub database and
// asserts the thread-scoped binding after the root is posted.
//
// Engine: the hub's retained mhc engine, on config.realEngine's provider when set (as
// real-autonomy), else on a loopback stub that calls the work tool for the QA request and
// answers the thread query from the hub database. The worker is always the fixture worker
// (--multi-turn --obey-correction --hold-dir): its first turn is held by a QA_HOLD token in the
// owner's request until the phone and desktop actions are done.
import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { lookup } from 'node:dns/promises';
import { randomBytes } from 'node:crypto';
import { dirname, join } from 'node:path';
import { familyOf, HarnessError, isInside, listProcesses, PLATFORM, POSIX } from './harness.mjs';
import { cmdlineOf, commOf, isLoopback, socketInodes, tcpTable } from './real-autonomy.mjs';

export const CHANNELS_REQUIRED_ASSERTIONS = Object.freeze([
  'slack-credentials-verified', 'engine-ready', 'workspace-registered', 'qa-channel-bound-to-supervisor', 'slack-socket-mode-connected',
  'owner-dm-sentinel-answered-without-goal', 'thread-request-admitted-once-from-slack', 'thread-reply-carries-acknowledgment-with-goal',
  'first-run-started-and-held', 'phone-pause-same-goal-on-hub-desktop-phone', 'phone-resume-same-goal-and-revision', 'stale-phone-pause-409',
  'desktop-correction-bumps-revision', 'stale-desktop-correction-409', 'goal-completed', 'no-stale-dispatch-started-after-correction',
  'worker-obeyed-the-correction', 'slack-query-answered-in-thread-with-same-goal', 'single-supervisor-and-goal-across-channels',
  'no-private-dm-text-in-thread', 'no-network-peers-other-than-slack-and-gateway', 'qa-slack-messages-deleted', 'fixture-workers-gone',
]);

const SLACK_API = 'https://slack.com/api';
const SLACK_HOSTS = ['slack.com', 'wss-primary.slack.com', 'wss-backup.slack.com'];
const WORKSPACE_ID = 'qa-channels';
const PROJECT = 'qa-channels';
const SUBMIT_TOOL = 'supervisor_submit_work';
const CORRECTION = 'Use DONE instead of READY';
// Ferryx runs this verifier itself (goals/evidence.rs verifier_signal); a Slack submit's
// generated verifier is prose, so the correction names the runnable one.
const CORRECTED_REQUIREMENTS = Object.freeze([{ id: 'r1', text: 'result.txt contains exactly DONE', verifier: 'verify.sh exits 0' }]);
const VERIFY = 'test "$(cat result.txt)" = DONE\n';
const GOAL_DEADLINE_MS = 8 * 60_000;
const SAMPLE_MS = 1_000;

const textOf = (content) => (typeof content === 'string' ? content : JSON.stringify(content ?? ''));
function offeredTools(json) {
  const nameOf = (tool) => tool?.function?.name ?? tool?.custom?.name ?? tool?.name ?? null;
  const lists = [json?.tools, ...(Array.isArray(json?.messages) ? json.messages.map((message) => message?.tools) : [])];
  return lists.filter(Array.isArray).flatMap((list) => list.map(nameOf));
}
function turnUserText(messages) {
  let start = 0;
  messages.forEach((message, index) => { if (message?.role === 'assistant' || message?.role === 'tool') start = index + 1; });
  return messages.slice(start).filter((message) => message?.role === 'user').map((message) => textOf(message?.content)).join('\n');
}

// Loopback provider stub (no realEngine): calls the work tool once for the QA request, answers
// the thread query from `status()`, grades with the converged assessment.
async function startChannelsStub(ctx, deps, { requestMarker, queryMarker, status }) {
  const requests = [];
  let toolCalls = 0;
  const server = createServer((request, response) => {
    let body = '';
    request.setEncoding('utf8');
    request.on('data', (chunk) => { body += chunk; });
    request.on('end', () => {
      let json = null;
      try { json = JSON.parse(body); } catch { /* recorded below */ }
      if (request.method === 'GET') {
        response.writeHead(200, { 'content-type': 'application/json' });
        response.end(JSON.stringify({ object: 'list', data: [{ id: 'qa-supervisor-1', object: 'model' }] }));
        return;
      }
      const messages = Array.isArray(json?.messages) ? json.messages : [];
      const tools = offeredTools(json);
      const conversation = tools.includes(SUBMIT_TOOL);
      const answered = messages.some((message) => message?.role === 'tool');
      const userText = turnUserText(messages);
      const entry = { at: new Date().toISOString(), conversation, answered, request: userText.includes(requestMarker), query: userText.includes(queryMarker) };
      requests.push(entry);
      ctx.evidence.appendJsonl('channels/provider-requests.jsonl', entry);
      const id = `qa-channels-${requests.length}`;
      const model = json?.model ?? 'qa-supervisor-1';
      const chunk = (delta, finish = null) => `data: ${JSON.stringify({ id, object: 'chat.completion.chunk', created: 0, model, choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`;
      response.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
      if (conversation && !answered && entry.request) {
        toolCalls += 1;
        response.write(chunk({ role: 'assistant', tool_calls: [{ index: 0, id: `call_qa_${toolCalls}`, type: 'function',
          function: { name: SUBMIT_TOOL, arguments: JSON.stringify({ title: 'Write result.txt', requirements: ['result.txt contains exactly READY'], worker: 'pty' }) } }] }));
        response.write(chunk({}, 'tool_calls'));
      } else {
        let text = JSON.stringify(deps.promptTextOf(json).includes(deps.convergenceMarker) ? deps.converged : { gaps: [], decision: { decision: 'complete' } });
        if (conversation && entry.query) {
          const goal = status();
          text = goal ? `Goal ${goal.goal_id} is ${goal.run_state} at revision ${goal.revision}.` : 'No goal is registered for this thread.';
        } else if (conversation) text = answered ? 'Done: your request went through the supervisor.' : 'Noted.';
        else if (userText.includes('[ferryx owner message')) text = 'noted';
        response.write(chunk({ role: 'assistant', content: text }));
        response.write(chunk({}, 'stop'));
      }
      response.write(`data: ${JSON.stringify({ id, object: 'chat.completion.chunk', created: 0, model, choices: [], usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 } })}\n\n`);
      response.end('data: [DONE]\n\n');
    });
  });
  await new Promise((resolveListen, rejectListen) => { server.once('error', rejectListen); server.listen(0, '127.0.0.1', resolveListen); });
  return {
    baseUrl: `http://127.0.0.1:${server.address().port}/v1`,
    requests,
    toolCalls: () => toolCalls,
    close: () => new Promise((resolveClose) => { server.closeAllConnections?.(); server.close(() => resolveClose()); }),
  };
}

// Real-provider mhc profile (as real-autonomy): the key stays a ${NAME} reference.
function realEngineEnv(ctx, real, data) {
  const agentDir = join(data, 'home', '.omo', 'agent');
  mkdirSync(agentDir, { recursive: true });
  const models = { providers: { [real.provider]: { api: real.api, baseUrl: real.baseUrl, apiKey: `\${${real.apiKeyEnv}}`,
    models: [{ id: real.model, contextWindow: 200_000, maxTokens: 8192 }] } } };
  writeFileSync(join(agentDir, 'models.json'), `${JSON.stringify(models, null, 2)}\n`, { mode: 0o600 });
  writeFileSync(join(agentDir, 'settings.json'), `${JSON.stringify({ retry: { modelFallback: false } }, null, 2)}\n`, { mode: 0o600 });
  writeFileSync(join(agentDir, 'mcp.json'), `${JSON.stringify({ mcpServers: { context7: { enabled: false }, grep_app: { enabled: false } } }, null, 2)}\n`, { mode: 0o600 });
  const envFile = join(data, 'hub-channels.env');
  writeFileSync(envFile, ['FERRYX_HUB_MHC_SUPERVISOR=1', `FERRYX_HUB_MHC_BIN=${ctx.config.mhcBin}`, `FERRYX_HUB_MHC_PROVIDER=${real.provider}`,
    `FERRYX_HUB_MHC_MODEL=${real.model}`, ''].join('\n'), { mode: 0o600 });
  return { envFile, agentDir, models };
}

export async function runChannels(ctx, deps) {
  const { config } = ctx;
  const slackConfig = config.realSlack;
  if (!slackConfig) {
    throw new HarnessError('PREREQUISITE_MISSING', 'channels needs config.realSlack (the approved QA channel, the QA Slack app and the owner user token env names); it is opt-in and never runs against a stand-in');
  }
  if (!POSIX) throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', 'channels runs the POSIX fixture worker pane; run it on Linux');
  const tokenEnvs = { bot: slackConfig.botTokenEnv, app: slackConfig.appTokenEnv, owner: slackConfig.ownerUserTokenEnv };
  const unset = Object.values(tokenEnvs).filter((name) => !process.env[name]);
  const real = config.realEngine ?? null;
  if (real && !process.env[real.apiKeyEnv]) unset.push(real.apiKeyEnv);
  if (unset.length > 0) {
    throw new HarnessError('PREREQUISITE_MISSING', `release blocker: env var(s) ${unset.join(', ')} named by realSlack/realEngine are not set in the runner's environment; external auth is required, never stubbed`);
  }
  const tokens = { bot: process.env[tokenEnvs.bot], app: process.env[tokenEnvs.app], owner: process.env[tokenEnvs.owner] };
  const git = deps.onPath('git');
  if (!git) throw new HarnessError('PREREQUISITE_MISSING', 'git is not on PATH; the fixture project is a git repository');

  const data = realpathSync(ctx.paths.data);
  const dbPath = join(data, 'hub', 'hub.sqlite');
  const socket = join(data, 'runtime', 'daemon.sock');
  for (const path of [dbPath, socket]) if (!isInside(path, data)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `${path} is outside the run root`);
  const hubDir = dirname(dbPath);
  const transcript = (line) => ctx.evidence.appendText('channels.log', `${new Date().toISOString()} ${line}\n`);
  const tag = `${ctx.runId}-${randomBytes(3).toString('hex')}`;
  const requestMarker = `QA-CHANNELS-${tag}`;
  const queryMarker = `QA-CHANNELS-QUERY-${tag}`;
  const sentinel = `QA-PRIVATE-${randomBytes(8).toString('hex')}`;
  const holdToken = `ch${randomBytes(4).toString('hex')}`;

  // Slack Web API with the named token; every call is logged (the redactor masks tokens).
  const slack = async (label, method, who, params = {}) => {
    let json = null;
    let failure = null;
    try {
      const response = await fetch(`${SLACK_API}/${method}`, { method: 'POST', signal: ctx.signal,
        headers: { authorization: `Bearer ${tokens[who]}`, 'content-type': 'application/x-www-form-urlencoded; charset=utf-8' },
        body: new URLSearchParams(Object.entries(params).map(([key, value]) => [key, String(value)])).toString() });
      json = await response.json();
    } catch (error) { failure = String(error?.message ?? error); }
    ctx.evidence.appendJsonl('channels/slack-web.jsonl', { at: new Date().toISOString(), label, method, token: tokenEnvs[who], params, ok: json?.ok ?? false, error: json?.error ?? failure,
      ts: json?.ts ?? null, channel: json?.channel ?? null, messages: json?.messages?.map?.((message) => ({ ts: message.ts, user: message.user ?? null, bot_id: message.bot_id ?? null, thread_ts: message.thread_ts ?? null, text: message.text })) ?? undefined });
    return json ?? { ok: false, error: failure };
  };

  // 1. Credentials: the bot, the owner user and the team must be the configured ones.
  const botAuth = await slack('auth-bot', 'auth.test', 'bot');
  const ownerAuth = await slack('auth-owner', 'auth.test', 'owner');
  const appOpen = await slack('auth-app', 'apps.connections.open', 'app');
  const credentialsOk = botAuth.ok && ownerAuth.ok && appOpen.ok && botAuth.team_id === slackConfig.teamId && ownerAuth.team_id === slackConfig.teamId
    && ownerAuth.user_id === slackConfig.ownerUserId;
  ctx.assert('slack-credentials-verified', credentialsOk, { bot: { ok: botAuth.ok, error: botAuth.error ?? null, team: botAuth.team_id, user: botAuth.user_id },
    owner: { ok: ownerAuth.ok, error: ownerAuth.error ?? null, team: ownerAuth.team_id, user: ownerAuth.user_id }, app: { ok: appOpen.ok, error: appOpen.error ?? null } });
  if (!credentialsOk) throw new HarnessError('SLACK_AUTH_FAILED', 'release blocker: the QA Slack app or owner token was rejected or names another team/user; see channels/slack-web.jsonl');
  const botUser = botAuth.user_id;
  const owner = `slack:${slackConfig.teamId}:${slackConfig.ownerUserId}`;

  // Allowed network peers: loopback, Slack and (with realEngine) the gateway.
  const allowed = [];
  const allow = async (hostname, port, kind) => {
    for (const { address } of await lookup(hostname, { all: true }).catch(() => [])) allowed.push({ ip: address, port, hostname, kind });
  };
  for (const hostname of SLACK_HOSTS) await allow(hostname, 443, 'slack');
  if (real) {
    const url = new URL(real.baseUrl);
    await allow(url.hostname, Number(url.port || (url.protocol === 'https:' ? 443 : 80)), 'gateway');
  }
  try { await allow(new URL(appOpen.url).hostname, 443, 'slack'); } catch { /* the probe URL is optional */ }

  // 2. Fixture project with the corrected verifier, held fixture worker.
  const workspaceRoot = join(data, 'ws');
  const repo = join(workspaceRoot, PROJECT);
  mkdirSync(repo, { recursive: true });
  writeFileSync(join(repo, 'verify.sh'), VERIFY, { mode: 0o755 });
  const gitArgs = ['-c', 'init.defaultBranch=main', '-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid'];
  for (const [label, args] of [['git-init', [...gitArgs, 'init', '-q', repo]], ['git-add', [...gitArgs, '-C', repo, 'add', 'verify.sh']],
    ['git-commit', [...gitArgs, '-C', repo, 'commit', '-q', '-m', 'qa fixture verifier']]]) {
    const result = await ctx.run(label, git, args, { deadlineMs: 30_000 });
    if (result.code !== 0) throw new HarnessError('FIXTURE_FAILED', `${label} exited ${result.code}: ${result.stderr}`);
  }
  const workerLogs = join(data, 'workers');
  const holdDir = join(data, 'holds');
  const requestsDir = join(data, 'requests');
  for (const dir of [workerLogs, holdDir, requestsDir]) mkdirSync(dir, { recursive: true });
  const shell = join(data, 'bin', 'qa-channels-shell');
  mkdirSync(dirname(shell), { recursive: true });
  writeFileSync(shell, `#!/bin/sh\ncase "$1" in\n  -*c*) shift; exec /bin/sh -c "$1" ;;\nesac\nexec ${deps.shellQuote(process.execPath)} ${deps.shellQuote(deps.worker)} ${deps.shellQuote(workerLogs)} --multi-turn --obey-correction ${deps.shellQuote(`--hold-dir=${holdDir}`)}\n`, { mode: 0o755 });

  let hub = null;
  let goalId = null;
  const goalRow = () => (hub && goalId ? hub.one('SELECT goal_id, run_state, revision, lane_key, state_detail FROM supervisor_goals WHERE goal_id = ?', goalId) : null);
  const stub = real ? null : await startChannelsStub(ctx, deps, { requestMarker, queryMarker, status: goalRow });
  const posted = { owner: [], bot: [] };
  let daemon = null;
  let dmChannel = null;
  const peers = new Map();
  const family = new Map();
  let sampler = null;
  try {
    const deadOmo = `ws://127.0.0.1:${await deps.freeLoopbackPort()}`;
    const slackEnv = {
      SHELL: shell, FERRYX_HUB_WORKSPACE: WORKSPACE_ID, OMON_LANE_AUTO_WORKTREE: '1', OMON_INBOX_ENABLED: '0',
      SLACK_BOT_TOKEN: tokens.bot, SLACK_APP_TOKEN: tokens.app, SLACK_BOT_USER_ID: botUser,
      FERRYX_HUB_SLACK_OWNER: slackConfig.ownerUserId, SLACK_ALLOWED_USERS: slackConfig.ownerUserId,
      SLACK_ALLOWED_CHANNELS: slackConfig.channelId, SLACK_FREE_RESPONSE_CHANNELS: slackConfig.channelId,
      OMON_STATUS_MESSAGE_ENABLED: 'off', SLACK_MISSED_BACKFILL: 'off', FERRYX_HUB_OMO_URL: deadOmo, OMON_OMO_BIN: '/usr/bin/false',
    };
    let env;
    if (real) {
      const profile = realEngineEnv(ctx, real, data);
      env = ctx.productEnv({ FERRYX_HUB_ENV_FILE: profile.envFile, FERRYX_HUB_TRACKER_ADDR: `127.0.0.1:${await deps.freeLoopbackPort()}`,
        MAHO_CODING_AGENT_DIR: profile.agentDir, MAHO_MEMORY_HOME: join(ctx.paths.memory, 'mhc'), [real.apiKeyEnv]: process.env[real.apiKeyEnv],
        DO_NOT_TRACK: '1', OMO_DISABLE_POSTHOG: '1', MAHO_OFFLINE: '1', MAHO_SKIP_VERSION_CHECK: '1', ...slackEnv });
      ctx.evidence.writeJson('channels/engine.json', { kind: 'real', provider: real.provider, model: real.model, baseUrl: real.baseUrl, apiKeyEnv: real.apiKeyEnv, models: profile.models });
    } else {
      env = await deps.prepareEngineEnv(ctx, { baseUrl: stub.baseUrl, mhcBin: config.mhcBin, label: 'channels', extra: slackEnv });
      ctx.evidence.writeJson('channels/engine.json', { kind: 'loopback stub', baseUrl: stub.baseUrl });
    }
    if ('FERRYX_HUB_SLACK_API' in env) delete env.FERRYX_HUB_SLACK_API;
    ctx.evidence.writeJson('channels/setup.json', { owner, team: slackConfig.teamId, channel: slackConfig.channelId, botUser, tokenEnvs, allowedPeers: allowed, requestMarker, queryMarker, holdToken });

    const setup = await ctx.cli(['supervisor', 'setup', '--owner', owner, '--project', PROJECT, '--repo-identity', `qa:${PROJECT}`,
      '--host', 'local', '--git-common-dir', realpathSync(join(repo, '.git')), '--json'], { label: 'supervisor-setup', env });
    if (setup.code !== 0 || !setup.json?.projectId) throw new HarnessError('SETUP_FAILED', `supervisor setup failed: ${setup.stdout} ${setup.stderr}`);
    const projectId = setup.json.projectId;
    const daemonLog = join(env.FERRYX_DATA_DIR, 'logs', 'daemon.log');
    if (!isInside(daemonLog, data)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `${daemonLog} is outside the run root`);
    mkdirSync(dirname(daemonLog), { recursive: true });
    const connectedLines = () => (existsSync(daemonLog) ? readFileSync(daemonLog, 'utf8').split('\n').filter((line) => line.includes('slack socket mode connected')).length : 0);
    daemon = await ctx.startDaemon({ env, deadlineMs: 60_000 });
    if (PLATFORM === 'linux') {
      const sample = async () => {
        const rows = await listProcesses();
        if (!rows) return;
        const table = tcpTable(daemon.pid);
        for (const member of [{ pid: daemon.pid, startStamp: null }, ...familyOf(rows, daemon.pid)]) {
          if (!family.has(member.pid)) family.set(member.pid, { pid: member.pid, startStamp: member.startStamp ?? null, comm: commOf(member.pid), cmdline: cmdlineOf(member.pid) });
          for (const inode of socketInodes(member.pid)) {
            const row = table.get(inode);
            if (!row || !['01', '02'].includes(row.state)) continue;
            const key = `${row.remote.ip}:${row.remote.port}`;
            if (!peers.has(key)) peers.set(key, { ...row.remote, pid: member.pid, comm: commOf(member.pid), at: new Date().toISOString() });
          }
        }
      };
      sampler = setInterval(() => { sample().catch(() => { /* a pid vanished mid-scan */ }); }, SAMPLE_MS);
    }
    const engine = await deps.awaitEngine(ctx, env, 'channels-ready', (json) => json?.engine?.state === 'ready' && json?.hubOpen === true, 180_000);
    ctx.assert('engine-ready', engine?.engine?.state === 'ready', { engine });
    const registered = await deps.registerWorkspace(ctx, socket, WORKSPACE_ID, realpathSync(workspaceRoot));
    ctx.assert('workspace-registered', registered.at(-1)?.type === 'registerWorkspaceOk', { replies: registered });
    hub = await deps.openHubReader(dbPath);

    // 3. Nothing is bound before the owner's thread exists.
    const preBound = hub.count("SELECT COUNT(*) AS n FROM supervisor_channel_bindings WHERE provider = 'slack' AND channel_id = ?", slackConfig.channelId);
    await deps.waitForChange(ctx, 'slack socket mode connected', dirname(daemonLog), () => ({ done: connectedLines() >= 1 }), 60_000);
    ctx.assert('slack-socket-mode-connected', connectedLines() >= 1, { daemonLog });

    const assistantCount = () => hub.count("SELECT COUNT(*) AS n FROM messages WHERE role = 'assistant'");
    const awaitAssistant = (label, before) => deps.waitForChange(ctx, label, hubDir, () => ({ done: assistantCount() > before }), 180_000);
    const goalsCount = () => hub.count('SELECT COUNT(*) AS n FROM supervisor_goals');

    // 4. The private DM: the owner tells the bot a sentinel; it must never reach the thread.
    let before = assistantCount();
    const dm = await slack('owner-dm-sentinel', 'chat.postMessage', 'owner', { channel: botUser, text: `Private note for this DM only: the codename is ${sentinel}. Do not register any work.` });
    if (!dm.ok) throw new HarnessError('SLACK_POST_FAILED', `the owner DM could not be posted (${dm.error})`);
    dmChannel = dm.channel;
    posted.owner.push({ channel: dm.channel, ts: dm.ts });
    await awaitAssistant('dm sentinel answered', before);
    ctx.assert('owner-dm-sentinel-answered-without-goal', goalsCount() === 0, { goals: goalsCount(), dm: { channel: dm.channel, ts: dm.ts } });

    // 5. The QA request as the thread root in the approved channel.
    before = assistantCount();
    const body = `<@${botUser}> ${requestMarker}: Create result.txt containing READY. Run sh verify.sh. Correct failures until it passes. QA_HOLD:${holdToken}`;
    const root = await slack('owner-thread-request', 'chat.postMessage', 'owner', { channel: slackConfig.channelId, text: body });
    if (!root.ok) throw new HarnessError('SLACK_POST_FAILED', `the QA request could not be posted (${root.error})`);
    posted.owner.push({ channel: root.channel, ts: root.ts });
    const sourceId = `${slackConfig.channelId}/${root.ts}`;
    const bindings = hub.all("SELECT binding_id, thread_id, owner_principal FROM supervisor_channel_bindings WHERE provider = 'slack' AND channel_id = ?", slackConfig.channelId);
    ctx.assert('qa-channel-bound-to-supervisor', preBound === 0 && bindings.length === 1 && bindings[0].thread_id === root.ts && bindings[0].owner_principal === owner,
      { preBound, bindings, rootTs: root.ts, owner, scope: 'thread only, never channel-wide' });
    const admitted = await deps.waitForChange(ctx, 'slack request admitted', hubDir, () => {
      const rows = hub.all("SELECT source_message_id, goal_id, owner_principal FROM supervisor_requests WHERE source_adapter = 'slack' AND source_message_id = ?", sourceId);
      return { done: rows.some((row) => row.goal_id), rows };
    }, 180_000);
    goalId = admitted.rows.find((row) => row.goal_id)?.goal_id ?? null;
    ctx.assert('thread-request-admitted-once-from-slack', admitted.rows.length === 1 && admitted.rows[0].owner_principal === owner && goalsCount() === 1
      && (stub ? stub.toolCalls() === 1 : true), { rows: admitted.rows, goals: goalsCount(), toolCalls: stub?.toolCalls() ?? null });
    if (!goalId) throw new HarnessError('ADMISSION_FAILED', 'the Slack request created no goal');
    await awaitAssistant('thread acknowledgment', before);
    const replies = async (label) => {
      const reply = await slack(label, 'conversations.replies', 'bot', { channel: slackConfig.channelId, ts: root.ts, limit: 100 });
      return reply.ok ? reply.messages ?? [] : [];
    };
    const ack = (await replies('replies-after-ack')).filter((message) => message.ts !== root.ts && (message.bot_id || message.user === botUser));
    ctx.assert('thread-reply-carries-acknowledgment-with-goal', ack.some((message) => String(message.text ?? '').includes(goalId)), { goalId, ack: ack.map((message) => ({ ts: message.ts, text: message.text })) });

    // 6. The first run starts and its worker holds the turn.
    const held = await deps.waitForChange(ctx, 'first turn held', workerLogs, () => {
      const entries = deps.readWorkerLogs(workerLogs).flatMap((entry) => entry.entries);
      return { done: entries.some((line) => line.kind === 'held' && line.token === holdToken), entries };
    }, 180_000);
    const admittedGoal = goalRow();
    const admittedRevision = admittedGoal?.revision;
    ctx.assert('first-run-started-and-held', admittedGoal?.run_state === 'running', { goal: admittedGoal, held: held.entries.filter((line) => line.kind === 'held') });

    // Surfaces: the desktop request/snapshot (daemon socket, ferryx-cli) and a paired phone (HTTP).
    const exchange = async (label, request) => {
      const replies = await deps.daemonExchange(socket, [{ type: 'handshake', version: deps.protocolVersion }, request], 30_000);
      ctx.evidence.appendJsonl('socket.jsonl', { label, request, replies, at: new Date().toISOString() });
      return replies.at(-1) ?? null;
    };
    const desktopGoal = async (label) => (await exchange(label, { type: 'supervisorSnapshot' }))?.snapshot?.supervisor?.goals?.find?.((goal) => goal.goalId === goalId) ?? null;
    let requestSeq = 0;
    const desktopRequest = async (label, envelope) => {
      requestSeq += 1;
      const file = join(requestsDir, `${String(requestSeq).padStart(2, '0')}-${label}.json`);
      writeFileSync(file, JSON.stringify(envelope));
      return ctx.cli(['supervisor', 'request', '--json', file], { label, env });
    };
    const configured = await exchange('remote-configure', { type: 'remoteConfigure', config: { mode: 'localNetwork', port: 0, allowControl: true } });
    const bound = (await exchange('remote-status', { type: 'remoteGetStatus' }))?.status?.boundAddress ?? null;
    if (configured?.type !== 'remoteConfigureOk' || !bound) throw new HarnessError('GATEWAY_UNAVAILABLE', `the isolated daemon's remote gateway did not start: ${JSON.stringify(configured)}`);
    const http = async (label, method, path, { token, body: payload } = {}) => {
      const headers = { accept: 'application/json' };
      if (token) headers.authorization = `Bearer ${token}`;
      if (payload !== undefined) headers['content-type'] = 'application/json';
      const response = await fetch(`http://${bound}${path}`, { method, headers, body: payload === undefined ? undefined : JSON.stringify(payload), signal: ctx.signal });
      const text = await response.text();
      let json = null;
      try { json = JSON.parse(text); } catch { /* recorded raw */ }
      ctx.evidence.appendJsonl('http.jsonl', { at: new Date().toISOString(), label, method, path, request: payload ?? null, status: response.status, body: json ?? text });
      return { status: response.status, body: json ?? text };
    };
    const code = await exchange('phone-pairing-code', { type: 'remoteCreateMachinePairingCode' });
    const paired = await http('phone-pair', 'POST', '/api/v1/pair/exchange', { body: { code: code?.code, deviceName: 'qa-phone', installationId: `qa-phone-${ctx.runId}` } });
    if (paired.status !== 200 || !paired.body?.token) throw new HarnessError('PAIRING_FAILED', `phone pairing failed: HTTP ${paired.status}`);
    const phoneToken = paired.body.token;
    const phoneGoal = async (label) => (await http(label, 'GET', '/api/v1/supervisor', { token: phoneToken })).body?.supervisor?.goals?.find?.((goal) => goal.goalId === goalId) ?? null;
    const views = async (label) => ({ hub: goalRow(), desktop: await desktopGoal(`${label}-desktop`), phone: await phoneGoal(`${label}-phone`) });
    const same = (view, state) => view.hub?.run_state === state && view.desktop?.runState === state && view.phone?.runState === state
      && view.desktop?.revision === view.hub.revision && view.phone?.revision === view.hub.revision;

    // 7. Phone pause and resume while the first turn is held.
    const pauseEnvelope = { requestId: `qa-channels-pause-${ctx.runId}`, projectId, goalId, expectedRevision: admittedRevision, action: 'pause', payload: { reason: 'qa phone pause' } };
    const paused = await http('phone-pause', 'POST', '/api/v1/supervisor/request', { token: phoneToken, body: pauseEnvelope });
    const pausedView = await views('paused');
    ctx.assert('phone-pause-same-goal-on-hub-desktop-phone', paused.status === 200 && same(pausedView, 'paused') && pausedView.hub.revision === admittedRevision + 1,
      { status: paused.status, body: paused.body, views: pausedView });
    const resumed = await http('phone-resume', 'POST', '/api/v1/supervisor/request', { token: phoneToken,
      body: { requestId: `qa-channels-resume-${ctx.runId}`, projectId, goalId, expectedRevision: pausedView.hub?.revision, action: 'resume', payload: {} } });
    const resumedView = await views('resumed');
    ctx.assert('phone-resume-same-goal-and-revision', resumed.status === 200 && same(resumedView, 'running') && resumedView.hub.revision === pausedView.hub?.revision,
      { status: resumed.status, body: resumed.body, views: resumedView });
    const stalePause = await http('phone-pause-stale', 'POST', '/api/v1/supervisor/request', { token: phoneToken, body: { ...pauseEnvelope, requestId: `${pauseEnvelope.requestId}-stale` } });
    ctx.assert('stale-phone-pause-409', stalePause.status === 409 && stalePause.body?.error?.code === 'REVISION_CONFLICT' && goalRow()?.run_state === 'running',
      { status: stalePause.status, body: stalePause.body, goal: goalRow() });

    // 8. Desktop correction at the current revision; its replay at the old one is refused.
    const preCorrection = goalRow()?.revision;
    const correctEnvelope = { requestId: `qa-channels-correct-${ctx.runId}`, projectId, goalId, expectedRevision: preCorrection, action: 'correct',
      payload: { text: CORRECTION, requirements: CORRECTED_REQUIREMENTS } };
    const corrected = await desktopRequest('desktop-correct', correctEnvelope);
    const correctedRevision = preCorrection + 1;
    const correctedView = { hub: goalRow(), desktop: await desktopGoal('corrected-desktop'), phone: await phoneGoal('corrected-phone') };
    ctx.assert('desktop-correction-bumps-revision', corrected.code === 0 && corrected.json?.receipt?.revision === correctedRevision && correctedView.hub?.revision === correctedRevision
      && correctedView.desktop?.revision === correctedRevision && correctedView.phone?.revision === correctedRevision, { json: corrected.json, views: correctedView });
    const staleCorrect = await desktopRequest('desktop-correct-stale', { ...correctEnvelope, requestId: `${correctEnvelope.requestId}-stale` });
    ctx.assert('stale-desktop-correction-409', staleCorrect.code !== 0 && staleCorrect.json?.code === 'REVISION_CONFLICT' && goalRow()?.revision === correctedRevision,
      { code: staleCorrect.code, json: staleCorrect.json });

    // 9. Release the held turn: the stale run settles, the corrected dispatch runs, the goal ends.
    writeFileSync(join(holdDir, holdToken), 'release');
    await deps.waitForChange(ctx, 'goal to complete or hold', hubDir, () => ({ done: deps.heldStates.has(goalRow()?.run_state) }), GOAL_DEADLINE_MS);
    const final = goalRow();
    ctx.assert('goal-completed', final?.run_state === 'completed', { goal: final });
    const dispatches = hub.all('SELECT id, request_id, goal_revision, state, run_id FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id', goalId);
    const started = dispatches.filter((row) => row.run_id);
    ctx.assert('no-stale-dispatch-started-after-correction', started.length >= 2 && started[0].goal_revision === admittedRevision
      && started.slice(1).every((row) => row.goal_revision >= correctedRevision), { dispatches, admittedRevision, correctedRevision });
    const turns = deps.readWorkerLogs(workerLogs).flatMap((entry) => entry.entries.filter((line) => line.kind === 'turn'));
    ctx.assert('worker-obeyed-the-correction', turns.at(-1)?.wrote === 'DONE' && turns.at(-1)?.verifyExit === 0, { turns });

    // 10. The owner asks in the thread; the reply names the same goal.
    before = assistantCount();
    const query = await slack('owner-thread-query', 'chat.postMessage', 'owner', { channel: slackConfig.channelId, thread_ts: root.ts,
      text: `${queryMarker}: What is the state of the goal you registered in this thread? Name its goal id and revision.` });
    if (!query.ok) throw new HarnessError('SLACK_POST_FAILED', `the thread query could not be posted (${query.error})`);
    posted.owner.push({ channel: query.channel, ts: query.ts });
    await awaitAssistant('thread query answered', before);
    const thread = await replies('replies-final');
    const botMessages = thread.filter((message) => message.ts !== root.ts && (message.bot_id || message.user === botUser));
    const answer = botMessages.filter((message) => Number(message.ts) > Number(query.ts));
    ctx.assert('slack-query-answered-in-thread-with-same-goal', answer.some((message) => String(message.text ?? '').includes(goalId)),
      { goalId, answer: answer.map((message) => ({ ts: message.ts, text: message.text })) });
    const supervisors = hub.count('SELECT COUNT(*) AS n FROM supervisor_identity');
    const requests = hub.all('SELECT source_adapter, source_message_id, goal_id FROM supervisor_requests ORDER BY id');
    ctx.assert('single-supervisor-and-goal-across-channels', supervisors === 1 && goalsCount() === 1 && requests.every((row) => row.goal_id === null || row.goal_id === goalId),
      { supervisors, goals: goalsCount(), requests });
    ctx.assert('no-private-dm-text-in-thread', thread.every((message) => !String(message.text ?? '').includes(sentinel)), { messages: thread.length });
    for (const message of botMessages) posted.bot.push({ channel: slackConfig.channelId, ts: message.ts });

    if (sampler) clearInterval(sampler);
    // Slack edge addresses rotate: resolve the allowed hosts again before judging a peer.
    for (const hostname of SLACK_HOSTS) await allow(hostname, 443, 'slack');
    const rogue = [...peers.values()].filter((peer) => !isLoopback(peer.ip) && !allowed.some((entry) => entry.ip === peer.ip && entry.port === peer.port));
    ctx.assert('no-network-peers-other-than-slack-and-gateway', PLATFORM === 'linux' && family.size >= 1 && rogue.length === 0,
      { platform: PLATFORM, allowed, peers: [...peers.values()], rogue, note: 'sampled every second from /proc of the daemon family (a sub-second connection can be missed); Slack hosts are resolved at start and again at the end, and a Socket Mode host outside that set is reported as rogue' });

    const workerPids = deps.readWorkerLogs(workerLogs).map((entry) => Number(entry.name.match(/^worker-(\d+)\.jsonl$/)?.[1])).filter(Number.isInteger);
    await ctx.terminate(daemon, 'channels-done');
    daemon = null;
    const alive = await deps.stillAlive(workerPids);
    ctx.assert('fixture-workers-gone', workerPids.length >= 1 && alive.length === 0, { workerPids, alive });
    ctx.evidence.writeJson('channels.json', { owner, goalId, sourceId, admittedRevision, correctedRevision, final, dispatches, turns,
      engine: real ? { kind: 'real', provider: real.provider, model: real.model } : { kind: 'loopback stub', providerRequests: stub.requests.length },
      worker: { kind: 'fixture', note: 'fixture-worker.mjs --multi-turn --obey-correction --hold-dir' } });
  } finally {
    if (sampler) clearInterval(sampler);
    if (daemon) await ctx.terminate(daemon, 'channels cleanup');
    // Cleanup: the bot's replies (thread and DM) with the bot token, the owner's posts with the
    // owner token. The DM's bot replies are listed with the bot token.
    if (dmChannel) {
      const history = await slack('dm-history', 'conversations.history', 'bot', { channel: dmChannel, limit: 50 });
      const since = Number(posted.owner.find((entry) => entry.channel === dmChannel)?.ts ?? Infinity);
      for (const message of history.messages ?? []) if ((message.bot_id || message.user === botUser) && Number(message.ts) >= since) posted.bot.push({ channel: dmChannel, ts: message.ts });
    }
    const deletions = [];
    for (const [who, list] of [['bot', posted.bot], ['owner', posted.owner]]) {
      for (const entry of list) {
        const result = await slack(`delete-${who}`, 'chat.delete', who, { channel: entry.channel, ts: entry.ts });
        deletions.push({ who, ...entry, ok: result.ok || result.error === 'message_not_found', error: result.error ?? null });
      }
    }
    ctx.evidence.writeJson('channels/cleanup.json', { deletions });
    if (posted.owner.length > 0) ctx.assert('qa-slack-messages-deleted', deletions.every((entry) => entry.ok), { deletions });
    await stub?.close();
    transcript(`done goal=${goalId ?? 'none'} deleted=${deletions.length}`);
  }
}
