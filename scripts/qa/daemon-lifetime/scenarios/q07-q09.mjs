// Q07-Q09: app replacement, control-channel upgrades (Unix), cross-revision owner, Windows pending.

import path from 'node:path';
import { assertion, block } from '../lib/summary.mjs';
import { HANDOVER_FAULTS } from '../lib/hooks.mjs';
import { data as D, helloSupportsHandover, inventoryTotal } from '../lib/wire.mjs';
import {
  evidence, dataConn, controlConn, ownerIdentity, nonceRoundTrip, assertInventoryKept, assertSameOwner,
  retireOwner, freshOwner, nonce, counterCommand, interrupt, DAEMON_READY_TIMEOUT_MS, RESPAWN_BACKOFF_MS,
} from './common.mjs';
import { surviveGuiEnd } from './q00-q03.mjs';
import { startSshd } from '../lib/sshd.mjs';
import { AUTHORITY_ARM, AUTHORITY_STAGE, HANDOVER_FAULT_REQUEST, receiptFile, authorityStages, authorityInvariant, receiptCount, stagePattern } from '../lib/receipts.mjs';

/** Arms the handover-authority receipts for one upgrade; the daemon acks it on its next stage. */
async function armAuthority(ctx, tag) {
  const operationId = `${tag}-${ctx.runId}`;
  await ctx.agent.call('writeFile', { file: path.join(ctx.layout.barrier, `${AUTHORITY_ARM}.arm.json`), content: JSON.stringify({ name: AUTHORITY_ARM, runId: ctx.runId, operationId }) });
  const text = await ctx.agent.call('readFile', { file: receiptFile(ctx.layout, AUTHORITY_STAGE) });
  return { operationId, from: receiptCount(text) };
}

async function readStages(ctx, from) {
  return authorityStages(await ctx.agent.call('readFile', { file: receiptFile(ctx.layout, AUTHORITY_STAGE) }), ctx.runId, from);
}

export async function Q07(ctx, result) {
  // A is the running generation; the on-disk app is replaced by B (same version, other bytes).
  const before = await ctx.state.conn.describe();
  const ownerBefore = await ownerIdentity(ctx);
  const authorityBefore = receiptCount(await ctx.agent.call('readFile', { file: receiptFile(ctx.layout, AUTHORITY_STAGE) }));
  await ctx.gui.quit(ctx.state.gui);
  const replaced = await ctx.replaceAppOnDisk(ctx.artifacts.A, ctx.artifacts.B);
  await evidence(ctx, result, 'replace.json', replaced);
  const gui = await ctx.gui.launch(replaced.artifact);
  ctx.state.gui = gui;
  const conn = await dataConn(ctx);
  const after = await conn.describe();
  const ownerAfter = await ownerIdentity(ctx);
  await evidence(ctx, result, 'after.json', { before, after, ownerBefore, ownerAfter });
  assertSameOwner(result, ownerBefore, ownerAfter, 'A generation');
  assertion(result, 'owner still serves generation A', ownerAfter.hello?.generation_key === ctx.keys.A, ownerAfter.hello?.generation_key);
  assertion(result, 'A staged runtime dir intact', await ctx.agent.call('exists', { file: path.join(ctx.stagedRoot, ctx.keys.A) }), ctx.keys.A);
  assertInventoryKept(result, before, after, 'B attaches the same sessions');
  await nonceRoundTrip(ctx, conn, ctx.state.localA, result, 'B GUI on A session');
  // Replacing the app on disk is not a handover: no process wrote an authority stage.
  const stages = await readStages(ctx, authorityBefore);
  assertion(result, 'no handover-authority stage after the replacement (no transfer started)', stages.length === 0, stages.map((s) => `${s.producerPid}:${s.stage}`));
  if (ctx.platform !== 'windows') {
    const jobB = await ctx.jobs.inspect(ctx.keys.B);
    assertion(result, 'B generation job registered but not running while A is Alive', jobB.state !== 'Running', jobB.state);
  }
  await ctx.state.conn.close().catch(() => {});
  ctx.state.conn = conn;
  // Put A back so later scenarios start from the same artifact.
  await ctx.gui.quit(ctx.state.gui);
  await ctx.replaceAppOnDisk(replaced.artifact, ctx.artifacts.A);
  ctx.state.gui = await ctx.gui.launch(ctx.artifacts.A);
}

