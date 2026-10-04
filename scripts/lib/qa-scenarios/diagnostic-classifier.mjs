#!/usr/bin/env node
// Task 3 scenario: diagnostic-classifier (--headless lane only; the native
// variant is driven by scripts/qa/pane-liveness.mjs via the generic native
// driver so there is exactly one evidence writer, registry and barrier hub).
//
// Drives the pane-liveness classifier through pre-armed private barriers:
//   - backend-write held   => classifier must report BlockedInIpcWrite
//   - presentation held    => classifier must report BlockedInPresentation
//   - releases             => classifier must report Idle/Healthy recovery
// Every receipt must echo the runId/operationId nonces (correlateReceipt).
// A headless run records nativeEvidence: deferred-to-task-10 and can never
// produce a native PASS.

import { join } from 'node:path';
import { mkdirSync } from 'node:fs';
import { BUDGETS, HarnessError, assertPositiveRecovery, validateFixtureSetup, fixtureKindsEnvValue, MonotonicBudget } from './common-harness.mjs';
import {
  MARKER_TEXT,
  focusWindowByPidDarwin, typeMarkerDarwin,
  focusWindowWindows, typeMarkerWindows,
  captureOwnedWindowDarwin, captureOwnedWindowWindows,
  performInspectionHandshake,
} from './native-driver.mjs';

export const CLASSIFIER_BARRIERS = Object.freeze(['backend-write', 'presentation']);

export function buildIsolatedEnv(context) {
  const dataDir = join(context.isolationRoot, 'data');
  const runtimeDir = join(context.isolationRoot, 'runtime');
  const homeDir = join(context.isolationRoot, 'home');
  // Pass-11 leak (controlled A/B, measured): the product resolves its persisted
  // `session_state.json` through `session_dir_override()` = FERRYX_SESSION_DIR
  // BEFORE any host-profile fallback (`src-tauri/src/daemon/server.rs:361`,
  // consumed at `src-tauri/src/ipc/session.rs:10`). Without this override the app
  // reads AND rewrites the HOST's real profile
  // (`%APPDATA%\com.ferryx.app\dev\session_state.json`), boots into the host's
  // restored layout instead of the empty state, and the pane step's `New
  // Terminal` affordance does not exist. The repository's own Windows QA recipe
  // requires both variables and says so:
  // `docs/evidence/windows-terminal-20260912/windows-environment.md:13`
  // ("Set BOTH FERRYX_SESSION_DIR and FERRYX_RUNTIME_DIR to unique QA paths").
  const sessionDir = join(context.isolationRoot, 'session');
  for (const dir of [dataDir, runtimeDir, homeDir, sessionDir]) mkdirSync(dir, { recursive: true, mode: 0o700 });
  // Review M5: allowlist ONLY. The child env is a fresh literal object, so no
  // ambient variable (FERRYX_MACHINE_TOKEN, account tokens, proxy settings,
  // ...) can leak into the isolated app; the QA barrier/run nonces are added
  // exclusively through the private channel keys below. Every FERRYX_* key that
  // appears here is either a product override the app itself reads
  // (FERRYX_DATA_DIR / FERRYX_RUNTIME_DIR / FERRYX_SESSION_DIR) or a QA channel
  // key this harness owns - never an ambient value.
  const env = {
    PATH: process.env.PATH,
    HOME: homeDir,
    FERRYX_DATA_DIR: dataDir,
    FERRYX_RUNTIME_DIR: runtimeDir,
    FERRYX_SESSION_DIR: sessionDir,
    // Product-facing fixture declaration: which fixture session kinds this
    // scenario's `fixture-setup` settlement must provision and report. Derived
    // from the single requirement map (never a second list), so the kinds the
    // runner later validates are exactly the kinds it declared here.
    FERRYX_QA_FIXTURE_KINDS: fixtureKindsEnvValue(context.scenario),
    ...context.barrierHub.env(),
  };
  for (const key of Object.keys(env)) {
    if (key.startsWith('FERRYX_') && !['FERRYX_DATA_DIR', 'FERRYX_RUNTIME_DIR', 'FERRYX_SESSION_DIR', 'FERRYX_QA_BARRIER_DIR', 'FERRYX_QA_RUN_ID', 'FERRYX_QA_OPERATION_ID', 'FERRYX_QA_FIXTURE_KINDS'].includes(key)) {
      throw new HarnessError('ASSERTION_FAILURE', `ambient FERRYX_* variable leaked into isolated env: ${key}`);
    }
  }
  return {
    env,
    dirs: { dataDir, runtimeDir, homeDir, sessionDir },
  };
}

