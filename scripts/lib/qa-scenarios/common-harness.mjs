#!/usr/bin/env node
// Task 3 (local-pane-liveness-root-remediation) shared QA harness primitives.
// Private barrier channel contract (product side is gated behind the
// `local-split-qa` Cargo feature and is NOT yet implemented in frozen sources;
// see .omo/evidence/local-pane-liveness-root-remediation/task-3-rust-proposal.md):
//   - Runner pre-arms a barrier by writing  <dir>/<barrier>.arm.json   (before launch)
//   - Product registers the arm with        <dir>/<barrier>.armed-ack.json  (startup scan + live watch)
//   - Product acknowledges holding with    <dir>/<barrier>.held.json
//   - Runner releases by writing            <dir>/<barrier>.release.json
//   - Product settles with append-only      <dir>/<barrier>.receipt.jsonl  (one JSON object per line)
//   - Runner->product commands (e.g. split-cancel) use <dir>/<command>.request.json
// Every arm file carries the unique runId and operationId nonces (also passed
// as FERRYX_QA_RUN_ID via the private env); every held/receipt line MUST echo
// them plus the producing component ID (`producer`). The runner correlates
// all three before trusting any receipt; mismatch is a typed failure.
// The channel is passed only via the inherited private env var FERRYX_QA_BARRIER_DIR.
// Missing held/receipt acknowledgement within the bounded deadline is an explicit
// failure; the runner can never mark an unsupported barrier PASS.

import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import {
  accessSync, constants, copyFileSync, createWriteStream, existsSync, mkdirSync, readdirSync, rmSync, statSync,
  watch, writeFileSync, realpathSync, lstatSync, renameSync, readFileSync, readlinkSync,
} from 'node:fs';
import { homedir, platform, release, arch } from 'node:os';
import { isAbsolute, join, resolve, dirname, parse } from 'node:path';
import { fileURLToPath } from 'node:url';

export const SCENARIOS = Object.freeze([
  'diagnostic-classifier',
  'split-happy',
  'split-attach-stall',
  'split-cancel',
  'split-concurrent',
  'retained-handover',
  'handover-abort',
  'suspension-ownership',
  'stale-binding',
]);

export const HEADLESS_ELIGIBLE = Object.freeze(['diagnostic-classifier']);

export const LOCAL_SPLIT_LIFECYCLE_CAPABILITY = 'localSplitLifecycleV1';
export const LOCAL_SPLIT_VALIDITY_MS = 600_000;

export const ATTEMPT_TOTAL_BUDGET_MS = 15_000;
export const STAGE_CREATE_OR_STATUS_MAX_MS = 9_000;
export const STAGE_ATTACH_OR_LISTENER_MAX_MS = 4_000;
export const STAGE_PRESENTATION_MAX_MS = 2_000;
export const STAGE_CWD_PROBE_MAX_MS = 500;
export const CANCEL_ACK_MAX_MS = 3_000;
export const DAEMON_CANCEL_CLEANUP_MAX_MS = 2_500;
export const WARM_NATIVE_READY_TARGET_MS = 2_000;

// Barrier -> the handover-side `targetRole` its arm spec may carry. The
// vocabulary is exactly 'predecessor'/'successor': `targetRole` names the side
// of a retained-handover transfer a barrier is aimed at, and BOTH ends of the
// channel enforce it - `prearm()` below and the product's `bind_target_session`
// (src-tauri/src/ipc/qa_barrier.rs) refuse every other value, including a
// session id or a wildcard. `null` therefore means "this barrier carries no
// targetRole": it is armed, held and released against whatever session the real
// stage runs on (the runner binds that concrete session itself through
// `bindBackendSession` and `<name>.bind.json`), so no role belongs in its arm
// spec.
//
// `producer` is the other vocabulary in this file and a DIFFERENT field: it is
// the component id every receipt must echo (`correlateReceipt`; the product's
// `PRODUCER_ID`), i.e. WHO emitted a line. Mapping it into `targetRole` made
// every pre-armed non-handover barrier throw inside `prearm()` before the
// product was launched, and the product's own binding rule rejects it too.
export const BARRIER_ROLES = Object.freeze({
  // Non-handover stages: no side to target.
  'backend-write': null,
  'presentation': null,
  'attach-handshake': null,
  'held-rpc': null,
  // Handover sides: the daemon's producers bind these to the session that
  // really exported/adopted it (daemon/qa_producers.rs).
  'predecessor-export': 'predecessor',
  'successor-adopt': 'successor',
  'commit': 'predecessor',
  'abort': 'successor',
});

// Authoritative 7-field attach/presentation tuple connecting visual state to PTY backend:
// (backendSessionId, incarnation, daemonEpoch, frontendSessionId, paneIdentity, bindingKey, attemptGeneration)
export const ATTACH_TUPLE_FIELDS = Object.freeze([
  'backendSessionId',
  'incarnation',
  'daemonEpoch',
  'frontendSessionId',
  'paneIdentity',
  'bindingKey',
  'attemptGeneration',
]);

export function requireSevenTupleReceipt(receipt, expected = {}, label = 'presentation') {
  if (!receipt || typeof receipt !== 'object') {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: receipt is not an object: ${JSON.stringify(receipt)}`);
  }
  const tuple = receipt.attachTuple ?? receipt;
  for (const field of ATTACH_TUPLE_FIELDS) {
    if (tuple?.[field] === undefined || tuple?.[field] === null || tuple?.[field] === '') {
      throw new HarnessError('ASSERTION_FAILURE', `${label}: presentation receipt missing 7-tuple field '${field}': ${JSON.stringify(receipt)}`);
    }
  }

  // Type checks
  if (typeof tuple.backendSessionId !== 'string' || tuple.backendSessionId.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: backendSessionId must be non-empty string: ${JSON.stringify(tuple)}`);
  }
  if (typeof tuple.daemonEpoch !== 'string' || tuple.daemonEpoch.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: daemonEpoch must be non-empty string: ${JSON.stringify(tuple)}`);
  }
  if (typeof tuple.frontendSessionId !== 'string' || tuple.frontendSessionId.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: frontendSessionId must be non-empty string: ${JSON.stringify(tuple)}`);
  }
  if (typeof tuple.paneIdentity !== 'string' || tuple.paneIdentity.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: paneIdentity must be non-empty string: ${JSON.stringify(tuple)}`);
  }
  if (typeof tuple.bindingKey !== 'string' || tuple.bindingKey.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: bindingKey must be non-empty string: ${JSON.stringify(tuple)}`);
  }
  if (typeof tuple.attemptGeneration !== 'number' || !Number.isInteger(tuple.attemptGeneration) || tuple.attemptGeneration < 0) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: attemptGeneration must be non-negative integer: ${JSON.stringify(tuple)}`);
  }
  if (typeof tuple.incarnation !== 'string' || tuple.incarnation.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: incarnation must be non-empty string: ${JSON.stringify(tuple)}`);
  }

  // Expected correlations
  if (expected.backendSessionId !== undefined && tuple.backendSessionId !== expected.backendSessionId) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: backendSessionId ${tuple.backendSessionId} does not match expected backend ${expected.backendSessionId}`);
  }
  if (expected.incarnation !== undefined && tuple.incarnation !== expected.incarnation) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: incarnation ${tuple.incarnation} does not match expected incarnation ${expected.incarnation}`);
  }
  if (expected.daemonEpoch !== undefined && tuple.daemonEpoch !== expected.daemonEpoch) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: daemonEpoch ${tuple.daemonEpoch} does not match expected daemonEpoch ${expected.daemonEpoch}`);
  }
  if (expected.frontendSessionId !== undefined && tuple.frontendSessionId !== expected.frontendSessionId) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: frontendSessionId ${tuple.frontendSessionId} does not match expected frontendSessionId ${expected.frontendSessionId}`);
  }
  if (expected.paneIdentity !== undefined && tuple.paneIdentity !== expected.paneIdentity) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: paneIdentity ${tuple.paneIdentity} does not match expected paneIdentity ${expected.paneIdentity}`);
  }
  if (expected.bindingKey !== undefined && tuple.bindingKey !== expected.bindingKey) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: bindingKey ${tuple.bindingKey} does not match expected bindingKey ${expected.bindingKey}`);
  }
  if (expected.attemptGeneration !== undefined && tuple.attemptGeneration !== expected.attemptGeneration) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: attemptGeneration ${tuple.attemptGeneration} does not match expected attemptGeneration ${expected.attemptGeneration}`);
  }

  // Presented verification: PanePresentationReceipt specifies presented: bool
  if (receipt.presented !== undefined && receipt.presented !== true) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: receipt presented flag is false: ${JSON.stringify(receipt)}`);
  }

  return tuple;
}

// Backwards compatibility alias
export const requireFiveTupleReceipt = requireSevenTupleReceipt;

// Monotonic elapsed clock tracker ensuring all waits consume remaining budget.
export class MonotonicBudget {
  constructor(totalMs = BUDGETS.attemptCeilingMs) {
    this.totalMs = totalMs;
    this.startAt = Date.now();
    this.deadlineAt = this.startAt + totalMs;
  }

  remainingMs(stageCapMs = Infinity) {
    const remaining = Math.max(0, this.deadlineAt - Date.now());
    return Math.min(remaining, stageCapMs);
  }

  consume(stageCapMs = Infinity, label = 'operation') {
    const rem = this.remainingMs(stageCapMs);
    if (rem <= 0) {
      throw new HarnessError('ASSERTION_FAILURE', `monotonic budget exhausted for ${label}: elapsed ${this.elapsedMs()}ms >= limit ${this.totalMs}ms`);
    }
    return rem;
  }

  elapsedMs() {
    return Date.now() - this.startAt;
  }

  isExceeded() {
    return Date.now() > this.deadlineAt;
  }

  // Pass-22 audit F2-11 (residual): give the window back the wall clock a
  // MEASUREMENT spent inside it, so the window bounds the SCENARIO's own time
  // rather than the harness's own observability. Additive and opt-in: a budget
  // nobody extends behaves exactly as it did before.
  extendBy(ms) {
    const extra = Number.isFinite(ms) && ms > 0 ? ms : 0;
    this.deadlineAt += extra;
    return this.deadlineAt;
  }
}

// The wall clock the measurements inside the measured attempt window spent
// (pass-22 audit F2-11 residual). That window is the scenario's own correctness
// budget, so this clock hands its time back to the window as it is spent: a stage
// cap (`remainingMs`) and the correctness ceiling (`isExceeded`, and the runner's
// `attemptBudgetAccounting`) then both bound the scenario, not the measurement.
//
// Nothing is relaxed and nothing moved: the reads stay inside the window at the
// same moments, each read's cost is still bounded per read (`charge.boundMs`) and
// reported per read (the action's `elapsedMs`), and the total handed back is
// therefore bounded by those same caps. A clock with no budget attached still
// accumulates, so the accounting can be read without one.
export class InstrumentationClock {
  constructor({ budget = null } = {}) {
    this.budget = budget;
    this.total = 0;
    this.samples = [];
  }

  note(label, ms) {
    const spent = Number.isFinite(ms) && ms > 0 ? Math.floor(ms) : 0;
    this.total += spent;
    this.samples.push({ label, ms: spent });
    this.budget?.extendBy?.(spent);
    return spent;
  }

  totalMs() {
    return this.total;
  }
}

// Scenario-specific fixture requirements: basic split scenarios only need source/target,
// whereas retained/handover/suspension cases require their specific control fixtures.
// Review Task 7: Never require all four fixtures for basic split smoke.
export const SCENARIO_FIXTURE_REQUIREMENTS = Object.freeze({
  'diagnostic-classifier': Object.freeze(['created', 'idle']),
  'split-happy': Object.freeze(['source']),
  'split-attach-stall': Object.freeze(['source']),
  'split-cancel': Object.freeze(['source']),
  'split-concurrent': Object.freeze(['source']),
  // `retained-handover` and `handover-abort` do NOT require an `adopted` fixture.
  // Adoption is the scenario's own OUTCOME, not a session that can exist before the
  // scenario runs: `fixture-setup` settles at boot, before any trigger, and the
  // adoption only happens when the runner's `trigger-handover` drives the real
  // upgrade (`ipc/terminal.rs::run_handover_watcher` -> `upgrade_binary`). Requiring
  // the kind at fixture time was therefore unsatisfiable by construction, which is
  // exactly the measured `fixture-setup requires at least one adopted session, got
  // 0`. What the scenario must prove is already asserted from the transfer receipt
  // itself (`handoverPreservesIncarnation` + `singleReader` over
  // `handover-transfer`), which carries `adoptedBackendSessionId`,
  // `adoptedIncarnation` and the live adopted reader. So the fixture these two need
  // is the session that gets handed over - `created`.
  'retained-handover': Object.freeze(['created']),
  'handover-abort': Object.freeze(['created']),
  'suspension-ownership': Object.freeze(['created', 'externally-stopped', 'idle']),
  'stale-binding': Object.freeze(['source']),
});

