// Task 3 runner unit tests (authored; execution delegated to the sole remote
// verifier per plan - never run locally). Follows the scripts/qa/*.test.mjs
// pattern that imports vitest from ui/node_modules.

import { test, expect, vi } from '../../ui/node_modules/vitest/dist/index.js';
import { mkdtempSync, mkdirSync, rmSync, existsSync, writeFileSync, chmodSync, watch, realpathSync } from 'node:fs';
import { readFileSync } from 'node:fs';
import { tmpdir, homedir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  BUDGETS, HEADLESS_ELIGIBLE, SCENARIOS, HarnessError, parseInvocation, preflight,
  BarrierHub, ResourceRegistry, correlateReceipt, computeCleanupGate,
  computeSourceDigest, withDeadline, assertPositiveRecovery, spawnOwned,
  BARRIER_ROLES,
} from '../lib/qa-scenarios/common-harness.mjs';
import { assertClassifierReceipt } from '../lib/qa-scenarios/diagnostic-classifier.mjs';
import { requireFiveTupleReceipt, assertInvariants } from './pane-liveness.mjs';

const fixtureRoot = () => realpathSync(mkdtempSync(join(realpathSync(tmpdir()), 'pane-liveness-test-')));

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

test('regression H4 five-tuple: presence alone is insufficient; identity must correlate', () => {
  const full = { frontendSessionId: 'f', paneIdentity: 'p', backendSessionId: 'b-1', bindingKey: 'k', attemptGeneration: 3 };
  expect(requireFiveTupleReceipt(full, { backendSessionId: 'b-1', bindingKey: 'k', attemptGeneration: 3 }, 't')).toBe(full);
  expect(() => requireFiveTupleReceipt(full, { backendSessionId: 'b-2' }, 't')).toThrowError(/does not match the created backend/);
  expect(() => requireFiveTupleReceipt(full, { bindingKey: 'other' }, 't')).toThrowError(/does not match the launch binding/);
  expect(() => requireFiveTupleReceipt(full, { attemptGeneration: 4 }, 't')).toThrowError(/does not match the created attempt/);
  expect(() => requireFiveTupleReceipt({ ...full, bindingKey: null }, {}, 't')).toThrowError(/missing 5-tuple field bindingKey/);
});

