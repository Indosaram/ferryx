// Shared scenario context helpers for the daemon-lifetime QA harness.

import path from 'node:path';
import { hjoin } from '../lib/hostpath.mjs';
import { randomBytes } from 'node:crypto';
import { data as D, control as C, liveSessions, compareInventories, identityOf, isLive } from '../lib/wire.mjs';
import { assertion } from '../lib/summary.mjs';

export const SUPERVISED_START_WAIT_MS = 5_000; // client.rs SUPERVISED_START_WAIT
export const DAEMON_READY_TIMEOUT_MS = 30_000; // client.rs DAEMON_READY_TIMEOUT
export const RESPAWN_BACKOFF_MS = 1_000; // supervisor_windows.rs RESPAWN_BACKOFF
/** owner_admission.rs starting_window_ms(): a younger starting.json is a pending start (row 5b). */
export const STARTING_WINDOW_MS = DAEMON_READY_TIMEOUT_MS * 2;

/** A QA owner the harness cannot adopt; code is OWNER_GONE, OWNER_UNCONFIRMED or OWNER_STATE_UNRECOVERABLE. */
export class OwnerStateError extends Error {
  constructor(code, detail) {
    super(`${code}: ${JSON.stringify(detail)}`);
    this.name = 'OwnerStateError';
    this.code = code;
    this.detail = detail;
  }
}

export function nonce(prefix = 'QA') {
  return `${prefix}_${randomBytes(6).toString('hex').toUpperCase()}`;
}

/** Shell line that prints a counter every 100ms, per platform. */
export function counterCommand(platform, tag) {
  // Windows sessions are spawned with seedShell('windows') = powershell (see below), so this
  // PowerShell syntax always meets the shell it was written for.
  if (platform === 'windows') return `for($i=0;;$i++){"${tag}_TICK_$i";Start-Sleep -Milliseconds 100}\r\n`;
  return `i=0; while :; do i=$((i+1)); echo ${tag}_TICK_$i; sleep 0.1; done\n`;
}

/**
 * Shell named in the harness's local Spawn. With shell null the daemon picks pwsh.exe when it is on
 * PATH, else powershell.exe (src-tauri/src/terminal/shell.rs:315-319); cmd.exe only on an explicit
 * "cmd" preference (shell.rs:302-305). The request's shell field (daemon/protocol.rs:535) goes
 * straight to resolve_startup_command (daemon/session_service.rs:2442-2443), so naming powershell
 * pins powershell.exe (shell.rs:298-301) and the Windows commands above never depend on PATH.
 */
export function seedShell(platform) {
  return platform === 'windows' ? 'powershell' : null;
}

/** Wait for a fresh shell's first output (prompt), and for the counter's second tick. */
export const SHELL_READY_TIMEOUT_MS = 30_000;
export const COUNTER_TIMEOUT_MS = 15_000;
/** Received-text tail kept in a seed failure. */
export const SEED_TEXT_TAIL = 4_000;

export class SeedError extends Error {
  constructor(message, detail) {
    super(`${message}: ${JSON.stringify(detail)}`);
    this.code = 'SEED_FAILED';
    this.detail = detail;
  }
}

export function echoCommand(platform, marker) {
  const half = Math.floor(marker.length / 2);
  // Split so the echoed input line never contains the marker itself; only the output does.
  if (platform === 'windows') return `Write-Output ('${marker.slice(0, half)}' + '${marker.slice(half)}')\r\n`;
  return `printf '%s%s\\n' '${marker.slice(0, half)}' '${marker.slice(half)}'\n`;
}

/** Writes raw evidence for a scenario and links it into the result. */
export async function evidence(ctx, result, name, content) {
  const file = path.posix.join(ctx.evidenceDir, `${result.id}-${name}`);
  const text = typeof content === 'string' ? content : JSON.stringify(content, null, 2);
  await ctx.writeEvidence(file, text);
  result.evidence.push(file);
  return file;
}

/** One data connection kept for the scenario; closed by the caller. */
export async function dataConn(ctx, runtimeDir = ctx.layout.runtime, opts = {}) {
  const opened = await ctx.agent.call('dataOpen', { runtimeDir, ...opts });
  const call = (frame, deadlineMs) => ctx.agent.call('call', { handle: opened.handle, frame, deadlineMs });
  return {
    handle: opened.handle,
    handshake: opened.handshake,
    call,
    describe: async () => (await call(D.describeLiveSessions())),
    waitOutput: (sessionId, marker, deadlineMs, { visible = false } = {}) => ctx.agent.call('waitOutput', { handle: opened.handle, sessionId, marker, visible, deadlineMs }),
    close: () => ctx.agent.call('close', { handle: opened.handle }),
  };
}