// The single derivation point for "which fixture kinds does this scenario
// need": `validateFixtureSetup` below (what the settled `fixture-setup`
// receipt must contain) and the isolated launch env's `FERRYX_QA_FIXTURE_KINDS`
// (what the product is told to provision) both read this helper, so the
// declaration sent to the product and the validation applied to its receipt can
// never drift. The `['source']` default is the default `validateFixtureSetup`
// has always applied to a scenario with no entry in the map.
export function fixtureKindsForScenario(scenario) {
  return [...(SCENARIO_FIXTURE_REQUIREMENTS[scenario] ?? ['source'])];
}

// Comma-separated, lower-case kind names: the frozen wire spelling of
// `FERRYX_QA_FIXTURE_KINDS` (the product lane consumes exactly this form).
export function fixtureKindsEnvValue(scenario) {
  return fixtureKindsForScenario(scenario).join(',');
}

export function validateFixtureSetup(fixture, scenario) {
  const requiredKinds = fixtureKindsForScenario(scenario);
  const sessions = Array.isArray(fixture?.sessions) ? fixture.sessions : [];
  const byKind = new Map();
  for (const session of sessions) {
    if (!session || typeof session.backendSessionId !== 'string' || session.backendSessionId.length === 0
      || typeof session.ownershipReceipt !== 'object' || session.ownershipReceipt === null) {
      throw new HarnessError('ASSERTION_FAILURE', `fixture-setup session lacks backendSessionId/ownershipReceipt: ${JSON.stringify(session)}`);
    }
    const kind = session.kind ?? 'source';
    byKind.set(kind, [...(byKind.get(kind) ?? []), session]);
  }

  for (const kind of requiredKinds) {
    const entries = byKind.get(kind) ?? [];
    if (entries.length < 1) {
      throw new HarnessError('ASSERTION_FAILURE', `scenario ${scenario}: fixture-setup requires at least one ${kind} session, got ${entries.length}`);
    }
  }

  if (requiredKinds.includes('externally-stopped')) {
    const stopped = byKind.get('externally-stopped')?.[0];
    if (typeof stopped?.stopProbeState !== 'string' || stopped.stopProbeState !== 'stopped') {
      throw new HarnessError('ASSERTION_FAILURE', `externally-stopped fixture lacks stop evidence: ${JSON.stringify(stopped)}`);
    }
  }

  return { sessions, byKind };
}

// Timing contract from the master plan Verification strategy.
export const BUDGETS = Object.freeze({
  attemptCeilingMs: 15_000,
  warmTargetMs: 2_000,
  stagePrepareCreateStatusMs: 9_000,
  stageAttachListenerMs: 4_000,
  stagePresentationMs: 2_000,
  cancelAckCeilingMs: 3_000,
  cancelDaemonResponseBudgetMs: 2_500,
  cleanupVerifyTimeoutMs: 10_000,
  barrierAckTimeoutMs: 5_000,
  // Windows interactive-desktop lane (pass-4 blockers NO_OWNED_WINDOW /
  // SPLIT_RIGHT_NOT_UNIQUE): bounded probe/wait budgets, all inside the
  // attempt ceiling or the outer launcher's own bound.
  windowsSessionProbeMs: 5_000,
  ownedWindowReadyMs: 8_000,
  splitFocusWaitMs: 4_000,
  // Pass-18/19 (task-9 win-pass17 + the 12-probe burst measurement): the
  // delegation is INTERMITTENT and BURSTY. 12 minimal probes (a bat whose first
  // line writes a marker, 10s window each) measured p1-p8 STALLED - 8
  // CONSECUTIVE, ~73s in total - and p9-p12 SUCCEEDED, 4 consecutive, 502-516ms
  // each. A 6-attempt retry with fresh roots, dirs and task names landed
  // entirely inside one burst and got nowhere, so the budget has to OUTLAST A
  // BURST, not merely beat a rate:
  //   * the whole retry SEQUENCE carries the budget (`interactiveRelaunchSequenceMs`),
  //     and an individual attempt gets only the stall window plus whatever is
  //     left of that sequence - one attempt can never spend it all;
  //   * the stall window is 10s: ~20x the measured SUCCESS latency (0.380s in
  //     pass-17 r1, 0.502-0.516s in the probes), and never below 5s;
  //   * 10s x 18 = 180s, the same 180s that used to be spent on ONE stalled
  //     attempt. A stall now costs 10s instead of 3 minutes, and the sequence
  //     outlasts the ~73s burst that was measured. If 180s of retries inside one
  //     burst still cannot get through, that is the measurement that decides
  //     whether anything heavier is justified - the condition lifts on its own.
  // Pass-19 measured exactly that: 15 attempts stalled back-to-back for ~153s
  // and the burst was STILL GOING when the 180s sequence ran out (pass 18 saw
  // ~73s), so 180s does not get through on this host at this time. The lever
  // that costs only wall-clock is a longer sequence, so the budget is raised to
  // 15 minutes (10s x 90) - still bounded, still fail-closed, still retried only
  // for a stall (a deterministic failure returns on the FIRST attempt). The
  // condition demonstrably lifts on its own (pass 18: 8 stalls then 4 clean
  // successes at ~0.5s; pass-17 r1 completed the whole chain), so waiting it out
  // is cheaper than anything heavier. Re-measure this number if a burst ever
  // exceeds the sequence: a run that exhausts 15 minutes is the evidence that
  // the host state itself needs resetting rather than retried.
  interactiveRelaunchSequenceMs: 900_000,
  interactiveRelaunchEntryMarkerMs: 10_000,
  interactiveRelaunchAttempts: 90,
  // Pass-6 cleanup defect (E/task-9/REPORT-PASS6.md §5): the app's own
  // `--daemon` descendant outlived `taskkill /T` (no `/F`), which kept the
  // isolation root held AND kept the stdio pipe it inherited open, so the
  // runner's node process never exited after printing its result. Teardown now
  // escalates to a forced tree/group kill, then retries the root removal inside
  // these bounds, then reports the holder when the root is still held.
  cleanupGraceMs: 400,
  cleanupRootRetryMs: 6_000,
  cleanupRootRetryIntervalMs: 250,
  cleanupRootRetryAttempts: 8,
  cleanupHolderProbeMs: 8_000,
  // Task-9 root cause 1: the debug binary boots against `devUrl`
  // (`http://127.0.0.1:5173`) and renders Chromium's ERR_CONNECTION_REFUSED
  // page when nothing serves it (pass-7 measured exactly 29 nodes of that error
  // page). Serving is pre-launch setup: its waits are bounded here and never
  // charged to `attemptCeilingMs`, which stays the trigger->settlement ceiling.
  frontendPortProbeMs: 2_000,
  frontendRequestMs: 4_000,
  frontendReadyMs: 8_000,
  frontendCloseMs: 2_000,
  // Task-9 root cause 2: with the UI served the app boots EMPTY ("No open
  // tabs"), so no pane - and therefore no pane toolbar and no split affordance
  // - exists. The pre-trigger UI pane step (click "New Terminal", then bind the
  // pane session the app itself presents) gets its own bounded budget for the
  // same reason: it is setup, not the measured attempt.
  paneAffordanceWaitMs: 8_000,
  // Chromium/WebView2 builds its accessibility tree LAZILY, triggered by the
  // FIRST UIA client attaching, and the tree is not ready at the instant of that
  // attach: task-9 pass 8 measured the first enumeration of the app's tree at
  // 16 elements / 2 named (all Chromium-internal, no `Document`/`RootWebArea`)
  // and the next enumeration at 93 elements / 56 named including
  // `New Terminal` (`task-9/win-pass8/activation/activation-probe.json`). Every
  // probe that can be a run's first UIA client therefore warms the tree with one
  // cheap enumeration and repeats its real query on this bounded interval within
  // this bounded budget before it decides. Both are charged to the pre-trigger
  // `setupCeilingMs` (the pane step and the split probe's own warm-up), never to
  // the frozen `attemptCeilingMs` correctness ceiling.
  uiaWarmBudgetMs: 4_000,
  uiaWarmRetryIntervalMs: 100,
  // Pass-9 measured the requirement directly (four fresh launches, first attach
  // delayed 1/3/6/12 s): the tree is NEVER built at the attach - exactly 16
  // Chromium-internal elements, 2 named, at every delay - and flips 317-353 ms
  // later, a 36 ms spread that is independent of launch-relative time
  // (`task-9/latency-probe.json`). Re-issuing the attach is what drives the
  // build, so the warm ATTACH loop gets this dedicated ceiling (~1.5x the worst
  // measurement) while the name-search budget above stays at the 4 s it already
  // had: the loop exits on the first observation that leaves the baseline, so
  // the budget is only ever reached when the tree really never activates.
  uiaWarmAttachBudgetMs: 500,
  uiaWarmAttachIntervalMs: 100,
  paneBindingReadyMs: 8_000,
  // Task-9 pass-13: the pre-split pane's session identity must come from an
  // honest producer even when the native surface never attaches - the product's
  // presentation producer requires the seven-field attachTuple and
  // `pane_liveness_presentation_receipt` returns None (emitting nothing at all)
  // without it, and three of those fields are frontend-owned identities the
  // daemon has no concept of, so nothing may be synthesised. The second source
  // is the isolated daemon's own session inventory, read over the control wire
  // (`handshake` + `listSessions` only) before and after the pane click. Every
  // blocking step of that read is bounded here - connect, per-read, and a total
  // deadline - so a hung or wedged daemon yields a typed failure instead of a
  // hang, and the PANE step's reads (setup, before the trigger) are charged to
  // the pre-trigger `setupCeilingMs`. The SPLIT step's own reads sit INSIDE the
  // measured attempt window instead, so they are charged there, under the small
  // bounded cap below - never by widening the ceiling.
  daemonInventoryConnectMs: 1_500,
  daemonInventoryReadMs: 2_000,
  daemonInventoryTotalMs: 3_000,
  // Pass-22 audit D2: the split step's pre-click and post-click inventory reads
  // happen INSIDE the measured attempt window (`attemptCeilingMs`), so their wall
  // clock is charged to that window explicitly, capped here per read. This is NOT
  // a relaxation of anything: the cap only bounds how much of the attempt window
  // a MEASUREMENT may spend, and a read the window cannot pay for is refused with
  // a typed evidence action instead of being taken. 1s is ~7% of the 15s ceiling,
  // and it bounds a `handshake` + `listSessions` round trip on a loopback
  // transport.
  splitInventoryReadCapMs: 1_000,
  setupCeilingMs: 45_000,
});

// Exit codes: 0 is reserved for a truthful native PASS (or a completed,
// explicitly deferred headless check run whose result.json records
// nativeEvidence: deferred-to-task-10 - never a native PASS).
export const EXIT = Object.freeze({
  scenarioFailure: 1,
  invalidInvocation: 2,
  nativeDeferredHeadless: 0,
  nativeAutomationUnsupported: 4,
  axUntrusted: 5,
  captureDenied: 6,
  barrierUnsupported: 7,
  markerRecognitionUnverified: 8,
});

