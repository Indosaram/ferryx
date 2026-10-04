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
  assertPositiveRecovery, BARRIER_ROLES, MonotonicBudget, validateFixtureSetup,
  requireSevenTupleReceipt, requireFiveTupleReceipt,
} from '../lib/qa-scenarios/common-harness.mjs';
import { runHeadlessDiagnosticClassifier, runNativeDiagnosticClassifier, buildIsolatedEnv } from '../lib/qa-scenarios/diagnostic-classifier.mjs';
import {
  MARKER_TEXT, assertAxTrustDarwin, assertNativeAutomationSupported, assertScreenCapture,
  captureScreenshot, clickSplitRightDarwin, focusWindowByPidDarwin, typeMarkerDarwin,
  awaitMarkerRecognition, focusWindowWindows, typeMarkerWindows, windowsDriver,
  awaitOwnedWindowWindows,
} from '../lib/qa-scenarios/native-driver.mjs';
import { admitWindowsInteractiveDesktop, readWindowsRelaunchRecord } from '../lib/qa-scenarios/windows-interactive.mjs';
import {
  runSplitHappyScenario,
  runSplitAttachStallScenario,
  runSplitCancelScenario,
  runSplitConcurrentScenario,
  assertSinglePty,
} from '../lib/qa-scenarios/split-scenarios.mjs';
import {
  runRetainedHandoverScenario,
  runHandoverAbortScenario,
  runSuspensionOwnershipScenario,
  runStaleBindingScenario,
  assertInvariants,
} from '../lib/qa-scenarios/lifecycle-scenarios.mjs';

export { requireSevenTupleReceipt, requireFiveTupleReceipt, assertSinglePty, assertInvariants };

const SOURCE_FILES = [
  'scripts/qa/pane-liveness.mjs',
  'scripts/lib/qa-scenarios/common-harness.mjs',
  'scripts/lib/qa-scenarios/native-driver.mjs',
  'scripts/lib/qa-scenarios/diagnostic-classifier.mjs',
  'scripts/lib/qa-scenarios/split-scenarios.mjs',
  'scripts/lib/qa-scenarios/lifecycle-scenarios.mjs',
  'scripts/lib/qa-scenarios/windows-interactive.mjs',
];
const runnerRoot = join(fileURLToPath(new URL('.', import.meta.url)), '../..');

// Per-scenario plan: barriers to pre-arm before launch and the invariants the
// driver asserts on settled receipts. Barrier semantics are defined by the
// private channel (task-3-rust-proposal.md); a binary without local-split-qa
// support fails the registration ACK explicitly (typed BARRIER_ACK_TIMEOUT).
// Exported so the runner unit suite can replay the pre-launch pre-arm of every
// scenario without launching a product.
export const SCENARIO_PLANS = {
  'diagnostic-classifier': {
    barriers: ['backend-write', 'presentation'], marker: true, splitMenu: false,
    // Receipt names are the product's real barrier settlements: the classifier
    // stages settle on the backend-write/presentation barriers themselves.
    receipts: ['fixture-setup', 'backend-write', 'presentation', 'marker-output'],
  },
  'split-happy': {
    barriers: [], marker: true, splitMenu: true,
    receipts: ['fixture-setup', 'split-create', 'presentation', 'marker-output'],
    fiveTuple: true, timings: true, singlePty: true,
  },
  'split-attach-stall': {
    barriers: ['attach-handshake'], barrierHoldMs: 16_000, marker: true, splitMenu: true,
    // The actionable failure settles on the held attach-handshake barrier; there
    // is no separate `failure-classified` receipt file.
    receipts: ['fixture-setup', 'split-create', 'attach-handshake'],
    failureDeadlineMs: BUDGETS.attemptCeilingMs, sameIdRetry: true, singlePty: true,
  },
  'split-cancel': {
    barriers: [], marker: false, splitMenu: true,
    receipts: ['fixture-setup', 'cancel-ack'],
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
    receipts: ['fixture-setup', 'stale-receipt-rejected', 'reattach-marker', 'marker-output'],
    invariants: ['staleReceiptRejected', 'reattachSameBackend'],
  },
};

