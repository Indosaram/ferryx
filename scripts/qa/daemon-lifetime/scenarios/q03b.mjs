// Q03b (adoption-objections todo 15, GAP-7): OS-attributed quit after a handover adoption.
//
// The 10-15 incident: a Dock quit of the app after a handover killed the sessions the successor
// had adopted. Q03b runs a Q08-style A->B handover so B adopts local sessions, quits the QA GUI the
// way the OS attributes a quit to the app (mac: Apple Event quit to the QA bundle id; linux: the
// WM close, SUPER+Q through MahoRD), then proves every adopted session's child pid is still the
// same live process and that the predecessor transferred every session it was asked to
// (requested == transferred, from its "Transferred sessions to the successor" log line,
// server.rs transfer_sessions_reply).
//
// Failure case: `run.mjs --only Q03b --fault kill-adopted` exports FERRYX_QA_FAULT=kill-adopted
// (qa_producers.rs QA_FAULT_KILL_ADOPTED, honored only beside FERRYX_QA_RUN_ID); the committed
// successor kills every adopted child, so Q03b FAILs with per-pid evidence.

import { hjoin } from '../lib/hostpath.mjs';
import { assertion, block } from '../lib/summary.mjs';
import { qaBundleIdentifier } from '../lib/guards.mjs';
import { macQuitScript } from '../lib/gui.mjs';
import { receiptCount } from '../lib/receipts.mjs';
import { evidence, dataConn, ownerIdentity, seedLocalSession, assertSameOwner, requireOwner, retireOwner, freshOwner, liveSessions, DAEMON_READY_TIMEOUT_MS } from './common.mjs';
import { seedPair, upgradeStep, faultTarget } from './q07-q09.mjs';

/** server.rs transfer_sessions_reply: the predecessor's record of one completed transfer. */
export const TRANSFERRED_LINE = 'Transferred sessions to the successor over the handover socket';

/**
 * How long each adopted child must outlive the GUI quit. The 10-13/10-15 incidents killed the
 * attributed processes as part of the app's termination, well inside this bound. The wait is the
 * bounded exit-event subscription made before the quit (waitExit), never a sleep.
 */
export const SURVIVAL_WINDOW_MS = 10_000;

/** { requested, transferred } of one TRANSFERRED_LINE (field order independent), or null. */
export function parseTransferredLine(line) {
  if (!line || !String(line).includes(TRANSFERRED_LINE)) return null;
  const requested = /\brequested=(\d+)/.exec(line);
  const transferred = /\btransferred=(\d+)/.exec(line);
  if (!requested || !transferred) return null;
  return { requested: Number(requested[1]), transferred: Number(transferred[1]) };
}

/** The local live sessions the predecessor will hand over, with their child pids. */
export function adoptableSessions(describe) {
  return liveSessions(describe).filter((s) => s.kind === 'local' && Number.isInteger(s.localChildPid));
}

/**
 * Per-pid survival verdict: alive only when the pid did not exit inside the window and still
 * probes as the same process (start time unchanged), so a reused pid never counts as survival.
 */
export function survivalRecord({ sessionId, pid, before, exit, after, successorListed }) {
  const exitedInWindow = Boolean(exit) && !exit.timedOut;
  const sameProcess = Boolean(before) && Boolean(after) && String(before.startTime) === String(after.startTime);
  return { sessionId, pid, before, after, exit, exitedInWindow, successorListed: Boolean(successorListed), alive: !exitedInWindow && sameProcess };
}

/**
 * Turns the transfer log line and the per-pid records into Q03b's assertions and its summary
 * block (result.adoption: adopted=N, alive=N, requested, transferred, per-pid).
 */
export function assertAdoptionSurvival(result, { transfer, records }) {
  const adopted = records.length;
  const alive = records.filter((r) => r.alive).length;
  result.adoption = {
    adopted,
    alive,
    requested: transfer?.requested ?? null,
    transferred: transfer?.transferred ?? null,
    pids: records.map((r) => ({ sessionId: r.sessionId, pid: r.pid, alive: r.alive, exitedInWindow: r.exitedInWindow, successorListed: r.successorListed })),
  };
  assertion(result, 'the successor adopted at least one local session', adopted > 0, result.adoption.pids);
  assertion(result, 'every handed-over session is served by the successor', records.every((r) => r.successorListed), result.adoption.pids);
  assertion(result, 'predecessor logged the transfer (Transferred sessions to the successor)', Boolean(transfer), transfer);
  assertion(result, 'requested == transferred in the predecessor log line', Boolean(transfer) && transfer.requested === transfer.transferred, transfer);
  assertion(result, 'transferred == adopted sessions', Boolean(transfer) && transfer.transferred === adopted, { transferred: transfer?.transferred ?? null, adopted });
  for (const r of records) {
    assertion(result, `adopted session ${r.sessionId} child pid ${r.pid} alive after the OS-attributed quit`, r.alive, { exit: r.exit, before: r.before, after: r.after });
  }
  assertion(result, `alive == adopted (${alive}/${adopted})`, adopted > 0 && alive === adopted, { adopted, alive });
  return result.adoption;
}

/** The QA bundle id the mac quit targets; refuses anything that is not this run's QA id. */
export function qaQuitTarget(ctx) {
  const expected = qaBundleIdentifier(ctx.runId);
  if (ctx.gui.bundleId !== expected) throw new Error(`refusing to quit ${ctx.gui.bundleId}: not this run's QA bundle id ${expected}`);
  return expected;
}

