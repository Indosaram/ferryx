// Q10-Q13: autostart opt-out, headless use, SSH continuity without replay, live-entry honesty,
// and legacy single-job migration.

import path from 'node:path';
import { assertion, block } from '../lib/summary.mjs';
import { data as D, isLive, liveSessions } from '../lib/wire.mjs';
import {
  evidence, dataConn, controlConn, ownerIdentity, nonceRoundTrip, assertSameOwner, retireOwner, freshOwner, nonce,
  echoCommand, interrupt, DAEMON_READY_TIMEOUT_MS,
} from './common.mjs';
import { startSshd } from '../lib/sshd.mjs';

export const OPTOUT_FILE = 'daemon-supervisor.optout'; // supervisor.rs OPTOUT_FILE_NAME, in <FERRYX_DATA_DIR>/remote

async function loginRegistration(ctx, key) {
  if (ctx.platform === 'mac') {
    const plist = (await ctx.agent.call('readFile', { file: path.join(ctx.layout.launchAgents, `${ctx.jobs.label(key)}.plist`) })) ?? '';
    return { runAtLoad: /<key>RunAtLoad<\/key>\s*<true\/>/.test(plist), raw: plist };
  }
  if (ctx.platform === 'linux') {
    const job = await ctx.jobs.inspect(key);
    return { enabled: job.unitFileState === 'enabled', raw: job.unitFileState };
  }
  const xml = await ctx.agent.call('exec', { command: 'schtasks', args: ['/Query', '/TN', ctx.jobs.task(key), '/XML'] });
  return { logonTrigger: /<LogonTrigger/.test(xml.stdout), raw: xml.stdout };
}

const autostarts = (reg) => reg.runAtLoad ?? reg.enabled ?? reg.logonTrigger;

export async function Q10(ctx, result) {
  const key = ctx.keys.A;
  const owner = await ownerIdentity(ctx);
  const optout = path.join(ctx.layout.data, 'remote', OPTOUT_FILE);
  const regBefore = await loginRegistration(ctx, key);
  assertion(result, 'newest generation registered for login before opt-out', autostarts(regBefore) === true, regBefore.raw?.slice?.(0, 400) ?? regBefore.raw);
  // Toggle through the product: the CLI-free path is the settings toggle, which writes the marker
  // and calls remove_autostart. The harness drives the same function through the QA GUI env.
  await ctx.gui.quit(ctx.state.gui);
  await ctx.agent.call('writeFile', { file: optout, content: '' });
  ctx.state.gui = await ctx.gui.launch(ctx.artifacts.A);
  const settled = await ctx.agent.call('waitLog', { file: path.join(ctx.layout.logs, `gui-${ctx.state.gui.tag}.log`), pattern: 'autostart|opt-?out', flags: 'i', deadlineMs: DAEMON_READY_TIMEOUT_MS });
  const regAfter = await loginRegistration(ctx, key);
  const ownerAfter = await ownerIdentity(ctx);
  await evidence(ctx, result, 'toggle.json', { regBefore, regAfter, settled, owner, ownerAfter });
  assertion(result, 'opt-out removed login autostart', autostarts(regAfter) === false, regAfter.raw?.slice?.(0, 400) ?? regAfter.raw);
  assertSameOwner(result, owner, ownerAfter, 'opt-out toggle never stops the running owner');
  if (ctx.platform === 'windows') {
    const tree = await ctx.jobs.windowsTree(ctx.stagedRoot);
    assertion(result, 'task re-import kept watchdog and daemon pids (carry-over todo 10 N4)', tree.watchdog?.pid === ctx.state.watchdog?.pid && tree.daemon?.pid === owner.pid, tree);
    ctx.carry.push({ item: 'todo 10 N4: /Create /XML /F re-import keeps watchdog and daemon alive', evidence: path.posix.join(ctx.evidenceDir, 'Q10-toggle.json') });
  }
  // Opted-out launch with an Absent owner: the newest generation still starts through its OS job.
  const retired = await retireOwner(ctx);
  assertion(result, 'owner retired before the opted-out launch', retired.retired, retired);
  if (ctx.platform === 'mac') {
    // Make the generation job NotRegistered so the opted-out bootstrap + plain kickstart path runs.
    await ctx.agent.call('exec', { command: 'launchctl', args: ['bootout', `gui/${ctx.uid}/${ctx.jobs.label(key)}`] });
    await ctx.agent.call('remove', { target: path.join(ctx.layout.launchAgents, `${ctx.jobs.label(key)}.plist`) });
  }
  const fresh = await freshOwner(ctx, ctx.artifacts.A);
  const job = await ctx.jobs.inspect(key);
  const guiText = (await ctx.agent.call('readFile', { file: path.join(ctx.layout.logs, `gui-${ctx.state.gui.tag}.log`) })) ?? '';
  await evidence(ctx, result, 'optout-launch.json', { fresh, job });
  await evidence(ctx, result, 'optout-launch-gui.log', guiText);
  const jobPid = ctx.platform === 'windows' ? (await ctx.jobs.windowsTree(ctx.stagedRoot)).daemon?.pid : job.pid;
  assertion(result, 'opted-out launch started the owner under the OS job, not as a GUI child', jobPid === fresh.pid, { jobPid, owner: fresh.pid });
  const row = (await ctx.agent.call('table')).find((r) => r.pid === fresh.pid);
  assertion(result, 'owner parent is not the GUI', row && row.ppid !== ctx.state.gui.pid, row);
  if (ctx.platform === 'mac') {
    assertion(result, 'opted-out NotRegistered job: plist has RunAtLoad and KeepAlive false, bootstrap + plain kickstart started it (carry-over todo 10 N2)', /<key>RunAtLoad<\/key>\s*<false\/>/.test((await loginRegistration(ctx, key)).raw) && job.state === 'Running', job.state);
    ctx.carry.push({ item: 'todo 10 N2 / DoneClaim risk: bootstrap does not start a RunAtLoad=false KeepAlive=false plist; plain kickstart does', evidence: path.posix.join(ctx.evidenceDir, 'Q10-optout-launch.json') });
  }
  await ctx.agent.call('remove', { target: optout });
}