// Headless diagnostic-classifier receipt assertion:
// Enforces precise positive stage completion for headless only (product stageProgress contract).
// Never fakes native presentation (requires coordinator-consumed); never accepts generic Unknown.
export function assertClassifierReceipt(receipt, expectation, barrierName, stage) {
  const verdict = receipt?.classifierVerdict ?? receipt?.verdict;
  for (const field of ['sessionId', 'operationId']) {
    if (!receipt?.[field]) {
      throw new HarnessError('ASSERTION_FAILURE', `barrier ${barrierName} [${stage}]: receipt missing ${field}`);
    }
  }

  if (!expectation.recovered) {
    if (!expectation.verdicts.includes(verdict)) {
      throw new HarnessError('ASSERTION_FAILURE',
        `barrier ${barrierName} [${stage}]: expected classifier verdict in [${expectation.verdicts.join(', ')}], got ${JSON.stringify(verdict)}`);
    }
    return verdict;
  }

  // Precise positive stage completion for headless only:
  const progress = receipt?.stageProgress;
  if (!progress || progress.releaseOutcome !== 'released') {
    throw new HarnessError('RECOVERY_UNPROVEN', `barrier ${barrierName} [${stage}]: missing stageProgress or releaseOutcome !== 'released': ${JSON.stringify(progress)}`);
  }

  if (barrierName === 'backend-write') {
    if (progress.backendWriteCompleted !== true || progress.success !== true) {
      throw new HarnessError('RECOVERY_UNPROVEN', `barrier ${barrierName} [${stage}]: backend-write stageProgress not positive (completed=${progress.backendWriteCompleted}, success=${progress.success})`);
    }
  } else if (barrierName === 'presentation') {
    if (progress.frameConsumed !== true || progress.renderPendingAfter !== false) {
      throw new HarnessError('RECOVERY_UNPROVEN', `barrier ${barrierName} [${stage}]: presentation stageProgress not positive (frameConsumed=${progress.frameConsumed}, renderPendingAfter=${progress.renderPendingAfter})`);
    }
    if (receipt.presentationEvidence !== 'coordinator-consumed') {
      throw new HarnessError('RECOVERY_UNPROVEN', `barrier ${barrierName} [${stage}]: presentationEvidence must be 'coordinator-consumed' for headless recovery, got ${JSON.stringify(receipt.presentationEvidence)}`);
    }
  }

  // Acceptance: Idle or Healthy; or truthful Unknown ONLY when evidenceMissing === true with verified stageProgress above.
  if (verdict === 'Idle' || verdict === 'Healthy') {
    return verdict;
  }
  if (verdict === 'Unknown' && receipt?.evidenceMissing === true) {
    return verdict;
  }
  throw new HarnessError('RECOVERY_UNPROVEN', `barrier ${barrierName} [${stage}]: post-release verdict ${JSON.stringify(verdict)} is not positive recovery (expected Idle/Healthy or truthful Unknown with evidenceMissing)`);
}