export async function controlConn(ctx, runtimeDir = ctx.layout.runtime) {
  const opened = await ctx.agent.call('controlOpen', { runtimeDir });
  if (opened.handle === null) return null;
  const call = (frame, deadlineMs) => ctx.agent.call('call', { handle: opened.handle, frame, deadlineMs });
  return {
    handle: opened.handle,
    hello: () => call(C.hello()),
    inventory: () => call(C.inventory()),
    requestUpgrade: (exe, key) => call(C.requestUpgrade(exe, key), 120_000),
    retireWhenIdle: () => call(C.retireWhenIdle()),
    shutdown: (n) => call(C.shutdown(n)),
    waitClosed: (deadlineMs) => ctx.agent.call('waitClosed', { handle: opened.handle, deadlineMs }),
    close: () => ctx.agent.call('close', { handle: opened.handle }),
  };
}

/** Owner identity right now: control Hello pid + OS start time, cross-checked with owner.json. */
export async function ownerIdentity(ctx, runtimeDir = ctx.layout.runtime) {
  const ctl = await controlConn(ctx, runtimeDir);
  const hello = ctl ? await ctl.hello() : null;
  if (ctl) await ctl.close();
  const ownerJson = await ctx.agent.call('readFile', { file: hjoin(runtimeDir, 'owner.json') });
  const owner = ownerJson ? JSON.parse(ownerJson) : null;
  // The raw serialized field: a Windows FILETIME exceeds 2^53, so JSON.parse would round it.
  const ownerStartTime = ownerJson ? (/"startTime"\s*:\s*(\d+)/.exec(ownerJson)?.[1] ?? null) : null;
  const pid = hello?.pid ?? owner?.pid ?? null;
  const os = pid ? await ctx.agent.call('probe', { pid }) : null;
  return { pid, hello, owner, ownerStartTime, os, platform: ctx.platform };
}

const LSTART_MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/**
 * Whether owner.json's startTime (owner_admission.rs process_start_time, decimal string) and the
 * agent's probe (proc.mjs probeIdentity) name the same process start. Units per platform:
 *   linux   - both /proc/<pid>/stat starttime clock ticks: exact.
 *   windows - both CreationTime FILETIME (100ns since 1601; Get-Process StartTime.ToFileTimeUtc): exact.
 *   mac     - record is microseconds since the Unix epoch (pbi_start_tvsec*1e6+usec); the probe is
 *             `ps -o lstart` in host-local time at 1s resolution (the mac agent runs on this host),
 *             so the record's whole seconds must equal the parsed lstart.
 * Returns false for a missing or unparseable value: an unverifiable start time is no proof.
 */
export function ownerStartMatches(platform, recorded, probed) {
  if (recorded === null || recorded === undefined || !/^\d+$/.test(String(recorded))) return false;
  if (probed === null || probed === undefined) return false;
  if (platform === 'linux' || platform === 'windows') return /^\d+$/.test(String(probed)) && BigInt(recorded) === BigInt(probed);
  if (platform === 'mac') {
    const m = /^\w{3} (\w{3})\s+(\d+) (\d+):(\d+):(\d+) (\d{4})$/.exec(String(probed).trim());
    const month = m ? LSTART_MONTHS.indexOf(m[1]) : -1;
    if (month < 0) return false;
    const seconds = new Date(Number(m[6]), month, Number(m[2]), Number(m[3]), Number(m[4]), Number(m[5])).getTime() / 1000;
    return BigInt(recorded) / 1_000_000n === BigInt(seconds);
  }
  return false;
}

/**
 * An owner is adoptable only when its recorded pid runs (OS probe). owner.json outlives its
 * daemon, so the record alone never proves liveness. A control Hello is not required: the daemon
 * spawns owner.json publication (server.rs spawn_owner_publication) before it binds control
 * (server.rs bind_control_endpoint), so a fresh owner may not answer Hello yet. A Hello that does
 * answer must name the same pid. When owner.json names that pid, its recorded startTime must match
 * the probed start time (ownerStartMatches): a reused pid running another process is not the owner.
 */