/** Seeds a local and an SSH session, each running a 100ms counter. */
async function seedPair(ctx, conn, tag) {
  const local = ctx.state.localA;
  await interrupt(conn, local);
  await conn.call(D.write(local, counterCommand(ctx.platform, `${tag}_L`)));
  const spawned = await conn.call(D.spawnSsh({ clientRequestId: `qa-ssh-${nonce('R')}`, workspaceId: ctx.sshd.workspaceId, hostStorePath: ctx.sshd.hostStorePath }), 60_000);
  if (spawned.type !== 'spawnOk') throw new Error(`ssh spawn: ${JSON.stringify(spawned)}`);
  await conn.call(D.attach(spawned.sessionId, 0));
  await conn.call(D.write(spawned.sessionId, counterCommand('linux', `${tag}_S`)));
  const lo = await conn.waitOutput(local, `${tag}_L_TICK_3`, 15_000);
  const so = await conn.waitOutput(spawned.sessionId, `${tag}_S_TICK_3`, 30_000);
  if (lo.timedOut || so.timedOut) throw new Error('counters produced no output');
  return { local, ssh: spawned.sessionId };
}

/** Highest counter value seen; continuity = values after are > values before with no restart at 1. */
export function counterProgress(text, tag) {
  const values = [...text.matchAll(new RegExp(`${tag}_TICK_(\\d+)`, 'g'))].map((m) => Number(m[1]));
  return { max: values.length ? Math.max(...values) : 0, restarted: values.some((v, i) => i > 0 && v === 1 && values[i - 1] > 1) };
}

async function stageGeneration(ctx, artifact) {
  // The GUI stages its runtime on launch (todo 3); here the harness reads back the staged path.
  const exe = ctx.platform === 'mac'
    ? path.join(ctx.stagedRoot, ctx.keys[artifact.name], 'Ferryx Daemon.app', 'Contents', 'MacOS', 'ferryx')
    : path.join(ctx.stagedRoot, ctx.keys[artifact.name], 'ferryx');
  return { exe, key: ctx.keys[artifact.name], present: await ctx.agent.call('exists', { file: exe }) };
}