export async function Q10b(ctx, result) {
  const retired = await retireOwner(ctx);
  assertion(result, 'owner retired', retired.retired, retired);
  const rows = await ctx.agent.call('table');
  const guis = rows.filter((r) => r.executable && ctx.guiExecutables.some((g) => g.toLowerCase() === r.executable.toLowerCase()));
  assertion(result, 'no QA GUI process running (recorded-path check of every process)', guis.length === 0, guis);
  const key = ctx.keys.A;
  const start = await ctx.jobs.start(key);
  await evidence(ctx, result, 'job-start.json', start);
  const changed = await ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid: retired.pid, deadlineMs: DAEMON_READY_TIMEOUT_MS });
  const ctl = changed.owner ? await controlConn(ctx) : null;
  const hello = ctl ? await ctl.hello() : null;
  const job = await ctx.jobs.inspect(key);
  const jobPid = ctx.platform === 'windows' ? (await ctx.jobs.windowsTree(ctx.stagedRoot)).daemon?.pid : job.pid;
  assertion(result, 'control Hello answers within DAEMON_READY_TIMEOUT', hello?.ok === true, hello);
  assertion(result, 'Hello pid equals the QA job pid', hello?.pid === jobPid, { hello: hello?.pid, jobPid });
  await ctx.agent.call('record', { pid: hello.pid, role: 'daemon-headless', executable: (await ctx.agent.call('probe', { pid: hello.pid })).executable });
  // The QA build's own CLI client entry point: `ferryx agents list` (agent_control) against the QA runtime.
  const cliEnv = ctx.isolationEnv;
  const list = await ctx.agent.call('exec', { command: ctx.artifacts.A.cliExecutable, args: ['agents', 'list', '--json'], env: cliEnv, deadlineMs: 90_000 });
  await evidence(ctx, result, 'cli-list.log', `exit ${list.status}\n${list.stdout}\n${list.stderr}`);
  assertion(result, 'CLI client listed sessions through the daemon', list.status === 0, list.status);
  const conn = await dataConn(ctx);
  ctx.state.conn = conn;
  const spawned = await conn.call(D.spawnLocal({ clientRequestId: `qa-headless-${nonce('R')}`, workspaceId: ctx.workspaceId, cwd: ctx.repoDir }));
  if (spawned.type === 'error' && !ctx.workspaceRegistered) await conn.call(D.registerWorkspace(ctx.workspaceId, ctx.repoDir));
  const sid = spawned.sessionId ?? (await conn.call(D.spawnLocal({ clientRequestId: `qa-headless-${nonce('R')}`, workspaceId: ctx.workspaceId, cwd: ctx.repoDir }))).sessionId;
  assertion(result, 'one local session spawned headless', Boolean(sid), spawned);
  ctx.state.localA = sid;
  const list2 = await ctx.agent.call('exec', { command: ctx.artifacts.A.cliExecutable, args: ['agents', 'list', '--json'], env: cliEnv, deadlineMs: 90_000 });
  await evidence(ctx, result, 'cli-list-after.log', `exit ${list2.status}\n${list2.stdout}\n${list2.stderr}`);
  const after = await ctl.hello();
  assertion(result, 'Hello pid unchanged after the CLI exits', after?.pid === hello.pid, { before: hello.pid, after: after?.pid });
  await ctl.close();
  ctx.state.gui = await ctx.gui.launch(ctx.artifacts.A);
}

