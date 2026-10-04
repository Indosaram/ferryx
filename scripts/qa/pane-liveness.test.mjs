// Task 3 runner unit tests (authored; execution delegated to the sole remote
// verifier per plan - never run locally). Follows the scripts/qa/*.test.mjs
// pattern that imports vitest from ui/node_modules.

import { test, expect } from '../../ui/node_modules/vitest/dist/index.js';
import { mkdtempSync, mkdirSync, rmSync, existsSync, writeFileSync, chmodSync, watch, realpathSync } from 'node:fs';
import { readFileSync } from 'node:fs';
import { tmpdir, homedir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  BUDGETS, EXIT, HEADLESS_ELIGIBLE, SCENARIOS, HarnessError, parseInvocation, preflight,
  BarrierHub, ResourceRegistry, correlateReceipt, computeCleanupGate,
  computeSourceDigest, withDeadline, assertPositiveRecovery, spawnOwned,
  MonotonicBudget, validateFixtureSetup, SCENARIO_FIXTURE_REQUIREMENTS, BARRIER_ROLES,
  requireSevenTupleReceipt, requireFiveTupleReceipt,
  LOCAL_SPLIT_LIFECYCLE_CAPABILITY, ATTACH_TUPLE_FIELDS,
} from '../lib/qa-scenarios/common-harness.mjs';
import { assertClassifierReceipt, runHeadlessDiagnosticClassifier, runNativeDiagnosticClassifier, buildIsolatedEnv } from '../lib/qa-scenarios/diagnostic-classifier.mjs';
import { assertInvariants, assertSinglePty, SCENARIO_PLANS } from './pane-liveness.mjs';
import {
  MARKER_TEXT,
  performInspectionHandshake,
} from '../lib/qa-scenarios/native-driver.mjs';
import {
  runSplitHappyScenario,
  runSplitAttachStallScenario,
  runSplitCancelScenario,
  runSplitConcurrentScenario,
} from '../lib/qa-scenarios/split-scenarios.mjs';
import {
  runRetainedHandoverScenario,
  runHandoverAbortScenario,
  runSuspensionOwnershipScenario,
  runStaleBindingScenario,
} from '../lib/qa-scenarios/lifecycle-scenarios.mjs';

const fixtureRoot = () => realpathSync(mkdtempSync(join(realpathSync(tmpdir()), 'pane-liveness-test-')));