// `ctx` is the single shared fullContext built by the entrypoint (evidence,
// registry, barrierHub, runId/operationId nonces, spawnOwned). This module
// creates NO second writer/registry/hub (review blocker 8).
export async function runHeadlessDiagnosticClassifier(ctx) {
  const { evidence, barrierHub } = ctx;
  const isolated = buildIsolatedEnv(ctx);
  const binaryArgs = ['diagnostic-classifier', '--headless'];
  evidence.action({ action: 'launch.binary', binary: ctx.binary, args: binaryArgs, env: { FERRYX_DATA_DIR: isolated.dirs.dataDir, FERRYX_RUNTIME_DIR: isolated.dirs.runtimeDir, FERRYX_SESSION_DIR: isolated.dirs.sessionDir, FERRYX_QA_BARRIER_DIR: barrierHub.dir, FERRYX_QA_OPERATION_ID: ctx.operationId } });
  ctx.spawnOwned(ctx.binary, binaryArgs, { env: isolated.env });

  // Registration ACKs (startup scan + live watch) precede any trigger.
  for (const barrier of CLASSIFIER_BARRIERS) await barrierHub.awaitRegistered(barrier);
  evidence.action({ action: 'barriers.registered', barriers: [...CLASSIFIER_BARRIERS] });
  await barrierHub.awaitReceipt('fixture-setup', 0, BUDGETS.stagePrepareCreateStatusMs);

  await barrierHub.awaitHeld('backend-write');
  const writeVerdict = assertClassifierReceipt(await barrierHub.awaitReceipt('backend-write', 0, BUDGETS.stageAttachListenerMs), { verdicts: ['BlockedInIpcWrite'] }, 'backend-write', 'held');
  barrierHub.release('backend-write');
  const writeRecovered = assertClassifierReceipt(await barrierHub.awaitReceipt('backend-write', 1, BUDGETS.stagePrepareCreateStatusMs), { verdicts: ['Idle', 'Healthy'], recovered: true }, 'backend-write', 'released');

  const presentationHeld = await barrierHub.awaitHeld('presentation');
  const presentationVerdict = assertClassifierReceipt(presentationHeld, { verdicts: ['BlockedInPresentation'] }, 'presentation', 'held');
  if (presentationHeld.presentationEvidence !== 'coordinator-pending' || presentationHeld.coordinatorEvidence !== 'coordinator-pending') {
    throw new HarnessError('ASSERTION_FAILURE', 'presentation held event lacks pending coordinator evidence');
  }
  barrierHub.release('presentation');
  const presentationRecovered = assertClassifierReceipt(await barrierHub.awaitReceipt('presentation', 0, BUDGETS.stagePrepareCreateStatusMs), { verdicts: ['Idle', 'Healthy'], recovered: true }, 'presentation', 'released');

  evidence.action({ action: 'classifier-stages', classifierStages: { writeVerdict, writeRecovered, presentationVerdict, presentationRecovered } });
  return { writeVerdict, writeRecovered, presentationVerdict, presentationRecovered };
}