export const TYPED_ERRORS = Object.freeze(new Set([
  'INVALID_SCENARIO', 'INVALID_FLAG', 'MISSING_FLAG', 'DUPLICATE_FLAG',
  'HEADLESS_NOT_PERMITTED', 'PATH_NOT_ABSOLUTE', 'PATH_EMPTY', 'BINARY_MISSING',
  'BINARY_NOT_EXECUTABLE', 'BINARY_IS_PRODUCTION_INSTALL', 'PREEXISTING_ISOLATION_ROOT',
  'PRODUCTION_RUNTIME_ROOT', 'UNSAFE_EVIDENCE_DIR', 'AX_UNTRUSTED', 'CAPTURE_DENIED',
  'NATIVE_AUTOMATION_UNSUPPORTED', 'BARRIER_ACK_TIMEOUT', 'ASSERTION_FAILURE',
  'MARKER_RECOGNITION_UNVERIFIED',
  'RECOVERY_UNPROVEN',
  // Windows interactive-desktop lane: each condition keeps its own typed
  // identity so a blocked run says exactly which window/session/selector
  // condition blocked it instead of collapsing into NATIVE_AUTOMATION_UNSUPPORTED.
  'NO_INTERACTIVE_SESSION', 'NO_OWNED_WINDOW', 'INTERACTIVE_RELAUNCH_FAILED',
  // Pass-18: the delegation is intermittent (the task's cmd.exe is created but
  // never runs its first line), so every attempt is detected and retried a
  // bounded number of times. All of them stalling is its own typed condition -
  // nonzero, fail-closed, never a pass and never a session-0 fallback.
  'DELEGATION_STALLED',
  'SPLIT_RIGHT_NOT_FOUND', 'SPLIT_RIGHT_NOT_UNIQUE', 'SPLIT_RIGHT_DISABLED',
  // Frontend-serve lane (task-9 root cause 1). Each condition keeps its own
  // typed identity so a run says exactly which part of "something must serve
  // the dev URL" failed - and a port already held by a foreign listener is
  // refused, never killed and never reused.
  'FRONTEND_DIST_MISSING', 'FRONTEND_PORT_OCCUPIED', 'FRONTEND_NOT_SERVED',
  'FRONTEND_DEVURL_MISMATCH',
  // Pane-binding lane (task-9 root cause 2). The app boots to its empty state,
  // so the split affordance has no pane to live in until one is created through
  // the UI and bound to the session the app really presents.
  'PANE_AFFORDANCE_NOT_FOUND', 'PANE_AFFORDANCE_NOT_UNIQUE', 'PANE_AFFORDANCE_DISABLED',
  'PANE_BINDING_UNBOUND', 'PANE_BINDING_AMBIGUOUS',
  // The marker was typed into a pane whose session never reported it: the pane
  // under test produced no output, so the scenario's own marker assertion is
  // measured as a product failure instead of riding another pane's receipt.
  'MARKER_SESSION_UNBOUND',
]));

export class HarnessError extends Error {
  constructor(code, detail) {
    super(`${code}: ${detail ?? ''}`);
    this.code = TYPED_ERRORS.has(code) ? code : 'ASSERTION_FAILURE';
    this.detail = detail ?? '';
  }
}

function rejectFlaggedArgs(argv) {
  const allowed = new Set(['scenario', 'binary', 'evidence-dir', 'isolation-root', 'headless']);
  const seen = new Map();
  const values = {};
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (!token.startsWith('--')) throw new HarnessError('INVALID_FLAG', `positional/unrecognized token ${token}`);
    const body = token.slice(2);
    const eq = body.indexOf('=');
    const name = eq === -1 ? body : body.slice(0, eq);
    if (!allowed.has(name)) throw new HarnessError('INVALID_FLAG', `unrecognized flag --${name}`);
    if (name === 'headless') {
      if (eq !== -1) throw new HarnessError('INVALID_FLAG', '--headless takes no value');
      if (seen.has(name)) throw new HarnessError('DUPLICATE_FLAG', '--headless');
      seen.set(name, true);
      values.headless = true;
      continue;
    }
    if (seen.has(name)) throw new HarnessError('DUPLICATE_FLAG', `--${name}`);
    let value = eq === -1 ? null : body.slice(eq + 1);
    if (value === null) {
      if (i + 1 >= argv.length) throw new HarnessError('MISSING_FLAG', `--${name} requires a value`);
      value = argv[i + 1];
      i += 1;
    }
    seen.set(name, true);
    values[name] = value;
  }
  for (const required of ['scenario', 'binary', 'evidence-dir', 'isolation-root']) {
    if (!seen.has(required)) throw new HarnessError('MISSING_FLAG', `--${required} is required`);
  }
  return values;
}

export function parseInvocation(argv) {
  const values = rejectFlaggedArgs(argv);
  const scenario = values.scenario;
  if (!SCENARIOS.includes(scenario)) throw new HarnessError('INVALID_SCENARIO', `unknown scenario ${scenario}`);
  const headless = values.headless === true;
  if (headless && !HEADLESS_ELIGIBLE.includes(scenario)) {
    throw new HarnessError('HEADLESS_NOT_PERMITTED', `--headless is permitted only for ${HEADLESS_ELIGIBLE.join(', ')} (plan Task 3 deferral rule)`);
  }
  return { scenario, headless, binary: values.binary, evidenceDir: values['evidence-dir'], isolationRoot: values['isolation-root'] };
}

function isProductionRuntimeRoot(candidate) {
  const p = resolve(candidate);
  const uid = process.getuid?.() ?? 0;
  const productionRoots = [
    `/tmp/rorca-${uid}`, `/tmp/rorca-${uid}-dev`, '/app/rorca', '/app/rorca-dev',
    join(homedir(), '.rorca'),
  ];
  return productionRoots.some(root => p === root || p.startsWith(`${root}/`) || p.startsWith(`${root}\\`));
}

function assertAbsolute(label, value) {
  if (!value || typeof value !== 'string' || value.includes('\0')) throw new HarnessError('PATH_EMPTY', `${label} must be a non-empty NUL-free path`);
  if (!isAbsolute(value)) throw new HarnessError('PATH_NOT_ABSOLUTE', `${label} must be absolute: ${value}`);
}

export function preflight({ scenario, headless, binary, evidenceDir, isolationRoot }) {
  assertAbsolute('--binary', binary);
  assertAbsolute('--evidence-dir', evidenceDir);
  assertAbsolute('--isolation-root', isolationRoot);
  if (isProductionRuntimeRoot(isolationRoot)) throw new HarnessError('PRODUCTION_RUNTIME_ROOT', `isolation root ${isolationRoot} collides with the production/default runtime`);
  if (isProductionRuntimeRoot(evidenceDir)) throw new HarnessError('PRODUCTION_RUNTIME_ROOT', `evidence dir ${evidenceDir} collides with the production/default runtime`);
  // Binary must be a real executable file and must not be the installed
  // production application bundle.
  let binaryStat;
  try { binaryStat = statSync(binary); } catch { throw new HarnessError('BINARY_MISSING', `binary not found: ${binary}`); }
  if (!binaryStat.isFile()) throw new HarnessError('BINARY_MISSING', `binary is not a regular file: ${binary}`);
  try { accessSync(binary, constants.X_OK); } catch { throw new HarnessError('BINARY_NOT_EXECUTABLE', `binary is not executable: ${binary}`); }
  const resolvedBinary = realpathish(binary);
  if (process.platform === 'darwin' && resolvedBinary.startsWith('/Applications/')) {
    throw new HarnessError('BINARY_IS_PRODUCTION_INSTALL', `refusing production install binary ${resolvedBinary}`);
  }
  // Review L2: Windows production install locations are refused as well.
  const normalizedWinPath = resolvedBinary.replace(/\\/g, '/');
  if (process.platform === 'win32' && (normalizedWinPath.includes('/Program Files/') || normalizedWinPath.includes('/WindowsApps/'))) {
    throw new HarnessError('BINARY_IS_PRODUCTION_INSTALL', `refusing production install binary ${resolvedBinary}`);
  }
  if (existsSync(isolationRoot)) throw new HarnessError('PREEXISTING_ISOLATION_ROOT', `isolation root already exists (runner requires a fresh task-owned root): ${isolationRoot}`);
  // Evidence dir: absolute, may exist; walk components creating 0700, refuse symlink components.
  const resolvedEvidence = resolve(evidenceDir);
  const { root: evidenceRoot } = parse(resolvedEvidence);
  let cursor = evidenceRoot;
  const relativeEvidence = resolvedEvidence.slice(evidenceRoot.length);
  const segments = relativeEvidence.split(/[\\/]+/).filter(Boolean);
  for (const part of segments) {
    cursor = join(cursor, part);
    try { mkdirSync(cursor, { mode: 0o700 }); } catch (error) { if (error.code !== 'EEXIST') throw error; }
    const st = lstatSync(cursor);
    if (st.isSymbolicLink()) {
      const isDarwinSystemSymlink = process.platform === 'darwin' && (cursor === '/tmp' || cursor === '/var' || cursor === '/etc');
      if (!isDarwinSystemSymlink) {
        throw new HarnessError('UNSAFE_EVIDENCE_DIR', `symlink component in evidence path: ${cursor}`);
      }
    }
  }
  return {
    scenario, headless, binary: resolvedBinary,
    binarySha256: sha256File(resolvedBinary),
    evidenceDir: resolve(evidenceDir), isolationRoot: resolve(isolationRoot),
    platform: platform(), release: release(), arch: arch(), host: process.env.HOSTNAME ?? null,
    argv: [process.execPath, fileURLToPath(new URL('../../qa/pane-liveness.mjs', import.meta.url))],
  };
}

function realpathish(p) {
  try { return realpathSync(p); } catch { return resolve(p); }
}

function sha256File(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

// ---------------------------------------------------------------------------
// Owned-process identity, bounded process-table read and forced reap.
//
// The pass-6 cleanup defect (E/task-9/REPORT-PASS6.md §5) has ONE cause and two
// symptoms: the app's own `--daemon` descendant outlives `taskkill /T` (no
// `/F`), so (a) the isolation root stays held (`directoriesRemoved: false`,
// `cleanupGate.ok: false`) and (b) the stdio pipe that descendant inherited
// stays open, so the runner's node process never exits after printing its
// result. Killing that descendant by exact PID makes the root removable
// (`ISO_REMOVED_AFTER_KILL=True`, measured before any kill).
//
// Everything here is exact-PID scoped and identity-verified, never a name or
// command-line pattern (the verifier's own pass-6 `CommandLine -like
// '*pane-liveness.mjs*'` cleanup matched every runner on the host - that is the
// anti-pattern):
//   * the signalled set is the PIDs THIS runner spawned through its own child
//     handles, plus their process trees/groups;
//   * a descendant discovered through the process table is signalled only when
//     its identity is positively verified (its executable is the binary this
//     run launched, or it lives inside this run's own isolation root);
//   * a descendant whose identity is positively NOT ours is reported and never
//     signalled.

export function normalizeProcessPath(value, platform = process.platform) {
  if (typeof value !== 'string' || value.length === 0) return null;
  const slashed = value.replace(/\\/g, '/').replace(/\/+$/, '');
  return platform === 'win32' ? slashed.toLowerCase() : slashed;
}

function isAbsoluteProcessPath(value) {
  return typeof value === 'string' && (value.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(value));
}

export function isPathInside(childPath, parentPath, platform = process.platform) {
  const child = normalizeProcessPath(childPath, platform);
  const parent = normalizeProcessPath(parentPath, platform);
  if (!child || !parent) return false;
  return child === parent || child.startsWith(`${parent}/`);
}

// Identity verdict for one process-table row: `true` is positive proof that the
// row belongs to this run, `false` is positive proof that it does not (such a
// row is never signalled), `null` means this platform exposed no executable
// path and the caller must fall back to handle/descendant scoping.
export function verifyProcessIdentity(row, { executable = null, roots = [], platform = process.platform } = {}) {
  const observed = normalizeProcessPath(row?.executable, platform);
  if (!observed) return { verified: null, reason: 'executable path is not observable on this platform' };
  const wanted = isAbsoluteProcessPath(executable) ? normalizeProcessPath(executable, platform) : null;
  if (wanted && observed === wanted) return { verified: true, reason: 'executable is the binary this run launched' };
  const ownedRoots = roots.filter(root => isAbsoluteProcessPath(root));
  if (ownedRoots.some(root => isPathInside(observed, root, platform))) {
    return { verified: true, reason: "executable lives inside this run's own isolation root" };
  }
  if (!wanted && ownedRoots.length === 0) {
    return { verified: null, reason: 'no owned executable or isolation root was recorded to compare against' };
  }
  return { verified: false, reason: `executable ${observed} is neither the launched binary nor inside this run's isolation root` };
}

// Breadth-first walk of ONE process-table read, starting from the exact PIDs
// this run spawned. Parent links are the scoping proof: a foreign runner's tree
// is rooted at a different PID and can never appear here.
export function descendantsOf(rows, rootPids) {
  const byParent = new Map();
  for (const row of rows ?? []) {
    if (!row || row.pid === null || row.pid === undefined) continue;
    const key = String(row.ppid);
    byParent.set(key, [...(byParent.get(key) ?? []), row]);
  }
  const seen = new Set((rootPids ?? []).map(String));
  const queue = [...seen];
  const descendants = [];
  while (queue.length > 0) {
    for (const row of byParent.get(queue.shift()) ?? []) {
      const key = String(row.pid);
      if (seen.has(key)) continue;
      seen.add(key);
      descendants.push(row);
      queue.push(key);
    }
  }
  return descendants;
}

export function isProcessAlive(pid) {
  try { process.kill(pid, 0); return true; } catch { return false; }
}

// Windows process table: exact fields from CIM in one bounded read, with no
// name or command-line matching anywhere. Newline-joined statements (the
// pass-5 here-string rule).
export function buildProcessTableScript() {
  return [
    "$ErrorActionPreference = 'SilentlyContinue';",
    '$rows = @();',
    'try { $rows = @(Get-CimInstance Win32_Process | ForEach-Object { [ordered]@{ pid = [int]$_.ProcessId; ppid = [int]$_.ParentProcessId; executable = [string]$_.ExecutablePath } }) } catch { $rows = @() };',
    'Write-Output (ConvertTo-Json -InputObject @($rows) -Compress -Depth 4);',
  ].join('\n');
}

// PowerShell 5.1 unwraps single-element arrays and prints nothing for an empty
// one; normalize at this system boundary instead of trusting the shape.
export function parseProcessTableJson(stdout) {
  const lines = String(stdout ?? '').split(/\r?\n/).map(line => line.trim()).filter(Boolean);
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    let parsed;
    try { parsed = JSON.parse(lines[index]); } catch { continue; }
    const rows = Array.isArray(parsed) ? parsed : [parsed];
    return rows
      .filter(row => row && Number.isFinite(Number(row.pid)))
      .map(row => ({ pid: Number(row.pid), ppid: Number(row.ppid), executable: row.executable || null }));
  }
  return [];
}

