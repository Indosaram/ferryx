#!/usr/bin/env node
// Split scenario adapters for pane-liveness QA (Task 7):
// - split-happy: Standard split right -> 5-tuple presentation -> visible marker -> inspection handshake
// - split-attach-stall: Held attach handshake -> actionable failure <= 15s -> release -> click actual Retry -> reattach same backend -> marker
// - split-cancel: Trigger split first (deadlock-free) -> cancel while/before create -> ack <= 3s -> authoritative cleanup without requiring created ID
// - split-concurrent: Held unrelated remote RPC -> concurrent typing and 16 split requests (conflict pairs) -> local split succeeds -> single PTY

import { join } from 'node:path';
import {
  APP_STDIO_BYTES_FAILED,
  BUDGETS,
  HarnessError,
  MonotonicBudget,
  SPLIT_INVENTORY_READ_FAILED,
  appStdioBytes,
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

// The `split-concurrent` conflict wave's reporting contract. The batch drives
// duplicate/fingerprint-conflict pairs against the real daemon, and the daemon's
// own pre-check (`daemon/session_service.rs::create_split`, typed
// `SPAWN_REQUEST_CONFLICT`) rejects a reused request identity carrying different
// parameters. A scenario PASS is bound to that rejection really being reported by
// the batch settlement, never to the batch merely having been requested.
export function assertConflictWaveReported(batch, label = 'split-concurrent-batch') {
  const rejected = batch?.conflictRejected;
  if (typeof rejected !== 'number' || rejected < 1) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: the conflicting-fingerprint request was not rejected (conflictRejected=${JSON.stringify(rejected)}): ${JSON.stringify(batch)}`);
  }
  const codes = Array.isArray(batch?.conflictRejectionCodes) ? batch.conflictRejectionCodes : [];
  if (!codes.includes('SpawnRequestConflict')) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: rejection codes lack SPAWN_REQUEST_CONFLICT (${JSON.stringify(codes)}): ${JSON.stringify(batch)}`);
  }
  return true;
}

// ---------------------------------------------------------------------------
// 0. Post-click observability the split scenarios share (task-9 pass-22)
//
// PASS 21 measured the split affordance click RESOLVING and the scenario then
// stopping one stage later (`BLOCKED` / `BARRIER_ACK_TIMEOUT`: "product did not
// settle barrier split-create receipt[0] within 9000ms"). The lead established by
// reading source that the split flow is INVISIBLE in the app's stderr - the
// `split-create` receipt is emitted only on `cmd_terminal_spawn`'s create-only
// early path, which returns BEFORE that command's only log line - and that
// `splitPane` / `handleSplitActive` have silent guard returns. So "clicked, no
// spawn, no receipt" had two readings the old harness could not tell apart:
//   (a) the split flow BAILED before create (a silent guard), or
//   (b) the split happened and only the receipt is the gap.
// A local split create creates a DAEMON SESSION, so the inventory delta measured
// across the click separates them: 2 -> 3 sessions means the create really
// happened, an unchanged count means it did not. The app-stdio byte delta taken
// at the same instant is the deliberate self-check on the invisibility claim: if
// the bytes GROW across the click, that claim is falsified and this evidence says
// so rather than hiding it.
//
// Both measurements ride machinery that already exists: the pane step's own
// read-only inventory reader (`createPaneInventoryReader`, whose delta arithmetic
// is `computeInventoryDelta`) and the always-on app-stdio sink. Neither is an
// assertion: the split scenarios' verdicts are unchanged, and a measurement that
// cannot be taken is recorded as a typed reason, never as a failure.
//
// That contract is ENFORCED here, not asserted (pass-22 audit D1/D2/D4):
//   * every read and every sampler is wrapped, so NO throw can escape into
//     `classifyNativeFailure` - a read that throws is a typed action
//     (`SPLIT_INVENTORY_READ_FAILED`) carrying the thrown error's own code and
//     message, and the scenario continues exactly as before;
//   * both reads sit INSIDE the measured attempt window (the pre-click read is
//     taken after the trigger budget exists, and the post-click read immediately
//     after the click), so each is CHARGED to that budget at the small bounded
//     cap `BUDGETS.splitInventoryReadCapMs` and reports the read's own
//     `elapsedMs` - a window that cannot pay for a read gets a typed refusal
//     (`INVENTORY_READ_REFUSED`) instead of a read that spends the ceiling
//     silently. No `BUDGETS` value is changed by any of this.
export const SPLIT_INVENTORY_ACTION = 'split-inventory-after';
export const PRE_SPLIT_INVENTORY_ACTION = 'pane-inventory-pre-split';
export const PANE_INVENTORY_AFTER_ACTION = 'pane-inventory-after';
export const INVENTORY_READER_MISSING = 'INVENTORY_READER_MISSING';
// `armSplitInventory` asks the reader for the reading it took last - the baseline
// the post-click delta is measured against. That is a reader METHOD like
// `snapshot`, so its failure is typed the same way instead of escaping into
// `classifyNativeFailure` (pass-22 audit R1).
export const SPLIT_INVENTORY_LAST_READING_ACTION = 'split-inventory-last-reading';
// The measurement's typed failure identities live in the shared harness module
// (the reader emits them too); re-exported here so a caller of these adapters
// needs one import.
export { APP_STDIO_BYTES_FAILED, INVENTORY_READ_REFUSED, SPLIT_INVENTORY_READ_FAILED } from './common-harness.mjs';