export async function Q11(ctx, result) {
  if (!ctx.hooks.writeAckHoldArm?.present) return block(result, 'HOOK_MISSING', ctx.hooks.writeAckHoldArm.owner);
  ctx.sshd = ctx.sshd ?? await startSshd(ctx);
  if (ctx.sshd.unavailable) return block(result, 'SSHD_UNAVAILABLE', ctx.sshd.unavailable);
  const conn = ctx.state.conn;
  const spawned = await conn.call(D.spawnSsh({ clientRequestId: `qa-q11-${nonce('R')}`, workspaceId: ctx.sshd.workspaceId, hostStorePath: ctx.sshd.hostStorePath }), 60_000);
  assertion(result, 'SSH session spawned against the private sshd', spawned.type === 'spawnOk', spawned);
  if (spawned.type !== 'spawnOk') return;
  const sid = spawned.sessionId;
  const input = path.join(ctx.sshd.repoRoot, 'ssh-input.log');
  await conn.call(D.attach(sid, 0));
  await conn.call(D.write(sid, `cat >> ${input}\n`));
  const before = await conn.describe();
  const sBefore = before.sessions.find((s) => s.backendId === sid);
  await evidence(ctx, result, 'before.json', before);
  assertion(result, 'SSH session live with a host-scoped remote pid', sBefore?.connected && sBefore?.remotePid != null && sBefore?.localChildPid == null, sBefore);
  // Arm the hold, type the marker through the GUI, wait for write-applied, quit while held.
  const op = `q11-${ctx.runId}`;
  await ctx.agent.call('writeFile', { file: path.join(ctx.layout.barrier, 'write-ack-hold.request.json'), content: JSON.stringify({ runId: ctx.runId, operationId: op, sessionId: sid }) });
  const marker = nonce('UNCERTAIN');
  await ctx.typeIntoSession(ctx.state.gui, sid, `${marker}\n`);
  const applied = await ctx.agent.call('waitLog', { file: path.join(ctx.layout.barrier, 'write-ack-hold.receipt.jsonl'), pattern: `"write-applied"[^\\n]*"${sid}"`, deadlineMs: 30_000 });
  assertion(result, 'write-applied observed while the ack is held', Boolean(applied.matched), applied);
  const quit = await ctx.gui.quit(ctx.state.gui);
  ctx.state.gui = await ctx.gui.launch(ctx.artifacts.A);
  const conn2 = await dataConn(ctx);
  const after = await conn2.describe();
  const sAfter = after.sessions.find((s) => s.backendId === sid);
  await evidence(ctx, result, 'after.json', { after, quit });
  assertion(result, 'same backend id and remote_pid after GUI quit/relaunch', sAfter?.remotePid === sBefore?.remotePid && isLive(sAfter), sAfter);
  await interrupt(conn2, sid);
  const log = await ctx.agent.call('exec', { command: 'ssh', args: ['-i', ctx.sshd.clientKey, '-o', `UserKnownHostsFile=${ctx.sshd.knownHosts}`, '-o', 'BatchMode=yes', '-p', String(ctx.sshd.port), `${ctx.sshd.user}@127.0.0.1`, `cat ${input}`] });
  await evidence(ctx, result, 'ssh-input.log', log.stdout);
  const count = log.stdout.split(marker).length - 1;
  assertion(result, 'remote log contains the marker exactly once (no replay of the uncertain write)', count === 1, count);
  await conn2.call(D.close(sid));
  await ctx.state.conn.close().catch(() => {});
  ctx.state.conn = conn2;
}