export function confirmOwner(identity) {
  if (!identity?.pid) return { ok: false, code: 'OWNER_GONE', detail: { pid: null, reason: 'no owner record and no control Hello' } };
  if (!identity.os) return { ok: false, code: 'OWNER_GONE', detail: { pid: identity.pid, hello: Boolean(identity.hello), reason: 'owner pid has no live process' } };
  if (identity.hello && identity.hello.pid !== identity.pid) return { ok: false, code: 'OWNER_UNCONFIRMED', detail: { pid: identity.pid, helloPid: identity.hello.pid ?? null, reason: 'control Hello answered for another pid' } };
  if (identity.owner?.pid === identity.pid && !ownerStartMatches(identity.platform, identity.ownerStartTime, identity.os.startTime)) {
    return { ok: false, code: 'OWNER_UNCONFIRMED', detail: { pid: identity.pid, platform: identity.platform ?? null, recordedStartTime: identity.ownerStartTime ?? null, probedStartTime: identity.os.startTime ?? null, reason: 'owner.json startTime does not match the live process (pid reuse)' } };
  }
  return { ok: true, code: null, detail: { pid: identity.pid } };
}

/** owner_admission.rs StartingRecord (camelCase); unreadable is its own state, as in row 1. */
export function parseStarting(text) {
  if (text === null || text === undefined) return { state: 'none' };
  try {
    const record = JSON.parse(text);
    if (!Number.isInteger(record?.launcherPid) || !Number.isInteger(record?.startedAtUnixMs)) throw new Error('launcherPid/startedAtUnixMs missing');
    return { state: 'present', record, key: `${record.launcherPid}:${record.startedAtUnixMs}` };
  } catch (error) {
    return { state: 'unreadable', error: String(error?.message ?? error) };
  }
}

async function readStarting(ctx, runtimeDir) {
  return parseStarting(await ctx.agent.call('readFile', { file: hjoin(runtimeDir, 'starting.json') }));
}

/**
 * owner.json names no live owner. Decides Absent vs a pending start the way admission does
 * (owner_admission.rs classify_owner rows 5b/6) without comparing host and controller clocks:
 * a present starting.json is pending until the owner record changes or STARTING_WINDOW_MS has
 * passed since this observation; the same record still present after that window is older than
 * the window, so admission ignores it too. Expired keys are kept in ctx.state.expiredStarts so a
 * later call does not wait on the same record again.
 *   absent  - no pending start (none, withdrawn or expired)
 *   owner   - the pending start published owner pid
 *   unknown - unreadable record or a different start replaced it; never treated as retired
 */
export async function awaitPendingStart(ctx, fromPid, runtimeDir = ctx.layout.runtime) {
  const first = await readStarting(ctx, runtimeDir);
  if (first.state === 'none') return { state: 'absent' };
  if (first.state === 'unreadable') return { state: 'unknown', reason: `starting record unreadable: ${first.error}` };
  const expired = (ctx.state.expiredStarts ??= new Set());
  if (expired.has(first.key)) return { state: 'absent', expired: first.key };
  const changed = await ctx.agent.call('waitOwnerChange', { runtimeDir, fromPid, deadlineMs: STARTING_WINDOW_MS });
  if (changed.owner) return { state: 'owner', pid: changed.owner.pid, starting: first.record, seen: changed.seen };
  const again = await readStarting(ctx, runtimeDir);
  if (again.state === 'none') return { state: 'absent', withdrawn: first.key };
  if (again.state === 'present' && again.key === first.key) {
    expired.add(first.key);
    return { state: 'absent', expired: first.key };
  }
  if (again.state === 'unreadable') return { state: 'unknown', reason: `starting record unreadable: ${again.error}` };
  return { state: 'unknown', reason: `a start by launcher pid ${again.record.launcherPid} replaced the pending one`, starting: again.record };
}

/** Drops the scenario's owner state after its owner retired: that connection is dead. */
export async function invalidateOwnerState(ctx) {
  const conn = ctx.state.conn;
  ctx.state.conn = null;
  ctx.state.daemon = null;
  ctx.state.localA = null;
  // The daemon behind this handle exited; closing only releases the agent-side handle.
  if (conn) await conn.close().catch(() => {});
}

/**
 * On a seed timeout: the bounded tail of what the session sent and the daemon's live sessions,
 * written as evidence and carried in the thrown error.
 */
