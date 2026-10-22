// Q07-Q09: app replacement, control-channel upgrades (Unix), cross-revision owner, Windows pending.

import path from 'node:path';
import { hjoin } from '../lib/hostpath.mjs';
import { assertion, block } from '../lib/summary.mjs';
import { HANDOVER_FAULTS, TRANSFER_FAULTS, FORCED_COMMIT_FAILURE } from '../lib/hooks.mjs';
import { data as D, helloSupportsHandover, inventoryTotal } from '../lib/wire.mjs';
import {
  evidence, dataConn, controlConn, ownerIdentity, nonceRoundTrip, assertInventoryKept, assertSameOwner,
  retireOwner, freshOwner, nonce, counterCommand, interruptSettled, setAsideHeadCatalog, requireOwner, DAEMON_READY_TIMEOUT_MS, RESPAWN_BACKOFF_MS,
} from './common.mjs';
import { surviveGuiEnd } from './q00-q03.mjs';
import { startSshd, recordSshHelper } from '../lib/sshd.mjs';
import { AUTHORITY_ARM, AUTHORITY_STAGE, HANDOVER_FAULT_REQUEST, receiptFile, authorityStages, authorityInvariant, receiptCount, stagePattern } from '../lib/receipts.mjs';

/** Arms the handover-authority receipts for one upgrade; the daemon acks it on its next stage. */
async function armAuthority(ctx, tag) {
  const operationId = `${tag}-${ctx.runId}`;
  await ctx.agent.call('writeFile', { file: hjoin(ctx.layout.barriers, `${AUTHORITY_ARM}.arm.json`), content: JSON.stringify({ name: AUTHORITY_ARM, runId: ctx.runId, operationId }) });
  const text = await ctx.agent.call('readFile', { file: receiptFile(ctx.layout, AUTHORITY_STAGE) });
  return { operationId, from: receiptCount(text) };
}

async function readStages(ctx, from) {
  return authorityStages(await ctx.agent.call('readFile', { file: receiptFile(ctx.layout, AUTHORITY_STAGE) }), ctx.runId, from);
}

/**
 * Adds the sessions a draining predecessor still serves to the owner's describe. DescribeLiveSessions
 * reports a daemon's own sessions only (plan todo 11: its counts back Shutdown's
 * confirm_live_sessions and the retirement predicate, and a routed SSH session does not end with
 * this daemon), so an SSH session kept by an older generation (plan line 51) is described by
 * that generation through its legacy socket, the route handover_routes.json names.
 */
export async function mergeRoutedSessions(after, manifestText, describeAt) {
  const own = new Set((after.sessions ?? []).map((s) => s.backendId));
  const routes = manifestText ? (JSON.parse(manifestText).routes ?? []) : [];
  const sessions = [...(after.sessions ?? [])];
  const routed = [];
  for (const route of routes) {
    const wanted = (route.sessions ?? []).filter((id) => !own.has(id));
    if (wanted.length === 0) continue;
    const described = await describeAt(route.legacySocketPath);
    for (const s of described?.sessions ?? []) {
      if (wanted.includes(s.backendId) && !own.has(s.backendId)) { sessions.push(s); own.add(s.backendId); routed.push({ backendId: s.backendId, via: route.legacySocketPath }); }
    }
  }
  return { ...after, sessions, routed };
}

/**
 * Each route is described on the protocol its own predecessor speaks: the manifest can hold a
 * draining HEAD route (5, Q08's A) next to a draining handover_v1_rev route (6) (rerun2f/2g). The
 * refusal names the version that daemon expects, so a mismatch reconnects once on it.
 */
export async function describeWithRoutes(ctx, conn) {
  const own = await conn.describe();
  const manifest = await ctx.agent.call('readFile', { file: hjoin(ctx.layout.runtime, 'handover_routes.json') });
  return mergeRoutedSessions(own, manifest, async (socketPath) => {
    let legacy = await dataConn(ctx, ctx.layout.runtime, { socketPath, allowMismatch: true });
    if (legacy.handshake?.type === 'protocolMismatch' && Number.isInteger(legacy.handshake.expectedVersion)) {
      await legacy.close();
      legacy = await dataConn(ctx, ctx.layout.runtime, { socketPath, version: legacy.handshake.expectedVersion });
    }
    try { return await legacy.describe(); } finally { await legacy.close(); }
  });
}

