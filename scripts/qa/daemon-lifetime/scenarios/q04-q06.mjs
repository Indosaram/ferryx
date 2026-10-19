// Q04-Q06: an unresponsive, unverifiable or contended owner never causes a stop, reload or second owner.

import path from 'node:path';
import { assertion, block } from '../lib/summary.mjs';
import { supervisorMutations } from '../lib/os-jobs.mjs';
import { evidence, dataConn, controlConn, ownerIdentity, seedLocalSession, nonceRoundTrip, assertSameOwner, SUPERVISED_START_WAIT_MS, DAEMON_READY_TIMEOUT_MS } from './common.mjs';
import { launchGuiAndOwner } from './q00-q03.mjs';

export const LISTENER_STALL = 'daemon-listener-stall';
export const PROBE_FAULTS = Object.freeze(['inspect-denied', 'start-time-mismatch', 'record-unreadable']);

/** Barrier arm file for one named QA fixture (qa_barrier.rs runner contract). */
export function armFile(layout, name, runId, operationId, extra = {}) {
  return { file: path.join(layout.barrier, `${name}.arm.json`), content: JSON.stringify({ name, runId, operationId, ...extra }) };
}

export function releaseFile(layout, name, runId, operationId) {
  return { file: path.join(layout.barrier, `${name}.release.json`), content: JSON.stringify({ name, runId, operationId }) };
}

async function guiLogMutations(ctx, gui) {
  const text = (await ctx.agent.call('readFile', { file: path.join(ctx.layout.logs, `gui-${gui.tag}.log`) })) ?? '';
  return { mutations: supervisorMutations(text.split('\n')), text };
}

async function jobPid(ctx, key) {
  if (ctx.platform === 'windows') return (await ctx.jobs.windowsTree(ctx.stagedRoot)).daemon?.pid ?? null;
  return (await ctx.jobs.inspect(key)).pid;
}

export async function Q04(ctx, result) {
  const op = `q04-${ctx.runId}`;
  const pidBefore = await jobPid(ctx, ctx.keys.A);
  const ownerBefore = await ownerIdentity(ctx);
  const quit = await ctx.gui.quit(ctx.state.gui);
  await evidence(ctx, result, 'gui-quit.json', quit);
  const arm = armFile(ctx.layout, LISTENER_STALL, ctx.runId, op);
  await ctx.agent.call('writeFile', arm);
  const held = path.join(ctx.layout.barrier, `${LISTENER_STALL}.held.json`);
  // A fresh data connection makes the daemon enter the stall (accept, no handshake answer).
  const probe = ctx.agent.call('dataOpen', { runtimeDir: ctx.layout.runtime }).catch((e) => ({ error: e.message }));
  const stalled = await ctx.agent.call('waitPath', { file: held, deadlineMs: 15_000 });
  assertion(result, 'daemon entered DaemonListenerStall', stalled, held);
  if (!stalled) return;
  const gui = await ctx.gui.launch(ctx.artifacts.A);
  ctx.state.gui = gui;
  // The GUI must wait past SUPERVISED_START_WAIT (and up to DAEMON_READY_TIMEOUT) without touching the job.
  const busy = await ctx.agent.call('waitLog', { file: path.join(ctx.layout.logs, `gui-${gui.tag}.log`), pattern: 'DAEMON_BUSY|DaemonBusy', deadlineMs: DAEMON_READY_TIMEOUT_MS + SUPERVISED_START_WAIT_MS + 15_000 });
  const pidDuring = await jobPid(ctx, ctx.keys.A);
  const { mutations, text } = await guiLogMutations(ctx, gui);
  await evidence(ctx, result, 'gui-during-stall.log', text);
  assertion(result, 'client reported DaemonBusy instead of starting anything (carry-over todo 2)', Boolean(busy.matched), busy);
  assertion(result, 'job pid unchanged during the stall', pidBefore !== null && pidBefore === pidDuring, { pidBefore, pidDuring });
  assertion(result, 'zero supervisor mutations in the GUI log', mutations.length === 0, mutations);
  await ctx.agent.call('writeFile', releaseFile(ctx.layout, LISTENER_STALL, ctx.runId, op));
  await probe;
  const conn = await dataConn(ctx);
  const ownerAfter = await ownerIdentity(ctx);
  assertSameOwner(result, ownerBefore, ownerAfter, 'after release');
  await nonceRoundTrip(ctx, conn, ctx.state.localA, result, 'original session after release');
  await evidence(ctx, result, 'after-release.json', { ownerAfter, describe: await conn.describe() });
  await ctx.state.conn.close().catch(() => {});
  ctx.state.conn = conn;
}