// POSIX process table (`ps -Ao pid=,ppid=,command=`): argv[0] counts as an
// executable only when it is absolute; on Linux the caller replaces it with the
// kernel's own /proc/<pid>/exe, which is authoritative.
export function parsePosixProcessTable(stdout) {
  const rows = [];
  for (const line of String(stdout ?? '').split('\n')) {
    const match = /^\s*(\d+)\s+(\d+)\s+(.*)$/.exec(line);
    if (!match) continue;
    const argv0 = match[3].trim().split(/\s+/)[0] ?? '';
    rows.push({ pid: Number(match[1]), ppid: Number(match[2]), executable: isAbsoluteProcessPath(argv0) ? argv0 : null });
  }
  return rows;
}

async function probeOutput(file, args, timeoutMs) {
  const child = spawn(file, args, { stdio: ['ignore', 'pipe', 'pipe'] });
  // A probe that cannot even be spawned (ENOENT) or that dies must resolve as
  // "no data": an unhandled ChildProcess 'error' event would otherwise crash the
  // runner's teardown instead of degrading the identity evidence.
  const unavailable = new Promise(resolvePromise => { child.once('error', () => resolvePromise({ stdout: '' })); });
  const outcome = await withDeadline(Promise.race([collectOutput(child), unavailable]), timeoutMs, `process-table probe (${file})`);
  if (outcome.timedOut || outcome.error) {
    try { child.kill('SIGKILL'); } catch { /* the deadline verdict is already decided */ }
    return { timedOut: true, stdout: '' };
  }
  return { timedOut: false, stdout: outcome.value?.stdout ?? '' };
}

// One bounded process-table read. An unanswerable probe returns []: that
// degrades identity evidence, it never turns a cleanup failure into a pass.
export async function defaultProcessSnapshot({ timeoutMs = BUDGETS.cleanupHolderProbeMs, platform = process.platform } = {}) {
  if (platform === 'win32') {
    const probe = await probeOutput('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', buildProcessTableScript()], timeoutMs);
    return probe.timedOut ? [] : parseProcessTableJson(probe.stdout);
  }
  const probe = await probeOutput('ps', ['-Ao', 'pid=,ppid=,command='], timeoutMs);
  if (probe.timedOut) return [];
  const rows = parsePosixProcessTable(probe.stdout);
  if (existsSync('/proc/self/exe')) {
    for (const row of rows) {
      try { row.executable = readlinkSync(`/proc/${row.pid}/exe`); } catch { /* keep argv[0] */ }
    }
  }
  return rows;
}

function defaultRunKill(file, args) {
  try {
    const child = spawn(file, args, { stdio: 'ignore' });
    // A kill helper that cannot be spawned must not raise an unhandled 'error'
    // event: the target's own 'exit' event decides the receipt either way.
    child.on('error', () => { /* the exit event decides */ });
  } catch { /* the exit event decides */ }
}

function defaultSignalProcess(pid, signal) { return process.kill(pid, signal); }

function defaultSleep(ms) { return new Promise(resolvePromise => setTimeout(resolvePromise, ms)); }

// Bounded wait for one child's own 'exit' event. The timer is cleared the moment
// the event lands, so a fast exit never leaves a pending handle behind.
function awaitExitWithin(child, send, timeoutMs) {
  return new Promise(resolvePromise => {
    let settled = false;
    const finish = value => { if (settled) return; settled = true; clearTimeout(timer); resolvePromise(value); };
    const timer = setTimeout(() => finish(false), timeoutMs);
    child.once('exit', () => finish(true));
    // A child that exited between the caller's check and this listener never
    // emits again: report the recorded state instead of burning the deadline.
    if (child.exitCode !== null || child.signalCode !== null) finish(true);
    else { try { send(); } catch { /* the exit event decides */ } }
  });
}

function sendGracefulKill({ platform, pid, child, runKill, signalProcess }) {
  if (platform === 'win32') { runKill('taskkill', ['/T', '/PID', String(pid)]); return; }
  try { signalProcess(-pid, 'SIGTERM'); } catch { try { child.kill('SIGTERM'); } catch { /* exit event decides */ } }
}

function sendForcedKill({ platform, pid, child, runKill, signalProcess }) {
  if (platform === 'win32') { runKill('taskkill', ['/T', '/F', '/PID', String(pid)]); return; }
  try { signalProcess(-pid, 'SIGKILL'); } catch { try { child.kill('SIGKILL'); } catch { /* exit event decides */ } }
}

function forceKillExactPid(pid, { platform, runKill, signalProcess }) {
  if (platform === 'win32') { runKill('taskkill', ['/F', '/PID', String(pid)]); return; }
  try { signalProcess(pid, 'SIGKILL'); } catch { /* already gone */ }
}

// Bounded retry of a removal that can fail while a force-reaped descendant is
// still releasing its handles (Windows) or its cwd (POSIX). Bounded by attempts
// AND a deadline; `removed` is always re-derived from the filesystem, so a held
// root can never be reported as removed.
export async function removePathWithRetry(target, {
  attempts = BUDGETS.cleanupRootRetryAttempts,
  intervalMs = BUDGETS.cleanupRootRetryIntervalMs,
  deadlineMs = BUDGETS.cleanupRootRetryMs,
  remove = path => rmSync(path, { recursive: true, force: true }),
  exists = path => existsSync(path),
  sleep = defaultSleep,
  now = () => Date.now(),
} = {}) {
  const startedAt = now();
  let used = 0;
  let removed = false;
  while (used < attempts) {
    used += 1;
    try { remove(target); } catch { /* a held handle is reported below, never swallowed */ }
    if (!exists(target)) { removed = true; break; }
    if (used >= attempts) break;
    if (now() - startedAt + intervalMs > deadlineMs) break;
    await sleep(intervalMs);
  }
  return { removed, attempts: used };
}

// Who still holds an isolation root? Reported, never killed. Candidates are
// limited to this run's own spawned PIDs and their descendants, each carrying
// its identity verdict, so the runner can name the holder (pass-6: the app's own
// `--daemon` descendant) without a host-wide name search.
export async function findIsolationRootHolders(root, {
  ownedPids = [], executable = null, roots = [], snapshot = defaultProcessSnapshot,
  alive = isProcessAlive, platform = process.platform,
} = {}) {
  const rows = await snapshot({ timeoutMs: BUDGETS.cleanupHolderProbeMs });
  const holders = [];
  for (const pid of ownedPids) {
    if (!alive(pid)) continue;
    holders.push({ pid, ppid: null, executable: executable ?? null, identityVerified: true, identity: 'runner-spawned-child', holderOf: root });
  }
  for (const row of descendantsOf(rows, ownedPids)) {
    if (!alive(row.pid)) continue;
    const verdict = verifyProcessIdentity(row, { executable, roots: [root, ...roots], platform });
    holders.push({ pid: row.pid, ppid: row.ppid ?? null, executable: row.executable ?? null, identityVerified: verdict.verified, identity: verdict.reason, holderOf: root });
  }
  return holders;
}

// ---------------------------------------------------------------------------
// App stdio sink: the app's own stdout/stderr, drained into per-run artifacts.
//
// `spawnOwned` pipes the child's stdout and stderr (`stdio: ['ignore','pipe',
// 'pipe']`) and NOTHING drained them, so the app's own lines - above all
// `[cmd_terminal_spawn] stage=... failed code=...` - reached no artifact at all:
// the verifier measured `runner.err` at ~2 KB with ZERO `cmd_terminal_spawn`
// hits, and could only localise the real cause by patching the STAGED
// `spawnOwned` with a verifier-only drain gated on `FERRYX_VERIFIER_APP_STDERR` -
// a patch that is not in the candidate. The sink is therefore ALWAYS ON in the
// runner (the entrypoint injects it into every spawn it makes); it is never
// behind an env var a later pass must remember to set.
//
// Three properties this class exists to guarantee:
//   * the child is NEVER blocked by back-pressure: every chunk is consumed as it
//     arrives and streamed straight to disk, and the bytes held in this process
//     are bounded by the write stream's own buffer, never by the run's output;
//   * nothing is dropped silently: each file is capped (APP_STDIO_MAX_BYTES) and
//     the cap is recorded IN the file as an explicit marker, with the dropped
//     byte count carried in the receipt;
//   * teardown closes both files through the same `registry.cleanup()` pass as
//     every other resource and reports what it really observed;
//   * a file that WAS closed cleanly says so in its own bytes, and the marker
//     is written before the stream ends, so a run killed mid-flight leaves a
//     file that cannot be mistaken for a complete drain (pass-18).

export const APP_STDIO_MAX_BYTES = 4 * 1024 * 1024;

// The marker written into a file the moment its cap is reached, so a truncated
// artifact says so in its own bytes instead of ending mid-line with no warning.
export const APP_STDIO_TRUNCATION_MARKER = '[app-stdio truncated:';

// The marker written into a file by `close()` AFTER every byte has been queued
// and BEFORE the stream is ended/flushed (pass-18). Its absence is the only
// thing that distinguishes a partial artifact - the file of a run killed
// mid-flight, or of a close that never happened - from a complete drain, so a
// reader can never read a truncated log as if the app had stopped printing.
export const APP_STDIO_CLOSE_MARKER = '[app-stdio closed:';

function defaultLogWriteStream(path) {
  return createWriteStream(path, { flags: 'a', mode: 0o600 });
}

// Bounded close of one log write stream: `end()` flushes and reports 'finish';
// a stream that never finishes is destroyed at the deadline and reported as not
// closed, so teardown can never hang on a stuck artifact.
function endWriteStreamBounded(stream, timeoutMs) {
  if (!stream || typeof stream.end !== 'function') return true;
  if (stream.writableEnded === true || stream.destroyed === true) return true;
  return new Promise(resolvePromise => {
    const timer = setTimeout(() => {
      try { stream.destroy(); } catch { /* the deadline verdict stands */ }
      resolvePromise(false);
    }, timeoutMs);
    try {
      stream.end(() => { clearTimeout(timer); resolvePromise(true); });
    } catch { clearTimeout(timer); resolvePromise(false); }
  });
}

export class AppStdioSink {
  constructor(dir, { maxBytes = APP_STDIO_MAX_BYTES, label = 'app', open = defaultLogWriteStream } = {}) {
    this.dir = dir;
    this.label = label;
    this.maxBytes = maxBytes;
    mkdirSync(dir, { recursive: true, mode: 0o700 });
    this.streams = [
      { name: 'stdout', path: join(dir, `${label}.stdout.log`) },
      { name: 'stderr', path: join(dir, `${label}.stderr.log`) },
    ];
    for (const entry of this.streams) {
      entry.bytes = 0;
      entry.droppedBytes = 0;
      entry.truncated = false;
      entry.writeError = null;
      entry.stream = open(entry.path);
      // A file that cannot be kept open degrades the diagnostic, it never
      // crashes the run: the failure is recorded and reported in the receipt.
      entry.stream?.on?.('error', error => { entry.writeError = String(error?.message ?? error); });
    }
    this.attachError = null;
    this.closed = false;
    this.closedOk = null;
  }

  // Where the two artifacts live, for result.json and the launch evidence.
  paths() {
    return { stdout: this.streams[0].path, stderr: this.streams[1].path };
  }