export async function Q07(ctx, result) {
  if (!requireOwner(ctx, result)) return;
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
  assertion(result, 'A staged runtime dir intact', await ctx.agent.call('exists', { file: hjoin(ctx.stagedRoot, ctx.keys.A) }), ctx.keys.A);
  assertInventoryKept(result, before, after, 'B attaches the same sessions');
  await nonceRoundTrip(ctx, conn, ctx.state.localA, result, 'B GUI on A session');
  // Replacing the app on disk is not a handover: no process wrote an authority stage.
  const stages = await readStages(ctx, authorityBefore);
  assertion(result, 'no handover-authority stage after the replacement (no transfer started)', stages.length === 0, stages.map((s) => `${s.producerPid}:${s.stage}`));
  if (ctx.platform !== 'windows') {
    const jobB = await ctx.jobs.inspect(ctx.keys.B);
    assertion(result, 'B generation job registered but not running while A is Alive', jobB.state !== 'Running', jobB.state);
  }
  // The B GUI registered the B generation job (supervisor_linux.rs reconcile_login_registration
  // enables the newest registered generation). Left behind, it starts whenever the lock frees and
  // becomes the next owner (run-9a5db442 Q13). Remove it by its recorded name only.
  const bJob = ctx.jobs.jobName(ctx.keys.B);
  const listed = await ctx.jobs.listQaJobs();
  const bInstalled = listed.some((j) => j === bJob || j === `${bJob}.service` || j === `${bJob}.plist`);
  if (bInstalled) await ctx.jobs.remove(bJob);
  const remaining = await ctx.jobs.listQaJobs();
  await evidence(ctx, result, 'b-job-teardown.json', { bJob, bInstalled, listed, remaining });
  assertion(result, 'B generation job removed by its recorded name after the replacement check', !remaining.some((j) => j === bJob || j === `${bJob}.service`), remaining);
  await ctx.state.conn.close().catch(() => {});
  ctx.state.conn = conn;
  // Put A back so later scenarios start from the same artifact.
  await ctx.gui.quit(ctx.state.gui);
  await ctx.replaceAppOnDisk(replaced.artifact, ctx.artifacts.A);
  ctx.state.gui = await ctx.gui.launch(ctx.artifacts.A);
}

/**
 * Seeds a local and an SSH session, each running a 100ms counter tagged `${tag}_L`/`${tag}_S`.
 * The pair carries its counter tag (upgradeStep labels its assertions with a step tag such as
 * A_B, which is not the counter tag) and the owner pid that serves the SSH session: SSH sessions
 * never move on upgrade (plan line 51), so that generation keeps serving it through every later
 * successor and is the one that must stay alive.
 */
export async function seedPair(ctx, conn, tag, { ssh: withSsh = true } = {}) {
  const local = ctx.state.localA;
  const sshOwnerPid = (await ownerIdentity(ctx)).pid;
  await interruptSettled(ctx, conn, local);
  await conn.call(D.write(local, counterCommand(ctx.platform, `${tag}_L`)));
  if (!withSsh) {
    const lo = await conn.waitOutput(local, `${tag}_L_TICK_3`, 15_000);
    if (lo.timedOut) throw new Error('local counter produced no output');
    return { local, ssh: null, tag, sshOwnerPid: null };
  }
  const spawned = await conn.call(D.spawnSsh({ clientRequestId: `qa-ssh-${nonce('R')}`, workspaceId: ctx.sshd.workspaceId, hostStorePath: ctx.sshd.hostStorePath }), 60_000);
  if (spawned.type !== 'spawnOk') throw new Error(`ssh spawn: ${JSON.stringify(spawned)}`);
  await recordSshHelper(ctx);
  await conn.call(D.attach(spawned.sessionId, 0));
  await conn.call(D.write(spawned.sessionId, counterCommand('linux', `${tag}_S`)));
  const lo = await conn.waitOutput(local, `${tag}_L_TICK_3`, 15_000);
  const so = await conn.waitOutput(spawned.sessionId, `${tag}_S_TICK_3`, 30_000);
  if (lo.timedOut || so.timedOut) throw new Error(`counters produced no output (local ${lo.timedOut ? 'silent' : 'ok'}, ssh ${so.timedOut ? 'silent' : 'ok'}; local tail ${JSON.stringify(String(lo.text ?? '').slice(-300))}, ssh tail ${JSON.stringify(String(so.text ?? '').slice(-300))})`);
  return { local, ssh: spawned.sessionId, tag, sshOwnerPid };
}

/** The process that must stay alive while the SSH session is open: the generation it was spawned on. */
export function sshOwnerProbeTarget(pair, pred) {
  return pair.sshOwnerPid ?? pred.pid;
}

/** Highest counter value seen; continuity = values after are > values before with no restart at 1. */
export function counterProgress(text, tag) {
  const values = [...text.matchAll(new RegExp(`${tag}_TICK_(\\d+)`, 'g'))].map((m) => Number(m[1]));
  return { max: values.length ? Math.max(...values) : 0, restarted: values.some((v, i) => i > 0 && v === 1 && values[i - 1] > 1) };
}

