// Q14: host reboot recovery, simulated without rebooting (plan ferryx-adoption-objections-remediation
// todo 14, f1730fa3). Every pre-boot PTY ends with its owner (the QA job's daemon retires), the
// QA GUI is told the host booted after that owner started (FERRYX_QA_HOST_BOOT_TIME_MS, honored
// only by a local-split-qa build while FERRYX_QA_RUN_ID is set, src-tauri/src/host_boot.rs), then
// the job and the GUI restart. The GUI's own saved session is the pane inventory: a pane still
// bound to its pre-boot backend id is a pane stuck on "Reconnecting".

import { hjoin } from '../lib/hostpath.mjs';
import { assertion, block } from '../lib/summary.mjs';
import { data as D } from '../lib/wire.mjs';
import { evidence, dataConn, liveDataConn, ownerIdentity, retireOwner, invalidateOwnerState, requireOwner, nonce, DAEMON_READY_TIMEOUT_MS, workspaceCatalogFile, webviewProjectKeys, webviewLocalStorageFile } from './common.mjs';

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

/** Paths of the daemon's pending SSH pairing requests (`sessions/remote-request-*.json`). */
async function listRemoteRequests(agent, sessionsDir) {
  const entries = await agent.call('listDir', { dir: sessionsDir }).catch(() => null);
  if (Array.isArray(entries)) return entries.filter((n) => /^remote-request-.*\.json$/.test(n)).map((n) => hjoin(sessionsDir, n));
  return [];
}

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
 * The GUI's persisted project list as this reboot's host would have it: only the seeded
 * workspace, active. The session file alone does not decide what the GUI restores: App.tsx
 * merges the recovered workspace into the persisted project list only when that list is a lone
 * startup/default entry (mergeRecoveredProjectBootstrap), and otherwise activates the first
 * stored project. Scenarios share one webview profile, so a project an earlier scenario added
 * (Q11 leaves its SSH workspace there) survives the simulated reboot, wins the restore, and the
 * seeded workspace is never activated. A reboot premise is a host that boots with this run's
 * workspace only, so the profile is reset to it.
 */
export function seededProjectBootstrap(target) {
  return {
    projects: [{
      workspaceId: target.workspaceId,
      repoRoot: target.repoRoot,
      gitRoot: target.repoRoot,
      gitRemote: null,
      target: { kind: 'local' },
    }],
    activeProjectId: target.workspaceId,
  };
}

/**
 * Points the GUI's persisted project list at the seeded workspace: the state a host that boots with
 * only this run's workspace has. The webview keeps that list in its own localStorage SQLite
 * database (ItemTable, values as UTF-16LE blobs) and App.tsx activates from it before it consults
 * the saved session, so seeding a session without seeding this list restores whatever project an
 * earlier scenario left behind. Written through the host's sqlite3 so the value is a real blob.
 */