// The app-stdio byte self-check (pass-22): the sink's cheap in-memory counters at
// ONE moment plus the delta across the click. A sink whose counters cannot be read
// is a measurement that could not be taken: the counters are reported absent,
// typed, and the scenario is untouched (audit D4).
function appStdioByteSelfCheck(ctx, bytesBefore) {
  try {
    const after = appStdioBytes(ctx?.appStdio);
    return {
      appStdioBytes: {
        before: bytesBefore,
        after,
        delta: bytesBefore && after
          ? { stdout: after.stdout - bytesBefore.stdout, stderr: after.stderr - bytesBefore.stderr, total: after.total - bytesBefore.total }
          : null,
      },
    };
  } catch (error) {
    return {
      appStdioBytes: { before: bytesBefore, after: null, delta: null },
      appStdioBytesError: { code: APP_STDIO_BYTES_FAILED, message: String(error?.message ?? error) },
    };
  }
}

// ONE read, charged and wrapped (pass-22 audit D1/D2/D4). The charge is what makes
// the cost explicit: the read is inside the MEASURED attempt window, so it is
// bounded by what that window has left, capped at `splitInventoryReadCapMs`, and
// the recorded action carries the cap it was given plus the read's own
// `elapsedMs`. The catch is the guarantee that no reader implementation - present
// or future - can turn this measurement into the run's verdict: `snapshot` is
// documented not to throw, and if it does anyway the action says so, typed, and
// the scenario carries on with nothing measured.
async function readSplitInventory(ctx, reader, label, budget, { compareToPrevious = false, extra = null } = {}) {
  const instrumentation = ctx?.instrumentation ?? null;
  const startedAt = Date.now();
  try {
    return await reader.snapshot(label, {
      compareToPrevious,
      extra,
      charge: { budget, capMs: BUDGETS.splitInventoryReadCapMs },
    });
  } catch (error) {
    // The sampler beside the read is a measurement too: a sampler that throws here
    // must not turn the typed action into a second throw (pass-22 audit R2).
    let sampled = {};
    try {
      sampled = typeof extra === 'function' ? (extra() ?? {}) : {};
    } catch (samplerError) {
      sampled = { extraError: { code: APP_STDIO_BYTES_FAILED, message: String(samplerError?.message ?? samplerError) } };
    }
    ctx?.evidence?.action?.({
      action: label,
      ...sampled,
      ok: false,
      code: SPLIT_INVENTORY_READ_FAILED,
      cause: error?.code ?? null,
      detail: `the inventory read threw instead of reporting a result: ${error?.message ?? error}`,
      sessionCount: null,
      sessionIds: null,
      delta: null,
      baselineLabel: null,
      elapsedMs: null,
    });
    return null;
  } finally {
    // The wall clock THIS read spent inside the measured window is instrumentation,
    // not the scenario's own cost, so it is handed back to the window it was charged
    // to (pass-22 audit F2-11 residual). Bounded by the charge above: each read is
    // capped at `splitInventoryReadCapMs`, so the total handed back is bounded too.
    instrumentation?.note?.(label, Date.now() - startedAt);
  }
}