/**
 * Bound on the GUI's runtime staging after launch. The GUI stages from its setup thread
 * (lib.rs spawn_blocking ensure_if_absent -> supervisor.rs generation_unit stage_runtime), after
 * admission; omarchy measured 22-28s and up to 63.6s from admission to "Daemon supervisor armed"
 * (run-int2-all runlog-gui-g12 40.3s, g23 63.6s).
 */
export const STAGE_TIMEOUT_MS = 120_000;

/**
 * The staged runtime of artifact's generation, waited for. The GUI stages asynchronously after its
 * window starts, so reading the path right after launch raced it (run-int2-all Q08 A_B: present
 * false, RequestUpgrade refused "staged executable ... does not exist" at 23:48:24.8, the B GUI
 * armed at 23:48:38.3). runtime_stage.rs publishes a generation by renaming a complete temp dir
 * to <root>/<key>, so the wait is on that dir appearing in the root (agent waitPath: directory
 * watch, bounded). waitPath creates the watched file's parent, so it must never watch inside
 * <key>: an empty <key> dir would make the stager reuse it as an existing generation.
 */
export async function stageGeneration(ctx, artifact, { deadlineMs = STAGE_TIMEOUT_MS } = {}) {
  const exe = ctx.platform === 'mac'
    ? hjoin(ctx.stagedRoot, ctx.keys[artifact.name], 'Ferryx Daemon.app', 'Contents', 'MacOS', 'ferryx')
    // runtime_stage.rs:38 stages ferryx.exe on Windows.
    : hjoin(ctx.stagedRoot, ctx.keys[artifact.name], ctx.platform === 'windows' ? 'ferryx.exe' : 'ferryx');
  const generationDir = hjoin(ctx.stagedRoot, ctx.keys[artifact.name]);
  const startedAt = Date.now();
  const appeared = await ctx.agent.call('waitPath', { file: generationDir, deadlineMs });
  return { exe, key: ctx.keys[artifact.name], present: appeared && await ctx.agent.call('exists', { file: exe }), waitedMs: Date.now() - startedAt };
}

