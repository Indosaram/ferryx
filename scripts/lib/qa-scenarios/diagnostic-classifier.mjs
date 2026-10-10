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

/**
 * The shell the QA app must spawn panes with, or null where the platform default is correct.
 *
 * Windows needs this pinned. The harness serves the frontend on `http://127.0.0.1:5173`, which is
 * the SAME origin the host user's real app uses, and WebView2 keeps localStorage in the shared
 * `%LOCALAPPDATA%\<bundle-id>\EBWebView` profile that an environment variable cannot redirect (it is
 * resolved through the Windows known-folder API). A host user whose stored
 * `ferryx.terminal.settings` names a shell therefore pushes that shell into the QA app through
 * `syncNativeOverrides` -> `cmd_terminal_set_preferences` -> `cached_terminal_preferences()`,
 * and `src-tauri/src/ipc/terminal.rs:4829` substitutes it for the frontend's null. Measured on
 * maho-win: the host profile held `{"shell":"wsl"}` while WSL was not installed, so every pane
 * and split spawned a shell that could never start, produced no PTY output, and the marker step
 * could not observe its echo (the daemon-side fixture spawn, which does not read frontend
 * settings, worked).
 *
 * Naming the shell here makes a QA run independent of whatever the host user happens to have
 * stored, without changing the product's own default.
 */