  // Attach to the child's two pipes. Both listeners stay attached for the whole
  // life of the process: consuming every chunk is exactly what keeps the child's
  // pipe from filling and blocking the app, so the drain is continuous and the
  // cap below only stops the FILE from growing, never the reading.
  attach(child) {
    try {
      this.drainInto(this.streams[0], child?.stdout);
      this.drainInto(this.streams[1], child?.stderr);
    } catch (error) {
      this.attachError = String(error?.message ?? error);
    }
    return this;
  }

  drainInto(entry, source) {
    if (!source || typeof source.on !== 'function') return entry;
    source.on('data', chunk => this.writeChunk(entry, chunk));
    // A read error on the pipe is recorded INTO the artifact instead of escaping
    // as an unhandled 'error' event in the middle of a run.
    source.on('error', error => this.writeChunk(entry, `\n[app-stdio ${entry.name} read error: ${error?.message ?? error}]\n`));
    return entry;
  }

  writeChunk(entry, chunk) {
    const buf = Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk), 'utf8');
    if (entry.truncated) { entry.droppedBytes += buf.length; return; }
    const remaining = this.maxBytes - entry.bytes;
    if (buf.length <= remaining) {
      entry.bytes += buf.length;
      this.writeRaw(entry, buf);
      return;
    }
    if (remaining > 0) {
      this.writeRaw(entry, buf.subarray(0, remaining));
      entry.bytes += remaining;
    }
    entry.droppedBytes += buf.length - Math.max(0, remaining);
    entry.truncated = true;
    this.writeRaw(entry, Buffer.from(
      `\n${APP_STDIO_TRUNCATION_MARKER} ${entry.path} reached the ${this.maxBytes}-byte cap; further output is dropped, and the dropped byte count is recorded in the cleanup receipt]\n`,
      'utf8',
    ));
  }

  writeRaw(entry, buf) {
    try { entry.stream.write(buf); } catch (error) { entry.writeError = String(error?.message ?? error); }
  }

  // What teardown and result.json report: the paths, what each stream received,
  // whether the cap bit, and whether the files really closed.
  receipt() {
    return {
      label: this.label,
      dir: this.dir,
      maxBytes: this.maxBytes,
      closed: this.closed === true,
      closedOk: this.closedOk,
      streams: this.streams.map(entry => ({
        name: entry.name,
        path: entry.path,
        bytes: entry.bytes,
        truncated: entry.truncated,
        droppedBytes: entry.droppedBytes,
        ...(entry.writeError ? { writeError: entry.writeError } : {}),
      })),
      ...(this.attachError ? { attachError: this.attachError } : {}),
    };
  }

  // Idempotent: a second close (the runner's teardown plus a test's own) returns
  // the same receipt instead of re-ending an already finished stream.
  async close({ timeoutMs = BUDGETS.frontendCloseMs } = {}) {
    if (this.closed) return this.receipt();
    this.closed = true;
    const outcomes = [];
    for (const entry of this.streams) {
      if (entry.droppedBytes > 0) {
        this.writeRaw(entry, Buffer.from(
          `\n[app-stdio ${entry.name} dropped ${entry.droppedBytes} bytes after the ${this.maxBytes}-byte cap]\n`,
          'utf8',
        ));
      }
      // Completeness marker LAST, still before the stream ends: everything above
      // (including the truncation marker and the dropped-byte count) is already
      // queued, so the file's final line is the drain's own statement that it
      // finished - written only by a close that really ran.
      this.writeRaw(entry, Buffer.from(
        `\n${APP_STDIO_CLOSE_MARKER} ${entry.name} drained ${entry.bytes} bytes${entry.truncated ? `, capped at ${this.maxBytes} bytes with ${entry.droppedBytes} dropped` : ', not truncated'}; this file is complete]\n`,
        'utf8',
      ));
      outcomes.push(await endWriteStreamBounded(entry.stream, timeoutMs));
    }
    this.closedOk = outcomes.every(Boolean);
    return this.receipt();
  }
}

// The result.json / launch-evidence projection of a sink: the paths a reader
// needs to find the app's own log without guessing, plus what the cap did. This
// is the single derivation the runner writes into result.json, so the artifact
// and the verdict can never disagree about where the app's output went.
export function appStdioResult(sink) {
  if (!sink || typeof sink.receipt !== 'function') return null;
  const receipt = sink.receipt();
  const pathOf = name => receipt.streams.find(entry => entry.name === name)?.path ?? null;
  return {
    dir: receipt.dir,
    stdout: pathOf('stdout'),
    stderr: pathOf('stderr'),
    maxBytes: receipt.maxBytes,
    bytes: receipt.streams.reduce((total, entry) => total + entry.bytes, 0),
    truncated: receipt.streams.some(entry => entry.truncated === true),
    droppedBytes: receipt.streams.reduce((total, entry) => total + entry.droppedBytes, 0),
    closed: receipt.closed === true,
    closedOk: receipt.closedOk,
  };
}

// The app-stdio sink's byte counters at ONE moment: the cheap, in-memory reading
// a run needs to answer "did the app write anything new across this step?"
// without reading the artifacts back. Used as the split step's self-check on the
// claim that the split flow is invisible in the app's stderr - a byte DELTA of
// zero across the click supports that claim, a growing one falsifies it.
// A sink that is not wired is reported as `null` instead of throwing: a missing
// diagnostic sink must never be able to fail a run.
export function appStdioBytes(sink) {
  if (!sink || typeof sink.receipt !== 'function') return null;
  const receipt = sink.receipt();
  const bytesOf = name => receipt.streams.find(entry => entry.name === name)?.bytes ?? 0;
  const stdout = bytesOf('stdout');
  const stderr = bytesOf('stderr');
  return { stdout, stderr, total: stdout + stderr };
}

// ---------------------------------------------------------------------------
// Measurement failure identities (pass-22 audit D1/D4).
//
// The pass-22 observability - the split step's post-click daemon inventory read,
// the app-stdio byte self-check, and the barrier-hub archive - is MEASUREMENT: it
// may never change a verdict. Its failures are therefore RESULT codes on a
// non-throwing path, recorded as their own evidence actions, after which the
// scenario continues exactly as it would have without the measurement. None of
// them is a harness verdict: nothing here is in `classifyNativeFailure`, so none
// of them can reach a FAIL verdict or a non-zero exit code.

// A read (or a sampler beside it) THREW instead of reporting a result. The action
// carries the thrown error's own code and message beside this identity.
export const SPLIT_INVENTORY_READ_FAILED = 'SPLIT_INVENTORY_READ_FAILED';

// A read was NOT taken because the budget charging it had nothing left. The
// action's `cause` names the budget and `detail` says what was left.
export const INVENTORY_READ_REFUSED = 'INVENTORY_READ_REFUSED';

// The refusal above was caused by the MEASURED attempt window (audit D2): the
// read is inside it, so it may only be taken if the window can still pay.
export const ATTEMPT_BUDGET_SPENT = 'ATTEMPT_BUDGET_SPENT';

// The app-stdio byte self-check could not be sampled (a sink whose counters could
// not be read). The counters are reported absent and the scenario is untouched.
export const APP_STDIO_BYTES_FAILED = 'APP_STDIO_BYTES_FAILED';

// ---------------------------------------------------------------------------
// Barrier-hub preservation (task-9 pass-22 observability gap).

// The barrier hub is where the product settles everything this harness reads:
// `<name>.arm.json`, `<name>.armed-ack.json`, `<name>.bound-ack.json`,
// `<name>.held.json`, `<name>.bind.json`, `<name>.release.json`,
// `<name>.request.json`, `<name>.receipt.jsonl` and `marker-recognition.json`.
// It lives INSIDE the isolation root (`<isolationRoot>/barriers`), which the
// cleanup pass removes - so the whole stream was discarded, and in pass 21 a
// receipt question ("was `split-create` settled?") could only be answered by
// inference from the app's stderr instead of by reading the stream. This is the
// bounded copy that keeps it: taken unconditionally by the runner's
// finalization pass, so no future pass has to remember an env var to get it.
export const BARRIER_ARCHIVE_MAX_BYTES = 1 << 20; // 1 MiB

// WHICH MOMENT the archive is (pass-22 audit D3). The runner takes it in its
// finalization pass BEFORE the process reap, so a hub file the product appends
// after the copy - the settlement a reader is looking for - is absent from the
// copy in exactly the same way as a settlement that never arrived, and nothing in
// the receipt said which case it was: `ok` only says the pass ran, and
// `truncated` only says the byte cap bit. These markers are the archive's
// equivalent of the app-stdio sink's own close marker (APP_STDIO_CLOSE_MARKER):
// the artifact states the moment it was taken, in the receipt AND in its own
// bytes, so a stream that stops short can never be read as "the product never
// settled".
export const BARRIER_ARCHIVE_SNAPSHOT_PHASE = 'BEFORE_PROCESS_REAP';
export const BARRIER_ARCHIVE_SNAPSHOT_MARKER = '[barrier-hub archive: snapshot taken BEFORE the process reap]';
export const BARRIER_ARCHIVE_TRUNCATION_SCOPE = 'BYTE_CAP_ONLY';
export const BARRIER_ARCHIVE_SNAPSHOT_FILE = 'barrier-hub-snapshot.json';

// What the archive receipt says about its own moment. `processes` is what the
// caller knows about the run's own processes (`{ pid, label, child }`); the app's
// still-running state is read off each child handle's `exitCode`/`signalCode`,
// which Node sets on exit and leaves null while the process runs - a liveness
// reading taken WITHOUT sending any process a signal. A handle that exposes
// neither field is reported as undetermined rather than guessed at.
function barrierArchiveSnapshot(processes) {
  const entries = (Array.isArray(processes) ? processes : []).map(process => {
    const child = process?.child ?? null;
    const observable = child !== null && typeof child === 'object'
      && (child.exitCode !== undefined || child.signalCode !== undefined);
    return {
      pid: process?.pid ?? null,
      label: process?.label ?? null,
      exited: observable ? (child.exitCode != null || child.signalCode != null) : null,
    };
  });
  return {
    phase: BARRIER_ARCHIVE_SNAPSHOT_PHASE,
    marker: BARRIER_ARCHIVE_SNAPSHOT_MARKER,
    takenBeforeProcessReap: true,
    truncatedScope: BARRIER_ARCHIVE_TRUNCATION_SCOPE,
    note: 'this copy is a snapshot taken BEFORE the process reap: anything the product appended after the copy is absent from it, so a stream that stops short of a settlement is NOT evidence that the settlement never arrived. `truncated` refers ONLY to the maxBytes byte cap. This same block is written into the copy as barrier-hub-snapshot.json.',
    appProcesses: {
      determined: entries.length > 0,
      stillRunning: entries.filter(entry => entry.exited === false).map(entry => entry.pid),
      exited: entries.filter(entry => entry.exited === true).map(entry => entry.pid),
      undetermined: entries.filter(entry => entry.exited === null).map(entry => entry.pid),
      processes: entries,
    },
    markerPath: null,
  };
}