/** Puts generation A back as the owner so later scenarios (Q07, Q08) start from A. */
async function restoreOwnerA(ctx, result, successorKey) {
  const retired = await retireOwner(ctx);
  const jobName = successorKey && successorKey !== ctx.keys.A ? ctx.jobs.jobName(successorKey) : null;
  if (jobName) await ctx.jobs.remove(jobName);
  const remaining = await ctx.jobs.listQaJobs();
  const fresh = await freshOwner(ctx, ctx.artifacts.A, { tag: 'QA_A', expectKey: ctx.keys.A });
  await evidence(ctx, result, 'restore-owner.json', { retired, removedJob: jobName, remaining, fresh: fresh.pid });
  assertion(result, 'successor retired and generation A restored for later scenarios', retired.retired && fresh.hello?.generation_key === ctx.keys.A, { retired, fresh: fresh.pid });
}

export async function Q03b(ctx, result) {
  if (!requireOwner(ctx, result)) return;
  for (const hook of ['handoverCandidate', 'authorityTransfer', 'killAdoptedFault']) {
    if (!ctx.hooks[hook]?.present) return block(result, 'HOOK_MISSING', ctx.hooks[hook]?.owner ?? hook);
  }
  if (ctx.platform !== 'mac' && !ctx.mahord) return block(result, 'MAHORD_UNAVAILABLE', 'the linux WM close needs MahoRD');
  const quitTarget = ctx.platform === 'mac' ? qaQuitTarget(ctx) : null;

  // 1. Two local sessions on the current owner; the A->B handover adopts both.
  const pair = await seedPair(ctx, ctx.state.conn, 'Q3B', { ssh: false });
  await seedLocalSession(ctx, ctx.state.conn, 'Q3B2');
  const handedOver = adoptableSessions(await ctx.state.conn.describe());
  const probed = [];
  for (const s of handedOver) probed.push({ sessionId: s.backendId, pid: s.localChildPid, before: await ctx.agent.call('probe', { pid: s.localChildPid }) });
  await evidence(ctx, result, 'handed-over.json', { handedOver, probed });

  const daemonLog = hjoin(ctx.layout.data, 'logs', 'daemon.log');
  const logFrom = receiptCount(await ctx.agent.call('readFile', { file: daemonLog }));
  const pred = await ownerIdentity(ctx);
  const target = await faultTarget(ctx, pred, ctx.artifacts.B);
  let step = null;
  try {
    // The commit may settle through the recorded decision (authority-recovered) on Linux.
    step = await upgradeStep(ctx, result, target, pair, 'Q3B_HANDOVER', { recoverable: true });
  } catch (error) {
    // kill-adopted (or a real loss) can break the step's own round trips; the per-pid survival
    // check below still runs so the FAIL keeps its per-pid evidence.
    assertion(result, 'Q08-style handover step completed', false, String(error?.stack ?? error));
  }
  const transferLine = await ctx.agent.call('waitLog', { file: daemonLog, pattern: `${TRANSFER_PATTERN}[^\\n]*`, fromLine: logFrom, deadlineMs: DAEMON_READY_TIMEOUT_MS });
  const transferredRow = transferLine.matched ? await fullLogLine(ctx, daemonLog, logFrom) : null;
  const transfer = parseTransferredLine(transferredRow);
  await evidence(ctx, result, 'transfer-line.json', { from: logFrom, line: transferredRow, transfer, waited: transferLine });

  const succ = await ownerIdentity(ctx);
  let successorIds = new Set();
  try {
    const conn = await dataConn(ctx);
    successorIds = new Set(((await conn.describe()).sessions ?? []).map((s) => s.backendId));
    await conn.close();
  } catch (error) {
    await evidence(ctx, result, 'successor-describe-error.txt', String(error?.stack ?? error));
  }

  // 2. Quit the QA GUI the way the OS attributes a quit to the app. Each adopted child's exit
  // event is subscribed before the quit is sent.
  const exits = probed.map((p) => ctx.agent.call('waitExit', { pid: p.pid, deadlineMs: SURVIVAL_WINDOW_MS }));
  const gui = ctx.state.gui;
  const quit = await ctx.gui.quit(gui);
  ctx.state.gui = null;
  await evidence(ctx, result, 'quit.json', { how: ctx.platform === 'mac' ? { command: '/usr/bin/osascript', args: ['-e', macQuitScript(quitTarget)] } : { mahord: 'Meta+q (WM close)' }, gui, quit });
  assertion(result, 'QA GUI exited on the OS-attributed quit', Boolean(quit) && !quit.timedOut && !quit.skipped, quit);

  // 3. Every adopted child pid is the same live process; 4. requested == transferred.
  const settled = await Promise.all(exits);
  const records = [];
  for (const [i, p] of probed.entries()) {
    records.push(survivalRecord({ ...p, exit: settled[i], after: await ctx.agent.call('probe', { pid: p.pid }), successorListed: successorIds.has(p.sessionId) }));
  }
  await evidence(ctx, result, 'per-pid.json', records);
  assertAdoptionSurvival(result, { transfer, records });
  const ownerAfter = await ownerIdentity(ctx);
  assertSameOwner(result, succ, ownerAfter, 'successor after the OS-attributed quit');
  await evidence(ctx, result, 'owners.json', { pred, succ, ownerAfter, step: step ? { pred: step.pred.pid, succ: step.succ.pid } : null });

  await restoreOwnerA(ctx, result, succ.hello?.generation_key ?? null);
}

const TRANSFER_PATTERN = TRANSFERRED_LINE.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** The complete daemon.log line of the transfer (waitLog's match may stop before its fields). */
async function fullLogLine(ctx, file, fromLine) {
  const text = await ctx.agent.call('readFile', { file });
  return String(text ?? '').split(/\r?\n/).filter((line) => line.trim()).slice(fromLine).find((line) => line.includes(TRANSFERRED_LINE)) ?? null;
}
