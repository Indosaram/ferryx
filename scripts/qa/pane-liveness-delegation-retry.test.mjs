// Pass-18/19 (task-9 win-pass17 + the 12-probe burst measurement) unit tests:
// the delegation-stall detection/retry and the app-stdio sink's teardown
// ordering.
//
// These live in their OWN file but run under the ONE canonical gate config
// (scripts/qa/pane-liveness-vitest.config.mjs), which lists both this file and
// the frozen scripts/qa/pane-liveness.test.mjs. The frozen gate argv is
// unchanged - `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` -
// so the frozen command really executes this coverage instead of leaving it
// outside every gate. This file adds 20 (86 under the one frozen command: the
// frozen suite holds 66 after the pass-21 window-root-fallback tests).
//
// Authored only - execution is delegated to the sole remote verifier per plan.
// Nothing here launches a product, a scheduled task, a PowerShell probe, a
// window, or a GUI action; the retry loop is driven through the module's own
// injected attempt seam and the sink through real files in a temp fixture root.

import { test, expect } from '../../ui/node_modules/vitest/dist/index.js';
import { EventEmitter } from 'node:events';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { BUDGETS, EXIT, HarnessError, ResourceRegistry, waitForFile, withDeadline } from '../lib/qa-scenarios/common-harness.mjs';

const fixtureRoot = () => realpathSync(mkdtempSync(join(realpathSync(tmpdir()), 'delegation-retry-')));

// ---------------------------------------------------------------------------
// The delegated bat: the entry marker is its FIRST statement.

test('the delegated bat writes its entry marker as the very first statement it executes', async () => {
  const { buildInteractiveRelaunchPlan, DELEGATION_ENTRY_MARKER_FILE } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const batDir = 'C:\\ev\\delegation\\attempt-1-abc123';
  const runnerArgs = [
    '--scenario', 'split-happy',
    '--binary', 'C:\\repo\\src-tauri\\target\\debug\\ferryx.exe',
    '--evidence-dir', 'C:\\ev',
    '--isolation-root', 'C:\\iso',
  ];
  const plan = buildInteractiveRelaunchPlan({
    taskName: 'ferryx-qa-split-happy-1-abc123',
    batDir,
    nodePath: 'C:\\Program Files\\nodejs\\node.exe',
    runnerPath: 'C:\\repo\\scripts\\qa\\pane-liveness.mjs',
    runnerArgs,
    cwd: 'C:\\repo',
  });

  // The measured pass-17 stall is a process that never executes its OWN FIRST
  // LINE, so the marker has to BE that line: `@echo off` first (it only
  // suppresses echoing and runs nothing), then the marker echo, then everything
  // else.
  const lines = plan.batBody.split('\r\n');
  expect(lines[0]).toBe('@echo off');
  expect(lines[1]).toContain('echo DELEGATION_ENTRY');
  expect(lines[1]).toContain(`>> "${plan.entryMarkerPath}"`);
  expect(plan.entryMarkerPath).toBe(join(batDir, DELEGATION_ENTRY_MARKER_FILE));

  // ... and the marker really is before the env, the cd and the runner.
  const markerIndex = lines.findIndex(line => line.includes('DELEGATION_ENTRY'));
  const envIndex = lines.findIndex(line => line.startsWith('set FERRYX_QA_WINDOWS_INTERACTIVE_RELAUNCH='));
  const recordIndex = lines.findIndex(line => line.startsWith('set FERRYX_QA_WINDOWS_RELAUNCH_RECORD='));
  const nodeIndex = lines.findIndex(line => line.includes('scripts\\qa\\pane-liveness.mjs'));
  expect(markerIndex).toBe(1);
  expect(markerIndex).toBeLessThan(envIndex);
  expect(envIndex).toBeLessThan(nodeIndex);
  expect(recordIndex).toBeLessThan(nodeIndex);

  // The proven mechanism and the frozen argv are unchanged: `/it`, the same
  // task name, the same argument-for-argument command line, the same exit file.
  expect(plan.createArgs).toEqual(['/create', '/tn', 'ferryx-qa-split-happy-1-abc123', '/tr', plan.batPath, '/sc', 'once', '/st', '00:00', '/f', '/it']);
  expect(plan.runArgs).toEqual(['/run', '/tn', 'ferryx-qa-split-happy-1-abc123']);
  expect(plan.endArgs).toEqual(['/end', '/tn', 'ferryx-qa-split-happy-1-abc123']);
  expect(plan.deleteArgs).toEqual(['/delete', '/tn', 'ferryx-qa-split-happy-1-abc123', '/f']);
  expect(plan.command).toBe(
    '"C:\\Program Files\\nodejs\\node.exe" "C:\\repo\\scripts\\qa\\pane-liveness.mjs" "--scenario" "split-happy" "--binary" "C:\\repo\\src-tauri\\target\\debug\\ferryx.exe" "--evidence-dir" "C:\\ev" "--isolation-root" "C:\\iso"',
  );
  expect(plan.batBody).toContain('cd /d "C:\\repo"');
  expect(plan.batBody).toContain(`echo %ERRORLEVEL% > "${plan.exitPath}"`);
  // The stalled attempt's own task state is queried for the ledger.
  expect(plan.queryArgs).toEqual(['/query', '/tn', 'ferryx-qa-split-happy-1-abc123', '/fo', 'list', '/v']);
});

// ---------------------------------------------------------------------------
// Fresh attempt identity + the anti-stale precondition.

