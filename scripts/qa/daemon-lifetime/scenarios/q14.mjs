// Q14: host reboot recovery, simulated without rebooting (plan ferryx-adoption-objections-remediation
// todo 14, f1730fa3). Every pre-boot PTY ends with its owner (the QA job's daemon retires), the
// QA GUI is told the host booted after that owner started (FERRYX_QA_HOST_BOOT_TIME_MS, honored
// only by a local-split-qa build while FERRYX_QA_RUN_ID is set, src-tauri/src/host_boot.rs), then
// the job and the GUI restart. The GUI's own saved session is the pane inventory: a pane still
// bound to its pre-boot backend id is a pane stuck on "Reconnecting".

import { hjoin } from '../lib/hostpath.mjs';
import { assertion, block } from '../lib/summary.mjs';
import { data as D } from '../lib/wire.mjs';
import { evidence, dataConn, ownerIdentity, retireOwner, invalidateOwnerState, requireOwner, nonce, DAEMON_READY_TIMEOUT_MS } from './common.mjs';

export const QA_HOST_BOOT_TIME_ENV = 'FERRYX_QA_HOST_BOOT_TIME_MS';
/** host_boot.rs logs this once the GUI read the overridden boot time. */
export const BOOT_OVERRIDE_RECEIPT = 'QA host boot time override applied';
/** The plan's bound: every pre-boot pane settles within 10 s of the GUI restoring the workspace. */
export const SETTLE_DEADLINE_MS = 10_000;
/** The GUI restores only after its daemon is ready and its project registered. */
export const RESTORE_DEADLINE_MS = DAEMON_READY_TIMEOUT_MS * 2;
export const PRE_BOOT_PANES = 2;
export const STANDBY_PREFIX = 'standby:'; // ui/src/lib/sessionLifecycle.ts STANDBY_BACKEND_PREFIX

const escapeRe = (s) => String(s).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

const isLocalWorkspace = (id, ws) => Boolean(ws) && !String(id).startsWith('ssh:') && !String(id).startsWith('daemon:')
  && ws.target?.kind !== 'ssh' && ws.target?.kind !== 'pairedDaemon';

/**
 * The workspace the pre-boot panes are persisted in: the saved session's active workspace when it
 * is local (the GUI activates it again), else the run's QA workspace.
 */
export function rebootTarget(existing, { workspaceId, repoDir }) {
  const active = existing?.activeWorkspaceId;
  const ws = active ? existing?.workspaces?.[active] : null;
  if (isLocalWorkspace(active, ws) && ws.repoRoot) {
    const root = ws.activeWorktreePath || ws.repoRoot;
    const worktrees = Array.isArray(ws.worktrees) && ws.worktrees.length ? ws.worktrees : [{ path: root, branch: '', head: '', isMain: true, isLocked: false }];
    return { workspaceId: active, repoRoot: ws.repoRoot, worktreePath: root, worktrees, reused: true };
  }
  return { workspaceId, repoRoot: repoDir, worktreePath: repoDir, worktrees: [{ path: repoDir, branch: '', head: '', isMain: true, isLocked: false }], reused: false };
}

/**
 * session_state.json as a GUI left it before the reboot: one shell tab per pre-boot pane, bound
 * to its backend id under the pre-boot owner's daemonEpoch. `seed` is a marker field the GUI's
 * serializer never writes, so its absence proves the GUI restored and re-saved this workspace.
 */
export function preBootSession(existing, { target, panes, daemonEpoch, seed, now }) {
  const base = existing && typeof existing === 'object' ? existing : {};
  const tabs = panes.map((p, i) => ({ id: `q14-tab-${i}-${seed}`, label: 'shell', sessionId: p.localSessionId }));
  const terminalSessions = Object.fromEntries(panes.map((p) => [p.localSessionId, {
    sessionId: p.localSessionId, localSessionId: p.localSessionId, backendSessionId: p.backendSessionId,
    cwd: target.worktreePath, worktreePath: target.worktreePath, createdAt: now,
    daemonEpoch: String(daemonEpoch), incarnation: p.incarnation ?? null, processState: 'running',
  }]));
  return {
    ...base,
    version: 2,
    timestamp: now,
    activeWorkspaceId: target.workspaceId,
    workspaces: {
      ...(base.workspaces ?? {}),
      [target.workspaceId]: {
        workspaceId: target.workspaceId, repoRoot: target.repoRoot, target: { kind: 'local' },
        worktrees: target.worktrees, activeWorktreePath: target.worktreePath,
        layout: { tabs, activeTabId: tabs[0]?.id ?? null, splitMode: 'none', primaryTabId: tabs[0]?.id ?? null, secondaryTabId: null },
        terminalSessions,
        qaRebootSeed: seed,
      },
    },
  };
}

/** Matches a non-empty saved session that no longer carries `text` anywhere. */
export function absentPattern(texts) {
  return `^(?![\\s\\S]*(?:${texts.map(escapeRe).join('|')}))[\\s\\S]*"timestamp"`;
}

