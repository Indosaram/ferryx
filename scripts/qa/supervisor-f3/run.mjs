#!/usr/bin/env bun
// F3 real-surface acceptance runner (supervisor-chat-redesign F3). Runs on the control host
// (the MacBook that holds MahoRD, tesseract, SSH keys and the WDA endpoint); builds and tests
// nothing locally. See docs/qa/supervisor-f3-real-surfaces.md for the command sequence.
//
//   bun scripts/qa/supervisor-f3/run.mjs --config <f3.json> --revision <40-hex> --evidence <new dir>
//   bun scripts/qa/supervisor-f3/run.mjs --validate <evidence dir>
//
// Topology: the Windows host is the session host. Its isolated daemon runs the fixture agent
// panes (decision, held, upstream) and the run-owned relay with its embedded account service;
// the macOS desktop joins the same account through that relay, and the iPhone signs in to the
// same account in Safari. Every surface therefore shows the one fleet's decision cards.
//
// Exit: 0 pass, 1 fail, 2 blocked (a lane or a scenario could not run: host unavailable, device
// or push permission absent), 5 usage, 130 interrupted. Evidence: <evidence>/run.json,
// <evidence>/verdict.json, <evidence>/manifest.json, <evidence>/<lane>/**.
import { createHash, randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { rpcProcess } from '../fixtures/native-configured-adapters.mjs';
import { EXIT, LANES, RUN_SCHEMA, scenariosForLane, validateRun } from './contract.mjs';
import { runDesktopLane } from './desktop-lane.mjs';
import { runIphoneLane } from './iphone-lane.mjs';
import { markNotRun, newLane, until } from './steps.mjs';

const USAGE = 'usage: run.mjs --config <f3.json> --revision <40-hex> --evidence <new dir> | --validate <evidence dir>';
const REQUIRED_HOST = ['hostId', 'platform', 'sshDestination', 'bun', 'hostHelper', 'remoteAgent', 'worktreeRoot', 'worktree', 'binary', 'cli', 'relay', 'uiDist', 'buildManifest', 'mahordAddress', 'pairingId'];

function parseArgs(argv) {
  const opts = {};
  for (let i = 0; i < argv.length; i += 1) {
    const value = () => { const next = argv[i + 1]; if (!next || next.startsWith('--')) throw new Error(`${argv[i]} needs a value`); i += 1; return next; };
    switch (argv[i]) {
      case '--config': opts.config = value(); break;
      case '--revision': opts.revision = value(); break;
      case '--evidence': opts.evidence = value(); break;
      case '--validate': opts.validate = value(); break;
      default: throw new Error(`unknown argument ${argv[i]}`);
    }
  }
  if (opts.validate) return opts;
  if (!opts.config || !opts.evidence || !/^[0-9a-f]{40}$/.test(opts.revision ?? '')) throw new Error(USAGE);
  return opts;
}

function loadConfig(path) {
  const config = JSON.parse(readFileSync(path, 'utf8'));
  const missing = [];
  for (const name of ['macos', 'windows']) for (const key of REQUIRED_HOST) if (config.hosts?.[name]?.[key] === undefined) missing.push(`hosts.${name}.${key}`);
  for (const key of ['serial', 'driverEndpoint']) if (!config.iphone?.[key]) missing.push(`iphone.${key}`);
  for (const key of ['mahordExecutable', 'tesseract', 'ownerEmail']) if (!config[key]) missing.push(key);
  if (config.hosts?.macos?.launchTerminalHotkey === undefined) missing.push('hosts.macos.launchTerminalHotkey');
  if (missing.length) throw new Error(`config ${path} is missing ${missing.join(', ')}`);
  return config;
}

/** Every file under the evidence dir with its hash: the raw artifact manifest. */
function manifest(dir) {
  const files = [];
  const walk = (at) => {
    for (const name of readdirSync(at).sort()) {
      const path = join(at, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (!['manifest.json'].includes(relative(dir, path))) {
        const bytes = readFileSync(path);
        files.push({ path: relative(dir, path), bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') });
      }
    }
  };
  walk(dir);
  return files;
}

/**
 * The session host's run-owned services and the facts every lane shares: relay origin, account
 * sign-in through the file mailer, the decision card the fleet holds, the fixture logs.
 */
async function startSession({ config, run, windows }) {
  const host = config.hosts.windows;
  const helper = rpcProcess(config.sshExecutable ?? 'ssh', ['-T', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=15', host.sshDestination,
    `powershell.exe -NoProfile -NonInteractive -Command "& '${host.bun}' '${host.hostHelper}' --rpc"`], { signal: run.signal });
  const relayRoot = `${host.worktree}\\.native-qa\\${run.runId}-relay`;
  const port = config.relayPort ?? 47930;
  const origin = `http://${host.tailnetAddress ?? host.mahordAddress}:${port}`;
  const tokens = new Map(['macos', 'windows'].map((name) => [config.hosts[name].hostId, `f3-${run.runId}-${name}`]));
  const shared = {
    relayOrigin: origin,
    phoneOrigin: origin,
    phoneUrl: `${origin}/`,
    decisionTitle: 'f3decision', pushTitle: 'f3push', upstreamTitle: 'f3upstream',
    ownerEmail: config.ownerEmail,
    machineToken: (hostId) => tokens.get(hostId),
    desktops: {},
    cross: [],
    observed: {},
    record(scenarioId, lane, value) {
      (shared.observed[scenarioId] ??= {})[lane] = value;
    },
    promptToken: `f3tok${run.runId.replace(/[^a-z0-9]/g, '').slice(-8)}`,
    get prompt() { return `F3 check ${shared.promptToken}: what is waiting on me?`; },
  };
  let relay = null;
  shared.start = async () => {
    relay = await helper.call('spawn-owned', { profile: relayRoot, key: 'relay', executable: host.relay, args: ['--port', String(port)],
      env: { FERRYX_DEPLOYMENT_MODE: 'selfhost', FERRYX_RELAY_MACHINE_TOKENS: [...tokens.values()].join(','), FERRYX_ACCOUNT_DATA_DIR: `${relayRoot}\\account`,
        FERRYX_ACCOUNT_ORIGIN: origin, FERRYX_ACCOUNT_RELAY_ORIGIN: origin, FERRYX_UI_DIST_DIR: host.uiDist, RUST_LOG: 'info' }, ready: 'ferryx-relay listening', deadlineMs: 60_000 });
    windows.write('relay', 'relay-spawn.json', relay);
  };
  const mailDir = `${relayRoot}\\account\\mail`;
  const nextLink = async (before) => {
    const files = await until('magic link mail', () => helper.call('list-files', { profile: relayRoot, dir: mailDir }), (list) => list.some((f) => !before.has(f.name) && f.bytes > 0), { deadlineMs: 60_000 });
    const fresh = files.find((f) => !before.has(f.name) && f.bytes > 0);
    return Buffer.from((await helper.call('read-b64', { profile: relayRoot, path: fresh.path })).base64, 'base64').toString('utf8').trim();
  };
  const mailNames = async () => new Set((await helper.call('list-files', { profile: relayRoot, dir: mailDir })).map((f) => f.name));
  // A desktop joins the account through device authorization (`ferryx-cli account login`),
  // its approve link opened from the file mailer as a person would.
  shared.enroll = async ({ helper: hostHelper, profile, env, cli }) => {
    const before = await mailNames();
    const login = await hostHelper.call('spawn-owned', { profile, key: 'account-login', executable: cli, args: ['account', 'login', '--email', config.ownerEmail, '--origin', origin], env });
    const link = await nextLink(before);
    const approved = await helper.call('http-get', { url: link.replace(origin, `http://127.0.0.1:${port}`) });
    const exit = await hostHelper.call('await-exit', { key: 'account-login' });
    if (approved.status !== 200 || exit.exit?.code !== 0) throw new Error(`account login failed: ${JSON.stringify({ approved: approved.status, exit })}`);
    return { login, approved: approved.status, exit: exit.exit };
  };
  // The phone signs in in Safari: the mail directory is listed BEFORE the iPhone lane taps
  // "send link", and the returned function waits for the one new link.
  shared.phoneSignInLink = async () => { const before = await mailNames(); return () => nextLink(before); };
  const session = () => shared.desktops.windows;
  // Fixture panes on the session host: spawned through the daemon `spawn` op with the wrapper
  // shells host-helper wrote, inside the isolated profile's workspace.
  shared.spawnPanes = async () => {
    const s = session();
    const panes = {};
    for (const mode of ['decision', 'held', 'upstream']) {
      const reply = await s.call(`spawn-${mode}`, [{ type: 'spawn', clientRequestId: randomUUID(), workspaceId: 'f3', worktree: null, cwd: s.setup.worktree, cols: 120, rows: 32, shell: s.wrappers.wrappers[mode].shell }]);
      if (reply?.type !== 'spawnOk') throw new Error(`spawn ${mode}: ${JSON.stringify(reply)}`);
      panes[mode] = reply.sessionId;
    }
    shared.panes = panes;
    windows.write('panes', 'panes.json', panes);
    return panes;
  };
  shared.decisionCard = async () => {
    const s = session();
    const view = await until('the fleet decision card', async () => (await s.call('panel', [{ type: 'supervisorReadV1', request: { view: 'panel' } }]))?.value,
      (v) => (v?.decisions ?? []).some((d) => d.sessionRef.endsWith(`:${shared.panes.decision}`)), { deadlineMs: 180_000 });
    const decision = view.decisions.find((d) => d.sessionRef.endsWith(`:${shared.panes.decision}`));
    return { sessionRef: decision.sessionRef, promptId: decision.promptId, commandFragment: 'prod.example.com', decision };
  };
  const logPath = (mode) => `${session().wrappers.logs}\\agent-${session().wrappers.wrappers[mode].title}.jsonl`;
  shared.agentLog = async (mode, after = 0) => {
    const file = await session().helper.call('read-b64', { profile: session().setup.profile, path: logPath(mode) });
    return Buffer.from(file.base64, 'base64').toString('utf8').split('\n').filter(Boolean).map((line) => JSON.parse(line)).slice(after);
  };
  shared.agentLogCursor = async (mode) => (await shared.agentLog(mode)).length;
  const typeInto = (mode, text) => session().call(`write-${mode}`, [{ type: 'write', sessionId: shared.panes[mode], data: Buffer.from(`${text}\r`).toString('base64') }]);
  shared.releaseHeldDecision = () => typeInto('held', 'F3-DRAW');
  shared.sendLongReport = (token) => typeInto('decision', `F3-REPORT:${token}`);
  shared.pushSubscriptions = async () => {
    const s = session();
    const file = await s.helper.call('read-b64', { profile: s.setup.profile, path: `${s.setup.profile}\\data\\remote\\push-subscriptions.json` }).catch(() => null);
    return file ? (JSON.parse(Buffer.from(file.base64, 'base64').toString('utf8')).subscriptions ?? []) : [];
  };
  shared.servedCandidate = async () => {
    const who = await helper.call('identity', {});
    return { ...who.candidate, servedBy: host.hostId, uiDist: host.uiDist };
  };
  shared.stop = async () => (relay ? [...(await helper.call('cleanup', {})), await helper.call('remove-owned-dir', { profile: relayRoot })] : []);
  shared.close = () => helper.close();
  return shared;
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.validate) {
    const run = JSON.parse(readFileSync(join(opts.validate, 'run.json'), 'utf8'));
    const verdict = validateRun(run);
    process.stdout.write(`${JSON.stringify(verdict, null, 2)}\n`);
    return verdict.exitCode;
  }
  const config = loadConfig(opts.config);
  const evidenceDir = resolve(opts.evidence);
  if (existsSync(evidenceDir) && readdirSync(evidenceDir).length > 0) throw new Error(`${evidenceDir} is not empty; give a new evidence directory`);
  mkdirSync(evidenceDir, { recursive: true });
  const controller = new AbortController();
  process.on('SIGINT', () => controller.abort(new Error('interrupted')));
  const runId = `f3-${new Date().toISOString().replace(/[^0-9]/g, '').slice(0, 14)}-${randomUUID().slice(0, 6)}`;
  const run = { runId, revision: opts.revision, evidenceDir, signal: controller.signal, mahordExecutable: config.mahordExecutable, tesseract: config.tesseract, sshExecutable: config.sshExecutable };
  const lanes = Object.fromEntries(LANES.map((name) => [name, newLane(name, evidenceDir)]));
  const startedAt = new Date().toISOString();
  const shared = await startSession({ config, run, windows: lanes.windows });
  try {
    // Windows first: it is the session host, and its relay and panes are what the others see.
    // An unreachable session host blocks every lane with that exact cause; nothing is skipped.
    try {
      await shared.start();
    } catch (error) {
      lanes.windows.receipt.blocked = { code: 'HOST_UNAVAILABLE', detail: `session host ${config.hosts.windows.sshDestination}: ${error.message}`, at: new Date().toISOString() };
    }
    if (!lanes.windows.receipt.blocked) {
      const windowsDone = runDesktopLane(lanes.windows, { host: config.hosts.windows, run, shared, signal: controller.signal });
      await until('windows daemon up', async () => shared.desktops.windows ?? lanes.windows.receipt.blocked ?? lanes.windows.receipt.failure, Boolean, { deadlineMs: 600_000, gapMs: 2_000 });
      let panesReady = false;
      if (shared.desktops.windows) {
        try { await shared.spawnPanes(); panesReady = true; } catch (error) { lanes.windows.receipt.failure = { code: 'FIXTURE_PANES', detail: String(error.stack ?? error) }; }
      }
      await Promise.all([
        windowsDone,
        panesReady ? runDesktopLane(lanes.macos, { host: config.hosts.macos, run, shared, signal: controller.signal }) : null,
        panesReady ? runIphoneLane(lanes.iphone, { device: config.iphone, run, shared }) : null,
      ]);
    }
    for (const name of ['macos', 'iphone']) {
      if (lanes[name].receipt.steps.length > 0) continue;
      lanes[name].receipt.blocked = { code: 'SESSION_HOST_UNAVAILABLE', detail: `the Windows session host did not provide the relay and fixture panes: ${JSON.stringify(lanes.windows.receipt.blocked ?? lanes.windows.receipt.failure)}`, at: new Date().toISOString() };
      markNotRun(lanes[name], scenariosForLane(name), 'SESSION_HOST_UNAVAILABLE');
    }
    markNotRun(lanes.windows, scenariosForLane('windows'), lanes.windows.receipt.blocked?.code ?? 'session host failed');
  } finally {
    lanes.windows.receipt.cleanup.push(...(await shared.stop().catch((error) => [{ resource: 'relay', action: 'reported-not-killed', error: error.message }])));
    shared.close();
  }
  // Cross-lane: both desktops recorded the same card and each saw the other's prompt pair.
  const d1 = shared.observed['F3-D1'] ?? {};
  if (d1.macos || d1.windows) shared.cross.push({ scenarioId: 'F3-D1', id: 'same-card-both-desktops', ok: Boolean(d1.macos && d1.windows) && JSON.stringify(d1.macos) === JSON.stringify(d1.windows), observed: d1 });
  const d2 = shared.observed['F3-D2'] ?? {};
  if (d2.macos || d2.windows) shared.cross.push({ scenarioId: 'F3-D2', id: 'same-prompt-and-reply-both-desktops', ok: Boolean(d2.macos && d2.windows) && d2.macos.owner === d2.windows.owner && d2.macos.reply === d2.windows.reply, observed: d2 });
  for (const id of ['F3-D3', 'F3-D4']) {
    const both = ['macos', 'windows'].every((name) => lanes[name].receipt.steps.find((s) => s.scenarioId === id)?.status === 'pass');
    shared.cross.push({ scenarioId: id, id: 'both-desktops-pass', ok: both, observed: { macos: lanes.macos.receipt.steps.find((s) => s.scenarioId === id)?.status, windows: lanes.windows.receipt.steps.find((s) => s.scenarioId === id)?.status } });
  }
  const receipt = { schema: RUN_SCHEMA, runId, revision: opts.revision, startedAt, finishedAt: new Date().toISOString(), config: opts.config,
    lanes: Object.fromEntries(LANES.map((name) => [name, { ...lanes[name].receipt, finishedAt: new Date().toISOString() }])), cross: shared.cross };
  writeFileSync(join(evidenceDir, 'run.json'), `${JSON.stringify(receipt, null, 2)}\n`);
  const verdict = validateRun(receipt);
  writeFileSync(join(evidenceDir, 'verdict.json'), `${JSON.stringify(verdict, null, 2)}\n`);
  writeFileSync(join(evidenceDir, 'manifest.json'), `${JSON.stringify({ runId, revision: opts.revision, files: manifest(evidenceDir) }, null, 2)}\n`);
  process.stdout.write(`${JSON.stringify({ f3: { runId, verdict: verdict.verdict, exitCode: verdict.exitCode, evidence: evidenceDir } })}\n`);
  return controller.signal.aborted ? EXIT.INTERRUPTED : verdict.exitCode;
}

if (import.meta.main) {
  let code;
  try { code = await main(); } catch (error) { // no-excuse-ok: catch (CLI boundary)
    process.stderr.write(`supervisor-f3: ${error.stack ?? error}\n${USAGE}\n`);
    code = EXIT.USAGE;
  }
  process.exit(code);
}
