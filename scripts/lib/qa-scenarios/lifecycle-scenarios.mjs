#!/usr/bin/env node
// Lifecycle scenario adapters for pane-liveness QA (Task 7):
// - retained-handover: Workload marker -> isolated handover -> preserves incarnation/identity & single reader -> marker again
// - handover-abort: Successor abort/commit rejection -> relinquishment before predecessor resumes -> no dual read -> workload preserved
// - suspension-ownership: External stops untouched vs owned auto-resume -> verified actuation receipt -> same PID resumed
// - stale-binding: Stale/mismatched 5-tuple attach rejected -> valid reattach renders on same backend without new PTY

import { join } from 'node:path';
import {
  BUDGETS,
  HarnessError,
  MonotonicBudget,
} from './common-harness.mjs';
import {
  MARKER_TEXT,
  selectNativeDriver,
  performInspectionHandshake,
} from './native-driver.mjs';

// Invariant assertions for lifecycle, handover, and suspension receipts
export function assertInvariants(names, receipt) {
  const r = receipt ?? {};
  if (names.includes('handoverPreservesIncarnation')) {
    if (!r.originalBackendSessionId || r.adoptedBackendSessionId !== r.originalBackendSessionId) {
      throw new HarnessError('ASSERTION_FAILURE', `retained-handover: incarnation not preserved across transfer: ${JSON.stringify(r)}`);
    }
    if (r.originalIncarnation == null || r.adoptedIncarnation == null) {
      throw new HarnessError('TASK4_IDENTITY_DEPENDENCY', `retained-handover: baseline incarnation is null - Task 4 identity reconciliation dependency unavailable: ${JSON.stringify(r)}`);
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
    if (r.readerCount !== 1) {
      throw new HarnessError('ASSERTION_FAILURE', `single-reader invariant violated (readerCount=${JSON.stringify(r.readerCount)}): ${JSON.stringify(r)}`);
    }
  }
  if (names.includes('noDualRead')) {
    if (r.dualReadObserved === true || (Array.isArray(r.readCounts) && r.readCounts.filter(c => c > 0).length > 1)) {
      throw new HarnessError('ASSERTION_FAILURE', `dual read observed during rollback: ${JSON.stringify(r)}`);
    }
  }
  if (names.includes('externalStopsUntouched')) {
    if (r.externallyStoppedAutoResumed === true) {
      throw new HarnessError('ASSERTION_FAILURE', `external stop was auto-resumed: ${JSON.stringify(r)}`);
    }
    if (r.externallyStoppedProbeState !== 'stopped') {
      throw new HarnessError('ASSERTION_FAILURE', `external stop not observed as stopped: ${JSON.stringify(r)}`);
    }
  }
  if (names.includes('ownedResumeSameProcess')) {
    if (r.ownedResumePid !== r.ownedSuspendPid || r.ownedResumed !== true) {
      throw new HarnessError('ASSERTION_FAILURE', `owned suspension did not resume the same process: ${JSON.stringify(r)}`);
    }
    if (r.verifiedActuationReceipt !== true) {
      throw new HarnessError('ASSERTION_FAILURE', `resume without verified actuation receipt: ${JSON.stringify(r)}`);
    }
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

// ---------------------------------------------------------------------------
// 1. retained-handover scenario adapter
// ---------------------------------------------------------------------------
export async function runRetainedHandoverScenario(ctx, plan, budget = new MonotonicBudget()) {
  const { evidence, barrierHub, pid } = ctx;
  const driver = selectNativeDriver(ctx);

  // Initial typing of agent workload marker
  await driver.focus(evidence, pid);
  await driver.typeMarker(evidence, pid);
  const initialMarkerReceipt = await barrierHub.awaitReceipt('marker-output', 0, budget.consume(BUDGETS.stagePresentationMs, 'initial marker'));
  evidence.action({ action: 'initial-marker', receipt: initialMarkerReceipt });

  // Trigger isolated handover
  barrierHub.command('trigger-handover', { targetEpoch: 'next', isolated: true });
  evidence.action({ action: 'trigger-handover', targetEpoch: 'next' });

  // Await handover transfer receipt
  const transfer = await barrierHub.awaitReceipt('handover-transfer', 0, budget.consume(BUDGETS.attemptCeilingMs, 'handover-transfer'));
  assertInvariants(['handoverPreservesIncarnation', 'singleReader'], transfer);
  evidence.action({ action: 'handover-transfer', receipt: transfer });

  // Type marker again in the adopted session
  if (ctx.platformPreflight === 'win32') await driver.focus(evidence, pid);
  await driver.typeMarker(evidence, pid);

  const postMarkerReceipt = await barrierHub.awaitReceipt('marker-output', 1, budget.consume(BUDGETS.stagePresentationMs, 'post-handover marker'));
  if (!String(postMarkerReceipt?.output ?? '').includes(MARKER_TEXT)) {
    throw new HarnessError('ASSERTION_FAILURE', `fresh visible output not observed after handover: ${JSON.stringify(postMarkerReceipt)}`);
  }
  evidence.action({ action: 'post-handover-marker', receipt: postMarkerReceipt });

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
    transferReceipt: transfer,
    postMarkerReceipt,
    markerRecognition,
    screenshotMetadata,
  };
}

// ---------------------------------------------------------------------------
// 2. handover-abort scenario adapter
// ---------------------------------------------------------------------------
export async function runHandoverAbortScenario(ctx, plan, budget = new MonotonicBudget()) {
  const { evidence, barrierHub, pid } = ctx;
  const driver = selectNativeDriver(ctx);

  // Trigger abort variant (e.g. commit rejection, lost abort reply, or successor exit)
  const variant = plan.abortVariant ?? 'commit-rejection';
  barrierHub.command('trigger-handover-abort', { variant });
  evidence.action({ action: 'trigger-handover-abort', variant });

  // Await rollback relinquishment receipt
  const rollback = await barrierHub.awaitReceipt('rollback-relinquishment', 0, budget.consume(BUDGETS.attemptCeilingMs, 'rollback-relinquishment'));
  assertInvariants(['relinquishmentBeforeResume', 'singleReader', 'noDualRead'], rollback);
  evidence.action({ action: 'rollback-relinquishment', receipt: rollback });

  // Type marker to verify confirmed resolution restores same workload
  await driver.focus(evidence, pid);
  await driver.typeMarker(evidence, pid);

  const markerReceipt = await barrierHub.awaitReceipt('marker-output', 0, budget.consume(BUDGETS.stagePresentationMs, 'restored workload marker'));
  if (!String(markerReceipt?.output ?? '').includes(MARKER_TEXT)) {
    throw new HarnessError('ASSERTION_FAILURE', `marker not observed in restored workload after abort rollback: ${JSON.stringify(markerReceipt)}`);
  }
  evidence.action({ action: 'restored-marker', receipt: markerReceipt });

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
    rollbackReceipt: rollback,
    markerReceipt,
    markerRecognition,
    screenshotMetadata,
  };
}

// ---------------------------------------------------------------------------
// 3. suspension-ownership scenario adapter
// ---------------------------------------------------------------------------
export async function runSuspensionOwnershipScenario(ctx, plan, budget = new MonotonicBudget()) {
  const { evidence, barrierHub, pid } = ctx;
  const driver = selectNativeDriver(ctx);

  // Trigger suspension check
  barrierHub.command('trigger-suspension-check', { testExternalStop: true });
  evidence.action({ action: 'trigger-suspension-check' });

  // Await suspension receipt
  const receipt = await barrierHub.awaitReceipt('suspension-receipt', 0, budget.consume(BUDGETS.attemptCeilingMs, 'suspension-receipt'));
  assertInvariants(['externalStopsUntouched', 'ownedResumeSameProcess'], receipt);
  evidence.action({ action: 'suspension-receipt', receipt });

  // Type marker to confirm resumed process is responsive
  await driver.focus(evidence, pid);
  await driver.typeMarker(evidence, pid);

  const markerReceipt = await barrierHub.awaitReceipt('marker-output', 0, budget.consume(BUDGETS.stagePresentationMs, 'resumed marker'));
  if (!String(markerReceipt?.output ?? '').includes(MARKER_TEXT)) {
    throw new HarnessError('ASSERTION_FAILURE', `marker not observed in resumed process: ${JSON.stringify(markerReceipt)}`);
  }
  evidence.action({ action: 'resumed-marker', receipt: markerReceipt });

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
    suspensionReceipt: receipt,
    markerReceipt,
    markerRecognition,
    screenshotMetadata,
  };
}