test('every attempt is a fresh delegation: fresh task name, fresh directory, fresh marker path', async () => {
  const { delegationAttemptIdentity } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const base = { scenario: 'split-happy', pid: 4242, token: 'tk1' };
  const first = delegationAttemptIdentity({ ...base, index: 1 });
  const second = delegationAttemptIdentity({ ...base, index: 2 });
  const otherRun = delegationAttemptIdentity({ scenario: 'split-happy', pid: 4242, index: 1, token: 'tk2' });
  expect(first.taskName).toBe('ferryx-qa-split-happy-4242-a1-tk1');
  expect(first.attemptDirName).toBe('attempt-1-tk1');
  // No attempt can reuse another attempt's task name or directory - and so not
  // its bat path, exit file or marker path either.
  expect(new Set([first.taskName, second.taskName, otherRun.taskName]).size).toBe(3);
  expect(new Set([first.attemptDirName, second.attemptDirName, otherRun.attemptDirName]).size).toBe(3);
});

test('a marker path that already exists is refused, never read as this attempt\'s success', async () => {
  const { delegationMarkerPrecondition } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const markerPath = 'C:\\ev\\delegation\\attempt-1-tk1\\delegation-entry.marker';
  expect(delegationMarkerPrecondition(markerPath, () => false)).toEqual({ ok: true, reason: null });
  const stale = delegationMarkerPrecondition(markerPath, () => true);
  expect(stale.ok).toBe(false);
  expect(stale.reason).toContain('already existed before this attempt started');
  expect(stale.reason).toContain('refusing to read a stale marker');
});

// ---------------------------------------------------------------------------
// The stalled attempt's own cmd.exe: identity-checked, exact PID, never a pattern.

test('only a cmd.exe carrying this attempt\'s exact bat path with the service as parent may be killed', async () => {
  const { classifyDelegationAttemptProcesses, buildDelegationProcessProbeScript } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const batPath = 'C:\\ev\\delegation\\attempt-2-tk1\\ferryx-qa-split-happy-1-a2-tk1.bat';
  const payload = {
    probe: 'delegation-attempt-process',
    batPath,
    matches: [
      // The pass-17 signature: the stalled cmd.exe, parented by the Schedule service.
      { pid: 26416, parentPid: 1264, parentName: 'svchost.exe', creationDate: '2026-10-05T05:39:39.648', commandLine: `cmd.exe /c ""${batPath}""` },
      // Same path, different parent: REFUSED, reported, never killed.
      { pid: 999, parentPid: 42, parentName: 'explorer.exe', commandLine: `cmd.exe /c "${batPath}"` },
      // The Schedule service as parent, but an EARLIER attempt's bat: refused.
      { pid: 1000, parentPid: 1264, parentName: 'svchost.exe', commandLine: 'cmd.exe /c "C:\\ev\\delegation\\attempt-1-tk1\\ferryx-qa-split-happy-1-a1-tk1.bat"' },
    ],
  };
  const classified = classifyDelegationAttemptProcesses(payload, { batPath });
  expect(classified.owned.map(entry => entry.pid)).toEqual([26416]);
  expect(classified.owned[0]).toMatchObject({ parentPid: 1264, parentName: 'svchost.exe' });
  expect(classified.refused.map(entry => entry.pid)).toEqual([999, 1000]);
  expect(classified.refused[0].reason).toContain('not the Schedule service host');
  expect(classified.refused[1].reason).toContain("exact bat path");

  // The path match is case-insensitive (the service may echo the action path in
  // another case) but the parent check is not relaxed.
  const cased = classifyDelegationAttemptProcesses({
    matches: [{ pid: 7, parentPid: 1264, parentName: 'SVCHOST.EXE', commandLine: `CMD.EXE /C "${batPath.toUpperCase()}"` }],
  }, { batPath });
  expect(cased.owned.map(entry => entry.pid)).toEqual([7]);

  // A single-element array is normalized at this boundary, and an empty or
  // shapeless probe owns nothing.
  expect(classifyDelegationAttemptProcesses({ matches: { pid: 8, parentPid: 1264, parentName: 'svchost.exe', commandLine: batPath } }, { batPath }).owned.map(entry => entry.pid)).toEqual([8]);
  expect(classifyDelegationAttemptProcesses({}, { batPath })).toEqual({ owned: [], refused: [] });
  expect(classifyDelegationAttemptProcesses({ matches: [] }, { batPath })).toEqual({ owned: [], refused: [] });
  // Without the attempt's own path there is nothing this run may signal.
  expect(classifyDelegationAttemptProcesses({ matches: [{ pid: 9, parentPid: 1264, parentName: 'svchost.exe', commandLine: 'cmd.exe /c other.bat' }] }, { batPath }).owned).toEqual([]);

  // The probe script carries the exact path, is newline-joined (one statement
  // per line) and contains no here-string.
  const script = buildDelegationProcessProbeScript(batPath);
  expect(script).toContain(`$bat = '${batPath}';`);
  expect(script).toContain("probe = 'delegation-attempt-process'");
  expect(script).not.toContain('@"');
  expect(script.split('\n').some(line => line.startsWith('Write-Output ('))).toBe(true);
  // Single quotes inside a path are escaped for PowerShell, not swallowed.
  expect(buildDelegationProcessProbeScript("C:\\a'b\\t.bat")).toContain("$bat = 'C:\\a''b\\t.bat';");
});

// ---------------------------------------------------------------------------
// The bounded retry: a stalled attempt is retried, a deterministic failure is not.

