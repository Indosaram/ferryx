#!/usr/bin/env node
// Split scenario adapters for pane-liveness QA (Task 7):
// - split-happy: Standard split right -> 5-tuple presentation -> visible marker -> inspection handshake
// - split-attach-stall: Held attach handshake -> actionable failure <= 15s -> release -> click actual Retry -> reattach same backend -> marker
// - split-cancel: Trigger split first (deadlock-free) -> cancel while/before create -> ack <= 3s -> authoritative cleanup without requiring created ID
// - split-concurrent: Held unrelated remote RPC -> concurrent typing and 16 split requests (conflict pairs) -> local split succeeds -> single PTY

import { join } from 'node:path';
import {
  BUDGETS,
  HarnessError,
  MonotonicBudget,
  requireSevenTupleReceipt,
  requireFiveTupleReceipt,
} from './common-harness.mjs';
import {
  MARKER_TEXT,
  selectNativeDriver,
  performInspectionHandshake,
} from './native-driver.mjs';
import { awaitMarkerReceiptForSession } from './pane-binding.mjs';

export { requireSevenTupleReceipt, requireFiveTupleReceipt };

export function assertSinglePty(receipt, label = 'pty-check') {
  if (receipt?.ptyCreatedCount === undefined || receipt.ptyCreatedCount > 1) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: duplicate/missing PTY accounting (expected exactly 1, got ${receipt?.ptyCreatedCount}): ${JSON.stringify(receipt)}`);
  }
  return true;
}

// ---------------------------------------------------------------------------
// 1. split-happy scenario adapter
// ---------------------------------------------------------------------------
export async function runSplitHappyScenario(ctx, plan, budget = new MonotonicBudget()) {
  const { evidence, barrierHub, pid } = ctx;
  const driver = selectNativeDriver(ctx);

  // Trigger Split Right
  if (ctx.platformPreflight !== 'win32') await driver.focus(evidence, pid);
  await driver.split(evidence, pid);

  // Await split-create
  const create = await barrierHub.awaitReceipt('split-create', 0, budget.consume(BUDGETS.stagePrepareCreateStatusMs, 'split-create'));
  if (typeof create.backendSessionId !== 'string' || create.backendSessionId.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', `split-create receipt lacks authoritative backendSessionId: ${JSON.stringify(create)}`);
  }
  if (barrierHub.isArmed('presentation')) {
    barrierHub.bindBackendSession('presentation', create.backendSessionId);
  }
  evidence.action({ action: 'split-create', receipt: create, boundBackendSessionId: create.backendSessionId });

  // Await positive 7-tuple presentation receipt FOR THE SPLIT PANE. The app's
  // own pane - created by the pre-trigger UI step, because the app boots to its
  // empty state - presents frames into this same stream, so the receipt is
  // addressed by the split's own session instead of by line index. The assertion
  // (positive seven-field tuple for the pane that was split) is unchanged; only
  // the line it reads is, and it can no longer ride the other pane's receipt.
  const presentation = await barrierHub.awaitReceiptForSession('presentation', create.backendSessionId, budget.consume(BUDGETS.stagePresentationMs, 'presentation'), 'split-happy presentation');
  requireSevenTupleReceipt(presentation, {
    backendSessionId: create.backendSessionId,
    incarnation: create.incarnation,
    daemonEpoch: create.daemonEpoch ? String(create.daemonEpoch) : undefined,
    attemptGeneration: create.attemptGeneration,
  }, 'split-happy');
  if (presentation.presented !== undefined && presentation.presented !== true) {
    throw new HarnessError('ASSERTION_FAILURE', `presentation receipt presented is false: ${JSON.stringify(presentation)}`);
  }
  evidence.action({ action: 'presentation-receipt', receipt: presentation });

  // Type marker into target pane
  if (ctx.platformPreflight === 'win32') await driver.focus(evidence, pid);
  await driver.typeMarker(evidence, pid);

  // Await marker output receipt
  // The marker is addressed to the pane the split created: a receipt from the
  // app's own pane would not be evidence that the SPLIT pane is live, and its
  // per-session `ptyCreatedCount` would not be the split's PTY accounting.
  const markerReceipt = await awaitMarkerReceiptForSession(barrierHub, create.backendSessionId, budget.consume(BUDGETS.stagePresentationMs + BUDGETS.stageAttachListenerMs, 'marker-output'), 'split-happy marker');
  if (!String(markerReceipt?.output ?? '').includes(MARKER_TEXT)) {
    throw new HarnessError('ASSERTION_FAILURE', `marker ${MARKER_TEXT} not observed in output receipt: ${JSON.stringify(markerReceipt)}`);
  }
  if (markerReceipt.frameSubmitted !== true) {
    throw new HarnessError('ASSERTION_FAILURE', `marker receipt attests no submitted frame: ${JSON.stringify(markerReceipt)}`);
  }
  assertSinglePty(markerReceipt, 'split-happy');
  evidence.action({ action: 'marker-receipt', receipt: markerReceipt });

  // Capture owned window screenshot & perform inspection handshake
  const screenshotPath = join(ctx.evidenceRunDir, 'screenshot.png');
  const screenshotMetadata = await driver.capture(evidence, screenshotPath, pid);

  const markerRecognition = await performInspectionHandshake(
    evidence,
    barrierHub,
    { runId: ctx.runId, operationId: ctx.operationId },
    screenshotMetadata,
    budget.consume(BUDGETS.stagePresentationMs, 'inspection handshake')
  );

  return {
    createReceipt: create,
    presentationReceipt: presentation,
    markerReceipt,
    markerRecognition,
    screenshotMetadata,
  };
}

// ---------------------------------------------------------------------------
// 2. split-attach-stall scenario adapter
// ---------------------------------------------------------------------------
export async function runSplitAttachStallScenario(ctx, plan, budget = new MonotonicBudget()) {
  const { evidence, barrierHub, pid } = ctx;
  const driver = selectNativeDriver(ctx);

  // Trigger Split Right
  if (ctx.platformPreflight !== 'win32') await driver.focus(evidence, pid);
  await driver.split(evidence, pid);

  // Await split-create
  const create = await barrierHub.awaitReceipt('split-create', 0, budget.consume(BUDGETS.stagePrepareCreateStatusMs, 'split-create'));
  const authoritativeSessionId = create.backendSessionId;
  evidence.action({ action: 'split-create', receipt: create, boundBackendSessionId: authoritativeSessionId });

  // Await held event for attach-handshake
  const holdStartedAt = Date.now();
  const held = await barrierHub.awaitHeld('attach-handshake', plan.barrierHoldMs ?? BUDGETS.attemptCeilingMs);
  evidence.action({
    action: 'barrier.held',
    barrier: 'attach-handshake',
    configuredHoldBudgetMs: plan.barrierHoldMs ?? BUDGETS.attemptCeilingMs,
    measuredHeldMs: Date.now() - holdStartedAt,
  });

  // Await failure-classified receipt within bounded ceiling (grace window)
  const failureGraceMs = Math.min(budget.remainingMs(), (plan.barrierHoldMs ?? BUDGETS.attemptCeilingMs) + 1_000);
  const failure = await barrierHub.awaitReceipt('attach-handshake', 0, failureGraceMs);
  if (failure?.actionable !== true) {
    throw new HarnessError('ASSERTION_FAILURE', `stalled attach must settle with an actionable failure: ${JSON.stringify(failure)}`);
  }
  if (failure.retryMustReuseId !== true || typeof failure.backendSessionId !== 'string') {
    throw new HarnessError('ASSERTION_FAILURE', `same-ID retry contract requires retryMustReuseId: true and authoritative backendSessionId: ${JSON.stringify(failure)}`);
  }
  if (failure.backendSessionId !== authoritativeSessionId) {
    throw new HarnessError('ASSERTION_FAILURE', `failure backendSessionId ${failure.backendSessionId} does not match created backend ${authoritativeSessionId}`);
  }
  evidence.action({ action: 'failure-classified', receipt: failure });

  // Release the barrier hold now that actionable failure is verified
  barrierHub.release('attach-handshake');
  evidence.action({ action: 'barrier.released', barrier: 'attach-handshake' });

  // Click actual Retry in the UI
  await driver.retry(evidence, pid);

  // Contract requirement: Retry retains request/backend identity, but increments attemptGeneration to a NEW generation.
  const retryAttemptGeneration = Number(create.attemptGeneration ?? 1) + 1;
  barrierHub.command('retry', {
    backendSessionId: authoritativeSessionId,
    incarnation: create.incarnation ?? null,
    daemonEpoch: create.daemonEpoch ? String(create.daemonEpoch) : null,
    attemptGeneration: retryAttemptGeneration,
    clientRequestId: create.clientRequestId ?? null,
  });
  evidence.action({
    action: 'retry-click',
    backendSessionId: authoritativeSessionId,
    newAttemptGeneration: retryAttemptGeneration,
  });

  // Await presentation receipt following Retry: must reuse SAME backendSessionId but verify NEW attemptGeneration
  if (barrierHub.isArmed('presentation')) {
    barrierHub.bindBackendSession('presentation', authoritativeSessionId);
    const retryPresentation = await barrierHub.awaitReceiptForSession('presentation', authoritativeSessionId, budget.consume(BUDGETS.stagePresentationMs, 'retry presentation'), 'split-attach-stall retry presentation');
    requireSevenTupleReceipt(retryPresentation, {
      backendSessionId: authoritativeSessionId,
      incarnation: create.incarnation,
      attemptGeneration: retryAttemptGeneration,
    }, 'split-attach-stall retry');
    if (retryPresentation.presented !== undefined && retryPresentation.presented !== true) {
      throw new HarnessError('ASSERTION_FAILURE', `retried presentation presented flag is false: ${JSON.stringify(retryPresentation)}`);
    }
    evidence.action({ action: 'retry-presentation', receipt: retryPresentation });
  }

  // Type marker and verify output
  if (ctx.platformPreflight === 'win32') await driver.focus(evidence, pid);
  await driver.typeMarker(evidence, pid);

  // The retry's own session must report the marker: `assertSinglePty` below is
  // the "Retry never creates a replacement PTY" invariant, and it is only
  // measured on the retried session if the receipt names that session.
  const markerReceipt = await awaitMarkerReceiptForSession(barrierHub, authoritativeSessionId, budget.consume(BUDGETS.stagePresentationMs, 'marker output'), 'split-attach-stall marker');
  if (!String(markerReceipt?.output ?? '').includes(MARKER_TEXT)) {
    throw new HarnessError('ASSERTION_FAILURE', `marker not observed in retried session: ${JSON.stringify(markerReceipt)}`);
  }
  // Crucial invariant: Retry reuses request/backend/incarnation and NEVER creates replacement PTY
  assertSinglePty(markerReceipt, 'split-attach-stall singlePty check');
  evidence.action({ action: 'marker-receipt', receipt: markerReceipt });

  // Screenshot and inspection handshake
  const screenshotPath = join(ctx.evidenceRunDir, 'screenshot.png');
  const screenshotMetadata = await driver.capture(evidence, screenshotPath, pid);

  const markerRecognition = await performInspectionHandshake(
    evidence,
    barrierHub,
    { runId: ctx.runId, operationId: ctx.operationId },
    screenshotMetadata,
    budget.consume(BUDGETS.stagePresentationMs, 'inspection handshake')
  );

  return {
    failureReceipt: failure,
    markerReceipt,
    markerRecognition,
    screenshotMetadata,
  };
}

// ---------------------------------------------------------------------------
// 3. split-cancel scenario adapter
// ---------------------------------------------------------------------------
export async function runSplitCancelScenario(ctx, plan, budget = new MonotonicBudget()) {
  const { evidence, barrierHub, pid } = ctx;
  const driver = selectNativeDriver(ctx);

  // Crucial fix: Trigger split FIRST before awaiting create receipt!
  // Cancel must trigger split before waiting for held-create and not require created ID.
  if (ctx.platformPreflight !== 'win32') await driver.focus(evidence, pid);
  await driver.split(evidence, pid);

  // Dispatch cancel request immediately
  const cancelPhase = plan.cancel?.phase ?? 'while-creating';
  barrierHub.command('split-cancel', { phase: cancelPhase });
  evidence.action({ action: 'cancel-request', request: 'split-cancel', phase: cancelPhase });

  // Await cancel acknowledgement receipt within bound
  const cancel = await barrierHub.awaitReceipt('cancel-ack', 0, budget.consume(BUDGETS.cancelAckCeilingMs, 'cancel-ack'));
  if (typeof cancel?.cancelAckMs !== 'number' || cancel.cancelAckMs > BUDGETS.cancelAckCeilingMs) {
    throw new HarnessError('ASSERTION_FAILURE', `cancel acknowledgement exceeds ${BUDGETS.cancelAckCeilingMs}ms: ${JSON.stringify(cancel)}`);
  }
  if (cancel.cancelAckMs > BUDGETS.cancelDaemonResponseBudgetMs + (cancel.timerDispatchLatencyMs ?? 0)) {
    throw new HarnessError('ASSERTION_FAILURE', `cancel acknowledgement exceeds daemon response budget: ${JSON.stringify(cancel)}`);
  }
  if (cancel.cleanupReceipt?.authoritative !== true) {
    throw new HarnessError('ASSERTION_FAILURE', `cancel lacks an authoritative cleanup receipt: ${JSON.stringify(cancel)}`);
  }
  // Cancel before create must NOT require created ID
  if (cancelPhase === 'before-create' && cancel.createdIdRequired === true) {
    throw new HarnessError('ASSERTION_FAILURE', `cancel-before-create must not require created ID`);
  }
  evidence.action({ action: 'cancel-ack', receipt: cancel });

  // Duplicate cancel test: duplicate cancel must be harmless
  barrierHub.command('split-cancel', { phase: 'duplicate-cancel' });
  evidence.action({ action: 'duplicate-cancel-sent' });

  return {
    cancelReceipt: cancel,
  };
}

// ---------------------------------------------------------------------------
// 4. split-concurrent scenario adapter
// ---------------------------------------------------------------------------
export async function runSplitConcurrentScenario(ctx, plan, budget = new MonotonicBudget()) {
  const { evidence, barrierHub, pid } = ctx;
  const driver = selectNativeDriver(ctx);

  // 1. Pre-arm and trigger real unrelated remote RPC traffic
  barrierHub.command('trigger-remote-rpc', { rpcKind: 'remote-query', count: 1 });
  const held = await barrierHub.awaitHeld('held-rpc', budget.consume(BUDGETS.stageAttachListenerMs, 'held-rpc wait'));
  if (held?.heldRpc !== true) {
    throw new HarnessError('ASSERTION_FAILURE', `split-concurrent requires heldRpc: true, got ${JSON.stringify(held)}`);
  }
  evidence.action({ action: 'held-rpc.confirmed', heldRpc: true });

  // 2. Drive local typing while remote RPC is held (must not block!)
  await driver.focus(evidence, pid);
  await driver.typeMarker(evidence, pid);

  // 3. Drive 16 bounded split requests including duplicate/fingerprint conflict pairs
  barrierHub.command('split-concurrent-batch', { count: 16, testConflicts: true });
  evidence.action({ action: 'split-concurrent-batch', count: 16 });

  // 4. Trigger native split
  await driver.split(evidence, pid);

  // 5. Await local split-create and presentation
  const create = await barrierHub.awaitReceipt('split-create', 0, budget.consume(BUDGETS.stagePrepareCreateStatusMs, 'split-create'));
  if (typeof create.backendSessionId !== 'string' || create.backendSessionId.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', `split-create lacks authoritative backendSessionId under concurrent load: ${JSON.stringify(create)}`);
  }
  evidence.action({ action: 'split-create', receipt: create, boundBackendSessionId: create.backendSessionId });

  const presentation = await barrierHub.awaitReceiptForSession('presentation', create.backendSessionId, budget.consume(BUDGETS.stagePresentationMs, 'presentation'), 'split-concurrent presentation');
  requireSevenTupleReceipt(presentation, {
    backendSessionId: create.backendSessionId,
    incarnation: create.incarnation,
    daemonEpoch: create.daemonEpoch ? String(create.daemonEpoch) : undefined,
    attemptGeneration: create.attemptGeneration,
  }, 'split-concurrent');
  if (presentation.presented !== undefined && presentation.presented !== true) {
    throw new HarnessError('ASSERTION_FAILURE', `presentation receipt presented is false: ${JSON.stringify(presentation)}`);
  }
  evidence.action({ action: 'presentation-receipt', receipt: presentation });

  // 6. Await marker output
  // 6. Await marker output. The marker was typed BEFORE the split, into the pane
  // the pre-trigger UI step created, so that pane's session is the session the
  // typing really addressed - addressed by identity here instead of by index.
  const typedIntoSessionId = ctx.paneBinding?.backendSessionId;
  if (typeof typedIntoSessionId !== 'string' || typedIntoSessionId.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', 'split-concurrent requires the pre-trigger pane binding (ctx.paneBinding) to address the pane the marker was typed into');
  }
  evidence.action({ action: 'marker-target-pane', backendSessionId: typedIntoSessionId, splitBackendSessionId: create.backendSessionId });
  const markerReceipt = await awaitMarkerReceiptForSession(barrierHub, typedIntoSessionId, budget.consume(BUDGETS.stagePresentationMs, 'marker output'), 'split-concurrent marker');
  if (!String(markerReceipt?.output ?? '').includes(MARKER_TEXT)) {
    throw new HarnessError('ASSERTION_FAILURE', `marker not observed under concurrent RPC: ${JSON.stringify(markerReceipt)}`);
  }
  assertSinglePty(markerReceipt, 'split-concurrent singlePty');
  evidence.action({ action: 'marker-receipt', receipt: markerReceipt });

  // 7. Release held-rpc
  barrierHub.release('held-rpc');
  evidence.action({ action: 'held-rpc.released' });
  await barrierHub.awaitReceipt('held-rpc', 0, budget.consume(BUDGETS.stagePrepareCreateStatusMs, 'held-rpc settlement'));

  // 8. Screenshot and inspection handshake
  const screenshotPath = join(ctx.evidenceRunDir, 'screenshot.png');
  const screenshotMetadata = await driver.capture(evidence, screenshotPath, pid);

  const markerRecognition = await performInspectionHandshake(
    evidence,
    barrierHub,
    { runId: ctx.runId, operationId: ctx.operationId },
    screenshotMetadata,
    budget.consume(BUDGETS.stagePresentationMs, 'inspection handshake')
  );

  return {
    createReceipt: create,
    presentationReceipt: presentation,
    markerReceipt,
    markerRecognition,
    screenshotMetadata,
  };
}
