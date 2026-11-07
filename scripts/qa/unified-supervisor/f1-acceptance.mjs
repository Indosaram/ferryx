// Scenario `f1-acceptance` (docs/plans/SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md, final
// verification F1): autonomy with no owner input on a fixture fleet of three computers, then one
// irreversible approval that must reach the owner.
//
// Opt-in: it runs only when the config carries `f1Fleet` (see config.schema.json). Without it the
// scenario refuses with PREREQUISITE_MISSING (never PASS) and --all leaves it out.
//
// Topology (every address, port, path and binary comes from config; nothing names a machine):
// - Hub computer (where this runner runs): the isolated hub daemon (ctx.startDaemon, hub on,
//   gateway in localNetwork mode on the product's fixed gateway port), the loopback relay
//   (config.ferryxRelay on a free port, reachable as f1Fleet.relayHost by the targets), a
//   loopback mock Web Push service, and an f1-host-agent.mjs child that owns this computer's
//   fixture provider proxy and pane logs. Its panes: the stalled session, later the approval.
// - targets[0], targets[1]: f1-host-agent.mjs over `ssh -T`, each starting one isolated target
//   daemon (hub off, relay mode, agent control on), paired to the hub through the relay with a
//   machine grant (the product's pairedHostPair). targets[0] runs the four outage sessions behind
//   its proxy; targets[1] runs the 429 session.
// - Relay reachability: the product dials a plaintext relay only on loopback (or RFC1918 with the
//   insecure opt-in; validate_relay_url refuses http on a tailnet 100.x address) and pairs only
//   with an https or loopback-http origin. So the hub pairs through http://127.0.0.1:<port>, and a
//   target with `relayTunnel` gets a harness-owned `ssh -R 127.0.0.1:<port>:127.0.0.1:<port>`,
//   opened once the relay port is known, and dials http://127.0.0.1:<port> on its own computer.
//
// Product read paths the verdict uses (never pane text, never log text):
// - GET /api/v1/supervisor/panel and /api/v1/fleet/sessions with the owner's paired machine
//   Control device token (the phone/desktop panel read path).
// - The owner event socket /api/v1/events (panel.decision / panel.action frames), raw.
// - The hub database supervisor_actions rows, opened read-only.
// - Push: the owner device subscribes a loopback endpoint (FERRYX_PUSH_ALLOW_LOOPBACK_ENDPOINT,
//   honoured by debug builds only) through POST /api/v1/push/subscribe; every POST the daemon's
//   real push sender makes to it is one push.
//
// Timing: the 20 minute outage and the 3 minute 429 reset are real wall-clock time (the behavior
// under test). Every other wait is event driven (journal events, directory watches, sockets)
// with a bounded deadline. The one fixed window is the product's decision-push batch window
// (push_policy DECISION_BATCH_WINDOW, 30 s) after the approval push, observed to prove that no
// second push follows; it is the behavior under test for "exactly one push".
import { createHash, randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, realpathSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { AgentChannel, backoffDelay, RECONNECT_BUDGET_MS } from './f1-agent-link.mjs';
import { fleetTeardownClean } from './f1-process-identity.mjs';
import { HarnessError, isInside, POSIX } from './harness.mjs';

export const F1_REQUIRED_ASSERTIONS = Object.freeze([
  'fleet-agents-ready', 'fleet-same-revision', 'relay-listening', 'hub-open', 'owner-device-paired', 'owner-push-subscribed',
  'owner-event-socket-open', 'targets-paired-by-machine-grant', 'three-computers', 'outage-injected-real-duration',
  'four-outage-sessions-on-one-computer', 'quota-session-reset-three-minutes', 'stall-session-provider-working',
  'all-recovery-sessions-finished', 'outage-sessions-recovered-after-outage', 'quota-resumed-at-reset',
  'stall-cancelled-and-resumed', 'one-log-row-per-incident', 'recovery-rows-closed', 'zero-cards-during-recovery',
  'zero-pushes-during-recovery', 'approval-one-card-with-recommendation', 'approval-not-answered-by-supervisor',
  'approval-exactly-one-push', 'fleet-teardown-clean',
]);

const AGENT = join(dirname(fileURLToPath(import.meta.url)), 'f1-host-agent.mjs');
const REAL_OUTAGE_MS = 20 * 60_000;
const REAL_QUOTA_RESET_MS = 3 * 60_000;
const GATEWAY_PORT = 43821;
// A computer that already runs a production Ferryx daemon holds 43821 on loopback and the harness
// must not stop it, so each target may name its own gateway port. Every other use stays fixed.
const gatewayPortFor = (target) => {
  const port = target?.gatewayPort ?? GATEWAY_PORT;
  if (!Number.isInteger(port) || port < 1024 || port > 65_535) throw new HarnessError('CONFIG_INVALID', `${target?.label}: gatewayPort must be an integer 1024..65535`);
  return port;
};
const PUSH_BATCH_WINDOW_MS = 30_000;
const HUB_WORKSPACE = 'f1-hub';
const TARGET_WORKSPACE = 'f1-target';
// A detached target agent with no attached client for this long tears itself down (runner gone).
// Longer than the reconnect budget, so a reconnecting runner always finds it.
const AGENT_ORPHAN_MS = RECONNECT_BUDGET_MS + 5 * 60_000;
const sleep = (ms) => new Promise((resolveSleep) => setTimeout(resolveSleep, ms));

const now = () => Date.now();

// A target's relay URL: loopback through its harness-owned reverse tunnel, else relayHost.
export const targetRelayUrl = (target, relayHost, port) => `http://${target.relayTunnel === true ? '127.0.0.1' : relayHost}:${port}`;

// The reverse tunnel for one target: the target's loopback <port> forwards to the hub computer's
// relay on the same port. The task-scoped sshOptions (known_hosts, identity) come before -R.
export function relayTunnelArgs(target, port) {
  return ['-T', '-N', '-v', '-o', 'BatchMode=yes', '-o', 'ExitOnForwardFailure=yes', '-o', 'ServerAliveInterval=15', ...(target.sshOptions ?? []),
    '-R', `127.0.0.1:${port}:127.0.0.1:${port}`, target.ssh];
}

// OpenSSH's own confirmation (ssh -v) that the remote listener for exactly `port` is bound.
export function relayTunnelReady(line, port) {
  const match = /remote forward success for: listen 127\.0\.0\.1:(\d+),/.exec(String(line ?? ''));
  return match !== null && Number(match[1]) === port;
}

// One ssh link to a target's detached agent. attempt 0 starts the agent (`--mode start`); every
// later link re-attaches to the same agent and asks for the lines after `after`.
export function agentLinkArgs(target, runId, { after, attempt }) {
  return ['-T', '-o', 'BatchMode=yes', '-o', 'ServerAliveInterval=15', '-o', 'ConnectTimeout=30', ...(target.sshOptions ?? []), target.ssh, target.bun,
    join(target.repoDir, 'scripts', 'qa', 'unified-supervisor', 'f1-host-agent.mjs'), '--attach', '--mode', attempt === 0 ? 'start' : 'reattach',
    '--data-root', target.dataRoot, '--run-id', runId, '--after', String(after), '--orphan-ms', String(AGENT_ORPHAN_MS)];
}

// One f1-host-agent.mjs over an AgentChannel. Each link is a recorded spawn `<id>-link<n>`; the
// channel delivers every agent line once (by oseq) into the run journal as source <id>, so RPC
// replies and pane events are journal waits whatever link carried them. Remote agents reconnect
// after a drop (f1-agent-link.mjs; every drop/attempt/reconnect/loss lands in
// f1/agent-links.jsonl); the hub computer's own stdio agent does not.
async function startAgent(ctx, id, linkSpec, { reconnect }) {
  const record = (entry) => {
    ctx.evidence.appendJsonl('f1/agent-links.jsonl', entry);
    ctx.journal.push({ source: 'agent-link', kind: entry.kind, channel: id, entry });
  };
  const connect = async ({ after, attempt, onLine }) => {
    const { executable, args, env } = linkSpec({ after, attempt });
    let markAttached;
    const attachedSeen = new Promise((resolveAttached) => { markAttached = resolveAttached; });
    const handle = await ctx.spawn(`${id}-link${attempt}`, executable, args, { stdin: 'pipe', graceMs: 20_000, env, onLine: (kind, raw, json) => {
      if (kind !== 'stdout') return;
      let value = json;
      if (value === null) { try { value = JSON.parse(raw.trim()); } catch { return; } }
      if (value?.event === 'agent-attached') { markAttached(value); return; }
      onLine(value, raw);
    } });
    handle.child.stdin.on('error', () => { /* the exit event decides */ });
    let attached = null;
    if (reconnect) {
      let timer;
      try {
        attached = await Promise.race([
          attachedSeen,
          handle.exited.then((exit) => { throw new HarnessError('AGENT_LINK_FAILED', `${id} link ${attempt} exited before attaching (code ${exit.code}, signal ${exit.signal})`); }),
          new Promise((_, rejectTimeout) => { timer = setTimeout(() => rejectTimeout(new HarnessError('AGENT_LINK_FAILED', `${id} link ${attempt} did not attach within 90000ms`)), 90_000); }),
        ]);
      } catch (error) {
        await ctx.terminate(handle, 'agent link did not attach').catch(() => null);
        throw error;
      } finally { clearTimeout(timer); }
    }
    const link = {
      send: (line) => { try { handle.child.stdin.write(`${line}\n`); } catch { /* the exit event decides */ } },
      // 'close' follows the last stdout line, so every line this link carried is delivered first.
      closed: handle.closed.then(() => ({ link: attempt, pid: handle.pid, code: handle.exitInfo?.code ?? null, signal: handle.exitInfo?.signal ?? null })),
      close: () => { try { handle.child.stdin.end(); } catch { /* gone */ } },
    };
    return { link, attached };
  };
  const channel = new AgentChannel({ id, connect, record, sleep, reconnect, deliver: (json, raw) => ctx.journal.push({ source: id, kind: 'stdout', line: raw, json }) });
  const readyCursor = ctx.journal.cursor();
  const opened = await channel.open();
  if (opened) record({ channel: id, kind: 'attached', atMs: Date.now(), at: new Date().toISOString(), link: 0, attached: opened });
  const ready = await ctx.waitFor(`${id} ready`, (e) => e.source === id && e.json?.event === 'agent-ready', { after: readyCursor, deadlineMs: 120_000 });
  let seq = 0;
  const rpc = async (op, payload = {}, deadlineMs = 120_000) => {
    seq += 1;
    const callId = seq;
    const cursor = ctx.journal.cursor();
    const startedMs = now();
    ctx.evidence.appendJsonl('f1/rpc.jsonl', { agent: id, id: callId, op, payload, at: new Date().toISOString() });
    const lostError = (info) => new HarnessError('AGENT_LOST', `${id} ${op}: the agent channel was lost (${info?.reason ?? 'unknown'}); no reply can be observed`);
    if (!channel.send(callId, JSON.stringify({ id: callId, op, ...payload }), { final: op === 'shutdown' })) throw lostError(channel.lostInfo);
    const lost = channel.whenLost().then((info) => { throw lostError(info); });
    lost.catch(() => { /* raced below */ });
    let reply;
    for (;;) {
      try {
        reply = await Promise.race([ctx.waitFor(`${id} ${op}`, (e) => e.source === id && e.json?.id === callId && 'ok' in e.json, { after: cursor, deadlineMs }), lost]);
        break;
      } catch (error) {
        // A drop during the call pauses its clock: the wait is extended (bounded by the
        // reconnect budget) only when the channel dropped after the call was sent.
        const dropped = channel.events.some((e) => e.kind === 'dropped' && e.atMs >= startedMs);
        if (error?.code !== 'DEADLINE_EXCEEDED' || !dropped || now() - startedMs > deadlineMs + channel.budgetMs) throw error;
        ctx.evidence.appendJsonl('f1/rpc.jsonl', { agent: id, id: callId, op, waitExtendedAfterDrop: true, at: new Date().toISOString() });
      }
    }
    ctx.evidence.appendJsonl('f1/rpc.jsonl', { agent: id, id: callId, op, reply: reply.json, at: new Date().toISOString() });
    if (!reply.json.ok) throw new HarnessError(reply.json.error?.code ?? 'AGENT_ERROR', `${id} ${op}: ${reply.json.error?.message}`);
    return reply.json.result;
  };
  return { id, channel, rpc, hostname: ready.json.hostname, agentPid: ready.json.pid };
}

const paneEvents = (ctx, agentId) => ctx.journal.events.filter((e) => e.source === agentId && e.json?.event === 'pane').map((e) => e.json.record);

function waitPane(ctx, agentId, label, predicate, deadlineMs) {
  return ctx.waitFor(label, (e) => e.source === agentId && e.json?.event === 'pane' && predicate(e.json.record), { deadlineMs });
}

// Loopback Web Push endpoint: counts what the daemon's real push sender posts.
function startPushSink(ctx) {
  const received = [];
  const server = Bun.serve({
    hostname: '127.0.0.1',
    port: 0,
    async fetch(request) {
      const body = new Uint8Array(await request.arrayBuffer());
      const entry = { atMs: now(), method: request.method, path: new URL(request.url).pathname, bytes: body.length,
        sha256: createHash('sha256').update(body).digest('hex'), headers: Object.fromEntries(request.headers) };
      received.push(entry);
      ctx.evidence.appendJsonl('f1/push-received.jsonl', entry);
      ctx.journal.push({ source: 'push-sink', kind: 'push', index: received.length, atMs: entry.atMs });
      return new Response(null, { status: 201 });
    },
  });
  return { url: `http://127.0.0.1:${server.port}/push/f1-${ctx.runId}`, received, stop: () => server.stop(true) };
}

async function pushKeys() {
  const pair = await crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, true, ['deriveBits']);
  const raw = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
  return { p256dh: Buffer.from(raw).toString('base64url'), auth: Buffer.from(crypto.getRandomValues(new Uint8Array(16))).toString('base64url') };
}