test('regression H3 invariants: handover/suspension/stale receipts are asserted, not logged', () => {
  expect(() => assertInvariants(['handoverPreservesIncarnation'], { originalBackendSessionId: 'b1', adoptedBackendSessionId: 'b2' })).toThrowError(/incarnation not preserved/);

  // Error code asserted via object .code property, not regex on error message
  let task4Err = null;
  try {
    assertInvariants(['handoverPreservesIncarnation'], { originalBackendSessionId: 'b1', adoptedBackendSessionId: 'b1', originalIncarnation: null, adoptedIncarnation: null });
  } catch (err) {
    task4Err = err;
  }
  expect(task4Err).toBeInstanceOf(HarnessError);
  expect(task4Err?.code).toBe('TASK4_IDENTITY_DEPENDENCY');

  expect(assertInvariants(['handoverPreservesIncarnation'], { originalBackendSessionId: 'b1', adoptedBackendSessionId: 'b1', originalIncarnation: 'inc-1', adoptedIncarnation: 'inc-1' })).toBe(true);
  expect(() => assertInvariants(['handoverPreservesIncarnation'], { originalBackendSessionId: 'b1', adoptedBackendSessionId: 'b1', originalIncarnation: 'inc-1', adoptedIncarnation: 'inc-2' })).toThrowError(/creation incarnation changed/);
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

test('regression channel v2: release controls bind run nonce AND operation identity', async () => {
  const root = fixtureRoot();
  const hub = new BarrierHub(root, { runId: 'run-R', operationId: 'op-R' });
  hub.prearm('commit', { plan: 'test' });
  hub.release('commit');
  const release = JSON.parse(readFileSync(join(hub.dir, 'commit.release.json'), 'utf8'));
  expect(release).toMatchObject({ name: 'commit', runId: 'run-R', operationId: 'op-R' });
  // Stale/wrong-operation emissions are rejected, never silently consumed.
  const foreign = { runId: 'run-OTHER', operationId: 'op-R', producer: 'p' };
  expect(() => correlateReceipt(foreign, hub, 'stale-check')).toThrowError(/runId mismatch/);
  rmSync(root, { recursive: true, force: true });
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

test('regression role contract: targetRole in arm spec, role ACK awaited, provenance and overwrite rejection', async () => {
  const root = fixtureRoot();
  try {
    const hub = new BarrierHub(root, { runId: 'run-role', operationId: 'op-role' });

    // 1. Role mapping: predecessor-export maps to predecessor, successor-adopt to successor, commit/abort to predecessor
    expect(BARRIER_ROLES['predecessor-export']).toBe('predecessor');
    expect(BARRIER_ROLES['commit']).toBe('predecessor');
    expect(BARRIER_ROLES['abort']).toBe('predecessor');
    expect(BARRIER_ROLES['successor-adopt']).toBe('successor');

    hub.prearm('predecessor-export');
    const arm = JSON.parse(readFileSync(join(hub.dir, 'predecessor-export.arm.json'), 'utf8'));
    expect(arm.targetRole).toBe('predecessor');

    // Awaiting registered times out deterministically using controlled timer if only a wrong-role ACK exists
    writeFileSync(join(hub.dir, 'predecessor-export.armed-ack.successor.json'), JSON.stringify({
      name: 'predecessor-export', runId: 'run-role', operationId: 'op-role', producer: 'ipc::qa_barrier', producerPid: 200, role: 'successor'
    }));
    vi.useFakeTimers();
    const timeoutPromise = hub.awaitRegistered('predecessor-export', 50);
    vi.advanceTimersByTime(50);
    const timeoutErr = await timeoutPromise.catch(e => e);
    expect(timeoutErr).toBeInstanceOf(HarnessError);
    expect(timeoutErr.code).toBe('BARRIER_ACK_TIMEOUT');
    vi.useRealTimers();

    // Awaiting registered succeeds when matching role ACK is written
    writeFileSync(join(hub.dir, 'predecessor-export.armed-ack.predecessor.json'), JSON.stringify({
      name: 'predecessor-export', runId: 'run-role', operationId: 'op-role', producer: 'ipc::qa_barrier', producerPid: 100, role: 'predecessor'
    }));
    const ack = await hub.awaitRegistered('predecessor-export', 1000);
    expect(ack).toMatchObject({ role: 'predecessor', producerPid: 100 });

    // Overwrite rejection: if generic armed-ack.json has conflicting role or PID, it throws ASSERTION_FAILURE
    hub.prearm('successor-adopt');
    writeFileSync(join(hub.dir, 'successor-adopt.armed-ack.successor.json'), JSON.stringify({
      name: 'successor-adopt', runId: 'run-role', operationId: 'op-role', producer: 'ipc::qa_barrier', producerPid: 300, role: 'successor'
    }));
    writeFileSync(join(hub.dir, 'successor-adopt.armed-ack.json'), JSON.stringify({
      name: 'successor-adopt', runId: 'run-role', operationId: 'op-role', producer: 'ipc::qa_barrier', producerPid: 400, role: 'predecessor'
    }));
    let overwriteErr = null;
    try {
      await hub.awaitRegistered('successor-adopt', 1000);
    } catch (err) {
      overwriteErr = err;
    }
    expect(overwriteErr).toBeInstanceOf(HarnessError);
    expect(overwriteErr.code).toBe('ASSERTION_FAILURE');
  } finally {
    vi.useRealTimers();
    rmSync(root, { recursive: true, force: true });
  }
});

test('regression negative registration checks: missing producerPid, absent role, empty producer, mismatched name, malformed generic ACK', async () => {
  const root = fixtureRoot();
  try {
    const hub = new BarrierHub(root, { runId: 'run-neg', operationId: 'op-neg' });

    // 1. Missing producerPid (undefined) must fail with ASSERTION_FAILURE
    hub.prearm('commit');
    writeFileSync(join(hub.dir, 'commit.armed-ack.predecessor.json'), JSON.stringify({
      name: 'commit', runId: 'run-neg', operationId: 'op-neg', producer: 'ipc::qa_barrier', role: 'predecessor'
      // producerPid omitted
    }));
    let errPid = null;
    try { await hub.awaitRegistered('commit', 500); } catch (e) { errPid = e; }
    expect(errPid).toBeInstanceOf(HarnessError);
    expect(errPid.code).toBe('ASSERTION_FAILURE');

    // Invalid non-integer or <= 0 producerPid
    writeFileSync(join(hub.dir, 'commit.armed-ack.predecessor.json'), JSON.stringify({
      name: 'commit', runId: 'run-neg', operationId: 'op-neg', producer: 'ipc::qa_barrier', producerPid: 0, role: 'predecessor'
    }));
    let errPidZero = null;
    try { await hub.awaitRegistered('commit', 500); } catch (e) { errPidZero = e; }
    expect(errPidZero?.code).toBe('ASSERTION_FAILURE');

    // 2. Absent role when targetRole is configured must fail with ASSERTION_FAILURE
    hub.prearm('abort');
    writeFileSync(join(hub.dir, 'abort.armed-ack.predecessor.json'), JSON.stringify({
      name: 'abort', runId: 'run-neg', operationId: 'op-neg', producer: 'ipc::qa_barrier', producerPid: 123
      // role omitted
    }));
    let errRole = null;
    try { await hub.awaitRegistered('abort', 500); } catch (e) { errRole = e; }
    expect(errRole?.code).toBe('ASSERTION_FAILURE');

    // Mismatched role
    writeFileSync(join(hub.dir, 'abort.armed-ack.predecessor.json'), JSON.stringify({
      name: 'abort', runId: 'run-neg', operationId: 'op-neg', producer: 'ipc::qa_barrier', producerPid: 123, role: 'successor'
    }));
    let errRoleMismatch = null;
    try { await hub.awaitRegistered('abort', 500); } catch (e) { errRoleMismatch = e; }
    expect(errRoleMismatch?.code).toBe('ASSERTION_FAILURE');

    // 3. Empty / whitespace producer must fail with ASSERTION_FAILURE
    hub.prearm('backend-write');
    writeFileSync(join(hub.dir, 'backend-write.armed-ack.json'), JSON.stringify({
      name: 'backend-write', runId: 'run-neg', operationId: 'op-neg', producer: '   ', producerPid: 456
    }));
    let errProducer = null;
    try { await hub.awaitRegistered('backend-write', 500); } catch (e) { errProducer = e; }
    expect(errProducer?.code).toBe('ASSERTION_FAILURE');

    // 4. Mismatched barrier name must fail with ASSERTION_FAILURE
    writeFileSync(join(hub.dir, 'backend-write.armed-ack.json'), JSON.stringify({
      name: 'wrong-barrier-name', runId: 'run-neg', operationId: 'op-neg', producer: 'ipc::qa_barrier', producerPid: 456
    }));
    let errName = null;
    try { await hub.awaitRegistered('backend-write', 500); } catch (e) { errName = e; }
    expect(errName?.code).toBe('ASSERTION_FAILURE');

    // 5. Malformed generic JSON must fail with ASSERTION_FAILURE (not swallowed)
    hub.prearm('predecessor-export');
    writeFileSync(join(hub.dir, 'predecessor-export.armed-ack.predecessor.json'), JSON.stringify({
      name: 'predecessor-export', runId: 'run-neg', operationId: 'op-neg', producer: 'ipc::qa_barrier', producerPid: 789, role: 'predecessor'
    }));
    writeFileSync(join(hub.dir, 'predecessor-export.armed-ack.json'), '{ malformed json: not valid syntax');
    let errMalformed = null;
    try { await hub.awaitRegistered('predecessor-export', 500); } catch (e) { errMalformed = e; }
    expect(errMalformed).toBeInstanceOf(HarnessError);
    expect(errMalformed.code).toBe('ASSERTION_FAILURE');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('regression targetBackendSessionId binding: strict schema, no aliases, no session ID in targetRole', () => {
  const root = fixtureRoot();
  try {
    const hub = new BarrierHub(root, { runId: 'run-sess', operationId: 'op-sess' });
    // Prearm presentation without preknown backend
    hub.prearm('presentation', { plan: 'split-happy' });
    const armBefore = JSON.parse(readFileSync(join(hub.dir, 'presentation.arm.json'), 'utf8'));
    expect(armBefore.targetBackendSessionId).toBeUndefined();
    expect(armBefore.targetRole).toBeUndefined();

    // Rejects non-role strings in targetRole (NEVER put backend ID in targetRole)
    expect(() => hub.prearm('bad-role-barrier', { targetRole: 'backend-uuid-1234' }))
      .toThrowError(/targetRole must be 'predecessor' or 'successor'/);

    // Dynamic discovery from authoritative backend emission (e.g. attach-handshake or split-create)
    const discoveredBackendSessionId = 'backend-uuid-authoritative-987';
    expect(hub.isArmed('presentation')).toBe(true);
    hub.bindBackendSession('presentation', discoveredBackendSessionId);

    const armAfter = JSON.parse(readFileSync(join(hub.dir, 'presentation.arm.json'), 'utf8'));
    expect(armAfter.targetBackendSessionId).toBe(discoveredBackendSessionId);
    // targetRole must NEVER be overwritten with backendSessionId
    expect(armAfter.targetRole).toBeUndefined();
    expect(hub.armed.get('presentation').targetBackendSessionId).toBe(discoveredBackendSessionId);
    expect(hub.armed.get('presentation').targetRole).toBeNull();

    // Cannot bind un-armed barrier
    expect(() => hub.bindBackendSession('unarmed-barrier', 'uuid')).toThrow();
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
