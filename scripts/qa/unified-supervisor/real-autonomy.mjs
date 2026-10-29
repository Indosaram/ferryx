// Scenario `real-autonomy` (plan todo 17, F1 D-1): the autonomy contract with a REAL supervisor
// model and a REAL provider worker CLI in the pane. Opt-in: it runs only when the config carries
// `realEngine`; without it the scenario refuses with PREREQUISITE_MISSING (never PASS), and
// `--all` skips it (status SKIPPED, which is never a PASS either) unless realEngine is set.
//
// - Supervisor model: the hub's retained mhc engine with a run-local models.json whose only
//   provider is `realEngine.provider` at `realEngine.baseUrl` (e.g. mahoquot-gateway). The
//   provider's apiKey field holds the NAME of the env var (`realEngine.apiKeyEnv`), which mhc
//   resolves from its environment (omo convention: an all-caps apiKey is an env var name), so
//   the key value is never written to disk or evidence. Model fallback is off.
// - Worker: Claude Code (`realEngine.workerCli` = "claude"), the agent Ferryx integrates through
//   lifecycle hooks (src-tauri/src/daemon/agent_hook_install.rs, ferryx_scope/ssh/agent_hook_core.rs).
//   The runner only creates the empty isolated `<HOME>/.claude` directory; the daemon's own boot
//   install (agent_extension.rs install_agent_state_extension_if_enabled) writes the hooks into
//   `<HOME>/.claude/settings.json`, each naming the daemon's own executable as
//   `<exe> agent-hook claude <Event>`. Only the `ferryx` binary (src-tauri/src/main.rs) dispatches
//   `agent-hook`; `ferryx-cli` (src/bin/cli.rs) does not, so the isolated daemon here runs from
//   `config.ferryxApp` (which must be the ferryx executable), not `ferryxCli`. The real
//   `~/.claude` is never touched (hashed before and after).
// - The pane: the dispatcher launches a login shell (SHELL = a runner-written wrapper), which
//   seeds Claude's first-run state for the pane's cwd in the isolated CLAUDE_CONFIG_DIR and
//   execs `claude --model <workerModel> --dangerously-skip-permissions`. Permission prompts are
//   bypassed so the worker can edit and run verify.sh unattended; the blast radius is the
//   isolated HOME/CLAUDE_CONFIG_DIR and the pane cwd, a task-owned lane worktree under the run
//   root. The worker talks to `realEngine.workerBaseUrl` with ANTHROPIC_AUTH_TOKEN taken from
//   the env var `workerApiKeyEnv` (default `apiKeyEnv`); telemetry and auto-update are off.
// - Forced failure: the committed verify.sh fails the first time it runs in a checkout (saying
//   so, and telling the caller to run it again) and otherwise checks exact bytes READY. Whoever
//   runs it first (the worker in its turn, or the hub's own check after the turn, which then
//   queues a corrective run) sees a failure that must be corrected before the goal can pass.
// The hub database is only read, read-only. TCP peers of the daemon's process family are
// sampled from /proc (Linux) while the run lives.
import { existsSync, mkdirSync, readdirSync, readFileSync, readlinkSync, realpathSync, statSync, writeFileSync } from 'node:fs';
import { lookup } from 'node:dns/promises';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { awaitGone, familyOf, HarnessError, isInside, listProcesses, PLATFORM, POSIX, sha256 } from './harness.mjs';

export const REAL_AUTONOMY_REQUIRED_ASSERTIONS = Object.freeze([
  'versions-recorded', 'product-hooks-installed-in-isolated-home', 'engine-ready-on-configured-model', 'workspace-registered',
  'submit-admitted', 'real-worker-pane-started-by-dispatch', 'agent-state-working-via-provider-hook', 'goal-completed-without-owner',
  'agent-state-idle-via-provider-hook', 'verifier-failure-observed-then-passed', 'r1-met-with-ferryx-check', 'decision-by-real-model',
  'every-engine-call-has-a-cause', 'only-submit-was-requested', 'final-bytes-verify-on-disk', 'no-network-peers-other-than-gateway',
  'worker-processes-gone', 'real-claude-config-untouched',
]);