/** Closes every QA session through the data protocol and asserts the live count is zero. */
export async function closeAllSessions(ctx, conn) {
  const desc = await conn.describe();
  for (const s of desc.sessions) await conn.call({ type: 'close', sessionId: s.backendId });
  const ctl = await controlConn(ctx);
  const inv = ctl ? await ctl.inventory() : null;
  if (ctl) await ctl.close();
  return inv;
}

export async function Q04b(ctx, result) {
  const key = ctx.keys.A;
  await ctx.gui.quit(ctx.state.gui);
  const inv = await closeAllSessions(ctx, ctx.state.conn);
  assertion(result, 'zero live sessions before idling the job', inv && inv.live_local + inv.live_remote + inv.live_paired === 0, inv);
  const before = await ownerIdentity(ctx);
  const ctl = await controlConn(ctx);
  const exit = ctx.agent.call('waitExit', { pid: before.pid, deadlineMs: 30_000 });
  const retire = await ctl.retireWhenIdle();
  const exited = await exit;
  assertion(result, 'idle daemon retired with exit (RetireWhenIdle ok)', retire?.ok === true && !exited.timedOut, { retire, exited });
  const plist = path.join(ctx.layout.launchAgents, `${ctx.jobs.label(key)}.plist`);
  const statBefore = await ctx.agent.call('stat', { file: plist });
  const idle = await ctx.jobs.inspect(key);
  await evidence(ctx, result, 'job-idle.json', idle);
  assertion(result, 'QA job is loaded and idle (KeepAlive SuccessfulExit=false did not restart it)', idle.state === 'RegisteredIdle', idle.state);
  const { gui, owner, timedOut } = await launchGuiAndOwner(ctx);
  ctx.state.gui = gui;
  const running = await ctx.jobs.inspect(key);
  const statAfter = await ctx.agent.call('stat', { file: plist });
  await evidence(ctx, result, 'job-after.json', { running, owner, statBefore, statAfter });
  assertion(result, 'GUI launch started a new owner', !timedOut && owner && owner.pid !== before.pid, owner);
  assertion(result, 'new owner runs under the QA job', running.state === 'Running' && running.pid === owner?.pid, running);
  assertion(result, 'plist not rewritten (kickstart only, no bootout/bootstrap)', statBefore.mtimeMs === statAfter.mtimeMs, { statBefore, statAfter });
  const { mutations } = await guiLogMutations(ctx, gui);
  assertion(result, 'GUI issued no kickstart -k, bootout or unload', !mutations.some((m) => /-k\b|bootout|unload/.test(m)), mutations);
  const identity = await ownerIdentity(ctx);
  await ctx.agent.call('record', { pid: identity.pid, role: 'daemon-A2', executable: identity.os.executable });
  ctx.state.daemon = identity;
  ctx.state.conn = await dataConn(ctx);
  ctx.state.localA = await seedLocalSession(ctx, ctx.state.conn, 'QA_A');
}