// A synthetic attempt result in the exact shape runDelegationAttempt returns, so
// the retry loop can be replayed without a Windows host, schtasks or a GUI.
function syntheticAttempt(index, outcome, extra = {}) {
  const stalled = outcome === 'STALLED';
  return {
    outcome,
    stalled,
    code: extra.code ?? null,
    exitCode: extra.exitCode ?? null,
    detail: `${outcome} on attempt ${index}`,
    innerStdout: extra.innerStdout ?? null,
    innerStderr: null,
    report: { attemptIndex: index, outPath: `C:\\ev\\delegation\\attempt-${index}\\relaunch.out` },
    ledger: {
      index,
      taskName: `ferryx-qa-split-happy-1-a${index}-tk`,
      entryMarkerPath: `C:\\ev\\delegation\\attempt-${index}-tk\\delegation-entry.marker`,
      markerAppeared: !stalled,
      markerWaitMs: stalled ? BUDGETS.interactiveRelaunchEntryMarkerMs : 40,
      // Mirrors the real ledger's budget fields so the sequence-level assertions
      // below are meaningful rather than vacuous.
      markerWindowMs: BUDGETS.interactiveRelaunchEntryMarkerMs,
      exitBudgetMs: BUDGETS.interactiveRelaunchSequenceMs,
      outcome,
      exitCode: extra.exitCode ?? null,
      teardown: stalled
        ? { endCode: 0, deleteCode: 0, kill: { attempted: true, matched: [{ pid: 26416 }], refused: [], killed: [{ pid: 26416, parentPid: 1264, code: 0, output: 'SUCCESS' }], reason: null } }
        : null,
    },
  };
}

const delegationContext = evidenceDir => ({
  scenario: 'split-happy',
  evidenceDir,
  argv: ['C:\\Program Files\\nodejs\\node.exe', 'C:\\repo\\scripts\\qa\\pane-liveness.mjs'],
});

test('a stalled attempt is retried with a fresh delegation, and the run reports how many attempts it needed', async () => {
  const { relaunchIntoInteractiveSession, WINDOWS_DELEGATION_LEDGER_FILE } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const root = fixtureRoot();
  const evidenceDir = join(root, 'ev');
  mkdirSync(evidenceDir, { recursive: true });
  const seen = [];
  const result = await relaunchIntoInteractiveSession({
    context: delegationContext(evidenceDir),
    rawArgv: ['--scenario', 'split-happy'],
    probe: null,
    cwd: 'C:\\repo',
    deps: {
      token: 'tk',
      runAttempt: async ({ index, token, delegationDir }) => {
        seen.push({ index, token, delegationDir });
        return index < 3
          ? syntheticAttempt(index, 'STALLED')
          : syntheticAttempt(index, 'COMPLETED', { exitCode: 1, innerStdout: 'inner-run-stdout' });
      },
    },
  });

  // Two stalled attempts, then one that ran its first line: 3 attempts were needed.
  expect(seen.map(entry => entry.index)).toEqual([1, 2, 3]);
  expect(seen.every(entry => entry.token === 'tk')).toBe(true);
  expect(seen.every(entry => entry.delegationDir === join(evidenceDir, 'delegation'))).toBe(true);
  expect(result.exitCode).toBe(1);
  expect(result.code).toBeNull();
  expect(result.stalled).toBe(false);
  expect(result.innerStdout).toBe('inner-run-stdout');
  expect(result.report.delegatedExitCode).toBe(1);
  expect(result.report.delegation.attemptsUsed).toBe(3);
  expect(result.report.delegation.stopReason).toBe('delegated-run-completed');
  expect(result.report.delegation.attempts.map(attempt => [attempt.index, attempt.outcome, attempt.markerAppeared]))
    .toEqual([[1, 'STALLED', false], [2, 'STALLED', false], [3, 'COMPLETED', true]]);
  expect(result.report.delegation.attempts[0].kill.killed).toEqual([26416]);

  // Every attempt is in the ledger on disk, in the run's evidence dir.
  const ledgerPath = join(evidenceDir, WINDOWS_DELEGATION_LEDGER_FILE);
  expect(existsSync(ledgerPath)).toBe(true);
  const ledger = JSON.parse(readFileSync(ledgerPath, 'utf8'));
  expect(ledger.ledgerPath).toBe(ledgerPath);
  expect(ledger.maxAttempts).toBe(BUDGETS.interactiveRelaunchAttempts);
  expect(ledger.entryMarkerBudgetMs).toBe(BUDGETS.interactiveRelaunchEntryMarkerMs);
  expect(ledger.attemptsUsed).toBe(3);
  expect(ledger.stalled).toBe(false);
  expect(ledger.code).toBeNull();
  expect(ledger.stopReason).toBe('delegated-run-completed');
  expect(ledger.sequenceBudgetMs).toBe(BUDGETS.interactiveRelaunchSequenceMs);
  expect(ledger.attempts.map(attempt => [attempt.index, attempt.taskName, attempt.markerAppeared, attempt.markerWaitMs, attempt.outcome, attempt.exitCode]))
    .toEqual([
      [1, 'ferryx-qa-split-happy-1-a1-tk', false, BUDGETS.interactiveRelaunchEntryMarkerMs, 'STALLED', null],
      [2, 'ferryx-qa-split-happy-1-a2-tk', false, BUDGETS.interactiveRelaunchEntryMarkerMs, 'STALLED', null],
      [3, 'ferryx-qa-split-happy-1-a3-tk', true, 40, 'COMPLETED', 1],
    ]);
  expect(new Set(ledger.attempts.map(attempt => attempt.taskName)).size).toBe(3);
  expect(new Set(ledger.attempts.map(attempt => attempt.entryMarkerPath)).size).toBe(3);
  rmSync(root, { recursive: true, force: true });
});