/** One control RequestUpgrade from the current owner to artifact; asserts the Q08 contract. */
async function upgradeStep(ctx, result, artifact, pair, tag) {
  const before = await ctx.state.conn.describe();
  const pred = await ownerIdentity(ctx);
  // Launching the new GUI stages its runtime; the new GUI itself never starts a second owner.
  await ctx.gui.quit(ctx.state.gui);
  ctx.state.gui = await ctx.gui.launch(artifact);
  const staged = await stageGeneration(ctx, artifact);
  assertion(result, `${tag}: ${artifact.name} runtime staged`, staged.present, staged);
  const ctl = await controlConn(ctx);
  const hello = await ctl.hello();
  assertion(result, `${tag}: predecessor advertises RequestUpgrade + handover-v1`, helloSupportsHandover(hello), hello.ops);
  const authority = await armAuthority(ctx, tag);
  const ownerChange = ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid: pred.pid, deadlineMs: DAEMON_READY_TIMEOUT_MS * 2 });
  const accepted = await ctl.requestUpgrade(staged.exe, staged.key);
  assertion(result, `${tag}: RequestUpgrade accepted`, accepted?.ok === true && accepted.accepted === true, accepted);
  const changed = await ownerChange;
  const succ = await ownerIdentity(ctx);
  await evidence(ctx, result, `${tag}-owners.json`, { pred, succ, seen: changed.seen });
  assertion(result, `${tag}: owner.json moved straight from predecessor to successor`, changed.owner && changed.seen.filter((p) => p !== pred.pid).length === 1, changed.seen);
  assertion(result, `${tag}: successor serves generation ${artifact.name}`, succ.hello?.generation_key === staged.key, succ.hello?.generation_key);
  // todo 8 receipts: the candidate's own stages through authority-adopted, sampled at each transition.
  const adopted = await ctx.agent.call('waitLog', { file: receiptFile(ctx.layout, AUTHORITY_STAGE), pattern: stagePattern(succ.pid, 'authority-adopted'), deadlineMs: 30_000 });
  const stages = await readStages(ctx, authority.from);
  const inv = authorityInvariant(stages, succ.pid);
  await evidence(ctx, result, `${tag}-authority.json`, { operationId: authority.operationId, stages, inv });
  assertion(result, `${tag}: candidate wrote authority-adopted`, Boolean(adopted.matched), adopted);
  assertion(result, `${tag}: predecessor wrote predecessor-prepared and predecessor-committed`, ['predecessor-prepared', 'predecessor-committed'].every((s) => stages.some((r) => r.producerPid === pred.pid && r.stage === s)), inv.stages);
  assertion(result, `${tag}: candidate stages before commit were sampled`, ['candidate-admitted', 'authority-validated', 'commit-sent'].every((s) => inv.candidatePreCommit.includes(s)), inv.candidatePreCommit);
  assertion(result, `${tag}: candidate accepted nothing on any inherited listener before commit`, inv.candidatePreCommit.length > 0 && inv.acceptedBeforeCommit.length === 0, inv.acceptedBeforeCommit);
  assertion(result, `${tag}: a probe flock on both lock files failed at every stage`, stages.length > 0 && inv.lockFreeAt.length === 0, inv.lockFreeAt);
  assertion(result, `${tag}: public socket path present at every stage`, stages.length > 0 && inv.socketMissingAt.length === 0, inv.socketMissingAt);
  // successor parent = OS job
  const row = (await ctx.agent.call('table')).find((r) => r.pid === succ.pid);
  const job = await ctx.jobs.inspect(staged.key);
  assertion(result, `${tag}: successor runs under its own generation job`, job.pid === succ.pid, { jobPid: job.pid, succ: succ.pid });
  assertion(result, `${tag}: successor parent is not the predecessor`, row && row.ppid !== pred.pid, row);
  // predecessor keeps serving the SSH session, drains, and is still alive
  assertion(result, `${tag}: predecessor still alive while it owns the SSH session`, (await ctx.agent.call('probe', { pid: pred.pid })) !== null, pred.pid);
  await ctx.agent.call('record', { pid: succ.pid, role: `daemon-${artifact.name}`, executable: succ.os.executable });
  const conn = await dataConn(ctx);
  const after = await conn.describe();
  await evidence(ctx, result, `${tag}-describe.json`, { before, after });
  assertInventoryKept(result, before, after, `${tag}: local + SSH identities`);
  const sshAfter = after.sessions.find((s) => s.backendId === pair.ssh);
  const sshBefore = before.sessions.find((s) => s.backendId === pair.ssh);
  assertion(result, `${tag}: SSH remote_pid unchanged`, sshAfter?.remotePid === sshBefore?.remotePid && sshAfter?.remotePid != null, { before: sshBefore, after: sshAfter });
  await conn.call(D.attach(pair.local, 0));
  await conn.call(D.attach(pair.ssh, 0));
  const lText = (await conn.waitOutput(pair.local, `${tag}_L_TICK_`, 5_000)).text ?? '';
  const sText = (await conn.waitOutput(pair.ssh, `${tag}_S_TICK_`, 5_000)).text ?? '';
  const lp1 = counterProgress(lText, `${tag}_L`);
  const sp1 = counterProgress(sText, `${tag}_S`);
  const lNext = await conn.waitOutput(pair.local, `${tag}_L_TICK_${lp1.max + 3}`, 10_000);
  const sNext = await conn.waitOutput(pair.ssh, `${tag}_S_TICK_${sp1.max + 3}`, 10_000);
  assertion(result, `${tag}: local counter continues without restart`, !lNext.timedOut && !counterProgress(lNext.text, `${tag}_L`).restarted, lp1);
  assertion(result, `${tag}: SSH counter continues through the successor without restart`, !sNext.timedOut && !counterProgress(sNext.text, `${tag}_S`).restarted, sp1);
  for (const sid of [pair.local, pair.ssh]) await nonceRoundTrip(ctx, conn, sid, result, `${tag}: ${sid === pair.local ? 'local' : 'ssh'} input once`);
  await ctl.close();
  await ctx.state.conn.close().catch(() => {});
  ctx.state.conn = conn;
  return { pred, succ };
}

