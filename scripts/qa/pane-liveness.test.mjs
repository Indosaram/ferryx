// Task 3 runner unit tests (authored; execution delegated to the sole remote
// verifier per plan - never run locally). Follows the scripts/qa/*.test.mjs
// pattern that imports vitest from ui/node_modules.

import { test, expect } from '../../ui/node_modules/vitest/dist/index.js';
import { EventEmitter } from 'node:events';
import { mkdtempSync, mkdirSync, rmSync, existsSync, writeFileSync, chmodSync, watch, realpathSync } from 'node:fs';
import { readFileSync } from 'node:fs';
import { tmpdir, homedir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  BUDGETS, EXIT, HEADLESS_ELIGIBLE, SCENARIOS, HarnessError, parseInvocation, preflight,
  BarrierHub, ResourceRegistry, correlateReceipt, computeCleanupGate,
  computeSourceDigest, withDeadline, assertPositiveRecovery, spawnOwned,
  reapProcess, removePathWithRetry,
  MonotonicBudget, validateFixtureSetup, SCENARIO_FIXTURE_REQUIREMENTS, BARRIER_ROLES,
  requireSevenTupleReceipt, requireFiveTupleReceipt,
  LOCAL_SPLIT_LIFECYCLE_CAPABILITY, ATTACH_TUPLE_FIELDS,
  AppStdioSink, appStdioResult, APP_STDIO_TRUNCATION_MARKER,
  appStdioBytes, archiveBarrierHub, BARRIER_ARCHIVE_MAX_BYTES,
  InstrumentationClock,
  ATTEMPT_BUDGET_SPENT, INVENTORY_READ_REFUSED,
  BARRIER_ARCHIVE_SNAPSHOT_FILE, BARRIER_ARCHIVE_SNAPSHOT_MARKER,
} from '../lib/qa-scenarios/common-harness.mjs';
import { assertClassifierReceipt, runHeadlessDiagnosticClassifier, runNativeDiagnosticClassifier, buildIsolatedEnv } from '../lib/qa-scenarios/diagnostic-classifier.mjs';
import { assertInvariants, assertSinglePty, SCENARIO_PLANS, archiveRunBarrierHub, attemptBudgetAccounting, ATTEMPT_CEILING_BASIS } from './pane-liveness.mjs';
import {
  MARKER_TEXT,
  performInspectionHandshake,
} from '../lib/qa-scenarios/native-driver.mjs';
import {
  assertConflictWaveReported,
  runSplitHappyScenario,
  runSplitAttachStallScenario,
  runSplitCancelScenario,
  runSplitConcurrentScenario,
  SPLIT_INVENTORY_ACTION,
  PRE_SPLIT_INVENTORY_ACTION,
  INVENTORY_READER_MISSING,
  SPLIT_INVENTORY_READ_FAILED,
  SPLIT_INVENTORY_LAST_READING_ACTION,
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

test('cleanup reaps registered owned processes by PID, removes registered roots, and drains app stdio into registered artifacts', async () => {
  const root = fixtureRoot();
  const registry = new ResourceRegistry();
  const { spawn } = await import('node:child_process');
  const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)']);
  registry.registerProcess(child, 'owned-fixture-child');
  const subRoot = join(root, 'owned-sub');
  mkdirSync(subRoot);
  registry.registerDirectory(subRoot);

  // App stdio sink (task-9 pass-12 gap): the app's own stdout/stderr are drained
  // into per-run artifacts under the evidence run dir, registered like every
  // other resource, and closed by the SAME cleanup pass. `spawnOwned` is driven
  // here exactly as the runner drives it (sink passed as an option).
  const appDir = join(root, 'run-dir');
  const sink = new AppStdioSink(appDir, { label: 'app' });
  registry.registerLog(sink, 'app-stdio');
  const drained = spawnOwned(registry, process.execPath, ['-e',
    "process.stdout.write('spawn-line\\n'); process.stderr.write('[cmd_terminal_spawn] stage=daemon_spawn failed code=X\\n');"],
  { stdioSink: sink });
  // The child's own 'close' event fires only after both stdio pipes have been
  // fully read, so the drain's writes are already queued when the sink is closed
  // below. No sleep and no polling: the child's event is the barrier.
  const drainedClosed = await withDeadline(
    new Promise(resolvePromise => drained.once('close', () => resolvePromise(true))),
    6000, 'drained-child-close',
  );
  expect(drainedClosed.timedOut).toBe(false);

  const cleanup = await registry.cleanup();
  const processReceipt = cleanup.find(r => r.kind === 'process');
  expect(processReceipt.exited).toBe(true);
  expect(registry.reaped).toContain(child.pid);
  expect(existsSync(subRoot)).toBe(false);

  // (1) both streams really landed in the run dir, with their real content
  const stdoutPath = join(appDir, 'app.stdout.log');
  const stderrPath = join(appDir, 'app.stderr.log');
  expect(sink.paths()).toEqual({ stdout: stdoutPath, stderr: stderrPath });
  expect(readFileSync(stdoutPath, 'utf8')).toContain('spawn-line');
  expect(readFileSync(stderrPath, 'utf8')).toContain('stage=daemon_spawn failed code=X');

  // (2) teardown closed them and recorded them in the cleanup receipts
  const logReceipt = cleanup.find(r => r.kind === 'log');
  expect([logReceipt.label, logReceipt.closed, logReceipt.closedOk]).toEqual(['app-stdio', true, true]);
  expect(logReceipt.streams.map(s => [s.name, s.path, s.truncated, s.droppedBytes]))
    .toEqual([['stdout', stdoutPath, false, 0], ['stderr', stderrPath, false, 0]]);

  // (3) the paths a reader finds in result.json (appStdioResult is the exact
  // projection the runner writes there and into the launch.binary action)
  const recorded = appStdioResult(sink);
  expect([recorded.stdout, recorded.stderr, recorded.closed, recorded.truncated, recorded.droppedBytes])
    .toEqual([stdoutPath, stderrPath, true, false, 0]);

  // (4) a burst larger than the cap is truncated with an explicit marker in the
  // file and a recorded dropped-byte count - never dropped silently
  const cappedDir = join(root, 'capped-dir');
  const capped = new AppStdioSink(cappedDir, { label: 'capped', maxBytes: 64 });
  const fakeChild = { stdout: new EventEmitter(), stderr: new EventEmitter() };
  capped.attach(fakeChild);
  fakeChild.stdout.emit('data', Buffer.alloc(4096, 0x61));
  fakeChild.stderr.emit('data', Buffer.from('under the cap'));
  const cappedReceipt = await capped.close();
  const cappedText = readFileSync(join(cappedDir, 'capped.stdout.log'), 'utf8');
  expect(cappedText.startsWith('a'.repeat(64))).toBe(true);
  expect(cappedText).toContain(APP_STDIO_TRUNCATION_MARKER);
  expect(cappedText).toContain('dropped 4032 bytes');
  expect(cappedReceipt.streams.map(s => [s.bytes, s.truncated, s.droppedBytes]))
    .toEqual([[64, true, 4032], [13, false, 0]]);
  expect(appStdioResult(capped).droppedBytes).toBe(4032);
  expect(appStdioResult(capped).truncated).toBe(true);
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

// ---------------------------------------------------------------------------
// Pass-6 cleanup-gate defect (E/task-9/REPORT-PASS6.md §5): the runner's own
// `taskkill /T` (no `/F`) left the app's `--daemon` descendant alive, so the
// isolation root stayed held (`directoriesRemoved: false`) AND the stdio pipe
// that descendant inherited stayed open, so node never exited after printing
// its result. One cause, two symptoms. These tests lock the forced reap, its
// exact-PID scoping, the bounded root-removal retry and the still-held holder
// report.

// A child handle that never exits: the escalation path must still be bounded
// and must never claim an unexited process as reaped.
function fakeRunningChild(pid) {
  return { pid, exitCode: null, signalCode: null, killed: false, once() {}, kill() { this.killed = true; } };
}

test('pass-6 cleanup: the escalated reap force-kills the owned tree by exact PID', async () => {
  const winKills = [];
  const win = await reapProcess(
    { pid: 4242, label: '', child: fakeRunningChild(4242), executable: 'C:\\stage\\ferryx.exe' },
    { platform: 'win32', rows: [], graceMs: 0, forceMs: 0, runKill: (file, args) => winKills.push([file, args]) },
  );
  expect(winKills).toEqual([['taskkill', ['/T', '/PID', '4242']], ['taskkill', ['/T', '/F', '/PID', '4242']]]);
  expect(win.escalated).toBe(true);
  expect(win.exited).toBe(false);
  expect(win.method).toBe('taskkill /T /F (forced process tree)');

  const posixSignals = [];
  const posix = await reapProcess(
    { pid: 4243, label: '', child: fakeRunningChild(4243), executable: '/stage/ferryx' },
    { platform: 'posix', rows: [], graceMs: 0, forceMs: 0, signalProcess: (pid, signal) => posixSignals.push([pid, signal]) },
  );
  expect(posixSignals).toEqual([[-4243, 'SIGTERM'], [-4243, 'SIGKILL']]);
  expect(posix.escalated).toBe(true);
  expect(posix.method).toBe('SIGKILL to task-owned process group (escalated from SIGTERM)');
});

test('pass-6 cleanup: reap scoping signals only this run\'s exact PIDs and never a name pattern', async () => {
  const calls = [];
  const rows = [
    { pid: 333, ppid: 111, executable: 'C:\\stage\\ferryx.exe' }, // the app's own --daemon descendant
    { pid: 444, ppid: 111, executable: 'C:\\Windows\\explorer.exe' }, // unrelated: never signalled
  ];
  const receipt = await reapProcess(
    { pid: 111, label: '', child: fakeRunningChild(111), executable: 'C:\\stage\\ferryx.exe' },
    {
      platform: 'win32', rows, roots: ['C:\\stage\\runtime\\split-happy'], graceMs: 0, forceMs: 0,
      runKill: (file, args) => calls.push([file, args]), alive: () => true,
    },
  );
  // Graceful, forced, then the identity-verified descendant - all by exact PID.
  const signalled = calls.flatMap(([, args]) => args.filter(token => /^\d+$/.test(token)));
  expect(signalled).toEqual(['111', '111', '333']);
  expect(receipt.descendants.map(d => ({ pid: d.pid, ppid: d.ppid, identityVerified: d.identityVerified }))).toEqual([
    { pid: 333, ppid: 111, identityVerified: true },
    { pid: 444, ppid: 111, identityVerified: false },
  ]);
  expect(receipt.descendantsForceReaped).toEqual([333]);
  // A name/command-line pattern (or a foreign PID) must never appear in a kill.
  const serialized = JSON.stringify(calls);
  for (const forbidden of ['*', '-like', '.mjs', 'ferryx.exe', 'explorer', '999']) {
    expect(serialized).not.toContain(forbidden);
  }
});

test('pass-6 cleanup: isolation-root removal is retried within a bounded budget', async () => {
  const sleeps = [];
  let present = true;
  let failures = 2;
  const removal = await removePathWithRetry('/iso/root', {
    attempts: 6, intervalMs: 10, deadlineMs: 1000,
    remove: () => { if (failures > 0) failures -= 1; else present = false; },
    exists: () => present,
    sleep: async ms => { sleeps.push(ms); },
    now: () => 0,
  });
  expect(removal).toEqual({ removed: true, attempts: 3 });
  expect(sleeps).toEqual([10, 10]);

  const stuckSleeps = [];
  const stuck = await removePathWithRetry('/iso/held', {
    attempts: 4, intervalMs: 10, deadlineMs: 1000,
    remove: () => { throw new Error('EBUSY: still held by the force-reaped descendant'); },
    exists: () => true,
    sleep: async ms => { stuckSleeps.push(ms); },
    now: () => 0,
  });
  expect(stuck).toEqual({ removed: false, attempts: 4 });
  expect(stuckSleeps).toEqual([10, 10, 10]); // bounded: never an unbounded poll
});

test('pass-6 cleanup: a still-held root keeps cleanupGate.ok=false and names the holder', async () => {
  const root = 'C:\\stage\\runtime\\split-happy';
  const registry = new ResourceRegistry({
    // The app is already gone and its own `--daemon` descendant still holds the
    // root: exactly the pass-6 measurement.
    snapshot: async () => [{ pid: 22456, ppid: 7716, executable: 'C:\\stage\\ferryx.exe' }],
    remove: () => { throw new Error('EBUSY: held by the daemon child'); },
    exists: () => true,
    sleep: async () => {},
    now: () => 0,
    alive: pid => pid === 22456,
    graceMs: 0,
    forceMs: 0,
    runKill: () => {},
    // Never signal a real PID from a fabricated fixture.
    signalProcess: () => {},
  });
  registry.registerProcess({ pid: 7716, exitCode: 0, signalCode: null, once() {}, kill() {} }, '', { executable: 'C:\\stage\\ferryx.exe' });
  registry.registerDirectory(root);
  const receipts = await registry.cleanup();
  const gate = computeCleanupGate(registry, receipts);
  expect(gate).toEqual({ processesReaped: true, socketsRemoved: true, directoriesRemoved: false, ok: false });
  const directory = receipts.find(receipt => receipt.kind === 'directory');
  expect(directory.removed).toBe(false);
  expect(directory.attempts).toBe(BUDGETS.cleanupRootRetryAttempts);
  expect(directory.holders).toEqual([{
    pid: 22456, ppid: 7716, executable: 'C:\\stage\\ferryx.exe',
    identityVerified: true, identity: 'executable is the binary this run launched', holderOf: root,
  }]);
});

test('pass-6 cleanup: a SIGTERM-ignoring process tree is force-reaped to the group', async () => {
  if (process.platform === 'win32') return; // taskkill /T /F covers the tree there
  const registry = new ResourceRegistry();
  const grandchildSrc = "process.on('SIGTERM', () => {}); setInterval(() => {}, 1000);";
  const leaderSrc = [
    "process.on('SIGTERM', () => {});",
    `const c = require('node:child_process').spawn(process.execPath, ['-e', ${JSON.stringify(grandchildSrc)}], { stdio: ['ignore', 'inherit', 'inherit'] });`,
    "process.stdout.write('READY:' + c.pid + '\\n');",
    'setInterval(() => {}, 1000);',
  ].join(' ');
  const child = spawnOwned(registry, process.execPath, ['-e', leaderSrc]);
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
  let reaped = false;
  try {
    const receipts = await registry.cleanup();
    const receipt = receipts.find(entry => entry.kind === 'process');
    expect(receipt.escalated).toBe(true); // SIGTERM was ignored: the force step ran
    expect(receipt.exited).toBe(true);
    expect(receipt.descendants.map(d => d.pid)).toContain(grandchildPid);
    reaped = true;
  } finally {
    // Fixture-owned fallback so a broken escalation cannot leak an orphan.
    if (!reaped) { try { process.kill(grandchildPid, 'SIGKILL'); } catch { /* already gone */ } }
    try { process.kill(-child.pid, 'SIGKILL'); } catch { /* already gone */ }
  }
  expect(() => process.kill(-child.pid, 0)).toThrow();
  expect(() => process.kill(grandchildPid, 0)).toThrow();
  expect(reaped).toBe(true);
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
    // Task-9: the retried pane's presentation and its marker are addressed by
    // session identity, not by line index (the app's own pane settles into the
    // same receipt streams), so the fake answers the session-addressed reads too.
    awaitReceiptForSession: async (name, sessionId) => {
      if (name === 'presentation') {
        return {
          attachTuple: {
            backendSessionId: sessionId,
            incarnation: 'inc-stall',
            daemonEpoch: '1',
            frontendSessionId: 'f-stall',
            paneIdentity: 'p-stall',
            bindingKey: 'k-stall',
            attemptGeneration: 2,
          },
          presented: true,
        };
      }
      if (name === 'marker-output') {
        return { sessionId, output: MARKER_TEXT, frameSubmitted: true, ptyCreatedCount: 1 };
      }
      throw new Error(`unexpected awaitReceiptForSession ${name}`);
    },
    receiptLines: () => [],
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
      await driver.newPane(evidence, 1234);
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

test('isolated launch env is an allowlist: product dir overrides and QA nonces only, every other FERRYX_* key refused', () => {
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
  // Pass-11 leak: the product's session-state override (`session_dir_override`,
  // FERRYX_SESSION_DIR) must resolve inside THIS run's isolation root, so a run
  // can only ever read/write its own `session/session_state.json` and never the
  // host's real profile (`%APPDATA%\com.ferryx.app\dev\session_state.json`).
  // Without it the app restores the host layout, the empty state never renders
  // and the pane step's `New Terminal` affordance cannot exist.
  expect(isolated.env.FERRYX_SESSION_DIR).toBe(join(root, 'session'));
  expect(isolated.dirs.sessionDir).toBe(join(root, 'session'));
  expect(existsSync(join(root, 'session'))).toBe(true);
  // Every product dir override resolves under this run's owned isolation root
  // (asserted as exact joined paths, so it holds on every platform), which is
  // what makes the host profile unreachable.
  expect(isolated.env.FERRYX_DATA_DIR).toBe(join(root, 'data'));
  expect(isolated.env.FERRYX_RUNTIME_DIR).toBe(join(root, 'runtime'));
  expect(isolated.env.HOME).toBe(join(root, 'home'));
  // The guard still throws on any FERRYX_* key the allowlist does not own: a
  // private channel cannot smuggle an ambient key into the isolated app.
  const smugglingHub = { dir: join(root, 'barriers'), env: () => ({ FERRYX_MACHINE_TOKEN: 'ambient' }) };
  expect(() => buildIsolatedEnv({ isolationRoot: join(root, 'guard-root'), barrierHub: smugglingHub }))
    .toThrow(/ambient FERRYX_\* variable leaked into isolated env: FERRYX_MACHINE_TOKEN/);
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
  expect(native.classifyWindowsFailure('PANE_AFFORDANCE_NOT_FOUND')).toBe('PANE_AFFORDANCE_NOT_FOUND');
  expect(native.classifyWindowsFailure('PANE_AFFORDANCE_NOT_UNIQUE')).toBe('PANE_AFFORDANCE_NOT_UNIQUE');
  expect(native.classifyWindowsFailure('PANE_AFFORDANCE_DISABLED')).toBe('PANE_AFFORDANCE_DISABLED');
  expect(native.classifyWindowsFailure('RETRY_BUTTON_NOT_FOUND')).toBe('NATIVE_AUTOMATION_UNSUPPORTED');
  expect(native.classifyWindowsFailure('')).toBe('NATIVE_AUTOMATION_UNSUPPORTED');
});

test('windows lane blocks are typed, nonzero, and never a pass', async () => {
  const runner = await import('./pane-liveness.mjs');
  const windowsCodes = [
    'NO_INTERACTIVE_SESSION', 'NO_OWNED_WINDOW', 'INTERACTIVE_RELAUNCH_FAILED',
    'SPLIT_RIGHT_NOT_FOUND', 'SPLIT_RIGHT_NOT_UNIQUE', 'SPLIT_RIGHT_DISABLED',
    // Task-9 lane: the frontend the debug binary expects, and the pane the split
    // affordance needs. Every one of them is a BLOCKED environment/harness
    // condition - nonzero, and never a pass.
    'FRONTEND_DIST_MISSING', 'FRONTEND_PORT_OCCUPIED', 'FRONTEND_NOT_SERVED',
    'FRONTEND_DEVURL_MISMATCH',
    'PANE_AFFORDANCE_NOT_FOUND', 'PANE_AFFORDANCE_NOT_UNIQUE', 'PANE_AFFORDANCE_DISABLED',
    'PANE_BINDING_UNBOUND', 'PANE_BINDING_AMBIGUOUS',
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
  // Task-9: a marker that never reached the pane under test is a FAIL (the pane
  // really produced no output) and keeps its typed identity.
  expect(runner.classifyNativeFailure('MARKER_SESSION_UNBOUND')).toEqual({ verdict: 'FAIL', exitCode: EXIT.scenarioFailure });
  expect(new HarnessError('MARKER_SESSION_UNBOUND', 'detail').code).toBe('MARKER_SESSION_UNBOUND');
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
  // The click itself: the invoke is its own statement inside the chosen branch's
  // `try`, so it is emitted indented - asserted on the TRIMMED line, which a
  // commented-out, nested, or otherwise altered invoke cannot satisfy.
  expect(lines.map(line => line.trim())).toContain('$invoke.Invoke();');
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
  // The window-text imports marshal Unicode. Without a charset the default ANSI
  // marshalling stops at the first UTF-16 NUL byte, which is how pass-7 recorded
  // `title`/`className` as one character ('F'/'T', and the IME window as class
  // 'I' / title 'D' for `IME` / `Default IME`). The handle/int/bool imports in
  // the same here-string marshal no text and stay charset-free, and both text
  // imports read into a 256-CHARACTER buffer, so declaration and call size agree.
  expect(lines).toContain('  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);');
  expect(lines).toContain('  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder text, int count);');
  expect(lines).toContain('  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);');
  expect(lines).toContain('  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);');
  expect(lines).toContain('  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);');
  expect(lines.filter(line => line.includes('DllImport')).filter(line => line.includes('CharSet')).length).toBe(2);
  expect(lines.filter(line => line.includes('New-Object System.Text.StringBuilder 256')).length).toBe(2);
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
  // The click itself. This builder emits the invoke unindented (top level, not
  // inside a `try`), so the exact line is asserted directly.
  expect(retry).toContain('$invoke.Invoke();');
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
  // The pass-6 process-table read is a PowerShell builder too: the same
  // line-structure rule has to hold for it (pass-5 defect class).
  const { buildProcessTableScript } = await import('../lib/qa-scenarios/common-harness.mjs');
  const scripts = {
    'owned-window wait': native.buildWindowsWindowWaitScript(4242, 8000),
    'split-right': native.buildWindowsSplitRightScript(4242, 2500),
    'new-pane': native.buildWindowsNewPaneScript(4242, { windows: [{ hwnd: 19663500, title: 'F', className: 'T' }] }),
    'owned-window capture': native.buildWindowsCaptureScript(4242, "C:\\ev\\shot's.png"),
    'retry': native.buildWindowsRetryScript(4242),
    'focus': native.buildWindowsFocusScript(4242),
    'type-marker': native.buildWindowsTypeMarkerScript(),
    'session probe': buildWindowsSessionProbeScript(),
    'process table': buildProcessTableScript(),
  };
  for (const [name, script] of Object.entries(scripts)) {
    // A here-string header or terminator sharing its line with anything else is
    // the pass-5 failure; a one-line script is the space join that caused it.
    expect([name, /@"[^\n]/.test(script)]).toEqual([name, false]);
    expect([name, /[^\n]"@/.test(script)]).toEqual([name, false]);
    expect([name, script.split('\n').length > 1]).toEqual([name, true]);
    expect([name, script.split('\n').includes('')]).toEqual([name, false]);
  }
  // Pass-9 defect (scripts lane; the reason two of three runs burned the full
  // 4 s): the warm emitter ran ONE `FindAll(Descendants, TrueCondition)` per
  // window root with no wait - `warmElements: 16` (the Chromium-internal
  // pre-activation tree) on all three runs - while the retry loop that did exist
  // retried the scope search under the affordance NAME condition
  // (`warmAttempts: 33` / `warmElapsedMs: 4087` on the two failures), and a
  // name-condition `FindAll` on an unbuilt tree does not re-trigger Chromium's
  // build. The attach is what drives the build (pass 9 measured the flip 317-353
  // ms after it), so both warmed builders must RE-ISSUE the attach inside a loop
  // that exits on the observed tree, never on a fixed sleep.
  const warmAttachLines = {
    'split-right': native.buildWindowsSplitRightScript(4242, 2500, { windows: [{ hwnd: 19663500, title: 'F', className: 'T' }] }).split('\n'),
    'new-pane': native.buildWindowsNewPaneScript(4242, { windows: [{ hwnd: 19663500, title: 'F', className: 'T' }] }).split('\n'),
  };
  for (const [name, lines] of Object.entries(warmAttachLines)) {
    const loopStart = lines.indexOf('$warmAttachSw = [System.Diagnostics.Stopwatch]::StartNew();');
    const reissue = lines.indexOf('    $warmNodes = $warmRoot.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition);');
    const counts = lines.indexOf('  $warmAttachCounts.Add($warmElements) | Out-Null;');
    const documentBreak = lines.indexOf('  if ($warmHasDocument) { break };');
    const baselineBreak = lines.indexOf('  if ($warmElements -gt $warmBaselineElements) { break };');
    const budgetBreak = lines.indexOf('  if ($warmAttachSw.ElapsedMilliseconds -ge $warmAttachBudgetMs) { break };');
    const intervalSleep = lines.indexOf('  Start-Sleep -Milliseconds $warmAttachIntervalMs;');
    // The re-issued attach is INSIDE the loop (after the stopwatch the loop is
    // measured from); the loop exits on the observed tree first and only then on
    // its own bounded deadline; and the sleep comes after the observation, so no
    // fixed sleep is ever the mechanism.
    expect([name, loopStart]).not.toEqual([name, -1]);
    expect([name, reissue > loopStart]).toEqual([name, true]);
    expect([name, counts > reissue]).toEqual([name, true]);
    expect([name, documentBreak > counts]).toEqual([name, true]);
    expect([name, baselineBreak > documentBreak]).toEqual([name, true]);
    expect([name, budgetBreak > baselineBreak]).toEqual([name, true]);
    expect([name, intervalSleep > budgetBreak]).toEqual([name, true]);
    // The attach loop is bounded by its own ~1.5x-of-measurement ceiling, not by
    // the 4 s name-search budget it must not consume.
    expect([name, lines.includes('$warmAttachBudgetMs = 500;')]).toEqual([name, true]);
    expect([name, lines.includes('$warmBudgetMs = 4000;')]).toEqual([name, true]);
    // ...and it reports its own attach count, the count at each attempt, and the
    // structural signal, so the next pass can confirm the loop worked from the
    // evidence alone.
    expect([name, lines.includes('$diag.warmElements = $warmElements;')]).toEqual([name, true]);
    expect([name, lines.includes('$diag.warmAttachCount = $warmAttachCount;')]).toEqual([name, true]);
    expect([name, lines.includes('$diag.warmAttachElementCounts = @($warmAttachCounts);')]).toEqual([name, true]);
    expect([name, lines.includes('$diag.warmDocumentSeen = $warmHasDocument;')]).toEqual([name, true]);
  }
  // Both classifiers surface that telemetry unchanged (it is never part of a
  // verdict), and a probe predating the attach loop reports null/empty instead
  // of a fabricated count.
  const warmVerdict = native.classifyWindowsNewPane({
    probe: 'new-pane', selectorNames: ['New Terminal'], interactive: true, sessionId: 1,
    windowsSearched: [{ hwnd: 19663500, title: 'F', className: 'T' }], windowsSearchedCount: 1,
    candidateCount: 0, actionableCount: 0, candidates: [], failure: 'PANE_AFFORDANCE_NOT_FOUND',
    warmElements: 110, warmAttempts: 1, warmElapsedMs: 29,
    warmAttachCount: 4, warmAttachElementCounts: [16, 16, 16, 110], warmDocumentSeen: true, warmAttachElapsedMs: 342,
  });
  expect(warmVerdict.code).toBe('PANE_AFFORDANCE_NOT_FOUND');
  expect(warmVerdict.warmElements).toBe(110);
  expect(warmVerdict.warmAttachCount).toBe(4);
  expect(warmVerdict.warmAttachElementCounts).toEqual([16, 16, 16, 110]);
  expect(warmVerdict.warmDocumentSeen).toBe(true);
  expect(warmVerdict.warmAttachElapsedMs).toBe(342);
  const legacyWarm = native.classifyWindowsSplitRight({ probe: 'split-right', failure: 'SPLIT_RIGHT_NOT_FOUND' });
  expect(legacyWarm.warmAttachCount).toBeNull();
  expect(legacyWarm.warmAttachElementCounts).toEqual([]);
  expect(legacyWarm.warmDocumentSeen).toBeNull();
  expect(legacyWarm.warmAttachElapsedMs).toBeNull();
});

// ---------------------------------------------------------------------------
// Pass-6 blocker (scripts lane). The split click EXECUTED for the first time
// and then measured `candidateCount: 0` window-wide, so the accessible name was
// absent from the whole owned window - and `MainWindowHandle` is not guaranteed
// to be the app's real UI window (the probe saw four owned top-level windows,
// two visible). These tests replay the builders and classifiers; nothing here
// launches PowerShell, a product process, a window, or a scheduled task.

test('split-affordance search enumerates every owned window and searches the visible ones main-handle-first', async () => {
  const native = await import('../lib/qa-scenarios/native-driver.mjs');
  const probe = {
    probe: 'owned-windows', pid: 7716, interactive: true, sessionId: 1, processExited: false,
    mainWindowHandle: 19663500, mainWindowVisible: true,
    windows: [
      { hwnd: 4787552, visible: true, className: 'T', title: '' },
      { hwnd: 19663500, visible: true, className: 'T', title: 'F' },
      { hwnd: 3001, visible: false, className: 'T', title: 'hidden' },
      { hwnd: 1313834, visible: true, className: 'T', title: 'F' },
    ],
  };
  const verdict = native.classifyOwnedWindowsProbe(probe);
  expect(verdict.ok).toBe(true);
  expect(verdict.code).toBeNull();
  expect(verdict.visibleWindowCount).toBe(3);
  // Deterministic order: the main handle first, then every other visible window
  // by ascending hwnd. An invisible window is never searched.
  const ordered = native.orderOwnedWindowsForSearch(verdict.windows, verdict.mainWindowHandle);
  expect(ordered.map(window => window.hwnd)).toEqual([19663500, 1313834, 4787552]);
  expect(ordered.map(window => window.title)).toEqual(['F', 'F', '']);
  // The search script receives exactly that order, one literal array per field.
  const lines = powerShellScriptLines(native.buildWindowsSplitRightScript(4242, 2500, { windows: ordered }));
  expect(lines).toContain('$windowHandles = @(19663500, 1313834, 4787552);');
  expect(lines).toContain("$windowTitles = @('F', 'F', '');");
  expect(lines).toContain("$windowClasses = @('T', 'T', 'T');");
  // A main handle that is not visible (or zero) is not privileged: the visible
  // owned windows are still the search set, in ascending-hwnd order.
  const noMain = native.classifyOwnedWindowsProbe({ ...probe, mainWindowHandle: 0, mainWindowVisible: false });
  expect(noMain.ok).toBe(true);
  expect(native.orderOwnedWindowsForSearch(noMain.windows, noMain.mainWindowHandle).map(window => window.hwnd)).toEqual([1313834, 4787552, 19663500]);
  // No visible owned window at all: typed NO_OWNED_WINDOW and nothing to search.
  const none = native.classifyOwnedWindowsProbe({ ...probe, windows: probe.windows.map(window => ({ ...window, visible: false })) });
  expect(none.ok).toBe(false);
  expect(none.code).toBe('NO_OWNED_WINDOW');
  expect(none.detail).toContain('topLevelWindows=');
  expect(native.orderOwnedWindowsForSearch(none.windows, none.mainWindowHandle)).toEqual([]);
  // The runner's own owned-window gate accepts ANY visible owned window too, so
  // `MainWindowHandle` naming a different (or no) window is no longer fatal...
  const otherVisible = native.classifyWindowsWindowProbe({
    probe: 'owned-window', pid: 7716, interactive: true, sessionId: 1, budgetMs: 8000, waitedMs: 120,
    mainWindowHandle: 0, processExited: false, ok: true, visibleWindowCount: 1,
    windows: [{ hwnd: 4787552, visible: true, className: 'T', title: '' }],
  });
  expect(otherVisible.ok).toBe(true);
  expect(otherVisible.visibleWindowCount).toBe(1);
  // ...and it still refuses a probe whose owned windows are all invisible.
  expect(native.classifyWindowsWindowProbe({ ...otherVisible, ok: true, visibleWindowCount: 0, windows: [{ hwnd: 4787552, visible: false, className: 'T', title: '' }] }).ok).toBe(false);
  // The enumeration script is line-joined like every other PowerShell builder.
  const enumLines = powerShellScriptLines(native.buildWindowsOwnedWindowsScript(4242));
  expect(enumLines).not.toContain('');
  expect(enumLines.filter(line => line === 'Add-Type @"')).toHaveLength(1);
  expect(enumLines[enumLines.length - 1]).toBe('Write-Output ($payload | ConvertTo-Json -Compress -Depth 6);');
  // The enumeration carries the same charset contract as the wait probe: its two
  // StringBuilder imports marshal Unicode (pass-7 read `title`/`className` as one
  // character because ANSI marshalling stopped at the first UTF-16 NUL byte),
  // its handle/int/bool imports stay charset-free, and exactly those two text
  // imports are charset-bearing.
  expect(enumLines).toContain('  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);');
  expect(enumLines).toContain('  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder text, int count);');
  expect(enumLines).toContain('  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);');
  expect(enumLines).toContain('  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);');
  expect(enumLines).toContain('  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);');
  expect(enumLines.filter(line => line.includes('DllImport')).filter(line => line.includes('CharSet')).length).toBe(2);
  expect(enumLines.filter(line => line.includes('New-Object System.Text.StringBuilder 256')).length).toBe(2);
});

test('split-affordance match is multi-property and exact, never a substring or first-match heuristic', async () => {
  const native = await import('../lib/qa-scenarios/native-driver.mjs');
  // The product's pane affordance only; the tab-bar popup (`Split terminal
  // right`) is a different affordance for a different trigger and is not
  // accepted by default, and no automation id is guessed.
  expect(native.SPLIT_AFFORDANCE_NAMES_WIN32).toEqual(['Split pane right']);
  expect(native.SPLIT_AFFORDANCE_AUTOMATION_IDS_WIN32).toEqual([]);
  const script = native.buildWindowsSplitRightScript(4242, 2500, {
    windows: [{ hwnd: 1, title: '', className: 'T' }],
    names: ['Split pane right', 'Split terminal right'],
    automationIds: ['split-pane-right'],
  });
  const lines = powerShellScriptLines(script);
  expect(lines).toContain("$selectorNames = @('Split pane right', 'Split terminal right');");
  expect(lines).toContain("$selectorAutomationIds = @('split-pane-right');");
  // Every bound name/automation id is an EXACT UIA property condition combined
  // with OrCondition - never a substring filter and never an index pick.
  expect(lines).toContain('$conditionName0 = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $selectorNames[0]);');
  expect(lines).toContain('$conditionName1 = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $selectorNames[1]);');
  expect(lines).toContain('$conditionAutomationId0 = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, $selectorAutomationIds[0]);');
  expect(lines).toContain('$conditionArray = [System.Windows.Automation.Condition[]]@($conditionName0, $conditionName1, $conditionAutomationId0);');
  expect(lines).toContain('$condition = New-Object System.Windows.Automation.OrCondition -ArgumentList (, $conditionArray);');
  expect(script).not.toMatch(/Name -match|Name -like/);
  // A single bound name needs no OrCondition wrapper at all.
  expect(powerShellScriptLines(native.buildWindowsSplitRightScript(4242, 2500, { windows: [{ hwnd: 1, title: '', className: 'T' }] })))
    .toContain('$condition = $conditionName0;');
  // A match found by AutomationId alone still classifies as a click, and the
  // bound property set the probe SEARCHED WITH is surfaced in the evidence. The
  // probe always reports that set (`$diag.selectorAutomationIds` is part of the
  // emitted payload on every path), so the fixture carries it exactly as the
  // script does - the field is never re-derived from the matched element.
  const byId = native.classifyWindowsSplitRight({
    probe: 'split-right', selector: 'Split pane right', interactive: true, sessionId: 1,
    selectorAutomationIds: ['split-pane-right'],
    mainWindowHandle: 1, windowVisible: true, visibleWindowCount: 1,
    focusedFound: true, focusSource: 'focused-element', scopeDepth: 2, scopeIsWindowRoot: false,
    windowsSearched: [{ hwnd: 1, title: '', className: 'T' }], windowsSearchedCount: 1, matchedWindowHwnd: 1,
    candidateCount: 1, actionableCount: 1,
    candidates: [{ index: 0, name: '', controlType: 'ControlType.Button', automationId: 'split-pane-right', enabled: true, offscreen: false, rectEmpty: false, rect: '10,10,20,20', inWindow: true }],
    chosen: { index: 0, automationId: 'split-pane-right' },
    result: 'SPLIT_CLICKED',
  });
  expect(byId.ok).toBe(true);
  expect(byId.matchedWindowHwnd).toBe(1);
  expect(byId.selectorAutomationIds).toEqual(['split-pane-right']);
  // A probe that reports no bound property set at all (a probe predating the
  // field) is classified against the module's default binding set for BOTH
  // properties - never against a property scraped from a match. The default id
  // set is empty today (the product exposes no AutomationId), so the recorded
  // value is honestly "the default binding set", not a measured zero. Refusing
  // to search for nothing stays typed instead of an empty match.
  const legacyProbe = native.classifyWindowsSplitRight({ probe: 'split-right', failure: 'SPLIT_RIGHT_NOT_FOUND' });
  expect(legacyProbe.selectorNames).toEqual([...native.SPLIT_AFFORDANCE_NAMES_WIN32]);
  expect(legacyProbe.selectorAutomationIds).toEqual([...native.SPLIT_AFFORDANCE_AUTOMATION_IDS_WIN32]);
  expect(() => native.buildWindowsSplitRightScript(4242, 2500, { names: [], automationIds: [] })).toThrowError(/at least one exact accessible name/);
});

test('split-affordance not-found path keeps the typed verdict and emits a bounded Split inventory', async () => {
  const native = await import('../lib/qa-scenarios/native-driver.mjs');
  const inventory = {
    windows: [
      { hwnd: 19663500, title: 'F', className: 'T', inspectedCount: 412, matchCount: 1, truncated: false, matches: [{ name: 'Split terminal right', controlType: 'ControlType.MenuItem', automationId: '', enabled: true, offscreen: false }] },
      { hwnd: 4787552, title: '', className: 'T', inspectedCount: 0, matchCount: 0, truncated: false, matches: [] },
    ],
    inspectedCount: 412, matchCount: 1, truncated: false, inspectCap: 4000, matchCap: 40,
    filter: 'name or automationId contains "split" (case-insensitive)',
  };
  const probe = {
    probe: 'split-right', selector: 'Split pane right', interactive: true, sessionId: 1,
    mainWindowHandle: 19663500, windowVisible: true, visibleWindowCount: 2,
    windowsSearched: [{ hwnd: 19663500, title: 'F', className: 'T' }, { hwnd: 4787552, title: '', className: 'T' }],
    windowsSearchedCount: 2, matchedWindowHwnd: null,
    windowSearchDepths: [{ hwnd: 19663500, depth: 10, containsFocus: true }, { hwnd: 4787552, depth: 0, containsFocus: false }],
    focusedFound: true, focusSource: null, scopeDepth: 10, scopeIsWindowRoot: false,
    candidateCount: 0, actionableCount: 0, candidates: [], chosen: null,
    failure: 'SPLIT_RIGHT_NOT_FOUND',
    detail: 'no ancestor of the focused pane contains the split affordance in any visible owned window',
    inventory,
  };
  const verdict = native.classifyWindowsSplitRight(probe);
  expect(verdict.ok).toBe(false);
  expect(verdict.code).toBe('SPLIT_RIGHT_NOT_FOUND');
  expect(verdict.derivedCode).toBe('SPLIT_RIGHT_NOT_FOUND');
  // Every pre-existing measured field survives.
  expect(verdict.visibleWindowCount).toBe(2);
  expect(verdict.windowsSearchedCount).toBe(2);
  expect(verdict.windowsSearched.map(window => window.hwnd)).toEqual([19663500, 4787552]);
  expect(verdict.windowSearchDepths).toHaveLength(2);
  expect(verdict.scopeDepth).toBe(10);
  expect(verdict.focusedFound).toBe(true);
  expect(verdict.candidates).toEqual([]);
  expect(verdict.actionableCount).toBe(0);
  expect(verdict.chosen).toBeNull();
  // The inventory names what IS there, per window, bounded.
  expect(verdict.inventory.matchCount).toBe(1);
  expect(verdict.inventory.inspectedCount).toBe(412);
  expect(verdict.inventory.windows).toHaveLength(2);
  expect(verdict.inventory.windows[0].matches[0].name).toBe('Split terminal right');
  // The recorded detail stays bounded: the inventory is a structured field.
  expect(verdict.detail).not.toContain('Split terminal right');
  expect(verdict.detail).toContain('"scopeDepth":10');
  // A probe that predates the inventory still classifies typed.
  const legacy = native.classifyWindowsSplitRight({ ...probe, inventory: null });
  expect(legacy.code).toBe('SPLIT_RIGHT_NOT_FOUND');
  expect(legacy.inventory).toBeNull();
  // The generated probe builds the inventory only on the not-found path, with
  // the bounded caps, and keeps every typed verdict.
  const script = native.buildWindowsSplitRightScript(4242, 2500, { windows: [{ hwnd: 1, title: '', className: 'T' }] });
  expect(script).toContain('$diag.inventory = BuildInventory $searched $inventoryInspectCap $inventoryMatchCap;');
  expect(script).toContain('$inventoryMatchCap = 40;');
  expect(script).toContain('$inventoryInspectCap = 4000;');
  expect(script).toContain('if ($inspected -ge $inspectCap) { $windowTruncated = $true; break };');
  expect(script).toContain('if ($matchList.Count -lt $matchCap) {');
  // The inventory is emitted on the not-found path only, before its typed Fail
  // (the fail is located by its own detail text: an earlier focus failure line
  // carries the same typed code).
  const notFoundStart = script.indexOf('if ($scope -eq $null) {');
  const notFoundFail = script.indexOf('no ancestor of the focused pane contains the split affordance in any visible owned window');
  expect(notFoundStart).toBeGreaterThan(-1);
  expect(notFoundFail).toBeGreaterThan(notFoundStart);
  expect(script.slice(notFoundStart, notFoundFail)).toContain('$diag.inventory = BuildInventory $searched $inventoryInspectCap $inventoryMatchCap;');
  for (const code of ['NO_OWNED_WINDOW', 'SPLIT_RIGHT_NOT_FOUND', 'SPLIT_RIGHT_NOT_UNIQUE', 'SPLIT_RIGHT_DISABLED']) {
    expect(script).toContain(code);
  }
});

// ---------------------------------------------------------------------------
// Pass-20 blocker (verifier REPORT-PASS20 trap: the split search failed on focus
// RESOLUTION instead of falling back to the window). The driver reported
// `SPLIT_RIGHT_NOT_FOUND` with `focusedFound:false` / `scopeDepth:-1` while an
// independent probe's UIA dump at the same moment showed
// `ControlType.Button | Split pane right` present in the same 108-element window:
// the focus-scoped search never ran. These tests replay the generated script and
// its classifier; nothing here launches PowerShell, a product, or a window.

test('split-affordance search falls back to the window roots when no focused pane can be resolved', async () => {
  const native = await import('../lib/qa-scenarios/native-driver.mjs');
  const script = native.buildWindowsSplitRightScript(4242, 2500, { windows: [{ hwnd: 19663500, title: 'F', className: 'T' }] });
  const lines = powerShellScriptLines(script);
  // The pass-20 signature: focus is a scope PREFERENCE now, never a precondition
  // that fails the search before it starts.
  expect(script).not.toContain("Fail 'SPLIT_RIGHT_NOT_FOUND' 'no focused pane could be identified");
  expect(lines).toContain("if ($focused -eq $null) { $diag.scopeOrigin = 'window-root' } else { $diag.scopeOrigin = 'focused-pane' }");
  expect(lines).toContain('  if ($focused -eq $null) {');
  // Every visible owned window's root is searched, and the roots that expose the
  // bound name are POOLED before the uniqueness decision - so a window-wide run
  // can never click the first of several matches.
  expect(lines).toContain('      $fallbackMatches = $fallbackRoot.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition);');
  expect(lines).toContain('        $scopeRoots.Add($fallbackRoot) | Out-Null;');
  expect(lines).toContain('$items = New-Object System.Collections.ArrayList;');
  expect(lines).toContain('foreach ($scopeNode in $scopeRoots) {');
  const pooled = script.indexOf('foreach ($scopeNode in $scopeRoots) {');
  const counted = script.indexOf('$diag.actionableCount = $actionable.Count;');
  const ambiguous = script.indexOf("Fail 'SPLIT_RIGHT_NOT_UNIQUE'");
  const disabled = script.indexOf("Fail 'SPLIT_RIGHT_DISABLED'");
  // Pool first, count second, then the ambiguity rule - the decision is made over
  // the whole searched scope. (`Fail 'SPLIT_RIGHT_NOT_FOUND'` also appears on the
  // walk's own not-found path, which is deliberately BEFORE the pooling, so the
  // verdict block's own ordering is what these three assert.)
  expect(pooled).toBeGreaterThan(-1);
  expect(counted).toBeGreaterThan(pooled);
  expect(ambiguous).toBeGreaterThan(counted);
  expect(disabled).toBeGreaterThan(ambiguous);
  // The clicked element is still addressed BY INDEX into the pooled list, and
  // the click is still the only branch that clicks.
  expect(lines).toContain('  $chosen = $items.Item([int]$actionable[0].index);');
  expect(lines.map(line => line.trim())).toContain('$invoke.Invoke();');
  // The typed not-found path still emits the bounded inventory before its Fail,
  // and the fallback did not remove a single typed code.
  const notFoundStart = script.indexOf('if ($scope -eq $null) {');
  const notFoundFail = script.indexOf('no ancestor of the focused pane contains the split affordance in any visible owned window');
  expect(notFoundStart).toBeGreaterThan(-1);
  expect(notFoundFail).toBeGreaterThan(notFoundStart);
  expect(script.slice(notFoundStart, notFoundFail)).toContain('$diag.inventory = BuildInventory $searched $inventoryInspectCap $inventoryMatchCap;');
  for (const code of ['NO_OWNED_WINDOW', 'SPLIT_RIGHT_NOT_FOUND', 'SPLIT_RIGHT_NOT_UNIQUE', 'SPLIT_RIGHT_DISABLED']) {
    expect(script).toContain(code);
  }
  // The three verdict details name the scope that actually searched, so a
  // window-root run is never reported as a focused-pane miss.
  expect(lines).toContain("if ($items.Count -eq 0) { Fail 'SPLIT_RIGHT_NOT_FOUND' \"the searched $($diag.scopeOrigin) scope contains no element with the bound accessible name\" }");
});

test('the window-root fallback reports a real match or a real ambiguity, never an invented one', async () => {
  const native = await import('../lib/qa-scenarios/native-driver.mjs');
  const candidate = (index, extra = {}) => ({
    index, name: 'Split pane right', controlType: 'ControlType.Button', automationId: '',
    enabled: true, offscreen: false, rectEmpty: false, rect: '10,10,20,20', inWindow: true, ...extra,
  });
  // The pass-20 probe shape: no focused element could be resolved, so the window
  // roots were the searched scope.
  const base = {
    probe: 'split-right', selector: 'Split pane right', interactive: true, sessionId: 1,
    mainWindowHandle: 19663500, windowVisible: true, visibleWindowCount: 2,
    windowsSearched: [{ hwnd: 19663500, title: 'F', className: 'T' }, { hwnd: 4787552, title: '', className: 'T' }],
    windowsSearchedCount: 2,
    focusedFound: false, focusSource: null, scopeOrigin: 'window-root', scopeDepth: 0, scopeIsWindowRoot: true,
  };
  // Exactly one actionable match across the pooled window roots: click it, and
  // record which scope produced it.
  const clicked = native.classifyWindowsSplitRight({ ...base, result: 'SPLIT_CLICKED', candidateCount: 1, actionableCount: 1, candidates: [candidate(0)], chosen: candidate(0) });
  expect(clicked.ok).toBe(true);
  expect(clicked.scopeOrigin).toBe('window-root');
  expect(clicked.chosen.index).toBe(0);
  // Two actionable matches pooled from two window roots: still typed, with the
  // full candidate list - the fallback is not allowed to resolve ambiguity.
  const ambiguous = native.classifyWindowsSplitRight({ ...base, candidateCount: 2, actionableCount: 2, candidates: [candidate(0), candidate(1)], failure: 'SPLIT_RIGHT_NOT_UNIQUE' });
  expect(ambiguous.ok).toBe(false);
  expect(ambiguous.code).toBe('SPLIT_RIGHT_NOT_UNIQUE');
  expect(ambiguous.candidates).toHaveLength(2);
  expect(ambiguous.chosen).toBeNull();
  // Nothing found anywhere: the typed verdict and the window scope are recorded.
  const absent = native.classifyWindowsSplitRight({ ...base, candidateCount: 0, actionableCount: 0, candidates: [], failure: 'SPLIT_RIGHT_NOT_FOUND' });
  expect(absent.ok).toBe(false);
  expect(absent.code).toBe('SPLIT_RIGHT_NOT_FOUND');
  expect(absent.scopeOrigin).toBe('window-root');
  expect(absent.scopeDepth).toBe(0);
  expect(absent.detail).toContain('searched window scope');
  // Present but not actionable is still DISABLED, not a click.
  expect(native.classifyWindowsSplitRight({ ...base, candidateCount: 1, actionableCount: 0, candidates: [candidate(0, { enabled: false })], failure: 'SPLIT_RIGHT_DISABLED' }).code).toBe('SPLIT_RIGHT_DISABLED');
  // A probe that predates the field reports null, never a fabricated origin.
  const legacy = native.classifyWindowsSplitRight({ probe: 'split-right', failure: 'SPLIT_RIGHT_NOT_FOUND' });
  expect(legacy.scopeOrigin).toBeNull();
  expect(legacy.code).toBe('SPLIT_RIGHT_NOT_FOUND');
});

// ---------------------------------------------------------------------------
// Task-9 lane (authored; executed by the sole remote verifier). Two root causes,
// both established by the decisive control in
// `.omo/evidence/local-pane-liveness-completion-replan/task-9/ACCESSIBILITY-EXPERIMENT.md`:
//   1. the debug binary boots against `devUrl` 127.0.0.1:5173 and the runner
//      served nothing, so the webview rendered ERR_CONNECTION_REFUSED;
//   2. with the UI served the app boots EMPTY, so no pane - and no pane toolbar
//      and no split affordance - existed.
// These tests replay the serve decision and the pane binding; nothing here
// launches a server on a fixed port, a product process, a window, or a task.

test('frontend serve decision uses the built ui/dist and fails typed when it is absent', async () => {
  const frontend = await import('../lib/qa-scenarios/frontend-server.mjs');
  const root = fixtureRoot();
  try {
    // No dist at all: the debug binary's devUrl would render an error page, so
    // the run blocks typed instead of launching into that page.
    expect(() => frontend.resolveFrontendDist(root)).toThrowError(/FRONTEND_DIST_MISSING/);
    expect(new HarnessError('FRONTEND_DIST_MISSING', 'x').code).toBe('FRONTEND_DIST_MISSING');
    // A dist directory without its index document is still a block.
    mkdirSync(join(root, frontend.FRONTEND_DIST_RELATIVE), { recursive: true });
    expect(() => frontend.resolveFrontendDist(root)).toThrowError(/FRONTEND_DIST_MISSING/);
    // The built assets resolve to the exact index the served route must answer
    // with, and an explicit distDir overrides the default location.
    writeFileSync(join(root, frontend.FRONTEND_DIST_RELATIVE, frontend.FRONTEND_INDEX_FILE), '<!doctype html><div id="root"></div>');
    const resolved = frontend.resolveFrontendDist(root);
    expect(resolved.indexPath).toBe(join(root, frontend.FRONTEND_DIST_RELATIVE, frontend.FRONTEND_INDEX_FILE));
    expect(existsSync(resolved.indexPath)).toBe(true);
    expect(frontend.resolveFrontendDist(root, join(root, frontend.FRONTEND_DIST_RELATIVE)).distDir).toBe(resolved.distDir);
    // The frozen devUrl is the one the config declares.
    expect(frontend.FRONTEND_DEV_URL).toBe('http://127.0.0.1:5173');
    expect(frontend.FRONTEND_PORT).toBe(5173);
    expect(frontend.contentTypeFor('/x/y/index.html')).toContain('text/html');
    expect(frontend.contentTypeFor('/x/y/app.js')).toContain('text/javascript');
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('the served frontend is checked against the config devUrl, and a drift blocks typed', async () => {
  const frontend = await import('../lib/qa-scenarios/frontend-server.mjs');
  const root = fixtureRoot();
  try {
    // A tree without the config has nothing to compare: not a block.
    expect(frontend.assertFrontendDevUrlMatches(root)).toEqual({ devUrl: null, checked: false });
    mkdirSync(join(root, 'src-tauri'), { recursive: true });
    writeFileSync(join(root, frontend.TAURI_CONF_RELATIVE), JSON.stringify({ build: { devUrl: 'http://127.0.0.1:5173', frontendDist: '../ui/dist' } }));
    expect(frontend.assertFrontendDevUrlMatches(root)).toMatchObject({ devUrl: 'http://127.0.0.1:5173', checked: true });
    // A drift would put the run back on the error page: typed, fail-closed.
    writeFileSync(join(root, frontend.TAURI_CONF_RELATIVE), JSON.stringify({ build: { devUrl: 'http://127.0.0.1:5199' } }));
    expect(() => frontend.assertFrontendDevUrlMatches(root)).toThrowError(/FRONTEND_DEVURL_MISMATCH/);
    writeFileSync(join(root, frontend.TAURI_CONF_RELATIVE), '{ not json');
    expect(() => frontend.assertFrontendDevUrlMatches(root)).toThrowError(/FRONTEND_DEVURL_MISMATCH/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('the frontend port is probed, never assumed: a real listener reads occupied and a released one reads free', async () => {
  const { probePortOccupied } = await import('../lib/qa-scenarios/frontend-server.mjs');
  const { createServer } = await import('node:http');
  const foreign = createServer((_req, res) => res.end('foreign'));
  await new Promise(resolvePromise => foreign.listen(0, '127.0.0.1', resolvePromise));
  const port = foreign.address().port;
  try {
    expect(await probePortOccupied({ host: '127.0.0.1', port, timeoutMs: 2000 })).toBe(true);
  } finally {
    await new Promise(resolvePromise => foreign.close(resolvePromise));
  }
  // The same port, now genuinely free: the probe measures, it does not guess.
  expect(await probePortOccupied({ host: '127.0.0.1', port, timeoutMs: 2000 })).toBe(false);
});

test('the served frontend answers with the app root document and is torn down by the registry cleanup', async () => {
  const frontend = await import('../lib/qa-scenarios/frontend-server.mjs');
  const root = fixtureRoot();
  const registry = new ResourceRegistry();
  const actions = [];
  try {
    mkdirSync(join(root, 'ui', 'dist'), { recursive: true });
    writeFileSync(join(root, 'ui', 'dist', frontend.FRONTEND_INDEX_FILE), '<!doctype html><div id="root">Ferryx</div>');
    const served = await frontend.ensureFrontendServed({ rootDir: root, registry, evidence: { action: action => actions.push(action) }, port: 0 });
    expect(served.port).toBeGreaterThan(0);
    expect(served.indexBytes).toBeGreaterThan(0);
    expect(served.route).toBe('static-ui-dist');
    expect(actions.map(action => action.action)).toEqual(['frontend.served']);
    expect(served.server.listening).toBe(true);
    // Teardown rides the existing cleanup path, with a receipt.
    const receipts = await registry.cleanup();
    expect(receipts.find(receipt => receipt.kind === 'server')).toMatchObject({ label: 'frontend-dist', closed: true });
    expect(served.server.listening).toBe(false);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('an occupied frontend port is refused typed and the foreign listener is left running', async () => {
  const frontend = await import('../lib/qa-scenarios/frontend-server.mjs');
  const { createServer } = await import('node:http');
  const root = fixtureRoot();
  const registry = new ResourceRegistry();
  const foreign = createServer((_req, res) => res.end('<div id="root">foreign</div>'));
  await new Promise(resolvePromise => foreign.listen(0, '127.0.0.1', resolvePromise));
  const port = foreign.address().port;
  try {
    mkdirSync(join(root, 'ui', 'dist'), { recursive: true });
    writeFileSync(join(root, 'ui', 'dist', frontend.FRONTEND_INDEX_FILE), '<div id="root"></div>');
    await expect(frontend.ensureFrontendServed({ rootDir: root, registry, port })).rejects.toThrowError(/FRONTEND_PORT_OCCUPIED/);
    // Never killed, never signalled, never reused - and nothing was registered
    // for cleanup because this run opened no listener.
    expect(foreign.listening).toBe(true);
    expect(registry.servers).toHaveLength(0);
    const stillOccupied = await frontend.probePortOccupied({ host: '127.0.0.1', port, timeoutMs: 2000 });
    expect(stillOccupied).toBe(true);
  } finally {
    await new Promise(resolvePromise => foreign.close(resolvePromise));
    rmSync(root, { recursive: true, force: true });
  }
});

test('a frontend that does not answer with the app root document blocks typed and is still torn down', async () => {
  const frontend = await import('../lib/qa-scenarios/frontend-server.mjs');
  const root = fixtureRoot();
  const registry = new ResourceRegistry();
  try {
    mkdirSync(join(root, 'ui', 'dist'), { recursive: true });
    writeFileSync(join(root, 'ui', 'dist', frontend.FRONTEND_INDEX_FILE), '<div id="root"></div>');
    // A 200 without the app's own root document is not readiness.
    await expect(frontend.ensureFrontendServed({
      rootDir: root, registry, port: 0,
      deps: { fetchIndexBody: async () => ({ status: 200, body: '<html>some other page</html>' }) },
    })).rejects.toThrowError(/FRONTEND_NOT_SERVED/);
    // A request that never answers is the same typed block.
    await expect(frontend.ensureFrontendServed({
      rootDir: root, registry, port: 0,
      deps: { fetchIndexBody: async () => { throw new Error('connection reset'); } },
    })).rejects.toThrowError(/FRONTEND_NOT_SERVED/);
    // Both attempts are closed by the same cleanup pass: a blocked run leaves no
    // listener behind.
    const receipts = await registry.cleanup();
    expect(receipts.filter(receipt => receipt.kind === 'server').map(receipt => receipt.closed)).toEqual([true, true]);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('pane binding binds the pane the app itself presented and excludes the fixture sessions', async () => {
  const { bindPaneSession, PANE_PRESENTATION_RECEIPT } = await import('../lib/qa-scenarios/pane-binding.mjs');
  const root = fixtureRoot();
  const hub = new BarrierHub(root, { runId: 'run-pane', operationId: 'op-pane' });
  const actions = [];
  const tuple = backendSessionId => ({
    backendSessionId, incarnation: 'inc-1', daemonEpoch: '7',
    frontendSessionId: `f-${backendSessionId}`, paneIdentity: `p-${backendSessionId}`,
    bindingKey: `k-${backendSessionId}`, attemptGeneration: 1,
  });
  const line = payload => JSON.stringify({ runId: 'run-pane', operationId: 'op-pane', producer: 'surface-host-gpu-render', ...payload });
  try {
    // The fixture session is a daemon session the product created at boot, before
    // any pane existed; it can never be the pane this step opened.
    writeFileSync(join(hub.dir, `${PANE_PRESENTATION_RECEIPT}.receipt.jsonl`),
      `${line({ sessionId: 'b-fixture', attachTuple: tuple('b-fixture'), presented: true })}\n`
      + `${line({ sessionId: 'b-pane', attachTuple: tuple('b-pane'), presented: true })}\n`);
    const binding = await bindPaneSession({
      evidence: { action: action => actions.push(action) },
      barrierHub: hub,
      fixture: { sessions: [{ kind: 'source', backendSessionId: 'b-fixture', ownershipReceipt: { owned: true } }] },
      timeoutMs: 500,
    });
    expect(binding.backendSessionId).toBe('b-pane');
    expect(binding.paneIdentity).toBe('p-b-pane');
    expect(binding.fixtureSessionIds).toEqual(['b-fixture']);
    expect(binding.observedPaneSessionIds).toEqual(['b-pane']);
    expect(actions.map(action => action.action)).toEqual(['pane-session-bound']);
    // Which source settled the binding is part of the evidence (task-9 pass-13):
    // the product's own tuple settled this one.
    expect(actions[0].settledBy).toBe('presentation-tuple');
    // A pane that never presents is a typed block, never an assumed binding -
    // and the block has to say WHICH stream it read and WHAT that stream held:
    // "no pane was created" (an absent or empty stream) and "a pane was created
    // but never presented" (lines that named only fixture sessions) are
    // different defects, and the observed session list alone cannot tell them
    // apart. This is the exact pass-9 failure shape: `observed sessions: []` with
    // no way to tell which of the two it was.
    writeFileSync(join(hub.dir, `${PANE_PRESENTATION_RECEIPT}.receipt.jsonl`),
      `${line({ sessionId: 'b-fixture', attachTuple: tuple('b-fixture'), presented: true })}\n`);
    const fixtureOnly = await bindPaneSession({
      barrierHub: hub,
      fixture: { sessions: [{ kind: 'source', backendSessionId: 'b-fixture', ownershipReceipt: { owned: true } }] },
      timeoutMs: 300,
    }).then(() => null, error => error);
    expect(fixtureOnly.code).toBe('PANE_BINDING_UNBOUND');
    expect(fixtureOnly.detail).toContain(join(hub.dir, `${PANE_PRESENTATION_RECEIPT}.receipt.jsonl`));
    expect(fixtureOnly.detail).toContain('receipt lines: 1');
    expect(fixtureOnly.detail).toContain('sessions named by those lines: ["b-fixture"]');
    expect(fixtureOnly.detail).toContain('fixture sessions excluded: ["b-fixture"]');
    expect(fixtureOnly.detail).toContain('observed pane sessions: []');
    // Nothing in the stream at all: the same typed code, and the report says so
    // instead of leaving the reader to guess.
    writeFileSync(join(hub.dir, `${PANE_PRESENTATION_RECEIPT}.receipt.jsonl`), '');
    const emptyStream = await bindPaneSession({
      barrierHub: hub,
      fixture: { sessions: [{ kind: 'source', backendSessionId: 'b-fixture', ownershipReceipt: { owned: true } }] },
      timeoutMs: 300,
    }).then(() => null, error => error);
    expect(emptyStream.code).toBe('PANE_BINDING_UNBOUND');
    expect(emptyStream.detail).toContain('receipt lines: 0');
    expect(emptyStream.detail).toContain('sessions named by those lines: []');
    expect(emptyStream.detail).toContain('fixture sessions excluded: ["b-fixture"]');
    // Two distinct non-fixture panes cannot be told apart: ambiguous, typed.
    writeFileSync(join(hub.dir, `${PANE_PRESENTATION_RECEIPT}.receipt.jsonl`),
      `${line({ sessionId: 'b-pane', attachTuple: tuple('b-pane'), presented: true })}\n`
      + `${line({ sessionId: 'b-other', attachTuple: tuple('b-other'), presented: true })}\n`);
    await expect(bindPaneSession({ barrierHub: hub, timeoutMs: 500 })).rejects.toThrowError(/PANE_BINDING_AMBIGUOUS/);
    // A receipt without the authoritative seven fields is not a binding.
    writeFileSync(join(hub.dir, `${PANE_PRESENTATION_RECEIPT}.receipt.jsonl`),
      `${line({ sessionId: 'b-pane', attachTuple: { backendSessionId: 'b-pane' }, presented: true })}\n`);
    await expect(bindPaneSession({ barrierHub: hub, timeoutMs: 500 })).rejects.toThrowError(/missing 7-tuple field/);

    // -----------------------------------------------------------------------
    // The measured daemon inventory delta (task-9 pass-13). The pre-split pane's
    // presentation producer requires the seven-field attachTuple and emits
    // NOTHING without it, so when no receipt settles a session the binding falls
    // back to the sessions the isolated daemon reports AFTER the click minus the
    // ones it reported BEFORE, excluding the settled fixture sessions.
    // -----------------------------------------------------------------------
    const inventoryOf = (sessions, extra = {}) => ({ ok: true, code: null, detail: null, transport: 'unix-socket', endpoint: { transport: 'unix-socket', socketPath: '/tmp/x/daemon.sock' }, sessions, epoch: 7, elapsedMs: 1, ...extra });
    // An empty stream: the receipt path settles nothing, so the delta is used.
    writeFileSync(join(hub.dir, `${PANE_PRESENTATION_RECEIPT}.receipt.jsonl`), '');
    let afterReads = 0;
    const deltaBinding = await bindPaneSession({
      evidence: { action: action => actions.push(action) },
      barrierHub: hub,
      fixture: { sessions: [{ kind: 'source', backendSessionId: 'b-fixture', ownershipReceipt: { owned: true } }] },
      inventory: {
        before: inventoryOf(['b-fixture']),
        readAfter: async () => { afterReads += 1; return inventoryOf(['b-fixture', 'b-pane']); },
      },
      timeoutMs: 300,
    });
    expect(afterReads).toBe(1);
    expect(deltaBinding.settledBy).toBe('inventory-delta');
    expect(deltaBinding.backendSessionId).toBe('b-pane');
    expect(deltaBinding.source).toBe('daemon-inventory-delta');
    expect(deltaBinding.fixtureSessionIds).toEqual(['b-fixture']);
    expect(deltaBinding.observedPaneSessionIds).toEqual(['b-pane']);
    expect(deltaBinding.inventoryDelta.added).toEqual(['b-pane']);
    expect(deltaBinding.inventoryDelta.removed).toEqual([]);
    expect(deltaBinding.inventoryDelta.epoch).toBe(7);
    // The three frontend-owned identities are ABSENT on this path, never
    // invented: the daemon has no concept of paneIdentity/bindingKey.
    expect(deltaBinding.frontendSessionId).toBeUndefined();
    expect(deltaBinding.paneIdentity).toBeUndefined();
    expect(deltaBinding.bindingKey).toBeUndefined();
    expect(actions[actions.length - 1].action).toBe('pane-session-bound');
    expect(actions[actions.length - 1].settledBy).toBe('inventory-delta');
    // The fixture session is excluded even when it appears inside the click
    // window: it is a session the product created before any pane existed, so it
    // can never be the pane the click opened.
    const fixtureRecreated = await bindPaneSession({
      barrierHub: hub,
      fixture: { sessions: [{ kind: 'source', backendSessionId: 'b-fixture', ownershipReceipt: { owned: true } }] },
      inventory: { before: inventoryOf(['b-old']), readAfter: async () => inventoryOf(['b-fixture', 'b-pane']) },
      timeoutMs: 300,
    });
    expect(fixtureRecreated.backendSessionId).toBe('b-pane');
    expect(fixtureRecreated.observedPaneSessionIds).toEqual(['b-pane']);
    // Zero added sessions: typed UNBOUND, and the detail names what the two
    // measured inventories really held.
    const zeroDelta = await bindPaneSession({
      barrierHub: hub,
      fixture: { sessions: [{ kind: 'source', backendSessionId: 'b-fixture', ownershipReceipt: { owned: true } }] },
      inventory: { before: inventoryOf(['b-fixture', 'b-old']), readAfter: async () => inventoryOf(['b-fixture']) },
      timeoutMs: 300,
    }).then(() => null, error => error);
    expect(zeroDelta.code).toBe('PANE_BINDING_UNBOUND');
    expect(zeroDelta.detail).toContain('the measured daemon inventory delta named no new session either');
    expect(zeroDelta.detail).toContain('before-read 2 session(s) ["b-fixture","b-old"]');
    expect(zeroDelta.detail).toContain('after-read 1 session(s) ["b-fixture"]');
    expect(zeroDelta.detail).toContain('removed ["b-old"]');
    // More than one new session: the SAME one-session guard with the SAME typed
    // code as the receipt path - never "whichever id sorted first" - and the
    // detail names every candidate so the next pass can see what appeared.
    const twoAdded = await bindPaneSession({
      barrierHub: hub,
      fixture: { sessions: [{ kind: 'source', backendSessionId: 'b-fixture', ownershipReceipt: { owned: true } }] },
      inventory: { before: inventoryOf(['b-fixture']), readAfter: async () => inventoryOf(['b-fixture', 'b-pane', 'b-restored']) },
      timeoutMs: 300,
    }).then(() => null, error => error);
    expect(twoAdded.code).toBe('PANE_BINDING_AMBIGUOUS');
    expect(twoAdded.detail).toContain('["b-pane","b-restored"]');
    expect(twoAdded.detail).toContain('cannot say which pane it split');
    // A read that could not be taken is a typed failure that says WHY, never a
    // guessed binding.
    const unreadable = await bindPaneSession({
      barrierHub: hub,
      fixture: { sessions: [{ kind: 'source', backendSessionId: 'b-fixture', ownershipReceipt: { owned: true } }] },
      inventory: { before: { ok: false, code: 'DAEMON_RUNTIME_MISSING', detail: 'no daemon socket at /tmp/x/runtime/daemon.sock' }, readAfter: async () => inventoryOf(['b-pane']) },
      timeoutMs: 300,
    }).then(() => null, error => error);
    expect(unreadable.code).toBe('PANE_BINDING_UNBOUND');
    expect(unreadable.detail).toContain('could not be measured: before-read DAEMON_RUNTIME_MISSING');
    // No inventory wired at all (a scenario that never needed it) keeps the same
    // typed code and the same enriched receipt detail as before this change.
    const noInventory = await bindPaneSession({
      barrierHub: hub,
      fixture: { sessions: [{ kind: 'source', backendSessionId: 'b-fixture', ownershipReceipt: { owned: true } }] },
      timeoutMs: 300,
    }).then(() => null, error => error);
    expect(noInventory.code).toBe('PANE_BINDING_UNBOUND');
    expect(noInventory.detail).toContain('no daemon inventory reader was wired into the pane step');
    expect(noInventory.detail).toContain('receipt lines: 0');
    // The product's own tuple is PREFERRED: when it settles, the delta is never
    // even read (no wasted read, and no chance of the fallback overriding it).
    writeFileSync(join(hub.dir, `${PANE_PRESENTATION_RECEIPT}.receipt.jsonl`),
      `${line({ sessionId: 'b-pane', attachTuple: tuple('b-pane'), presented: true })}\n`);
    let unusedAfterReads = 0;
    const preferred = await bindPaneSession({
      barrierHub: hub,
      inventory: { before: inventoryOf([]), readAfter: async () => { unusedAfterReads += 1; return inventoryOf(['b-other']); } },
      timeoutMs: 300,
    });
    expect(preferred.settledBy).toBe('presentation-tuple');
    expect(preferred.backendSessionId).toBe('b-pane');
    expect(unusedAfterReads).toBe(0);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

// ---------------------------------------------------------------------------
// The inventory CLIENT itself, against fake daemons that speak the product's own
// framing (newline-delimited JSON; `handshake`/`handshakeOk` then
// `listSessions`/`listSessionsOk`), over BOTH transports.
//
// Split out of the binding test above, and given an explicit socket timeout. That
// test's NAME promises the fixture-exclusion property, and bundling real sockets
// into the same budget let one platform-specific transport hang mask the property
// entirely: on the gate host the whole test died at 10020ms without ever asserting
// the binding it is named for. Here a transport failure names the transport.

// A real socket server whose LISTEN and CLOSE can never stay pending - both are
// hazard points measured in this file:
//   * `listen` with no 'error' handler neither resolves nor rejects when the bind
//     fails (measured on Windows: a Unix-socket path carrying a drive letter fails
//     EACCES and the listen callback never runs at all);
//   * `server.close()` completes only once every connection has ENDED, and a
//     connection whose peer half-closed stays open FOREVER when the server side
//     never reads it - which is exactly the silent-listener case below, whose
//     handler answers nothing. That `close()` sat in a `finally` and hung the test
//     to its deadline with no error to show for it.
// So every server here is opened through `startNetServer`, which tracks its own
// connections and absorbs their reset (a peer tearing its own connection down is
// not a failure of the server under test), and closed through `closeNetServer`,
// which destroys the tracked connections, asks the server to drop any remaining
// ones where the runtime supports it, BOUNDS the wait, and records the outcome in
// `receipts` for the test to assert - instead of swallowing it, and instead of
// throwing from a `finally` where a leaked server would mask a transport failure.
function startNetServer(createNetServer, handler) {
  const connections = new Set();
  const server = createNetServer(socket => {
    connections.add(socket);
    socket.on('close', () => connections.delete(socket));
    socket.on('error', () => { /* the peer closed it: this test asserts the FRAMES, not the teardown */ });
    handler(socket);
  });
  return { server, connections };
}

async function listenNetServer(entry, ...args) {
  await new Promise((resolve, reject) => {
    entry.server.once('error', reject);
    entry.server.listen(...args, () => { entry.server.off('error', reject); resolve(); });
  });
  return entry;
}

async function closeNetServer(entry, label, receipts, { timeoutMs = 2_000 } = {}) {
  const destroyed = entry.connections.size;
  for (const socket of entry.connections) { try { socket.destroy(); } catch { /* already gone */ } }
  entry.server.closeAllConnections?.();
  const outcome = await new Promise(resolve => {
    const timer = setTimeout(() => resolve({ closed: false, timedOut: true }), timeoutMs);
    timer.unref?.();
    entry.server.close(() => { clearTimeout(timer); resolve({ closed: true, timedOut: false }); });
  });
  const receipt = { label, destroyed, ...outcome };
  receipts.push(receipt);
  return receipt;
}

test('the daemon inventory client speaks the product framing over every real transport and types every failure', async () => {
  const inventoryModule = await import('../lib/qa-scenarios/daemon-inventory.mjs');
  const { createServer: createNetServer } = await import('node:net');
  const root = fixtureRoot();
  // Teardown receipts, asserted at the END of the body: a server that could not be
  // closed inside its bound is a LEAK and says so, but a leak can never mask a
  // transport assertion the way a throw from a `finally` would.
  const teardowns = [];
  try {
    const fakeDaemon = ({ sessions, epoch = 7, token = null }) => {
      const received = [];
      const entry = startNetServer(createNetServer, socket => {
        socket.setEncoding('utf8');
        let buffered = '';
        socket.on('data', chunk => {
          buffered += chunk;
          let index = buffered.indexOf('\n');
          while (index !== -1) {
            const frame = JSON.parse(buffered.slice(0, index));
            buffered = buffered.slice(index + 1);
            received.push(frame);
            if (frame.type === 'handshake') {
              if (token !== null && frame.token !== token) {
                socket.write(`${JSON.stringify({ type: 'error', message: 'rejected', code: 'TRANSPORT_UNAUTHORIZED' })}\n`);
                socket.end();
                return;
              }
              socket.write(`${JSON.stringify({ type: 'handshakeOk', version: inventoryModule.DAEMON_PROTOCOL_VERSION, pid: 4242, epoch })}\n`);
            } else if (frame.type === 'listSessions') {
              socket.write(`${JSON.stringify({ type: 'listSessionsOk', epoch, sessions })}\n`);
            }
            index = buffered.indexOf('\n');
          }
        });
      });
      return { ...entry, received };
    };

    // POSIX: the endpoint IS the Unix socket, and no token is presented because
    // the socket's ownership and mode are the credential. The runtime dir name is
    // deliberately one character: a Unix socket path is capped at 104 bytes on
    // macOS, and this temp root is already long.
    //
    // The unix-socket transport is asserted where a filesystem Unix socket is
    // REAL. On Windows there is none: `net.Server.listen` on a path carrying a
    // drive letter fails `EACCES` (measured on the gate host - `listen EACCES:
    // permission denied C:\...\u\daemon.sock`), so the listener callback never ran
    // and this test hung to its 10s deadline without asserting anything. The
    // Windows lane therefore asserts what the same client really does with a
    // unix-socket endpoint there - refuses it, typed and bounded, and never
    // invents a reading from it - while the loopback-port transport a Windows
    // host does have is exercised end to end below.
    const unixRuntime = join(root, 'u');
    mkdirSync(unixRuntime, { recursive: true });
    if (process.platform === 'win32') {
      const noUnixSocket = await inventoryModule.readDaemonSessionInventory({ runtimeDir: unixRuntime, platform: 'linux', totalMs: 3_000 });
      expect(noUnixSocket.ok).toBe(false);
      expect(noUnixSocket.code).toBe('DAEMON_RUNTIME_MISSING');
      expect(noUnixSocket.sessions).toBeNull();
      expect(noUnixSocket.transport).toBe('unix-socket');
    } else {
      const unixFake = fakeDaemon({ sessions: ['s-fixture', 's-pane'], epoch: 11 });
      await listenNetServer(unixFake, join(unixRuntime, 'daemon.sock'));
      try {
        const read = await inventoryModule.readDaemonSessionInventory({ runtimeDir: unixRuntime, platform: 'linux', totalMs: 3_000 });
        expect(read.ok).toBe(true);
        expect(read.sessions).toEqual(['s-fixture', 's-pane']);
        expect(read.epoch).toBe(11);
        expect(read.transport).toBe('unix-socket');
        // READ-ONLY: the only requests on the wire are the two read verbs.
        expect(unixFake.received.map(frame => frame.type)).toEqual(['handshake', 'listSessions']);
        expect(unixFake.received.every(frame => inventoryModule.READ_ONLY_REQUEST_TYPES.includes(frame.type))).toBe(true);
        expect(unixFake.received[0].version).toBe(inventoryModule.DAEMON_PROTOCOL_VERSION);
        expect(unixFake.received[0].token).toBeUndefined();
      } finally { await closeNetServer(unixFake, 'unix-fake-daemon', teardowns); }
    }

    // Windows: the endpoint file holds the loopback port, and the first frame
    // must carry this boot's token (trimmed) as a top-level field.
    const winRuntime = join(root, 'runtime-win');
    mkdirSync(winRuntime, { recursive: true });
    const winFake = fakeDaemon({ sessions: ['s-a'], epoch: 3, token: 'tok-abc' });
    await listenNetServer(winFake, 0, '127.0.0.1');
    const winPort = winFake.server.address().port;
    writeFileSync(join(winRuntime, 'daemon.port'), `${winPort}\n`);
    writeFileSync(join(winRuntime, 'daemon.token'), '  tok-abc \n');
    try {
      const read = await inventoryModule.readDaemonSessionInventory({ runtimeDir: winRuntime, platform: 'win32', totalMs: 3_000 });
      expect(read.ok).toBe(true);
      expect(read.sessions).toEqual(['s-a']);
      expect(read.transport).toBe('loopback-port');
      expect(winFake.received[0]).toEqual({ type: 'handshake', version: inventoryModule.DAEMON_PROTOCOL_VERSION, token: 'tok-abc' });
      // The bearer credential is never part of what the harness records.
      expect(JSON.stringify(read.endpoint)).not.toContain('tok-abc');
      expect(JSON.stringify(inventoryModule.describeDaemonEndpoint(read.endpoint))).not.toContain('tok-abc');
      // A wrong token is refused by the daemon's own first-frame gate, typed.
      writeFileSync(join(winRuntime, 'daemon.token'), 'wrong-token\n');
      const unauthorized = await inventoryModule.readDaemonSessionInventory({ runtimeDir: winRuntime, platform: 'win32', totalMs: 3_000 });
      expect(unauthorized.ok).toBe(false);
      expect(unauthorized.code).toBe('DAEMON_UNAUTHORIZED');
      // Every failure this client can report is one of its declared identities.
      expect(inventoryModule.DAEMON_INVENTORY_FAILURES).toContain(unauthorized.code);
      // Missing runtime files, an unparseable port and an absent token each keep
      // their own typed identity instead of collapsing into one failure.
      writeFileSync(join(winRuntime, 'daemon.token'), 'tok-abc\n');
      const missingRuntime = await inventoryModule.readDaemonSessionInventory({ runtimeDir: join(root, 'runtime-absent'), platform: 'linux', totalMs: 500 });
      expect(missingRuntime.code).toBe('DAEMON_RUNTIME_MISSING');
      writeFileSync(join(winRuntime, 'daemon.port'), 'not-a-port\n');
      const badPort = await inventoryModule.readDaemonSessionInventory({ runtimeDir: winRuntime, platform: 'win32', totalMs: 500 });
      expect(badPort.code).toBe('DAEMON_PORT_INVALID');
      writeFileSync(join(winRuntime, 'daemon.port'), `${winPort}\n`);
      rmSync(join(winRuntime, 'daemon.token'));
      const noToken = await inventoryModule.readDaemonSessionInventory({ runtimeDir: winRuntime, platform: 'win32', totalMs: 500 });
      expect(noToken.code).toBe('DAEMON_TOKEN_MISSING');
      // The token is restored before the silent listener: this case is about the
      // ANSWER never arriving, not about the credential (the case above removed it).
      writeFileSync(join(winRuntime, 'daemon.token'), 'tok-abc\n');
      // A listener that ACCEPTS and never answers: the client must fail typed, never
      // hang. Its connection is destroyed by `closeNetServer` - a handler that answers
      // nothing never reads its socket, so the peer's half-close would otherwise leave
      // this connection open forever and `close()` pending with it.
      const silent = startNetServer(createNetServer, () => {});
      await listenNetServer(silent, 0, '127.0.0.1');
      try {
        writeFileSync(join(winRuntime, 'daemon.port'), `${silent.server.address().port}\n`);
        const timedOut = await inventoryModule.readDaemonSessionInventory({ runtimeDir: winRuntime, platform: 'win32', readTimeoutMs: 300, totalMs: 900 });
        expect(timedOut.ok).toBe(false);
        expect(timedOut.code).toBe('DAEMON_READ_TIMEOUT');
      } finally { await closeNetServer(silent, 'silent-listener', teardowns); }
      // An already-expired total deadline is the same kind of typed refusal.
      writeFileSync(join(winRuntime, 'daemon.port'), `${winPort}\n`);
      const expired = await inventoryModule.readDaemonSessionInventory({ runtimeDir: winRuntime, platform: 'win32', totalMs: 0 });
      expect(expired.code).toBe('DAEMON_DEADLINE');
      // A protocol the harness does not speak is refused by the daemon and
      // reported as such rather than parsed as a session list.
      const mismatchFake = startNetServer(createNetServer, socket => {
        socket.setEncoding('utf8');
        let buffered = '';
        socket.on('data', chunk => {
          buffered += chunk;
          if (buffered.includes('\n')) socket.write(`${JSON.stringify({ type: 'protocolMismatch', expectedVersion: 6, receivedVersion: 5 })}\n`);
        });
      });
      await listenNetServer(mismatchFake, 0, '127.0.0.1');
      try {
        writeFileSync(join(winRuntime, 'daemon.port'), `${mismatchFake.server.address().port}\n`);
        const mismatch = await inventoryModule.readDaemonSessionInventory({ runtimeDir: winRuntime, platform: 'win32', totalMs: 1_000 });
        expect(mismatch.code).toBe('DAEMON_PROTOCOL_MISMATCH');
      } finally { await closeNetServer(mismatchFake, 'protocol-mismatch-daemon', teardowns); }
      // The measured delta is pure arithmetic over the two inventories, and the
      // fixture sessions are excluded from the added set.
      expect(inventoryModule.computeInventoryDelta({
        before: { sessions: ['f', 'a'] }, after: { sessions: ['f', 'a', 'b'] }, fixtureSessionIds: ['f'],
      })).toEqual({ added: ['b'], removed: [], beforeCount: 2, afterCount: 3, fixtureExcluded: ['f'] });
      expect(inventoryModule.computeInventoryDelta({
        before: { sessions: ['f'] }, after: { sessions: ['f', 'n1', 'n2'] }, fixtureSessionIds: ['f'],
      }).added).toEqual(['n1', 'n2']);
    } finally { await closeNetServer(winFake, 'loopback-fake-daemon', teardowns); }

    // The reader factory wires the runtime dir, consumes the pre-trigger setup
    // budget, and records the read as evidence without the token.
    const { createPaneInventoryReader } = await import('../lib/qa-scenarios/pane-binding.mjs');
    const readerActions = [];
    const budgetCalls = [];
    const reader = createPaneInventoryReader({
      isolationRoot: root,
      platform: 'win32',
      evidence: { action: action => readerActions.push(action) },
      budget: { consume: (cap, label) => { budgetCalls.push([cap, label]); return 700; } },
      readInventory: async request => {
        expect(request.runtimeDir).toBe(join(root, 'runtime'));
        expect(request.platform).toBe('win32');
        expect(request.totalMs).toBe(700);
        return { ok: true, code: null, detail: null, transport: 'loopback-port', endpoint: { transport: 'loopback-port', portPath: join(root, 'runtime', 'daemon.port'), tokenPath: join(root, 'runtime', 'daemon.token') }, sessions: ['s2', 's1'], epoch: 5, elapsedMs: 2 };
      },
    });
    expect(reader.runtimeDir).toBe(join(root, 'runtime'));
    const snapshotted = await reader.snapshot('pane-inventory-before');
    expect(snapshotted.ok).toBe(true);
    expect(budgetCalls).toEqual([[BUDGETS.daemonInventoryTotalMs, 'pane-inventory-before']]);
    expect(readerActions).toEqual([{
      action: 'pane-inventory-before', ok: true, code: null, detail: null,
      transport: 'loopback-port',
      endpoint: { transport: 'loopback-port', portPath: join(root, 'runtime', 'daemon.port'), tokenPath: join(root, 'runtime', 'daemon.token') },
      sessionCount: 2, sessionIds: ['s1', 's2'], epoch: 5, elapsedMs: 2,
    }]);
    expect(JSON.stringify(readerActions)).not.toContain('tok-abc');
    // Every server this test opened was closed inside its bound (the sockets the
    // silent listener never read are destroyed by `closeNetServer` first). Asserted
    // here, last, so it is reported without ever hiding the transport result.
    expect({ leaked: teardowns.filter(receipt => receipt.closed !== true) }).toEqual({ leaked: [] });
  } finally { rmSync(root, { recursive: true, force: true }); }
  // An explicit budget for a test that exercises real sockets on purpose. Every
  // await inside it is still bounded on its own; this only stops a slow host from
  // reading as a transport defect.
}, 30_000);

test('marker receipts are addressed by session, so another pane cannot satisfy the assertion', async () => {
  const { awaitMarkerReceiptForSession } = await import('../lib/qa-scenarios/pane-binding.mjs');
  const root = fixtureRoot();
  const hub = new BarrierHub(root, { runId: 'run-marker', operationId: 'op-marker' });
  const line = payload => JSON.stringify({ runId: 'run-marker', operationId: 'op-marker', producer: 'terminal-output-observer', ...payload });
  try {
    writeFileSync(join(hub.dir, 'marker-output.receipt.jsonl'),
      `${line({ sessionId: 'b-pane', output: MARKER_TEXT, frameSubmitted: true, ptyCreatedCount: 1 })}\n`);
    const receipt = await awaitMarkerReceiptForSession(hub, 'b-pane', 500, 'unit marker');
    expect(receipt.sessionId).toBe('b-pane');
    expect(receipt.ptyCreatedCount).toBe(1);
    // The pane under test produced nothing: typed FAIL with the observed
    // sessions, instead of a pass that rode the other pane's receipt.
    await expect(awaitMarkerReceiptForSession(hub, 'b-split', 300, 'unit marker'))
      .rejects.toThrowError(/MARKER_SESSION_UNBOUND/);
    await expect(awaitMarkerReceiptForSession(hub, 'b-split', 300, 'unit marker'))
      .rejects.toThrowError(/\["b-pane"\]/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('split scenarios declare the pre-trigger pane step; scenarios that never split do not', () => {
  for (const scenario of ['split-happy', 'split-attach-stall', 'split-cancel', 'split-concurrent']) {
    expect([scenario, SCENARIO_PLANS[scenario].pane]).toEqual([scenario, true]);
    // The fixture contract of every split scenario is unchanged: they still
    // declare and validate their own fixture kinds.
    expect(SCENARIO_FIXTURE_REQUIREMENTS[scenario]).toEqual(['source']);
  }
  for (const scenario of ['diagnostic-classifier', 'retained-handover', 'handover-abort', 'suspension-ownership', 'stale-binding']) {
    expect([scenario, Boolean(SCENARIO_PLANS[scenario].pane)]).toEqual([scenario, false]);
  }
});

// H-18: the conflicting-fingerprint rejection. The `split-concurrent` batch drives
// duplicate/fingerprint-conflict pairs against the real daemon, and the daemon's own
// pre-check (`daemon/session_service.rs::create_split`) rejects a reused request
// identity carrying different parameters with the typed `SPAWN_REQUEST_CONFLICT`.
// This binds the scenario's PASS to that rejection really being reported, and each
// mutation below proves the assertion can fail.
test('split-concurrent binds PASS to the conflict wave really rejecting the reused fingerprint', () => {
  const settled = {
    stage: 'split-concurrent-batch',
    conflictRequests: 2,
    conflictRejected: 2,
    conflictRejectionCodes: ['SpawnRequestConflict'],
    conflictDistinctSessionIds: [],
  };
  expect(assertConflictWaveReported(settled)).toBe(true);

  // Mutation 1: a batch that produced no rejection cannot pass, even though every
  // other field is healthy.
  expect(() => assertConflictWaveReported({ ...settled, conflictRejected: 0, conflictRejectionCodes: [] }))
    .toThrowError(/ASSERTION_FAILURE/);

  // Mutation 2: a rejection under another code is not this rejection.
  expect(() => assertConflictWaveReported({ ...settled, conflictRejectionCodes: ['InternalError'] }))
    .toThrowError(/SPAWN_REQUEST_CONFLICT/);

  // Mutation 3: a batch that never settled (e.g. admission failure) is not a pass.
  expect(() => assertConflictWaveReported({
    stage: 'split-concurrent-batch',
    conflictRejected: 0,
    conflictRejectionCodes: [],
    unsettledReason: 'batch admission failed',
  })).toThrowError(/ASSERTION_FAILURE/);

  // Mutation 4: a missing settlement is not a pass.
  expect(() => assertConflictWaveReported(undefined)).toThrowError(/ASSERTION_FAILURE/);
});

test('split-happy binds its presentation and marker receipts to the SPLIT pane, not to the app pane that settled first', async () => {
  const { runSplitHappyScenario } = await import('../lib/qa-scenarios/split-scenarios.mjs');
  const root = fixtureRoot();
  const runId = 'run-split-bind';
  const operationId = 'op-split-bind';
  const hub = new BarrierHub(root, { runId, operationId });
  const line = payload => JSON.stringify({ runId, operationId, producer: 'surface-host-gpu-render', ...payload });
  const tuple = backendSessionId => ({
    backendSessionId, incarnation: 'inc-split', daemonEpoch: '3',
    frontendSessionId: `f-${backendSessionId}`, paneIdentity: `p-${backendSessionId}`,
    bindingKey: `k-${backendSessionId}`, attemptGeneration: 1,
  });
  const screenshotPath = join(root, 'screenshot.png');
  writeFileSync(screenshotPath, Buffer.from('split-bind-screenshot'));
  writeFileSync(join(hub.dir, 'marker-recognition.json'), JSON.stringify({
    runId, operationId, recognizer: 'test-inspector', text: MARKER_TEXT,
    paneBounds: { x: 0, y: 0, w: 400, h: 400 }, screenshotSha256: computeSourceDigest([screenshotPath]),
  }));
  try {
    writeFileSync(join(hub.dir, 'split-create.receipt.jsonl'),
      `${line({ backendSessionId: 'b-split', incarnation: 'inc-split', daemonEpoch: '3', attemptGeneration: 1 })}\n`);
    // The app's own pane (created by the UI step) presents FIRST, then the split.
    writeFileSync(join(hub.dir, 'presentation.receipt.jsonl'),
      `${line({ sessionId: 'b-pane', attachTuple: tuple('b-pane'), presented: true })}\n`
      + `${line({ sessionId: 'b-split', attachTuple: tuple('b-split'), presented: true })}\n`);
    writeFileSync(join(hub.dir, 'marker-output.receipt.jsonl'),
      `${line({ sessionId: 'b-pane', output: MARKER_TEXT, frameSubmitted: true, ptyCreatedCount: 1 })}\n`
      + `${line({ sessionId: 'b-split', output: MARKER_TEXT, frameSubmitted: true, ptyCreatedCount: 1 })}\n`);
    const result = await runSplitHappyScenario({
      scenario: 'split-happy',
      evidence: { action: () => {} },
      barrierHub: hub,
      pid: 1234,
      platformPreflight: 'mock',
      evidenceRunDir: root,
      runId,
      operationId,
    }, SCENARIO_PLANS['split-happy'], new MonotonicBudget());
    // Both the seven-field tuple and the marker belong to the split's own pane.
    expect(result.createReceipt.backendSessionId).toBe('b-split');
    expect(result.presentationReceipt.attachTuple.backendSessionId).toBe('b-split');
    expect(result.markerReceipt.sessionId).toBe('b-split');
    expect(result.markerReceipt.ptyCreatedCount).toBe(1);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('a split whose own pane never presents or reports cannot pass on the app pane receipts', async () => {
  const { runSplitHappyScenario } = await import('../lib/qa-scenarios/split-scenarios.mjs');
  const root = fixtureRoot();
  const runId = 'run-split-absent';
  const operationId = 'op-split-absent';
  const hub = new BarrierHub(root, { runId, operationId });
  const line = payload => JSON.stringify({ runId, operationId, producer: 'surface-host-gpu-render', ...payload });
  const tuple = backendSessionId => ({
    backendSessionId, incarnation: 'inc-split', daemonEpoch: '3',
    frontendSessionId: `f-${backendSessionId}`, paneIdentity: `p-${backendSessionId}`,
    bindingKey: `k-${backendSessionId}`, attemptGeneration: 1,
  });
  const ctx = {
    scenario: 'split-happy',
    evidence: { action: () => {} },
    barrierHub: hub,
    pid: 1234,
    platformPreflight: 'mock',
    evidenceRunDir: root,
    runId,
    operationId,
  };
  try {
    writeFileSync(join(hub.dir, 'split-create.receipt.jsonl'),
      `${line({ backendSessionId: 'b-split', incarnation: 'inc-split', daemonEpoch: '3', attemptGeneration: 1 })}\n`);
    // Only the app's own pane settles: the split pane is absent, so the scenario
    // must NOT pass on the other pane's tuple.
    writeFileSync(join(hub.dir, 'presentation.receipt.jsonl'),
      `${line({ sessionId: 'b-pane', attachTuple: tuple('b-pane'), presented: true })}\n`);
    await expect(runSplitHappyScenario(ctx, SCENARIO_PLANS['split-happy'], new MonotonicBudget(400)))
      .rejects.toThrowError(/BARRIER_ACK_TIMEOUT/);
    // The split pane presents but never reports the marker: typed FAIL.
    writeFileSync(join(hub.dir, 'presentation.receipt.jsonl'),
      `${line({ sessionId: 'b-pane', attachTuple: tuple('b-pane'), presented: true })}\n`
      + `${line({ sessionId: 'b-split', attachTuple: tuple('b-split'), presented: true })}\n`);
    writeFileSync(join(hub.dir, 'marker-output.receipt.jsonl'),
      `${line({ sessionId: 'b-pane', output: MARKER_TEXT, frameSubmitted: true, ptyCreatedCount: 1 })}\n`);
    await expect(runSplitHappyScenario(ctx, SCENARIO_PLANS['split-happy'], new MonotonicBudget(400)))
      .rejects.toThrowError(/MARKER_SESSION_UNBOUND/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('the pane affordance is searched by its exact accessible name and only one actionable match is clicked', async () => {
  const native = await import('../lib/qa-scenarios/native-driver.mjs');
  expect(native.PANE_AFFORDANCE_NAMES_WIN32).toEqual(['New Terminal']);
  expect(native.PANE_AFFORDANCE_AUTOMATION_IDS_WIN32).toEqual([]);
  expect(native.PANE_AFFORDANCE_SELECTOR_DARWIN).toEqual({ role: 'button', title: 'New Terminal' });
  const script = native.buildWindowsNewPaneScript(4242, { windows: [{ hwnd: 19663500, title: 'F', className: 'T' }] });
  const lines = powerShellScriptLines(script);
  expect(lines).toContain("$selectorNames = @('New Terminal');");
  expect(lines).toContain('$windowHandles = @(19663500);');
  expect(lines).toContain("$windowTitles = @('F');");
  expect(lines).toContain('$condition = $conditionName0;');
  // The click itself. The invoke is its own statement inside the chosen branch's
  // `try`, so the builder emits it indented (`    $invoke.Invoke();`). Assert the
  // statement as a WHOLE line with only its leading indentation normalized away:
  // still exact - a commented-out, nested, or otherwise altered invoke cannot
  // satisfy it - and no longer coupled to the try-block's indentation. The
  // indentation is not the defect (pass 8 confirmed the invoke is present and
  // correct in the emitted script), so the expectation moves, not the builder.
  expect(lines.map(line => line.trim())).toContain('$invoke.Invoke();');
  // Exact property conditions only: no substring filter and no index pick.
  expect(script).not.toMatch(/Name -match|Name -like/);
  // No here-string and no P/Invoke, so the pass-5 defect class cannot recur.
  expect(script).not.toContain('@"');
  expect(script).not.toContain('DllImport');
  expect(lines).not.toContain('');
  expect(() => native.buildWindowsNewPaneScript(4242, { names: [], automationIds: [] })).toThrowError(/refusing to search for nothing/);

  const base = {
    probe: 'new-pane', selectorNames: ['New Terminal'], interactive: true, sessionId: 1,
    windowsSearched: [{ hwnd: 1, title: 'F', className: 'T' }], windowsSearchedCount: 1,
  };
  const candidate = over => ({
    windowHwnd: 1, name: 'New Terminal', controlType: 'ControlType.Button', automationId: '',
    enabled: true, offscreen: false, rectEmpty: false, rect: '1,1,10,10', ...over,
  });
  const clicked = native.classifyWindowsNewPane({ ...base, candidateCount: 1, actionableCount: 1, candidates: [candidate()], chosen: candidate(), result: 'PANE_CLICKED' });
  expect(clicked.ok).toBe(true);
  expect(clicked.code).toBeNull();
  expect(native.classifyWindowsNewPane({ ...base, candidateCount: 0, actionableCount: 0, candidates: [], failure: 'PANE_AFFORDANCE_NOT_FOUND' }).code).toBe('PANE_AFFORDANCE_NOT_FOUND');
  expect(native.classifyWindowsNewPane({ ...base, candidateCount: 2, actionableCount: 2, candidates: [candidate(), candidate({ windowHwnd: 2 })] }).code).toBe('PANE_AFFORDANCE_NOT_UNIQUE');
  expect(native.classifyWindowsNewPane({ ...base, candidateCount: 1, actionableCount: 0, candidates: [candidate({ enabled: false })] }).code).toBe('PANE_AFFORDANCE_DISABLED');
  expect(native.classifyWindowsNewPane({ probe: 'new-pane', windowsSearched: [], failure: 'NO_OWNED_WINDOW' }).code).toBe('NO_OWNED_WINDOW');
  // Every pre-existing split verdict is untouched by the new probe.
  expect(native.classifyWindowsSplitRight({ probe: 'split-right', failure: 'SPLIT_RIGHT_NOT_FOUND' }).code).toBe('SPLIT_RIGHT_NOT_FOUND');
});

// ---------------------------------------------------------------------------
// Task-9 pass-22 observability: the post-split daemon inventory (a local split
// create adds a daemon session, so 2 -> 3 separates "the split really created"
// from "the flow bailed before create") and the app-stdio byte self-check on the
// claim that the split flow is invisible in the app's stderr. Both are
// MEASUREMENTS: neither may change a verdict.

test('the split step records a post-click daemon inventory with its session delta and the app-stdio byte self-check', async () => {
  const { createPaneInventoryReader } = await import('../lib/qa-scenarios/pane-binding.mjs');
  const root = fixtureRoot();
  const runId = 'run-split-inventory';
  const operationId = 'op-split-inventory';
  const hub = new BarrierHub(root, { runId, operationId });
  const actions = [];
  const sequence = [];
  // The two readings the daemon really answered: the pane step's own after-read
  // (2 sessions: the fixture and the UI pane) and the split click's effect
  // (3 sessions: the split created a daemon session).
  const inventories = [['fixture-1', 'pane-1'], ['fixture-1', 'pane-1', 'split-1']];
  let readIndex = 0;
  const reader = createPaneInventoryReader({
    runtimeDir: join(root, 'runtime'),
    platform: 'win32',
    evidence: { action: action => actions.push(action) },
    readInventory: async request => {
      expect(request.runtimeDir).toBe(join(root, 'runtime'));
      sequence.push(`read:${readIndex}`);
      const sessions = inventories[Math.min(readIndex, inventories.length - 1)];
      readIndex += 1;
      return {
        ok: true, code: null, detail: null, transport: 'loopback-port',
        endpoint: { transport: 'loopback-port', portPath: join(root, 'runtime', 'daemon.port'), tokenPath: join(root, 'runtime', 'daemon.token') },
        sessions, epoch: 11, elapsedMs: 1,
      };
    },
  });
  // The always-on app-stdio sink the split step self-checks against. The click
  // writes one app line, exactly the kind the lead's reading says the split flow
  // cannot produce - so the recorded byte delta has to show it.
  const appLine = '[cmd_terminal_spawn] request received has_worktree=false has_cwd=true\n';
  const sink = new AppStdioSink(join(root, 'app-stdio'), { label: 'app' });
  const child = { stdout: new EventEmitter(), stderr: new EventEmitter() };
  sink.attach(child);
  const driver = {
    focus: async () => sequence.push('focus'),
    split: async () => { sequence.push('split-click'); child.stderr.emit('data', Buffer.from(appLine)); },
  };
  const fakeHub = {
    ...hub,
    command: name => sequence.push(`command:${name}`),
    awaitReceipt: async () => ({ cancelAckMs: 120, timerDispatchLatencyMs: 5, cleanupReceipt: { authoritative: true, reapedPids: [] }, createdIdRequired: false }),
  };
  try {
    // The pane step's own after-read, exactly as the runner takes it: this is the
    // pre-split reading the split's delta must be measured against.
    await reader.snapshot('pane-inventory-after');
    const result = await runSplitCancelScenario({
      scenario: 'split-cancel',
      evidence: { action: action => actions.push(action) },
      barrierHub: fakeHub,
      pid: 1234,
      platformPreflight: 'darwin',
      nativeDriver: driver,
      paneInventory: reader,
      appStdio: sink,
    }, { cancel: { phase: 'while-creating' } }, new MonotonicBudget());
    expect(result.cancelReceipt.cleanupReceipt.authoritative).toBe(true);

    // (1) the order: the click, then the cancel request (the scenario's own
    // measured moment), then the post-click inventory read - never before the
    // click, and never a second baseline read, because the pane step's own
    // after-read is reused as the baseline.
    expect(sequence.slice(0, 5)).toEqual(['read:0', 'focus', 'split-click', 'command:split-cancel', 'read:1']);
    expect(actions.map(action => action.action)).not.toContain(PRE_SPLIT_INVENTORY_ACTION);

    // (2) the recorded action: sessionCount, sessionIds, and the delta against
    // the pre-split reading.
    const after = actions.find(action => action.action === SPLIT_INVENTORY_ACTION);
    expect(after.ok).toBe(true);
    expect(after.sessionCount).toBe(3);
    expect(after.sessionIds).toEqual(['fixture-1', 'pane-1', 'split-1']);
    expect(after.delta).toEqual({
      added: ['split-1'],
      removed: [],
      beforeCount: 2,
      afterCount: 3,
      // This reader was built without a settled fixture payload, so nothing was
      // excluded - and the field is present anyway, so a reader can tell "nothing
      // was excluded" apart from "the guard was never wired" (F2-14).
      fixtureExcluded: [],
      baselineLabel: 'pane-inventory-after',
      baselineSessionIds: ['fixture-1', 'pane-1'],
    });

    // (3) the app-stdio byte self-check at the same moment: the bytes really grew
    // across the click, so this evidence falsifies the invisibility claim rather
    // than hiding it.
    expect(after.appStdioBytes.before).toEqual({ stdout: 0, stderr: 0, total: 0 });
    expect(after.appStdioBytes.after.total).toBe(Buffer.byteLength(appLine));
    expect(after.appStdioBytes.delta).toEqual({ stdout: 0, stderr: Buffer.byteLength(appLine), total: Buffer.byteLength(appLine) });
    // The projection itself, read straight off the sink: the same counters the
    // action carried, with no file read and no side effect.
    expect(appStdioBytes(sink)).toEqual({ stdout: 0, stderr: Buffer.byteLength(appLine), total: Buffer.byteLength(appLine) });
  } finally {
    await sink.close();
    rmSync(root, { recursive: true, force: true });
  }
});

test('the split delta never reports a fixture session as the split\'s own addition, and says what it excluded', async () => {
  const { createPaneInventoryReader } = await import('../lib/qa-scenarios/pane-binding.mjs');
  const root = fixtureRoot();
  const actions = [];
  // The fixture session is (re)created INSIDE the click window: it is absent from
  // the pre-split reading and present in the post-click one - exactly the shape
  // that used to read as "the split created it" (F2-14). The click itself created
  // one session, `split-1`, and the delta has to say only that.
  const inventories = [['pane-1'], ['fixture-1', 'pane-1', 'split-1']];
  let readIndex = 0;
  const reader = createPaneInventoryReader({
    runtimeDir: join(root, 'runtime'),
    platform: 'win32',
    fixture: { sessions: [{ kind: 'source', backendSessionId: 'fixture-1', ownershipReceipt: { owned: true } }] },
    evidence: { action: action => actions.push(action) },
    readInventory: async () => {
      const sessions = inventories[Math.min(readIndex, inventories.length - 1)];
      readIndex += 1;
      return {
        ok: true, code: null, detail: null, transport: 'unix-socket',
        endpoint: { transport: 'unix-socket', socketPath: join(root, 'runtime', 'daemon.sock') },
        sessions, epoch: 9, elapsedMs: 1,
      };
    },
  });
  try {
    // The pane step's own pre-split baseline, then the split step's post-click read
    // measured against it - the same two reads the runner takes.
    const before = await reader.snapshot(PRE_SPLIT_INVENTORY_ACTION);
    expect(before.ok).toBe(true);
    const after = await reader.snapshot(SPLIT_INVENTORY_ACTION, { compareToPrevious: true });
    expect(after.ok).toBe(true);
    const action = actions.find(candidate => candidate.action === SPLIT_INVENTORY_ACTION);
    // The measurement itself is UNCHANGED: the read really did report three
    // sessions, the fixture among them.
    expect(action.sessionCount).toBe(3);
    expect(action.sessionIds).toEqual(['fixture-1', 'pane-1', 'split-1']);
    // ... but the fixture is never part of what the click ADDED, and the action
    // names what was excluded instead of leaving the reader to trust it.
    expect(action.delta.added).toEqual(['split-1']);
    expect(action.delta.fixtureExcluded).toEqual(['fixture-1']);
    expect(action.delta.beforeCount).toBe(1);
    expect(action.delta.afterCount).toBe(3);
    expect(action.delta.baselineLabel).toBe(PRE_SPLIT_INVENTORY_ACTION);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('the post-split inventory falls back to a fresh pre-split read, and a run with no reader records a typed reason instead of failing', async () => {
  const { createPaneInventoryReader } = await import('../lib/qa-scenarios/pane-binding.mjs');
  const root = fixtureRoot();
  const actions = [];
  const reader = createPaneInventoryReader({
    runtimeDir: join(root, 'runtime'),
    platform: 'win32',
    evidence: { action: action => actions.push(action) },
    readInventory: async () => ({
      ok: true, code: null, detail: null, transport: 'unix-socket',
      endpoint: { transport: 'unix-socket', socketPath: join(root, 'runtime', 'daemon.sock') },
      sessions: actions.filter(action => action.action.startsWith('pane-inventory')).length === 0 ? ['fixture-1'] : ['fixture-1', 'split-1'],
      epoch: 3, elapsedMs: 1,
    }),
  });
  const driver = { focus: async () => {}, split: async () => {} };
  const fakeHub = {
    command: () => {},
    awaitReceipt: async () => ({ cancelAckMs: 5, cleanupReceipt: { authoritative: true } }),
  };
  try {
    // (a) NO pane-step after-read (the pane binding settled on the product's own
    // presentation receipt instead): the baseline is taken HERE, before the click,
    // so the pane step's own session can never be folded into the split's delta.
    const result = await runSplitCancelScenario({
      scenario: 'split-cancel',
      evidence: { action: action => actions.push(action) },
      barrierHub: fakeHub,
      pid: 1,
      platformPreflight: 'darwin',
      nativeDriver: driver,
      paneInventory: reader,
    }, { cancel: { phase: 'while-creating' } }, new MonotonicBudget());
    expect(result.cancelReceipt.cleanupReceipt.authoritative).toBe(true);
    const inventoryActions = actions
      .filter(action => action.action.startsWith('pane-inventory') || action.action === SPLIT_INVENTORY_ACTION)
      .map(action => action.action);
    expect(inventoryActions).toEqual([PRE_SPLIT_INVENTORY_ACTION, SPLIT_INVENTORY_ACTION]);
    expect(actions.find(action => action.action === SPLIT_INVENTORY_ACTION).delta)
      .toMatchObject({ added: ['split-1'], beforeCount: 1, afterCount: 2, baselineLabel: PRE_SPLIT_INVENTORY_ACTION, baselineSessionIds: ['fixture-1'] });

    // (b) no reader wired at all: the action says so, typed, and the scenario's
    // own verdict is untouched - a missing measurement can never turn a blocked
    // run into a failure.
    actions.length = 0;
    const noReader = await runSplitCancelScenario({
      scenario: 'split-cancel',
      evidence: { action: action => actions.push(action) },
      barrierHub: fakeHub,
      pid: 1,
      platformPreflight: 'darwin',
      nativeDriver: driver,
    }, { cancel: { phase: 'while-creating' } }, new MonotonicBudget());
    expect(noReader.cancelReceipt.cancelAckMs).toBe(5);
    const missing = actions.find(action => action.action === SPLIT_INVENTORY_ACTION);
    expect([missing.ok, missing.code, missing.sessionCount, missing.sessionIds, missing.delta, missing.baselineLabel])
      .toEqual([false, INVENTORY_READER_MISSING, null, null, null, null]);
    expect(missing.appStdioBytes).toEqual({ before: null, after: null, delta: null });

    // The same wiring in a second adapter: split-happy still fails with its own
    // typed BARRIER_ACK_TIMEOUT (never ASSERTION_FAILURE) when the measurement
    // cannot be taken.
    actions.length = 0;
    const happyHub = { awaitReceipt: async () => { throw new HarnessError('BARRIER_ACK_TIMEOUT', 'no split-create receipt'); } };
    await expect(runSplitHappyScenario({
      scenario: 'split-happy',
      evidence: { action: action => actions.push(action) },
      barrierHub: happyHub,
      pid: 1,
      platformPreflight: 'darwin',
      nativeDriver: driver,
    }, SCENARIO_PLANS['split-happy'], new MonotonicBudget()))
      .rejects.toThrowError(/BARRIER_ACK_TIMEOUT/);
    expect(actions.find(action => action.action === SPLIT_INVENTORY_ACTION).code).toBe(INVENTORY_READER_MISSING);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('the run preserves the barrier hub directory into its evidence dir, bounded and never fatal', () => {
  const root = fixtureRoot();
  const hubDir = join(root, 'isolation', 'barriers');
  const runDir = join(root, 'evidence', 'task-3-harness', 'split-happy', 'run-x');
  mkdirSync(hubDir, { recursive: true });
  mkdirSync(runDir, { recursive: true });
  // The four artifact families a hub really holds: the pre-armed spec, the
  // product's registration ACK, its live binding ACK, and the append-only
  // receipt stream the pass-21 question was about.
  const artifacts = {
    'split-create.arm.json': '{"name":"split-create"}',
    'split-create.armed-ack.json': '{"producer":"qa-barrier"}\n',
    'attach-handshake.bound-ack.json': '{"targetBackendSessionId":"b1"}\n',
    'split-create.receipt.jsonl': '{"backendSessionId":"b1"}\n',
  };
  for (const [name, text] of Object.entries(artifacts)) writeFileSync(join(hubDir, name), text);
  mkdirSync(join(hubDir, 'nested'), { recursive: true });
  const artifactBytes = Object.values(artifacts).reduce((total, text) => total + Buffer.byteLength(text), 0);
  try {
    // (1) every regular file is preserved byte for byte under the run's own
    // evidence dir, and the subdirectory is named as skipped, not silently lost.
    const archive = archiveBarrierHub(hubDir, runDir);
    expect([archive.ok, archive.reason, archive.truncated, archive.maxBytes])
      .toEqual([true, null, false, BARRIER_ARCHIVE_MAX_BYTES]);
    expect(archive.dir).toBe(join(runDir, 'barrier-hub'));
    expect(archive.filesCopied).toEqual(Object.keys(artifacts).sort());
    expect(archive.filesSkipped).toEqual([{ name: 'nested', reason: 'NOT_A_FILE' }]);
    expect(archive.bytes).toBe(artifactBytes);
    for (const [name, text] of Object.entries(artifacts)) {
      expect(readFileSync(join(runDir, 'barrier-hub', name), 'utf8')).toBe(text);
    }

    // (2) the byte cap bites at a deterministic point and names what it left
    // behind instead of dropping it.
    const capped = archiveBarrierHub(hubDir, join(root, 'evidence-capped'), { maxBytes: 40 });
    expect(capped.truncated).toBe(true);
    expect(capped.bytes).toBeLessThanOrEqual(40);
    const cappedSkips = capped.filesSkipped.filter(skip => skip.reason.startsWith('BYTE_CAP'));
    expect(cappedSkips.length).toBeGreaterThan(0);
    expect(capped.filesCopied.length + cappedSkips.length).toBe(Object.keys(artifacts).length);

    // (3) never fatal: a missing hub, an unset evidence dir, an unset hub and an
    // fs failure are each a typed reason, never a throw.
    expect(archiveBarrierHub(join(root, 'no-such-hub'), runDir))
      .toMatchObject({ ok: false, reason: 'BARRIER_DIR_MISSING', filesCopied: [], bytes: 0 });
    expect(archiveBarrierHub(hubDir, null)).toMatchObject({ ok: false, reason: 'EVIDENCE_DIR_UNSET' });
    expect(archiveBarrierHub(null, runDir)).toMatchObject({ ok: false, reason: 'BARRIER_DIR_UNSET' });
    const statFails = archiveBarrierHub(hubDir, join(root, 'evidence-stat'), { stat: () => { throw new Error('boom'); } });
    expect([statFails.ok, statFails.filesCopied, statFails.filesSkipped.every(skip => skip.reason.startsWith('STAT_FAILED'))])
      .toEqual([true, [], true]);
    const copyFails = archiveBarrierHub(hubDir, join(root, 'evidence-copy'), { copy: () => { throw new Error('boom'); } });
    expect(copyFails.filesCopied).toEqual([]);
    expect(copyFails.filesSkipped.filter(skip => skip.reason.startsWith('COPY_FAILED')).length).toBe(Object.keys(artifacts).length);

    // (4) the runner's own finalization seam archives the hub into the evidence
    // RUN dir - this is exactly what the runner's `finally` block calls.
    const seamRunDir = join(root, 'evidence-seam');
    const seam = archiveRunBarrierHub({ barrierHub: { dir: hubDir }, evidence: { runDir: seamRunDir } });
    expect(seam.ok).toBe(true);
    expect(existsSync(join(seamRunDir, 'barrier-hub', 'split-create.receipt.jsonl'))).toBe(true);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

// ---------------------------------------------------------------------------
// Pass-22 audit repairs: a measurement may never change a verdict (D1), may never
// sit inside the measured window charged to nothing (D2), and may never present a
// pre-reap snapshot as a stream that settled (D3/D4).

test('a throwing inventory read is recorded as a typed action and the split scenario proceeds unchanged', async () => {
  const root = fixtureRoot();
  const actions = [];
  const budget = new MonotonicBudget();
  const charges = [];
  // The exact failure the audit proved could flip a verdict: the reader's own
  // `snapshot` raising the ASSERTION_FAILURE that `MonotonicBudget.consume`
  // throws when its budget is exhausted - the code `classifyNativeFailure` maps to
  // a FAIL verdict.
  const throwingReader = {
    snapshot: async (label, options) => {
      charges.push({ label, options });
      throw new HarnessError('ASSERTION_FAILURE', 'monotonic budget exhausted for split-inventory-after: elapsed 15001ms >= limit 15000ms');
    },
    lastReading: () => null,
  };
  const driver = { focus: async () => {}, split: async () => {} };
  const fakeHub = {
    command: () => {},
    awaitReceipt: async () => ({ cancelAckMs: 5, cleanupReceipt: { authoritative: true } }),
  };
  try {
    const result = await runSplitCancelScenario({
      scenario: 'split-cancel',
      evidence: { action: action => actions.push(action) },
      barrierHub: fakeHub,
      pid: 1,
      platformPreflight: 'darwin',
      nativeDriver: driver,
      paneInventory: throwingReader,
    }, { cancel: { phase: 'while-creating' } }, budget);
    // (1) the scenario's own settlement is untouched: a measurement that throws
    // cannot turn this scenario into an ASSERTION_FAILURE.
    expect(result.cancelReceipt.cancelAckMs).toBe(5);
    // (2) BOTH reads are recorded - the pre-click read and the post-click one -
    // typed, carrying the thrown error's own code and message.
    const recorded = actions.filter(action => action.action === PRE_SPLIT_INVENTORY_ACTION || action.action === SPLIT_INVENTORY_ACTION);
    expect(recorded.map(action => action.action)).toEqual([PRE_SPLIT_INVENTORY_ACTION, SPLIT_INVENTORY_ACTION]);
    for (const action of recorded) {
      expect([action.ok, action.code, action.cause]).toEqual([false, SPLIT_INVENTORY_READ_FAILED, 'ASSERTION_FAILURE']);
      expect(action.detail).toContain('monotonic budget exhausted');
      expect([action.sessionCount, action.sessionIds, action.delta]).toEqual([null, null, null]);
    }
    // (3) the cost the audit found charged to NOTHING is now charged to the
    // MEASURED attempt budget, at the small bounded cap, for every read of the
    // pair - and no BUDGETS value had to move for that.
    expect(charges.map(charge => charge.label)).toEqual([PRE_SPLIT_INVENTORY_ACTION, SPLIT_INVENTORY_ACTION]);
    for (const charge of charges) {
      expect(charge.options.charge.budget).toBe(budget);
      expect(charge.options.charge.capMs).toBe(BUDGETS.splitInventoryReadCapMs);
    }
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('a read the measured attempt window cannot pay for is refused, typed, instead of being taken', async () => {
  const { createPaneInventoryReader } = await import('../lib/qa-scenarios/pane-binding.mjs');
  const root = fixtureRoot();
  const actions = [];
  const reads = [];
  const reader = createPaneInventoryReader({
    runtimeDir: join(root, 'runtime'),
    platform: 'darwin',
    evidence: { action: action => actions.push(action) },
    readInventory: async () => {
      reads.push('read');
      return {
        ok: true, code: null, detail: null, transport: 'unix-socket',
        endpoint: { transport: 'unix-socket', socketPath: join(root, 'runtime', 'daemon.sock') },
        sessions: ['fixture-1'], epoch: 1, elapsedMs: 1,
      };
    },
  });
  // A REAL budget whose window has nothing left: `remainingMs` clamps at 0.
  const spent = new MonotonicBudget(0);
  const driver = { focus: async () => {}, split: async () => {} };
  const fakeHub = {
    command: () => {},
    awaitReceipt: async () => ({ cancelAckMs: 7, cleanupReceipt: { authoritative: true } }),
  };
  try {
    // (1) the reader refuses it: no read is taken at all, the result is typed, and
    // the action carries the cap and the bound it was refused under.
    const refused = await reader.snapshot(PRE_SPLIT_INVENTORY_ACTION, { charge: { budget: spent, capMs: BUDGETS.splitInventoryReadCapMs } });
    expect([refused.ok, refused.code, refused.cause]).toEqual([false, INVENTORY_READ_REFUSED, ATTEMPT_BUDGET_SPENT]);
    expect(reads).toEqual([]);
    expect(actions[0].charge).toEqual({ capMs: BUDGETS.splitInventoryReadCapMs, boundMs: 0, scope: 'measured-attempt' });

    // (2) the same refusal inside a real scenario: its own stages still run and
    // its settlement is unchanged, and the measurement costs the window nothing.
    // The window's two readings are separated here on purpose - `consume` (what
    // the stages ask) still pays, `remainingMs` (what the charge asks) is spent -
    // because a real `MonotonicBudget` cannot be both at once, and waiting for a
    // real one to expire would make this test pass by timing luck.
    const spentForMeasurement = {
      totalMs: BUDGETS.attemptCeilingMs,
      deadlineAt: Date.now() + BUDGETS.attemptCeilingMs,
      consume: (cap = Infinity) => Math.min(BUDGETS.attemptCeilingMs, cap),
      remainingMs: () => 0,
      elapsedMs: () => BUDGETS.attemptCeilingMs,
      isExceeded: () => false,
    };
    actions.length = 0;
    const result = await runSplitCancelScenario({
      scenario: 'split-cancel',
      evidence: { action: action => actions.push(action) },
      barrierHub: fakeHub,
      pid: 1,
      platformPreflight: 'darwin',
      nativeDriver: driver,
      paneInventory: reader,
    }, { cancel: { phase: 'while-creating' } }, spentForMeasurement);
    expect(result.cancelReceipt.cancelAckMs).toBe(7);
    const after = actions.find(action => action.action === SPLIT_INVENTORY_ACTION);
    expect([after.ok, after.code, after.cause, after.charge.boundMs]).toEqual([false, INVENTORY_READ_REFUSED, ATTEMPT_BUDGET_SPENT, 0]);
    expect(reads).toEqual([]);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('the barrier-hub archive receipt says which moment it is, and the runner seam never throws', () => {
  const root = fixtureRoot();
  const hubDir = join(root, 'isolation', 'barriers');
  const runDir = join(root, 'evidence', 'run-archive');
  mkdirSync(hubDir, { recursive: true });
  mkdirSync(runDir, { recursive: true });
  writeFileSync(join(hubDir, 'split-create.receipt.jsonl'), '{"backendSessionId":"b1"}\n');
  // A child handle that has NOT exited (`exitCode`/`signalCode` still null, which
  // is how Node reports a live child) beside one that has.
  const runningApp = { exitCode: null, signalCode: null };
  const exitedApp = { exitCode: 0, signalCode: null };
  try {
    // (1) the copy is taken BEFORE the process reap and now says so: a stream that
    // stops short of a settlement the product appended afterwards is no longer
    // indistinguishable from one that never received it.
    const archive = archiveBarrierHub(hubDir, runDir, {
      processes: [{ pid: 111, label: 'app', child: runningApp }, { pid: 222, label: 'fixture', child: exitedApp }],
    });
    expect(archive.ok).toBe(true);
    expect(archive.snapshot).toMatchObject({
      phase: 'BEFORE_PROCESS_REAP',
      marker: BARRIER_ARCHIVE_SNAPSHOT_MARKER,
      takenBeforeProcessReap: true,
      truncatedScope: 'BYTE_CAP_ONLY',
    });
    expect(archive.snapshot.note).toContain('BEFORE the process reap');
    expect(archive.snapshot.note).toContain('truncated');
    expect(archive.snapshot.appProcesses).toMatchObject({ determined: true, stillRunning: [111], exited: [222], undetermined: [] });
    // `truncated` is the byte cap and nothing else - the capped case says so too,
    // instead of reading as "the product stopped printing".
    const capped = archiveBarrierHub(hubDir, join(root, 'evidence-capped'), { maxBytes: 0 });
    expect([capped.truncated, capped.snapshot.truncatedScope]).toEqual([true, 'BYTE_CAP_ONLY']);
    // (2) the same block travels in the copied bytes, so a reader who has the
    // archive and nothing else can still see the moment it was taken.
    const markerPath = join(runDir, 'barrier-hub', BARRIER_ARCHIVE_SNAPSHOT_FILE);
    expect(archive.snapshot.markerPath).toBe(markerPath);
    expect(JSON.parse(readFileSync(markerPath, 'utf8'))).toMatchObject({
      phase: 'BEFORE_PROCESS_REAP', takenBeforeProcessReap: true, truncated: false, filesCopied: ['split-create.receipt.jsonl'],
    });
    // A marker that cannot be written is reported, never fatal to the archive.
    const markerFails = archiveBarrierHub(hubDir, join(root, 'evidence-marker'), { writeMarker: () => { throw new Error('boom'); } });
    expect([markerFails.ok, markerFails.snapshot.markerPath, markerFails.snapshot.markerError]).toEqual([true, null, 'boom']);

    // (3) the runner's own finalization seam: the registry's processes travel with
    // the copy, and a throw while reading the seam's own arguments cannot escape
    // the `finally` that would otherwise destroy the run's result.json.
    const seam = archiveRunBarrierHub({
      barrierHub: { dir: hubDir },
      evidence: { runDir: join(root, 'evidence-seam') },
      registry: { processes: [{ pid: 111, label: 'app', child: runningApp }], reaped: [] },
    });
    expect(seam.ok).toBe(true);
    expect(seam.snapshot.appProcesses.stillRunning).toEqual([111]);
    const seamFails = archiveRunBarrierHub({ barrierHub: { get dir() { throw new Error('no dir'); } }, evidence: { runDir: join(root, 'evidence-seam') } });
    expect([seamFails.ok, seamFails.filesCopied, seamFails.reason.startsWith('ARCHIVE_SEAM_FAILED')]).toEqual([false, [], true]);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

// ---------------------------------------------------------------------------
// Pass-22 audit residuals: the ceiling must bound the SCENARIO's own time rather
// than the harness's instrumentation (F2-11), and no reader METHOD may escape into
// `classifyNativeFailure` (R1).

test('instrumentation time spent inside the measured window is excluded from the ceiling the scenario is judged against', async () => {
  const { createPaneInventoryReader } = await import('../lib/qa-scenarios/pane-binding.mjs');
  const root = fixtureRoot();
  const actions = [];
  const reads = [];
  const budget = new MonotonicBudget(BUDGETS.attemptCeilingMs);
  const instrumentation = new InstrumentationClock({ budget });
  // A read that really costs wall clock. A timer never fires early, so the lower
  // bounds asserted below cannot pass by luck.
  const reader = createPaneInventoryReader({
    runtimeDir: join(root, 'runtime'),
    platform: 'darwin',
    evidence: { action: action => actions.push(action) },
    readInventory: async () => {
      reads.push('read');
      await new Promise(resolve => setTimeout(resolve, 25));
      return {
        ok: true, code: null, detail: null, transport: 'unix-socket',
        endpoint: { transport: 'unix-socket', socketPath: join(root, 'runtime', 'daemon.sock') },
        sessions: ['fixture-1'], epoch: 1, elapsedMs: 25,
      };
    },
  });
  const driver = { focus: async () => {}, split: async () => {} };
  const fakeHub = {
    command: () => {},
    awaitReceipt: async () => ({ cancelAckMs: 13, cleanupReceipt: { authoritative: true } }),
  };
  try {
    const result = await runSplitCancelScenario({
      scenario: 'split-cancel',
      evidence: { action: action => actions.push(action) },
      barrierHub: fakeHub,
      pid: 1,
      platformPreflight: 'darwin',
      nativeDriver: driver,
      paneInventory: reader,
      instrumentation,
    }, { cancel: { phase: 'while-creating' } }, budget);
    // (1) the scenario settles, and both reads really ran and really cost wall clock.
    expect(result.cancelReceipt.cancelAckMs).toBe(13);
    expect(reads.length).toBe(2);
    expect(instrumentation.totalMs()).toBeGreaterThanOrEqual(40);
    // (2) the window the scenario is judged against did not lose that time: the
    // deadline moved forward by exactly what the measurements spent, so neither a
    // stage cap nor the ceiling is decided by instrumentation.
    expect(budget.deadlineAt - budget.startAt).toBeGreaterThanOrEqual(BUDGETS.attemptCeilingMs + 40);
    // (3) the accounting reports BOTH numbers, and the ceiling compares the adjusted
    // one, with the per-read attribution that makes the subtraction auditable.
    const accounting = attemptBudgetAccounting({ budget, instrumentation, triggerLabel: 'cancel-request' });
    expect(accounting.instrumentationMs).toBe(instrumentation.totalMs());
    expect(accounting.scenarioMs).toBe(Math.max(0, accounting.attemptMs - accounting.instrumentationMs));
    expect(accounting.ceilingMs).toBe(BUDGETS.attemptCeilingMs);
    expect(accounting.ceilingBasis).toBe(ATTEMPT_CEILING_BASIS);
    expect(accounting.exceeded).toBe(false);
    expect(accounting.samples.map(sample => sample.label)).toEqual([PRE_SPLIT_INVENTORY_ACTION, SPLIT_INVENTORY_ACTION]);
    // (4) the same arithmetic at the boundary, with the raw clock deliberately past
    // the ceiling and instrumentation over the top of it: the ceiling is the
    // scenario's own time, so this is NOT exceeded.
    const overrun = {
      totalMs: BUDGETS.attemptCeilingMs, startAt: 0, deadlineAt: BUDGETS.attemptCeilingMs,
      elapsedMs: () => BUDGETS.attemptCeilingMs + 1_500, remainingMs: () => 0,
      isExceeded: () => true, consume: () => 1,
    };
    const synthetic = { totalMs: () => 2_000, samples: [{ label: SPLIT_INVENTORY_ACTION, ms: 2_000 }] };
    expect(attemptBudgetAccounting({ budget: overrun, instrumentation: synthetic, triggerLabel: 'split-menu-click' }))
      .toMatchObject({
        attemptMs: BUDGETS.attemptCeilingMs + 1_500,
        instrumentationMs: 2_000,
        scenarioMs: BUDGETS.attemptCeilingMs - 500,
        ceilingMs: BUDGETS.attemptCeilingMs,
        exceeded: false,
      });
    // (5) and it still fails closed: the same raw clock with NO instrumentation
    // recorded is over the ceiling, so the exclusion cannot be used to excuse a
    // slow scenario.
    expect(attemptBudgetAccounting({ budget: overrun, instrumentation: new InstrumentationClock(), triggerLabel: 'split-menu-click' }).exceeded).toBe(true);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('a reader whose lastReading throws is recorded as a typed action and the split scenario still settles', async () => {
  const root = fixtureRoot();
  const actions = [];
  const budget = new MonotonicBudget();
  // The reader METHOD that used to be called unguarded (pass-22 audit R1): it is the
  // baseline accessor, not the read, and a reader whose implementation throws from it
  // must not be able to reach `classifyNativeFailure` either.
  const reader = {
    snapshot: async label => {
      actions.push({ action: label, ok: true, code: null, delta: null });
      return {
        ok: true, code: null, detail: null, transport: 'unix-socket', endpoint: null,
        sessions: ['fixture-1'], epoch: 1, elapsedMs: 1,
      };
    },
    lastReading: () => { throw new HarnessError('ASSERTION_FAILURE', 'lastReading exploded'); },
  };
  const driver = { focus: async () => {}, split: async () => {} };
  const fakeHub = {
    command: () => {},
    awaitReceipt: async () => ({ cancelAckMs: 11, cleanupReceipt: { authoritative: true } }),
  };
  try {
    const result = await runSplitCancelScenario({
      scenario: 'split-cancel',
      evidence: { action: action => actions.push(action) },
      barrierHub: fakeHub,
      pid: 1,
      platformPreflight: 'darwin',
      nativeDriver: driver,
      paneInventory: reader,
    }, { cancel: { phase: 'while-creating' } }, budget);
    // (1) the scenario's own settlement is untouched - the mirror of the throwing
    // `snapshot` case above.
    expect(result.cancelReceipt.cancelAckMs).toBe(11);
    // (2) the failure is typed on its own action, carrying the thrown error's code.
    const typed = actions.find(action => action.action === SPLIT_INVENTORY_LAST_READING_ACTION);
    expect([typed.ok, typed.code, typed.cause]).toEqual([false, SPLIT_INVENTORY_READ_FAILED, 'ASSERTION_FAILURE']);
    expect(typed.detail).toContain('lastReading()');
    expect([typed.sessionCount, typed.sessionIds, typed.delta]).toEqual([null, null, null]);
    // (3) the arm continued with no baseline, so both reads still happened - the
    // typed action first, then the pre-click baseline and the post-click read
    // (`cancel-request` is the scenario's own action and is not an inventory one).
    expect(actions.map(action => action.action).filter(name => name.startsWith('split-inventory') || name.startsWith('pane-inventory')))
      .toEqual([SPLIT_INVENTORY_LAST_READING_ACTION, PRE_SPLIT_INVENTORY_ACTION, SPLIT_INVENTORY_ACTION]);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