async function seedFailure(ctx, conn, { tag, stage, sessionId, marker, waited, deadlineMs }) {
  const text = String(waited?.text ?? '');
  let describe;
  try { describe = await conn.describe(); } catch (error) { describe = { error: String(error?.message ?? error) }; }
  const detail = {
    tag, stage, sessionId, marker, deadlineMs, shell: seedShell(ctx.platform),
    dsrReplies: waited?.dsrReplies ?? null, replyErrors: waited?.replyErrors ?? [],
    receivedBytes: text.length, receivedTail: text.slice(-SEED_TEXT_TAIL), describe,
  };
  const file = path.posix.join(ctx.evidenceDir, `seed-${tag}-${stage}.json`);
  await ctx.writeEvidence(file, JSON.stringify(detail, null, 2));
  detail.evidence = file;
  return new SeedError(`counter ${tag}: ${stage} timed out`, detail);
}

/**
 * Registers the QA workspace once and spawns a local session running a counter. The counter is
 * written only after the shell's first visible output (its prompt), so input never races shell
 * startup. ConPTY's cursor query (ESC[6n) is answered by the agent's output route
 * (lib/agent.mjs createOutputRoute) and is not visible text, so it never counts as the prompt.
 */
export async function seedLocalSession(ctx, conn, tag) {
  if (!ctx.workspaceRegistered) {
    const reg = await conn.call(D.registerWorkspace(ctx.workspaceId, ctx.repoDir));
    if (reg.type !== 'registerWorkspaceOk') throw new Error(`registerWorkspace: ${JSON.stringify(reg)}`);
    ctx.workspaceRegistered = true;
  }
  const spawned = await conn.call(D.spawnLocal({ clientRequestId: `qa-${tag}-${nonce('R')}`, workspaceId: ctx.workspaceId, cwd: ctx.repoDir, shell: seedShell(ctx.platform) }), 60_000);
  if (spawned.type !== 'spawnOk') throw new Error(`spawn: ${JSON.stringify(spawned)}`);
  const attached = await conn.call(D.attach(spawned.sessionId, 0));
  if (attached.type !== 'attachOk') throw new Error(`attach: ${JSON.stringify(attached)}`);
  const ready = await conn.waitOutput(spawned.sessionId, '', SHELL_READY_TIMEOUT_MS, { visible: true });
  if (ready.timedOut) throw await seedFailure(ctx, conn, { tag, stage: 'shell-ready', sessionId: spawned.sessionId, marker: '', waited: ready, deadlineMs: SHELL_READY_TIMEOUT_MS });
  const write = await conn.call(D.write(spawned.sessionId, counterCommand(ctx.platform, tag)));
  if (write.type !== 'writeOk') throw new Error(`write: ${JSON.stringify(write)}`);
  const first = await conn.waitOutput(spawned.sessionId, `${tag}_TICK_2`, COUNTER_TIMEOUT_MS);
  if (first.timedOut) throw await seedFailure(ctx, conn, { tag, stage: 'counter', sessionId: spawned.sessionId, marker: `${tag}_TICK_2`, waited: first, deadlineMs: COUNTER_TIMEOUT_MS });
  return spawned.sessionId;
}

/** Interrupts the counter (Ctrl-C) so the session takes input again. */
export async function interrupt(conn, sessionId) {
  return conn.call({ type: 'write', sessionId, data: Buffer.from('\u0003').toString('base64') });
}

/**
 * Fresh nonce echoed exactly once in one session (input accepted, output delivered, no replay).
 * The daemon sends output only on a connection that attached the session (server.rs Attach ->
 * pump_session_stream); a data connection opened after a GUI quit/relaunch has no stream yet, so
 * attach first. The agent gives each attach its own stream connection (agent.mjs call()).
 */
export async function nonceRoundTrip(ctx, conn, sessionId, result, label) {
  const attached = await conn.call(D.attach(sessionId, 0));
  assertion(result, `${label}: attach accepted`, attached?.type === 'attachOk', attached?.type === 'attachOk' ? 'attachOk' : attached);
  await interrupt(conn, sessionId);
  const marker = nonce('NONCE');
  const w = await conn.call(D.write(sessionId, echoCommand(ctx.platform, marker)));
  const out = await conn.waitOutput(sessionId, marker, 15_000);
  assertion(result, `${label}: write accepted`, w.type === 'writeOk', w);
  assertion(result, `${label}: nonce ${marker} echoed exactly once`, !out.timedOut && out.count === 1, { count: out.count, timedOut: Boolean(out.timedOut) });
  return marker;
}