/** One control RequestUpgrade from the current owner to artifact; asserts the Q08 contract. */
async function upgradeStep(ctx, result, artifact, pair, tag, { guiDriven = false } = {}) {
  // Q08's GUI-end round trips between A_B and B_C interrupt the pair's local counter and start a
  // QA_A counter instead (rerun2b: B_C local max 297); every step starts from fresh pair counters.
  await restartCounters(ctx, ctx.state.conn, pair);
  // B_C: the SSH session is still A's; B describes only its own sessions.
  const before = await describeWithRoutes(ctx, ctx.state.conn);
  const pred = await ownerIdentity(ctx);
  // Q08b (plan todo 12): a HEAD GUI meeting an older-protocol owner sends RequestUpgrade itself
  // (update_pending::resolve_mismatch). Arm the receipts and the owner watch before it launches,
  // and read the predecessor's Hello while it still owns the control endpoint.
  const early = guiDriven ? await controlConn(ctx) : null;
  const earlyHello = early ? await early.hello() : null;
  const earlyAuthority = guiDriven ? await armAuthority(ctx, tag) : null;
  // <FERRYX_DATA_DIR>/logs/daemon.log (run.mjs daemonLogFile; not imported here: run.mjs imports this module).
  const daemonLog = hjoin(ctx.layout.data, 'logs', 'daemon.log');
  const logFrom = guiDriven ? receiptCount(await ctx.agent.call('readFile', { file: daemonLog })) : 0;
  const earlyChange = guiDriven ? ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid: pred.pid, deadlineMs: DAEMON_READY_TIMEOUT_MS * 2 }) : null;
  // Launching the new GUI stages its runtime; the new GUI itself never starts a second owner.
  await ctx.gui.quit(ctx.state.gui);
  ctx.state.gui = await ctx.gui.launch(artifact);
  const staged = await stageGeneration(ctx, artifact);
  assertion(result, `${tag}: ${artifact.name} runtime staged`, staged.present, staged);
  const ctl = early ?? await controlConn(ctx);
  const hello = earlyHello ?? await ctl.hello();
  assertion(result, `${tag}: predecessor advertises RequestUpgrade + handover-v1`, helloSupportsHandover(hello), hello.ops);
  const authority = earlyAuthority ?? await armAuthority(ctx, tag);
  const ownerChange = earlyChange ?? ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid: pred.pid, deadlineMs: DAEMON_READY_TIMEOUT_MS * 2 });
  if (guiDriven) {
    // The old owner's own record (control_server.rs) that the HEAD GUI's RequestUpgrade was accepted.
    const sent = await ctx.agent.call('waitLog', { file: daemonLog, pattern: 'RequestUpgrade (accepted|refused)[^\\n]*', fromLine: logFrom, deadlineMs: DAEMON_READY_TIMEOUT_MS });
    assertion(result, `${tag}: the HEAD GUI's RequestUpgrade was accepted by the old owner`, /RequestUpgrade accepted/.test(sent.matched ?? ''), sent);
  } else {
    const accepted = await ctl.requestUpgrade(staged.exe, staged.key);
    assertion(result, `${tag}: RequestUpgrade accepted`, accepted?.ok === true && accepted.accepted === true, accepted);
  }
  const changed = await ownerChange;
  const succ = await ownerIdentity(ctx);
  await evidence(ctx, result, `${tag}-owners.json`, { pred, succ, seen: changed.seen });
  assertion(result, `${tag}: owner.json moved straight from predecessor to successor`, changed.owner && changed.seen.filter((p) => p !== pred.pid).length === 1, changed.seen);
  assertion(result, `${tag}: successor serves generation ${artifact.name}`, succ.hello?.generation_key === staged.key, succ.hello?.generation_key);
  // todo 8 receipts: the candidate's own stages through authority-adopted, sampled at each transition.
  // Q08b: a v1 predecessor with no retained session retires 100ms after its commit, which can beat
  // its CommitHandoverOk; the candidate then owns through the recorded decision (authority-recovered,
  // rerun2h), the designed outcome of a lost reply. The predecessor-committed receipt follows the
  // same reply path, so it is required only when the candidate saw the reply.
  const adopted = await ctx.agent.call('waitLog', { file: receiptFile(ctx.layout, AUTHORITY_STAGE), pattern: stagePattern(succ.pid, ownedStagePattern(guiDriven)), deadlineMs: 30_000 });
  const stages = await readStages(ctx, authority.from);
  const inv = authorityInvariant(stages, succ.pid);
  await evidence(ctx, result, `${tag}-authority.json`, { operationId: authority.operationId, stages, inv });
  assertion(result, `${tag}: candidate wrote ${guiDriven ? 'authority-adopted or authority-recovered' : 'authority-adopted'}`, Boolean(adopted.matched), adopted);
  const commit = commitEvidence({ stages, predPid: pred.pid, succPid: succ.pid, guiDriven, ownedLine: adopted.matched });
  assertion(result, `${tag}: ${commit.label}`, commit.ok, inv.stages);
  assertion(result, `${tag}: candidate stages before commit were sampled`, ['candidate-admitted', 'authority-validated', 'commit-sent'].every((s) => inv.candidatePreCommit.includes(s)), inv.candidatePreCommit);
  assertion(result, `${tag}: candidate accepted nothing on any inherited listener before commit`, inv.candidatePreCommit.length > 0 && inv.acceptedBeforeCommit.length === 0, inv.acceptedBeforeCommit);
  assertion(result, `${tag}: a probe flock on both lock files failed at every stage`, stages.length > 0 && inv.lockFreeAt.length === 0, inv.lockFreeAt);
  assertion(result, `${tag}: public socket path present at every stage`, stages.length > 0 && inv.socketMissingAt.length === 0, inv.socketMissingAt);
  // successor parent = OS job
  const row = (await ctx.agent.call('table')).find((r) => r.pid === succ.pid);
  // Windows: Get-ScheduledTask exposes no pid (jobs.inspect pid is null); the generation's task
  // runs a --daemon-supervisor watchdog from <stagedRoot>\<key>, whose --daemon child is the owner.
  const job = ctx.platform === 'windows'
    ? { pid: (await ctx.jobs.windowsTree(hjoin(ctx.stagedRoot, staged.key))).daemon?.pid ?? null }
    : await ctx.jobs.inspect(staged.key);
  assertion(result, `${tag}: successor runs under its own generation job`, job.pid !== null && job.pid === succ.pid, { jobPid: job.pid, succ: succ.pid });
  assertion(result, `${tag}: successor parent is not the predecessor`, row && row.ppid !== pred.pid, row);
  // The generation that owns the SSH session keeps serving it (draining) and is still alive. For
  // B_C that is A, not B: routes are direct and B never owned the session (plan line 51).
  if (pair.ssh) {
    const sshOwner = sshOwnerProbeTarget(pair, pred);
    assertion(result, `${tag}: SSH-owning generation still alive while it serves the SSH session`, (await ctx.agent.call('probe', { pid: sshOwner })) !== null, { sshOwner, pred: pred.pid });
  }
  await ctx.agent.call('record', { pid: succ.pid, role: `daemon-${artifact.name}`, executable: succ.os.executable });
  const conn = guiDriven ? await successorDataConn(ctx, result, tag, succ.pid) : await dataConn(ctx);
  const after = await describeWithRoutes(ctx, conn);
  await evidence(ctx, result, `${tag}-describe.json`, { before, after });
  assertInventoryKept(result, before, after, `${tag}: local + SSH identities`);
  if (pair.ssh) {
    const sshAfter = after.sessions.find((s) => s.backendId === pair.ssh);
    const sshBefore = before.sessions.find((s) => s.backendId === pair.ssh);
    assertion(result, `${tag}: SSH remote_pid unchanged`, sshAfter?.remotePid === sshBefore?.remotePid && sshAfter?.remotePid != null, { before: sshBefore, after: sshAfter });
  }
  for (const sid of pairSessions(pair)) await conn.call(D.attach(sid, 0));
  const { local: lTag, ssh: sTag } = counterTags(pair);
  const lText = (await conn.waitOutput(pair.local, `${lTag}_TICK_`, 5_000)).text ?? '';
  const lp1 = counterProgress(lText, lTag);
  const lNext = await conn.waitOutput(pair.local, `${lTag}_TICK_${lp1.max + 3}`, 10_000);
  assertion(result, `${tag}: local counter continues without restart`, !lNext.timedOut && !counterProgress(lNext.text, lTag).restarted, lp1);
  if (pair.ssh) {
    const sText = (await conn.waitOutput(pair.ssh, `${sTag}_TICK_`, 5_000)).text ?? '';
    const sp1 = counterProgress(sText, sTag);
    const sNext = await conn.waitOutput(pair.ssh, `${sTag}_TICK_${sp1.max + 3}`, 10_000);
    assertion(result, `${tag}: SSH counter continues through the successor without restart`, !sNext.timedOut && !counterProgress(sNext.text, sTag).restarted, sp1);
  }
  for (const sid of pairSessions(pair)) await nonceRoundTrip(ctx, conn, sid, result, `${tag}: ${sid === pair.local ? 'local' : 'ssh'} input once`);
  await ctl.close();
  await ctx.state.conn.close().catch(() => {});
  ctx.state.conn = conn;
  return { pred, succ };
}