export async function runF1Acceptance(ctx, deps) {
  const { config } = ctx;
  const fleet = config.f1Fleet;
  if (!fleet) throw new HarnessError('PREREQUISITE_MISSING', 'f1-acceptance needs config.f1Fleet (hub agent root, two ssh target computers, relayHost); it is opt-in');
  if (!POSIX) throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', 'f1-acceptance runs POSIX fixture panes; run the hub computer on Linux or macOS');
  if (typeof config.ferryxRelay !== 'string') throw new HarnessError('CONFIG_INVALID', 'f1-acceptance needs ferryxRelay on the hub computer');
  if (!(await deps.portIsFree(GATEWAY_PORT))) throw new HarnessError('PREREQUISITE_MISSING', `127.0.0.1:${GATEWAY_PORT} is in use on the hub computer; the hub gateway binds that fixed port and nothing is stopped to free it`);
  const outageMs = fleet.outageDurationMs ?? REAL_OUTAGE_MS;
  const quotaResetMs = fleet.quotaResetMs ?? REAL_QUOTA_RESET_MS;
  const data = realpathSync(ctx.paths.data);
  const dbPath = join(data, 'hub', 'hub.sqlite');
  if (!isInside(dbPath, data)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `${dbPath} is outside the run root`);
  const hubSocket = join(data, 'runtime', 'daemon.sock');
  const hubLog = join(data, 'data', 'logs', 'daemon.log');
  const evidence = { schema: 'f1-acceptance/1', runId: ctx.runId, startedAtMs: now(), config: { outageMs, quotaResetMs, relayHost: fleet.relayHost }, hosts: {}, scenarios: {}, panel: [], frames: [], clauses: [] };
  const clause = (name, ok, detail) => { evidence.clauses.push({ clause: name, pass: ok === true, atMs: now(), detail }); return ctx.assert(name, ok === true, detail); };
  const writeEvidence = () => ctx.evidence.writeJson('f1/f1-evidence.json', { ...evidence, finishedAtMs: now() });
  const agents = [];
  let tunnelsClosing = false;
  const pushSink = startPushSink(ctx);
  let socket = null;
  try {
    // 1. Fleet agents: the hub computer's own, then each target over ssh.
    const localRev = (await ctx.run('git-rev', deps.onPath('git') ?? '/usr/bin/git', ['-C', dirname(AGENT), 'rev-parse', 'HEAD'], { deadlineMs: 10_000 })).stdout.trim();
    const hubAgent = await startAgent(ctx, 'agent-hub', () => ({ executable: process.execPath, args: [AGENT], env: ctx.productEnv() }), { reconnect: false });
    agents.push(hubAgent);
    // ssh needs the operator's agent socket and known_hosts; it runs nothing but the agent script.
    const sshEnv = Object.fromEntries(Object.entries({ PATH: process.env.PATH, HOME: process.env.HOME, USER: process.env.USER, SSH_AUTH_SOCK: process.env.SSH_AUTH_SOCK })
      .filter(([, value]) => value !== undefined));
    for (const [index, target] of fleet.targets.entries()) {
      const linkSpec = (link) => ({ executable: fleet.sshBin, args: agentLinkArgs(target, ctx.runId, link), env: sshEnv });
      agents.push(await startAgent(ctx, `agent-${index === 0 ? 'outage' : 'quota'}`, linkSpec, { reconnect: true }));
    }
    const [, outageAgent, quotaAgent] = agents;
    const inits = [];
    inits.push(await hubAgent.rpc('init', { runId: ctx.runId, dataRoot: fleet.hub.agentDataRoot, bun: process.execPath }));
    for (const [index, target] of fleet.targets.entries()) inits.push(await agents[index + 1].rpc('init', { runId: ctx.runId, dataRoot: target.dataRoot, bun: target.bun }));
    const revs = [];
    for (const [index, agent] of agents.entries()) revs.push(await agent.rpc('rev', { ferryxCli: index === 0 ? config.ferryxCli : fleet.targets[index - 1].ferryxCli }));
    clause('fleet-agents-ready', inits.every((init) => typeof init.runRoot === 'string'), { inits });
    clause('fleet-same-revision', revs.every((rev) => rev.head === localRev && rev.dirty === null && typeof rev.ferryxCli?.sha256 === 'string'), { localRev, revs });
    for (const [index, agent] of agents.entries()) {
      evidence.hosts[agent.id] = { role: ['hub', 'outage', 'quota'][index], label: index === 0 ? fleet.hub.label : fleet.targets[index - 1].label, hostname: inits[index].hostname, agentPid: inits[index].agentPid, runRoot: inits[index].runRoot, clockAtInitMs: inits[index].nowMs,
        repo: { root: revs[index].repoRoot, head: revs[index].head, dirty: revs[index].dirty }, ferryxCli: revs[index].ferryxCli };
    }
    const proxies = [];
    for (const agent of agents) proxies.push((await agent.rpc('proxyStart', { quotaResetMs })).url);

    // 2. Relay on the hub computer (recorded pid), reachable by the targets as relayHost.
    const relayPort = await deps.freeLoopbackPort();
    const relayUrl = `http://${fleet.relayHost}:${relayPort}`;
    const machineToken = `f1-${ctx.runId}-${randomUUID()}`;
    const relayCursor = ctx.journal.cursor();
    const relay = await ctx.spawn('relay', config.ferryxRelay, ['--port', String(relayPort)], {
      env: ctx.productEnv({ FERRYX_DEPLOYMENT_MODE: 'selfhost', FERRYX_RELAY_MACHINE_TOKENS: machineToken, FERRYX_ACCOUNT_DATA_DIR: join(data, 'relay-account'), RUST_LOG: 'info' }), cwd: data, graceMs: 5_000,
    });
    await ctx.waitFor('relay listening', (e) => e.source === 'relay' && String(e.line ?? '').includes('ferryx-relay listening'), { after: relayCursor, deadlineMs: 30_000 });
    const hubRelayOrigin = `http://127.0.0.1:${relayPort}`;
    clause('relay-listening', true, { pid: relay.pid, relayUrl, hubRelayOrigin, bind: `0.0.0.0:${relayPort}` });

    // 2b. Reverse tunnels, opened only now that the relay port is known. Each is a recorded spawn
    //     on this computer (torn down by its own handle, reclaimable by --cleanup); the target's
    //     port is checked free first and nothing is stopped to free it.
    const tunnels = [];
    const tunnelRecord = (entry) => {
      const full = { atMs: now(), at: new Date().toISOString(), ...entry };
      ctx.evidence.appendJsonl('f1/agent-links.jsonl', full);
      ctx.journal.push({ source: 'agent-link', kind: entry.kind, channel: entry.channel, entry: full });
    };
    const openTunnel = async (id, args) => {
      const tunnelCursor = ctx.journal.cursor();
      const handle = await ctx.spawn(id, fleet.sshBin, args, { env: sshEnv, graceMs: 5_000 });
      const opened = await ctx.waitFor(`${id} forward`, (e) => e.source === id && (e.kind === 'exit' || relayTunnelReady(e.line, relayPort)), { after: tunnelCursor, deadlineMs: 45_000 }).catch(() => null);
      if (opened === null) await ctx.terminate(handle, 'relay tunnel forward not confirmed').catch(() => null);
      return { handle, ok: opened !== null && opened.kind !== 'exit', exit: opened?.kind === 'exit' ? { code: opened.code, signal: opened.signal } : null };
    };
    // A dropped tunnel (target paused off the network) is re-opened with backoff inside the
    // reconnect budget; every drop and its gap are evidence, and a tunnel not back within the
    // budget fails relay-tunnels-held.
    const keepTunnel = async (t) => {
      for (;;) {
        const exit = await t.handle.exited;
        if (tunnelsClosing) return;
        const drop = { droppedAtMs: now(), exit: { code: exit.code, signal: exit.signal }, attempts: 0, reconnectedAtMs: null };
        t.drops.push(drop);
        tunnelRecord({ channel: t.id, kind: 'dropped', pid: t.handle.pid, exit: drop.exit });
        for (let attempt = 1; drop.reconnectedAtMs === null; attempt += 1) {
          const delay = backoffDelay(attempt - 1);
          if (now() + delay - drop.droppedAtMs > RECONNECT_BUDGET_MS) { t.lost = true; tunnelRecord({ channel: t.id, kind: 'lost', reason: 'reconnect budget spent', attempts: attempt - 1, sinceDropMs: now() - drop.droppedAtMs }); return; }
          await sleep(delay);
          if (tunnelsClosing) return;
          drop.attempts = attempt;
          tunnelRecord({ channel: t.id, kind: 'reconnect-attempt', attempt, afterMs: now() - drop.droppedAtMs });
          try {
            t.respawns += 1;
            const reopened = await openTunnel(`${t.id}-r${t.respawns}`, t.args);
            if (reopened.ok) { t.handle = reopened.handle; drop.reconnectedAtMs = now(); tunnelRecord({ channel: t.id, kind: 'reconnected', attempt, pid: reopened.handle.pid, gapMs: drop.reconnectedAtMs - drop.droppedAtMs }); }
            else tunnelRecord({ channel: t.id, kind: 'reconnect-failed', attempt, exit: reopened.exit });
          } catch (error) { tunnelRecord({ channel: t.id, kind: 'reconnect-failed', attempt, error: String(error?.message ?? error) }); }
        }
      }
    };
    for (const [index, target] of fleet.targets.entries()) {
      const agent = agents[index + 1];
      evidence.hosts[agent.id].relayUrl = targetRelayUrl(target, fleet.relayHost, relayPort);
      if (target.relayTunnel !== true) continue;
      const before = await agent.rpc('portFree', { port: relayPort });
      if (!before.free) throw new HarnessError('PREREQUISITE_MISSING', `${target.label}: 127.0.0.1:${relayPort} is in use there; the relay tunnel cannot bind it and nothing is stopped to free it`);
      const id = `tunnel-${agent.id.replace(/^agent-/, '')}`;
      const args = relayTunnelArgs(target, relayPort);
      const first = await openTunnel(id, args);
      if (!first.ok) throw new HarnessError('PREREQUISITE_MISSING', `${target.label}: the relay tunnel did not open its forward (${JSON.stringify(first.exit ?? 'no confirmation within 45000ms')})`);
      const after = await agent.rpc('portFree', { port: relayPort });
      const tunnel = { id, label: target.label, handle: first.handle, pid: first.handle.pid, args, port: relayPort, listenerOnTarget: after.free === false, drops: [], respawns: 0, lost: false };
      tunnels.push(tunnel);
      void keepTunnel(tunnel);
      evidence.hosts[agent.id].relayTunnel = { pid: first.handle.pid, executable: fleet.sshBin, args, port: relayPort };
    }
    if (tunnels.length > 0) clause('relay-tunnels-open', tunnels.every((t) => t.listenerOnTarget), { tunnels: tunnels.map(({ label, pid, port, listenerOnTarget }) => ({ label, pid, port, listenerOnTarget })) });

    // 3. Hub daemon: owner set-up on a hub-computer fixture repo, hub on, loopback push allowed.
    const hubRepo = await hubAgent.rpc('repo');
    const hubEnv = ctx.productEnv({
      SHELL: inits[0].shell, FERRYX_ALLOW_INSECURE_RELAY: '1', FERRYX_PUSH_ALLOW_LOOPBACK_ENDPOINT: '1',
      FERRYX_HUB_TRACKER_ADDR: `127.0.0.1:${await deps.freeLoopbackPort()}`,
    });
    const setup = await ctx.cli(['supervisor', 'setup', '--owner', config.ownerPrincipal, '--project', 'f1', '--repo-identity', 'qa:f1', '--host', 'local', '--git-common-dir', realpathSync(hubRepo.gitCommonDir), '--json'], { label: 'supervisor-setup', env: hubEnv });
    if (setup.code !== 0) throw new HarnessError('SETUP_FAILED', `supervisor setup failed: ${JSON.stringify(setup.json)} ${setup.stderr}`);
    await ctx.startDaemon({ env: hubEnv, deadlineMs: 60_000 });
    mkdirSync(dirname(hubLog), { recursive: true });
    await deps.waitForChange(ctx, 'hub is up', dirname(hubLog), () => ({ done: existsSync(hubLog) && readFileSync(hubLog, 'utf8').includes('ferryx hub is up') }), 120_000);
    clause('hub-open', true, { hubLog });
    const hubCall = (label, request, deadlineMs) => hubAgent.rpc('call', { socket: hubSocket, request, deadlineMs }, (deadlineMs ?? 60_000) + 10_000)
      .then((reply) => { ctx.evidence.appendJsonl('f1/hub-socket.jsonl', { label, request, reply, at: new Date().toISOString() }); return reply; });
    const registered = await hubCall('hub-register-workspace', { type: 'registerWorkspace', workspaceId: HUB_WORKSPACE, repoRoot: realpathSync(hubRepo.workspaceRoot) });

    // 4. The owner's paired device: panel reads, the event socket and the push subscription.
    const configured = await hubCall('hub-remote-configure', { type: 'remoteConfigure', config: { mode: 'localNetwork', port: 0, allowControl: true } });
    const status = await hubCall('hub-remote-status', { type: 'remoteGetStatus' });
    const bound = status?.status?.boundAddress ?? null;
    if (configured?.type !== 'remoteConfigureOk' || !bound) throw new HarnessError('GATEWAY_UNAVAILABLE', `hub gateway did not start: ${JSON.stringify({ configured, status })}`);
    const base = `http://${bound}`;
    const http = async (label, method, path, { token, body } = {}) => {
      const headers = { accept: 'application/json' };
      if (token) headers.authorization = `Bearer ${token}`;
      if (body !== undefined) headers['content-type'] = 'application/json';
      const response = await fetch(`${base}${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), signal: ctx.signal });
      const text = await response.text();
      let json = null;
      try { json = JSON.parse(text); } catch { /* raw */ }
      const entry = { label, method, path, request: body ?? null, status: response.status, body: json ?? text, atMs: now() };
      ctx.evidence.appendJsonl('f1/http.jsonl', entry);
      return entry;
    };
    const code = await hubCall('owner-pairing-code', { type: 'remoteCreateMachinePairingCode' });
    const paired = await http('owner-pair', 'POST', '/api/v1/pair/exchange', { body: { code: code?.code, deviceName: 'f1-owner-phone', installationId: `f1-owner-${ctx.runId}` } });
    const owner = { token: paired.body?.token ?? null, id: paired.body?.device?.id ?? null };
    const policy = await hubCall('hub-agent-control-on', { type: 'agentControlSettingsSet', enabled: true, excludedDeviceIds: [] });
    clause('owner-device-paired', paired.status === 200 && Boolean(owner.token) && policy?.type === 'agentControlSettingsOk', { status: paired.status, device: paired.body?.device ?? null, policy });
    if (!owner.token) throw new HarnessError('PAIRING_FAILED', 'owner device pairing failed');
    const subscribed = await http('owner-push-subscribe', 'POST', '/api/v1/push/subscribe', { token: owner.token, body: { endpoint: pushSink.url, keys: await pushKeys(), phoneNeedsInput: true, phoneFinishedMode: 'off' } });
    clause('owner-push-subscribed', subscribed.status === 204, { status: subscribed.status, body: subscribed.body, endpoint: pushSink.url, note: 'a 400 here means the hub binary is not a debug build (loopback endpoints are refused in release)' });
    const ticket = await http('owner-socket-ticket', 'POST', '/api/v1/socket-ticket', { token: owner.token, body: { target: '/api/v1/events' } });
    socket = new WebSocket(`ws://${bound}/api/v1/events?ticket=${encodeURIComponent(ticket.body?.ticket ?? '')}`);
    socket.addEventListener('message', (message) => {
      let frame = null;
      try { frame = JSON.parse(String(message.data)); } catch { return; }
      if (!String(frame?.type ?? '').startsWith('panel.')) return;
      const entry = { atMs: now(), frame };
      evidence.frames.push(entry);
      ctx.evidence.appendJsonl('f1/owner-frames.jsonl', entry);
      ctx.journal.push({ source: 'owner-socket', kind: 'frame', type: frame.type, sessionRef: frame.sessionRef ?? frame.action?.sessionRef ?? null, decision: frame.type === 'panel.decision' ? frame.decision !== null : null });
    });
    const opened = await new Promise((resolveOpen) => {
      socket.addEventListener('open', () => resolveOpen(true), { once: true });
      socket.addEventListener('error', () => resolveOpen(false), { once: true });
      setTimeout(() => resolveOpen(false), 30_000);
    });
    // A closed, errored or resynced owner socket may have lost decision frames: any of these
    // during recovery invalidates the zero-cards verdict.
    const socketBreaks = [];
    socket.addEventListener('close', (event) => {
      const entry = { atMs: now(), kind: 'close', code: event.code, reason: event.reason };
      socketBreaks.push(entry);
      ctx.evidence.appendJsonl('f1/owner-frames.jsonl', entry);
    });
    socket.addEventListener('error', () => {
      const entry = { atMs: now(), kind: 'error' };
      socketBreaks.push(entry);
      ctx.evidence.appendJsonl('f1/owner-frames.jsonl', entry);
    });
    clause('owner-event-socket-open', opened && ticket.status === 200, { ticketStatus: ticket.status });
    const panel = async (label) => {
      const read = await http(`panel-${label}`, 'GET', '/api/v1/supervisor/panel', { token: owner.token });
      const fleetRead = await http(`fleet-${label}`, 'GET', '/api/v1/fleet/sessions', { token: owner.token });
      const snapshot = { label, atMs: now(), status: read.status, decisions: read.body?.decisions ?? null, handling: read.body?.handling ?? null, log: read.body?.log ?? null, paused: read.body?.paused ?? null, fleet: fleetRead.body?.sessions ?? null };
      evidence.panel.push(snapshot);
      return snapshot;
    };

    // 5. Targets: isolated daemons in relay mode, paired to the hub with a machine grant.
    const targets = [];
    for (const [index, target] of fleet.targets.entries()) {
      const agent = agents[index + 1];
      const targetUrl = targetRelayUrl(target, fleet.relayHost, relayPort);
      const gatewayPort = gatewayPortFor(target);
      const free = await agent.rpc('portFree', { port: gatewayPort });
      if (!free.free) throw new HarnessError('PREREQUISITE_MISSING', `${target.label}: port ${gatewayPort} is in use there; nothing is stopped to free it`);
      const daemon = await agent.rpc('daemonStart', { ferryxCli: target.ferryxCli, env: { FERRYX_ALLOW_INSECURE_RELAY: '1', FERRYX_MACHINE_TOKEN: machineToken, FERRYX_RELAY_URL: targetUrl } });
      const call = (request) => agent.rpc('call', { request });
      const control = await call({ type: 'agentControlSettingsSet', enabled: true, excludedDeviceIds: [] });
      const relayMode = await call({ type: 'remoteConfigure', config: { mode: 'relay', port: gatewayPort, allowControl: true, relayUrl: targetUrl } });
      await agent.rpc('waitLog', { path: daemon.log, needle: 'relay control channel connected', deadlineMs: 90_000 }, 100_000);
      const pin = await call({ type: 'remoteCreateMachinePairingCode' });
      const pair = await hubCall(`hub-pair-${target.label}`, { type: 'pairedHostPair', request: { relayOrigin: hubRelayOrigin, pin: pin?.code, displayLabel: target.label } }, 90_000);
      const repo = await agent.rpc('repo');
      const ws = await call({ type: 'registerWorkspace', workspaceId: TARGET_WORKSPACE, repoRoot: repo.workspaceRoot });
      targets.push({ label: target.label, agent, daemon, hostId: pair?.host?.hostId ?? null, grant: pair?.host?.grantScope ?? null, repo, control, relayMode, ws });
      evidence.hosts[agent.id].daemon = { pid: daemon.pid, executable: daemon.executable, args: daemon.args, socket: daemon.socket };
      evidence.hosts[agent.id].hostId = pair?.host?.hostId ?? null;
    }
    clause('targets-paired-by-machine-grant', targets.every((t) => t.hostId && t.grant === 'machine' && t.control?.type === 'agentControlSettingsOk' && t.relayMode?.type === 'remoteConfigureOk' && t.ws?.type === 'registerWorkspaceOk') && registered?.type === 'registerWorkspaceOk',
      { targets: targets.map(({ label, hostId, grant, control, relayMode, ws }) => ({ label, hostId, grant, control: control?.type, relayMode: relayMode?.type, ws: ws?.type })), hubWorkspace: registered?.type });

    // 6. Inject the faults. The outage clock starts on targets[0] before its sessions exist, so
    //    each session's first turn already fails; the 429 session's reset starts at its first turn.
    const [outageTarget, quotaTarget] = targets;
    const outage = await outageAgent.rpc('outage', { durationMs: outageMs });
    const outageSessions = [];
    for (let n = 1; n <= 4; n += 1) {
      outageSessions.push(await outageAgent.rpc('spawnPane', { workspaceId: TARGET_WORKSPACE, cwd: outageTarget.repo.repo, role: 'outage', label: `outage-${n}`, proxyUrl: proxies[1] }));
    }
    const quotaSession = await quotaAgent.rpc('spawnPane', { workspaceId: TARGET_WORKSPACE, cwd: quotaTarget.repo.repo, role: 'quota', label: 'quota-1', proxyUrl: proxies[2] });
    const stallSession = await hubAgent.rpc('spawnPane', { socket: hubSocket, workspaceId: HUB_WORKSPACE, cwd: hubRepo.repo, role: 'stall', label: 'stall-1', proxyUrl: proxies[0] });
    const stallWorking = await waitPane(ctx, hubAgent.id, 'stall pane reported working', (r) => r.label === 'stall-1' && r.kind === 'reported' && r.state === 'working', 60_000).then(() => true, () => false);
    const fleetAfterFaults = await panel('after-faults');
    const stallEntry = fleetAfterFaults.fleet?.find?.((s) => s.sessionRef?.endsWith?.(`:${stallSession.sessionId}`)) ?? null;
    // The hub computer's own host id, as the hub's fleet read names it.
    const hubHostId = stallEntry?.hostId ?? null;
    const ref = (hostId, session) => `${hostId}:${session.sessionId}`;
    evidence.scenarios.outage = { host: outageTarget.label, hostId: outageTarget.hostId, injectedAtMs: outage.startedAtMs, endsAtMs: outage.untilMs, durationMs: outageMs, sessions: outageSessions.map((s) => ({ label: s.label, sessionRef: ref(outageTarget.hostId, s), spawnedAtMs: s.spawnedAtMs })) };
    evidence.scenarios.quota = { host: quotaTarget.label, hostId: quotaTarget.hostId, resetMs: quotaResetMs, session: { label: quotaSession.label, sessionRef: ref(quotaTarget.hostId, quotaSession), spawnedAtMs: quotaSession.spawnedAtMs } };
    evidence.scenarios.stall = { host: fleet.hub.label, hostId: hubHostId, session: { label: stallSession.label, sessionRef: hubHostId ? ref(hubHostId, stallSession) : null, sessionId: stallSession.sessionId, spawnedAtMs: stallSession.spawnedAtMs } };
    clause('outage-injected-real-duration', outageMs === REAL_OUTAGE_MS, { outageMs, required: REAL_OUTAGE_MS, injectedAtMs: outage.startedAtMs });
    clause('quota-session-reset-three-minutes', quotaResetMs === REAL_QUOTA_RESET_MS, { quotaResetMs, required: REAL_QUOTA_RESET_MS });
    clause('three-computers', typeof hubHostId === 'string' && hubHostId !== '' && new Set(inits.map((init) => init.hostname)).size === 3
      && targets.every((t) => typeof t.hostId === 'string' && t.hostId !== '') && new Set([hubHostId, ...targets.map((t) => t.hostId)]).size === 3,
      { hostnames: inits.map((init) => init.hostname), hostIds: [hubHostId, ...targets.map((t) => t.hostId)] });
    clause('four-outage-sessions-on-one-computer', outageSessions.length === 4 && new Set(outageSessions.map((s) => s.sessionId)).size === 4, { sessions: evidence.scenarios.outage.sessions });
    clause('stall-session-provider-working', stallWorking && stallEntry?.state === 'working' && stallEntry?.stateSource === 'provider', { stallEntry });

    // 7. Recovery runs with no owner input. Wait (bounded) for every session's own completion.
    const recoveryDeadline = outageMs + 20 * 60_000;
    const doneOf = (agentId, label) => waitPane(ctx, agentId, `${label} done`, (r) => r.label === label && r.kind === 'done', recoveryDeadline).then((e) => e.json.record.atMs, () => null);
    const doneAt = await Promise.all([
      ...outageSessions.map((s) => doneOf(outageAgent.id, s.label)), doneOf(quotaAgent.id, 'quota-1'), doneOf(hubAgent.id, 'stall-1'),
    ]);
    clause('all-recovery-sessions-finished', doneAt.every((at) => Number.isFinite(at)), { doneAt });

    // The incident rows close in place once the scheduler judges the resumed sessions healthy.
    const hub = await deps.openHubReader(dbPath);
    const rows = () => hub.all('SELECT id, incident_key, session_ref, kind, summary, basis, evidence_json, live, created_at, updated_at FROM supervisor_actions ORDER BY id');
    const recoveryRows = () => rows().filter((row) => row.incident_key.startsWith('recovery:'));
    await deps.waitForChange(ctx, 'recovery rows closed', dirname(dbPath), () => {
      const current = recoveryRows();
      return { done: current.length >= 3 && current.every((row) => row.live === 0), current };
    }, 5 * 60_000).catch(() => null);
    const afterRecovery = await panel('after-recovery');
    const recoveryEndMs = now();
    const socketOpenAtRecoveryEnd = socket.readyState === WebSocket.OPEN;

    // Recovery actions, per session, from what each pane received and returned.
    const actionsOf = (agentId, label) => paneEvents(ctx, agentId).filter((r) => r.label === label && ['input', 'turn', 'done', 'reported'].includes(r.kind))
      .map(({ atMs, kind, text, escape, status, attempt, state }) => ({ atMs, kind, text, escape, status, attempt, state }));
    for (const s of evidence.scenarios.outage.sessions) s.recoveryActions = actionsOf(outageAgent.id, s.label);
    evidence.scenarios.quota.session.recoveryActions = actionsOf(quotaAgent.id, 'quota-1');
    evidence.scenarios.stall.session.recoveryActions = actionsOf(hubAgent.id, 'stall-1');
    const outageProxy = await outageAgent.rpc('proxyLog');
    const quotaProxy = await quotaAgent.rpc('proxyLog');
    evidence.scenarios.outage.proxy = outageProxy;
    evidence.scenarios.quota.proxy = quotaProxy;

    const outageOk = evidence.scenarios.outage.sessions.every((s) => {
      const turns = s.recoveryActions.filter((a) => a.kind === 'turn');
      const resumed = s.recoveryActions.some((a) => a.kind === 'input' && a.text === 'continue');
      return turns.some((t) => t.status === 502) && turns.at(-1)?.status === 200 && turns.at(-1).atMs >= outage.untilMs && resumed;
    });
    clause('outage-sessions-recovered-after-outage', outageOk, { outageEndsAtMs: outage.untilMs, sessions: evidence.scenarios.outage.sessions.map((s) => ({ sessionRef: s.sessionRef, turns: s.recoveryActions.filter((a) => a.kind === 'turn') })) });
    const quotaUntil = quotaProxy.quotaUntil['quota-1'];
    const quotaTurns = evidence.scenarios.quota.session.recoveryActions.filter((a) => a.kind === 'turn');
    const firstQuotaOk = quotaTurns.find((t) => t.status === 200) ?? null;
    // Resumed at the reset the 429 named: not before it, and within one minute after it (the
    // scheduler's 15 s tick plus the 30 s resume grace), never by the 10 min fallback.
    clause('quota-resumed-at-reset', Number.isFinite(quotaUntil) && quotaTurns[0]?.status === 429 && quotaTurns.at(-1)?.status === 200
      && firstQuotaOk !== null && firstQuotaOk.atMs >= quotaUntil && firstQuotaOk.atMs <= quotaUntil + 60_000
      && quotaTurns.filter((t) => t.status === 429).length === 1, { quotaUntil, latestAllowedMs: Number.isFinite(quotaUntil) ? quotaUntil + 60_000 : null, firstOkAtMs: firstQuotaOk?.atMs ?? null, turns: quotaTurns });
    const stallInputs = evidence.scenarios.stall.session.recoveryActions.filter((a) => a.kind === 'input');
    const stallStart = evidence.scenarios.stall.session.spawnedAtMs;
    clause('stall-cancelled-and-resumed', stallInputs.some((a) => a.escape) && stallInputs.some((a) => a.text === 'continue') && stallInputs[0]?.atMs - stallStart >= 5 * 60_000,
      { inputs: stallInputs, spawnedAtMs: stallStart, note: 'Stop types Escape (provider interrupt), then Send types the resume text; the stall threshold is 5 min of an unchanged screen' });

    const finalRows = recoveryRows();
    evidence.actionRows = rows();
    const scopes = finalRows.map((row) => row.incident_key.replace(/:g\d+$/, ''));
    const expectedScopes = [`recovery:upstream:${outageTarget.hostId}`, `recovery:quota:${evidence.scenarios.quota.session.sessionRef}`, `recovery:stall:${evidence.scenarios.stall.session.sessionRef}`].sort();
    const panelLogKeys = (afterRecovery.log ?? []).map((entry) => entry.incidentKey).filter((key) => key?.startsWith('recovery:'));
    clause('one-log-row-per-incident', finalRows.length === 3 && JSON.stringify([...scopes].sort()) === JSON.stringify(expectedScopes)
      && new Set(finalRows.map((row) => row.incident_key)).size === 3 && rows().length === 3 && panelLogKeys.length === 3
      && new Set(panelLogKeys).size === 3 && panelLogKeys.every((key) => finalRows.some((row) => row.incident_key === key)),
    { rows: finalRows.map(({ id, incident_key, session_ref, kind, summary, live, created_at, updated_at, evidence_json }) => ({ id, incident_key, session_ref, kind, summary, live, created_at, updated_at, evidence: evidence_json })), expectedScopes, panelLogKeys, allRows: rows().length });
    clause('recovery-rows-closed', finalRows.length === 3 && finalRows.every((row) => row.live === 0) && (afterRecovery.handling ?? []).length === 0, { live: finalRows.map((row) => ({ key: row.incident_key, live: row.live })), handling: afterRecovery.handling });
    const recoveryCardFrames = evidence.frames.filter((f) => f.frame.type === 'panel.decision' && f.frame.decision !== null);
    const panelDecisionCounts = evidence.panel.map((p) => ({ label: p.label, decisions: p.decisions?.length ?? null }));
    const resyncsDuringRecovery = evidence.frames.filter((f) => f.frame.type === 'panel.resync' && f.atMs <= recoveryEndMs);
    const breaksDuringRecovery = socketBreaks.filter((b) => b.atMs <= recoveryEndMs);
    clause('zero-cards-during-recovery', recoveryCardFrames.length === 0 && panelDecisionCounts.every((p) => p.decisions === 0)
      && socketOpenAtRecoveryEnd && breaksDuringRecovery.length === 0 && resyncsDuringRecovery.length === 0,
    { cardFrames: recoveryCardFrames, panelDecisionCounts, socketOpenAtRecoveryEnd, socketBreaks: breaksDuringRecovery, resyncs: resyncsDuringRecovery });
    const recoveryPushes = pushSink.received.filter((p) => p.atMs <= recoveryEndMs);
    clause('zero-pushes-during-recovery', recoveryPushes.length === 0, { pushes: recoveryPushes });

    // 8. The irreversible step: a non-local `sqlx migrate run` approval on a hub-computer pane the
    //    owner's device created through the machine session API (so the push may name it).
    const cardCursor = ctx.journal.cursor();
    const pushCursor = pushSink.received.length;
    await hubAgent.rpc('setMode', { role: 'approval', label: 'approval-1', proxyUrl: proxies[0] });
    const created = await http('approval-session-create', 'POST', '/api/v1/sessions', { token: owner.token, body: {
      requestId: randomUUID(), workspaceId: HUB_WORKSPACE, worktree: null, cols: 120, rows: 40, inheritFromSessionId: null, cwdRelative: 'f1-project', startup: { kind: 'shell' },
    } });
    const approvalDrawn = await waitPane(ctx, hubAgent.id, 'approval drawn', (r) => r.label === 'approval-1' && r.kind === 'approval-drawn', 60_000).catch(() => null);
    const approvalSessionId = approvalDrawn?.json?.record?.sessionId ?? null;
    const approvalRef = hubHostId && approvalSessionId ? `${hubHostId}:${approvalSessionId}` : null;
    const card = await ctx.waitFor('approval card frame', (e) => e.source === 'owner-socket' && e.type === 'panel.decision' && e.decision === true, { after: cardCursor, deadlineMs: 120_000 }).catch(() => null);
    const firstPush = await ctx.waitFor('approval push', (e) => e.source === 'push-sink' && e.index === pushCursor + 1, { after: cardCursor, deadlineMs: PUSH_BATCH_WINDOW_MS + 90_000 }).catch(() => null);
    // The batch window, observed in full after the first push: a second push would land in it.
    await new Promise((resolveWindow) => setTimeout(resolveWindow, PUSH_BATCH_WINDOW_MS + 15_000));
    const afterApproval = await panel('after-approval');
    const decisions = afterApproval.decisions ?? [];
    const only = decisions[0] ?? null;
    const cardFrames = evidence.frames.filter((f) => f.frame.type === 'panel.decision' && f.frame.decision !== null);
    evidence.scenarios.approval = { host: fleet.hub.label, sessionRef: approvalRef, createStatus: created.status, createdAtMs: created.atMs, cardFrameAtMs: card?.at ?? null, firstPushAtMs: firstPush?.atMs ?? null, decisions, cardFrames: cardFrames.length };
    clause('approval-one-card-with-recommendation', decisions.length === 1 && only?.sessionRef === approvalRef && Number.isInteger(only?.recommendedIndex)
      && only.recommendedIndex >= 0 && only.recommendedIndex < (only.prompt?.options?.length ?? 0) && String(only?.reason ?? '').trim() !== ''
      && /sqlx migrate run/.test(`${only?.prompt?.command ?? ''} ${only?.prompt?.question ?? ''}`) && cardFrames.length === 1 && cardFrames[0].frame.sessionRef === approvalRef,
    { createStatus: created.status, approvalRef, decisions, cardFrames });
    const approvalInputs = paneEvents(ctx, hubAgent.id).filter((r) => r.label === 'approval-1' && r.kind === 'input');
    const answeredRows = rows().filter((row) => row.session_ref === approvalRef && row.kind === 'answered');
    clause('approval-not-answered-by-supervisor', approvalRef !== null && approvalInputs.length === 0 && answeredRows.length === 0, { approvalRef, approvalInputs, answeredRows });
    const approvalPushes = pushSink.received.slice(pushCursor);
    clause('approval-exactly-one-push', approvalPushes.length === 1 && pushSink.received.length === 1, { pushes: approvalPushes, windowMs: PUSH_BATCH_WINDOW_MS });
    evidence.pushes = { total: pushSink.received.length, duringRecovery: recoveryPushes.length, afterApproval: approvalPushes.length, received: pushSink.received.map(({ atMs, path, bytes, sha256 }) => ({ atMs, path, bytes, sha256 })) };
    evidence.decisionCards = { duringRecovery: recoveryCardFrames.length, total: cardFrames.length, panelAfterApproval: decisions.length };
    // Every tunnel is up at the end and none stayed down past the reconnect budget. A drop that
    // came back is listed with its gap; what the target missed meanwhile is judged by the
    // product clauses above, never excused here.
    if (tunnels.length > 0) clause('relay-tunnels-held', tunnels.every((t) => !t.lost && t.handle.exitInfo === null), { tunnels: tunnels.map((t) => ({ label: t.label, firstPid: t.pid, pid: t.handle.pid, exit: t.handle.exitInfo, lost: t.lost, drops: t.drops })) });
  } finally {
    // 9. Teardown by recorded identity: each agent stops the daemons it started; ssh/agent
    //    children are then terminated by the harness through their own handles.
    const receipts = [];
    tunnelsClosing = true;
    try { socket?.close(); } catch { /* closed */ }
    for (const agent of [...agents].reverse()) {
      try { receipts.push({ agent: agent.id, ...(await agent.rpc('shutdown', {}, 60_000)) }); } catch (error) { receipts.push({ agent: agent.id, error: String(error?.message ?? error) }); }
    }
    for (const agent of agents) agent.channel.close();
    pushSink.stop();
    evidence.teardown = receipts;
    evidence.agentLinks = agents.map((agent) => ({ agent: agent.id, state: agent.channel.state, lastOseq: agent.channel.lastOseq, links: agent.channel.links, events: agent.channel.events }));
    if (!ctx.signal.aborted && agents.length > 0) {
      const clean = fleetTeardownClean(receipts);
      try { clause('fleet-teardown-clean', clean, { receipts }); } catch { /* recorded late; the run fails */ }
    }
    writeEvidence();
  }
}
