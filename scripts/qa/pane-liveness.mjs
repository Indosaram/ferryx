#!/usr/bin/env node
// Task 3 QA runner (local-pane-liveness-root-remediation).
// Strict native-default CLI:
//   node scripts/qa/pane-liveness.mjs --scenario NAME --binary ABS \
//     --evidence-dir ABS --isolation-root ABS
// `--headless` is accepted ONLY for diagnostic-classifier within Task 3 and
// records nativeEvidence: deferred-to-task-10; it can never yield native PASS.
// Production/default runtime roots, preexisting isolation roots, relative
// paths, unknown flags and unlisted scenarios are rejected nonzero BEFORE
// any launch. Unsupported native automation fails explicitly (typed exit
// codes). No fake API or stub PASS exists in this runner: every scenario
// settles through receipts that echo the unique runId/operationId nonces.

import { randomUUID } from 'node:crypto';
import { join, resolve } from 'node:path';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import {
  BUDGETS, EXIT, HarnessError, parseInvocation, preflight,
  computeCleanupGate, computeSourceDigest, withDeadline,
  assertPositiveRecovery, BARRIER_ROLES,
} from '../lib/qa-scenarios/common-harness.mjs';
import { runHeadlessDiagnosticClassifier, buildIsolatedEnv } from '../lib/qa-scenarios/diagnostic-classifier.mjs';
import {
  MARKER_TEXT, assertAxTrustDarwin, assertNativeAutomationSupported, assertScreenCapture,
  captureScreenshot, clickSplitRightDarwin, focusWindowByPidDarwin, typeMarkerDarwin,
  awaitMarkerRecognition, focusWindowWindows, typeMarkerWindows, windowsDriver,
} from '../lib/qa-scenarios/native-driver.mjs';

const SOURCE_FILES = [
  'scripts/qa/pane-liveness.mjs',
  'scripts/lib/qa-scenarios/common-harness.mjs',
  'scripts/lib/qa-scenarios/native-driver.mjs',
  'scripts/lib/qa-scenarios/diagnostic-classifier.mjs',
];
const runnerRoot = join(fileURLToPath(new URL('.', import.meta.url)), '../..');

// Per-scenario plan: barriers to pre-arm before launch and the invariants the
// driver asserts on settled receipts. Barrier semantics are defined by the
// private channel (task-3-rust-proposal.md); a binary without local-split-qa
// support fails the registration ACK explicitly (typed BARRIER_ACK_TIMEOUT).
const SCENARIO_PLANS = {
  'diagnostic-classifier': {
    barriers: ['backend-write', 'presentation'], marker: true, splitMenu: false,
    receipts: ['fixture-setup', 'classifier', 'marker-output'],
  },
  'split-happy': {
    barriers: [], marker: true, splitMenu: true,
    receipts: ['fixture-setup', 'split-create', 'presentation', 'marker-output'],
    fiveTuple: true, timings: true, singlePty: true,
  },
  'split-attach-stall': {
    barriers: ['attach-handshake'], barrierHoldMs: 16_000, marker: false, splitMenu: true,
    receipts: ['fixture-setup', 'split-create', 'failure-classified'],
    failureDeadlineMs: BUDGETS.attemptCeilingMs, sameIdRetry: true,
  },
  'split-cancel': {
    barriers: [], marker: false, splitMenu: true,
    receipts: ['fixture-setup', 'split-create', 'cancel-ack'],
    cancel: { request: 'split-cancel', phase: 'while-creating' },
    cancelAckCeilingMs: BUDGETS.cancelAckCeilingMs, singlePty: true,
  },
  'split-concurrent': {
    barriers: ['held-rpc'], marker: true, splitMenu: true,
    receipts: ['fixture-setup', 'held-rpc', 'split-create', 'presentation', 'marker-output'],
    requireHeldRpc: true, fiveTuple: true, timings: true, singlePty: true,
  },
  'retained-handover': {
    barriers: ['predecessor-export', 'successor-adopt'], marker: true, splitMenu: false,
    receipts: ['fixture-setup', 'handover-transfer', 'marker-output'],
    invariants: ['handoverPreservesIncarnation', 'singleReader'],
  },
  'handover-abort': {
    barriers: ['commit', 'abort'], marker: true, splitMenu: false,
    receipts: ['fixture-setup', 'rollback-relinquishment', 'marker-output'],
    invariants: ['relinquishmentBeforeResume', 'singleReader', 'noDualRead'],
  },
  'suspension-ownership': {
    barriers: [], marker: true, splitMenu: false,
    receipts: ['fixture-setup', 'suspension-receipt', 'marker-output'],
    invariants: ['externalStopsUntouched', 'ownedResumeSameProcess'],
  },
  'stale-binding': {
    barriers: [], marker: true, splitMenu: false,
    receipts: ['fixture-setup', 'reattach-marker', 'stale-receipt-rejected'],
    invariants: ['staleReceiptRejected', 'reattachSameBackend'],
  },
};