test('every attempt stalling fails closed with DELEGATION_STALLED - never a pass, never a session-0 fallback', async () => {
  const { relaunchIntoInteractiveSession, admitWindowsInteractiveDesktop, WINDOWS_DELEGATION_LEDGER_FILE } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const root = fixtureRoot();
  const evidenceDir = join(root, 'ev');
  mkdirSync(evidenceDir, { recursive: true });
  const result = await relaunchIntoInteractiveSession({
    context: delegationContext(evidenceDir),
    rawArgv: ['--scenario', 'split-happy'],
    probe: null,
    cwd: 'C:\\repo',
    deps: { token: 'tk', runAttempt: async ({ index }) => syntheticAttempt(index, 'STALLED') },
  });

  expect(result.exitCode).toBeNull();
  expect(result.code).toBe('DELEGATION_STALLED');
  expect(result.stalled).toBe(true);
  expect(result.detail).toContain(`${BUDGETS.interactiveRelaunchAttempts} delegation attempt(s) stalled`);
  expect(result.detail).toContain(`the ${BUDGETS.interactiveRelaunchSequenceMs}ms retry sequence ended by attempts-exhausted`);
  expect(result.detail).toContain('refusing to fall back to a session-0 launch');
  expect(result.report.delegation.attemptsUsed).toBe(BUDGETS.interactiveRelaunchAttempts);
  expect(result.report.delegation.stalled).toBe(true);
  expect(result.report.delegation.code).toBe('DELEGATION_STALLED');
  expect(result.report.delegation.stopReason).toBe('attempts-exhausted');
  expect(result.report.delegation.sequenceBudgetMs).toBe(BUDGETS.interactiveRelaunchSequenceMs);
  // The ledger states the interpreter that produced it (pass-19 discipline).
  expect(result.report.delegation.interpreter.node).toBe(process.version);

  const ledger = JSON.parse(readFileSync(join(evidenceDir, WINDOWS_DELEGATION_LEDGER_FILE), 'utf8'));
  expect(ledger.attempts).toHaveLength(BUDGETS.interactiveRelaunchAttempts);
  expect(ledger.attempts.every(attempt => attempt.markerAppeared === false)).toBe(true);
  expect(ledger.attempts.every(attempt => attempt.outcome === 'STALLED')).toBe(true);
  expect(ledger.attempts.every(attempt => attempt.markerWindowMs === BUDGETS.interactiveRelaunchEntryMarkerMs)).toBe(true);
  expect(ledger.attemptsUsed).toBe(BUDGETS.interactiveRelaunchAttempts);
  expect(ledger.stalled).toBe(true);
  expect(ledger.code).toBe('DELEGATION_STALLED');
  expect(ledger.stopReason).toBe('attempts-exhausted');
  expect(ledger.interpreter.node).toBe(process.version);
  expect(new Set(ledger.attempts.map(attempt => attempt.taskName)).size).toBe(BUDGETS.interactiveRelaunchAttempts);

  // The typed code is nonzero, keeps its identity, and is a BLOCKED condition.
  const runner = await import('./pane-liveness.mjs');
  expect(runner.classifyNativeFailure('DELEGATION_STALLED')).toEqual({ verdict: 'BLOCKED', exitCode: EXIT.nativeAutomationUnsupported });
  expect(runner.classifyNativeFailure('DELEGATION_STALLED').exitCode).not.toBe(0);
  expect(new HarnessError('DELEGATION_STALLED', 'detail').code).toBe('DELEGATION_STALLED');

  // Admission maps it to a typed block (never a delegated pass, never a
  // session-0 launch) and carries the attempt count. The admission path is
  // env-dependent by design (a delegated run refuses to relaunch again), so pin
  // that env OFF for this replay and restore it, making the result independent
  // of where the gate happens to run.
  const { WINDOWS_INTERACTIVE_RELAUNCH_ENV } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const savedRelaunchEnv = process.env[WINDOWS_INTERACTIVE_RELAUNCH_ENV];
  delete process.env[WINDOWS_INTERACTIVE_RELAUNCH_ENV];
  let admitted;
  try {
    admitted = await admitWindowsInteractiveDesktop({
      invocation: { headless: false },
      context: { ...delegationContext(evidenceDir), platform: 'win32' },
      rawArgv: [],
      deps: {
        probeSession: async () => ({
          ok: true,
          probe: { probe: 'windows-session', interactive: false, sessionId: 0, activeConsoleSessionId: 1, explorerSessions: [1] },
        }),
        relaunch: async () => result,
      },
    });
  } finally {
    if (savedRelaunchEnv === undefined) delete process.env[WINDOWS_INTERACTIVE_RELAUNCH_ENV];
    else process.env[WINDOWS_INTERACTIVE_RELAUNCH_ENV] = savedRelaunchEnv;
  }
  expect(admitted.mode).toBe('blocked');
  expect(admitted.code).toBe('DELEGATION_STALLED');
  expect(admitted.detail).toContain('session-0');
  expect(admitted.delegation.attemptsUsed).toBe(BUDGETS.interactiveRelaunchAttempts);
  expect(admitted.evidence.delegation.attemptsUsed).toBe(BUDGETS.interactiveRelaunchAttempts);
  rmSync(root, { recursive: true, force: true });
});