/**
 * Classifies each pre-boot pane from the GUI's saved session:
 *   exited       - unbound (null) or a frontend standby id: the pane is exited and offers a new shell
 *   replaced     - bound to a different backend the current owner lists live: a new shell opened
 *   reconnecting - still bound to its pre-boot backend id: the stuck "Reconnecting" pane
 *   other        - bound to an id the current owner does not list live
 *   missing      - the pane is gone from the saved workspace
 */
export function classifyPreBootPanes(saved, { workspaceId, panes, liveIds }) {
  const sessions = saved?.workspaces?.[workspaceId]?.terminalSessions ?? {};
  const live = new Set(liveIds);
  const rows = panes.map((p) => {
    const entry = sessions[p.localSessionId];
    const bound = entry ? entry.backendSessionId ?? null : undefined;
    let state;
    if (entry === undefined) state = 'missing';
    else if (bound === null || String(bound).startsWith(STANDBY_PREFIX)) state = 'exited';
    else if (bound === p.backendSessionId) state = 'reconnecting';
    else if (live.has(bound)) state = 'replaced';
    else state = 'other';
    return { localSessionId: p.localSessionId, preBootBackendId: p.backendSessionId, bound: bound ?? null, state };
  });
  const count = (s) => rows.filter((r) => r.state === s).length;
  return { rows, counts: { preBoot: rows.length, exited: count('exited'), replaced: count('replaced'), reconnecting: count('reconnecting'), other: count('other'), missing: count('missing') } };
}

const parse = (text) => { try { return text ? JSON.parse(text) : null; } catch { return null; } };