/** Counter tags the pair was seeded with (seedPair), never the upgrade step's label. */
/**
 * A data connection served by the successor. A draining handover_v1_rev predecessor still accepts
 * on the shared public data listener (24cbfd25 gates only its control listener, control_server.rs:101
 * there; HEAD gates both through HandoverManager::gated_accept), so a connect can land on it and be
 * refused with protocolMismatch (rerun2e). Each attempt is a new accept on that listener; bounded.
 */
export async function successorDataConn(ctx, result, tag, succPid, attempts = 20) {
  const refused = [];
  for (let i = 0; i < attempts; i += 1) {
    let conn = null;
    try { conn = await dataConn(ctx); } catch (error) { refused.push(String(error.message ?? error)); continue; }
    if (conn.handshake?.pid === succPid) {
      await evidence(ctx, result, `${tag}-data-accepts.json`, { succPid, refusedBeforeSuccessor: refused });
      return conn;
    }
    refused.push(conn.handshake);
    await conn.close();
  }
  await evidence(ctx, result, `${tag}-data-accepts.json`, { succPid, refusedBeforeSuccessor: refused });
  throw new Error(`no data connection reached the successor ${succPid} in ${attempts} accepts`);
}

export function counterTags(pair) {
  return { local: `${pair.tag}_L`, ssh: `${pair.tag}_S` };
}

/**
 * The candidate stage that makes it the owner. Q08b (guiDriven): a v1 predecessor with no
 * retained session retires 100ms after its commit, which can beat its CommitHandoverOk; the
 * candidate then owns through the recorded decision (authority-recovered, rerun2h), the designed
 * outcome of a lost reply. Every HEAD-to-HEAD step must adopt from a received reply.
 */
export function ownedStagePattern(guiDriven) {
  return guiDriven ? '(authority-adopted|authority-recovered)' : 'authority-adopted';
}

/**
 * The commit evidence one upgrade step must show. A HEAD predecessor writes predecessor-prepared
 * and predecessor-committed. A handover_v1_rev predecessor (24cbfd25) writes predecessor-prepared
 * only on its data-socket PrepareHandover path (server.rs:5723 there), never on control
 * RequestUpgrade, so Q08b needs predecessor-committed only. That receipt follows the reply path,
 * so when the reply was lost (the candidate recovered) the candidate must instead show that it
 * sent the commit and then recovered from the recorded decision.
 */