// ---------------------------------------------------------------------------
// 4. stale-binding scenario adapter
// ---------------------------------------------------------------------------
export async function runStaleBindingScenario(ctx, plan, budget = new MonotonicBudget()) {
  const { evidence, barrierHub, pid } = ctx;
  const driver = selectNativeDriver(ctx);

  // A stale attempt has to be offered against a LIVE seven-field binding, and the
  // only session that has one is the pane this runner created and bound. Measured
  // with the pane step absent: the product reported
  // `FERRYX_QA_STALE_BINDING_UNSERVICED: no daemon session with a live
  // seven-field binding yet; the command stays pending` and the
  // `stale-receipt-rejected` receipt could never settle, because the registration
  // fence that writes it had nothing to reject. The binding itself comes from the
  // pane's own attach tuple, so no bind file is written here - this only refuses to
  // proceed without one.
  const paneTarget = ctx.paneBinding?.backendSessionId;
  if (typeof paneTarget !== 'string' || paneTarget.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', 'stale-binding: the pane step settled no bound session, so there is no live binding to offer a stale attempt against');
  }
  evidence.action({ action: 'stale-binding-live-binding', backendSessionId: paneTarget });

  // Trigger stale attach where delayed receipt changes identity fields.
  //
  // `attemptGeneration` cannot be used here: a pane's FIRST binding legitimately
  // has generation 0, and the product's own mutation is
  // `active.attempt_generation.checked_sub(1)?`
  // (`native_terminal/surface_host.rs::mutate_attach_tuple_field`), which cannot go
  // below zero. Measured: `FERRYX_QA_STALE_BINDING_UNSATISFIABLE: the live binding
  // has attemptGeneration 0 and cannot be made strictly older`. `paneIdentity` is
  // the same class of field - the product lists it in
  // `STALE_BINDING_MUTABLE_FIELDS` as one the registration fence really compares -
  // and it is always satisfiable, so the stale attempt is offered and the fence's
  // own rejection is what settles the receipt.
  const mutateField = 'paneIdentity';
  barrierHub.command('trigger-stale-binding', { mutateField });
  evidence.action({ action: 'trigger-stale-binding', mutateField });

  // Await stale receipt rejected
  const rejectedReceipt = await barrierHub.awaitReceipt('stale-receipt-rejected', 0, budget.consume(BUDGETS.attemptCeilingMs, 'stale-receipt-rejected'));
  assertInvariants(['staleReceiptRejected'], rejectedReceipt);
  evidence.action({ action: 'stale-receipt-rejected', receipt: rejectedReceipt });

  // Await reattach-marker or trigger legitimate reattach
  const reattachReceipt = await barrierHub.awaitReceipt('reattach-marker', 0, budget.consume(BUDGETS.attemptCeilingMs, 'reattach-marker'));
  assertInvariants(['reattachSameBackend'], reattachReceipt);
  evidence.action({ action: 'reattach-marker', receipt: reattachReceipt });

  // Type marker to confirm valid reattach renders on same backend
  await driver.focus(evidence, pid);
  await driver.typeMarker(evidence, pid);

  const markerReceipt = await barrierHub.awaitReceipt('marker-output', 0, budget.consume(BUDGETS.stagePresentationMs, 'reattach marker'));
  if (!String(markerReceipt?.output ?? '').includes(MARKER_TEXT)) {
    throw new HarnessError('ASSERTION_FAILURE', `marker not observed on legitimate reattach: ${JSON.stringify(markerReceipt)}`);
  }
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
    rejectedReceipt,
    reattachReceipt,
    markerReceipt,
    markerRecognition,
    screenshotMetadata,
  };
}
