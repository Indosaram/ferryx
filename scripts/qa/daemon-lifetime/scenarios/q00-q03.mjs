// Q00-Q03: ownership precondition and GUI-independence on quit, kill and OS termination.

import path from 'node:path';
import { assertion, block } from '../lib/summary.mjs';
import { evidence, dataConn, ownerIdentity, seedLocalSession, nonceRoundTrip, assertInventoryKept, assertSameOwner, DAEMON_READY_TIMEOUT_MS } from './common.mjs';

/** Launches GUI A and waits for its daemon to publish owner.json. */
export async function launchGuiAndOwner(ctx, artifact = ctx.artifacts.A, opts = {}) {
  const ownerFile = path.join(ctx.layout.runtime, 'owner.json');
  const prior = await ctx.agent.call('readFile', { file: ownerFile });
  const fromPid = prior ? JSON.parse(prior).pid : null;
  const gui = await ctx.gui.launch(artifact, opts);
  const changed = await ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid: opts.expectSameOwner ? -1 : fromPid, deadlineMs: DAEMON_READY_TIMEOUT_MS * 2 });
  return { gui, owner: changed.owner, timedOut: Boolean(changed.timedOut) };
}

export async function Q00(ctx, result) {
  const { gui, owner, timedOut } = await launchGuiAndOwner(ctx);
  ctx.state.gui = gui;
  assertion(result, 'QA GUI launched from the verified QA artifact', Boolean(gui.pid), gui);
  if (timedOut || !owner) {
    assertion(result, 'daemon published owner.json', false, 'no owner.json within 2 x DAEMON_READY_TIMEOUT');
    return;
  }
  const key = ctx.keys.A;
  const job = await ctx.jobs.inspect(key);
  await evidence(ctx, result, 'job-inspect.json', job);
  const identity = await ownerIdentity(ctx);
  await evidence(ctx, result, 'owner.json', identity);
  let jobPid = job.pid;
  if (ctx.platform === 'windows') {
    const tree = await ctx.jobs.windowsTree(ctx.stagedRoot);
    await evidence(ctx, result, 'task-tree.json', tree);
    jobPid = tree.daemon?.pid ?? null;
    assertion(result, 'scheduled task is Running', job.state === 'Running', job.state);
    assertion(result, 'watchdog runs the staged generation executable', Boolean(tree.watchdog), tree.watchdog);
    ctx.state.watchdog = tree.watchdog;
  } else {
    assertion(result, 'QA OS job reports Running', job.state === 'Running', job.state);
  }
  assertion(result, 'daemon pid is the QA job\'s pid', jobPid !== null && jobPid === identity.pid, { jobPid, ownerPid: identity.pid });
  assertion(result, 'owner.json pid equals control Hello pid', identity.owner?.pid === identity.hello?.pid, identity);
  assertion(result, 'Hello generation_key is the A generation', identity.hello?.generation_key === key, identity.hello?.generation_key);
  const exe = identity.os?.executable ?? '';
  assertion(result, 'daemon executable is the staged runtime copy, not the app bundle', exe.toLowerCase().startsWith(ctx.stagedRoot.toLowerCase()), { exe, stagedRoot: ctx.stagedRoot });
  if (ctx.platform === 'mac') {
    assertion(result, 'launchd program is the staged helper executable', String(job.program ?? '').includes('Ferryx Daemon.app/Contents/MacOS/ferryx'), job.program);
    const verify = await ctx.agent.call('exec', { command: 'codesign', args: ['--verify', '--deep', '--strict', '--verbose=2', path.join(ctx.stagedRoot, key, 'Ferryx Daemon.app')] });
    await evidence(ctx, result, 'codesign-staged.log', `exit ${verify.status}\n${verify.stdout}${verify.stderr}`);
    assertion(result, 'staged helper passes codesign --verify --deep --strict', verify.status === 0, verify.status);
  }
  if (ctx.platform === 'linux') {
    const legacy = await ctx.jobs.inspect(null);
    assertion(result, 'legacy QA unit is not enabled alongside the generation unit', legacy.unitFileState !== 'enabled', legacy.unitFileState);
  }
  await ctx.agent.call('record', { pid: identity.pid, role: 'daemon-A', executable: exe });
  ctx.state.daemon = identity;
  const conn = await dataConn(ctx);
  ctx.state.localA = await seedLocalSession(ctx, conn, 'QA_A');
  ctx.state.conn = conn;
  const desc = await conn.describe();
  await evidence(ctx, result, 'describe-live.json', desc);
  assertion(result, 'seeded local session is live with a child pid', desc.sessions.some((s) => s.backendId === ctx.state.localA && s.readerOpen && s.localChildPid), desc.sessions);
  // Carry-over: the plan names task-4/5/6 job logs as Q00 output.
  const jobLog = { mac: 'task-4-launchd.log', linux: 'task-5-systemd.log', windows: 'task-6-schtasks.log' }[ctx.platform];
  await ctx.writeEvidence(path.posix.join(ctx.evidenceDir, jobLog), JSON.stringify({ job, identity }, null, 2));
  ctx.carry.push({ item: `${jobLog} from Q00`, evidence: path.posix.join(ctx.evidenceDir, jobLog) });
}

/** Shared body of Q01-Q03: end the GUI one way, then prove daemon, children and sessions survived. */
export async function surviveGuiEnd(ctx, result, how, artifact = ctx.artifacts.A) {
  const conn = ctx.state.conn;
  const before = await conn.describe();
  const ownerBefore = await ownerIdentity(ctx);
  await evidence(ctx, result, 'before.json', { before, ownerBefore });
  const ended = await how(ctx.state.gui);
  await evidence(ctx, result, 'gui-end.json', ended);
  assertion(result, 'GUI process exited', !ended?.timedOut && !(ended?.exited?.timedOut), ended);
  const ownerMid = await ownerIdentity(ctx);
  assertSameOwner(result, ownerBefore, ownerMid, 'after GUI end');
  const relaunch = await ctx.gui.launch(artifact);
  ctx.state.gui = relaunch;
  const conn2 = await dataConn(ctx);
  const after = await conn2.describe();
  await evidence(ctx, result, 'after.json', { after, relaunch });
  assertInventoryKept(result, before, after, 'after relaunch');
  const ownerAfter = await ownerIdentity(ctx);
  assertSameOwner(result, ownerBefore, ownerAfter, 'after relaunch');
  await nonceRoundTrip(ctx, conn2, ctx.state.localA, result, 'reattached session');
  await conn.close();
  ctx.state.conn = conn2;
  // restart the counter so later scenarios see continuous output again
  await conn2.call({ type: 'write', sessionId: ctx.state.localA, data: Buffer.from(ctx.platform === 'windows' ? 'for($i=0;;$i++){"QA_A_TICK_$i";Start-Sleep -Milliseconds 100}\r\n' : 'i=0; while :; do i=$((i+1)); echo QA_A_TICK_$i; sleep 0.1; done\n').toString('base64') });
}

export async function Q01(ctx, result) {
  await surviveGuiEnd(ctx, result, (gui) => ctx.gui.quit(gui));
}

export async function Q02(ctx, result) {
  await surviveGuiEnd(ctx, result, (gui) => ctx.gui.kill(gui));
}

export async function Q03(ctx, result) {
  if (ctx.platform === 'windows' && ctx.state.gui && ctx.state.gui.jobAssigned === false) {
    block(result, 'JOB_ASSIGN_FAILED', 'AssignProcessToJobObject returned false for the QA GUI');
    return;
  }
  await surviveGuiEnd(ctx, result, (gui) => ctx.gui.forceTerminate(gui));
}