/** One injected transfer failure: the predecessor keeps both sessions writable, the candidate exits 0. */
async function faultVariant(ctx, result, fault, artifact, pair) {
  const pred = await ownerIdentity(ctx);
  const op = `fault-${fault}-${ctx.runId}`;
  await ctx.agent.call('writeFile', { file: path.join(ctx.layout.barrier, HANDOVER_FAULT_REQUEST), content: JSON.stringify({ runId: ctx.runId, operationId: op, fault }) });
  const authority = await armAuthority(ctx, `F_${fault}`);
  const staged = await stageGeneration(ctx, artifact);
  const ctl = await controlConn(ctx);
  const resp = await ctl.requestUpgrade(staged.exe, staged.key);
  // The candidate is the process that writes candidate-admitted (todo 8). Every variant except a
  // reply dropped after the decision leaves the predecessor serving and the candidate exits;
  // after-record, the recorded decision makes the candidate the owner (authority-recovered).
  const file = receiptFile(ctx.layout, AUTHORITY_STAGE);
  const admitted = await ctx.agent.call('waitLog', { file, pattern: `"producerPid":(?!${pred.pid},)\\d+,[^\\n]*"stage":"candidate-admitted"`, deadlineMs: 60_000 });
  const candidatePid = admitted.matched ? Number(/"producerPid":(\d+)/.exec(admitted.matched)[1]) : null;
  const recovers = fault === 'drop-commit-ok-after-record';
  const settled = !candidatePid ? { timedOut: true }
    : recovers ? await ctx.agent.call('waitLog', { file, pattern: stagePattern(candidatePid, 'authority-recovered'), deadlineMs: 60_000 })
    : await ctx.agent.call('waitExit', { pid: candidatePid, deadlineMs: 60_000 });
  const after = await ownerIdentity(ctx);
  const stages = await readStages(ctx, authority.from);
  const candidateStages = stages.filter((s) => s.producerPid === candidatePid).map((s) => s.stage);
  await evidence(ctx, result, `fault-${fault}.json`, { resp, admitted, candidatePid, settled, pred, after, stages });
  assertion(result, `${fault}: candidate admitted (candidate-admitted receipt)`, Boolean(candidatePid), admitted);
  if (!recovers) {
    assertion(result, `${fault}: candidate exited`, Boolean(candidatePid) && !settled.timedOut, settled);
    assertSameOwner(result, pred, after, `${fault}: predecessor remains owner`);
    assertion(result, `${fault}: candidate never adopted or recovered authority`, !candidateStages.some((s) => s === 'authority-adopted' || s === 'authority-recovered'), candidateStages);
    assertion(result, `${fault}: predecessor never committed`, !stages.some((s) => s.producerPid === pred.pid && s.stage === 'predecessor-committed'), stages.map((s) => `${s.producerPid}:${s.stage}`));
  } else {
    assertion(result, `${fault}: candidate recovered the committed handover from the recorded decision`, Boolean(settled.matched), settled);
    assertion(result, `${fault}: the recorded decision made the candidate the owner`, after.pid === candidatePid, { after: after.pid, candidatePid });
  }
  const conn = await dataConn(ctx);
  for (const sid of [pair.local, pair.ssh]) {
    await conn.call(D.attach(sid, 0));
    await nonceRoundTrip(ctx, conn, sid, result, `${fault}: ${sid === pair.local ? 'local' : 'ssh'} writable on predecessor`);
  }
  await conn.close();
  await ctl.close();
  // The request selects only this handover: a later upgrade (Q08b) must run fault-free.
  await ctx.agent.call('remove', { target: path.join(ctx.layout.barrier, HANDOVER_FAULT_REQUEST) });
}