// Typed native failures keep their identity in result.error.code. The verdict
// and exit code stay fail-closed and unchanged for the pre-existing codes: an
// environment/harness block is BLOCKED and nonzero, a product assertion failure
// is FAIL, and the cleanup gate can still force FAIL - a blocked run never
// becomes a pass.
export const BLOCKED_CODES = Object.freeze([
  'AX_UNTRUSTED', 'CAPTURE_DENIED', 'NATIVE_AUTOMATION_UNSUPPORTED', 'BARRIER_ACK_TIMEOUT',
  'MARKER_RECOGNITION_UNVERIFIED', 'TASK4_IDENTITY_DEPENDENCY',
  // Windows interactive-desktop lane (pass-4 blockers): the run could not reach
  // a state in which it owns a visible window and a unique affordance.
  'NO_INTERACTIVE_SESSION', 'NO_OWNED_WINDOW', 'INTERACTIVE_RELAUNCH_FAILED',
  'SPLIT_RIGHT_NOT_FOUND', 'SPLIT_RIGHT_NOT_UNIQUE', 'SPLIT_RIGHT_DISABLED',
]);

export function classifyNativeFailure(code) {
  const verdict = BLOCKED_CODES.includes(code) ? 'BLOCKED' : 'FAIL';
  const exitCode = code === 'AX_UNTRUSTED' ? EXIT.axUntrusted
    : code === 'CAPTURE_DENIED' ? EXIT.captureDenied
    : code === 'NATIVE_AUTOMATION_UNSUPPORTED' ? EXIT.nativeAutomationUnsupported
    : code === 'BARRIER_ACK_TIMEOUT' ? EXIT.barrierUnsupported
    : code === 'MARKER_RECOGNITION_UNVERIFIED' ? EXIT.markerRecognitionUnverified
    : code === 'TASK4_IDENTITY_DEPENDENCY' ? EXIT.task4IdentityDependency
    : BLOCKED_CODES.includes(code) ? EXIT.nativeAutomationUnsupported
    : EXIT.scenarioFailure;
  return { verdict, exitCode };
}

