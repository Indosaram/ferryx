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
import { BUDGETS, HarnessError, assertPositiveRecovery } from './common-harness.mjs';

export const CLASSIFIER_BARRIERS = Object.freeze(['backend-write', 'presentation']);

export function buildIsolatedEnv(context) {
  const dataDir = join(context.isolationRoot, 'data');
  const runtimeDir = join(context.isolationRoot, 'runtime');
  const homeDir = join(context.isolationRoot, 'home');
  for (const dir of [dataDir, runtimeDir, homeDir]) mkdirSync(dir, { recursive: true, mode: 0o700 });
  // Review M5: allowlist ONLY. The child env is a fresh literal object, so no
  // ambient variable (FERRYX_MACHINE_TOKEN, account tokens, proxy settings,
  // ...) can leak into the isolated app; the QA barrier/run nonces are added
  // exclusively through the private channel keys below.
  const env = {
    PATH: process.env.PATH,
    HOME: homeDir,
    FERRYX_DATA_DIR: dataDir,
    FERRYX_RUNTIME_DIR: runtimeDir,
    ...context.barrierHub.env(),
  };
  for (const key of Object.keys(env)) {
    if (key.startsWith('FERRYX_') && !['FERRYX_DATA_DIR', 'FERRYX_RUNTIME_DIR', 'FERRYX_QA_BARRIER_DIR', 'FERRYX_QA_RUN_ID'].includes(key)) {
      throw new HarnessError('ASSERTION_FAILURE', `ambient FERRYX_* variable leaked into isolated env: ${key}`);
    }
  }
  return {
    env,
    dirs: { dataDir, runtimeDir, homeDir },
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
  evidence.action({ action: 'launch.binary', binary: ctx.binary, args: binaryArgs, env: { FERRYX_DATA_DIR: isolated.dirs.dataDir, FERRYX_RUNTIME_DIR: isolated.dirs.runtimeDir, FERRYX_QA_BARRIER_DIR: barrierHub.dir } });
  ctx.spawnOwned(ctx.binary, binaryArgs, { env: isolated.env });

  // Registration ACKs (startup scan + live watch) precede any trigger.
  for (const barrier of CLASSIFIER_BARRIERS) await barrierHub.awaitRegistered(barrier);
  evidence.action({ action: 'barriers.registered', barriers: [...CLASSIFIER_BARRIERS] });
  await barrierHub.awaitReceipt('fixture-setup', 0, BUDGETS.stagePrepareCreateStatusMs);

  await barrierHub.awaitHeld('backend-write');
  const writeVerdict = assertClassifierReceipt(await barrierHub.awaitReceipt('backend-write', 0, BUDGETS.stageAttachListenerMs), { verdicts: ['BlockedInIpcWrite'] }, 'backend-write', 'held');
  barrierHub.release('backend-write');
  const writeRecovered = assertClassifierReceipt(await barrierHub.awaitReceipt('backend-write', 1, BUDGETS.stagePrepareCreateStatusMs), { verdicts: ['Idle', 'Healthy'], recovered: true }, 'backend-write', 'released');

  await barrierHub.awaitHeld('presentation');
  const presentationVerdict = assertClassifierReceipt(await barrierHub.awaitReceipt('presentation', 0, BUDGETS.stagePresentationMs), { verdicts: ['BlockedInPresentation'] }, 'presentation', 'held');
  barrierHub.release('presentation');
  const presentationRecovered = assertClassifierReceipt(await barrierHub.awaitReceipt('presentation', 1, BUDGETS.stagePrepareCreateStatusMs), { verdicts: ['Idle', 'Healthy'], recovered: true }, 'presentation', 'released');

  evidence.action({ action: 'classifier-stages', classifierStages: { writeVerdict, writeRecovered, presentationVerdict, presentationRecovered } });
  return { writeVerdict, writeRecovered, presentationVerdict, presentationRecovered };
}