export async function Q14(ctx, result) {
  if (!requireOwner(ctx, result)) return;
  if (!ctx.hooks.qaHostBootOverride?.present) return block(result, 'HOOK_MISSING', ctx.hooks.qaHostBootOverride?.owner ?? 'qaHostBootOverride');
  const conn = ctx.state.conn;
  if (!ctx.workspaceRegistered) {
    const reg = await conn.call(D.registerWorkspace(ctx.workspaceId, ctx.repoDir));
    if (!assertion(result, 'QA workspace registered with the pre-boot owner', reg?.type === 'registerWorkspaceOk', reg)) return;
    ctx.workspaceRegistered = true;
  }
  // Pre-boot sessions: plain local shells of the current QA owner.
  const spawned = [];
  for (let i = 0; i < PRE_BOOT_PANES; i += 1) {
    spawned.push(await conn.call(D.spawnLocal({ clientRequestId: `qa-q14-${nonce('R')}`, workspaceId: ctx.workspaceId, cwd: ctx.repoDir }), 60_000));
  }
  if (!assertion(result, `${PRE_BOOT_PANES} pre-boot local sessions spawned`, spawned.every((s) => s.type === 'spawnOk'), spawned)) return;
  const daemonEpoch = spawned[0].epoch;
  const before = await conn.describe();
  const panes = spawned.map((s, i) => ({
    localSessionId: `q14-pane-${i}-${ctx.runId}`, backendSessionId: s.sessionId, incarnation: s.session?.incarnation ?? null,
    childPid: before.sessions?.find((x) => x.backendId === s.sessionId)?.localChildPid ?? null,
  }));
  await evidence(ctx, result, 'pre-boot.json', { daemonEpoch, panes, describe: before });
  assertion(result, 'every pre-boot session has a local child pid', panes.every((p) => p.childPid), panes);

  // (a) Stop the isolated QA job: quit the GUI, close every session, RetireWhenIdle. Every
  // pre-boot PTY ends, as it would with the host.
  const retired = await retireOwner(ctx);
  await invalidateOwnerState(ctx);
  if (!assertion(result, 'pre-boot QA owner retired (job stopped)', retired.retired, retired)) return;
  const childExits = [];
  for (const p of panes.filter((x) => x.childPid)) childExits.push({ pid: p.childPid, exit: await ctx.agent.call('waitExit', { pid: p.childPid, deadlineMs: 10_000 }) });
  assertion(result, 'every pre-boot PTY child exited', childExits.every((c) => !c.exit?.timedOut), childExits);

  // The GUI's session as it would be on disk at boot: written only after the GUI quit.
  const sessionFile = hjoin(ctx.layout.sessions, 'session_state.json');
  const existing = parse(await ctx.agent.call('readFile', { file: sessionFile }));
  const target = rebootTarget(existing, { workspaceId: ctx.workspaceId, repoDir: ctx.repoDir });
  const seed = nonce('Q14SEED');
  const seeded = preBootSession(existing, { target, panes, daemonEpoch, seed, now: Date.now() });
  await ctx.agent.call('mkdir', { dir: ctx.layout.sessions });
  await ctx.agent.call('writeFile', { file: sessionFile, content: JSON.stringify(seeded) });
  await evidence(ctx, result, 'seeded-session.json', { target, seeded });

  // (b) The simulated boot: now, on the host's clock, after the pre-boot owner started.
  const hostNow = Number((await ctx.agent.call('exec', { command: 'node', args: ['-e', 'process.stdout.write(String(Date.now()))'] })).stdout.trim());
  assertion(result, 'simulated boot time is after the pre-boot daemon epoch', Number.isSafeInteger(hostNow) && hostNow > Number(daemonEpoch), { hostNow, daemonEpoch });

  // (c) Restart the job, then the GUI with the overridden boot time.
  const started = await ctx.jobs.start(ctx.keys.A);
  const changed = await ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid: retired.pid, deadlineMs: DAEMON_READY_TIMEOUT_MS });
  if (!assertion(result, 'QA job restarted a new owner', Boolean(changed.owner), { started, changed })) return;
  const owner = await ownerIdentity(ctx);
  if (!assertion(result, 'the restarted owner runs', Boolean(owner.pid && owner.os), { pid: owner.pid })) return;
  await ctx.agent.call('record', { pid: owner.pid, role: 'daemon-q14', executable: owner.os.executable });
  ctx.state.daemon = owner;
  ctx.state.conn = await dataConn(ctx);
  const launchedAt = Date.now();
  ctx.state.gui = await ctx.gui.launch(ctx.artifacts.A, { env: { [QA_HOST_BOOT_TIME_ENV]: String(hostNow) } });

  // (d) The GUI restored the seeded workspace (its marker is gone from the saved session), then
  // within SETTLE_DEADLINE_MS no saved pane is still bound to a pre-boot backend id.
  const restored = await ctx.agent.call('waitLog', { file: sessionFile, pattern: absentPattern([seed]), deadlineMs: RESTORE_DEADLINE_MS });
  const restoredAt = Date.now();
  const guiLog = hjoin(ctx.layout.logs, `gui-${ctx.state.gui.tag}.log`);
  const override = await ctx.agent.call('waitLog', { file: guiLog, pattern: BOOT_OVERRIDE_RECEIPT, deadlineMs: 5_000 });
  assertion(result, 'the QA GUI read the overridden host boot time', Boolean(override.matched), { env: QA_HOST_BOOT_TIME_ENV, hostNow, matched: override.matched ?? null });
  if (!restored.matched) {
    const saved = parse(await ctx.agent.call('readFile', { file: sessionFile }));
    await evidence(ctx, result, 'not-restored.json', { target, savedActive: saved?.activeWorkspaceId ?? null, saved });
    if (saved && saved.activeWorkspaceId && saved.activeWorkspaceId !== target.workspaceId) {
      return block(result, 'RESTORE_TARGET_NOT_ACTIVE', `the GUI activated ${saved.activeWorkspaceId}, not the seeded ${target.workspaceId}`);
    }
    assertion(result, `the GUI restored and re-saved the seeded workspace within ${RESTORE_DEADLINE_MS} ms`, false, { target: target.workspaceId });
    return;
  }
  assertion(result, 'the GUI restored and re-saved the seeded workspace', true, { afterLaunchMs: restoredAt - launchedAt });
  const settled = await ctx.agent.call('waitLog', { file: sessionFile, pattern: absentPattern(panes.map((p) => p.backendSessionId)), deadlineMs: SETTLE_DEADLINE_MS });
  const settledAt = Date.now();
  const saved = parse(await ctx.agent.call('readFile', { file: sessionFile }));
  const after = ctx.state.conn ? await ctx.state.conn.describe() : null;
  const liveIds = (after?.sessions ?? []).filter((s) => s.readerOpen === true).map((s) => s.backendId);
  const verdict = classifyPreBootPanes(saved, { workspaceId: target.workspaceId, panes, liveIds });
  result.identities = { ...(result.identities ?? {}), panes: verdict.counts };
  await evidence(ctx, result, 'panes.json', { verdict, settledMatched: Boolean(settled.matched), restoredAfterLaunchMs: restoredAt - launchedAt, settleMs: settledAt - restoredAt, liveIds });
  await evidence(ctx, result, 'saved-session.json', saved ?? {});
  assertion(result, 'no pre-boot backend id is live in the restarted owner', !panes.some((p) => liveIds.includes(p.backendSessionId)), { liveIds });
  assertion(result, 'zero panes stuck on Reconnecting (bound to a pre-boot backend id)', verdict.counts.reconnecting === 0, { reconnecting: verdict.counts.reconnecting, panes: verdict.rows.filter((r) => r.state === 'reconnecting') });
  assertion(result, `every pre-boot pane exited and offers a new shell within ${SETTLE_DEADLINE_MS} ms of the restore`, Boolean(settled.matched) && verdict.counts.exited + verdict.counts.replaced === verdict.counts.preBoot, { counts: verdict.counts, rows: verdict.rows, settleMs: settledAt - restoredAt });
}