// Review blocker 4: the five-tuple is not presence-only. It must correlate
// with the authoritative backendSessionId/attemptGeneration established by
// the split-create receipt for THIS attempt.
export function requireFiveTupleReceipt(receipt, expected, label) {
  const fiveTuple = ['frontendSessionId', 'paneIdentity', 'backendSessionId', 'bindingKey', 'attemptGeneration'];
  for (const field of fiveTuple) {
    if (receipt?.[field] === undefined || receipt?.[field] === null || receipt?.[field] === '') {
      throw new HarnessError('ASSERTION_FAILURE', `${label}: presentation receipt missing 5-tuple field ${field}: ${JSON.stringify(receipt)}`);
    }
  }
  if (expected.backendSessionId !== undefined && receipt.backendSessionId !== expected.backendSessionId) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: backendSessionId ${receipt.backendSessionId} does not match the created backend ${expected.backendSessionId}`);
  }
  if (expected.bindingKey !== undefined && receipt.bindingKey !== expected.bindingKey) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: bindingKey ${receipt.bindingKey} does not match the launch binding ${expected.bindingKey}`);
  }
  if (expected.attemptGeneration !== undefined && receipt.attemptGeneration !== expected.attemptGeneration) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: attemptGeneration ${receipt.attemptGeneration} does not match the created attempt ${expected.attemptGeneration}`);
  }
  return receipt;
}

// Review blocker 3: per-invariant machine checks; logging a receipt alone
// never passes.
export function assertInvariants(names, receipt) {
  const r = receipt ?? {};
  if (names.includes('handoverPreservesIncarnation')) {
    if (!r.originalBackendSessionId || r.adoptedBackendSessionId !== r.originalBackendSessionId) {
      throw new HarnessError('ASSERTION_FAILURE', `retained-handover: incarnation not preserved across transfer: ${JSON.stringify(r)}`);
    }
    if (r.originalIncarnation == null || r.adoptedIncarnation == null) {
      throw new HarnessError('TASK4_IDENTITY_DEPENDENCY', `retained-handover: baseline incarnation is null - Task 4 identity reconciliation dependency unavailable (creation nonce not yet implemented): ${JSON.stringify(r)}`);
    }
    if (typeof r.originalIncarnation !== 'string' || r.adoptedIncarnation !== r.originalIncarnation) {
      throw new HarnessError('ASSERTION_FAILURE', `retained-handover: creation incarnation changed across transfer: ${JSON.stringify(r)}`);
    }
  }
  if (names.includes('relinquishmentBeforeResume')) {
    if (r.relinquishmentReceiptReceived !== true || typeof r.successorReaderReleased !== 'boolean') {
      throw new HarnessError('ASSERTION_FAILURE', `handover-abort: no authoritative relinquishment receipt before predecessor resume: ${JSON.stringify(r)}`);
    }
  }
  if (names.includes('singleReader')) {
    if (r.readerCount !== 1) throw new HarnessError('ASSERTION_FAILURE', `single-reader invariant violated (readerCount=${JSON.stringify(r.readerCount)}): ${JSON.stringify(r)}`);
  }
  if (names.includes('noDualRead')) {
    if (r.dualReadObserved === true || (Array.isArray(r.readCounts) && r.readCounts.filter(c => c > 0).length > 1)) {
      throw new HarnessError('ASSERTION_FAILURE', `dual read observed during rollback: ${JSON.stringify(r)}`);
    }
  }
  if (names.includes('externalStopsUntouched')) {
    if (r.externallyStoppedAutoResumed === true) throw new HarnessError('ASSERTION_FAILURE', `external stop was auto-resumed: ${JSON.stringify(r)}`);
    if (r.externallyStoppedProbeState !== 'stopped') throw new HarnessError('ASSERTION_FAILURE', `external stop not observed as stopped: ${JSON.stringify(r)}`);
  }
  if (names.includes('ownedResumeSameProcess')) {
    if (r.ownedResumePid !== r.ownedSuspendPid || r.ownedResumed !== true) {
      throw new HarnessError('ASSERTION_FAILURE', `owned suspension did not resume the same process: ${JSON.stringify(r)}`);
    }
    if (r.verifiedActuationReceipt !== true) throw new HarnessError('ASSERTION_FAILURE', `resume without verified actuation receipt: ${JSON.stringify(r)}`);
  }
  if (names.includes('staleReceiptRejected')) {
    if (r.rejected !== true || typeof r.reason !== 'string' || r.reason.length === 0) {
      throw new HarnessError('ASSERTION_FAILURE', `stale receipt was not explicitly rejected: ${JSON.stringify(r)}`);
    }
  }
  if (names.includes('reattachSameBackend')) {
    if (!r.backendSessionId || r.newPtyCreated === true) {
      throw new HarnessError('ASSERTION_FAILURE', `legitimate reattach must reuse the backend without a new PTY: ${JSON.stringify(r)}`);
    }
  }
  return true;
}

function assertSinglePty(receipt, label) {
  if (receipt?.ptyCreatedCount === undefined || receipt.ptyCreatedCount > 1) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: duplicate/missing PTY accounting: ${JSON.stringify(receipt)}`);
  }
  return true;
}