// Copy the barrier hub's regular files into `<evidenceRunDir>/barrier-hub/`.
//
// CONTRACT: this NEVER throws and is NEVER fatal to the run. A hub that cannot
// be archived is reported (`ok: false` + typed `reason`); a file that cannot be
// stat'd or copied is named in `filesSkipped` with its own reason; the copy is
// bounded by `maxBytes`, and the files the cap left behind are named in
// `filesSkipped` (never silently dropped). Files are visited in sorted order so
// the cap bites at a deterministic point. The archive is a SNAPSHOT taken at
// teardown - `bytes`/`filesCopied` say exactly what was preserved, and
// `snapshot` says WHICH MOMENT it is (D3). BARRIER_ARCHIVE_SNAPSHOT_FILE in the
// destination dir carries the same block for a reader who has the copied bytes
// and nothing else; it is the archive's own record and never one of the hub's
// files, so it is not counted in `bytes`/`filesCopied`.
export function archiveBarrierHub(barrierDir, evidenceRunDir, {
  maxBytes = BARRIER_ARCHIVE_MAX_BYTES,
  listDir = readdirSync,
  stat = statSync,
  copy = copyFileSync,
  mkdir = mkdirSync,
  exists = existsSync,
  writeMarker = writeFileSync,
  processes = [],
} = {}) {
  const destDir = typeof evidenceRunDir === 'string' && evidenceRunDir.length > 0
    ? join(evidenceRunDir, 'barrier-hub')
    : null;
  const receipt = {
    sourceDir: typeof barrierDir === 'string' ? barrierDir : null,
    dir: destDir,
    ok: false,
    reason: null,
    filesCopied: [],
    filesSkipped: [],
    bytes: 0,
    truncated: false,
    maxBytes,
    snapshot: barrierArchiveSnapshot(processes),
  };
  try {
    if (destDir === null) {
      receipt.reason = 'EVIDENCE_DIR_UNSET';
      return receipt;
    }
    if (receipt.sourceDir === null || receipt.sourceDir.length === 0) {
      receipt.reason = 'BARRIER_DIR_UNSET';
      return receipt;
    }
    if (!exists(receipt.sourceDir)) {
      receipt.reason = 'BARRIER_DIR_MISSING';
      return receipt;
    }
    const names = listDir(receipt.sourceDir).filter(name => typeof name === 'string').sort();
    mkdir(destDir, { recursive: true, mode: 0o700 });
    for (const name of names) {
      const source = join(receipt.sourceDir, name);
      let size;
      try {
        const info = stat(source);
        if (info.isFile() !== true) {
          receipt.filesSkipped.push({ name, reason: 'NOT_A_FILE' });
          continue;
        }
        size = info.size;
      } catch (error) {
        receipt.filesSkipped.push({ name, reason: `STAT_FAILED: ${error?.message ?? error}` });
        continue;
      }
      if (receipt.bytes + size > maxBytes) {
        receipt.truncated = true;
        receipt.filesSkipped.push({ name, reason: `BYTE_CAP: ${size} bytes would exceed the ${maxBytes}-byte archive cap` });
        continue;
      }
      try {
        copy(source, join(destDir, name));
        receipt.bytes += size;
        receipt.filesCopied.push(name);
      } catch (error) {
        receipt.filesSkipped.push({ name, reason: `COPY_FAILED: ${error?.message ?? error}` });
      }
    }
    receipt.ok = true;
    // The snapshot block travels in the copy's own bytes too, not only in the
    // receipt that describes it. Written LAST, so its presence means the copy
    // loop really ran. Never fatal: a marker that cannot be written is named in
    // the receipt instead of failing the archive.
    try {
      const markerPath = join(destDir, BARRIER_ARCHIVE_SNAPSHOT_FILE);
      writeMarker(markerPath, `${JSON.stringify({
        ...receipt.snapshot,
        sourceDir: receipt.sourceDir,
        dir: destDir,
        filesCopied: receipt.filesCopied,
        bytes: receipt.bytes,
        truncated: receipt.truncated,
        maxBytes: receipt.maxBytes,
      }, null, 2)}\n`, { mode: 0o600 });
      receipt.snapshot.markerPath = markerPath;
    } catch (error) {
      receipt.snapshot.markerError = String(error?.message ?? error);
    }
    return receipt;
  } catch (error) {
    // A diagnostic that cannot be taken is reported, never thrown: the verdict
    // of the run it describes must not depend on it.
    receipt.reason = `ARCHIVE_FAILED: ${error?.message ?? error}`;
    return receipt;
  }
}

// ---------------------------------------------------------------------------
// Resource registry + bounded event-driven cleanup.

export class ResourceRegistry {
  constructor(deps = {}) {
    this.processes = []; // { pid, label, child, executable }
    this.sockets = [];
    this.directories = [];
    // In-process listeners this run opened (the static frontend server). They
    // are closed by the SAME cleanup pass as everything else, so a served
    // frontend cannot outlive the run.
    this.servers = []; // { server, label }
    // Per-run app-stdio artifacts (the app's own stdout/stderr, drained to
    // disk). Closed by the same cleanup pass as everything else.
    this.logs = []; // { sink, label }
    this.reaped = [];
    // Injectable only so the runner unit suite can replay teardown decisions
    // deterministically; production always uses the bounded real helpers.
    this.deps = { ...deps };
  }

  registerProcess(child, label, { executable = null } = {}) {
    this.processes.push({ pid: child.pid, label, child, executable });
    return child;
  }

  registerSocket(path) { this.sockets.push(path); }

  registerServer(server, label = 'server') {
    this.servers.push({ server, label });
    return server;
  }

  registerDirectory(path) { this.directories.push(path); }

  // The app's own stdout/stderr artifacts. Registered like every other resource
  // so the SAME cleanup pass closes them and cleanup.json lists them, but
  // deliberately NOT part of the cleanup gate: the gate's semantics
  // (processes/sockets/directories) stay exactly as they were, so a diagnostic
  // file can never flip a verdict.
  registerLog(sink, label = 'app-stdio') { this.logs.push({ sink, label }); return sink; }

  // Graceful signal first, then a FORCED kill of this run's own tree/group when
  // the child does not exit, then verification from the child's 'exit' event
  // within a bounded deadline. Broad kills (pkill/killall/name patterns) are
  // forbidden. Directories registered here are removed only after the caller has
  // persisted evidence (the runner writes cleanup.json from the evidence
  // runDir), and their removal is retried within a bounded budget so a
  // force-reaped descendant can release the handles it still holds.
  async cleanup() {
    const receipts = [];
    const deps = this.deps;
    const snapshot = deps.snapshot ?? defaultProcessSnapshot;
    // ONE process-table read per teardown, taken BEFORE any signal: it captures
    // the descendant set (a graceful app exit can orphan its own `--daemon`
    // child, and a daemon that left the process group escapes a group kill) and
    // the identity evidence that decides whether a discovered PID may be
    // signalled at all.
    // The read is only needed when this run actually spawned something: with no
    // owned PID there is no tree to walk and nothing to name. A held root still
    // gets its holder probe in the failure path below.
    const rows = this.processes.length > 0 ? await snapshot({ timeoutMs: BUDGETS.cleanupHolderProbeMs }) : [];
    const ownedPids = this.processes.map(entry => entry.pid);
    const roots = [...this.directories];
    const executable = this.processes.find(entry => entry.executable)?.executable ?? null;
    for (const entry of this.processes) {
      const receipt = await reapProcess(entry, {
        rows, roots, snapshot, platform: deps.platform, graceMs: deps.graceMs, forceMs: deps.forceMs,
        runKill: deps.runKill, signalProcess: deps.signalProcess, alive: deps.alive,
      });
      receipts.push(receipt);
      if (receipt.exited) this.reaped.push(entry.pid);
    }
    // App stdio artifacts: closed AFTER the process reap (so the child's last
    // lines are already on their way to disk) and BEFORE the roots below are
    // removed. Closing is bounded and the receipt re-states what was observed.
    for (const entry of this.logs) {
      const receipt = await entry.sink.close({ timeoutMs: deps.logCloseMs ?? BUDGETS.frontendCloseMs });
      receipts.push({ kind: 'log', ...receipt, label: entry.label });
    }
    for (const socketPath of this.sockets) {
      let removed = false;
      try { rmSync(socketPath, { force: true }); removed = !existsSync(socketPath); } catch { removed = false; }
      receipts.push({ kind: 'socket', path: socketPath, removed });
    }
    // In-process listeners first: a still-open listener would keep serving (and
    // keep a port held) while the roots below are removed.
    for (const entry of this.servers) {
      const closed = await closeServerBounded(entry.server, { timeoutMs: deps.serverCloseMs ?? BUDGETS.frontendCloseMs });
      receipts.push({ kind: 'server', label: entry.label, closed });
    }
    for (const dir of this.directories) {
      const removal = await removePathWithRetry(dir, {
        remove: deps.remove, exists: deps.exists, sleep: deps.sleep, now: deps.now,
        attempts: deps.attempts, intervalMs: deps.intervalMs, deadlineMs: deps.deadlineMs,
      });
      const receipt = { kind: 'directory', path: dir, removed: removal.removed, attempts: removal.attempts };
      if (!removal.removed) {
        // Still held: name the holder (pid + identity) instead of claiming a
        // clean teardown. Nothing reported here is ever signalled.
        receipt.holders = await findIsolationRootHolders(dir, {
          ownedPids, executable, roots, snapshot, alive: deps.alive,
        });
      }
      receipts.push(receipt);
    }
    return receipts;
  }
}

// Close one in-process listener within a bounded deadline. The listener lives
// inside this process, so a run that returns still cannot leave it serving; the
// receipt reports what was really observed instead of assuming the close.
export async function closeServerBounded(server, { timeoutMs = BUDGETS.frontendCloseMs } = {}) {
  if (!server || typeof server.close !== 'function') return true;
  if (server.listening === false) return true;
  return new Promise(resolvePromise => {
    const timer = setTimeout(() => {
      try { server.closeAllConnections?.(); } catch { /* nothing left to close */ }
      resolvePromise(false);
    }, timeoutMs);
    try {
      server.close(() => { clearTimeout(timer); resolvePromise(true); });
    } catch {
      clearTimeout(timer);
      resolvePromise(false);
    }
  });
}

// Force-reap ONE process this run spawned, together with its own tree. The
// escalation is bounded at every step and the receipt always re-states what was
// observed, so a surviving child can never be reported as reaped.
export async function reapProcess(entry, options = {}) {
  const { pid, label, child, executable = null } = entry;
  const platform = options.platform ?? process.platform;
  const runKill = options.runKill ?? defaultRunKill;
  const signalProcess = options.signalProcess ?? defaultSignalProcess;
  const alive = options.alive ?? isProcessAlive;
  const graceMs = options.graceMs ?? BUDGETS.cleanupGraceMs;
  const forceMs = options.forceMs ?? BUDGETS.cleanupVerifyTimeoutMs;
  const roots = options.roots ?? [];
  const rows = options.rows ?? await (options.snapshot ?? defaultProcessSnapshot)({ timeoutMs: BUDGETS.cleanupHolderProbeMs });

  // Descendants captured BEFORE any signal, each with its identity verdict.
  const descendants = descendantsOf(rows, [pid]).map(row => {
    const verdict = verifyProcessIdentity(row, { executable, roots, platform });
    return { pid: row.pid, ppid: row.ppid ?? null, executable: row.executable ?? null, identityVerified: verdict.verified, identity: verdict.reason };
  });
  const selfRow = (rows ?? []).find(row => Number(row?.pid) === Number(pid)) ?? null;
  const selfIdentity = verifyProcessIdentity(selfRow, { executable, roots, platform });

  const alreadyTerminated = !child || child.exitCode !== null || child.signalCode !== null;
  let exited = true;
  let escalated = false;
  if (!alreadyTerminated) {
    exited = await awaitExitWithin(child, () => sendGracefulKill({ platform, pid, child, runKill, signalProcess }), graceMs);
    if (!exited) {
      escalated = true;
      exited = await awaitExitWithin(child, () => sendForcedKill({ platform, pid, child, runKill, signalProcess }), forceMs);
    }
    if (!exited) {
      // Last resort only, and only AFTER the tree/group force kill: on Windows
      // this terminates just this one process, which is exactly how the app's
      // own `--daemon` descendant (the pass-6 isolation-root holder) survived
      // the previous version, where it was the first move instead of the last.
      exited = await awaitExitWithin(child, () => { try { child.kill(); } catch { /* exit event decides */ } }, forceMs);
    }
  }

  // Sweep the captured descendants by exact PID. This is what actually closes
  // the pass-6 leak when the app exits on its own (orphaning its daemon) or the
  // daemon left the process group: an unverified PID is never signalled.
  const forceReaped = [];
  for (const descendant of descendants) {
    if (descendant.identityVerified === false) continue;
    if (!alive(descendant.pid)) continue;
    forceKillExactPid(descendant.pid, { platform, runKill, signalProcess });
    forceReaped.push(descendant.pid);
  }

  return {
    kind: 'process', pid, label, exited, alreadyTerminated, escalated,
    code: child?.exitCode ?? null, signal: child?.signalCode ?? null,
    identityVerified: selfIdentity.verified,
    method: alreadyTerminated ? 'already-terminated (no signal sent)'
      : platform === 'win32' ? (escalated ? 'taskkill /T /F (forced process tree)' : 'taskkill /T (process tree)')
      : (escalated ? 'SIGKILL to task-owned process group (escalated from SIGTERM)' : 'SIGTERM to task-owned process group'),
    ...(descendants.length > 0 ? { descendants } : {}),
    ...(forceReaped.length > 0 ? { descendantsForceReaped: forceReaped } : {}),
  };
}

// Bounded race helper: never a fixed sleep; resolves with { timedOut } so the
// caller can convert a timeout into a typed assertion failure.
// `options.onStop` is invoked when the deadline fires so the inner wait can
// cancel its watcher and timer instead of leaking them (review M3).
export function withDeadline(promise, ms, label, options = {}) {
  return new Promise(resolvePromise => {
    const timer = setTimeout(() => {
      try { options.onStop?.(); } catch { /* cancellation must never mask the timeout */ }
      resolvePromise({ timedOut: true, label, budgetMs: ms });
    }, ms);
    promise.then(value => { clearTimeout(timer); resolvePromise({ timedOut: false, label, value }); },
      error => { clearTimeout(timer); resolvePromise({ timedOut: false, label, error }); });
  });
}