test('a deterministic delegation failure is not retried and keeps INTERACTIVE_RELAUNCH_FAILED', async () => {
  const { relaunchIntoInteractiveSession } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const root = fixtureRoot();
  const evidenceDir = join(root, 'ev');
  mkdirSync(evidenceDir, { recursive: true });
  const seen = [];
  const result = await relaunchIntoInteractiveSession({
    context: delegationContext(evidenceDir),
    rawArgv: [],
    probe: null,
    cwd: 'C:\\repo',
    deps: {
      token: 'tk',
      runAttempt: async ({ index }) => {
        seen.push(index);
        return syntheticAttempt(index, 'CREATE_FAILED', { code: 'INTERACTIVE_RELAUNCH_FAILED' });
      },
    },
  });
  // Exactly ONE attempt: a retry loop must not be able to hide a deterministic
  // failure behind a second identical refusal.
  expect(seen).toEqual([1]);
  expect(result.exitCode).toBeNull();
  expect(result.code).toBe('INTERACTIVE_RELAUNCH_FAILED');
  expect(result.code).not.toBe('DELEGATION_STALLED');
  expect(result.stalled).toBe(false);
  expect(result.report.delegation.attemptsUsed).toBe(1);
  expect(result.report.delegation.stopReason).toBe('deterministic-failure');
  expect(result.report.delegation.attempts.map(attempt => attempt.outcome)).toEqual(['CREATE_FAILED']);
  rmSync(root, { recursive: true, force: true });
});

test('the entry-marker window, the attempt cap and the sequence budget stay bounded', () => {
  // Sized from the measurements: success latency 0.380s (pass-17 r1) and
  // 0.502-0.516s (the 12 probes), so the 10s window is ~20x and never below 5s.
  expect(BUDGETS.interactiveRelaunchEntryMarkerMs).toBe(10_000);
  expect(BUDGETS.interactiveRelaunchEntryMarkerMs).toBeGreaterThanOrEqual(5_000);
  expect(BUDGETS.interactiveRelaunchEntryMarkerMs / 516).toBeGreaterThan(15);
  // The sequence is sized to OUTLAST A BURST, not to beat a stall rate. The
  // measured burst distribution on the gate host is 1 attempt (pass-17 r1), 15
  // attempts back-to-back for ~153s (pass-19) and 26+ attempts for ~260s and
  // still stalling when its sequence ran out - so `cfb4374b` ("raise the
  // delegation retry sequence to outlast the measured 153s burst") raised the
  // sequence to 15 minutes (10s x 90). These are the CURRENT deliberate values,
  // not a re-derived ideal: the budget is not changed here, only asserted.
  expect(BUDGETS.interactiveRelaunchAttempts).toBe(90);
  expect(BUDGETS.interactiveRelaunchSequenceMs).toBe(900_000);
  expect(BUDGETS.interactiveRelaunchAttempts * BUDGETS.interactiveRelaunchEntryMarkerMs)
    .toBe(BUDGETS.interactiveRelaunchSequenceMs);
  // The budget must stay ABOVE the longest MEASURED burst: 26 attempts at the 10s
  // window is the ~260s lower bound that was still stalling when its sequence ran
  // out, so lowering either lever back under it is the regression this catches.
  const MEASURED_BURST_ATTEMPTS = 26;
  expect(BUDGETS.interactiveRelaunchAttempts).toBeGreaterThan(MEASURED_BURST_ATTEMPTS);
  expect(BUDGETS.interactiveRelaunchSequenceMs)
    .toBeGreaterThan(MEASURED_BURST_ATTEMPTS * BUDGETS.interactiveRelaunchEntryMarkerMs);
  // The old single-attempt budget name is gone: one attempt must never be able
  // to spend the whole sequence on its own.
  expect(BUDGETS.interactiveRelaunchTimeoutMs).toBeUndefined();
});

// ---------------------------------------------------------------------------
// Pass-19: the verifier's own precondition discipline applied to this pass's
// generated text - assert the anchor count is exactly 1 and read the patched
// text back, rather than trusting that "the file changed".

const countOccurrences = (text, needle) => text.split(needle).length - 1;

test('the bat text has exactly one entry-marker statement, and it is the first one that runs', async () => {
  const { buildInteractiveRelaunchPlan, buildDelegationProcessProbeScript, DELEGATION_ENTRY_MARKER_FILE } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const batDir = 'C:\\ev\\delegation\\attempt-1-tk';
  const plan = buildInteractiveRelaunchPlan({
    taskName: 'ferryx-qa-split-happy-1-a1-tk',
    batDir,
    nodePath: 'C:\\Program Files\\nodejs\\node.exe',
    runnerPath: 'C:\\repo\\scripts\\qa\\pane-liveness.mjs',
    runnerArgs: ['--scenario', 'split-happy'],
    cwd: 'C:\\repo',
  });
  // Read back the generated text: exactly ONE statement writes the marker (an
  // anchor that silently missed would show as 0 here, and a duplicated injection
  // as 2), and it is line index 1 - the first line that executes anything.
  expect(countOccurrences(plan.batBody, 'echo DELEGATION_ENTRY')).toBe(1);
  expect(countOccurrences(plan.batBody, `>> "${plan.entryMarkerPath}"`)).toBe(1);
  expect(countOccurrences(plan.batBody, plan.entryMarkerPath)).toBe(1);
  const lines = plan.batBody.split('\r\n');
  expect(lines[0]).toBe('@echo off');
  expect(lines[1].startsWith('echo DELEGATION_ENTRY')).toBe(true);
  expect(lines[1]).toContain('task=ferryx-qa-split-happy-1-a1-tk');
  expect(lines[1]).toContain('%CD%');
  expect(plan.entryMarkerPath).toBe(join(batDir, DELEGATION_ENTRY_MARKER_FILE));
  // The marker is written before the runner line, and the runner line is the
  // only one that starts the delegated process.
  expect(lines.findIndex(line => line.includes('pane-liveness.mjs'))).toBeGreaterThan(1);
  expect(countOccurrences(plan.batBody, 'pane-liveness.mjs')).toBe(1);
  // Every generated PowerShell builder here is line-structured (one statement
  // per line, no here-string) and states the interpreter that will run it.
  const probe = buildDelegationProcessProbeScript('C:\\ev\\d\\a.bat');
  expect(countOccurrences(probe, '$bat = ')).toBe(1);
  expect(probe).toContain('powershell = [string]$PSVersionTable.PSVersion');
  expect(probe).not.toContain('@"');
  expect(probe.split('\n').includes('')).toBe(false);
});