export async function Q05(ctx, result) {
  for (const fault of PROBE_FAULTS) {
    const ownerBefore = await ownerIdentity(ctx);
    const pidBefore = await jobPid(ctx, ctx.keys.A);
    await ctx.gui.quit(ctx.state.gui);
    const gui = await ctx.gui.launch(ctx.artifacts.A, { env: { FERRYX_QA_PROBE_FAULT: fault } });
    ctx.state.gui = gui;
    const logFile = path.join(ctx.layout.logs, `gui-${gui.tag}.log`);
    const unknown = await ctx.agent.call('waitLog', { file: logFile, pattern: `Unknown[^\\n]*${fault}|${fault}[^\\n]*Unknown`, deadlineMs: DAEMON_READY_TIMEOUT_MS + 15_000 });
    const ownerAfter = await ownerIdentity(ctx);
    const pidAfter = await jobPid(ctx, ctx.keys.A);
    const { mutations, text } = await guiLogMutations(ctx, gui);
    await evidence(ctx, result, `${fault}.log`, text);
    assertion(result, `${fault}: typed Unknown logged`, Boolean(unknown.matched), unknown);
    assertion(result, `${fault}: zero supervisor mutations`, mutations.length === 0, mutations);
    assertion(result, `${fault}: job pid unchanged`, pidBefore === pidAfter, { pidBefore, pidAfter });
    assertSameOwner(result, ownerBefore, ownerAfter, fault);
  }
  // The fault env is per-process: relaunch without it for the following scenarios.
  await ctx.gui.quit(ctx.state.gui);
  ctx.state.gui = await ctx.gui.launch(ctx.artifacts.A);
}

/** Every process running a QA executable with --daemon (not the watchdog). */
async function daemonProcesses(ctx) {
  const rows = await ctx.agent.call('table');
  return rows.filter((r) => r.executable && ctx.qaRoots.some((root) => r.executable.toLowerCase().startsWith(root.toLowerCase())) && /--daemon(\s|$|")/.test(r.commandLine) && !/--daemon-supervisor/.test(r.commandLine));
}

export async function Q06(ctx, result) {
  if (!ctx.hooks.forceFallback?.present) {
    block(result, 'HOOK_MISSING', ctx.hooks.forceFallback?.owner ?? 'forceFallback');
    return;
  }
  await ctx.gui.quit(ctx.state.gui);
  await closeAllSessions(ctx, ctx.state.conn);
  const before = await ownerIdentity(ctx);
  const ctl = await controlConn(ctx);
  const exit = ctx.agent.call('waitExit', { pid: before.pid, deadlineMs: 30_000 });
  await ctl.retireWhenIdle();
  const exited = await exit;
  assertion(result, 'owner retired: next admission sees Absent', !exited.timedOut, exited);
  // Three concurrent cold starts: two normal GUIs and one forced onto the detached fallback.
  const launches = await Promise.all([
    ctx.gui.launch(ctx.artifacts.A, { tag: 'q06a' }),
    ctx.gui.launch(ctx.artifacts.A, { tag: 'q06b' }),
    ctx.gui.launch(ctx.artifacts.A, { tag: 'q06c', env: { FERRYX_QA_FORCE_FALLBACK: '1' } }),
  ]);
  const owner = await ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid: before.pid, deadlineMs: DAEMON_READY_TIMEOUT_MS * 2 });
  // Let every launcher finish admission: each GUI reports attached or busy in its log.
  for (const g of launches) {
    await ctx.agent.call('waitLog', { file: path.join(ctx.layout.logs, `gui-${g.tag}.log`), pattern: 'attached|DAEMON_BUSY|handshake ok|Connected', flags: 'i', deadlineMs: DAEMON_READY_TIMEOUT_MS * 2 });
  }
  const daemons = await daemonProcesses(ctx);
  await evidence(ctx, result, 'daemons.json', { launches, owner, daemons, ownersSeen: owner.seen });
  assertion(result, 'exactly one daemon process', daemons.length === 1, daemons.map((d) => d.pid));
  assertion(result, 'owner.json only ever named one new owner', owner.seen.filter((p) => p !== before.pid).length === 1, owner.seen);
  for (const g of launches.slice(1)) await ctx.gui.quit(g);
  ctx.state.gui = launches[0];
  const identity = await ownerIdentity(ctx);
  await ctx.agent.call('record', { pid: identity.pid, role: 'daemon-A3', executable: identity.os.executable });
  ctx.state.daemon = identity;
  ctx.state.conn = await dataConn(ctx);
  ctx.state.localA = await seedLocalSession(ctx, ctx.state.conn, 'QA_A');
}