async function runNativeScenario(ctx) {
  const plan = SCENARIO_PLANS[ctx.scenario];
  const { evidence, barrierHub } = ctx;

  // Permission gates FIRST: typed rejections with no native actions recorded.
  assertNativeAutomationSupported();
  // Pass-4 blocker 1: a Windows run launched from an SSH session lands in
  // session 0, where no window can ever be shown. The admission decision (and,
  // when it failed, the measured session/interactivity evidence) is recorded
  // before any launch; a blocked admission never launches and never passes.
  if (ctx.windowsAdmission) {
    evidence.action({ action: 'windows-interactive-admission', ...ctx.windowsAdmission.evidence });
    if (ctx.windowsAdmission.mode === 'blocked') {
      throw new HarnessError(ctx.windowsAdmission.code, ctx.windowsAdmission.detail);
    }
  }
  if (ctx.windowsRelaunchRecord) {
    evidence.action({ action: 'windows-interactive-relaunch', ...ctx.windowsRelaunchRecord });
  }
  if (ctx.platformPreflight === 'darwin') {
    await assertAxTrustDarwin(evidence);
    await assertScreenCapture(evidence);
  }

  const isolated = buildIsolatedEnv(ctx);
  evidence.action({
    action: 'launch.binary',
    binary: ctx.binary,
    env: {
      FERRYX_DATA_DIR: isolated.dirs.dataDir,
      FERRYX_RUNTIME_DIR: isolated.dirs.runtimeDir,
      FERRYX_QA_BARRIER_DIR: barrierHub.dir,
      FERRYX_QA_OPERATION_ID: ctx.operationId,
    },
  });
  // The GUI lane installs the private channel from this env at boot and
  // settles `fixture-setup` line 0 from the real isolated-profile session
  // inventory before any trigger.
  const child = ctx.spawnOwned(ctx.binary, [], { env: isolated.env });
  const pid = child.pid;
  ctx.pid = pid;

  // Monotonic budget tracker: all waits consume remaining budget
  const budget = new MonotonicBudget(BUDGETS.attemptCeilingMs);

  // Scenario-specific fixture setup validation (never requires all four fixtures for basic split!)
  const rawFixture = await barrierHub.awaitReceipt('fixture-setup', 0, budget.consume(BUDGETS.stagePrepareCreateStatusMs, 'fixture-setup'));
  const fixture = validateFixtureSetup(rawFixture, ctx.scenario);
  evidence.action({ action: 'fixture-setup', sessions: fixture.sessions });

  // Pass-4 blocker 1 (continued): the driver may only address a window this run
  // really owns and that is really visible. In Windows session 0 the app's
  // windows are created but can never be shown, so this bounded wait fails
  // typed (`NO_INTERACTIVE_SESSION` / `NO_OWNED_WINDOW`, with the measured
  // session id and per-window visibility) instead of letting a driver click a
  // window it does not own. macOS is unchanged.
  if (ctx.platformPreflight === 'win32') {
    await awaitOwnedWindowWindows(evidence, pid, budget.consume(BUDGETS.ownedWindowReadyMs, 'owned-window'));
  }

  // Every armed barrier must be registered by the product before triggers.
  for (const barrier of plan.barriers) {
    await barrierHub.awaitRegistered(barrier, budget.consume(BUDGETS.barrierAckTimeoutMs, `register ${barrier}`));
  }
  if (plan.barriers.length > 0) evidence.action({ action: 'barriers.registered', barriers: [...plan.barriers] });

  let scenarioResult;
  let triggerLabel = 'split-menu-click';

  switch (ctx.scenario) {
    case 'diagnostic-classifier':
      triggerLabel = 'diagnostic-classifier';
      scenarioResult = await runNativeDiagnosticClassifier(ctx, plan, budget);
      break;
    case 'split-happy':
      triggerLabel = 'split-menu-click';
      scenarioResult = await runSplitHappyScenario(ctx, plan, budget);
      break;
    case 'split-attach-stall':
      triggerLabel = 'split-menu-click';
      scenarioResult = await runSplitAttachStallScenario(ctx, plan, budget);
      break;
    case 'split-cancel':
      triggerLabel = 'cancel-request';
      scenarioResult = await runSplitCancelScenario(ctx, plan, budget);
      break;
    case 'split-concurrent':
      triggerLabel = 'split-concurrent-trigger';
      scenarioResult = await runSplitConcurrentScenario(ctx, plan, budget);
      break;
    case 'retained-handover':
      triggerLabel = 'trigger-handover';
      scenarioResult = await runRetainedHandoverScenario(ctx, plan, budget);
      break;
    case 'handover-abort':
      triggerLabel = 'trigger-handover-abort';
      scenarioResult = await runHandoverAbortScenario(ctx, plan, budget);
      break;
    case 'suspension-ownership':
      triggerLabel = 'trigger-suspension-check';
      scenarioResult = await runSuspensionOwnershipScenario(ctx, plan, budget);
      break;
    case 'stale-binding':
      triggerLabel = 'trigger-stale-binding';
      scenarioResult = await runStaleBindingScenario(ctx, plan, budget);
      break;
    default:
      throw new HarnessError('INVALID_SCENARIO', `unhandled scenario ${ctx.scenario}`);
  }

  const attemptMs = budget.elapsedMs();
  if (budget.isExceeded()) {
    throw new HarnessError('ASSERTION_FAILURE', `attempt ${attemptMs}ms (trigger: ${triggerLabel}) exceeds correctness ceiling ${BUDGETS.attemptCeilingMs}ms`);
  }
  evidence.action({
    action: 'attempt-budget',
    triggerLabel,
    attemptMs,
    ceilingMs: BUDGETS.attemptCeilingMs,
    warmTargetMs: attemptMs <= BUDGETS.warmTargetMs ? 'met' : 'exceeded-reportable',
  });

  // Release any unreleased barriers
  for (const barrier of plan.barriers) {
    try { barrierHub.release(barrier); } catch { /* ignore if already released */ }
  }

  return {
    attemptMs,
    deadlineAt: budget.deadlineAt,
    triggerLabel,
    markerRecognition: scenarioResult?.markerRecognition ?? null,
    scenarioResult,
  };
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

  // Pass-4 blocker 1: a Windows run launched from an SSH session lands in
  // session 0, where the app can never own a visible window. Admission probes
  // the session before anything else; when an active console session is
  // reachable it re-runs THIS runner with the unchanged argv inside that
  // session (scheduled task with /it - the mechanism the verifier proved) and
  // adopts the delegated exit code, so the delegated run owns
  // result.json/latest.json and this process writes no verdict of its own. A
  // session that cannot be reached fails typed, never as a pass.
  const windowsAdmission = await admitWindowsInteractiveDesktop({ invocation, context, rawArgv: argv });
  if (windowsAdmission?.mode === 'delegated') {
    process.stderr.write(`${JSON.stringify({
      verdict: 'DELEGATED-TO-INTERACTIVE-SESSION',
      code: null,
      session: windowsAdmission.evidence?.verdict ?? null,
      delegated: windowsAdmission.relaunch,
    })}\n`);
    if (windowsAdmission.innerStdout) process.stdout.write(windowsAdmission.innerStdout);
    return windowsAdmission.exitCode;
  }

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
    windowsAdmission,
    windowsRelaunchRecord: readWindowsRelaunchRecord(),
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
    const classified = classifyNativeFailure(code);
    result.verdict = classified.verdict;
    result.error = { code, message: error.message };
    exitCode = classified.exitCode;
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