// The reader's remembered baseline, read through the same guard as `snapshot`
// (pass-22 audit R1): `lastReading` is a reader method, so a reader whose
// implementation throws from it is recorded as a typed action and the arm
// continues with no baseline - exactly the state a reader that never took a
// usable reading produces, and never a throw into `classifyNativeFailure`.
function readLastReading(ctx, reader) {
  if (typeof reader?.lastReading !== 'function') return null;
  try {
    return reader.lastReading();
  } catch (error) {
    ctx?.evidence?.action?.({
      action: SPLIT_INVENTORY_LAST_READING_ACTION,
      ok: false,
      code: SPLIT_INVENTORY_READ_FAILED,
      cause: error?.code ?? null,
      detail: `the inventory reader's lastReading() threw instead of reporting a reading: ${error?.message ?? error}`,
      sessionCount: null,
      sessionIds: null,
      delta: null,
      baselineLabel: null,
      elapsedMs: null,
    });
    return null;
  }
}

// Called immediately BEFORE the split click: remembers the app-stdio byte
// counters and makes sure the reader holds a PRE-SPLIT reading for the delta to be
// measured against. The pane step's own after-read (`pane-inventory-after`) is
// that reading whenever it was taken AND landed; when the pane binding settled on
// the product's presentation receipt instead - or its after-read failed - no
// usable baseline exists, so one is taken here, before the click, rather than
// letting the delta silently fold the pane step's own session into the split's.
//
// `budget` is the MEASURED attempt budget this pair's reads are charged to, and it
// travels with `armed` so the read after the click is charged by the same window
// the read before it was (see `readSplitInventory`).
export async function armSplitInventory(ctx, budget = null) {
  const reader = typeof ctx?.paneInventory?.snapshot === 'function' ? ctx.paneInventory : null;
  let appStdioBytesBefore = null;
  let appStdioBytesBeforeError = null;
  try {
    appStdioBytesBefore = appStdioBytes(ctx?.appStdio);
  } catch (error) {
    appStdioBytesBeforeError = { code: APP_STDIO_BYTES_FAILED, message: String(error?.message ?? error) };
  }
  const armed = { reader, budget, baselineLabel: null, appStdioBytesBefore, appStdioBytesBeforeError };
  if (reader === null) return armed;
  const previous = readLastReading(ctx, reader);
  if (previous?.label === PANE_INVENTORY_AFTER_ACTION && previous.result?.ok === true) {
    armed.baselineLabel = previous.label;
    return armed;
  }
  const baseline = await readSplitInventory(ctx, reader, PRE_SPLIT_INVENTORY_ACTION, budget);
  armed.baselineLabel = baseline?.ok === true ? PRE_SPLIT_INVENTORY_ACTION : null;
  return armed;
}