export async function Q08(ctx, result) {
  for (const hook of ['handoverCandidate', 'authorityTransfer']) {
    if (!ctx.hooks[hook]?.present) return block(result, 'HOOK_MISSING', ctx.hooks[hook].owner);
  }
  ctx.sshd = ctx.sshd ?? await startSshd(ctx);
  if (ctx.sshd.unavailable) return block(result, 'SSHD_UNAVAILABLE', ctx.sshd.unavailable);
  const pair = await seedPair(ctx, ctx.state.conn, 'Q08');
  // upgradeStep asserts the candidate never accepted on a public listener before commit, from
  // the accept counters of its handover-authority receipts (todo 8).
  const ab = await upgradeStep(ctx, result, ctx.artifacts.B, pair, 'A_B');
  for (const fn of [(g) => ctx.gui.quit(g), (g) => ctx.gui.kill(g), (g) => ctx.gui.forceTerminate(g)]) await surviveGuiEnd(ctx, result, fn, ctx.artifacts.B);
  const bc = await upgradeStep(ctx, result, ctx.artifacts.C, pair, 'B_C');
  for (const fn of [(g) => ctx.gui.quit(g), (g) => ctx.gui.kill(g), (g) => ctx.gui.forceTerminate(g)]) await surviveGuiEnd(ctx, result, fn, ctx.artifacts.C);
  // A and B drain; the SSH session still lives on A (routes are direct, never chained).
  const aExit = ctx.agent.call('waitExit', { pid: ab.pred.pid, deadlineMs: 60_000 });
  const bExit = ctx.agent.call('waitExit', { pid: bc.pred.pid, deadlineMs: 60_000 });
  const bExited = await bExit;
  assertion(result, 'B exited after its commit (it owns no SSH session)', !bExited.timedOut, bExited);
  assertion(result, 'A still alive while the SSH session is open', (await ctx.agent.call('probe', { pid: ab.pred.pid })) !== null, ab.pred.pid);
  await ctx.state.conn.call(D.close(pair.ssh));
  const aExited = await aExit;
  assertion(result, 'A exited only after the SSH session closed', !aExited.timedOut, aExited);
  const manifest = await ctx.agent.call('readFile', { file: path.join(ctx.layout.runtime, 'handover_routes.json') });
  assertion(result, 'A route removed from the manifest', !manifest || !JSON.parse(manifest).routes?.some((r) => r.sessions?.includes(pair.ssh)), manifest);
  // Four transfer failure variants plus a forced commit failure, each from C to B.
  if (!ctx.hooks.handoverFaultArm?.present) {
    assertion(result, 'transfer failure variants runnable', false, `HOOK_MISSING: ${ctx.hooks.handoverFaultArm?.owner}`);
    return;
  }
  const pair2 = await seedPair(ctx, ctx.state.conn, 'Q08F');
  for (const fault of HANDOVER_FAULTS) await faultVariant(ctx, result, fault, ctx.artifacts.B, pair2);
}

