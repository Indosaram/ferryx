// Shared scenario context helpers for the daemon-lifetime QA harness.

import path from 'node:path';
import { randomBytes } from 'node:crypto';
import { data as D, control as C, liveSessions, compareInventories, identityOf, isLive } from '../lib/wire.mjs';
import { assertion } from '../lib/summary.mjs';

export const SUPERVISED_START_WAIT_MS = 5_000; // client.rs SUPERVISED_START_WAIT
export const DAEMON_READY_TIMEOUT_MS = 30_000; // client.rs DAEMON_READY_TIMEOUT
export const RESPAWN_BACKOFF_MS = 1_000; // supervisor_windows.rs RESPAWN_BACKOFF

export function nonce(prefix = 'QA') {
  return `${prefix}_${randomBytes(6).toString('hex').toUpperCase()}`;
}

/** Shell line that prints a counter every 100ms, per platform. */
export function counterCommand(platform, tag) {
  if (platform === 'windows') return `for($i=0;;$i++){"${tag}_TICK_$i";Start-Sleep -Milliseconds 100}\r\n`;
  return `i=0; while :; do i=$((i+1)); echo ${tag}_TICK_$i; sleep 0.1; done\n`;
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
    waitOutput: (sessionId, marker, deadlineMs) => ctx.agent.call('waitOutput', { handle: opened.handle, sessionId, marker, deadlineMs }),
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
  const ownerJson = await ctx.agent.call('readFile', { file: path.join(runtimeDir, 'owner.json') });
  const owner = ownerJson ? JSON.parse(ownerJson) : null;
  const pid = hello?.pid ?? owner?.pid ?? null;
  const os = pid ? await ctx.agent.call('probe', { pid }) : null;
  return { pid, hello, owner, os };
}

/** Registers the QA workspace once and spawns a local session running a counter. */
export async function seedLocalSession(ctx, conn, tag) {
  if (!ctx.workspaceRegistered) {
    const reg = await conn.call(D.registerWorkspace(ctx.workspaceId, ctx.repoDir));
    if (reg.type !== 'registerWorkspaceOk') throw new Error(`registerWorkspace: ${JSON.stringify(reg)}`);
    ctx.workspaceRegistered = true;
  }
  const spawned = await conn.call(D.spawnLocal({ clientRequestId: `qa-${tag}-${nonce('R')}`, workspaceId: ctx.workspaceId, cwd: ctx.repoDir }), 60_000);
  if (spawned.type !== 'spawnOk') throw new Error(`spawn: ${JSON.stringify(spawned)}`);
  const attached = await conn.call(D.attach(spawned.sessionId, 0));
  if (attached.type !== 'attachOk') throw new Error(`attach: ${JSON.stringify(attached)}`);
  const write = await conn.call(D.write(spawned.sessionId, counterCommand(ctx.platform, tag)));
  if (write.type !== 'writeOk') throw new Error(`write: ${JSON.stringify(write)}`);
  const first = await conn.waitOutput(spawned.sessionId, `${tag}_TICK_2`, 15_000);
  if (first.timedOut) throw new Error(`counter ${tag} produced no output`);
  return spawned.sessionId;
}

/** Interrupts the counter (Ctrl-C) so the session takes input again. */
export async function interrupt(conn, sessionId) {
  return conn.call({ type: 'write', sessionId, data: Buffer.from('\u0003').toString('base64') });
}

/** Fresh nonce echoed exactly once in one session (input accepted, output delivered, no replay). */
export async function nonceRoundTrip(ctx, conn, sessionId, result, label) {
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
 */
export async function retireOwner(ctx) {
  if (ctx.state.gui) { await ctx.gui.quit(ctx.state.gui).catch(() => {}); ctx.state.gui = null; }
  const owner = await ownerIdentity(ctx);
  if (!owner.pid) return { retired: true, pid: null };
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
  const before = await ctx.agent.call('readFile', { file: `${ctx.layout.runtime}/owner.json` });
  const fromPid = before ? JSON.parse(before).pid : null;
  const gui = await ctx.gui.launch(artifact, { env });
  ctx.state.gui = gui;
  const changed = await ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid, deadlineMs: DAEMON_READY_TIMEOUT_MS * 2 });
  if (!changed.owner) throw new Error(`no new owner after launching ${artifact.name}`);
  const identity = await ownerIdentity(ctx);
  await ctx.agent.call('record', { pid: identity.pid, role: `daemon-${artifact.name}`, executable: identity.os.executable });
  ctx.state.daemon = identity;
  ctx.state.conn = await dataConn(ctx, ctx.layout.runtime, { version: dataVersion });
  ctx.state.localA = await seedLocalSession(ctx, ctx.state.conn, tag);
  return identity;
}