function deferredStop() {
  let resolveFn;
  const promise = new Promise(resolvePromise => { resolveFn = resolvePromise; });
  return { promise, stop: () => resolveFn() };
}

// ---------------------------------------------------------------------------
// Pre-armed private barrier events (filesystem, watcher-driven).

export class BarrierHub {
  constructor(rootDir, { runId, operationId } = {}) {
    this.dir = join(rootDir, 'barriers');
    mkdirSync(this.dir, { recursive: true, mode: 0o700 });
    this.armed = new Map();
    this.runId = runId ?? null;
    this.operationId = operationId ?? null;
    this.commands = [];
  }

  // The private channel env: the barrier dir, the run nonce and the operation
  // nonce. The operation nonce travels through the env as well as through every
  // arm file because a scenario that pre-arms no barrier (split-cancel,
  // suspension-ownership, stale-binding) still has to settle receipts the
  // product can correlate; the key is omitted when no nonce exists so the
  // headless lane's env is byte-identical to before.
  env() {
    const env = { FERRYX_QA_BARRIER_DIR: this.dir, FERRYX_QA_RUN_ID: this.runId ?? '' };
    if (this.operationId) env.FERRYX_QA_OPERATION_ID = this.operationId;
    return env;
  }

  // Pre-arm BEFORE launch/trigger; acknowledge held vs released state later.
  prearm(name, {
    deadlineMs = BUDGETS.attemptCeilingMs,
    plan,
    targetRole,
    targetBackendSessionId,
    clientRequestId,
    sourceBackendSessionId,
    workspaceId,
    worktreePath,
  } = {}) {
    if (this.armed.has(name)) throw new HarnessError('ASSERTION_FAILURE', `barrier ${name} armed twice`);
    if (!this.runId || !this.operationId) throw new HarnessError('ASSERTION_FAILURE', 'barrier prearm requires runId and operationId nonces');

    if (targetRole !== undefined && targetRole !== null) {
      if (targetRole !== 'predecessor' && targetRole !== 'successor') {
        throw new HarnessError('ASSERTION_FAILURE', `targetRole must be 'predecessor' or 'successor', never a session ID or wildcard: got ${JSON.stringify(targetRole)}`);
      }
    }

    const spec = {
      name,
      runId: this.runId,
      operationId: this.operationId,
      deadlineMs,
      plan: plan ?? null,
      armedAt: new Date().toISOString(),
      ...(targetRole ? { targetRole } : {}),
      ...(targetBackendSessionId ? { targetBackendSessionId } : {}),
      ...(clientRequestId ? { clientRequestId } : {}),
      ...(sourceBackendSessionId ? { sourceBackendSessionId } : {}),
      ...(workspaceId ? { workspaceId } : {}),
      ...(worktreePath ? { worktreePath } : {}),
    };
    writeFileSync(join(this.dir, `${name}.arm.json`), JSON.stringify(spec, null, 2), { mode: 0o600 });
    this.armed.set(name, { ...spec, heldAt: null, releasedAt: null, receipts: [] });
    return this.armed.get(name);
  }

  // Product-side registration ACK (startup scan + live watch). A barrier
  // without a registered ACK can never satisfy a scenario.
  async awaitRegistered(name, timeoutMs = BUDGETS.barrierAckTimeoutMs) {
    const path = join(this.dir, `${name}.armed-ack.json`);
    const stop = deferredStop();
    const outcome = await withDeadline(waitForFile(path, stop.promise), timeoutMs, `barrier ${name} registered`, { onStop: stop.stop });
    if (outcome.timedOut) throw new HarnessError('BARRIER_ACK_TIMEOUT', `product did not register armed barrier ${name} within ${timeoutMs}ms`);
    if (outcome.error) throw outcome.error instanceof HarnessError ? outcome.error : new HarnessError('BARRIER_ACK_TIMEOUT', `${name} registration wait failed: ${outcome.error.message}`);
    return correlateReceipt(JSON.parse(outcome.value), this, `barrier ${name} registration`);
  }

  // Product-side live-arm binding ACK (<name>.bound-ack.json).
  async awaitBound(name, timeoutMs = BUDGETS.barrierAckTimeoutMs) {
    const path = join(this.dir, `${name}.bound-ack.json`);
    const stop = deferredStop();
    const outcome = await withDeadline(waitForFile(path, stop.promise), timeoutMs, `barrier ${name} bound`, { onStop: stop.stop });
    if (outcome.timedOut) throw new HarnessError('BARRIER_ACK_TIMEOUT', `product did not acknowledge binding for barrier ${name} within ${timeoutMs}ms`);
    if (outcome.error) throw outcome.error instanceof HarnessError ? outcome.error : new HarnessError('BARRIER_ACK_TIMEOUT', `${name} bound wait failed: ${outcome.error.message}`);
    const parsed = JSON.parse(outcome.value);
    correlateReceipt(parsed, this, `barrier ${name} bound-ack`);
    if (typeof parsed.targetBackendSessionId !== 'string' || parsed.targetBackendSessionId.length === 0) {
      throw new HarnessError('ASSERTION_FAILURE', `bound-ack for ${name} lacks valid targetBackendSessionId: ${JSON.stringify(parsed)}`);
    }
    return parsed;
  }

  // Event-driven wait for <name>.held.json (bounded).
  async awaitHeld(name, timeoutMs = BUDGETS.barrierAckTimeoutMs) {
    const path = join(this.dir, `${name}.held.json`);
    const stop = deferredStop();
    const outcome = await withDeadline(waitForFile(path, stop.promise), timeoutMs, `barrier ${name} held`, { onStop: stop.stop });
    if (outcome.timedOut) throw new HarnessError('BARRIER_ACK_TIMEOUT', `product did not acknowledge barrier ${name} within ${timeoutMs}ms`);
    if (outcome.error) throw outcome.error instanceof HarnessError ? outcome.error : new HarnessError('BARRIER_ACK_TIMEOUT', `${name} held wait failed: ${outcome.error.message}`);
    const entry = this.armed.get(name);
    if (entry) entry.heldAt = new Date().toISOString();
    return correlateReceipt(JSON.parse(outcome.value), this, `barrier ${name} held`);
  }

  // Receipts are append-only JSONL; `index` is the 0-based line to await so
  // successive settlements of the same barrier are distinguishable.
  async awaitReceipt(name, index, timeoutMs = BUDGETS.attemptCeilingMs) {
    const path = join(this.dir, `${name}.receipt.jsonl`);
    const stop = deferredStop();
    const outcome = await withDeadline(waitForLine(path, index + 1, stop.promise), timeoutMs, `barrier ${name} receipt[${index}]`, { onStop: stop.stop });
    if (outcome.timedOut) throw new HarnessError('BARRIER_ACK_TIMEOUT', `product did not settle barrier ${name} receipt[${index}] within ${timeoutMs}ms`);
    if (outcome.error) throw outcome.error instanceof HarnessError ? outcome.error : new HarnessError('BARRIER_ACK_TIMEOUT', `${name} receipt[${index}] wait failed: ${outcome.error.message}`);
    const entry = this.armed.get(name);
    const parsed = correlateReceipt(JSON.parse(outcome.value), this, `barrier ${name} receipt[${index}]`);
    if (entry) entry.receipts.push(parsed);
    return parsed;
  }

  // Event-driven wait for the first receipt line that satisfies `match`, over
  // lines already settled as well as lines appended while waiting.
  //
  // Needed because one receipt stream can carry more than one session's
  // settlements: with a real pane present, the app's own pane AND the scenario's
  // split pane both present frames into `presentation.receipt.jsonl`, so a line
  // INDEX binds the assertion to whichever pane happened to present first. A
  // session-addressed wait binds it to the pane under test - the assertion is
  // unchanged, only the line it reads is.
  async awaitReceiptMatching(name, { match, timeoutMs = BUDGETS.attemptCeilingMs, label = name } = {}) {
    if (typeof match !== 'function') {
      throw new HarnessError('ASSERTION_FAILURE', `awaitReceiptMatching(${name}) requires a match predicate`);
    }
    const path = join(this.dir, `${name}.receipt.jsonl`);
    const stop = deferredStop();
    const outcome = await withDeadline(
      waitForMatchingLine(path, match, stop.promise),
      timeoutMs,
      `barrier ${name} matching receipt`,
      { onStop: stop.stop },
    );
    if (outcome.timedOut) throw new HarnessError('BARRIER_ACK_TIMEOUT', `${label}: no ${name} receipt matched within ${timeoutMs}ms`);
    if (outcome.error) throw outcome.error instanceof HarnessError ? outcome.error : new HarnessError('BARRIER_ACK_TIMEOUT', `${label}: ${outcome.error.message}`);
    const entry = this.armed.get(name);
    const parsed = correlateReceipt(JSON.parse(outcome.value), this, label);
    if (entry) entry.receipts.push(parsed);
    return parsed;
  }

  // The receipt stream's own file, and its physical line count. A blocked
  // pane-binding report needs both to tell "the stream held nothing" (the pane
  // step created no pane at all) from "the stream held lines that named only
  // fixture sessions" (a pane was created but never presented) - the observed
  // session list alone cannot separate those two defects.
  receiptPath(name) {
    return join(this.dir, `${name}.receipt.jsonl`);
  }

  receiptLineCount(name) {
    let text;
    try { text = readFileSync(this.receiptPath(name), 'utf8'); } catch { return 0; }
    return text.split('\n').filter(line => line.trim().length > 0).length;
  }

  // Every settled line of one receipt stream, for evidence and for the
  // ambiguity/absence reports below. A missing file is an empty stream, never
  // an error: "nothing settled yet" is a measurement.
  receiptLines(name) {
    const path = join(this.dir, `${name}.receipt.jsonl`);
    let text;
    try { text = readFileSync(path, 'utf8'); } catch { return []; }
    const parsed = [];
    for (const line of text.split('\n')) {
      if (!line.trim()) continue;
      try {
        const record = JSON.parse(line);
        if (record && typeof record === 'object') parsed.push(record);
      } catch { /* a torn line is not a settlement */ }
    }
    return parsed;
  }

  // A receipt addressed by the session it names: `sessionId` for producer
  // receipts (marker-output) and the presentation receipt's own
  // `attachTuple.backendSessionId`.
  async awaitReceiptForSession(name, sessionId, timeoutMs = BUDGETS.attemptCeilingMs, label = name) {
    if (typeof sessionId !== 'string' || sessionId.length === 0) {
      throw new HarnessError('ASSERTION_FAILURE', `awaitReceiptForSession(${name}) requires a concrete session id, got ${JSON.stringify(sessionId)}`);
    }
    return this.awaitReceiptMatching(name, {
      timeoutMs,
      label: `${label} for session ${sessionId}`,
      match: receipt => receipt?.sessionId === sessionId || receipt?.attachTuple?.backendSessionId === sessionId,
    });
  }

  // Runner->product command through the same private channel (e.g. the
  // split-cancel request). The product side is a proposed hook
  // (task-3-rust-proposal.md); without it the scenario fails explicitly.
  command(name, payload) {
    if (!this.runId || !this.operationId) throw new HarnessError('ASSERTION_FAILURE', 'command requires runId and operationId nonces');
    const record = { name, runId: this.runId, operationId: this.operationId, issuedAt: new Date().toISOString(), ...payload };
    writeFileSync(join(this.dir, `${name}.request.json`), JSON.stringify(record, null, 2), { mode: 0o600 });
    this.commands.push(record);
    return record;
  }

  release(name) {
    const entry = this.armed.get(name);
    if (!entry) throw new HarnessError('ASSERTION_FAILURE', `release of un-armed barrier ${name}`);
    // Channel ownership update: release controls carry the run nonce and
    // operation identity too, so the product can reject stale/replayed or
    // wrong-operation controls (not only receipts).
    writeFileSync(join(this.dir, `${name}.release.json`), JSON.stringify({
      name, runId: this.runId, operationId: this.operationId, releasedAt: new Date().toISOString(),
    }, null, 2), { mode: 0o600 });
    entry.releasedAt = new Date().toISOString();
  }

  snapshot() {
    return [...this.armed.entries()].map(([name, e]) => ({
      name, armedAt: e.armedAt, heldAt: e.heldAt, releasedAt: e.releasedAt, receiptCount: e.receipts.length, plan: e.plan,
    }));
  }

  isArmed(name) {
    return this.armed.has(name);
  }