test('headless presentation releases after held event before its only settlement receipt', async () => {
  const root = fixtureRoot();
  const released = new Set();
  const receiptCalls = [];
  const held = { sessionId: 'session', operationId: 'op', classifierVerdict: 'BlockedInPresentation', presentationEvidence: 'coordinator-pending', coordinatorEvidence: 'coordinator-pending' };
  const hub = {
    dir: root,
    env: () => ({}),
    awaitRegistered: async () => {},
    awaitHeld: async () => held,
    release: name => released.add(name),
    awaitReceipt: async (name, index) => {
      if (name === 'fixture-setup') return {};
      if (name === 'backend-write' && index === 0) return { sessionId: 'session', operationId: 'op', classifierVerdict: 'BlockedInIpcWrite' };
      expect(released.has(name)).toBe(true);
      if (name === 'presentation') { expect(index).toBe(0); receiptCalls.push(index); }
      return { sessionId: 'session', operationId: 'op', classifierVerdict: 'Idle', presentationEvidence: 'coordinator-consumed', stageProgress: { releaseOutcome: 'released', backendWriteCompleted: true, success: true, frameConsumed: true, renderPendingAfter: false } };
    },
  };
  try {
    const result = await runHeadlessDiagnosticClassifier({ isolationRoot: root, barrierHub: hub, binary: 'unused', spawnOwned: () => {}, evidence: { action: () => {} } });
    expect(result.presentationVerdict).toBe('BlockedInPresentation');
    expect(result.presentationRecovered).toBe('Idle');
    expect(receiptCalls).toEqual([0]);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

function executableFile(dir, name = 'fake-binary') {
  const path = join(dir, name);
  writeFileSync(path, '#!/bin/sh\nexit 0\n', { mode: 0o755 });
  return path;
}

test('the nine plan scenarios are the only accepted names, and headless is diagnostic-classifier only', () => {
  expect(SCENARIOS).toHaveLength(9);
  expect(SCENARIOS).toContain('diagnostic-classifier');
  expect(HEADLESS_ELIGIBLE).toEqual(['diagnostic-classifier']);
  const parsed = parseInvocation(['--scenario', 'split-happy', '--binary', '/abs/b', '--evidence-dir', '/abs/e', '--isolation-root', '/abs/r']);
  expect(parsed).toMatchObject({ scenario: 'split-happy', headless: false });
  expect(() => parseInvocation(['--scenario', 'split-cancel', '--binary', '/abs/b', '--evidence-dir', '/abs/e', '--isolation-root', '/abs/r', '--headless']))
    .toThrowError(HarnessError);
  expect(() => parseInvocation(['--scenario', 'invented', '--binary', '/abs/b', '--evidence-dir', '/abs/e', '--isolation-root', '/abs/r']))
    .toThrowError(/INVALID_SCENARIO/);
});

test('unknown flags, missing flags, duplicates and positionals are rejected before launch', () => {
  const base = ['--scenario', 'split-happy', '--binary', '/abs/b', '--evidence-dir', '/abs/e', '--isolation-root', '/abs/r'];
  expect(() => parseInvocation([...base, '--headless-mode'])).toThrowError(/INVALID_FLAG/);
  expect(() => parseInvocation(base.slice(0, 2))).toThrowError(/MISSING_FLAG/);
  expect(() => parseInvocation([...base, '--binary', '/abs/other'])).toThrowError(/DUPLICATE_FLAG/);
  expect(() => parseInvocation([...base, 'extra'])).toThrowError(/INVALID_FLAG/);
});

test('preflight rejects production runtime roots, preexisting isolation roots, relative paths and production installs', () => {
  const root = fixtureRoot();
  const binary = executableFile(root);
  const evidence = join(root, 'evidence');
  const freshRoot = join(root, 'isolation');
  const prodRoot = process.platform === 'win32'
    ? join(homedir(), '.rorca')
    : `/tmp/rorca-${process.getuid?.() ?? 0}`;
  expect(() => preflight({ scenario: 'split-happy', headless: false, binary, evidenceDir: evidence, isolationRoot: prodRoot }))
    .toThrowError(/PRODUCTION_RUNTIME_ROOT/);
  expect(() => preflight({ scenario: 'split-happy', headless: false, binary, evidenceDir: evidence, isolationRoot: join(homedir(), '.rorca') }))
    .toThrowError(/PRODUCTION_RUNTIME_ROOT/);
  expect(() => preflight({ scenario: 'split-happy', headless: false, binary, evidenceDir: evidence, isolationRoot: root }))
    .toThrowError(/PREEXISTING_ISOLATION_ROOT/);
  expect(() => preflight({ scenario: 'split-happy', headless: false, binary: 'relative/bin', evidenceDir: evidence, isolationRoot: freshRoot }))
    .toThrowError(/PATH_NOT_ABSOLUTE/);
  expect(() => preflight({ scenario: 'split-happy', headless: false, binary: join(root, 'missing'), evidenceDir: evidence, isolationRoot: freshRoot }))
    .toThrowError(/BINARY_MISSING/);
  if (process.platform !== 'win32') {
    chmodSync(binary, 0o644);
    expect(() => preflight({ scenario: 'split-happy', headless: false, binary, evidenceDir: evidence, isolationRoot: freshRoot }))
      .toThrowError(/BINARY_NOT_EXECUTABLE/);
  }
  const ok = preflight({ scenario: 'split-happy', headless: false, binary: executableFile(root, 'b2'), evidenceDir: evidence, isolationRoot: freshRoot });
  expect(ok.binarySha256).toMatch(/^[0-9a-f]{64}$/);
  rmSync(root, { recursive: true, force: true });
});

test('barriers pre-arm before trigger, acknowledge held/released state, and time out without product acks', async () => {
  const root = fixtureRoot();
  const hub = new BarrierHub(root, { runId: 'run-1', operationId: 'op-1' });
  hub.prearm('backend-write', { plan: 'test' });
  const arm = JSON.parse(readFileSync(join(hub.dir, 'backend-write.arm.json'), 'utf8'));
  expect(arm).toMatchObject({ runId: 'run-1', operationId: 'op-1' });
  expect(() => new BarrierHub(root).prearm('x')).toThrowError(/nonces/);
  expect(() => hub.prearm('backend-write')).toThrowError(/armed twice/);
  // No product ack within a bounded window => typed failure, never a silent PASS.
  await expect(hub.awaitHeld('backend-write', 50)).rejects.toThrowError(/BARRIER_ACK_TIMEOUT/);
  await expect(hub.awaitRegistered('backend-write', 50)).rejects.toThrowError(/BARRIER_ACK_TIMEOUT/);
  // Simulated product acknowledgement through the private channel files.
  writeFileSync(join(hub.dir, 'backend-write.armed-ack.json'), JSON.stringify({ runId: 'run-1', operationId: 'op-1', producer: 'qa-test' }));
  await hub.awaitRegistered('backend-write', 1000);
  writeFileSync(join(hub.dir, 'backend-write.held.json'), JSON.stringify({ runId: 'run-1', operationId: 'op-1', producer: 'qa-test', sessionId: 's1' }));
  const held = await hub.awaitHeld('backend-write', 1000);
  expect(held).toMatchObject({ sessionId: 's1' });
  // Append-only JSONL receipts: index selects the settlement line.
  writeFileSync(join(hub.dir, 'backend-write.receipt.jsonl'), JSON.stringify({ runId: 'run-1', operationId: 'op-1', producer: 'qa-test', classifierVerdict: 'BlockedInIpcWrite' }) + '\n');
  const first = await hub.awaitReceipt('backend-write', 0, 1000);
  expect(first.classifierVerdict).toBe('BlockedInIpcWrite');
  await expect(hub.awaitReceipt('backend-write', 1, 50)).rejects.toThrowError(/BARRIER_ACK_TIMEOUT/);
  hub.release('backend-write');
  expect(() => hub.release('never-armed')).toThrowError(/un-armed/);
  // Commands carry the same nonces.
  hub.command('split-cancel', { phase: 'while-creating' });
  expect(JSON.parse(readFileSync(join(hub.dir, 'split-cancel.request.json'), 'utf8'))).toMatchObject({ runId: 'run-1', operationId: 'op-1', phase: 'while-creating' });
  rmSync(root, { recursive: true, force: true });
});

test('timing budgets match the plan contract', () => {
  expect(BUDGETS.attemptCeilingMs).toBe(15_000);
  expect(BUDGETS.stagePrepareCreateStatusMs).toBe(9_000);
  expect(BUDGETS.stageAttachListenerMs).toBe(4_000);
  expect(BUDGETS.stagePresentationMs).toBe(2_000);
  expect(BUDGETS.cancelAckCeilingMs).toBe(3_000);
  expect(BUDGETS.cancelDaemonResponseBudgetMs).toBe(2_500);
});

test('cleanup reaps registered owned processes by PID and removes registered roots', async () => {
  const root = fixtureRoot();
  const registry = new ResourceRegistry();
  const { spawn } = await import('node:child_process');
  const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)']);
  registry.registerProcess(child, 'owned-fixture-child');
  const subRoot = join(root, 'owned-sub');
  mkdirSync(subRoot);
  registry.registerDirectory(subRoot);
  const cleanup = await registry.cleanup();
  const processReceipt = cleanup.find(r => r.kind === 'process');
  expect(processReceipt.exited).toBe(true);
  expect(registry.reaped).toContain(child.pid);
  expect(existsSync(subRoot)).toBe(false);
  rmSync(root, { recursive: true, force: true });
});

// ---------------------------------------------------------------------------
// Consolidated review v4 regression cases (runnable remotely by the sole
// verifier; authored, not executed locally).

test('regression H10 false-pass: a cleanup failure cannot leave a passing gate', () => {
  const registry = new ResourceRegistry();
  registry.processes.push({ pid: 4242, label: 'unreaped', child: null });
  registry.registerDirectory('/tmp/definitely-not-removed-dir');
  const gate = computeCleanupGate(registry, []);
  expect(gate.ok).toBe(false);
  expect(gate.processesReaped).toBe(false);
  expect(gate.directoriesRemoved).toBe(false);
  const okRegistry = new ResourceRegistry();
  okRegistry.registerDirectory('/tmp/also-not-removed');
  const okGate = computeCleanupGate(okRegistry, [{ kind: 'directory', path: '/tmp/also-not-removed', removed: true }]);
  expect(okGate.ok).toBe(true);
});

test('regression P2 correlation: receipts must echo runId/operationId/producer nonces', () => {
  const hub = { runId: 'run-A', operationId: 'op-A' };
  expect(() => correlateReceipt({ runId: 'run-B', operationId: 'op-A', producer: 'p' }, hub, 'x')).toThrowError(/runId mismatch/);
  expect(() => correlateReceipt({ runId: 'run-A', operationId: 'op-B', producer: 'p' }, hub, 'x')).toThrowError(/operationId mismatch/);
  expect(() => correlateReceipt({ runId: 'run-A', operationId: 'op-A' }, hub, 'x')).toThrowError(/producer/);
  expect(() => correlateReceipt(null, hub, 'x')).toThrowError(/not an object/);
  expect(correlateReceipt({ runId: 'run-A', operationId: 'op-A', producer: 'ipc::native_terminal' }, hub, 'x')).toMatchObject({ producer: 'ipc::native_terminal' });
});

test('regression M3 deadline: unsettled waits become typed timeouts without leaks; errors propagate typed', async () => {
  const hung = withDeadline(new Promise(() => {}), 50, 'held-attach');
  const outcome = await hung;
  expect(outcome.timedOut).toBe(true);
  const settled = await withDeadline(Promise.resolve('ok'), 1000, 'fast');
  expect(settled).toMatchObject({ timedOut: false, value: 'ok' });
  const rejected = await withDeadline(Promise.reject(new Error('boom')), 1000, 'fast');
  expect(rejected.error).toBeInstanceOf(Error);
});

test('regression H4 authoritative 7-tuple: presence alone is insufficient; all 7 fields must correlate and presented must be true', () => {
  expect(ATTACH_TUPLE_FIELDS).toEqual([
    'backendSessionId',
    'incarnation',
    'daemonEpoch',
    'frontendSessionId',
    'paneIdentity',
    'bindingKey',
    'attemptGeneration',
  ]);

  const fullTuple = {
    backendSessionId: 'b-1',
    incarnation: 'inc-1',
    daemonEpoch: '1',
    frontendSessionId: 'f',
    paneIdentity: 'p',
    bindingKey: 'k',
    attemptGeneration: 3,
  };
  const fullReceipt = {
    attachTuple: fullTuple,
    presented: true,
  };

  expect(requireSevenTupleReceipt(fullReceipt, {
    backendSessionId: 'b-1',
    incarnation: 'inc-1',
    daemonEpoch: '1',
    bindingKey: 'k',
    attemptGeneration: 3,
  }, 't')).toBe(fullTuple);

  // Flat receipt (direct properties) also supported
  expect(requireSevenTupleReceipt({ ...fullTuple, presented: true }, { backendSessionId: 'b-1' }, 't')).toMatchObject(fullTuple);

  // Missing individual fields in 7-tuple
  expect(() => requireSevenTupleReceipt({ ...fullTuple, incarnation: null }, {}, 't')).toThrowError(/missing 7-tuple field 'incarnation'/);
  expect(() => requireSevenTupleReceipt({ ...fullTuple, daemonEpoch: '' }, {}, 't')).toThrowError(/missing 7-tuple field 'daemonEpoch'/);
  expect(() => requireSevenTupleReceipt({ ...fullTuple, bindingKey: null }, {}, 't')).toThrowError(/missing 7-tuple field 'bindingKey'/);

  // Mismatched correlations
  expect(() => requireSevenTupleReceipt(fullReceipt, { backendSessionId: 'b-2' }, 't')).toThrowError(/does not match expected backend/);
  expect(() => requireSevenTupleReceipt(fullReceipt, { incarnation: 'inc-2' }, 't')).toThrowError(/does not match expected incarnation/);
  expect(() => requireSevenTupleReceipt(fullReceipt, { daemonEpoch: '2' }, 't')).toThrowError(/does not match expected daemonEpoch/);
  expect(() => requireSevenTupleReceipt(fullReceipt, { attemptGeneration: 4 }, 't')).toThrowError(/does not match expected attemptGeneration/);

  // presented: false rejected
  expect(() => requireSevenTupleReceipt({ attachTuple: fullTuple, presented: false }, {}, 't')).toThrowError(/presented flag is false/);

  // frameSubmitted alone is never positive proof
  const frameOnly = { attachTuple: fullTuple, frameSubmitted: true, presented: false };
  expect(() => requireSevenTupleReceipt(frameOnly, {}, 't')).toThrowError(/presented flag is false/);
});

test('regression H3 invariants: handover/suspension/stale receipts are asserted, not logged', () => {
  expect(() => assertInvariants(['handoverPreservesIncarnation'], { originalBackendSessionId: 'b1', adoptedBackendSessionId: 'b2' })).toThrowError(/incarnation not preserved/);
  expect(() => assertInvariants(['singleReader'], { readerCount: 2 })).toThrowError(/single-reader/);
  expect(() => assertInvariants(['noDualRead'], { dualReadObserved: true })).toThrowError(/dual read/);
  expect(() => assertInvariants(['relinquishmentBeforeResume'], { relinquishmentReceiptReceived: false })).toThrowError(/relinquishment/);
  expect(() => assertInvariants(['externalStopsUntouched'], { externallyStoppedAutoResumed: true })).toThrowError(/auto-resumed/);
  expect(() => assertInvariants(['ownedResumeSameProcess'], { ownedResumePid: 1, ownedSuspendPid: 2, ownedResumed: true, verifiedActuationReceipt: true })).toThrowError(/same process/);
  expect(() => assertInvariants(['staleReceiptRejected'], { rejected: false })).toThrowError(/not explicitly rejected/);
  expect(() => assertInvariants(['reattachSameBackend'], { backendSessionId: 'b1', newPtyCreated: true })).toThrowError(/without a new PTY/);
  expect(assertInvariants(['singleReader'], { readerCount: 1 })).toBe(true);
});

test('regression H6 source digest: evidence pointer binds to runner bytes', () => {
  const digest = computeSourceDigest(['scripts/qa/pane-liveness.mjs', 'scripts/lib/qa-scenarios/common-harness.mjs']);
  expect(digest).toMatch(/^[0-9a-f]{64}$/);
});

test('BarrierHub prearm validates targetRole and awaitBound correlates bound-ack', async () => {
  const root = fixtureRoot();
  const hub = new BarrierHub(root, { runId: 'run-bind', operationId: 'op-bind' });

  // Valid targetRole: predecessor or successor
  hub.prearm('pred-barrier', { targetRole: 'predecessor' });
  const predSpec = JSON.parse(readFileSync(join(hub.dir, 'pred-barrier.arm.json'), 'utf8'));
  expect(predSpec.targetRole).toBe('predecessor');

  hub.prearm('succ-barrier', { targetRole: 'successor' });
  const succSpec = JSON.parse(readFileSync(join(hub.dir, 'succ-barrier.arm.json'), 'utf8'));
  expect(succSpec.targetRole).toBe('successor');

  // Invalid targetRole: session ID or wildcard rejected
  expect(() => hub.prearm('bad-role-1', { targetRole: 'sess-1234' }))
    .toThrowError(/targetRole must be 'predecessor' or 'successor'/);
  expect(() => hub.prearm('bad-role-2', { targetRole: '*' }))
    .toThrowError(/targetRole must be 'predecessor' or 'successor'/);

  // Live bind and awaitBound
  hub.bindBackendSession('pred-barrier', 'backend-999');
  const bindFile = JSON.parse(readFileSync(join(hub.dir, 'pred-barrier.bind.json'), 'utf8'));
  expect(bindFile.targetBackendSessionId).toBe('backend-999');

  // Simulate product-written <name>.bound-ack.json
  const boundAck = {
    name: 'pred-barrier',
    runId: 'run-bind',
    operationId: 'op-bind',
    producer: 'qa_barrier',
    producerPid: 1234,
    targetBackendSessionId: 'backend-999',
    boundAtMs: Date.now(),
  };
  writeFileSync(join(hub.dir, 'pred-barrier.bound-ack.json'), JSON.stringify(boundAck, null, 2));

  const ack = await hub.awaitBound('pred-barrier', 1000);
  expect(ack.targetBackendSessionId).toBe('backend-999');

  rmSync(root, { recursive: true, force: true });
});

// The runner pre-arms every barrier of a scenario BEFORE it launches the
// product (pane-liveness.mjs `main`), so a barrier whose mapped role `prearm()`
// refuses kills the scenario with an uncaught ASSERTION_FAILURE and exit 1
// before the binary is ever spawned - the exact failure observed for
// split-attach-stall with `BARRIER_ROLES['attach-handshake'] = 'producer'`.
// This replays that pre-arm expression for all nine scenarios without a launch.
test('regression BARRIER_ROLES: every scenario pre-arm carries a role prearm() accepts', () => {
  const root = fixtureRoot();
  const hub = new BarrierHub(root, { runId: 'run-roles', operationId: 'op-roles' });
  try {
    // targetRole is the handover-side selector only: predecessor/successor, or
    // no role at all. Nothing else may appear in the map.
    for (const [name, role] of Object.entries(BARRIER_ROLES)) {
      expect(role === null || role === 'predecessor' || role === 'successor').toBe(true);
      expect(() => hub.prearm(`map-${name}`, { targetRole: role })).not.toThrow();
    }

    for (const scenario of SCENARIOS) {
      const plan = SCENARIO_PLANS[scenario];
      expect(plan).toBeDefined();
      for (const barrier of plan.barriers) {
        // A barrier a scenario arms must be declared in the role map.
        expect(Object.prototype.hasOwnProperty.call(BARRIER_ROLES, barrier)).toBe(true);
        const targetRole = plan.barrierRoles?.[barrier] ?? BARRIER_ROLES[barrier];
        // The arm name stays filename-legal on every platform (no ':').
        expect(() => hub.prearm(`${scenario}--${barrier}`, { targetRole })).not.toThrow();
      }
    }

    // Non-handover stages carry no targetRole; handover barriers name their side.
    expect(BARRIER_ROLES['backend-write']).toBeNull();
    expect(BARRIER_ROLES['presentation']).toBeNull();
    expect(BARRIER_ROLES['attach-handshake']).toBeNull();
    expect(BARRIER_ROLES['held-rpc']).toBeNull();
    expect(BARRIER_ROLES['predecessor-export']).toBe('predecessor');
    expect(BARRIER_ROLES['successor-adopt']).toBe('successor');
    expect(BARRIER_ROLES['commit']).toBe('predecessor');
    expect(BARRIER_ROLES['abort']).toBe('successor');

    // An arm written with no role must not carry a targetRole field at all.
    const arm = JSON.parse(readFileSync(join(hub.dir, 'map-attach-handshake.arm.json'), 'utf8'));
    expect(arm).not.toHaveProperty('targetRole');
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('regression B4: post-release recovery must be positively observed; Unknown is nonpassing', () => {
  expect(assertPositiveRecovery({ classifierVerdict: 'Idle' }, 't')).toBe('Idle');
  expect(assertPositiveRecovery({ classifierVerdict: 'Healthy' }, 't')).toBe('Healthy');
  expect(() => assertPositiveRecovery({ classifierVerdict: 'Unknown', evidenceMissing: true }, 't')).toThrowError(/RECOVERY_UNPROVEN/);
  expect(() => assertPositiveRecovery({ classifierVerdict: 'Unknown' }, 't')).toThrowError(/RECOVERY_UNPROVEN/);
  expect(() => assertPositiveRecovery({ classifierVerdict: 'BlockedInPresentation' }, 't')).toThrowError(/RECOVERY_UNPROVEN/);
  expect(() => assertPositiveRecovery({}, 't')).toThrowError(/RECOVERY_UNPROVEN/);
});

test('regression B1: entrypoint module evaluates its argv-resolution path behaviorally (no source-text pin)', async () => {
  // The entrypoint guard executes resolve(process.argv[1]) at module scope,
  // so a missing `resolve` binding throws at evaluation time.
  const mod = await import('./pane-liveness.mjs');
  expect(typeof mod.requireFiveTupleReceipt).toBe('function');
  expect(typeof mod.assertInvariants).toBe('function');
  expect(typeof mod.isEntrypoint).toBe('function');
  expect(typeof mod.main).toBe('function');
  expect(mod.isEntrypoint(fileURLToPath(import.meta.url), import.meta.url)).toBe(true);
  expect(mod.isEntrypoint('/nonexistent/other/file.mjs', import.meta.url)).toBe(false);
});

test('regression B2: marker-recognition hash path executes behaviorally (no source-text pin)', async () => {
  // Drive awaitMarkerRecognition through its real code path: with every
  // earlier check satisfied, only the running createHash('sha256') over the
  // actual screenshot bytes can produce the 'binds a different screenshot'
  // verdict - proving the hash function executes and compares.
  const root = fixtureRoot();
  const hub = new BarrierHub(root, { runId: 'run-B2', operationId: 'op-B2' });
  const evidence = { action: () => {} };
  const screenshotPath = join(root, 'screenshot.png');
  const { createHash } = await import('node:crypto');
  const { writeFileSync: wf } = await import('node:fs');
  wf(screenshotPath, Buffer.from('not-really-a-png-but-hashable'));
  const wrong = {
    runId: 'run-B2', operationId: 'op-B2', recognizer: 'test-recognizer',
    text: 'FERRYX_SPLIT_READY',
    paneBounds: { x: 0, y: 0, w: 100, h: 100 },
    screenshotSha256: createHash('sha256').update(Buffer.from('different bytes entirely')).digest('hex'),
  };
  wf(join(hub.dir, 'marker-recognition.json'), JSON.stringify(wrong));
  const { awaitMarkerRecognition } = await import('../lib/qa-scenarios/native-driver.mjs');
  await expect(awaitMarkerRecognition(evidence, hub.dir, { runId: 'run-B2', operationId: 'op-B2' }, { x: 0, y: 0, w: 50, h: 50 }, screenshotPath))
    .rejects.toThrowError(/binds a different screenshot/);
  rmSync(root, { recursive: true, force: true });
});

test('regression B3: real spawnOwned group reap - leader awaits grandchild exit, absence proven once, fallback truly reclaims', async () => {
  if (process.platform === 'win32') return; // in-tree taskkill /T covers descendants there
  const root = fixtureRoot();
  const registry = new ResourceRegistry();
  const exitMarker = join(root, 'grandchild-exit.json');
  // Grandchild: announces readiness over dedicated fd 3 pipe before entering
  // interval. Inherits fd 1 (stdout) so pipe closure proves absence.
  const grandchildSrc = "require('node:fs').writeSync(3, 'grandchild-ready\\n'); setInterval(() => {}, 1000);";
  // Leader (the spawnOwned child / group leader): prearms the grandchild exit
  // observation immediately after spawn, waits for grandchild's own ready event
  // over fd 3, and only then announces READY:<pid> over stdout.
  const leaderSrc = [
    `const c = require('node:child_process').spawn(process.execPath, ['-e', ${JSON.stringify(grandchildSrc)}], { stdio: ['ignore', 'inherit', 'inherit', 'pipe'] });`,
    `const grandchildExited = new Promise(resolveExit => { c.once('exit', () => resolveExit(true)); });`,
    `c.stdio[3].once('data', chunk => {`,
    `  if (chunk.toString().includes('grandchild-ready')) {`,
    `    process.stdout.write('READY:' + c.pid + '\\n');`,
    `  }`,
    `});`,
    `process.on('SIGTERM', () => {`,
    `  grandchildExited.then(() => {`,
    `    require('node:fs').writeFileSync(${JSON.stringify(exitMarker)}, JSON.stringify({ grandchildPid: c.pid, leaderObservedExit: true }));`,
    `    process.exit(0);`,
    `  });`,
    `  c.kill('SIGTERM');`,
    `});`,
    `setInterval(() => {}, 1000);`,
  ].join(' ');
  const child = spawnOwned(registry, process.execPath, ['-e', leaderSrc]);
  // Readiness: leader receives grandchild's own ready event over fd 3 pipe,
  // prearms exit observation, and announces READY:<pid> over stdout.
  const grandchildPid = await new Promise((resolvePromise, rejectPromise) => {
    let buffer = '';
    const onData = chunk => {
      buffer += String(chunk);
      const lines = buffer.split('\n');
      buffer = lines.pop();
      for (const raw of lines) {
        const line = raw.trim();
        if (line.startsWith('READY:')) {
          child.stdout.off('data', onData);
          resolvePromise(parseInt(line.slice(6), 10));
        }
      }
    };
    child.stdout.on('data', onData);
    child.once('error', rejectPromise);
  });
  expect(Number.isInteger(grandchildPid)).toBe(true);
  // The leader's stdout pipe is inherited by the grandchild, so its 'close'
  // event fires only when BOTH processes are gone - the runner-side absence
  // signal, armed BEFORE cleanup is triggered.
  const stdoutClosed = new Promise(resolvePromise => { child.stdout.once('close', () => resolvePromise(true)); });
  // Marker watcher armed before cleanup; onStop closes the watcher on timeout
  // so nothing leaks (no polling interval anywhere).
  let markerWatcher = null;
  const markerWritten = new Promise(resolvePromise => {
    if (existsSync(exitMarker)) return resolvePromise(true);
    markerWatcher = watch(dirname(exitMarker), { persistent: true }, (event, filename) => {
      if (existsSync(exitMarker)) { markerWatcher.close(); markerWatcher = null; resolvePromise(true); }
    });
  });
  let primaryOk = false;
  let fallbackUsed = false;
  try {
    // Pre-cleanup liveness: the group AND the grandchild must be observable
    // right now, so post-cleanup absence can only mean the reap worked.
    expect(() => process.kill(-child.pid, 0)).not.toThrow();
    expect(() => process.kill(grandchildPid, 0)).not.toThrow();
    await registry.cleanup();
    expect(registry.reaped).toContain(child.pid);
    // Leader exit observed; its handler guarantees marker-after-grandchild-exit.
    const markerOutcome = await withDeadline(markerWritten, 6000, 'grandchild-exit-marker', { onStop: () => { if (markerWatcher) { markerWatcher.close(); markerWatcher = null; } } });
    expect(markerOutcome.timedOut).toBe(false);
    expect(existsSync(exitMarker)).toBe(true);
    // Pipe closure proves both fd holders (leader + grandchild) are gone.
    const closedOutcome = await withDeadline(stdoutClosed, 5000, 'leader-stdout-close');
    expect(closedOutcome.timedOut).toBe(false);
    // Absence asserted EXACTLY ONCE, only after the leader exit was observed.
    expect(() => process.kill(-child.pid, 0)).toThrow();
    expect(() => process.kill(grandchildPid, 0)).toThrow();
    primaryOk = true;
  } finally {
    // Owned fixture fallback for the mutation/broken-group-kill case: the
    // grandchild PID is fixture-owned and known to the test, so reclamation
    // does not depend on the (possibly broken) production group path. It is
    // recorded and only runs when the primary path did not complete, so a
    // mutation both fails the assertions above and still leaves no orphan.
    if (!primaryOk) {
      fallbackUsed = true;
      try { process.kill(grandchildPid, 'SIGTERM'); } catch { /* already gone */ }
      const closed = await withDeadline(stdoutClosed, 5000, 'fallback-stdout-close');
      expect(closed.timedOut).toBe(false);
      expect(() => process.kill(grandchildPid, 0)).toThrow();
    }
    if (markerWatcher) markerWatcher.close();
    try { await registry.cleanup(); } catch { /* already cleaned */ }
    rmSync(root, { recursive: true, force: true });
  }
  expect(fallbackUsed).toBe(false);
});

test('regression B5: headless diagnostic-classifier requires exact positive stageProgress; rejects missing/false progress and fake presentation', () => {
  const baseReceipt = { sessionId: 's-1', operationId: 'op-1' };

  // 1. Missing stageProgress or non-released outcome is rejected
  expect(() => assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Idle' }, { recovered: true }, 'backend-write', 'released'))
    .toThrowError(/RECOVERY_UNPROVEN/);
  expect(() => assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Idle', stageProgress: { releaseOutcome: 'held' } }, { recovered: true }, 'backend-write', 'released'))
    .toThrowError(/RECOVERY_UNPROVEN/);

  // 2. backend-write: rejects false / incomplete progress
  expect(() => assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Idle', stageProgress: { releaseOutcome: 'released', backendWriteCompleted: false, success: true } }, { recovered: true }, 'backend-write', 'released'))
    .toThrowError(/RECOVERY_UNPROVEN/);
  expect(() => assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Idle', stageProgress: { releaseOutcome: 'released', backendWriteCompleted: true, success: false } }, { recovered: true }, 'backend-write', 'released'))
    .toThrowError(/RECOVERY_UNPROVEN/);

  // backend-write: accepts Idle/Healthy or truthful Unknown with evidenceMissing: true
  const validWriteProgress = { releaseOutcome: 'released', backendWriteCompleted: true, success: true, durationMs: 5 };
  expect(assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Idle', stageProgress: validWriteProgress }, { recovered: true }, 'backend-write', 'released'))
    .toBe('Idle');
  expect(assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Unknown', evidenceMissing: true, stageProgress: validWriteProgress }, { recovered: true }, 'backend-write', 'released'))
    .toBe('Unknown');
  // Rejects generic Unknown (evidenceMissing !== true) even with positive stageProgress
  expect(() => assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Unknown', stageProgress: validWriteProgress }, { recovered: true }, 'backend-write', 'released'))
    .toThrowError(/RECOVERY_UNPROVEN/);

  // 3. presentation: rejects false / incomplete progress
  expect(() => assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Idle', presentationEvidence: 'coordinator-consumed', stageProgress: { releaseOutcome: 'released', frameConsumed: false, renderPendingAfter: false } }, { recovered: true }, 'presentation', 'released'))
    .toThrowError(/RECOVERY_UNPROVEN/);
  expect(() => assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Idle', presentationEvidence: 'coordinator-consumed', stageProgress: { releaseOutcome: 'released', frameConsumed: true, renderPendingAfter: true } }, { recovered: true }, 'presentation', 'released'))
    .toThrowError(/RECOVERY_UNPROVEN/);

  // presentation: rejects fake presentation (frame-submitted) or missing presentationEvidence
  expect(() => assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Idle', presentationEvidence: 'frame-submitted', stageProgress: { releaseOutcome: 'released', frameConsumed: true, renderPendingAfter: false } }, { recovered: true }, 'presentation', 'released'))
    .toThrowError(/RECOVERY_UNPROVEN/);
  expect(() => assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Idle', stageProgress: { releaseOutcome: 'released', frameConsumed: true, renderPendingAfter: false } }, { recovered: true }, 'presentation', 'released'))
    .toThrowError(/RECOVERY_UNPROVEN/);

  // presentation: accepts Idle/Healthy or truthful Unknown with evidenceMissing: true and coordinator-consumed
  const validPresentationProgress = { releaseOutcome: 'released', frameConsumed: true, renderPendingAfter: false };
  expect(assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Idle', presentationEvidence: 'coordinator-consumed', stageProgress: validPresentationProgress }, { recovered: true }, 'presentation', 'released'))
    .toBe('Idle');
  expect(assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Unknown', evidenceMissing: true, presentationEvidence: 'coordinator-consumed', stageProgress: validPresentationProgress }, { recovered: true }, 'presentation', 'released'))
    .toBe('Unknown');
  // Rejects generic Unknown (evidenceMissing !== true) even with coordinator-consumed
  expect(() => assertClassifierReceipt({ ...baseReceipt, classifierVerdict: 'Unknown', presentationEvidence: 'coordinator-consumed', stageProgress: validPresentationProgress }, { recovered: true }, 'presentation', 'released'))
    .toThrowError(/RECOVERY_UNPROVEN/);
});

// ---------------------------------------------------------------------------
// Task 7 Scenario Adapter and Runner Lifecycle Tests (authored; never run locally)
// ---------------------------------------------------------------------------

test('MonotonicBudget enforces monotonic consumption and deadline exhaustion', () => {
  const budget = new MonotonicBudget(500);
  expect(budget.totalMs).toBe(500);
  expect(budget.remainingMs()).toBeGreaterThan(0);
  expect(budget.remainingMs()).toBeLessThanOrEqual(500);
  expect(budget.remainingMs(100)).toBeLessThanOrEqual(100);
  expect(budget.consume(100, 'test-op')).toBeLessThanOrEqual(100);
  expect(budget.elapsedMs()).toBeGreaterThanOrEqual(0);
  expect(budget.isExceeded()).toBe(false);

  const expired = new MonotonicBudget(-1);
  expect(expired.remainingMs()).toBe(0);
  expect(expired.isExceeded()).toBe(true);
  expect(() => expired.consume(50, 'expired-op')).toThrowError(/monotonic budget exhausted/);
});

test('validateFixtureSetup enforces scenario-specific fixture requirements without forcing 4 fixtures on basic split', () => {
  // Basic split scenarios accept only 'source' (or created)
  const basicFixture = {
    sessions: [
      { kind: 'source', backendSessionId: 'b-source', ownershipReceipt: { owned: true } },
    ],
  };
  const valid = validateFixtureSetup(basicFixture, 'split-happy');
  expect(valid.sessions).toHaveLength(1);
  expect(valid.byKind.get('source')).toHaveLength(1);

  // suspension-ownership requires 'created', 'externally-stopped', 'idle'
  expect(() => validateFixtureSetup(basicFixture, 'suspension-ownership')).toThrowError(/fixture-setup requires at least one/);

  const suspensionFixture = {
    sessions: [
      { kind: 'created', backendSessionId: 'b-1', ownershipReceipt: { owned: true } },
      { kind: 'externally-stopped', backendSessionId: 'b-2', ownershipReceipt: { owned: true }, stopProbeState: 'stopped' },
      { kind: 'idle', backendSessionId: 'b-3', ownershipReceipt: { owned: true } },
    ],
  };
  expect(validateFixtureSetup(suspensionFixture, 'suspension-ownership').sessions).toHaveLength(3);

  // Missing stopProbeState on externally-stopped
  const invalidStopped = {
    sessions: [
      { kind: 'created', backendSessionId: 'b-1', ownershipReceipt: { owned: true } },
      { kind: 'externally-stopped', backendSessionId: 'b-2', ownershipReceipt: { owned: true }, stopProbeState: 'running' },
      { kind: 'idle', backendSessionId: 'b-3', ownershipReceipt: { owned: true } },
    ],
  };
  expect(() => validateFixtureSetup(invalidStopped, 'suspension-ownership')).toThrowError(/lacks stop evidence/);

  // Missing backendSessionId or ownershipReceipt
  expect(() => validateFixtureSetup({ sessions: [{ kind: 'source', backendSessionId: '', ownershipReceipt: {} }] }, 'split-happy'))
    .toThrowError(/lacks backendSessionId\/ownershipReceipt/);
  expect(() => validateFixtureSetup({ sessions: [{ kind: 'source', backendSessionId: 'b-1', ownershipReceipt: null }] }, 'split-happy'))
    .toThrowError(/lacks backendSessionId\/ownershipReceipt/);
});

test('performInspectionHandshake writes capture-ready.json and verifies matching inspection artifact', async () => {
  const root = fixtureRoot();
  const hub = new BarrierHub(root, { runId: 'run-handshake', operationId: 'op-handshake' });
  const evidence = { action: () => {} };
  const screenshotPath = join(root, 'screenshot.png');
  writeFileSync(screenshotPath, Buffer.from('fake-screenshot-data'));
  const actualSha = computeSourceDigest([screenshotPath]);

  const screenshotMetadata = {
    path: screenshotPath,
    screenshotSha256: actualSha,
    windowBounds: { x: 0, y: 0, width: 800, height: 600 },
    targetPaneBounds: { x: 400, y: 0, width: 400, height: 600 },
  };

  // Valid handshake: write matching marker-recognition.json
  const validArtifact = {
    runId: 'run-handshake',
    operationId: 'op-handshake',
    recognizer: 'independent-test-inspector',
    text: MARKER_TEXT,
    paneBounds: { x: 400, y: 0, w: 400, h: 600 },
    screenshotSha256: actualSha,
  };
  writeFileSync(join(hub.dir, 'marker-recognition.json'), JSON.stringify(validArtifact, null, 2));

  const res = await performInspectionHandshake(
    evidence,
    hub,
    { runId: 'run-handshake', operationId: 'op-handshake' },
    screenshotMetadata,
    1000
  );
  expect(res.verified).toBe(true);
  expect(res.recognizer).toBe('independent-test-inspector');

  // Verify capture-ready.json was written with matching metadata
  const captureReady = JSON.parse(readFileSync(join(hub.dir, 'capture-ready.json'), 'utf8'));
  expect(captureReady).toMatchObject({
    runId: 'run-handshake',
    operationId: 'op-handshake',
    screenshotSha256: actualSha,
  });

  // Rejection on mismatched sha256
  writeFileSync(join(hub.dir, 'marker-recognition.json'), JSON.stringify({ ...validArtifact, screenshotSha256: 'deadbeef'.repeat(8) }));
  await expect(performInspectionHandshake(evidence, hub, { runId: 'run-handshake', operationId: 'op-handshake' }, screenshotMetadata, 1000))
    .rejects.toThrowError(/binds a different screenshot/);

  // Rejection on mismatched runId
  writeFileSync(join(hub.dir, 'marker-recognition.json'), JSON.stringify({ ...validArtifact, runId: 'wrong-run' }));
  await expect(performInspectionHandshake(evidence, hub, { runId: 'run-handshake', operationId: 'op-handshake' }, screenshotMetadata, 1000))
    .rejects.toThrowError(/runId mismatch/);

  // Rejection on mismatched text
  writeFileSync(join(hub.dir, 'marker-recognition.json'), JSON.stringify({ ...validArtifact, text: 'WRONG_MARKER' }));
  await expect(performInspectionHandshake(evidence, hub, { runId: 'run-handshake', operationId: 'op-handshake' }, screenshotMetadata, 1000))
    .rejects.toThrowError(/does not equal expected marker/);

  rmSync(root, { recursive: true, force: true });
});

test('split-cancel scenario adapter issues cancel before awaiting create and accepts cancel-ack without requiring created ID', async () => {
  const root = fixtureRoot();
  const hub = new BarrierHub(root, { runId: 'run-cancel', operationId: 'op-cancel' });
  const evidence = { action: () => {} };
  const commandsSent = [];
  const fakeHub = {
    ...hub,
    command: (name, payload) => {
      commandsSent.push({ name, payload });
      return hub.command(name, payload);
    },
    awaitReceipt: async (name) => {
      if (name === 'cancel-ack') {
        return {
          cancelAckMs: 120,
          timerDispatchLatencyMs: 5,
          cleanupReceipt: { authoritative: true, reapedPids: [] },
          createdIdRequired: false,
        };
      }
      throw new Error(`unexpected awaitReceipt ${name}`);
    },
  };

  const fakeCtx = {
    scenario: 'split-cancel',
    evidence,
    barrierHub: fakeHub,
    pid: 1234,
    platformPreflight: 'mock',
  };

  const result = await runSplitCancelScenario(fakeCtx, { cancel: { phase: 'while-creating' } }, new MonotonicBudget());
  expect(result.cancelReceipt.cancelAckMs).toBe(120);
  expect(result.cancelReceipt.cleanupReceipt.authoritative).toBe(true);
  expect(commandsSent.some(c => c.name === 'split-cancel')).toBe(true);

  rmSync(root, { recursive: true, force: true });
});

test('split-attach-stall asserts actionable failure, releases hold, and executes Retry with NEW attemptGeneration on same backend', async () => {
  const root = fixtureRoot();
  const hub = new BarrierHub(root, { runId: 'run-stall', operationId: 'op-stall' });
  const actions = [];
  const fakeHub = {
    ...hub,
    isArmed: (name) => name === 'presentation',
    bindBackendSession: () => {},
    release: (name) => actions.push(`release:${name}`),
    command: (name, payload) => actions.push({ name, payload }),
    awaitHeld: async () => ({ sessionId: 'b-stall-1', heldRpc: false }),
    awaitReceipt: async (name) => {
      if (name === 'split-create') {
        return {
          backendSessionId: 'b-stall-1',
          incarnation: 'inc-stall',
          daemonEpoch: '1',
          attemptGeneration: 1, // Initial attempt generation is 1
        };
      }
      if (name === 'attach-handshake') {
        return { actionable: true, retryMustReuseId: true, backendSessionId: 'b-stall-1', failureDeliveredAtMs: 250 };
      }
      if (name === 'presentation') {
        // Presentation following Retry must verify the NEW attemptGeneration (2)
        return {
          attachTuple: {
            backendSessionId: 'b-stall-1',
            incarnation: 'inc-stall',
            daemonEpoch: '1',
            frontendSessionId: 'f-stall',
            paneIdentity: 'p-stall',
            bindingKey: 'k-stall',
            attemptGeneration: 2, // Incremented new generation!
          },
          presented: true,
        };
      }
      if (name === 'marker-output') {
        return { output: MARKER_TEXT, ptyCreatedCount: 1, presented: true };
      }
      throw new Error(`unexpected receipt ${name}`);
    },
    recordCaptureReady: (meta) => hub.recordCaptureReady(meta),
  };

  const screenshotPath = join(root, 'screenshot.png');
  writeFileSync(screenshotPath, Buffer.from('stall-screenshot'));
  const actualSha = computeSourceDigest([screenshotPath]);
  writeFileSync(join(hub.dir, 'marker-recognition.json'), JSON.stringify({
    runId: 'run-stall',
    operationId: 'op-stall',
    recognizer: 'test-inspector',
    text: MARKER_TEXT,
    paneBounds: { x: 0, y: 0, w: 500, h: 500 },
    screenshotSha256: actualSha,
  }));

  const fakeCtx = {
    scenario: 'split-attach-stall',
    evidence: { action: (a) => actions.push(a.action) },
    barrierHub: fakeHub,
    pid: 1234,
    platformPreflight: 'mock',
    evidenceRunDir: root,
    runId: 'run-stall',
    operationId: 'op-stall',
  };

  const plan = { barrierHoldMs: 16000 };
  const res = await runSplitAttachStallScenario(fakeCtx, plan, new MonotonicBudget());
  expect(res.failureReceipt.actionable).toBe(true);
  expect(actions).toContain('barrier.released');
  expect(actions).toContain('retry-click');
  // Verify that retry command carried NEW attemptGeneration: 2 and retained backendSessionId: 'b-stall-1'
  const retryCmd = actions.find(a => a?.name === 'retry');
  expect(retryCmd.payload).toMatchObject({
    backendSessionId: 'b-stall-1',
    attemptGeneration: 2,
    incarnation: 'inc-stall',
  });
  expect(res.markerRecognition.verified).toBe(true);

  rmSync(root, { recursive: true, force: true });
});

test('retained-handover and handover-abort adapters verify transfer and relinquishment invariants', async () => {
  const root = fixtureRoot();
  const hub = new BarrierHub(root, { runId: 'run-ho', operationId: 'op-ho' });
  const screenshotPath = join(root, 'screenshot.png');
  writeFileSync(screenshotPath, Buffer.from('ho-screenshot'));
  const actualSha = computeSourceDigest([screenshotPath]);
  writeFileSync(join(hub.dir, 'marker-recognition.json'), JSON.stringify({
    runId: 'run-ho',
    operationId: 'op-ho',
    recognizer: 'ho-inspector',
    text: MARKER_TEXT,
    paneBounds: { x: 0, y: 0, w: 600, h: 600 },
    screenshotSha256: actualSha,
  }));

  const fakeHub = {
    ...hub,
    command: () => {},
    recordCaptureReady: (meta) => hub.recordCaptureReady(meta),
    awaitReceipt: async (name) => {
      if (name === 'marker-output') return { output: MARKER_TEXT, frameSubmitted: true, ptyCreatedCount: 1 };
      if (name === 'handover-transfer') {
        return {
          originalBackendSessionId: 'b-orig',
          adoptedBackendSessionId: 'b-orig',
          originalIncarnation: 'inc-1',
          adoptedIncarnation: 'inc-1',
          readerCount: 1,
        };
      }
      if (name === 'rollback-relinquishment') {
        return {
          relinquishmentReceiptReceived: true,
          successorReaderReleased: true,
          readerCount: 1,
          dualReadObserved: false,
        };
      }
      throw new Error(`unexpected ${name}`);
    },
  };

  const fakeCtx = {
    evidence: { action: () => {} },
    barrierHub: fakeHub,
    pid: 5678,
    platformPreflight: 'mock',
    evidenceRunDir: root,
    runId: 'run-ho',
    operationId: 'op-ho',
  };

  const hoRes = await runRetainedHandoverScenario(fakeCtx, {}, new MonotonicBudget());
  expect(hoRes.transferReceipt.readerCount).toBe(1);

  const abortRes = await runHandoverAbortScenario(fakeCtx, {}, new MonotonicBudget());
  expect(abortRes.rollbackReceipt.relinquishmentReceiptReceived).toBe(true);

  rmSync(root, { recursive: true, force: true });
});

test('suspension-ownership and stale-binding adapters verify process actuation and reattach invariants', async () => {
  const root = fixtureRoot();
  const hub = new BarrierHub(root, { runId: 'run-so', operationId: 'op-so' });
  const screenshotPath = join(root, 'screenshot.png');
  writeFileSync(screenshotPath, Buffer.from('so-screenshot'));
  const actualSha = computeSourceDigest([screenshotPath]);
  writeFileSync(join(hub.dir, 'marker-recognition.json'), JSON.stringify({
    runId: 'run-so',
    operationId: 'op-so',
    recognizer: 'so-inspector',
    text: MARKER_TEXT,
    paneBounds: { x: 0, y: 0, w: 600, h: 600 },
    screenshotSha256: actualSha,
  }));

  const fakeHub = {
    ...hub,
    command: () => {},
    recordCaptureReady: (meta) => hub.recordCaptureReady(meta),
    awaitReceipt: async (name) => {
      if (name === 'suspension-receipt') {
        return {
          externallyStoppedAutoResumed: false,
          externallyStoppedProbeState: 'stopped',
          ownedResumePid: 9999,
          ownedSuspendPid: 9999,
          ownedResumed: true,
          verifiedActuationReceipt: true,
        };
      }
      if (name === 'stale-receipt-rejected') {
        return { rejected: true, reason: 'attemptGeneration mismatch: stale token' };
      }
      if (name === 'reattach-marker') {
        return { backendSessionId: 'b-same', newPtyCreated: false };
      }
      if (name === 'marker-output') {
        return { output: MARKER_TEXT, frameSubmitted: true, ptyCreatedCount: 1 };
      }
      throw new Error(`unexpected ${name}`);
    },
  };

  const fakeCtx = {
    evidence: { action: () => {} },
    barrierHub: fakeHub,
    pid: 9999,
    platformPreflight: 'mock',
    evidenceRunDir: root,
    runId: 'run-so',
    operationId: 'op-so',
  };

  const soRes = await runSuspensionOwnershipScenario(fakeCtx, {}, new MonotonicBudget());
  expect(soRes.suspensionReceipt.ownedResumed).toBe(true);

  const staleRes = await runStaleBindingScenario(fakeCtx, {}, new MonotonicBudget());
  expect(staleRes.rejectedReceipt.rejected).toBe(true);
  expect(staleRes.reattachReceipt.newPtyCreated).toBe(false);

  rmSync(root, { recursive: true, force: true });
});

test('source digest resolves relative paths against an injected base and preserves absolute paths', async () => {
  const root = fixtureRoot();
  const path = join(root, 'source.mjs');
  const bytes = Buffer.from('export const value = 1;');
  const { createHash } = await import('node:crypto');
  writeFileSync(path, bytes);
  try {
    const expected = createHash('sha256').update(bytes).digest('hex');
    expect(computeSourceDigest(['source.mjs'], root)).toBe(expected);
    expect(computeSourceDigest([path])).toBe(expected);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('driver dispatch is explicit, mock-safe, and accepts an injected adapter driver', async () => {
  const native = await import('../lib/qa-scenarios/native-driver.mjs');
  expect(native.selectNativeDriver({ platformPreflight: 'darwin' }).focus).toBe(native.focusWindowByPidDarwin);
  expect(native.selectNativeDriver({ platformPreflight: 'win32' }).split).toBe(native.windowsDriver);

  const root = fixtureRoot();
  const path = join(root, 'screenshot.png');
  writeFileSync(path, Buffer.from('fixture-screenshot'));
  try {
    for (const platformPreflight of ['mock', 'linux', undefined]) {
      const driver = native.selectNativeDriver({ platformPreflight });
      const evidence = { action: () => { throw new Error('mock driver emitted a native action'); } };
      await driver.focus(evidence, 1234);
      await driver.split(evidence, 1234);
      await driver.typeMarker(evidence, 1234);
      await driver.retry(evidence, 1234);
      expect(await driver.capture(evidence, path, 1234)).toEqual({ path, screenshotSha256: computeSourceDigest([path]) });
      await expect(driver.capture(evidence, join(root, 'missing.png'), 1234)).rejects.toThrow();
    }

    const calls = [];
    const driver = {
      focus: async () => calls.push('focus'),
      split: async () => calls.push('split'),
    };
    expect(native.selectNativeDriver({ platformPreflight: 'darwin', nativeDriver: driver })).toBe(driver);
    const result = await runSplitCancelScenario({
      platformPreflight: 'darwin', nativeDriver: driver, pid: 1234,
      evidence: { action: () => {} },
      barrierHub: {
        command: () => calls.push('cancel'),
        awaitReceipt: async () => {
          calls.push('receipt');
          return { cancelAckMs: 1, cleanupReceipt: { authoritative: true } };
        },
      },
    }, { cancel: { phase: 'before-create' } });
    expect(result.cancelReceipt.cleanupReceipt.authoritative).toBe(true);
    expect(calls).toEqual(['focus', 'split', 'cancel', 'receipt', 'cancel']);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('private channel env carries the operation nonce so barrier-less scenarios can correlate', () => {
  const root = fixtureRoot();
  const hub = new BarrierHub(root, { runId: 'run-env', operationId: 'op-env' });
  expect(hub.env()).toMatchObject({
    FERRYX_QA_BARRIER_DIR: hub.dir,
    FERRYX_QA_RUN_ID: 'run-env',
    FERRYX_QA_OPERATION_ID: 'op-env',
  });
  // The isolated launch env refuses every FERRYX_* key outside its allowlist, so
  // a successful build proves the operation nonce is allowlisted too.
  const isolated = buildIsolatedEnv({ isolationRoot: root, barrierHub: hub });
  expect(isolated.env.FERRYX_QA_OPERATION_ID).toBe('op-env');
  // No nonce: the key is omitted so the headless lane's env is unchanged.
  const bare = new BarrierHub(join(root, 'bare'), { runId: 'run-bare' });
  expect(bare.env().FERRYX_QA_OPERATION_ID).toBeUndefined();
  rmSync(root, { recursive: true, force: true });
});

// ---------------------------------------------------------------------------
// Pass-4 blockers (Windows interactive desktop lane). These replay the runner's
// own expressions; nothing here launches a product, a PowerShell probe, or a
// scheduled task.

test('windows session admission relaunches into the active console session or fails typed', async () => {
  const { classifyWindowsSession, asArray } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  // Measured pass-4 shape: SSH lands in session 0 (`services`, Disc) while
  // session 1 (`console`/sook) is Active with explorer/winlogon/dwm.
  const session0 = {
    probe: 'windows-session', interactive: false, sessionId: 0,
    activeConsoleSessionId: 1, explorerSessions: [1],
    qwinsta: [' services                0  Disc', ' console      sook        1  Active'],
  };
  expect(classifyWindowsSession(session0)).toMatchObject({ interactive: false, sessionId: 0, consoleSessionId: 1, code: null });
  // A relaunch was already attempted and this process is still non-interactive:
  // typed block, never a second relaunch (no loop).
  expect(classifyWindowsSession(session0, { alreadyRelaunched: true }).code).toBe('NO_INTERACTIVE_SESSION');
  // No active console session at all.
  expect(classifyWindowsSession({ probe: 'windows-session', interactive: false, sessionId: 0, activeConsoleSessionId: null, explorerSessions: [] }).code)
    .toBe('NO_INTERACTIVE_SESSION');
  // The only "console" session is this very non-interactive session.
  expect(classifyWindowsSession({ interactive: false, sessionId: 0, activeConsoleSessionId: 0, explorerSessions: [0] }).code)
    .toBe('NO_INTERACTIVE_SESSION');
  // A malformed/absent probe can never be read as interactive.
  expect(classifyWindowsSession({}).code).toBe('NO_INTERACTIVE_SESSION');
  expect(classifyWindowsSession({ interactive: 'true', sessionId: 1 }).interactive).toBe(false);
  // The delegated run itself (session 1) is interactive.
  expect(classifyWindowsSession({ interactive: true, sessionId: 1, activeConsoleSessionId: null }).interactive).toBe(true);
  // A process whose own session IS the active console session is on the
  // interactive desktop even if the UserInteractive API reports a quirk; the
  // owned-window wait remains the stronger second gate.
  expect(classifyWindowsSession({ interactive: false, sessionId: 1, activeConsoleSessionId: 1 }).interactive).toBe(true);
  // Session 0 is never interactive, whatever the API reports.
  expect(classifyWindowsSession({ interactive: true, sessionId: 0, activeConsoleSessionId: 0, explorerSessions: [0] }).interactive).toBe(false);
  // PowerShell single-element arrays are normalized at the boundary.
  expect(asArray(undefined)).toEqual([]);
  expect(asArray(1)).toEqual([1]);
  expect(asArray([1])).toEqual([1]);
});

test('windows interactive admission is inert off win32 and on the headless lane', async () => {
  const { admitWindowsInteractiveDesktop } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  // macOS keeps its existing launch path: admission never probes or relaunches.
  expect(await admitWindowsInteractiveDesktop({ invocation: { headless: false }, context: { platform: 'darwin' }, rawArgv: [] })).toBeNull();
  // The headless lane never touches a GUI window, so it is never relaunched.
  expect(await admitWindowsInteractiveDesktop({ invocation: { headless: true }, context: { platform: 'win32' }, rawArgv: [] })).toBeNull();
});

test('windows probe stdout is parsed tolerantly and never invented', async () => {
  const { parseWindowsProbeLine, parseRelaunchExitFile } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  expect(parseWindowsProbeLine('noise\n{"probe":"owned-window","ok":true}\n', 'owned-window'))
    .toMatchObject({ ok: true, probe: { ok: true } });
  expect(parseWindowsProbeLine('{"probe":"split-right"}', 'owned-window').ok).toBe(false);
  expect(parseWindowsProbeLine('', 'owned-window').ok).toBe(false);
  expect(parseWindowsProbeLine('null', 'owned-window').ok).toBe(false);
  expect(parseWindowsProbeLine('[1,2]', 'owned-window').ok).toBe(false);
  expect(parseRelaunchExitFile(' 1\r\n')).toBe(1);
  expect(parseRelaunchExitFile('0')).toBe(0);
  expect(parseRelaunchExitFile('')).toBeNull();
  expect(parseRelaunchExitFile('ErrorLevel 1')).toBeNull();
});

test('interactive relaunch plan keeps the frozen argv and uses the proven scheduled-task mechanism', async () => {
  const { buildInteractiveRelaunchPlan, WINDOWS_INTERACTIVE_RELAUNCH_ENV, WINDOWS_RELAUNCH_RECORD_ENV } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const runnerArgs = [
    '--scenario', 'split-happy',
    '--binary', 'C:\\repo\\src-tauri\\target\\debug\\ferryx.exe',
    '--evidence-dir', 'C:\\ev',
    '--isolation-root', 'C:\\iso',
  ];
  const plan = buildInteractiveRelaunchPlan({
    taskName: 'ferryx-qa-split-happy-1-abc123',
    batDir: 'C:\\relaunch',
    nodePath: 'C:\\Program Files\\nodejs\\node.exe',
    runnerPath: 'C:\\repo\\scripts\\qa\\pane-liveness.mjs',
    runnerArgs,
    cwd: 'C:\\repo',
  });
  // The verifier's proven recipe: /it binds the task to the interactive console user.
  expect(plan.createArgs).toEqual(['/create', '/tn', 'ferryx-qa-split-happy-1-abc123', '/tr', plan.batPath, '/sc', 'once', '/st', '00:00', '/f', '/it']);
  expect(plan.runArgs).toEqual(['/run', '/tn', 'ferryx-qa-split-happy-1-abc123']);
  expect(plan.deleteArgs).toEqual(['/delete', '/tn', 'ferryx-qa-split-happy-1-abc123', '/f']);
  // The scenario argv shape is passed through unchanged, argument for argument.
  expect(plan.command).toBe(
    '"C:\\Program Files\\nodejs\\node.exe" "C:\\repo\\scripts\\qa\\pane-liveness.mjs" "--scenario" "split-happy" "--binary" "C:\\repo\\src-tauri\\target\\debug\\ferryx.exe" "--evidence-dir" "C:\\ev" "--isolation-root" "C:\\iso"',
  );
  expect(plan.batBody).toContain(`set ${WINDOWS_INTERACTIVE_RELAUNCH_ENV}=1`);
  expect(plan.batBody).toContain(`set ${WINDOWS_RELAUNCH_RECORD_ENV}=${plan.recordPath}`);
  expect(plan.batBody).toContain('cd /d "C:\\repo"');
  expect(plan.batBody).toContain(`echo %ERRORLEVEL% > "${plan.exitPath}"`);
});

test('owned-window wait fails typed with the measured session id and window visibility', async () => {
  const { classifyWindowsWindowProbe } = await import('../lib/qa-scenarios/native-driver.mjs');
  const session0 = {
    probe: 'owned-window', pid: 3924, interactive: false, sessionId: 0, budgetMs: 8000, waitedMs: 8000,
    mainWindowHandle: 0, processExited: false, ok: false,
    windows: [
      { hwnd: 24838986, visible: false, className: 'T', title: 'F' },
      { hwnd: 107151794, visible: false, className: 'T', title: '' },
      { hwnd: 8389492, visible: false, className: 'C', title: 'C' },
    ],
  };
  const nonInteractive = classifyWindowsWindowProbe(session0);
  expect(nonInteractive.ok).toBe(false);
  expect(nonInteractive.code).toBe('NO_INTERACTIVE_SESSION');
  expect(nonInteractive.detail).toContain('sessionId=0');
  expect(nonInteractive.detail).toContain('"visible":false');
  // An interactive session that never shows an owned window is the distinct code.
  const noWindow = classifyWindowsWindowProbe({ ...session0, interactive: true, sessionId: 1 });
  expect(noWindow.code).toBe('NO_OWNED_WINDOW');
  expect(noWindow.detail).toContain('mainWindowHandle=0');
  // A real owned, visible window passes.
  const ready = classifyWindowsWindowProbe({
    ...session0, interactive: true, sessionId: 1, waitedMs: 900, mainWindowHandle: 8389492, ok: true,
    windows: [{ hwnd: 8389492, visible: true, className: 'T', title: 'Ferryx' }],
  });
  expect(ready.ok).toBe(true);
  expect(ready.mainWindowHandle).toBe(8389492);
  // A zero handle can never pass, whatever the probe claims.
  expect(classifyWindowsWindowProbe({ ...session0, ok: true }).ok).toBe(false);
});

test('split-right selection is scoped to the focused pane and never picks the first match', async () => {
  const { classifyWindowsSplitRight } = await import('../lib/qa-scenarios/native-driver.mjs');
  const base = {
    probe: 'split-right', selector: 'Split pane right', interactive: true, sessionId: 1,
    mainWindowHandle: 8389492, windowVisible: true, focusedFound: true,
    focusSource: 'pane-focus-sink', scopeDepth: 3, scopeIsWindowRoot: false,
  };
  const candidate = (index, extra = {}) => ({
    index, name: 'Split pane right', controlType: 'ControlType.Button', automationId: '',
    enabled: true, offscreen: false, rectEmpty: false, rect: '10,10,20,20', inWindow: true, ...extra,
  });
  // Exactly one actionable affordance inside the focused pane scope: click it.
  const unique = classifyWindowsSplitRight({ ...base, result: 'SPLIT_CLICKED', candidateCount: 1, actionableCount: 1, candidates: [candidate(0)], chosen: candidate(0) });
  expect(unique.ok).toBe(true);
  expect(unique.chosen.index).toBe(0);
  // The pass-4 blocker: more than one actionable match in the pane scope stays
  // typed and is never resolved by taking the first match.
  const ambiguous = classifyWindowsSplitRight({ ...base, candidateCount: 2, actionableCount: 2, candidates: [candidate(0), candidate(1)] });
  expect(ambiguous.ok).toBe(false);
  expect(ambiguous.code).toBe('SPLIT_RIGHT_NOT_UNIQUE');
  expect(ambiguous.candidates).toHaveLength(2);
  // Present but not actionable (disabled / offscreen / empty rect / outside the window).
  expect(classifyWindowsSplitRight({ ...base, candidateCount: 1, actionableCount: 0, candidates: [candidate(0, { enabled: false })] }).code).toBe('SPLIT_RIGHT_DISABLED');
  expect(classifyWindowsSplitRight({ ...base, candidateCount: 1, actionableCount: 0, candidates: [candidate(0, { offscreen: true })] }).code).toBe('SPLIT_RIGHT_DISABLED');
  // Absent, or no focused pane identified: never a click.
  expect(classifyWindowsSplitRight({ ...base, candidateCount: 0, actionableCount: 0, candidates: [] }).code).toBe('SPLIT_RIGHT_NOT_FOUND');
  expect(classifyWindowsSplitRight({ ...base, focusedFound: false, candidateCount: 0, actionableCount: 0, candidates: [] }).code).toBe('SPLIT_RIGHT_NOT_FOUND');
  // Window scope: a handle that is zero or not visible fails closed.
  expect(classifyWindowsSplitRight({ ...base, mainWindowHandle: 0, windowVisible: false, candidateCount: 1, actionableCount: 1, candidates: [candidate(0)] }).code).toBe('NO_OWNED_WINDOW');
  expect(classifyWindowsSplitRight({ ...base, windowVisible: false, candidateCount: 1, actionableCount: 1, candidates: [candidate(0)] }).code).toBe('NO_OWNED_WINDOW');
  // The probe's own typed verdict is honoured, and a divergence from the
  // measured shape is disclosed in the recorded detail.
  const declared = classifyWindowsSplitRight({ ...base, candidateCount: 2, actionableCount: 2, candidates: [candidate(0), candidate(1)], failure: 'SPLIT_RIGHT_DISABLED', detail: 'InvokePattern failed: not invokable' });
  expect(declared.code).toBe('SPLIT_RIGHT_DISABLED');
  expect(declared.derivedCode).toBe('SPLIT_RIGHT_NOT_UNIQUE');
  expect(declared.detail).toContain('measured shape derives SPLIT_RIGHT_NOT_UNIQUE');
});

test('windows automation failures keep their typed identity instead of collapsing', async () => {
  const native = await import('../lib/qa-scenarios/native-driver.mjs');
  expect(native.classifyWindowsFailure('SPLIT_RIGHT_NOT_UNIQUE\r\n  + CategoryInfo ...')).toBe('SPLIT_RIGHT_NOT_UNIQUE');
  expect(native.classifyWindowsFailure('SPLIT_RIGHT_NOT_FOUND')).toBe('SPLIT_RIGHT_NOT_FOUND');
  expect(native.classifyWindowsFailure('SPLIT_RIGHT_DISABLED')).toBe('SPLIT_RIGHT_DISABLED');
  expect(native.classifyWindowsFailure('... NO_OWNED_WINDOW ...')).toBe('NO_OWNED_WINDOW');
  expect(native.classifyWindowsFailure('... NO_INTERACTIVE_SESSION ...')).toBe('NO_INTERACTIVE_SESSION');
  expect(native.classifyWindowsFailure('RETRY_BUTTON_NOT_FOUND')).toBe('NATIVE_AUTOMATION_UNSUPPORTED');
  expect(native.classifyWindowsFailure('')).toBe('NATIVE_AUTOMATION_UNSUPPORTED');
});

test('windows lane blocks are typed, nonzero, and never a pass', async () => {
  const runner = await import('./pane-liveness.mjs');
  const windowsCodes = [
    'NO_INTERACTIVE_SESSION', 'NO_OWNED_WINDOW', 'INTERACTIVE_RELAUNCH_FAILED',
    'SPLIT_RIGHT_NOT_FOUND', 'SPLIT_RIGHT_NOT_UNIQUE', 'SPLIT_RIGHT_DISABLED',
  ];
  for (const code of windowsCodes) {
    expect(runner.classifyNativeFailure(code)).toEqual({ verdict: 'BLOCKED', exitCode: EXIT.nativeAutomationUnsupported });
    expect(runner.classifyNativeFailure(code).exitCode).not.toBe(0);
    // The typed code survives HarnessError instead of collapsing to ASSERTION_FAILURE.
    expect(new HarnessError(code, 'detail').code).toBe(code);
  }
  // Every pre-existing mapping is unchanged.
  expect(runner.classifyNativeFailure('AX_UNTRUSTED')).toEqual({ verdict: 'BLOCKED', exitCode: EXIT.axUntrusted });
  expect(runner.classifyNativeFailure('CAPTURE_DENIED')).toEqual({ verdict: 'BLOCKED', exitCode: EXIT.captureDenied });
  expect(runner.classifyNativeFailure('NATIVE_AUTOMATION_UNSUPPORTED')).toEqual({ verdict: 'BLOCKED', exitCode: EXIT.nativeAutomationUnsupported });
  expect(runner.classifyNativeFailure('BARRIER_ACK_TIMEOUT')).toEqual({ verdict: 'BLOCKED', exitCode: EXIT.barrierUnsupported });
  expect(runner.classifyNativeFailure('MARKER_RECOGNITION_UNVERIFIED')).toEqual({ verdict: 'BLOCKED', exitCode: EXIT.markerRecognitionUnverified });
  expect(runner.classifyNativeFailure('TASK4_IDENTITY_DEPENDENCY')).toEqual({ verdict: 'BLOCKED', exitCode: EXIT.task4IdentityDependency });
  expect(runner.classifyNativeFailure('ASSERTION_FAILURE')).toEqual({ verdict: 'FAIL', exitCode: EXIT.scenarioFailure });
  expect(runner.classifyNativeFailure('RECOVERY_UNPROVEN')).toEqual({ verdict: 'FAIL', exitCode: EXIT.scenarioFailure });
});

// ---------------------------------------------------------------------------
// Pass-5 blocker (scripts lane). The driver's own PowerShell scripts are
// LINE-structured: a here-string header (`@"`) must end its line and its
// terminator (`"@`) must start one. Space-joining the arrays produced
// `UnexpectedCharactersAfterHereStringHeader` on the real desktop, so the split
// click never executed and the selector scoping was never reached. These tests
// replay the builders' generated strings; nothing here launches PowerShell, a
// product process, a window, or a scheduled task.

const powerShellScriptLines = script => script.split('\n');

test('split-right PowerShell script keeps the here-string header and terminator on their own lines', async () => {
  const { buildWindowsSplitRightScript } = await import('../lib/qa-scenarios/native-driver.mjs');
  const script = buildWindowsSplitRightScript(4242, 2500);
  const lines = powerShellScriptLines(script);
  // The measured defect signature: `Add-Type @" using System; ...` on one line,
  // and a terminator sharing a line with the statement before it.
  expect(script).not.toMatch(/@"[^\n]/);
  expect(script).not.toMatch(/[^\n]"@/);
  expect(lines).not.toContain('');
  expect(lines.filter(line => line === 'Add-Type @"')).toHaveLength(1);
  expect(lines.filter(line => line === '"@;')).toHaveLength(1);
  const header = lines.indexOf('Add-Type @"');
  const footer = lines.indexOf('"@;');
  // Header and terminator own their lines: the statements around them are whole,
  // separate lines, and the here-string body sits between them.
  expect(lines[header - 1]).toBe('Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, Microsoft.VisualBasic;');
  expect(lines[header + 1]).toBe('using System;');
  expect(lines[footer - 1]).toBe('}');
  expect(lines[footer + 1]).toBe('$targetPid = 4242;');
  expect(header).toBeLessThan(footer);
  // The C# body survives verbatim.
  expect(lines).toContain('using System.Runtime.InteropServices;');
  expect(lines).toContain('public struct FerryxQaRectStruct { public int Left; public int Top; public int Right; public int Bottom; }');
  expect(lines).toContain('public class FerryxQaRect {');
  expect(lines).toContain('  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out FerryxQaRectStruct rect);');
  expect(lines).toContain('  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);');
  // Every typed failure path and the candidate/chosen evidence are unchanged.
  expect(lines).toContain('function Fail($code, $detail) { $diag.failure = $code; $diag.detail = $detail; Emit; exit 0 }');
  expect(lines).toContain('$diag.candidates = $candidates;');
  expect(lines).toContain('$diag.candidateCount = $items.Count;');
  expect(lines).toContain('$diag.actionableCount = $actionable.Count;');
  expect(script).toContain("$diag.chosen = [ordered]@{ index = [int]$actionable[0].index;");
  expect(script).toContain("$diag.result = 'SPLIT_CLICKED';");
  for (const code of ['NO_OWNED_WINDOW', 'SPLIT_RIGHT_NOT_FOUND', 'SPLIT_RIGHT_NOT_UNIQUE', 'SPLIT_RIGHT_DISABLED']) {
    expect(script).toContain(code);
  }
  // The probe prints exactly one JSON line and exits 0 on every path.
  expect(lines[lines.length - 1]).toBe('Emit;');
  expect(lines).toContain("function Emit { Write-Output ($diag | ConvertTo-Json -Compress -Depth 8) }");
});

test('owned-window wait PowerShell script keeps the here-string header and terminator on their own lines', async () => {
  const { buildWindowsWindowWaitScript } = await import('../lib/qa-scenarios/native-driver.mjs');
  const script = buildWindowsWindowWaitScript(4242, 8000);
  const lines = powerShellScriptLines(script);
  expect(script).not.toMatch(/@"[^\n]/);
  expect(script).not.toMatch(/[^\n]"@/);
  // One array element per line: the EnumProc callback body is separated by the
  // newline join itself, not by embedded escapes, so no blank line survives.
  expect(lines).not.toContain('');
  const header = lines.indexOf('Add-Type @"');
  const footer = lines.indexOf('"@;');
  expect(lines[header - 1]).toBe("$ErrorActionPreference = 'Stop';");
  expect(lines[header + 1]).toBe('using System;');
  expect(lines[footer - 1]).toBe('}');
  expect(lines[footer + 1]).toBe('$targetPid = 4242;');
  expect(header).toBeLessThan(footer);
  expect(lines).toContain('public class FerryxQaWin {');
  expect(lines).toContain('  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);');
  expect(lines).toContain('  [DllImport("user32.dll")] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder text, int count);');
  // The EnumProc delegate body is its own line per statement, including the
  // closing brace of the `if` block and the delegate's `return`.
  expect(lines).toContain('$cb = [FerryxQaWin+EnumProc]{ param($hWnd, $lParam)');
  expect(lines).toContain('  if ($owner -eq [uint32]$targetPid) {');
  expect(lines).toContain('  }');
  expect(lines).toContain('  return $true };');
  expect(lines).toContain('$budgetMs = 8000;');
  expect(lines).toContain("  probe = 'owned-window';");
  expect(lines[lines.length - 1]).toBe('Write-Output ($payload | ConvertTo-Json -Compress -Depth 6);');
});

test('owned-window capture PowerShell script keeps the here-string header and terminator on their own lines', async () => {
  const { buildWindowsCaptureScript } = await import('../lib/qa-scenarios/native-driver.mjs');
  const script = buildWindowsCaptureScript(4242, "C:\\ev\\shot's.png");
  const lines = powerShellScriptLines(script);
  expect(script).not.toMatch(/@"[^\n]/);
  expect(script).not.toMatch(/[^\n]"@/);
  expect(lines).not.toContain('');
  const header = lines.indexOf('Add-Type @"');
  const footer = lines.indexOf('"@;');
  expect(lines[header - 1]).toBe('if (-not $handle) { throw "NO_OWNED_WINDOW" }');
  expect(lines[header + 1]).toBe('  using System;');
  expect(lines[footer - 1]).toBe('  }');
  expect(lines[footer + 1]).toBe('$r = New-Object RECT;');
  expect(header).toBeLessThan(footer);
  expect(lines).toContain('  using System.Runtime.InteropServices;');
  expect(lines).toContain('  public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }');
  expect(lines).toContain('    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);');
  // The single-quoted capture path is still escaped for PowerShell, and the
  // probe still prints its bounds line.
  expect(lines).toContain("$b.Save('C:\\ev\\shot''s.png');");
  expect(lines[lines.length - 1]).toBe('"$($r.Left),$($r.Top),$w,$h"');
});

test('retry, focus, and marker PowerShell scripts keep one statement per line and carry no here-string', async () => {
  const { buildWindowsRetryScript, buildWindowsFocusScript, buildWindowsTypeMarkerScript } = await import('../lib/qa-scenarios/native-driver.mjs');
  // These three carry no here-string, so their only risk is the same space join
  // silently flattening statements onto one line. Each is asserted line by line.
  const retry = powerShellScriptLines(buildWindowsRetryScript(4242));
  expect(retry).toHaveLength(16);
  expect(retry[0]).toBe('Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes;');
  expect(retry[1]).toBe('$proc = Get-Process -Id 4242 -ErrorAction Stop;');
  expect(retry).toContain('if (-not $root) { throw "NO_OWNED_WINDOW" }');
  expect(retry).toContain('if ($items.Count -eq 0) {');
  expect(retry).toContain('  foreach ($btn in $allButtons) {');
  expect(retry).toContain('}');
  expect(retry).toContain('if ($items.Count -eq 0) { throw "RETRY_BUTTON_NOT_FOUND" }');
  expect(retry[retry.length - 1]).toBe('"RETRY_CLICKED"');

  expect(powerShellScriptLines(buildWindowsFocusScript(4242))).toEqual([
    '$proc = Get-Process -Id 4242 -ErrorAction Stop;',
    'Add-Type -AssemblyName Microsoft.VisualBasic;',
    '[Microsoft.VisualBasic.Interaction]::AppActivate($proc.Id) | Out-Null;',
    "'FOCUSED'",
  ]);

  expect(powerShellScriptLines(buildWindowsTypeMarkerScript())).toEqual([
    'Add-Type -AssemblyName System.Windows.Forms;',
    "[System.Windows.Forms.SendKeys]::SendWait('Write-Output ''FERRYX_SPLIT_READY''{ENTER}');",
    "'TYPED'",
  ]);

  for (const script of [buildWindowsRetryScript(4242), buildWindowsFocusScript(4242), buildWindowsTypeMarkerScript()]) {
    expect(script).not.toContain('@"');
    expect(script).not.toContain('"@');
  }
});

test('windows session probe script is newline-joined like every other PowerShell builder', async () => {
  const { buildWindowsSessionProbeScript } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const lines = powerShellScriptLines(buildWindowsSessionProbeScript());
  expect(lines).toHaveLength(19);
  expect(lines[0]).toBe("$ErrorActionPreference = 'Continue';");
  expect(lines).toContain('$sessions = @();');
  expect(lines).toContain('$self = Get-Process -Id $PID;');
  expect(lines).toContain('$payload = [ordered]@{');
  expect(lines).toContain('  userName = $env:USERNAME;');
  expect(lines).not.toContain('');
  expect(lines[lines.length - 1]).toBe('Write-Output ($payload | ConvertTo-Json -Compress -Depth 6);');
  // No here-string here: the join is line-based purely so a here-string can
  // never be added to a flattened script again.
  expect(lines.join('\n')).not.toContain('@"');
});

test('no generated PowerShell script merges a statement onto the here-string header or terminator line', async () => {
  const native = await import('../lib/qa-scenarios/native-driver.mjs');
  const { buildWindowsSessionProbeScript } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const scripts = {
    'owned-window wait': native.buildWindowsWindowWaitScript(4242, 8000),
    'split-right': native.buildWindowsSplitRightScript(4242, 2500),
    'owned-window capture': native.buildWindowsCaptureScript(4242, "C:\\ev\\shot's.png"),
    'retry': native.buildWindowsRetryScript(4242),
    'focus': native.buildWindowsFocusScript(4242),
    'type-marker': native.buildWindowsTypeMarkerScript(),
    'session probe': buildWindowsSessionProbeScript(),
  };
  for (const [name, script] of Object.entries(scripts)) {
    // A here-string header or terminator sharing its line with anything else is
    // the pass-5 failure; a one-line script is the space join that caused it.
    expect([name, /@"[^\n]/.test(script)]).toEqual([name, false]);
    expect([name, /[^\n]"@/.test(script)]).toEqual([name, false]);
    expect([name, script.split('\n').length > 1]).toEqual([name, true]);
    expect([name, script.split('\n').includes('')]).toEqual([name, false]);
  }
});