// The fail-closed branches, driven through the injected `deps` seam with the
// REAL attempt runner (no Windows host, no schtasks, no scheduled task).

test('an attempt whose marker path already exists is refused before any tool runs, and is not a stall', async () => {
  const { runDelegationAttempt } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const root = fixtureRoot();
  const delegationDir = join(root, 'ev', 'delegation');
  const calls = [];
  const result = await runDelegationAttempt({
    index: 1,
    token: 'tk',
    context: delegationContext(join(root, 'ev')),
    rawArgv: [],
    probe: null,
    cwd: 'C:\\repo',
    delegationDir,
    budgetMs: 60_000,
    entryMarkerMs: 40,
    deps: {
      // A stale marker from a previous attempt sits at this attempt's path.
      exists: path => String(path).endsWith('delegation-entry.marker'),
      runTool: async (file, args) => { calls.push([file, ...args]); return { code: 0, stdout: '', stderr: '' }; },
    },
  });
  // Refused, typed, and NOT a stall: a stale marker can never be read as this
  // attempt's success, and must never be retried as if the host had stalled.
  expect(result.outcome).toBe('STALE_MARKER_REFUSED');
  expect(result.stalled).toBe(false);
  expect(result.code).toBe('INTERACTIVE_RELAUNCH_FAILED');
  expect(result.exitCode).toBeNull();
  expect(result.ledger.markerPreexisting).toBe(true);
  expect(result.detail).toContain('refusing to read a stale marker');
  // No scheduled task was created, run, queried or killed.
  expect(calls).toEqual([]);
  rmSync(root, { recursive: true, force: true });
});

test('a stalled attempt ends its own task and kills only the identity-checked cmd.exe', async () => {
  const { runDelegationAttempt } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const root = fixtureRoot();
  const evidenceDir = join(root, 'ev');
  const delegationDir = join(evidenceDir, 'delegation');
  const batPath = join(delegationDir, 'attempt-1-tk', `ferryx-qa-split-happy-${process.pid}-a1-tk.bat`);
  const calls = [];
  const result = await runDelegationAttempt({
    index: 1,
    token: 'tk',
    context: delegationContext(evidenceDir),
    rawArgv: [],
    probe: null,
    cwd: 'C:\\repo',
    delegationDir,
    budgetMs: 60_000,
    // A short window so the test is fast; production uses the 10s budget.
    entryMarkerMs: 40,
    deps: {
      exists: () => false,
      runTool: async (file, args) => { calls.push([file, ...args]); return { code: 0, stdout: '', stderr: '' }; },
      probeAttemptProcesses: async () => ({
        ok: true,
        probe: {
          probe: 'delegation-attempt-process',
          powershell: '5.1.22621.1',
          matches: [
            { pid: 26416, parentPid: 1264, parentName: 'svchost.exe', creationDate: '2026-10-05T05:39:39.648', commandLine: `cmd.exe /c ""${batPath}""` },
            // Carries this attempt's path but a foreign parent: refused, never killed.
            { pid: 999, parentPid: 42, parentName: 'explorer.exe', commandLine: `cmd.exe /c "${batPath}"` },
          ],
        },
      }),
    },
  });
  expect(result.outcome).toBe('STALLED');
  expect(result.stalled).toBe(true);
  expect(result.code).toBeNull();
  expect(result.ledger.markerAppeared).toBe(false);
  expect(result.ledger.markerWindowMs).toBe(40);
  // The teardown ORDER after the attempt is declared stalled: the stalled task's
  // own state is queried, then the task is ended and deleted, then exactly one
  // exact-PID kill - nothing else, and no pattern-based kill.
  const taskName = `ferryx-qa-split-happy-${process.pid}-a1-tk`;
  const [createCall, runCall, ...afterStall] = calls;
  expect(createCall.slice(0, 2)).toEqual(['schtasks', '/create']);
  expect(createCall).toContain('/it');
  expect(runCall).toEqual(['schtasks', '/run', '/tn', taskName]);
  expect(afterStall).toEqual([
    ['schtasks', '/query', '/tn', taskName, '/fo', 'list', '/v'],
    ['schtasks', '/end', '/tn', taskName],
    ['schtasks', '/delete', '/tn', taskName, '/f'],
    ['taskkill', '/PID', '26416', '/F'],
  ]);
  expect(result.ledger.teardown.kill.killed).toEqual([{ pid: 26416, parentPid: 1264, code: 0, output: '' }]);
  expect(result.ledger.teardown.kill.refused.map(entry => entry.pid)).toEqual([999]);
  expect(result.ledger.taskQuery.code).toBe(0);
  expect(result.detail).toContain('stalled before the bat\'s first line');
  rmSync(root, { recursive: true, force: true });
});