async function resetWebviewProjects(ctx, target) {
  const files = webviewLocalStorageFile(ctx);
  const keys = webviewProjectKeys();
  const bootstrap = seededProjectBootstrap(target);
  const present = await ctx.agent.call('exists', { file: files.db });
  const out = { file: files.db, present: Boolean(present), wrote: false, before: null, after: null, error: null };
  if (!present) return out;
  // UTF-16LE like WebKit writes it; sqlite3 takes the bytes as a hex blob literal.
  const utf16 = (text) => {
    const bytes = Buffer.from(String(text), 'utf16le');
    let hex = '';
    for (const byte of bytes) hex += byte.toString(16).padStart(2, '0');
    return "X'" + hex + "'";
  };
  const quote = (text) => String(text).replace(/'/g, "''");
  const readKey = async (key) => (await ctx.agent.call('exec', {
    command: 'sqlite3',
    args: [files.db, "select hex(value) from ItemTable where key='" + quote(key) + "'"],
  })).stdout.trim();
  const decode = (hex) => (hex ? Buffer.from(hex, 'hex').toString('utf16le') : '');
  out.before = { projects: decode(await readKey(keys.projects)), activeProject: decode(await readKey(keys.activeProject)) };
  const write = await ctx.agent.call('exec', {
    command: 'sqlite3',
    args: [files.db, [
      "insert or replace into ItemTable (key, value) values ('" + quote(keys.projects) + "', " + utf16(JSON.stringify(bootstrap.projects)) + ");",
      "insert or replace into ItemTable (key, value) values ('" + quote(keys.activeProject) + "', " + utf16(bootstrap.activeProjectId) + ");",
    ].join(' ')],
  });
  out.write = { status: write.status, stderr: String(write.stderr ?? '').slice(0, 400) };
  out.after = { projects: decode(await readKey(keys.projects)), activeProject: decode(await readKey(keys.activeProject)) };
  out.wrote = write.status === 0 && out.after.activeProject === bootstrap.activeProjectId
    && out.after.projects === JSON.stringify(bootstrap.projects);
  if (!out.wrote) out.error = 'the webview project list still names another active project';
  return out;
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
  const conn = await liveDataConn(ctx);
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
  // Scenarios share one sessions dir, so a file left by an earlier scenario (Q11 leaves its SSH
  // workspace there) would be adopted as this reboot's pre-boot state and its unrelated panes
  // keep the seed marker alive. Q14's premise is a host that boots with only the QA workspace, so
  // start from an empty save instead of whatever ran before.
  const previous = parse(await ctx.agent.call('readFile', { file: sessionFile }));
  await evidence(ctx, result, 'previous-session.json', { previous: previous ? { activeWorkspaceId: previous.activeWorkspaceId ?? null, workspaces: Object.keys(previous.workspaces ?? {}).length } : null });
  await ctx.agent.call('writeFile', { file: sessionFile, content: JSON.stringify({ version: 0, activeWorkspaceId: null, workspaces: {} }) });
  // Q08b/Q13 set the HEAD workspace catalog aside for an older daemon that reads version 1 only,
  // and never put it back. The rebooted GUI cannot activate the seeded workspace without it, so
  // restore the set-aside copy before seeding: the reboot premise is a host that boots with the
  // workspace catalog it had.
  const catalogFile = workspaceCatalogFile(ctx);
  if ((await ctx.agent.call('readFile', { file: catalogFile })) === null) {
    for (const label of ['Q08b', 'Q13']) {
      const kept = hjoin(ctx.layout.root, 'set-aside', `${label}-machine-workspaces.v1.json`);
      const text = await ctx.agent.call('readFile', { file: kept });
      if (text === null) continue;
      await ctx.agent.call('writeFile', { file: catalogFile, content: text });
      await evidence(ctx, result, 'catalog-restored.json', { label, file: catalogFile });
      break;
    }
  }
  // The daemon's own remote-project registry outlives the GUI: Q11's SSH pairing is persisted
  // in data/remote_projects.json and the rebooted owner restores it, so the boot window shows
  // the SSH workspace's agent panes and a "Restart pane" modal stops the session save before
  // it lands (restore-window screenshots, runs 20261110t224537z / 20261111t040857z). The reboot
  // premise is a host with only this run's local workspace, so the registry starts empty.
  const remoteProjects = hjoin(ctx.layout.data, 'remote_projects.json');
  const prevRemote = await ctx.agent.call('readFile', { file: remoteProjects });
  // Q11's SSH pairing request also outlives the GUI: the daemon keeps sessions/remote-request-*.json
  // and offers it to the boot window, whose pairing screen is long expired ("The verification time
  // has expired") and sits over the workspace (restore-window screenshot, run 20261111t045256z -
  // Escape does not dismiss it). A fresh host has no pending request.
  // No shell here: `ls` globbing is a POSIX assumption the Windows target cannot meet, and the
  // agent's own file methods answer the same question on every platform.
  const requests = await listRemoteRequests(ctx.agent, ctx.layout.sessions);
  for (const file of requests) await ctx.agent.call('remove', { target: file });
  await evidence(ctx, result, 'remote-requests-reset.json', { removed: requests });
  await evidence(ctx, result, 'remote-projects-reset.json', { file: remoteProjects, before: prevRemote ? prevRemote.slice(0, 400) : null });
  const existing = null;
  const target = rebootTarget(existing, { workspaceId: ctx.workspaceId, repoDir: ctx.repoDir });
  const seed = nonce('Q14SEED');
  const seeded = preBootSession(existing, { target, panes, daemonEpoch, seed, now: Date.now() });
  await ctx.agent.call('mkdir', { dir: ctx.layout.sessions });
  await ctx.agent.call('writeFile', { file: sessionFile, content: JSON.stringify(seeded) });
  await evidence(ctx, result, 'seeded-session.json', { target, seeded });

  // The GUI's own project list decides what it activates at boot: App.tsx merges the recovered
  // workspace into that list only while the list is a lone startup/default entry, and otherwise
  // activates its first project. Scenarios share one webview profile, so a project an earlier
  // scenario registered (Q11 leaves its SSH workspace) would win this restore and the seeded
  // workspace would never be activated. Reset the profile to the reboot premise: this run's
  // workspace, active.
  const profile = await resetWebviewProjects(ctx, target);
  await evidence(ctx, result, 'profile-projects.json', profile);

  // (b) The simulated boot: now, on the host's clock, after the pre-boot owner started.
  const hostNow = Number((await ctx.agent.call('exec', { command: 'node', args: ['-e', 'process.stdout.write(String(Date.now()))'] })).stdout.trim());
  assertion(result, 'simulated boot time is after the pre-boot daemon epoch', Number.isSafeInteger(hostNow) && hostNow > Number(daemonEpoch), { hostNow, daemonEpoch });

  // (c) Restart the job, then the GUI with the overridden boot time.
  // A simulated boot starts the generation whose launchd job is still on disk. Q07/Q08 publish B and
  // C, and the product's supervisor keeps only the newest generation, so A's plist and its runtime
  // dir are gone by the time Q14 runs; bootstrapping a removed generation is not what a reboot does.
  // Prefer A when it survived (an isolated Q00+Q14 run), else the newest generation still installed.
  const bootCandidates = ['A', 'bump', 'B', 'C'].filter((k) => ctx.keys[k]);
  let bootKey = bootCandidates[0];
  for (const k of bootCandidates) {
    const plist = `${ctx.layout.launchAgents}/${ctx.jobs.label(ctx.keys[k])}.plist`;
    if (await ctx.agent.call('exists', { file: plist })) { bootKey = k; break; }
  }
  const started = await ctx.jobs.start(ctx.keys[bootKey]);
  const changed = await ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid: retired.pid, deadlineMs: DAEMON_READY_TIMEOUT_MS });
  if (!assertion(result, 'QA job restarted a new owner', Boolean(changed.owner), { bootKey, started, changed })) return;
  const owner = await ownerIdentity(ctx);
  if (!assertion(result, 'the restarted owner runs', Boolean(owner.pid && owner.os), { pid: owner.pid })) return;
  await ctx.agent.call('record', { pid: owner.pid, role: 'daemon-q14', executable: owner.os.executable });
  ctx.state.daemon = owner;
  ctx.state.conn = await dataConn(ctx);
  const launchedAt = Date.now();
  // Every earlier scenario leaves its GUI running (Q11's last window is alive at this point),
  // and the Tauri single-instance plugin keys on the session bus: the boot GUI would detect the
  // leftover instance, forward to it and exit before ever mapping a window - restore-window-list.txt
  // (run 20261111t073303z) shows the desktop with no QA GUI window at failure time. A reboot
  // premise has no GUI running, so the run's own GUI scopes are stopped first (harness-owned).
  const stopLeftovers = await ctx.agent.call('exec', { command: 'systemctl', args: ['--user', 'stop', 'ferryx-qa-' + ctx.runId + '-gui-g*.scope'] });
  await evidence(ctx, result, 'leftover-gui-stop.json', { status: stopLeftovers.status, stderr: String(stopLeftovers.stderr ?? '').slice(0, 200) });
  ctx.state.gui = await ctx.gui.launch(ctx.artifacts[bootKey], { env: { [QA_HOST_BOOT_TIME_ENV]: String(hostNow) } });

  // The webview profile is shared across scenarios, so Q11's SSH pairing state re-opens in the
  // boot window as an expired-QR modal ("The verification time has expired") that sits over the
  // workspace and stops the restore save (restore-window screenshot, run 20261111t044248z).
  // Escape on a bare window is a no-op, so dismissing blind is safe; a fresh host has no modal
  // and loses nothing.
  let dismissed = 0;
  if (ctx.mahord) {
    for (let i = 0; i < 2; i += 1) {
      await ctx.mahord.key('Escape');
      await new Promise((resolve) => setTimeout(resolve, 300));
      dismissed += 1;
    }
  }
  // Live boot state, not just failure state: whether the boot window ever maps decides between
  // a UI stall and an environment problem, and only a mid-window capture shows it (the g4 scope
  // ran 91s with no mapped window in run 20261111t073303z).
  await new Promise((resolve) => setTimeout(resolve, 10_000));
  const liveWins = await ctx.agent.call('exec', {
    command: 'sh',
    args: ['-c', 'DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/1000 hyprctl clients -a 2>/dev/null || DISPLAY=:0 wmctrl -l 2>/dev/null || true', 'sh'],
  });
  await evidence(ctx, result, 'boot-window-list.txt', String(liveWins.stdout ?? ''));
  try {
    if (ctx.mahord) {
      const png = await ctx.mahord.screenshot();
      if (png) await evidence(ctx, result, 'boot-window.png.b64', png.toString('base64'));
    }
  } catch { /* evidence only */ }
  // A WebKitGTK window can silently fail to map after repeated GUI cycles under XWayland: the
  // process runs, spawns terminals and publishes receipts, but no window ever exists (pair runs
  // 20261110t224537z..20261111t145836z), so the UI never renders, never hydrates, never saves.
  // A rebooted host maps its window; if this boot did not, restart the recorded GUI once.
  const guiPid = ctx.state.gui?.pid;
  const hasWindow = (text) => Boolean(guiPid) && String(text ?? '').includes('pid: ' + guiPid);
  let windowRetry = { guiPid, mapped: !guiPid ? 'unknown' : hasWindow(String(liveWins?.stdout ?? '')), restarted: false };
  if (guiPid && !hasWindow(String(liveWins?.stdout ?? ''))) {
    const exit = await ctx.agent.call('waitExit', { pid: guiPid, deadlineMs: 5_000 }).catch(() => ({ timedOut: true }));
    if (exit?.timedOut) await ctx.agent.call('kill', { pid: guiPid, signal: 'SIGKILL' }).catch(() => {});
    ctx.state.gui = await ctx.gui.launch(ctx.artifacts[bootKey], { env: { [QA_HOST_BOOT_TIME_ENV]: String(hostNow) } });
    await new Promise((resolve) => setTimeout(resolve, 10_000));
    const wins2 = await ctx.agent.call('exec', {
      command: 'sh',
      args: ['-c', 'DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/1000 hyprctl clients -a 2>/dev/null || DISPLAY=:0 wmctrl -l 2>/dev/null || true', 'sh'],
    });
    const pid2 = ctx.state.gui?.pid;
    windowRetry = { guiPid: pid2, mapped: Boolean(pid2) && String(wins2.stdout ?? '').includes('pid: ' + pid2), restarted: true };
    if (ctx.mahord) {
      for (let i = 0; i < 2; i += 1) { await ctx.mahord.key('Escape'); await new Promise((resolve) => setTimeout(resolve, 300)); }
    }
  }
  await evidence(ctx, result, 'boot-window-retry.json', windowRetry);
  // The Wine virtual desktop (explorer.exe /desktop, KakaoTalk) can cover the whole screen in
  // pair runs; an occluded WebKitGTK view throttles rendering and the boot hydration stalls
  // before the first save (isolated runs are never covered and pass). Focus the boot window so
  // the QA webview is the visible surface regardless of what else is on the desktop.
  if (windowRetry.mapped) {
    const focus = await ctx.agent.call('exec', {
      command: 'sh',
      args: ['-c', 'DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/1000 hyprctl dispatch focuswindow pid:' + windowRetry.guiPid + ' 2>/dev/null || DISPLAY=:0 wmctrl -a -p ' + windowRetry.guiPid + ' 2>/dev/null || true', 'sh'],
    });
    windowRetry.focused = focus.status === 0;
  }
  await evidence(ctx, result, 'boot-window-retry.json', windowRetry);
  await evidence(ctx, result, 'boot-modal-dismiss.json', { dismissed });

  // (d) The GUI restored the seeded workspace (its marker is gone from the saved session), then
  // within SETTLE_DEADLINE_MS no saved pane is still bound to a pre-boot backend id.
  const restored = await ctx.agent.call('waitLog', { file: sessionFile, pattern: absentPattern([`"qaRebootSeed":"${seed}"`]), deadlineMs: RESTORE_DEADLINE_MS });
  const restoredAt = Date.now();
  const guiLog = hjoin(ctx.layout.logs, `gui-${ctx.state.gui.tag}.log`);
  const override = await ctx.agent.call('waitLog', { file: guiLog, pattern: BOOT_OVERRIDE_RECEIPT, deadlineMs: 5_000 });
  assertion(result, 'the QA GUI read the overridden host boot time', Boolean(override.matched), { env: QA_HOST_BOOT_TIME_ENV, hostNow, matched: override.matched ?? null });
  if (!restored.matched) {
    const saved = parse(await ctx.agent.call('readFile', { file: sessionFile }));
    // Was the save rewritten at all after launch? An unchanged file means the GUI never restored;
    // a rewritten file whose seeded pane still carries the marker means it restored another pane.
    const stat = await ctx.agent.call('exec', { command: '/usr/bin/stat', args: ['-c', '%Y %s', sessionFile] })
      .catch((error) => ({ error: String(error?.message ?? error) }));
    await evidence(ctx, result, 'not-restored.json', {
      target, savedActive: saved?.activeWorkspaceId ?? null, saved, stat, launchedAt, sessionFile,
    });
    // The saved file and the logs cannot show what the boot window displays when the restore
    // stalls, so the failure path captures the screen: a modal, a blank window and a healthy
    // render read differently here (runs 20261110t224537z/20261111t003152z: no save, no log
    // difference).
    try {
      if (ctx.mahord) {
        const png = await ctx.mahord.screenshot();
        if (png) await evidence(ctx, result, 'restore-window.png.b64', png.toString('base64'));
      } else {
        const shot = await ctx.agent.call('exec', {
          command: 'sh', args: ['-c', 'DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/1000 grim - 2>/dev/null | base64 -w0', 'sh'],
        });
        if (shot.stdout) await evidence(ctx, result, 'restore-window.png.b64', shot.stdout.trim());
      }
      // The screenshot shows pixels, not ownership: name every window on the desktop so the
      // app that draws an overlapping modal is identifiable (run 20261111t073303z: the expired
      // QR modal's strings do not exist in the QA artifact's frontend).
      const wins = await ctx.agent.call('exec', {
        command: 'sh',
        args: ['-c', 'DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/1000 hyprctl clients -a 2>/dev/null || DISPLAY=:0 wmctrl -l 2>/dev/null || true', 'sh'],
      });
      if (String(wins.stdout ?? '').trim()) await evidence(ctx, result, 'restore-window-list.txt', String(wins.stdout));
    } catch { /* evidence only */ }
    if (saved && saved.activeWorkspaceId && saved.activeWorkspaceId !== target.workspaceId) {
      return block(result, 'RESTORE_TARGET_NOT_ACTIVE', `the GUI activated ${saved.activeWorkspaceId}, not the seeded ${target.workspaceId}`);
    }
    assertion(result, `the GUI restored and re-saved the seeded workspace within ${RESTORE_DEADLINE_MS} ms`, false, { target: target.workspaceId });
    return;
  }
  assertion(result, 'the GUI restored and re-saved the seeded workspace', true, { afterLaunchMs: restoredAt - launchedAt });
  // Only the pane's live binding matters here: the product deliberately keeps the retired backend
  // id in lastBackendSessionId as history (ui/src/state/workspaceStore.ts retainedLastBackendSessionId),
  // so banning the bare id anywhere in the JSON never settles once a pane has rebound.
  const settled = await ctx.agent.call('waitLog', { file: sessionFile, pattern: absentPattern(panes.map((p) => `"backendSessionId":"${p.backendSessionId}"`)), deadlineMs: SETTLE_DEADLINE_MS });
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