export async function Q12(ctx, result) {
  const conn = ctx.state.conn;
  // exited local entry
  const spawned = await conn.call(D.spawnLocal({ clientRequestId: `qa-q12-${nonce('R')}`, workspaceId: ctx.workspaceId, cwd: ctx.repoDir }));
  const exited = spawned.sessionId;
  await conn.call(D.attach(exited, 0));
  await conn.call(D.write(exited, ctx.platform === 'windows' ? 'exit\r\n' : 'exit\n'));
  // the reader closes when the shell exits: wait for the inventory to say so via a fresh describe
  let desc;
  for (const deadline = Date.now() + 15_000; Date.now() < deadline;) {
    desc = await conn.describe();
    const e = desc.sessions.find((s) => s.backendId === exited);
    if (!e || e.readerOpen === false) break;
    await conn.waitOutput(exited, '\u0000never', 500);
  }
  const entry = desc.sessions.find((s) => s.backendId === exited);
  assertion(result, 'exited local PTY reports readerOpen=false (or is gone)', !entry || entry.readerOpen === false, entry);
  assertion(result, 'exited entry is not counted live', !liveSessions(desc).some((s) => s.backendId === exited), null);
  // disconnected SSH entry: stop the private sshd's connection by killing the recorded sshd pid
  if (ctx.sshd && !ctx.sshd.unavailable) {
    const ssh = await conn.call(D.spawnSsh({ clientRequestId: `qa-q12s-${nonce('R')}`, workspaceId: ctx.sshd.workspaceId, hostStorePath: ctx.sshd.hostStorePath }), 60_000);
    if (ssh.type === 'spawnOk') {
      await ctx.agent.call('kill', { pid: ctx.sshd.pid, signal: 'SIGTERM' });
      const rows = (await conn.describe()).sessions;
      const s = rows.find((x) => x.backendId === ssh.sessionId);
      await evidence(ctx, result, 'disconnected.json', s ?? null);
      assertion(result, 'disconnected SSH entry never satisfies the live predicate', !s || !isLive(s) || s.connected === true, s);
      assertion(result, 'remote_pid never placed in local_child_pid', !s || s.localChildPid == null, s);
      ctx.sshd = null;
    }
  }
  await evidence(ctx, result, 'describe.json', await conn.describe());
  const ctl = await controlConn(ctx);
  const inv = await ctl.inventory();
  const live = liveSessions(await conn.describe());
  assertion(result, 'control Inventory live_local equals the live local entries', inv.live_local === live.filter((s) => s.kind === 'local').length, { inv, live: live.length });
  await ctl.close();
}