test('a stall with no matching process kills nothing and says so', async () => {
  const { runDelegationAttempt } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const root = fixtureRoot();
  const evidenceDir = join(root, 'ev');
  const calls = [];
  const result = await runDelegationAttempt({
    index: 2,
    token: 'tk',
    context: delegationContext(evidenceDir),
    rawArgv: [],
    probe: null,
    cwd: 'C:\\repo',
    delegationDir: join(evidenceDir, 'delegation'),
    budgetMs: 60_000,
    entryMarkerMs: 40,
    deps: {
      exists: () => false,
      runTool: async (file, args) => { calls.push([file, ...args]); return { code: 0, stdout: '', stderr: '' }; },
      probeAttemptProcesses: async () => ({ ok: true, probe: { matches: [] } }),
    },
  });
  expect(result.outcome).toBe('STALLED');
  expect(calls.some(call => call[0] === 'taskkill')).toBe(false);
  expect(result.ledger.teardown.kill.killed).toEqual([]);
  expect(result.ledger.teardown.kill.reason).toContain('nothing was killed');
  rmSync(root, { recursive: true, force: true });
});

// ---------------------------------------------------------------------------
// The app-stdio sink's teardown ordering.

test('the sink flushes, then marks completeness before the stream ends', async () => {
  const { AppStdioSink, appStdioResult, APP_STDIO_CLOSE_MARKER, APP_STDIO_TRUNCATION_MARKER } = await import('../lib/qa-scenarios/common-harness.mjs');
  const root = fixtureRoot();
  const runDir = join(root, 'run-dir');
  const sink = new AppStdioSink(runDir, { label: 'app', maxBytes: 64 });
  const child = { stdout: new EventEmitter(), stderr: new EventEmitter() };
  sink.attach(child);
  child.stdout.emit('data', Buffer.alloc(100, 0x61));
  child.stderr.emit('data', Buffer.from('under the cap'));
  const receipt = await sink.close();

  // close() resolves on the stream's own 'finish', so every byte - including the
  // markers - is readable the moment it returns: nothing is left in a buffer
  // when the run dir is summarised.
  const text = readFileSync(join(runDir, 'app.stdout.log'), 'utf8');
  expect(text.startsWith('a'.repeat(64))).toBe(true);
  // Truncation marker, then the dropped-byte count, then the completeness
  // marker, and only then the end of the file.
  const truncatedAt = text.indexOf(APP_STDIO_TRUNCATION_MARKER);
  const droppedAt = text.indexOf('dropped 36 bytes after the 64-byte cap');
  const closedAt = text.indexOf(APP_STDIO_CLOSE_MARKER);
  expect(truncatedAt).toBeGreaterThan(-1);
  expect(droppedAt).toBeGreaterThan(truncatedAt);
  expect(closedAt).toBeGreaterThan(droppedAt);
  expect(text.trimEnd().endsWith('this file is complete]')).toBe(true);
  const stderrText = readFileSync(join(runDir, 'app.stderr.log'), 'utf8');
  expect(stderrText).toContain(`${APP_STDIO_CLOSE_MARKER} stderr drained 13 bytes, not truncated; this file is complete]`);

  expect([receipt.closed, receipt.closedOk]).toEqual([true, true]);
  expect(receipt.streams.map(entry => [entry.name, entry.truncated, entry.droppedBytes])).toEqual([['stdout', true, 36], ['stderr', false, 0]]);
  // The projection result.json carries agrees with the files.
  expect(appStdioResult(sink)).toMatchObject({ closed: true, closedOk: true, truncated: true, droppedBytes: 36 });
  rmSync(root, { recursive: true, force: true });
});

test('a sink whose close never ran cannot look complete', async () => {
  const { AppStdioSink, appStdioResult, APP_STDIO_CLOSE_MARKER } = await import('../lib/qa-scenarios/common-harness.mjs');
  const root = fixtureRoot();
  const runDir = join(root, 'killed-run');
  const sink = new AppStdioSink(runDir, { label: 'app' });
  const child = { stdout: new EventEmitter(), stderr: new EventEmitter() };
  sink.attach(child);
  child.stdout.emit('data', Buffer.from('partial output, the run was killed mid-flight'));
  // The run is killed here: close() never runs.
  const stdoutPath = join(runDir, 'app.stdout.log');
  const partial = existsSync(stdoutPath) ? readFileSync(stdoutPath, 'utf8') : '';
  // The completeness marker is written by close() alone, so it cannot be present
  // no matter how much of the buffered output has reached the disk yet.
  expect(partial).not.toContain(APP_STDIO_CLOSE_MARKER);
  // ... and nothing else claims the drain finished either.
  expect(sink.receipt().closed).toBe(false);
  expect(sink.receipt().closedOk).toBeNull();
  expect(appStdioResult(sink).closed).toBe(false);
  expect(appStdioResult(sink).closedOk).toBeNull();
  // The close marker really is close()'s own doing: closing now writes it.
  await sink.close();
  expect(readFileSync(stdoutPath, 'utf8')).toContain(APP_STDIO_CLOSE_MARKER);
  expect(sink.receipt().closed).toBe(true);
  rmSync(root, { recursive: true, force: true });
});