export async function Q08b(ctx, result) {
  if (!ctx.artifacts.v1bump) return block(result, 'ARTIFACT_MISSING', 'handover_v1_rev build (tag daemon-lifetime-handover-v1) not available');
  for (const hook of ['protocolBump', ...(ctx.platform === 'windows' ? ['windowsPending'] : ['handoverCandidate', 'authorityTransfer'])]) {
    if (!ctx.hooks[hook]?.present) return block(result, 'HOOK_MISSING', ctx.hooks[hook].owner);
  }
  const retired = await retireOwner(ctx);
  assertion(result, 'previous QA owner retired', retired.retired, retired);
  const old = await freshOwner(ctx, ctx.artifacts.v1bump, { tag: 'Q08B', dataVersion: 6 });
  await evidence(ctx, result, 'old-owner.json', { old, sameRevision: ctx.revisions.handoverV1Rev === ctx.revisions.head });
  assertion(result, 'old owner speaks protocol 6', old.hello?.protocol_version === 6, old.hello);
  await ctx.gui.quit(ctx.state.gui);
  const gui = await ctx.gui.launch(ctx.artifacts.A);
  ctx.state.gui = gui;
  const mismatch = await ctx.agent.call('dataOpen', { runtimeDir: ctx.layout.runtime, version: 5, allowMismatch: true });
  assertion(result, 'HEAD data handshake is refused with a protocol mismatch', mismatch.handshake?.type !== 'handshakeOk', mismatch.handshake);
  await ctx.agent.call('close', { handle: mismatch.handle });
  const ctl = await controlConn(ctx);
  const hello = await ctl.hello();
  const inv = await ctl.inventory();
  await evidence(ctx, result, 'control.json', { hello, inv });
  assertion(result, 'control Hello succeeds across revisions', hello?.ok === true && hello.pid === old.pid, hello);
  assertion(result, 'control Inventory succeeds across revisions', inv?.ok === true && inventoryTotal(inv) >= 1, inv);
  if (ctx.platform === 'windows') {
    const pending = await ctx.agent.call('waitLog', { file: path.join(ctx.layout.logs, `gui-${gui.tag}.log`), pattern: 'DAEMON_UPDATE_PENDING|DaemonUpdatePending', deadlineMs: DAEMON_READY_TIMEOUT_MS });
    assertion(result, 'Windows enters the pending state', Boolean(pending.matched), pending);
    assertion(result, 'old owner untouched', (await ctx.agent.call('probe', { pid: old.pid })) !== null, old.pid);
  } else {
    ctx.sshd = ctx.sshd ?? await startSshd(ctx);
    if (ctx.sshd.unavailable) return block(result, 'SSHD_UNAVAILABLE', ctx.sshd.unavailable);
    const pair = await seedPair(ctx, ctx.state.conn, 'Q8B');
    await upgradeStep(ctx, result, ctx.artifacts.A, pair, 'V1_HEAD');
    await ctx.state.conn.call(D.close(pair.ssh));
  }
  await ctl.close();
}