export function commitEvidence({ stages, predPid, succPid, guiDriven, ownedLine }) {
  const by = (pid, stage) => stages.some((r) => r.producerPid === pid && r.stage === stage);
  if (guiDriven && /authority-recovered/.test(ownedLine ?? '')) {
    return { label: 'lost commit reply settled by the recorded decision (commit-sent then authority-recovered)', ok: ['commit-sent', 'authority-recovered'].every((s) => by(succPid, s)) };
  }
  const pred = guiDriven ? ['predecessor-committed'] : ['predecessor-prepared', 'predecessor-committed'];
  return { label: `predecessor wrote ${pred.join(' and ')}`, ok: pred.every((s) => by(predPid, s)) };
}

/** The pair's sessions: Q08b's local-only pair (seedPair { ssh: false }) has no SSH session. */
export function pairSessions(pair) {
  return pair.ssh ? [pair.local, pair.ssh] : [pair.local];
}

/** Next counter tag of a pair: Q08 -> Q08R1 -> Q08R2, so a restarted counter never reads as a restart. */
export function nextCounterTag(tag) {
  const m = /^(.*)R(\d+)$/.exec(tag);
  return m ? `${m[1]}R${Number(m[2]) + 1}` : `${tag}R1`;
}

/**
 * nonceRoundTrip interrupts the counter of each session it writes to (run-q08 rerun2: B_C's
 * counters stopped at the max A_B left them at, 318/305). Restarts both under a fresh tag.
 */
export async function restartCounters(ctx, conn, pair) {
  pair.tag = nextCounterTag(pair.tag);
  const { local, ssh } = counterTags(pair);
  // The connection may be one that never streamed this pair (surviveGuiEnd replaces it).
  for (const sid of pairSessions(pair)) await conn.call(D.attach(sid, 0));
  for (const sid of pairSessions(pair)) await interruptSettled(ctx, conn, sid);
  await conn.call(D.write(pair.local, counterCommand(ctx.platform, local)));
  const lo = await conn.waitOutput(pair.local, `${local}_TICK_3`, 15_000);
  if (lo.timedOut) throw new Error(`restarted counters produced no output (${pair.tag})`);
  if (!pair.ssh) return;
  await conn.call(D.write(pair.ssh, counterCommand('linux', ssh)));
  const so = await conn.waitOutput(pair.ssh, `${ssh}_TICK_3`, 30_000);
  if (so.timedOut) throw new Error(`restarted counters produced no output (${pair.tag})`);
}

/**
 * The artifact a fault variant upgrades to. After drop-commit-ok-after-record the candidate
 * recovers and becomes the owner, so the next variant's request to that same generation is
 * refused "already running this generation" (rerun2 abort-after-adopt), and the generation it
 * replaced still runs its own unit while it drains the SSH session ("successor unit ... is already
 * active", rerun2b). Use a generation that is neither the owner nor running.
 */
export async function faultTarget(ctx, pred, artifact) {
  const usable = async (a) => a && ctx.keys[a.name] !== pred.hello?.generation_key && (await ctx.jobs.inspect(ctx.keys[a.name])).state !== 'Running';
  if (await usable(artifact)) return artifact;
  for (const a of [ctx.artifacts.A, ctx.artifacts.C, ctx.artifacts.B]) if (await usable(a)) return a;
  return artifact;
}

/**
 * One injected handover failure: the predecessor keeps both sessions writable, the candidate
 * exits. A todo 8 transfer failure (TRANSFER_FAULTS) also never reaches CommitHandover.
 */