test('the registry closes the sink before it removes the run roots', async () => {
  const { AppStdioSink, appStdioResult } = await import('../lib/qa-scenarios/common-harness.mjs');
  const root = fixtureRoot();
  const runDir = join(root, 'run-dir');
  const isoRoot = join(root, 'iso-root');
  mkdirSync(isoRoot);
  const registry = new ResourceRegistry();
  const sink = new AppStdioSink(runDir, { label: 'app' });
  registry.registerLog(sink, 'app-stdio');
  registry.registerDirectory(isoRoot);
  const receipts = await registry.cleanup();
  const logIndex = receipts.findIndex(entry => entry.kind === 'log');
  const directoryIndex = receipts.findIndex(entry => entry.kind === 'directory');
  // Closed (flushed) BEFORE the roots are removed - and the runner only writes
  // result.json / cleanup.json after cleanup() has returned.
  expect(logIndex).toBeGreaterThan(-1);
  expect(directoryIndex).toBeGreaterThan(logIndex);
  expect(receipts[logIndex]).toMatchObject({ kind: 'log', closed: true, closedOk: true });
  // The sink's own artifact label is what its receipt carries; the registry's
  // resource label is recorded in cleanup.json's `registered.logs`. Pinning
  // either here would couple this suite to a detail the pass-19 change does not
  // touch, so the receipt is asserted on what it must report instead.
  expect(receipts[logIndex].streams.map(entry => entry.path))
    .toEqual([join(runDir, 'app.stdout.log'), join(runDir, 'app.stderr.log')]);
  expect(receipts[directoryIndex].removed).toBe(true);
  expect(existsSync(isoRoot)).toBe(false);
  expect(appStdioResult(sink)).toMatchObject({ closed: true, closedOk: true });
  expect(readFileSync(join(runDir, 'app.stdout.log'), 'utf8')).toContain('[app-stdio closed:');
  rmSync(root, { recursive: true, force: true });
});

// ---------------------------------------------------------------------------
// Pass-20 defect (verifier REPORT-PASS20 section 10 / trap 18): the delegated
// exit file was watched for its CREATION and then read ONCE. cmd.exe's
// `echo %ERRORLEVEL% > file` creates the file and writes its content in two
// steps, so that single read could land in between and return "" - which
// discarded a COMPLETED inner run carrying the first native scenario verdict of
// the effort, while the file on disk held 4 parseable bytes (`34 20 0D 0A` =
// "4 \r\n"). Creation is not content. These tests drive the real wait against
// real files in a temp fixture root; nothing is launched.

test('the delegated exit file is read for its content, not for its creation', async () => {
  const { awaitExitFile, parseRelaunchExitFile } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const root = fixtureRoot();
  try {
    const exitPath = join(root, 'relaunch.exit');
    // cmd.exe's redirect, step 1: the file EXISTS and is EMPTY.
    writeFileSync(exitPath, '', 'ascii');
    const pending = awaitExitFile(exitPath, 5_000);
    // Step 2, the measured payload: 4 bytes, "4 \r\n".
    writeFileSync(exitPath, '4 \r\n', 'ascii');
    const waited = await pending;
    expect(waited.timedOut).toBe(false);
    expect(waited.empty).toBe(false);
    expect(waited.bytes).toBe(4);
    expect(parseRelaunchExitFile(waited.text)).toBe(4);
    // ...and the 0-byte read of that same file is never accepted as a verdict.
    expect(parseRelaunchExitFile('')).toBeNull();
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('an exit file created but never written fails typed, naming the path, the bytes and the deadline', async () => {
  const { awaitExitFile } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const root = fixtureRoot();
  try {
    const exitPath = join(root, 'relaunch.exit');
    writeFileSync(exitPath, '', 'ascii');
    const waited = await awaitExitFile(exitPath, 300);
    // The file appeared, so this is NOT the "no exit file" outcome: it is an
    // appeared-but-unwritten exit file, reported with the evidence the pass-20
    // failure lacked (its detail was a bare `""`).
    expect(waited.timedOut).toBe(false);
    expect(waited.empty).toBe(true);
    expect(waited.bytes).toBe(0);
    expect(waited.path).toBe(exitPath);
    expect(waited.budgetMs).toBe(300);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('an exit file that never appears is still the typed no-exit-file outcome', async () => {
  const { awaitExitFile } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const root = fixtureRoot();
  try {
    const waited = await awaitExitFile(join(root, 'relaunch.exit'), 200);
    expect(waited.timedOut).toBe(true);
    expect(waited.empty).toBe(false);
    expect(waited.bytes).toBe(0);
    expect(waited.text).toBeNull();
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('a half-written exit payload is not accepted; the completed payload is', async () => {
  const { awaitExitFile, parseRelaunchExitFile } = await import('../lib/qa-scenarios/windows-interactive.mjs');
  const root = fixtureRoot();
  try {
    const exitPath = join(root, 'relaunch.exit');
    writeFileSync(exitPath, '', 'ascii');
    const pending = awaitExitFile(exitPath, 5_000);
    writeFileSync(exitPath, 'x', 'ascii');
    writeFileSync(exitPath, '0 \r\n', 'ascii');
    const waited = await pending;
    expect(parseRelaunchExitFile(waited.text)).toBe(0);
    expect(waited.bytes).toBe(4);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('waitForFile keeps its creation-only semantics for callers that pass no acceptance predicate', async () => {
  const root = fixtureRoot();
  try {
    const emptyPath = join(root, 'empty');
    writeFileSync(emptyPath, '', 'ascii');
    // The predicate is OPT-IN: an existing empty file still resolves for the
    // barrier and marker callers exactly as it did before.
    await expect(waitForFile(emptyPath)).resolves.toBe('');
    const contentPath = join(root, 'content');
    writeFileSync(contentPath, 'ready\n', 'ascii');
    await expect(waitForFile(contentPath, undefined, { accept: value => value.trim() === 'ready' })).resolves.toBe('ready\n');
    // A predicate that is not satisfied yet does NOT resolve: the watcher stays
    // armed for the write, bounded by the caller's own deadline - which is what
    // the exit-file wait passes and what the old read-once path did not do.
    let stop = () => {};
    const stopPromise = new Promise(resolvePromise => { stop = resolvePromise; });
    const armed = await withDeadline(waitForFile(emptyPath, stopPromise, { accept: value => value.trim() !== '' }), 120, 'not-yet-satisfied', { onStop: stop });
    expect(armed.timedOut).toBe(true);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