  bindBackendSession(name, backendSessionId, { clientRequestId } = {}) {
    if (!backendSessionId || typeof backendSessionId !== 'string' || backendSessionId.trim() === '' || backendSessionId === '*') {
      throw new HarnessError('ASSERTION_FAILURE', `bindBackendSession requires concrete, non-wildcard session ID, got ${JSON.stringify(backendSessionId)}`);
    }
    const entry = this.armed.get(name);
    if (entry) {
      entry.targetBackendSessionId = backendSessionId;
    }
    const bindRecord = {
      name,
      runId: this.runId,
      operationId: this.operationId,
      targetBackendSessionId: backendSessionId,
      clientRequestId: clientRequestId ?? null,
      boundAt: new Date().toISOString(),
    };
    writeFileSync(join(this.dir, `${name}.bind.json`), JSON.stringify(bindRecord, null, 2), { mode: 0o600 });
    return bindRecord;
  }

  recordCaptureReady(metadata) {
    if (!this.runId || !this.operationId) throw new HarnessError('ASSERTION_FAILURE', 'capture-ready requires runId and operationId nonces');
    const record = {
      runId: this.runId,
      operationId: this.operationId,
      capturedAt: new Date().toISOString(),
      ...metadata,
    };
    writeFileSync(join(this.dir, 'capture-ready.json'), JSON.stringify(record, null, 2), { mode: 0o600 });
    return record;
  }
}

// `options.accept` is an optional acceptance predicate over the file's CONTENT.
// It exists because CREATION IS NOT CONTENT: cmd.exe's `echo %ERRORLEVEL% > file`
// creates the file and then writes its payload in a second step, so a watcher
// armed on the file's appearance can read it in between and see zero bytes - the
// pass-20 defect that discarded a completed run whose exit file held 4 perfectly
// parseable bytes (`34 20 0D 0A` = "4 \r\n"). With a predicate, the watcher stays
// armed across the create event and resolves on the write that satisfies it. With
// no predicate the behaviour is exactly as before (resolve as soon as the file is
// readable), so every existing caller is unchanged.
export function waitForFile(path, stopPromise, options = {}) {
  const accept = typeof options.accept === 'function' ? options.accept : null;
  return new Promise((resolvePromise, rejectPromise) => {
    const present = () => { try { return readFileSync(path, 'utf8'); } catch { return null; } };
    const satisfied = () => {
      const data = present();
      if (data === null) return null;
      return accept === null || accept(data) ? data : null;
    };
    const initial = satisfied();
    if (initial !== null) return resolvePromise(initial);
    let watcher;
    const close = () => { if (watcher) watcher.close(); clearTimeout(timer); };
    const finish = error => {
      if (error) { close(); rejectPromise(error); return; }
      const data = present();
      if (data === null) { close(); rejectPromise(new HarnessError('BARRIER_ACK_TIMEOUT', `watcher race: ${path} vanished`)); return; }
      // Present but not yet accepted (created, not yet written): keep the watcher
      // armed for the next event instead of resolving with content that is not
      // there yet.
      if (accept !== null && !accept(data)) return;
      close();
      resolvePromise(data);
    };
    const timer = setTimeout(() => finish(new HarnessError('BARRIER_ACK_TIMEOUT', `watcher deadline for ${path}`)), 60_000);
    // Cancellation (review M3): an outer deadline closes the watcher and
    // clears the timer instead of leaking both.
    let stopHandler = null;
    if (stopPromise) {
      stopHandler = () => finish(new HarnessError('BARRIER_ACK_TIMEOUT', `wait cancelled by outer deadline: ${path}`));
      stopPromise.then(stopHandler);
    }
    try {
      watcher = watch(dirname(path), { persistent: true }, (event, filename) => {
        if (satisfied() !== null) finish();
      });
      if (satisfied() !== null) finish();
    } catch (error) { finish(error); }
  });
}

// ---------------------------------------------------------------------------
// Evidence writer: E/task-3-harness/<scenario>/run-<uuid>/ + atomic latest.json.

export class EvidenceWriter {
  constructor(evidenceBase, scenario) {
    this.runDir = join(evidenceBase, 'task-3-harness', scenario, `run-${randomUUID()}`);
    mkdirSync(this.runDir, { recursive: true, mode: 0o700 });
    this.scenario = scenario;
    this.actions = [];
  }

  action(entry) {
    const record = { at: new Date().toISOString(), ...entry };
    this.actions.push(record);
    writeFileSync(join(this.runDir, 'actions.jsonl'), this.actions.map(a => JSON.stringify(a)).join('\n') + '\n', { mode: 0o600 });
  }

  write(name, payload) {
    writeFileSync(join(this.runDir, name), typeof payload === 'string' ? payload : JSON.stringify(payload, null, 2), { mode: 0o600 });
  }

  finish(result) {
    this.write('result.json', result);
    // Atomic pointer: write temp then rename, single resolution point for F1.
    const pointer = {
      scenario: this.scenario, runDir: this.runDir, verdict: result.verdict,
      nativeEvidence: result.nativeEvidence ?? null, sourceDigest: result.sourceDigest ?? null,
      argv: result.invocation?.argv ?? null, finishedAt: new Date().toISOString(),
    };
    const tmp = join(this.runDir, `.latest.${randomUUID()}.json`);
    writeFileSync(tmp, JSON.stringify(pointer, null, 2), { mode: 0o600 });
    renameSync(tmp, join(dirname(this.runDir), 'latest.json'));
  }
}

export function spawnOwned(registry, command, args, options = {}) {
  // Review M1 / repair B3: on posix the child MUST lead its own process group
  // so descendants are reaped with the group. `detached` is forced AFTER the
  // caller spread so no caller option can defeat group ownership; win32 keeps
  // the in-tree `taskkill /T` cleanup instead. The child stays registered by
  // exact PID and its exit event is still awaited - no broad kills.
  //
  // App stdio sink (the pass-12 gap): when the caller supplies one, BOTH pipes
  // are drained to disk as they arrive, so the app's own `[cmd_terminal_spawn]
  // stage=...` lines always land in an artifact. It is an option, never an
  // env-var gate, and the entrypoint injects it into EVERY spawn it makes, so a
  // normal QA run leaves the app's log on disk with nothing for a future pass to
  // remember. Draining cannot change what the child sees: every chunk is
  // consumed continuously (no back-pressure), and the child's exit code and
  // cleanup semantics are untouched.
  const { stdioSink = null, ...spawnOptions } = options;
  const child = spawn(command, args, { stdio: ['ignore', 'pipe', 'pipe'], ...spawnOptions, detached: process.platform !== 'win32' });
  if (stdioSink && typeof stdioSink.attach === 'function') stdioSink.attach(child);
  registry.registerProcess(child, args.join(' '), { executable: command });
  return child;
}

// Correlation gate: every product emission must echo the run/operation
// nonces and identify its producing component. A receipt from another run,
// a stale file or an anonymous producer is rejected (review P2).
export function correlateReceipt(receipt, hub, label) {
  if (!receipt || typeof receipt !== 'object') throw new HarnessError('ASSERTION_FAILURE', `${label}: receipt is not an object`);
  if (receipt.runId !== hub.runId) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: runId mismatch (expected ${hub.runId}, got ${JSON.stringify(receipt.runId)})`);
  }
  if (receipt.operationId !== hub.operationId) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: operationId mismatch (expected ${hub.operationId}, got ${JSON.stringify(receipt.operationId)})`);
  }
  if (typeof receipt.producer !== 'string' || receipt.producer.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', `${label}: receipt lacks a producer component ID`);
  }
  return receipt;
}

// Repair B4: post-release classifier recovery must be POSITIVELY observed
// (Idle/Healthy). A truthful Unknown - with or without evidenceMissing - is
// recorded as a nonpassing RECOVERY_UNPROVEN failure. Barrier release alone
// never counts as recovery.
export function assertPositiveRecovery(receipt, label) {
  const verdict = receipt?.classifierVerdict ?? receipt?.verdict;
  if (verdict === 'Idle' || verdict === 'Healthy') return verdict;
  if (verdict === 'Unknown') {
    throw new HarnessError('RECOVERY_UNPROVEN', `${label}: post-release verdict is Unknown${receipt?.evidenceMissing === true ? ' (evidenceMissing)' : ''} - recovery was not positively observed; release alone is not recovery`);
  }
  throw new HarnessError('RECOVERY_UNPROVEN', `${label}: post-release verdict ${JSON.stringify(verdict)} is not a positive recovery observation`);
}

// The predicate form of `waitForLine`: resolve the first line of `path` that
// `match` accepts, watching the directory for appends until the outer deadline
// cancels the wait (the same bounded, event-driven shape as every other wait in
// this file - no fixed sleeps and no polling loop of its own).
function waitForMatchingLine(path, match, stopPromise) {
  return new Promise((resolvePromise, rejectPromise) => {
    const found = () => {
      let text;
      try { text = readFileSync(path, 'utf8'); } catch { return null; }
      for (const line of text.split('\n')) {
        if (!line.trim()) continue;
        let parsed;
        try { parsed = JSON.parse(line); } catch { continue; }
        if (parsed && typeof parsed === 'object' && match(parsed)) return line;
      }
      return null;
    };
    const initial = found();
    if (initial !== null) return resolvePromise(initial);
    let watcher;
    const finish = error => {
      if (watcher) watcher.close();
      clearTimeout(timer);
      if (error) rejectPromise(error);
      else {
        const value = found();
        if (value === null) rejectPromise(new HarnessError('BARRIER_ACK_TIMEOUT', `receipt line race: ${path}`));
        else resolvePromise(value);
      }
    };
    const timer = setTimeout(() => finish(new HarnessError('BARRIER_ACK_TIMEOUT', `deadline waiting for a matching line of ${path}`)), 60_000);
    if (stopPromise) {
      stopPromise.then(() => finish(new HarnessError('BARRIER_ACK_TIMEOUT', `wait cancelled by outer deadline: ${path}`)));
    }
    try {
      watcher = watch(dirname(path), { persistent: true }, () => { if (found() !== null) finish(); });
    } catch (error) { finish(error); }
  });
}

function waitForLine(path, minCount, stopPromise) {
  return new Promise((resolvePromise, rejectPromise) => {
    const lines = () => { try { return readFileSync(path, 'utf8').split('\n').filter(l => l.trim() !== ''); } catch { return []; } };
    const present = lines();
    if (present.length >= minCount) return resolvePromise(present[minCount - 1]);
    let watcher;
    const finish = error => {
      if (watcher) watcher.close();
      clearTimeout(timer);
      if (error) rejectPromise(error);
      else {
        const current = lines();
        const value = current[minCount - 1] ?? null;
        if (value === null) rejectPromise(new HarnessError('BARRIER_ACK_TIMEOUT', `receipt line race: ${path}`));
        else resolvePromise(value);
      }
    };
    const timer = setTimeout(() => finish(new HarnessError('BARRIER_ACK_TIMEOUT', `deadline waiting for line ${minCount} of ${path}`)), 60_000);
    let stopHandler = null;
    if (stopPromise) {
      stopHandler = () => finish(new HarnessError('BARRIER_ACK_TIMEOUT', `wait cancelled by outer deadline: ${path}`));
      stopPromise.then(stopHandler);
    }
    try {
      watcher = watch(dirname(path), { persistent: true }, () => { if (lines().length >= minCount) finish(); });
    } catch (error) { finish(error); }
  });
}

// Digest binding the evidence pointer to the exact runner source bytes.
export function computeSourceDigest(paths, baseDir = fileURLToPath(new URL('../../../', import.meta.url))) {
  const hash = createHash('sha256');
  // Relative source paths belong to the checkout, not the gate's ui/ cwd.
  for (const path of paths) hash.update(readFileSync(resolve(baseDir, path)));
  return hash.digest('hex');
}

// Cleanup gate: PASS requires every registered process reaped and every
// registered socket/directory removed. Recording failures while exiting 0 is
// forbidden (review blocker 10).
export function computeCleanupGate(registry, receipts) {
  const processesReaped = registry.processes.every(p => registry.reaped.includes(p.pid));
  const socketsRemoved = registry.sockets.every(s => receipts.find(r => r.kind === 'socket' && r.path === s)?.removed === true);
  const directoriesRemoved = registry.directories.every(d => receipts.find(r => r.kind === 'directory' && r.path === d)?.removed === true);
  return { processesReaped, socketsRemoved, directoriesRemoved, ok: processesReaped && socketsRemoved && directoriesRemoved };
}

export function collectOutput(child) {
  return new Promise(resolvePromise => {
    let stdout = ''; let stderr = '';
    child.stdout?.on('data', chunk => { stdout += chunk; });
    child.stderr?.on('data', chunk => { stderr += chunk; });
    child.once('exit', (code, signal) => resolvePromise({ stdout, stderr, code, signal }));
  });
}