/** Pre/post DescribeLiveSessions comparison with assertions. */
export function assertInventoryKept(result, before, after, label) {
  const problems = compareInventories(liveSessions(before), after.sessions);
  assertion(result, `${label}: every live session kept its identity`, problems.length === 0, problems);
  return problems.length === 0;
}

export function assertSameOwner(result, before, after, label) {
  const same = before.pid === after.pid && before.os && after.os && String(before.os.startTime) === String(after.os.startTime);
  assertion(result, `${label}: daemon pid and start time unchanged`, same, { before: { pid: before.pid, start: before.os?.startTime }, after: { pid: after.pid, start: after.os?.startTime } });
  return same;
}

export { identityOf, isLive, liveSessions };

/**
 * Ends the current owner the plan-sanctioned way for QA: quit the GUI, close every QA session,
 * then control RetireWhenIdle (zero live sessions) and await the owner's exit event.
 * A recorded owner without a live process is retired only when no start is pending
 * (awaitPendingStart); a pending start that publishes an owner is retired in turn, once.
 */
export async function retireOwner(ctx, { depth = 0 } = {}) {
  if (ctx.state.gui) { await ctx.gui.quit(ctx.state.gui).catch(() => {}); ctx.state.gui = null; }
  const owner = await ownerIdentity(ctx);
  // owner.json outlives its daemon: a recorded pid that no longer runs is retired only when no
  // start is pending, because a pending start publishes the next owner (row 5b).
  if (!owner.pid || !owner.os) {
    const pending = await awaitPendingStart(ctx, owner.pid);
    if (pending.state === 'absent') return { retired: true, pid: owner.pid, reason: owner.pid ? 'owner pid no longer runs' : 'no owner record', pending };
    if (pending.state === 'unknown') return { retired: false, pid: owner.pid, reason: pending.reason, pending };
    if (depth > 0) return { retired: false, pid: pending.pid, reason: 'another pending start published an owner while retiring', pending };
    return { ...(await retireOwner(ctx, { depth: depth + 1 })), pending };
  }
  if (ctx.state.conn) {
    const desc = await ctx.state.conn.describe().catch(() => ({ sessions: [] }));
    for (const s of desc.sessions ?? []) await ctx.state.conn.call({ type: 'close', sessionId: s.backendId }).catch(() => {});
    await ctx.state.conn.close().catch(() => {});
    ctx.state.conn = null;
  }
  const ctl = await controlConn(ctx);
  if (!ctl) return { retired: false, pid: owner.pid, reason: 'no control endpoint' };
  const exit = ctx.agent.call('waitExit', { pid: owner.pid, deadlineMs: 30_000 });
  const resp = await ctl.retireWhenIdle();
  const exited = await exit;
  return { retired: resp?.ok === true && !exited.timedOut, pid: owner.pid, resp, exited };
}

/** Launches a GUI, waits for a new owner and seeds one local session; records the daemon pid. */
export async function freshOwner(ctx, artifact, { env = {}, tag = 'QA_A', dataVersion = 5 } = {}) {
  const before = await ctx.agent.call('readFile', { file: hjoin(ctx.layout.runtime, 'owner.json') });
  const fromPid = before ? JSON.parse(before).pid : null;
  const gui = await ctx.gui.launch(artifact, { env });
  ctx.state.gui = gui;
  const changed = await ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid, deadlineMs: DAEMON_READY_TIMEOUT_MS * 2 });
  if (!changed.owner) throw new Error(`no new owner after launching ${artifact.name}`);
  const identity = await ownerIdentity(ctx);
  // Seed only a confirmed owner; a published record whose pid already died is OWNER_GONE.
  const confirmed = confirmOwner(identity);
  if (!confirmed.ok) throw new OwnerStateError(confirmed.code, { ...confirmed.detail, artifact: artifact.name, seen: changed.seen });
  await ctx.agent.call('record', { pid: identity.pid, role: `daemon-${artifact.name}`, executable: identity.os.executable });
  ctx.state.daemon = identity;
  ctx.state.conn = await dataConn(ctx, ctx.layout.runtime, { version: dataVersion });
  ctx.state.localA = await seedLocalSession(ctx, ctx.state.conn, tag);
  return identity;
}