async function faultVariant(ctx, result, fault, artifact, pair) {
  const pred = await ownerIdentity(ctx);
  artifact = await faultTarget(ctx, pred, artifact);
  const op = `fault-${fault}-${ctx.runId}`;
  await ctx.agent.call('writeFile', { file: hjoin(ctx.layout.barriers, HANDOVER_FAULT_REQUEST), content: JSON.stringify({ runId: ctx.runId, operationId: op, fault }) });
  const authority = await armAuthority(ctx, `F_${fault}`);
  const staged = await stageGeneration(ctx, artifact);
  const ctl = await controlConn(ctx);
  const resp = await ctl.requestUpgrade(staged.exe, staged.key);
  // The candidate is the process that writes candidate-admitted (todo 8). Every variant except a
  // reply dropped after the decision leaves the predecessor serving and the candidate exits;
  // after-record, the recorded decision makes the candidate the owner (authority-recovered).
  const file = receiptFile(ctx.layout, AUTHORITY_STAGE);
  // Only receipts appended after this variant armed its authority count: an earlier upgrade's
  // candidate (run-int1: B 906546 from A_B) wrote candidate-admitted too.
  const admitted = await ctx.agent.call('waitLog', { file, pattern: `"producerPid":(?!${pred.pid},)\\d+,[^\\n]*"stage":"candidate-admitted"`, fromLine: authority.from, deadlineMs: 60_000 });
  const candidatePid = admitted.matched ? Number(/"producerPid":(\d+)/.exec(admitted.matched)[1]) : null;
  const recovers = fault === 'drop-commit-ok-after-record';
  const settled = !candidatePid ? { timedOut: true }
    : recovers ? await ctx.agent.call('waitLog', { file, pattern: stagePattern(candidatePid, 'authority-recovered'), fromLine: authority.from, deadlineMs: 60_000 })
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
    if (TRANSFER_FAULTS.includes(fault)) {
      assertion(result, `${fault}: candidate aborted before commit (candidate-aborted, no commit-sent)`, candidateStages.includes('candidate-aborted') && !candidateStages.includes('commit-sent'), candidateStages);
    }
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
  await ctx.agent.call('remove', { target: hjoin(ctx.layout.barriers, HANDOVER_FAULT_REQUEST) });
}

export async function Q08(ctx, result) {
  if (!requireOwner(ctx, result)) return;
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
  const manifest = await ctx.agent.call('readFile', { file: hjoin(ctx.layout.runtime, 'handover_routes.json') });
  assertion(result, 'A route removed from the manifest', !manifest || !JSON.parse(manifest).routes?.some((r) => r.sessions?.includes(pair.ssh)), manifest);
  // todo 8's four authority-transfer failure variants (missing fd kind, getsockname mismatch,
  // lock fd not locked, request expired), then the forced commit failure as its own case, then
  // the remaining commit-phase faults; each from C to B, each leaving both sessions writable.
  for (const hook of ['handoverFaultArm', 'transferFaultArm']) {
    if (!ctx.hooks[hook]?.present) {
      assertion(result, 'transfer failure variants runnable', false, `HOOK_MISSING: ${ctx.hooks[hook]?.owner}`);
      return;
    }
  }
  const pair2 = await seedPair(ctx, ctx.state.conn, 'Q08F');
  for (const fault of TRANSFER_FAULTS) await faultVariant(ctx, result, fault, ctx.artifacts.B, pair2);
  await faultVariant(ctx, result, FORCED_COMMIT_FAILURE, ctx.artifacts.B, pair2);
  for (const fault of HANDOVER_FAULTS.filter((f) => f !== FORCED_COMMIT_FAILURE)) await faultVariant(ctx, result, fault, ctx.artifacts.B, pair2);
}

/**
 * Linux Q08b: the v1bump owner renders every unit it writes (its own, and the HEAD successor's on
 * RequestUpgrade) without the QA-only variables, because its supervisor_linux forwarded_env
 * predates QA_NAME_KEYS (d9dd5334). Its daemon then runs isolated with no QA supervisor name,
 * supervisor::supported() is false, and RequestUpgrade is refused with "No daemon supervisor is
 * supported in this runtime" (run-q08 rerun daemon.log 17:53:40). Production units never carry
 * these variables and are not isolated, so this is QA plumbing for the old revision, not the
 * upgrade contract: drop-ins give both generation units this run's QA environment.
 */
export async function installV1QaEnv(ctx) {
  const out = {};
  // One prefix drop-in for every generation unit of this run: the v1bump unit is named after its
  // own version (2026.1013.2-<sha>, rerun2b), not the HEAD version the harness keys use.
  const r = await ctx.jobs.installQaEnvDropIn(null, ctx.isolationEnv);
  out.dropIn = { dir: `${ctx.jobs.base}-.service.d`, status: r?.status ?? null, stderr: r?.stderr ?? '' };
  if (r?.status !== 0) throw new Error(`QA env drop-in failed: ${JSON.stringify(out.dropIn)}`);
  return out;
}