// Native diagnostic-classifier scenario:
// Drives real OS events, write stage hold, presentation hold, and EOF event on an owned output stream.
// Proves correct classification (BlockedInIpcWrite / BlockedInPresentation), no payload in logs,
// positive recovery on release, and visible fresh output via owned-window screenshot inspection handshake.
export async function runNativeDiagnosticClassifier(ctx, plan, budget = new MonotonicBudget()) {
  const { evidence, barrierHub, pid } = ctx;

  // 1. Initial typing into owned pane
  if (ctx.platformPreflight === 'win32') {
    await focusWindowWindows(evidence, pid);
    await typeMarkerWindows(evidence, pid);
  } else {
    await focusWindowByPidDarwin(evidence, pid);
    await typeMarkerDarwin(evidence, pid);
  }

  // 2. Writer stage hold & recovery assertion
  await barrierHub.awaitHeld('backend-write', budget.consume(BUDGETS.stageAttachListenerMs, 'backend-write hold'));
  const writeHeld = await barrierHub.awaitReceipt('backend-write', 0, budget.consume(BUDGETS.stageAttachListenerMs, 'backend-write held receipt'));
  if (writeHeld.classifierVerdict !== 'BlockedInIpcWrite') {
    throw new HarnessError('ASSERTION_FAILURE', `classifier must report BlockedInIpcWrite while writer is held, got ${JSON.stringify(writeHeld.classifierVerdict)}`);
  }
  barrierHub.release('backend-write');
  const writeRecovered = await barrierHub.awaitReceipt('backend-write', 1, budget.consume(BUDGETS.stagePrepareCreateStatusMs, 'backend-write released receipt'));
  assertPositiveRecovery(writeRecovered, 'classifier after write release');

  // 3. Presentation stage hold & recovery assertion
  await barrierHub.awaitHeld('presentation', budget.consume(BUDGETS.stagePresentationMs, 'presentation hold'));
  const presentationHeld = await barrierHub.awaitReceipt('presentation', 0, budget.consume(BUDGETS.stagePresentationMs, 'presentation held receipt'));
  if (presentationHeld.classifierVerdict !== 'BlockedInPresentation') {
    throw new HarnessError('ASSERTION_FAILURE', `classifier must report BlockedInPresentation while presentation is held, got ${JSON.stringify(presentationHeld.classifierVerdict)}`);
  }
  barrierHub.release('presentation');
  const presentationRecovered = await barrierHub.awaitReceipt('presentation', 1, budget.consume(BUDGETS.stagePrepareCreateStatusMs, 'presentation released receipt'));
  assertPositiveRecovery(presentationRecovered, 'classifier after presentation release');

  // 4. End one owned output stream to exercise existing EOF event
  barrierHub.command('exercise-eof', { action: 'end-output-stream' });
  evidence.action({ action: 'exercise-eof', stream: 'owned-output-stream' });
  const eofReceipt = await barrierHub.awaitReceipt('eof-handled', 0, budget.consume(BUDGETS.stageAttachListenerMs, 'eof handled receipt'));
  if (eofReceipt.eofObserved !== true) {
    throw new HarnessError('ASSERTION_FAILURE', `expected EOF event to be observed on ended output stream: ${JSON.stringify(eofReceipt)}`);
  }
  if (eofReceipt.newPtyCreated === true) {
    throw new HarnessError('ASSERTION_FAILURE', `EOF event handling must not create a new PTY: ${JSON.stringify(eofReceipt)}`);
  }

  // 5. Post-release marker output verification
  if (ctx.platformPreflight === 'win32') {
    await focusWindowWindows(evidence, pid);
    await typeMarkerWindows(evidence, pid);
  } else {
    await typeMarkerDarwin(evidence, pid);
  }
  const markerReceipt = await barrierHub.awaitReceipt('marker-output', 0, budget.consume(BUDGETS.stagePresentationMs, 'marker output'));
  if (!String(markerReceipt?.output ?? '').includes(MARKER_TEXT)) {
    throw new HarnessError('ASSERTION_FAILURE', `marker ${MARKER_TEXT} not observed after classifier recovery: ${JSON.stringify(markerReceipt)}`);
  }
  if (markerReceipt.frameSubmitted !== true) {
    throw new HarnessError('ASSERTION_FAILURE', `marker receipt attests no submitted frame: ${JSON.stringify(markerReceipt)}`);
  }

  // 6. Capture owned window screenshot with bounds metadata
  const screenshotPath = join(ctx.evidenceRunDir, 'screenshot.png');
  let screenshotMetadata;
  if (ctx.platformPreflight === 'win32') {
    screenshotMetadata = await captureOwnedWindowWindows(evidence, screenshotPath, pid);
  } else {
    screenshotMetadata = await captureOwnedWindowDarwin(evidence, screenshotPath, pid);
  }

  // 7. Bounded inspection handshake
  const markerRecognition = await performInspectionHandshake(
    evidence,
    barrierHub,
    { runId: ctx.runId, operationId: ctx.operationId },
    screenshotMetadata,
    budget.consume(BUDGETS.stagePresentationMs, 'inspection handshake')
  );

  const classifierStages = {
    writeHeld: writeHeld.classifierVerdict,
    writeRecovered: writeRecovered.classifierVerdict,
    presentationHeld: presentationHeld.classifierVerdict,
    presentationRecovered: presentationRecovered.classifierVerdict,
    eofHandled: true,
  };
  evidence.action({ action: 'classifier-stages', classifierStages });

  return {
    classifierStages,
    markerRecognition,
    screenshotMetadata,
  };
}