// Called immediately AFTER the split click returns: the post-click daemon
// inventory (`split-inventory-after`) carrying its delta against the pre-split
// reading, plus the app-stdio byte self-check sampled at the same moment. Returns
// the read result, or null when this run had no inventory reader wired (recorded
// as a typed reason on the action, never thrown: this is a measurement, and a
// missing measurement must not be able to change a verdict).
export async function recordSplitInventory(ctx, armed, label = SPLIT_INVENTORY_ACTION) {
  const reader = armed?.reader ?? null;
  const bytesBefore = armed?.appStdioBytesBefore ?? null;
  const byteSelfCheck = () => {
    const sampled = appStdioByteSelfCheck(ctx, bytesBefore);
    // The ARM-time sample can fail too (the counters are read once before the
    // click); when it did, the recorded action says so instead of leaving a null
    // delta unexplained.
    return armed?.appStdioBytesBeforeError === null || armed?.appStdioBytesBeforeError === undefined
      ? sampled
      : { ...sampled, appStdioBytesBeforeError: armed.appStdioBytesBeforeError };
  };
  if (reader === null) {
    ctx?.evidence?.action?.({
      action: label,
      ...byteSelfCheck(),
      ok: false,
      code: INVENTORY_READER_MISSING,
      detail: 'this run wired no daemon inventory reader into the pane step, so no post-split session count was measurable',
      sessionCount: null,
      sessionIds: null,
      delta: null,
      baselineLabel: null,
      elapsedMs: null,
    });
    return null;
  }
  return readSplitInventory(ctx, reader, label, armed?.budget ?? null, { compareToPrevious: true, extra: byteSelfCheck });
}

// ---------------------------------------------------------------------------
// 1. split-happy scenario adapter
// ---------------------------------------------------------------------------
export async function runSplitHappyScenario(ctx, plan, budget = new MonotonicBudget()) {
  const { evidence, barrierHub, pid } = ctx;
  const driver = selectNativeDriver(ctx);

  // Trigger Split Right
  if (ctx.platformPreflight !== 'win32') await driver.focus(evidence, pid);
  const splitInventory = await armSplitInventory(ctx, budget);
  await driver.split(evidence, pid);
  // Immediately after the click resolved: the post-click daemon inventory (a
  // local split create adds a daemon session) and the app-stdio byte self-check.
  await recordSplitInventory(ctx, splitInventory);

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
  const splitInventory = await armSplitInventory(ctx, budget);
  await driver.split(evidence, pid);
  // Immediately after the click resolved: the post-click daemon inventory (a
  // local split create adds a daemon session) and the app-stdio byte self-check.
  await recordSplitInventory(ctx, splitInventory);

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
  const splitInventory = await armSplitInventory(ctx, budget);
  await driver.split(evidence, pid);

  // Dispatch cancel request immediately
  const cancelPhase = plan.cancel?.phase ?? 'while-creating';
  barrierHub.command('split-cancel', { phase: cancelPhase });
  evidence.action({ action: 'cancel-request', request: 'split-cancel', phase: cancelPhase });

  // The post-click measurement is taken AFTER the cancel request is on the wire,
  // never before it: the cancel is what this scenario measures, and delaying its
  // dispatch behind a daemon read would move the measured moment from "cancel
  // while creating" to "cancel after create". The request is already written, so
  // the product's own settlement path is untouched by the read that follows.
  await recordSplitInventory(ctx, splitInventory);

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
    cancelReceipt: cancel,
    markerRecognition,
    screenshotMetadata,
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
  const splitInventory = await armSplitInventory(ctx, budget);
  await driver.split(evidence, pid);
  // Immediately after the click resolved: the post-click daemon inventory (a
  // local split create adds a daemon session) and the app-stdio byte self-check.
  await recordSplitInventory(ctx, splitInventory);

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

  // 7b. The conflict wave's own settlement. Deliberately NOT taken from the
  // scenario's attempt budget: the 16-request batch is not part of the measured
  // split attempt, so a slow-but-correct batch must not spend the attempt
  // ceiling. The wait is still bounded, and a batch that never settles fails.
  const batch = await barrierHub.awaitReceipt(
    'split-concurrent-batch', 0, BUDGETS.stagePrepareCreateStatusMs,
  );
  assertConflictWaveReported(batch);
  evidence.action({
    action: 'split-concurrent-batch-settlement',
    conflictRejected: batch.conflictRejected,
    conflictRejectionCodes: batch.conflictRejectionCodes,
    conflictDistinctSessionIds: batch.conflictDistinctSessionIds,
  });

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