export const QA_PINNED_SHELL = process.platform === 'win32' ? 'pwsh' : null;

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
  // Windows app data is NOT covered by HOME. WebView2 keeps its profile (and therefore the
  // frontend's localStorage) under %LOCALAPPDATA%\<bundle-id>\EBWebView, and the app's own
  // Roaming data sits under %APPDATA%. Leaving those unset made the QA app share the host user's
  // real profile: the measured symptom was a pane/split spawned with `shell: "wsl"` (the host
  // user's stored shell preference) on a host where WSL is not installed, so the shell never
  // started and the marker was never observed, while the daemon-side fixture spawn - which does
  // not read frontend settings - correctly used the default shell.
  const appDataDir = join(context.isolationRoot, 'appdata');
  const localAppDataDir = join(context.isolationRoot, 'localappdata');
  for (const dir of [dataDir, runtimeDir, homeDir, sessionDir, appDataDir, localAppDataDir]) mkdirSync(dir, { recursive: true, mode: 0o700 });
  // Review M5: allowlist ONLY. The child env is a fresh literal object, so no
  // ambient variable (FERRYX_MACHINE_TOKEN, account tokens, proxy settings,
  // ...) can leak into the isolated app; the QA barrier/run nonces are added
  // exclusively through the private channel keys below. Every FERRYX_* key that
  // appears here is either a product override the app itself reads
  // (FERRYX_DATA_DIR / FERRYX_RUNTIME_DIR / FERRYX_SESSION_DIR) or a QA channel
  // key this harness owns - never an ambient value.
  // Linux GUI sessions need their own display environment, and the allowlist
  // above deliberately carries none of it. Measured on omarchy: with only
  // PATH/HOME the app aborts inside `tao` with "Failed to initialize GTK", so
  // the webview never comes up and no product stage can settle - the run stalls
  // at `fixture-setup` with `barriers: []`, which reads like a product defect
  // and is not one. GTK needs XDG_RUNTIME_DIR plus a display socket
  // (WAYLAND_DISPLAY or DISPLAY), and GLib wants the session bus. These are
  // socket locations and paths only - no token, credential or FERRYX_* value -
  // so they are passed on Linux and nowhere else, and the FERRYX_* leak guard
  // below still applies to the assembled environment.
  const displayEnv = process.platform === 'linux'
    ? Object.fromEntries(
      ['XDG_RUNTIME_DIR', 'WAYLAND_DISPLAY', 'DISPLAY', 'DBUS_SESSION_BUS_ADDRESS',
        'XDG_SESSION_TYPE', 'XDG_CURRENT_DESKTOP', 'XAUTHORITY']
        .filter(key => typeof process.env[key] === 'string' && process.env[key].length > 0)
        .map(key => [key, process.env[key]]))
    : {};
  const env = {
    PATH: process.env.PATH,
    HOME: homeDir,
    APPDATA: appDataDir,
    LOCALAPPDATA: localAppDataDir,
    ...displayEnv,
    ...(QA_PINNED_SHELL ? { FERRYX_QA_SHELL: QA_PINNED_SHELL } : {}),
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
    if (key.startsWith('FERRYX_') && !['FERRYX_DATA_DIR', 'FERRYX_RUNTIME_DIR', 'FERRYX_SESSION_DIR', 'FERRYX_QA_BARRIER_DIR', 'FERRYX_QA_RUN_ID', 'FERRYX_QA_OPERATION_ID', 'FERRYX_QA_FIXTURE_KINDS', 'FERRYX_QA_SHELL'].includes(key)) {
      throw new HarnessError('ASSERTION_FAILURE', `ambient FERRYX_* variable leaked into isolated env: ${key}`);
    }
  }
  return {
    env,
    dirs: { dataDir, runtimeDir, homeDir, sessionDir, appDataDir, localAppDataDir },
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

  // The write stage is addressed by the session the runner BOUND, never by the
  // operation nonce. A real keystroke's request id is minted by the frontend
  // input queue (`req-<queueRunId>-<sessionId>-<itemId>`,
  // `ui/src/lib/nativeTerminalInputQueue.ts`), while the runner's nonce travels
  // only in the product's inherited env and in the arm file, so the two can
  // never be equal on the GUI lane. Measured on maho-win against the app's own
  // `ferryx-switch-debug.jsonl`: every
  // `terminal.surface.input.stage.backend_write` line carried
  // `operationId: "req-f3c8e184-…-5"` while the arm held
  // `qa-op-96831e1e-…` - which is why `awaitHeld('backend-write')` timed out on
  // every run of the campaign.
  const paneTarget = ctx.paneBinding?.backendSessionId;
  if (typeof paneTarget !== 'string' || paneTarget.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', 'diagnostic-classifier: the pane step settled no bound session, so the barrier stages have no target');
  }
  // BOTH armed barriers are addressed the same way. The render coordinator
  // adopts `<name>.bind.json` before it will hold, so an unbound `presentation`
  // barrier reports `presentation_binding_failed` and never holds either.
  barrierHub.bindBackendSession('backend-write', paneTarget);
  barrierHub.bindBackendSession('presentation', paneTarget);
  evidence.action({ action: 'classifier-barriers-bound', backendSessionId: paneTarget });

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
  // `presentation` is deliberately ARMED for this scenario, so the render
  // coordinator is holding the pane's frame from boot on. The classifier is
  // therefore right to report `BlockedInPresentation` immediately after the write
  // release - and `assertPositiveRecovery` would read that correct verdict as an
  // unproven recovery. What this assertion is about is the WRITE stage's own
  // progress, so it is judged on that stage's progress evidence, exactly as the
  // headless lane's `assertClassifierReceipt(recovered)` path does.
  const writeProgress = writeRecovered?.stageProgress;
  if (writeProgress?.releaseOutcome !== 'released' || writeProgress.backendWriteCompleted !== true || writeProgress.success !== true) {
    throw new HarnessError('RECOVERY_UNPROVEN', `classifier after write release: the write stage did not report completed progress: ${JSON.stringify(writeProgress)}`);
  }
  if (writeRecovered.classifierVerdict === 'BlockedInIpcWrite') {
    throw new HarnessError('RECOVERY_UNPROVEN', `classifier after write release still reports BlockedInIpcWrite: ${JSON.stringify(writeRecovered)}`);
  }

  // 3. Presentation stage hold & recovery assertion
  //
  // The hold appears when a frame is DISPATCHED through the armed path AFTER the pane
  // step has bound the session. The pane's own first dispatch is too early - the runner
  // can only write `presentation.bind.json` once the pane exists - and the armed path
  // reports that as a `presentation_binding_failed` record and then dispatches the frame
  // normally instead of dropping it. So this step types into the pane again, which is
  // the dispatch that adopts the bind and holds.
  //
  // The receipts are therefore matched BY VERDICT, not by index: index 0 of the
  // `presentation` stream is that early binding-failure record on every run, and an
  // index-based read would assert `BlockedInPresentation` against a failure record.
  if (ctx.platformPreflight === 'win32') {
    await focusWindowWindows(evidence, pid);
    await typeMarkerWindows(evidence, pid);
  } else {
    await typeMarkerDarwin(evidence, pid);
  }
  await barrierHub.awaitHeld('presentation', budget.consume(BUDGETS.stagePresentationMs, 'presentation hold'));
  const presentationHeld = await barrierHub.awaitReceiptMatching('presentation', {
    match: receipt => receipt?.classifierVerdict === 'BlockedInPresentation',
    timeoutMs: budget.consume(BUDGETS.stagePresentationMs, 'presentation held receipt'),
    label: 'presentation held',
  });
  barrierHub.release('presentation');
  const presentationRecovered = await barrierHub.awaitReceiptMatching('presentation', {
    match: receipt => receipt?.classifierVerdict === 'Idle' || receipt?.classifierVerdict === 'Healthy',
    timeoutMs: budget.consume(BUDGETS.stagePrepareCreateStatusMs, 'presentation released receipt'),
    label: 'presentation released',
  });
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
  //
  // The receipt is awaited BY MATCH, not by index. The marker is typed at three points, and the
  // first two happen while the `presentation` barrier holds the pane's frames - this scenario
  // arms it from boot on purpose - so a receipt from those legitimately carries
  // `frameSubmitted: false`. Index 0 would assert a submitted frame against the wrong marker.
  // What this step is about is a marker whose frame really was submitted, so that is what it
  // awaits; the read settles as soon as such a receipt exists, however many precede it.
  if (ctx.platformPreflight === 'win32') {
    await focusWindowWindows(evidence, pid);
    await typeMarkerWindows(evidence, pid);
  } else {
    await typeMarkerDarwin(evidence, pid);
  }
  const markerReceipt = await barrierHub.awaitReceiptMatching('marker-output', {
    match: receipt => receipt?.frameSubmitted === true
      && String(receipt?.output ?? '').includes(MARKER_TEXT),
    timeoutMs: budget.consume(BUDGETS.stagePresentationMs, 'marker output'),
    label: 'marker output with a submitted frame',
  });
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