async function runNativeScenario(ctx) {
  const plan = SCENARIO_PLANS[ctx.scenario];
  const { evidence, barrierHub } = ctx;

  // Permission gates FIRST: typed rejections with no native actions recorded.
  assertNativeAutomationSupported();
  if (ctx.platformPreflight === 'darwin') {
    await assertAxTrustDarwin(evidence);
    await assertScreenCapture(evidence);
  }

  const isolated = buildIsolatedEnv(ctx);
  evidence.action({ action: 'launch.binary', binary: ctx.binary, env: { FERRYX_DATA_DIR: isolated.dirs.dataDir, FERRYX_RUNTIME_DIR: isolated.dirs.runtimeDir, FERRYX_QA_BARRIER_DIR: barrierHub.dir } });
  const child = ctx.spawnOwned(ctx.binary, [], { env: isolated.env });
  const pid = child.pid;

  // Fixture sessions (created/adopted/externally-stopped/idle) acknowledged
  // by the product before any native action. Review P3: strict schema - each
  // kind exactly once, every session carries a non-empty backendSessionId and
  // an ownership receipt; the externally-stopped fixture must include its
  // stop evidence. Four anonymous objects never pass.
  const fixture = await barrierHub.awaitReceipt('fixture-setup', 0, BUDGETS.stagePrepareCreateStatusMs);
  const sessions = Array.isArray(fixture?.sessions) ? fixture.sessions : [];
  const byKind = new Map();
  for (const session of sessions) {
    if (!session || typeof session.backendSessionId !== 'string' || session.backendSessionId.length === 0
      || typeof session.ownershipReceipt !== 'object' || session.ownershipReceipt === null) {
      throw new HarnessError('ASSERTION_FAILURE', `fixture-setup session lacks backendSessionId/ownershipReceipt: ${JSON.stringify(session)}`);
    }
    byKind.set(session.kind, [...(byKind.get(session.kind) ?? []), session]);
  }
  for (const kind of ['created', 'adopted', 'externally-stopped', 'idle']) {
    const entries = byKind.get(kind) ?? [];
    if (entries.length !== 1) throw new HarnessError('ASSERTION_FAILURE', `fixture-setup requires exactly one ${kind} session, got ${entries.length}`);
  }
  const stopped = byKind.get('externally-stopped')[0];
  if (typeof stopped.stopProbeState !== 'string' || stopped.stopProbeState !== 'stopped') {
    throw new HarnessError('ASSERTION_FAILURE', `externally-stopped fixture lacks stop evidence: ${JSON.stringify(stopped)}`);
  }
  evidence.action({ action: 'fixture-setup', sessions: fixture.sessions });

  // Every armed barrier must be registered by the product before triggers.
  for (const barrier of plan.barriers) await barrierHub.awaitRegistered(barrier);
  if (plan.barriers.length > 0) evidence.action({ action: 'barriers.registered', barriers: [...plan.barriers] });

  // Review blocker 1: the monotonic budget starts at the actual decisive
  // trigger (menu click, cancel request, or marker typing on non-split
  // scenarios), and attemptMs is finalized only AFTER the last decisive await.
  const triggerAt = Date.now();
  const deadlineAt = triggerAt + BUDGETS.attemptCeilingMs;
  let triggerLabel = 'split-menu-click';

  if (plan.splitMenu && !plan.cancel) {
    if (ctx.platformPreflight === 'win32') await windowsDriver(evidence, pid);
    else { await focusWindowByPidDarwin(evidence, pid); await clickSplitRightDarwin(evidence, pid); }
  }

  let expectedFiveTuple = {};
  let presentationReceipt = null;
  let markerFrame = null;
  if (plan.receipts.includes('split-create')) {
    const create = await barrierHub.awaitReceipt('split-create', 0, BUDGETS.stagePrepareCreateStatusMs);
    expectedFiveTuple = { backendSessionId: create.backendSessionId, attemptGeneration: create.attemptGeneration };
    if (typeof create.backendSessionId !== 'string' || create.backendSessionId.length === 0) {
      throw new HarnessError('ASSERTION_FAILURE', `split-create receipt lacks authoritative backendSessionId: ${JSON.stringify(create)}`);
    }
    // Dynamic targetBackendSessionId binding without fake preknown backend:
    // Once authoritative backendSessionId is emitted by split-create, bind it to session-targeted barriers (e.g. presentation).
    if (barrierHub.isArmed('presentation')) {
      barrierHub.bindBackendSession('presentation', create.backendSessionId);
    }
    evidence.action({ action: 'split-create', receipt: create, boundBackendSessionId: create.backendSessionId });
  }

  // Review blocker 2: split-cancel issues a real cancel request through the
  // private channel while creation is in flight, then demands an
  // authoritative acknowledgement and cleanup receipt.
  if (plan.cancel) {
    if (ctx.platformPreflight === 'win32') await windowsDriver(evidence, pid);
    else { await focusWindowByPidDarwin(evidence, pid); await clickSplitRightDarwin(evidence, pid); }
    triggerLabel = 'cancel-request';
    barrierHub.command(plan.cancel.request, { phase: plan.cancel.phase });
    evidence.action({ action: 'cancel-request', request: plan.cancel.request, phase: plan.cancel.phase });
  }

  if (plan.requireHeldRpc) {
    const held = await barrierHub.awaitHeld('held-rpc');
    if (held?.heldRpc !== true) throw new HarnessError('ASSERTION_FAILURE', `split-concurrent requires heldRpc: true, got ${JSON.stringify(held)}`);
    evidence.action({ action: 'held-rpc.confirmed', heldRpc: true });
  }
  if (plan.barriers.includes('attach-handshake')) {
    const holdStartedAt = Date.now();
    const held = await barrierHub.awaitHeld('attach-handshake', plan.barrierHoldMs ?? BUDGETS.attemptCeilingMs);
    // Review L3: report both the configured budget and the measured span.
    evidence.action({ action: 'barrier.held', barrier: 'attach-handshake', configuredHoldBudgetMs: plan.barrierHoldMs ?? BUDGETS.attemptCeilingMs, measuredHeldMs: Date.now() - holdStartedAt });
    // Correlate actual operation/backend before binding downstream barriers:
    const authoritativeSessionId = held?.sessionId ?? held?.backendSessionId;
    if (authoritativeSessionId && typeof authoritativeSessionId === 'string') {
      if (barrierHub.isArmed('presentation')) {
        barrierHub.bindBackendSession('presentation', authoritativeSessionId);
      }
      evidence.action({ action: 'attach-handshake.bound', boundBackendSessionId: authoritativeSessionId });
    }
  }

  // Decisive receipts.
  if (plan.receipts.includes('presentation')) {
    const presentation = await barrierHub.awaitReceipt('presentation', 0, BUDGETS.attemptCeilingMs);
    // Root ruling: the five-tuple is a FINAL native acceptance requirement -
    // NOT waived because current Rust native state lacks the fields. Until
    // the product emits them (unresolved dependency, Architect91 owns the
    // solution), this gate fails typed with the missing field named. It is
    // never relaxed or fabricated.
    requireFiveTupleReceipt(presentation, expectedFiveTuple, 'presentation');
    // frameSubmitted is distinct and necessary-but-not-sufficient: proof of
    // presentation additionally requires independent marker recognition below.
    if (typeof presentation.backendSessionId !== 'string' || presentation.backendSessionId.length === 0) {
      throw new HarnessError('ASSERTION_FAILURE', `presentation receipt lacks authoritative backendSessionId: ${JSON.stringify(presentation)}`);
    }
    if (expectedFiveTuple.backendSessionId !== undefined && presentation.backendSessionId !== expectedFiveTuple.backendSessionId) {
      throw new HarnessError('ASSERTION_FAILURE', `presentation backendSessionId ${presentation.backendSessionId} does not match the created backend ${expectedFiveTuple.backendSessionId}`);
    }
    if (presentation.frameSubmitted !== true) {
      throw new HarnessError('ASSERTION_FAILURE', `presentation receipt attests no submitted frame: ${JSON.stringify(presentation)}`);
    }
    presentationReceipt = presentation;
    evidence.action({ action: 'presentation-receipt', receipt: presentation });
  }
  if (plan.receipts.includes('failure-classified')) {
    // Review M4: await with a grace window beyond the ceiling so a failure
    // genuinely delivered LATE is observed and classified FAIL instead of
    // surfacing as a BLOCKED barrier timeout that hides the violation.
    const graceMs = (plan.barrierHoldMs ?? BUDGETS.attemptCeilingMs) + 1_000;
    const failure = await barrierHub.awaitReceipt('attach-handshake', 0, graceMs);
    if (failure?.actionable !== true) throw new HarnessError('ASSERTION_FAILURE', `stalled attach must settle with an actionable failure: ${JSON.stringify(failure)}`);
    if (Date.now() > deadlineAt) {
      throw new HarnessError('ASSERTION_FAILURE', `failure observed after the 15s deadline (runner-measured); late delivery must FAIL, not BLOCK`);
    }
    // Fault duration vs attempt duration: a 16s held attach must cause a
    // timely failure by 15s; a failure delivered at 16s must FAIL.
    if (typeof failure.failureDeliveredAtMs !== 'number' || failure.failureDeliveredAtMs > BUDGETS.attemptCeilingMs) {
      throw new HarnessError('ASSERTION_FAILURE', `failure not delivered within the 15s ceiling: ${JSON.stringify(failure)}`);
    }
    if (plan.sameIdRetry && failure.retryMustReuseId === true && typeof failure.backendSessionId !== 'string') {
      throw new HarnessError('ASSERTION_FAILURE', `same-ID retry contract requires an authoritative backendSessionId: ${JSON.stringify(failure)}`);
    }
    evidence.action({ action: 'failure-classified', receipt: failure });
  }
  if (plan.receipts.includes('cancel-ack')) {
    const cancel = await barrierHub.awaitReceipt('cancel-ack', 0, plan.cancelAckCeilingMs);
    if (typeof cancel?.cancelAckMs !== 'number' || cancel.cancelAckMs > BUDGETS.cancelAckCeilingMs) {
      throw new HarnessError('ASSERTION_FAILURE', `cancel acknowledgement exceeds ${BUDGETS.cancelAckCeilingMs}ms: ${JSON.stringify(cancel)}`);
    }
    if (cancel.cancelAckMs > BUDGETS.cancelDaemonResponseBudgetMs + (cancel.timerDispatchLatencyMs ?? 0)) {
      throw new HarnessError('ASSERTION_FAILURE', `cancel acknowledgement exceeds daemon response budget: ${JSON.stringify(cancel)}`);
    }
    if (cancel.cleanupReceipt?.authoritative !== true) {
      throw new HarnessError('ASSERTION_FAILURE', `cancel lacks an authoritative cleanup receipt (never inferred from the ack): ${JSON.stringify(cancel)}`);
    }
    evidence.action({ action: 'cancel-ack', receipt: cancel });
  }
  if (plan.receipts.includes('classifier')) {
    // Review blocker 7: native classifier verdicts are asserted against the
    // held/released barrier states, not merely received.
    await barrierHub.awaitHeld('backend-write');
    const writeHeld = await barrierHub.awaitReceipt('backend-write', 0, BUDGETS.stageAttachListenerMs);
    if (writeHeld.classifierVerdict !== 'BlockedInIpcWrite') {
      throw new HarnessError('ASSERTION_FAILURE', `classifier must report BlockedInIpcWrite while the writer is held, got ${JSON.stringify(writeHeld.classifierVerdict)}`);
    }
    barrierHub.release('backend-write');
    const writeRecovered = await barrierHub.awaitReceipt('backend-write', 1, BUDGETS.stagePrepareCreateStatusMs);
    // Repair B4: positive observed recovery only; Unknown is nonpassing.
    assertPositiveRecovery(writeRecovered, 'classifier after write release');
    await barrierHub.awaitHeld('presentation');
    const presentationHeld = await barrierHub.awaitReceipt('presentation', 0, BUDGETS.stagePresentationMs);
    if (presentationHeld.classifierVerdict !== 'BlockedInPresentation') {
      throw new HarnessError('ASSERTION_FAILURE', `classifier must report BlockedInPresentation while presentation is held, got ${JSON.stringify(presentationHeld.classifierVerdict)}`);
    }
    barrierHub.release('presentation');
    const presentationRecovered = await barrierHub.awaitReceipt('presentation', 1, BUDGETS.stagePrepareCreateStatusMs);
    assertPositiveRecovery(presentationRecovered, 'classifier after presentation release');
    evidence.action({ action: 'classifier-stages', classifierStages: { writeHeld: writeHeld.classifierVerdict, writeRecovered: writeRecovered.classifierVerdict, presentationHeld: presentationHeld.classifierVerdict, presentationRecovered: presentationRecovered.classifierVerdict } });
  }
  for (const receiptName of ['handover-transfer', 'rollback-relinquishment', 'suspension-receipt', 'reattach-marker', 'stale-receipt-rejected']) {
    if (plan.receipts.includes(receiptName)) {
      const receipt = await barrierHub.awaitReceipt(receiptName, 0, BUDGETS.attemptCeilingMs);
      if (plan.invariants?.length) assertInvariants(plan.invariants, receipt);
      evidence.action({ action: 'scenario-receipt', name: receiptName, receipt });
    }
  }

  // Marker output is decisive when the plan requires a visible marker; only
  // after it resolves is attemptMs finalized (review blocker 1).
  if (plan.marker) {
    if (ctx.platformPreflight === 'win32') {
      // Review blocker 5: the Windows marker is typed on EVERY marker path,
      // split or not (focus + SendKeys), never only inside the split flow.
      await focusWindowWindows(evidence, pid);
      await typeMarkerWindows(evidence, pid);
    } else {
      if (!plan.splitMenu && !plan.cancel) triggerLabel = 'marker-typing';
      await typeMarkerDarwin(evidence, pid);
    }
    const markerReceipt = await barrierHub.awaitReceipt('marker-output', 0, BUDGETS.stagePresentationMs + BUDGETS.stageAttachListenerMs);
    if (!String(markerReceipt?.output ?? '').includes(MARKER_TEXT)) {
      throw new HarnessError('ASSERTION_FAILURE', `marker ${MARKER_TEXT} not observed in output receipt: ${JSON.stringify(markerReceipt)}`);
    }
    // Channel v2: a matching frameSubmitted receipt alone never PASSes. It is
    // recorded here and the screenshot marker correlation below must also
    // hold before any native PASS is minted.
    if (markerReceipt.frameSubmitted !== true) {
      throw new HarnessError('ASSERTION_FAILURE', `marker receipt attests no submitted frame: ${JSON.stringify(markerReceipt)}`);
    }
    if (plan.singlePty) assertSinglePty(markerReceipt, 'marker-output');
    markerFrame = markerReceipt;
    evidence.action({ action: 'marker-receipt', receipt: markerReceipt });
  }

  const attemptMs = Date.now() - triggerAt;
  if (plan.timings || plan.failureDeadlineMs !== undefined || plan.cancel) {
    if (attemptMs > BUDGETS.attemptCeilingMs || Date.now() > deadlineAt) {
      throw new HarnessError('ASSERTION_FAILURE', `attempt ${attemptMs}ms (trigger: ${triggerLabel}) exceeds correctness ceiling ${BUDGETS.attemptCeilingMs}ms`);
    }
    evidence.action({ action: 'attempt-budget', triggerLabel, attemptMs, ceilingMs: BUDGETS.attemptCeilingMs, warmTargetMs: attemptMs <= BUDGETS.warmTargetMs ? 'met' : 'exceeded-reportable' });
  }

  for (const barrier of plan.barriers) barrierHub.release(barrier);

  // Screenshot after the decisive window; persisted before cleanup unlinks roots.
  const screenshotPath = join(ctx.evidenceRunDir, 'screenshot.png');
  if (ctx.platformPreflight === 'win32') {
    // Review blocker 9: owned registered child + bounded deadline; never a
    // raw unregistered unbounded spawn.
    const shot = ctx.spawnOwned('powershell.exe', ['-NoProfile', '-Command',
      "Add-Type -AssemblyName System.Drawing,System.Windows.Forms; $vs=[System.Windows.Forms.SystemInformation]::VirtualScreen; $b=New-Object System.Drawing.Bitmap($vs.Width,$vs.Height); $g=[System.Drawing.Graphics]::FromImage($b); $g.CopyFromScreen($vs.X,$vs.Y,0,0,$b.Size); $b.Save('" + screenshotPath.replace(/'/g, "''") + "'); 'SHOT_OK'"]);
    const outcome = await withDeadline(new Promise(resolvePromise => {
      shot.once('error', () => resolvePromise(-1));
      shot.once('exit', c => resolvePromise(c));
    }), BUDGETS.stagePrepareCreateStatusMs, 'windows-screenshot');
    evidence.action({ action: 'screen-capture', outcome, path: screenshotPath });
    if (outcome.timedOut || outcome.value !== 0 || !existsSync(screenshotPath)) throw new HarnessError('CAPTURE_DENIED', 'Windows screen capture failed or timed out');
  } else {
    await captureScreenshot(evidence, screenshotPath);
  }

  // Root ruling: frameSubmitted (distinct) + INDEPENDENT marker recognition
  // together prove presentation; neither alone can PASS, and no variance
  // heuristic substitutes for recognition.
  let markerRecognition = null;
  if (plan.marker) {
    markerRecognition = await awaitMarkerRecognition(evidence, barrierHub.dir, { runId: ctx.runId, operationId: ctx.operationId }, markerFrame?.markerRegionPx ?? null, screenshotPath);
  }

  return { attemptMs, deadlineAt, triggerLabel, markerRecognition };
}

export async function main(argv) {
  let invocation;
  try {
    invocation = parseInvocation(argv);
  } catch (error) {
    console.error(JSON.stringify({ verdict: 'REJECTED-BEFORE-LAUNCH', code: error.code, message: error.message }));
    return EXIT.invalidInvocation;
  }

  let context;
  try {
    context = preflight(invocation);
  } catch (error) {
    console.error(JSON.stringify({ verdict: 'REJECTED-BEFORE-LAUNCH', code: error.code, message: error.message }));
    return EXIT.invalidInvocation;
  }

  const { EvidenceWriter, ResourceRegistry, BarrierHub, spawnOwned: spawnOwnedFn } = await import('../lib/qa-scenarios/common-harness.mjs');

  // Review blocker 8: exactly ONE evidence writer, registry, barrier hub and
  // nonce pair, created here and shared with every callee as fullContext.
  const runId = `qa-run-${randomUUID()}`;
  const operationId = `qa-op-${randomUUID()}`;
  const evidence = new EvidenceWriter(context.evidenceDir, context.scenario);
  const registry = new ResourceRegistry();
  const barrierHub = new BarrierHub(context.isolationRoot, { runId, operationId });
  registry.registerDirectory(context.isolationRoot);
  const fullContext = {
    ...context,
    evidence, registry, barrierHub, runId, operationId,
    evidenceRunDir: evidence.runDir,
    platformPreflight: context.platform,
    spawnOwned: (command, args, options) => spawnOwnedFn(registry, command, args, options),
  };

  const plan = SCENARIO_PLANS[context.scenario];
  // Pre-arm BEFORE launch (plan: controls arm before trigger).
  for (const barrier of plan.barriers) {
    const targetRole = plan.barrierRoles?.[barrier] ?? BARRIER_ROLES[barrier];
    barrierHub.prearm(barrier, {
      plan: `${context.scenario}${plan.invariants ? ` (${plan.invariants.join(',')})` : ''}`,
      targetRole,
    });
  }
  evidence.action({ action: 'barriers.prearmed', barriers: [...plan.barriers], runId, operationId });

  // Review blocker 6: the evidence pointer binds to the exact runner source
  // bytes via sourceDigest (carried into latest.json by EvidenceWriter.finish).
  const sourceDigest = computeSourceDigest(SOURCE_FILES.map(p => join(runnerRoot, p)));
  const result = {
    schema: 1,
    scenario: context.scenario,
    mode: invocation.headless ? 'headless' : 'native',
    invocation: { argv: [...context.argv, ...argv] },
    host: { platform: context.platform, release: context.release, arch: context.arch },
    binary: { path: context.binary, sha256: context.binarySha256 },
    sourceDigest,
    runId,
    operationId,
    verdict: 'FAIL',
    nativeEvidence: invocation.headless ? 'deferred-to-task-10' : null,
  };
  let exitCode = EXIT.scenarioFailure;

  try {
    if (invocation.headless) {
      const classifierStages = await runHeadlessDiagnosticClassifier(fullContext);
      Object.assign(result, {
        verdict: 'DEFERRED-NATIVE',
        classifierStages,
        barriers: barrierHub.snapshot(),
        deferred: { reason: 'headless diagnostic-classifier completed barrier/cleanup checks only', satisfiesTask10: false },
        nativeEvidence: 'deferred-to-task-10',
      });
      exitCode = EXIT.nativeDeferredHeadless;
    } else {
      const native = await runNativeScenario(fullContext);
      Object.assign(result, {
        verdict: 'PASS',
        nativeEvidence: 'native-receipt',
        attemptMs: native.attemptMs,
        triggerLabel: native.triggerLabel,
        deadlineAt: native.deadlineAt,
        markerRecognition: native.markerRecognition,
        screenshot: join(evidence.runDir, 'screenshot.png'),
        barriers: barrierHub.snapshot(),
        commands: barrierHub.commands,
      });
      exitCode = 0;
    }
  } catch (error) {
    const code = error.code ?? 'ASSERTION_FAILURE';
    result.verdict = ['AX_UNTRUSTED', 'CAPTURE_DENIED', 'NATIVE_AUTOMATION_UNSUPPORTED', 'BARRIER_ACK_TIMEOUT', 'MARKER_RECOGNITION_UNVERIFIED', 'TASK4_IDENTITY_DEPENDENCY'].includes(code) ? 'BLOCKED' : 'FAIL';
    result.error = { code, message: error.message };
    exitCode = code === 'AX_UNTRUSTED' ? EXIT.axUntrusted
      : code === 'CAPTURE_DENIED' ? EXIT.captureDenied
      : code === 'NATIVE_AUTOMATION_UNSUPPORTED' ? EXIT.nativeAutomationUnsupported
      : code === 'BARRIER_ACK_TIMEOUT' ? EXIT.barrierUnsupported
      : code === 'MARKER_RECOGNITION_UNVERIFIED' ? EXIT.markerRecognitionUnverified
      : code === 'TASK4_IDENTITY_DEPENDENCY' ? EXIT.task4IdentityDependency
      : EXIT.scenarioFailure;
  } finally {
    // Evidence persisted BEFORE temporary roots are unlinked; cleanup.json is
    // always emitted, including on deliberate assertion failures.
    const receipts = await registry.cleanup();
    const gate = computeCleanupGate(registry, receipts);
    // Review blocker 10: cleanup failures can never ride along a PASS/exit-0.
    if (!gate.ok) {
      result.verdict = 'FAIL';
      result.cleanupGate = { ...gate, gateFailed: true };
      exitCode = EXIT.scenarioFailure;
    } else {
      result.cleanupGate = { ...gate, gateFailed: false };
    }
    result.barriers = barrierHub.snapshot();
    evidence.write('cleanup.json', {
      registered: {
        processes: registry.processes.map(p => ({ pid: p.pid, label: p.label })),
        sockets: registry.sockets,
        directories: registry.directories,
      },
      reaped: registry.reaped,
      receipts,
      gate,
    });
    evidence.finish(result);
  }

  console.log(JSON.stringify(result, null, 2));
  return exitCode;
}

// Review M6: exact resolved-path comparison; no basename collision and no
// win32 separator mismatch.
export function isEntrypoint(argv1 = process.argv[1], importUrl = import.meta.url) {
  return Boolean(argv1 && fileURLToPath(importUrl) === resolve(argv1));
}

if (isEntrypoint()) {
  process.exitCode = await main(process.argv.slice(2));
}