export async function Q13(ctx, result) {
  if (!ctx.artifacts.baseline) return block(result, 'ARTIFACT_MISSING', 'pre-plan baseline daemon binary not built');
  const retired = await retireOwner(ctx);
  assertion(result, 'QA generation owner retired', retired.retired, retired);
  const legacyEnv = { FERRYX_DATA_DIR: ctx.layout.data, FERRYX_RUNTIME_DIR: ctx.layout.runtime, FERRYX_SESSION_DIR: ctx.layout.sessions };
  const installed = await ctx.jobs.installLegacy({ executable: ctx.artifacts.baseline.daemonExecutable, env: legacyEnv });
  await evidence(ctx, result, 'legacy-install.json', installed);
  const ready = await ctx.agent.call('waitPath', { file: ctx.platform === 'windows' ? path.join(ctx.layout.runtime, 'daemon.port') : path.join(ctx.layout.runtime, 'daemon.sock'), deadlineMs: DAEMON_READY_TIMEOUT_MS });
  assertion(result, 'legacy daemon published its endpoint', ready, null);
  const conn = await dataConn(ctx);
  const legacyPid = conn.handshake.pid;
  const legacyExe = (await ctx.agent.call('probe', { pid: legacyPid }))?.executable;
  await ctx.agent.call('record', { pid: legacyPid, role: 'daemon-legacy', executable: legacyExe });
  assertion(result, 'legacy daemon has no control endpoint', (await controlConn(ctx)) === null, null);
  await conn.call(D.registerWorkspace(ctx.workspaceId, ctx.repoDir));
  const sp = await conn.call(D.spawnLocal({ clientRequestId: `qa-q13-${nonce('R')}`, workspaceId: ctx.workspaceId, cwd: ctx.repoDir }));
  assertion(result, 'legacy daemon spawned a live session', sp.type === 'spawnOk', sp);
  const legacyBefore = await ctx.agent.call('probe', { pid: legacyPid });
  // New GUI meets the legacy owner.
  const gui = await ctx.gui.launch(ctx.artifacts.A);
  ctx.state.gui = gui;
  const guiLog = path.join(ctx.layout.logs, `gui-${gui.tag}.log`);
  await ctx.agent.call('waitLog', { file: guiLog, pattern: 'legacy|attached|handshake', flags: 'i', deadlineMs: DAEMON_READY_TIMEOUT_MS });
  await conn.call(D.attach(sp.sessionId, 0));
  await nonceRoundTrip(ctx, conn, sp.sessionId, result, 'legacy session usable with the new GUI attached');
  const legacyAfter = await ctx.agent.call('probe', { pid: legacyPid });
  assertion(result, 'legacy pid and start time unchanged', legacyBefore && legacyAfter && String(legacyBefore.startTime) === String(legacyAfter.startTime), { legacyBefore, legacyAfter });
  const legacyJob = await ctx.jobs.inspect(null);
  const text = (await ctx.agent.call('readFile', { file: guiLog })) ?? '';
  await evidence(ctx, result, 'gui.log', text);
  await evidence(ctx, result, 'legacy-job.json', legacyJob);
  assertion(result, 'no bootout/stop/End sent to the legacy job', !new RegExp(`(bootout|stop|/End|/Delete)[^\\n]*${ctx.jobs.base}(?![.-])`).test(text), null);
  assertion(result, 'no UpgradeBinary sent to the legacy owner', !/upgradeBinary|UpgradeBinary/.test(text), null);
  if (ctx.platform === 'windows') {
    // Carry-over todo 14: the real Get-ScheduledTask State read on maho-win.
    assertion(result, 'legacy task State read for real (Running)', legacyJob.state === 'Running', legacyJob);
    ctx.carry.push({ item: 'todo 14: Get-ScheduledTask State read of the legacy task on maho-win', evidence: path.posix.join(ctx.evidenceDir, 'Q13-legacy-job.json') });
  } else {
    const reg = ctx.platform === 'mac'
      ? /<key>RunAtLoad<\/key>\s*<false\/>/.test((await ctx.agent.call('readFile', { file: path.join(ctx.layout.launchAgents, `${ctx.jobs.base}.plist`) })) ?? '')
      : legacyJob.unitFileState !== 'enabled';
    assertion(result, 'legacy job lost login autostart without being stopped', reg, legacyJob.unitFileState ?? null);
  }
  // Zero-session explicit end of the legacy daemon (stand-in for logout/reboot).
  await conn.call(D.close(sp.sessionId));
  const listed = await conn.call(D.listSessions());
  const desc = await conn.call(D.describeLiveSessions()).catch(() => null);
  const zero = (desc?.type === 'describeLiveSessionsOk' ? liveSessions(desc).length : listed.sessions?.length ?? -1) === 0;
  assertion(result, 'legacy live count is zero before the explicit shutdown', zero, { listed, desc });
  if (!zero) return;
  await ctx.gui.quit(ctx.state.gui);
  const exit = ctx.agent.call('waitExit', { pid: legacyPid, deadlineMs: 30_000 });
  await conn.call(D.shutdown()).catch(() => null);
  const exited = await exit;
  assertion(result, 'legacy daemon exited after the zero-session Shutdown', !exited.timedOut, exited);
  const fresh = await freshOwner(ctx, ctx.artifacts.A);
  const jobs = await ctx.jobs.listQaJobs();
  await evidence(ctx, result, 'after-migration.json', { fresh, jobs });
  assertion(result, 'newest generation started on the next admission', fresh.hello?.generation_key === ctx.keys.A, fresh.hello);
  assertion(result, 'legacy plist/unit/task removed', !jobs.some((j) => j === ctx.jobs.jobName(null) || j === `${ctx.jobs.base}.service`), jobs);
  void echoCommand;
}