const WORKSPACE_ID = 'qa-real-autonomy';
const PROJECT = 'qa-real-autonomy';
const HOST = 'local';
const BODY = 'Create result.txt containing READY. Run the verifier. Correct failures until it passes.';
const WORK = Object.freeze({
  title: 'Write result.txt',
  instruction: 'Create result.txt containing READY. Run the verifier (sh verify.sh). Correct failures until it passes.',
  requirements: [{ id: 'r1', text: 'result.txt contains exactly the five bytes READY (no newline)', verifier: 'verify.sh exits 0' }],
  operations: ['edit', 'test'],
});
const CONTEXT_WINDOW = 200_000;
const GOAL_DEADLINE_MS = 14 * 60_000;
const SAMPLE_MS = 1_000;

const quote = (value) => `'${String(value).replace(/'/g, `'\\''`)}'`;

const verifyScript = (log) => [
  '#!/bin/sh',
  '# QA verifier: result.txt must be exactly the five bytes READY. The first run in a checkout',
  '# always fails by design, so a failure has to be observed and the verifier run again.',
  `log=${quote(log)}`,
  'if [ ! -e .qa-verify-seen ]; then',
  '  : > .qa-verify-seen',
  '  echo "$(pwd) first 1" >> "$log"',
  '  echo "verify: FAIL (the first verification in this checkout always fails; run sh verify.sh again)" >&2',
  '  exit 1',
  'fi',
  'if printf READY | cmp -s - result.txt; then echo "$(pwd) pass 0" >> "$log"; echo "verify: PASS"; exit 0; fi',
  'echo "$(pwd) bytes 1" >> "$log"',
  'echo "verify: FAIL (result.txt must be exactly READY with no trailing newline)" >&2',
  'exit 1',
  '',
].join('\n');

// Seeds Claude Code's first-run state in the isolated CLAUDE_CONFIG_DIR for the pane's cwd, so
// the TUI starts at its prompt: onboarding done, the cwd trusted, bypass mode accepted. It never
// writes settings.json (the product's hook file) and never a credential.
const SEED = `import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
const dir = process.env.CLAUDE_CONFIG_DIR;
const cwd = process.argv[2];
const file = join(dir, '.claude.json');
let state = {};
try { if (existsSync(file)) state = JSON.parse(readFileSync(file, 'utf8')); } catch {}
state.hasCompletedOnboarding = true;
state.theme = state.theme ?? 'dark';
state.bypassPermissionsModeAccepted = true;
state.autoUpdates = false;
state.projects = state.projects ?? {};
state.projects[cwd] = { ...(state.projects[cwd] ?? {}), hasTrustDialogAccepted: true, hasCompletedProjectOnboarding: true, allowedTools: [] };
writeFileSync(file, JSON.stringify(state, null, 2), { mode: 0o600 });
`;

function fileStamp(path) {
  try { return { exists: true, sha256: sha256(readFileSync(path)), mtimeMs: statSync(path).mtimeMs }; } catch { return { exists: false }; }
}

// First value under `key` anywhere in a JSON reply (the CLI wraps the daemon reply).
function deep(value, key) {
  if (!value || typeof value !== 'object') return undefined;
  if (Object.hasOwn(value, key)) return value[key];
  for (const item of Object.values(value)) {
    const found = deep(item, key);
    if (found !== undefined) return found;
  }
  return undefined;
}

// ---------------------------------------------------------------------------
// Linux /proc TCP peers of a process family (ESTABLISHED or SYN_SENT).

function decodeAddress(hexPort, v6) {
  const [hex, port] = hexPort.split(':');
  let ip;
  if (!v6) {
    ip = [0, 1, 2, 3].map((i) => parseInt(hex.slice(6 - 2 * i, 8 - 2 * i), 16)).join('.');
  } else {
    const full = [0, 1, 2, 3].map((w) => hex.slice(w * 8, w * 8 + 8).match(/../g).reverse().join('')).join('').toLowerCase();
    const groups = full.match(/..../g);
    if (groups.slice(0, 5).every((g) => g === '0000') && groups[5] === 'ffff') {
      ip = [groups[6], groups[7]].flatMap((g) => [parseInt(g.slice(0, 2), 16), parseInt(g.slice(2), 16)]).join('.');
    } else {
      ip = groups.map((g) => parseInt(g, 16).toString(16)).join(':');
    }
  }
  return { ip, port: parseInt(port, 16) };
}

function tcpTable(pid) {
  const rows = new Map();
  for (const [file, v6] of [['tcp', false], ['tcp6', true]]) {
    let text = '';
    try { text = readFileSync(`/proc/${pid}/net/${file}`, 'utf8'); } catch { continue; }
    for (const line of text.split('\n').slice(1)) {
      const fields = line.trim().split(/\s+/);
      if (fields.length < 10) continue;
      rows.set(fields[9], { remote: decodeAddress(fields[2], v6), state: fields[3] });
    }
  }
  return rows;
}

function socketInodes(pid) {
  const inodes = [];
  let fds = [];
  try { fds = readdirSync(`/proc/${pid}/fd`); } catch { return inodes; }
  for (const fd of fds) {
    try {
      const match = /^socket:\[(\d+)\]$/.exec(readlinkSync(`/proc/${pid}/fd/${fd}`));
      if (match) inodes.push(match[1]);
    } catch { /* closed during the scan */ }
  }
  return inodes;
}

function commOf(pid) {
  try { return readFileSync(`/proc/${pid}/comm`, 'utf8').trim(); } catch { return null; }
}

function cmdlineOf(pid) {
  try { return readFileSync(`/proc/${pid}/cmdline`, 'utf8').split('\0').filter(Boolean).join(' '); } catch { return null; }
}

const isLoopback = (ip) => ip.startsWith('127.') || ip === '0:0:0:0:0:0:0:1';

export async function runRealAutonomy(ctx, deps) {
  const { config } = ctx;
  const real = config.realEngine;
  if (!real) {
    throw new HarnessError('PREREQUISITE_MISSING', 'real-autonomy needs config.realEngine (supervisor provider endpoint/model/apiKeyEnv and the worker CLI); it is opt-in and never runs on fixtures');
  }
  if (!POSIX) throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', 'real-autonomy runs a POSIX pane shell wrapper; run it on Linux');
  const workerKeyEnv = real.workerApiKeyEnv ?? real.apiKeyEnv;
  const unset = [...new Set([real.apiKeyEnv, workerKeyEnv])].filter((name) => !process.env[name]);
  if (unset.length > 0) {
    throw new HarnessError('PREREQUISITE_MISSING', `env var(s) ${unset.join(', ')} named by realEngine are not set in the runner's environment`);
  }
  const supervisorKey = process.env[real.apiKeyEnv];
  const workerKey = process.env[workerKeyEnv];
  let appInfo = null;
  try { appInfo = statSync(config.ferryxApp); } catch { /* refused below */ }
  if (!appInfo?.isFile()) {
    throw new HarnessError('PREREQUISITE_MISSING', `ferryxApp ${config.ferryxApp} must be the ferryx executable (not an app bundle): only it dispatches the agent-hook reporter the installed hooks run`);
  }
  const git = deps.onPath('git');
  if (!git) throw new HarnessError('PREREQUISITE_MISSING', 'git is not on PATH; the fixture project is a git repository');
  const paneShell = deps.onPath('bash') ?? '/bin/sh';

  const data = realpathSync(ctx.paths.data);
  const dbPath = join(data, 'hub', 'hub.sqlite');
  if (!isInside(dbPath, data)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `hub database ${dbPath} is outside the run root`);
  const realHome = homedir();
  const realClaude = [join(realHome, '.claude', 'settings.json'), join(realHome, '.claude.json')];
  const realBefore = realClaude.map(fileStamp);

  // Allowed network peers: loopback and the configured gateway endpoint(s).
  const allowed = [];
  for (const raw of new Set([real.baseUrl, real.workerBaseUrl])) {
    const url = new URL(raw);
    const port = Number(url.port || (url.protocol === 'https:' ? 443 : 80));
    const addresses = await lookup(url.hostname, { all: true }).catch(() => []);
    for (const { address } of addresses) allowed.push({ ip: address, port });
  }

  // Versions (no secrets): the worker CLI and mhc, with the isolated HOME.
  const probeEnv = ctx.productEnv({ DISABLE_AUTOUPDATER: '1', CLAUDE_CONFIG_DIR: join(data, 'version-probe', '.claude') });
  const workerVersion = await ctx.run('worker-version', real.workerBin, ['--version'], { env: probeEnv, cwd: data, deadlineMs: 30_000 });
  const mhcVersion = await ctx.run('mhc-version', config.mhcBin, ['--version'], { env: probeEnv, cwd: data, deadlineMs: 30_000 });
  const versions = {
    worker: { cli: real.workerCli, bin: real.workerBin, version: workerVersion.stdout.trim() || null, model: real.workerModel, baseUrl: real.workerBaseUrl, apiKeyEnv: workerKeyEnv },
    supervisor: { mhc: config.mhcBin, version: mhcVersion.stdout.trim() || null, provider: real.provider, api: real.api, model: real.model, baseUrl: real.baseUrl, apiKeyEnv: real.apiKeyEnv },
    daemonExecutable: config.ferryxApp,
  };
  ctx.assert('versions-recorded', workerVersion.code === 0 && Boolean(versions.worker.version) && mhcVersion.code === 0, versions);

  // Fixture repository with the committed verifier.
  const workspaceRoot = join(data, 'ws');
  const repo = join(workspaceRoot, PROJECT);
  const verifyLog = join(data, 'verify-runs.log');
  mkdirSync(repo, { recursive: true });
  writeFileSync(join(repo, 'verify.sh'), verifyScript(verifyLog), { mode: 0o755 });
  writeFileSync(verifyLog, '');
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

  // Isolated HOME: only the empty Claude config dir exists before the daemon boots.
  const home = join(data, 'home');
  const claudeDir = join(home, '.claude');
  mkdirSync(claudeDir, { recursive: true });
  const hooksFile = join(claudeDir, 'settings.json');
  if (existsSync(hooksFile)) throw new HarnessError('HARNESS_ERROR', `${hooksFile} exists before the daemon booted`);

  // Pane shell wrapper and the first-run seed.
  const bin = join(data, 'bin');
  mkdirSync(bin, { recursive: true });
  const seed = join(bin, 'seed-claude.mjs');
  writeFileSync(seed, SEED, { mode: 0o644 });
  const shell = join(bin, 'qa-real-worker-shell');
  writeFileSync(shell, [
    '#!/bin/sh',
    '# -c/-lc/-lic probes (login PATH resolution) run a real shell; a pane (-l) runs the worker.',
    'case "$1" in -*c*) shift; exec /bin/sh -c "$1" ;; esac',
    `${quote(process.execPath)} ${quote(seed)} "$(pwd)"`,
    `SHELL=${quote(paneShell)}`,
    'export SHELL',
    `exec ${quote(real.workerBin)} --model ${quote(real.workerModel)} --dangerously-skip-permissions`,
    '',
  ].join('\n'), { mode: 0o755 });

  // Real-provider mhc profile (the stub variant is mhc.mjs prepareEngineEnv).
  const agentDir = join(home, '.omo', 'agent');
  mkdirSync(agentDir, { recursive: true });
  const models = { providers: { [real.provider]: { api: real.api, baseUrl: real.baseUrl, apiKey: real.apiKeyEnv,
    models: [{ id: real.model, contextWindow: CONTEXT_WINDOW, maxTokens: 8192 }] } } };
  writeFileSync(join(agentDir, 'models.json'), `${JSON.stringify(models, null, 2)}\n`, { mode: 0o600 });
  writeFileSync(join(agentDir, 'settings.json'), `${JSON.stringify({ retry: { modelFallback: false } }, null, 2)}\n`, { mode: 0o600 });
  writeFileSync(join(agentDir, 'mcp.json'), `${JSON.stringify({ mcpServers: { context7: { enabled: false }, grep_app: { enabled: false } } }, null, 2)}\n`, { mode: 0o600 });
  const envFile = join(data, 'hub-real.env');
  writeFileSync(envFile, ['FERRYX_HUB_MHC_SUPERVISOR=1', `FERRYX_HUB_MHC_BIN=${config.mhcBin}`, `FERRYX_HUB_MHC_PROVIDER=${real.provider}`,
    `FERRYX_HUB_MHC_MODEL=${real.model}`, ''].join('\n'), { mode: 0o600 });
  const trackerAddr = `127.0.0.1:${await deps.freeLoopbackPort()}`;
  const quiet = { DO_NOT_TRACK: '1', OMO_DISABLE_POSTHOG: '1', MAHO_OFFLINE: '1', MAHO_SKIP_VERSION_CHECK: '1', OMON_INBOX_ENABLED: '0',
    DISABLE_TELEMETRY: '1', DISABLE_ERROR_REPORTING: '1', DISABLE_AUTOUPDATER: '1', CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1' };
  const env = ctx.productEnv({
    FERRYX_HUB_ENV_FILE: envFile,
    FERRYX_HUB_TRACKER_ADDR: trackerAddr,
    MAHO_CODING_AGENT_DIR: agentDir,
    MAHO_MEMORY_HOME: join(ctx.paths.memory, 'mhc'),
    SHELL: shell,
    FERRYX_HUB_WORKSPACE: WORKSPACE_ID,
    OMON_LANE_AUTO_WORKTREE: '1',
    FERRYX_SUPERVISOR_TURN_START_MS: '120000',
    CLAUDE_CONFIG_DIR: claudeDir,
    ANTHROPIC_BASE_URL: real.workerBaseUrl,
    ANTHROPIC_AUTH_TOKEN: workerKey,
    [real.apiKeyEnv]: supervisorKey,
    ...quiet,
  });
  ctx.evidence.writeJson('real-autonomy/engine-env.json', { envFile, agentDir, models, trackerAddr, quiet, shell, claudeDir, allowedPeers: allowed, versions });

  const setup = await ctx.cli(['supervisor', 'setup', '--owner', config.ownerPrincipal, '--project', PROJECT, '--repo-identity', `qa:${PROJECT}`,
    '--host', HOST, '--git-common-dir', gitCommonDir, '--json'], { label: 'supervisor-setup', env });
  if (setup.code !== 0 || !setup.json?.projectId) throw new HarnessError('SETUP_FAILED', `supervisor setup failed: ${JSON.stringify(setup.json)} ${setup.stderr}`);

  // The daemon runs from the ferryx executable: the hooks it installs name it.
  const daemon = await ctx.startDaemon({ env, deadlineMs: 60_000, executable: config.ferryxApp });
  await ctx.waitForPath(hooksFile, { deadlineMs: 30_000, label: 'product hook install' });
  let hooks = null;
  try { hooks = JSON.parse(readFileSync(hooksFile, 'utf8')); } catch { /* asserted below */ }
  const commands = Object.entries(hooks?.hooks ?? {}).flatMap(([event, groups]) => (Array.isArray(groups) ? groups : [])
    .flatMap((group) => (group?.hooks ?? []).map((hook) => ({ event, command: hook?.command ?? null }))));
  const names = (event) => commands.some((entry) => entry.event === event && typeof entry.command === 'string'
    && entry.command.endsWith(` agent-hook claude ${event}`) && !entry.command.includes('ferryx-cli'));
  ctx.assert('product-hooks-installed-in-isolated-home', ['SessionStart', 'UserPromptSubmit', 'PreToolUse', 'Stop'].every(names), { hooksFile, commands });

  // Sample the daemon family's TCP peers while the run lives (observation only, never a wait).
  const peers = new Map();
  const family = new Map();
  let sampling = false;
  const sample = async () => {
    if (sampling || PLATFORM !== 'linux') return;
    sampling = true;
    try {
      const rows = await listProcesses();
      if (!rows) return;
      const members = [{ pid: daemon.pid, startStamp: null }, ...familyOf(rows, daemon.pid)];
      const table = tcpTable(daemon.pid);
      for (const member of members) {
        if (!family.has(member.pid)) family.set(member.pid, { pid: member.pid, startStamp: member.startStamp ?? null, comm: commOf(member.pid), cmdline: cmdlineOf(member.pid) });
        for (const inode of socketInodes(member.pid)) {
          const row = table.get(inode);
          if (!row || !['01', '02'].includes(row.state)) continue;
          const key = `${row.remote.ip}:${row.remote.port}`;
          if (!peers.has(key)) peers.set(key, { ...row.remote, pid: member.pid, comm: commOf(member.pid), at: new Date().toISOString() });
        }
      }
    } finally { sampling = false; }
  };
  const sampler = setInterval(() => { sample().catch(() => { /* a pid vanished mid-scan */ }); }, SAMPLE_MS);
  try {
    const engine = await deps.awaitEngine(ctx, env, 'real-ready', (json) => json?.engine?.state === 'ready' && json?.hubOpen === true, 180_000);
    ctx.assert('engine-ready-on-configured-model', engine?.engine?.state === 'ready' && JSON.stringify(engine).includes(real.model), { engine, model: real.model });
    const registered = await deps.registerWorkspace(ctx, join(data, 'runtime', 'daemon.sock'), WORKSPACE_ID, realpathSync(workspaceRoot));
    ctx.assert('workspace-registered', registered.at(-1)?.type === 'registerWorkspaceOk', { replies: registered });

    // The owner's one request, through the owned MCP channel.
    const mcp = await ctx.spawn('mcp', config.ferryxCli, ['mcp'], { env, cwd: data, stdin: 'pipe', graceMs: 3_000 });
    mcp.child.stdin.on('error', () => { /* the exit event decides */ });
    const sourceId = `qa-real-autonomy-${ctx.runId}`;
    const sentFrames = [];
    const frame = { jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name: 'supervisor.request', arguments: { requestId: sourceId, action: 'submit', payload: { request: BODY, work: WORK } } } };
    const cursor = ctx.journal.cursor();
    ctx.evidence.appendJsonl('requests.jsonl', { label: 'submit', surface: 'ferryx-cli mcp', frame, at: new Date().toISOString() });
    sentFrames.push(frame);
    mcp.child.stdin.write(`${JSON.stringify(frame)}\n`);
    const reply = await ctx.waitFor('mcp reply to submit', (e) => e.source === 'mcp' && e.kind === 'stdout' && e.json?.id === 1, { after: cursor, deadlineMs: 60_000 });
    let body = null;
    try { body = JSON.parse(reply.json?.result?.content?.[0]?.text ?? 'null'); } catch { /* asserted below */ }
    ctx.evidence.appendJsonl('responses.jsonl', { label: 'submit', reply: reply.json, body, at: new Date().toISOString() });
    const goalId = body?.receipt?.goal_id ?? null;
    ctx.assert('submit-admitted', reply.json?.error == null && body?.outcome === 'admitted' && typeof goalId === 'string', { reply: reply.json, body });
    if (!goalId) throw new HarnessError('ADMISSION_FAILED', 'the submit returned no goal; nothing can run');

    const hub = await deps.openHubReader(dbPath);
    // The first dispatch starts a real pane: its start receipt is the worker's own working report.
    const started = await deps.waitForChange(ctx, 'real worker dispatch to start or hold', dirname(dbPath), () => {
      const goal = hub.one('SELECT run_state, state_detail FROM supervisor_goals WHERE goal_id = ?', goalId);
      const dispatches = hub.all('SELECT request_id, state, run_id, provider_session_id FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id', goalId);
      return { done: dispatches.some((row) => row.state === 'started' || row.state === 'settled') || deps.heldStates.has(goal?.run_state), goal, dispatches };
    }, 5 * 60_000);
    const binding = hub.one('SELECT kind, host_id, provider_session_id FROM supervisor_execution_bindings WHERE goal_id = ?', goalId);
    const sessionId = binding?.provider_session_id ?? null;
    ctx.assert('real-worker-pane-started-by-dispatch', binding?.kind === 'pty' && typeof sessionId === 'string'
      && started.dispatches.some((row) => row.provider_session_id === sessionId && ['started', 'settled'].includes(row.state)), { goal: started.goal, dispatches: started.dispatches, binding });
    // The daemon's observer log answers from its retained readings, so a working reading that
    // already happened is returned at once (after=0).
    const working = sessionId ? await ctx.cli(['agents', 'wait', sessionId, '--until', 'working', '--after', '0', '--timeout-ms', '20000', '--json'], { label: 'agents-wait-working', env, deadlineMs: 40_000 }) : null;
    const workingObservation = deep(working?.json, 'observation') ?? null;
    ctx.assert('agent-state-working-via-provider-hook', workingObservation?.state === 'working' && workingObservation?.stateSource === 'provider'
      && workingObservation?.tier === 'native', { reply: working?.json });

    // Nothing more is asked of anyone: the goal must end by itself.
    const observedDetails = [];
    const ended = await deps.waitForChange(ctx, 'goal to complete or hold', dirname(dbPath), () => {
      const goal = hub.one('SELECT run_state, state_detail, revision, lane_key FROM supervisor_goals WHERE goal_id = ?', goalId);
      if (goal?.state_detail && observedDetails.at(-1) !== goal.state_detail) observedDetails.push(goal.state_detail);
      return { done: deps.heldStates.has(goal?.run_state), goal };
    }, GOAL_DEADLINE_MS);
    const goal = ended.goal;
    ctx.assert('goal-completed-without-owner', goal?.run_state === 'completed', { goal, observedDetails });

    const after = Number(workingObservation?.sequence ?? 0);
    const idle = sessionId ? await ctx.cli(['agents', 'wait', sessionId, '--until', 'idle', '--after', String(after), '--timeout-ms', '20000', '--json'], { label: 'agents-wait-idle', env, deadlineMs: 40_000 }) : null;
    const idleObservation = deep(idle?.json, 'observation') ?? deep(idle?.json, 'snapshot')?.find?.((entry) => entry?.sessionId === sessionId && entry?.state === 'idle') ?? null;
    ctx.assert('agent-state-idle-via-provider-hook', idleObservation?.state === 'idle' && idleObservation?.stateSource === 'provider'
      && Number(idleObservation?.sequence) > after, { reply: idle?.json, after });

    const dispatches = hub.all('SELECT id, request_id, goal_revision, state, run_id, created_at, settled_at FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id', goalId);
    const verifyRuns = readFileSync(verifyLog, 'utf8').split('\n').filter(Boolean);
    const firstFail = verifyRuns.findIndex((line) => / 1$/.test(line));
    ctx.assert('verifier-failure-observed-then-passed', firstFail >= 0 && verifyRuns.slice(firstFail + 1).some((line) => / 0$/.test(line)), { verifyRuns, dispatches });

    const requirement = hub.one("SELECT status, evidence_json FROM lane_goal_requirements WHERE lane_key = ? AND req_id = 'r1'", goal?.lane_key ?? '');
    let refs = [];
    try { refs = JSON.parse(requirement?.evidence_json ?? '[]'); } catch { /* asserted below */ }
    ctx.assert('r1-met-with-ferryx-check', requirement?.status === 'met' && refs.includes('sig:req:r1'), { requirement, refs });

    const judge = hub.all('SELECT purpose, kind, outcome, used_fallback FROM judge_calls ORDER BY rowid');
    const graded = judge.filter((call) => ['gap_assess', 'convergence_check'].includes(call.purpose));
    const modelCalls = hub.all('SELECT id, cause, purpose, role, goal_id, provider, model, outcome, input_tokens, output_tokens FROM supervisor_model_calls ORDER BY id');
    // models.json names one provider and one model, and fallback is off: every ok engine call
    // ran on the configured real model.
    ctx.assert('decision-by-real-model', graded.length >= 2 && graded.every((call) => String(call.kind).startsWith('mhc_') && call.outcome === 'ok' && !call.used_fallback)
      && modelCalls.some((call) => call.outcome === 'ok')
      && modelCalls.every((call) => call.model === null || call.model === real.model || String(call.model).endsWith(`/${real.model}`)),
    { judge, modelCalls, model: real.model });
    ctx.assert('every-engine-call-has-a-cause', modelCalls.length >= 1 && modelCalls.every((call) => typeof call.cause === 'string' && call.cause.length > 0 && call.cause !== 'unattributed'), { modelCalls });

    const mcpCalls = sentFrames.map((sent) => ({ id: sent.id, method: sent.method, tool: sent.params?.name ?? null, action: sent.params?.arguments?.action ?? null }));
    const replyIds = ctx.journal.events.filter((e) => e.source === 'mcp' && e.kind === 'stdout' && e.json && e.json.id !== undefined && e.json.id !== null).map((e) => e.json.id);
    const requests = hub.all('SELECT source_adapter, source_message_id, goal_id FROM supervisor_requests ORDER BY id');
    ctx.assert('only-submit-was-requested', mcpCalls.length === 1 && mcpCalls[0].action === 'submit' && JSON.stringify(replyIds) === '[1]'
      && requests.length === 1 && requests[0].source_message_id === sourceId && requests[0].goal_id === goalId, { mcpCalls, replyIds, requests });

    // The runner checks the final bytes itself, in the lane's checkout.
    const lane = hub.one('SELECT cwd FROM lanes WHERE key = ?', goal?.lane_key ?? '');
    const cwd = lane?.cwd ?? null;
    const check = cwd ? await ctx.run('verify-final', '/bin/sh', ['verify.sh'], { cwd, deadlineMs: 20_000 }) : null;
    const bytes = cwd && existsSync(join(cwd, 'result.txt')) ? readFileSync(join(cwd, 'result.txt'), 'utf8') : null;
    ctx.assert('final-bytes-verify-on-disk', check?.code === 0 && bytes === 'READY' && Boolean(cwd) && isInside(cwd, data), { cwd, code: check?.code, bytes });

    await sample();
    clearInterval(sampler);
    const rogue = [...peers.values()].filter((peer) => !isLoopback(peer.ip) && !allowed.some((entry) => entry.ip === peer.ip && entry.port === peer.port));
    ctx.assert('no-network-peers-other-than-gateway', PLATFORM === 'linux' && family.size > 1 && rogue.length === 0,
      { platform: PLATFORM, allowed, peers: [...peers.values()], rogue, note: 'sampled every second from /proc of the daemon family; a connection shorter than the interval can be missed' });

    // Cleanup: the daemon goes, and with it every process of its family (panes, claude, mhc).
    await ctx.terminate(mcp, 'cleanup');
    await ctx.terminate(daemon, 'worker-cleanup');
    const members = [...family.values()].filter((member) => member.pid !== daemon.pid);
    const gone = await Promise.all(members.map((member) => awaitGone({ pid: member.pid, startStamp: member.startStamp }, 15_000)));
    const left = members.filter((_, index) => !gone[index]);
    for (const member of left) {
      try { process.kill(member.pid, 'SIGKILL'); } catch { /* already gone */ }
    }
    ctx.assert('worker-processes-gone', members.some((member) => /claude/i.test(`${member.comm ?? ''} ${member.cmdline ?? ''}`)) && left.length === 0, { members, left });

    const realAfter = realClaude.map(fileStamp);
    ctx.assert('real-claude-config-untouched', JSON.stringify(realBefore) === JSON.stringify(realAfter), { files: realClaude, before: realBefore, after: realAfter });

    ctx.evidence.writeJson('real-autonomy.json', {
      worker: { kind: 'real', ...versions.worker, permissions: '--dangerously-skip-permissions, confined to the isolated HOME/CLAUDE_CONFIG_DIR and the task-owned lane worktree' },
      supervisor: { kind: 'real', ...versions.supervisor },
      sourceId, goalId, goal, dispatches, observedDetails, requirement, judgeCalls: judge, modelCalls, verifyRuns,
      agentState: { working: workingObservation, idle: idleObservation }, hooks: commands, final: { cwd, code: check?.code, bytes },
    });
  } finally {
    clearInterval(sampler);
  }
}