export async function Q08b(ctx, result) {
  if (!requireOwner(ctx, result)) return;
  if (!ctx.artifacts.v1bump) return block(result, 'ARTIFACT_MISSING', 'handover_v1_rev build (tag daemon-lifetime-handover-v1) not available');
  for (const hook of ['protocolBump', ...(ctx.platform === 'windows' ? ['windowsPending'] : ['handoverCandidate', 'authorityTransfer'])]) {
    if (!ctx.hooks[hook]?.present) return block(result, 'HOOK_MISSING', ctx.hooks[hook].owner);
  }
  const retired = await retireOwner(ctx);
  assertion(result, 'previous QA owner retired', retired.retired, retired);
  // v1bump (24cbfd25) only reads a version-1 workspace catalog; the HEAD owners wrote version 2.
  await evidence(ctx, result, 'catalog-set-aside.json', await setAsideHeadCatalog(ctx, 'Q08b'));
  if (ctx.platform === 'linux') await evidence(ctx, result, 'qa-env-dropins.json', await installV1QaEnv(ctx));
  const old = await freshOwner(ctx, ctx.artifacts.v1bump, { tag: 'Q08B', dataVersion: 6 });
  await evidence(ctx, result, 'old-owner.json', { old, sameRevision: ctx.revisions.handoverV1Rev === ctx.revisions.head });
  assertion(result, 'old owner speaks protocol 6', old.hello?.protocol_version === 6, old.hello);
  // Probe and seed the old owner BEFORE any HEAD GUI exists: on Unix the HEAD GUI upgrades it on
  // its own (rerun2c: the probes ran after the upgrade and met the HEAD owner).
  const mismatch = await ctx.agent.call('dataOpen', { runtimeDir: ctx.layout.runtime, version: 5, allowMismatch: true });
  assertion(result, 'HEAD data handshake is refused with a protocol mismatch', mismatch.handshake?.type !== 'handshakeOk', mismatch.handshake);
  await ctx.agent.call('close', { handle: mismatch.handle });
  const ctl = await controlConn(ctx);
  const hello = await ctl.hello();
  const inv = await ctl.inventory();
  await evidence(ctx, result, 'control.json', { hello, inv });
  assertion(result, 'control Hello succeeds across revisions', hello?.ok === true && hello.pid === old.pid, hello);
  assertion(result, 'control Inventory succeeds across revisions', inv?.ok === true && inventoryTotal(inv) >= 1, inv);
  await ctl.close();
  if (ctx.platform === 'windows') {
    await ctx.gui.quit(ctx.state.gui);
    const gui = await ctx.gui.launch(ctx.artifacts.A);
    ctx.state.gui = gui;
    const pending = await ctx.agent.call('waitLog', { file: hjoin(ctx.layout.logs, `gui-${gui.tag}.log`), pattern: 'DAEMON_UPDATE_PENDING|DaemonUpdatePending', deadlineMs: DAEMON_READY_TIMEOUT_MS });
    assertion(result, 'Windows enters the pending state', Boolean(pending.matched), pending);
    assertion(result, 'old owner untouched', (await ctx.agent.call('probe', { pid: old.pid })) !== null, old.pid);
  } else {
    // Local session only: the handover_v1_rev build predates FERRYX_QA_SSH_KNOWN_HOSTS (ssh/direct.rs
    // in 24cbfd25 has no QA known_hosts pin), so under StrictHostKeyChecking=yes it can never reach
    // the harness's private sshd (rerun2d: "No ED25519 host key is known"). The plan's Q08b moves
    // the old owner's sessions "as in Q08"; local sessions are the ones that move.
    await evidence(ctx, result, 'ssh-leg.json', { ssh: false, reason: 'handover_v1_rev has no FERRYX_QA_SSH_KNOWN_HOSTS pin; its ssh cannot verify the private sshd host key' });
    const pair = await seedPair(ctx, ctx.state.conn, 'Q8B', { ssh: false });
    await upgradeStep(ctx, result, ctx.artifacts.A, pair, 'V1_HEAD', { guiDriven: true });
  }
}

export async function Q09(ctx, result) {
  if (!requireOwner(ctx, result)) return;
  for (const hook of ['protocolBump', 'windowsPending', 'disableControl']) {
    if (!ctx.hooks[hook]?.present) return block(result, 'HOOK_MISSING', ctx.hooks[hook].owner);
  }
  const shotDir = hjoin(ctx.layout.root, 'shots');
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
  const logFile = hjoin(ctx.layout.logs, `gui-${gui.tag}.log`);
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
  const marker = await ctx.agent.call('exists', { file: hjoin(ctx.layout.runtime, `retired-${ctx.keys.bump}.json`) });
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
  const disabled = await ctx.agent.call('waitLog', { file: hjoin(ctx.layout.logs, `gui-${g3.tag}.log`), pattern: 'DaemonUpdatePending[^\\n]*(liveSessions|live_sessions)":null|DAEMON_UPDATE_PENDING[^\\n]*null', deadlineMs: DAEMON_READY_TIMEOUT_MS });
  await shot('no-control');
  assertion(result, 'pending state with unknown count (button disabled)', Boolean(disabled.matched), disabled);
  assertion(result, 'A untouched', (await ctx.agent.call('check', { pid: a3.pid })) === 'match', a3.pid);
  await ctlA.close().catch(() => {});
  await ctl2.close().catch(() => {});
}