export async function Q09(ctx, result) {
  for (const hook of ['protocolBump', 'windowsPending', 'disableControl']) {
    if (!ctx.hooks[hook]?.present) return block(result, 'HOOK_MISSING', ctx.hooks[hook].owner);
  }
  const shotDir = path.join(ctx.layout.root, 'shots');
  const shot = async (name) => {
    const png = await ctx.mahord.screenshot();
    if (png) await ctx.writeEvidence(path.posix.join(ctx.evidenceDir, `Q09-${name}.png`), png);
  };
  void shotDir;
  // Part 1: A (bumped protocol, control on) with live sessions meets B (HEAD).
  await retireOwner(ctx);
  const a = await freshOwner(ctx, ctx.artifacts.bump, { tag: 'Q09A', dataVersion: 6 });
  const extra = await ctx.state.conn.call(D.spawnLocal({ clientRequestId: `qa-q09-${nonce('R')}`, workspaceId: ctx.workspaceId, cwd: ctx.repoDir }));
  const ctlA = await controlConn(ctx);
  const n = inventoryTotal(await ctlA.inventory());
  await ctx.gui.quit(ctx.state.gui);
  const gui = await ctx.gui.launch(ctx.artifacts.A);
  ctx.state.gui = gui;
  const logFile = path.join(ctx.layout.logs, `gui-${gui.tag}.log`);
  const pending = await ctx.agent.call('waitLog', { file: logFile, pattern: `DaemonUpdatePending[^\\n]*\\b${n}\\b|DAEMON_UPDATE_PENDING[^\\n]*\\b${n}\\b`, deadlineMs: DAEMON_READY_TIMEOUT_MS });
  await shot('pending');
  assertion(result, `typed pending state with the exact count ${n}`, Boolean(pending.matched), pending);
  assertion(result, 'A pid unchanged (no Shutdown, no terminate)', (await ctx.agent.call('check', { pid: a.pid })) === 'match', a.pid);
  const log1 = (await ctx.agent.call('readFile', { file: logFile })) ?? '';
  assertion(result, 'GUI sent RetireWhenIdle once on entering pending', (log1.match(/RetireWhenIdle/g) ?? []).length === 1, null);
  await evidence(ctx, result, 'pending-gui.log', log1);
  // stale count -> COUNT_CHANGED, A alive
  const stale = await ctlA.shutdown(n + 1);
  assertion(result, 'stale count answers COUNT_CHANGED', stale?.ok === false && stale.code === 'COUNT_CHANGED', stale);
  assertion(result, 'A alive after the stale confirm', (await ctx.agent.call('probe', { pid: a.pid })) !== null, a.pid);
  // RetireWhenIdle path: close A's terminals without pressing the button.
  const watchdog = (await ctx.jobs.windowsTree(ctx.stagedRoot)).watchdog;
  const aExit = ctx.agent.call('waitExit', { pid: a.pid, deadlineMs: 60_000 });
  const wExit = watchdog ? ctx.agent.call('waitExit', { pid: watchdog.pid, deadlineMs: 60_000 }) : Promise.resolve({ timedOut: true });
  const newOwner = ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid: a.pid, deadlineMs: DAEMON_READY_TIMEOUT_MS * 3 });
  for (const sid of [ctx.state.localA, extra.sessionId]) await ctx.state.conn.call(D.close(sid));
  const aDone = await aExit;
  const wDone = await wExit;
  const marker = await ctx.agent.call('exists', { file: path.join(ctx.layout.runtime, `retired-${ctx.keys.bump}.json`) });
  const changed = await newOwner;
  await evidence(ctx, result, 'retire-when-idle.json', { aDone, wDone, marker, changed, watchdog });
  assertion(result, 'A exited by RetireWhenIdle when its last terminal closed', !aDone.timedOut && marker, { aDone, marker });
  // A respawned A would publish owner.json under A's generation key before the new owner does.
  const newHello = (await ownerIdentity(ctx)).hello;
  assertion(result, 'A watchdog exited (Stop on the retirement marker)', !wDone.timedOut, wDone);
  assertion(result, `no A-generation respawn past 2 x ${RESPAWN_BACKOFF_MS}ms backoff: only one new owner, of the newest generation`,
    Boolean(changed.owner) && changed.seen.filter((p) => p !== a.pid).length === 1 && newHello?.generation_key === ctx.keys.A,
    { seen: changed.seen, newKey: newHello?.generation_key });
  // Part 2: confirm with the exact count ends A and starts the newest generation.
  await retireOwner(ctx);
  const a2 = await freshOwner(ctx, ctx.artifacts.bump, { tag: 'Q09C', dataVersion: 6 });
  await ctx.gui.quit(ctx.state.gui);
  ctx.state.gui = await ctx.gui.launch(ctx.artifacts.A);
  const ctl2 = await controlConn(ctx);
  const n2 = inventoryTotal(await ctl2.inventory());
  const a2Exit = ctx.agent.call('waitExit', { pid: a2.pid, deadlineMs: 60_000 });
  const confirm = await ctl2.shutdown(n2);
  const a2Done = await a2Exit;
  assertion(result, 'confirm with the exact count is accepted and A exits', confirm?.ok === true && !a2Done.timedOut, { confirm, a2Done });
  const started = await ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid: a2.pid, deadlineMs: DAEMON_READY_TIMEOUT_MS * 2 });
  assertion(result, 'newest generation starts after the confirmed exit', Boolean(started.owner), started);
  // Part 3: A without a control endpoint -> button disabled, A untouched.
  await retireOwner(ctx);
  const a3 = await freshOwner(ctx, ctx.artifacts.bump, { tag: 'Q09D', env: { FERRYX_QA_DISABLE_CONTROL: '1' }, dataVersion: 6 });
  assertion(result, 'A started without a control endpoint', (await controlConn(ctx)) === null, null);
  await ctx.gui.quit(ctx.state.gui);
  const g3 = await ctx.gui.launch(ctx.artifacts.A);
  ctx.state.gui = g3;
  const disabled = await ctx.agent.call('waitLog', { file: path.join(ctx.layout.logs, `gui-${g3.tag}.log`), pattern: 'DaemonUpdatePending[^\\n]*(liveSessions|live_sessions)":null|DAEMON_UPDATE_PENDING[^\\n]*null', deadlineMs: DAEMON_READY_TIMEOUT_MS });
  await shot('no-control');
  assertion(result, 'pending state with unknown count (button disabled)', Boolean(disabled.matched), disabled);
  assertion(result, 'A untouched', (await ctx.agent.call('check', { pid: a3.pid })) === 'match', a3.pid);
  await ctlA.close().catch(() => {});
  await ctl2.close().catch(() => {});
}
