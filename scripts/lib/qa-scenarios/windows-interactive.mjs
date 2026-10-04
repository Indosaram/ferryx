#!/usr/bin/env node
// Windows interactive-desktop admission for the pane-liveness native lane.
//
// Measured by the pass-4 verifier (task-9/win-pass4): an SSH-launched run lands
// in Windows **session 0** (`INTERACTIVE_SESSION=False`, `SESSION_ID=0`), the
// app's three top-level windows are all `visible=False`, and `MainWindowHandle`
// never becomes non-zero - a window can never be shown in session 0, so the
// native driver can never focus or click anything (`NO_OWNED_WINDOW`).
// `qwinsta` shows session 0 = `services` (Disc) while session 1 =
// `console`/sook/**Active** (explorer/winlogon/dwm). Driving the SAME run into
// session 1 through a scheduled task bound to the logged-on console user
// (`schtasks /create ... /it` then `/run`) is the mechanism the verifier proved
// works: the app owns a real window and UIA enumeration succeeds.
//
// This module implements exactly that mechanism, fail-closed and typed:
//   * probe the current session's interactivity (session id, UserInteractive,
//     active console session from the interactive desktop's explorer);
//   * if this process is not interactive but an active console session exists,
//     re-launch THIS runner (identical argv) inside that session via a
//     scheduled task with `/it`, wait for its bounded exit file, and adopt its
//     exit code - the delegated run owns the evidence (result.json/latest.json);
//   * if no interactive session is reachable - or a relaunch was already
//     attempted - fail typed `NO_INTERACTIVE_SESSION` (never a pass);
//   * a relaunch that never settles fails typed `INTERACTIVE_RELAUNCH_FAILED`.
//
// Pass-18 (task-9 win-pass17): that delegation is INTERMITTENT. Measured on one
// host, one staged tree and one unchanged argv: r1 completed the whole chain in
// 5s while r2/r3 stalled, and in the stalled runs the task's `cmd.exe` existed
// (PPID = the Schedule `svchost`, started at exactly the task's Last Run Time,
// its `conhost` allocated 7ms later) but NEVER executed its own first line - 1
// thread, 0 CPU, waitReason=Executive, and the bat's first marker was never
// written. The cause is unknown and is NOT guessed here; what is implemented is
// detection, a bounded retry and an honest report:
//   * the delegated bat writes an entry marker as its very FIRST statement;
//   * "no marker inside `BUDGETS.interactiveRelaunchEntryMarkerMs` (10s, ~20x the
//     measured 0.380-0.516s success latency)" is a STALLED ATTEMPT, not a hang:
//     it is ended (`/end` + `/delete` by task name, then an exact-PID kill of
//     that attempt's own `cmd.exe`, identity-checked against the unique bat path
//     this process created) and retried;
//   * the retry is bounded by BOTH an attempt cap and a SEQUENCE budget
//     (`BUDGETS.interactiveRelaunchSequenceMs`, 180s): the 12-probe measurement
//     showed the stall is BURSTY (8 consecutive stalls over ~73s, then 4
//     consecutive successes at ~0.5s), so the budget has to outlast a burst and a
//     single attempt may only spend its own 10s window plus teardown;
//   * every attempt is recorded (index, task name, whether the marker appeared,
//     the wait, the outcome, the teardown) in the run's evidence dir and the run
//     reports HOW MANY attempts it needed, so the intermittency is measured
//     rather than hidden;
//   * every attempt is a FRESH delegation (fresh task name, fresh attempt dir,
//     fresh marker path) and a marker path that already exists is refused - a
//     stale marker can never be read as this attempt's success;
//   * all attempts stalling fails typed `DELEGATION_STALLED`, nonzero: never a
//     pass, never an unbounded wait, and never a silent session-0 fallback.
// A deterministic failure (schtasks refusing /create or /run, a marker that
// appears but never exits, an unreadable exit code) is NOT retried, so a retry
// loop cannot hide one.
// The macOS path never enters this module (admission returns null off win32 and
// for the headless lane, which never touches a GUI window).

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { BUDGETS, waitForFile, withDeadline } from './common-harness.mjs';

export const WINDOWS_INTERACTIVE_RELAUNCH_ENV = 'FERRYX_QA_WINDOWS_INTERACTIVE_RELAUNCH';
export const WINDOWS_RELAUNCH_RECORD_ENV = 'FERRYX_QA_WINDOWS_RELAUNCH_RECORD';
export const WINDOWS_RELAUNCH_RECORD_FILE = 'windows-interactive-relaunch.json';
// Pass-18: the whole delegation's attempt log, written into the run's evidence
// dir after EVERY attempt so an interrupted run still carries a truthful,
// partial ledger instead of nothing.
export const WINDOWS_DELEGATION_LEDGER_FILE = 'windows-interactive-delegation.json';
// The marker file the delegated bat writes as its very first statement. Its
// name is fixed, but the directory it lives in is created fresh for each
// attempt, so the path itself is never reused across attempts or across runs.
export const DELEGATION_ENTRY_MARKER_FILE = 'delegation-entry.marker';

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested by replaying the runner's own expressions).

// PowerShell 5.1 `ConvertTo-Json` unwraps single-element arrays in some shapes;
// normalize at this system boundary instead of trusting the shape.
export function asArray(value) {
  if (value === null || value === undefined) return [];
  return Array.isArray(value) ? value : [value];
}

// The probe scripts always exit 0 and print exactly one JSON line, so every
// typed decision below is made in JS and is replayable without launching.
export function parseWindowsProbeLine(stdout, expectedProbe) {
  if (typeof stdout !== 'string') return { ok: false, reason: 'no powershell stdout captured' };
  const lines = stdout.split(/\r?\n/).map(line => line.trim()).filter(Boolean);
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    let parsed;
    try { parsed = JSON.parse(lines[index]); } catch { continue; }
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) continue;
    if (expectedProbe && parsed.probe !== expectedProbe) continue;
    return { ok: true, probe: parsed };
  }
  return { ok: false, reason: `no ${expectedProbe ?? 'json'} probe line in powershell stdout` };
}

export function classifyWindowsSession(probe, { alreadyRelaunched = false } = {}) {
  const rawSessionId = Number(probe?.sessionId);
  const sessionId = Number.isFinite(rawSessionId) ? rawSessionId : null;
  const explorerSessions = asArray(probe?.explorerSessions).map(Number).filter(Number.isFinite);
  const declaredConsole = Number(probe?.activeConsoleSessionId);
  const consoleSessionId = Number.isFinite(declaredConsole)
    ? declaredConsole
    : (explorerSessions.length > 0 ? Math.max(...explorerSessions) : null);
  // A window can only be visible on an interactive desktop: session 0 (the
  // SSH/service session) is never one, and a process whose own session IS the
  // active console session is on the interactive desktop even if the
  // UserInteractive API reports a quirk. The owned-window wait is the second,
  // stronger gate.
  const onActiveConsoleSession = consoleSessionId !== null && consoleSessionId !== 0 && sessionId === consoleSessionId;
  const interactive = sessionId !== 0 && (probe?.interactive === true || onActiveConsoleSession);
  if (interactive) {
    return {
      interactive: true, sessionId, consoleSessionId: null, code: null,
      detail: `runner is in interactive desktop session ${sessionId}`,
    };
  }
  const measured = `interactive=${JSON.stringify(probe?.interactive ?? null)} sessionId=${JSON.stringify(sessionId)} activeConsoleSessionId=${JSON.stringify(consoleSessionId)} explorerSessions=${JSON.stringify(explorerSessions)}`;
  if (alreadyRelaunched) {
    return {
      interactive: false, sessionId, consoleSessionId, code: 'NO_INTERACTIVE_SESSION',
      detail: `relaunch into the active console session was already attempted and this process is still non-interactive (${measured}); refusing to relaunch again`,
    };
  }
  const reachable = consoleSessionId !== null && consoleSessionId !== 0 && consoleSessionId !== sessionId;
  if (!reachable) {
    return {
      interactive: false, sessionId, consoleSessionId, code: 'NO_INTERACTIVE_SESSION',
      detail: `no reachable interactive desktop session: ${measured}`,
    };
  }
  return {
    interactive: false, sessionId, consoleSessionId, code: null,
    detail: `non-interactive session ${sessionId} with active console session ${consoleSessionId} reachable (${measured})`,
  };
}

// Parse the batch file's `echo %ERRORLEVEL% > exit` payload.
export function parseRelaunchExitFile(text) {
  if (typeof text !== 'string') return null;
  const trimmed = text.replace(/^\uFEFF/, '').trim();
  if (!/^-?\d+$/.test(trimmed)) return null;
  const value = Number.parseInt(trimmed, 10);
  return Number.isFinite(value) ? value : null;
}

function quoteBatArg(value) {
  return `"${String(value).replace(/"/g, '""')}"`;
}

// The verifier's proven recipe, made deterministic and testable: a batch file
// that runs THIS runner with the unchanged argv, inside a scheduled task bound
// to the interactive console user (`/it`), writing stdout/stderr/exit-code files
// the waiting parent can read - and, as its FIRST statement, the entry marker
// that proves the process executed its own first line (pass-18). `batDir` is
// the directory this one attempt owns; the caller gives every attempt a fresh
// one, so the marker, the exit file and the record are never reused.
export function buildInteractiveRelaunchPlan({
  taskName, batDir, nodePath, runnerPath, runnerArgs, cwd,
  // The budget for THIS attempt's exit file. The caller passes what is left of
  // the whole retry sequence (pass-19), so a single attempt can never spend the
  // sequence on its own.
  timeoutMs = BUDGETS.interactiveRelaunchSequenceMs,
}) {
  const batPath = join(batDir, `${taskName}.bat`);
  const outPath = join(batDir, 'relaunch.out');
  const errPath = join(batDir, 'relaunch.err');
  const exitPath = join(batDir, 'relaunch.exit');
  const recordPath = join(batDir, 'relaunch-record.json');
  const entryMarkerPath = join(batDir, DELEGATION_ENTRY_MARKER_FILE);
  const command = [quoteBatArg(nodePath), quoteBatArg(runnerPath), ...runnerArgs.map(quoteBatArg)].join(' ');
  const batBody = [
    '@echo off',
    // FIRST statement, before the env, the cd and the runner: the measured
    // pass-17 stall is a process that never executes its own first line, so the
    // marker has to BE that line to be able to see the stall at all.
    `echo DELEGATION_ENTRY %DATE% %TIME% task=${taskName} cwd=%CD% >> ${quoteBatArg(entryMarkerPath)}`,
    `set ${WINDOWS_INTERACTIVE_RELAUNCH_ENV}=1`,
    `set ${WINDOWS_RELAUNCH_RECORD_ENV}=${recordPath}`,
    `cd /d ${quoteBatArg(cwd)}`,
    `${command} > ${quoteBatArg(outPath)} 2> ${quoteBatArg(errPath)}`,
    `echo %ERRORLEVEL% > ${quoteBatArg(exitPath)}`,
  ].join('\r\n');
  return {
    taskName, batPath, batBody, outPath, errPath, exitPath, recordPath, entryMarkerPath, command, timeoutMs,
    createArgs: ['/create', '/tn', taskName, '/tr', batPath, '/sc', 'once', '/st', '00:00', '/f', '/it'],
    runArgs: ['/run', '/tn', taskName],
    endArgs: ['/end', '/tn', taskName],
    deleteArgs: ['/delete', '/tn', taskName, '/f'],
    // A stalled attempt's own task state, recorded RAW (schtasks output is
    // localized, so it is stored as evidence - the pass-17 signature was
    // Status=Running / LastResult=267009 - never parsed by a locale-dependent
    // key match).
    queryArgs: ['/query', '/tn', taskName, '/fo', 'list', '/v'],
  };
}

// ---------------------------------------------------------------------------
// Pass-18: fresh-attempt identity, the anti-stale precondition, and the
// identity-checked teardown of a stalled attempt's own cmd.exe.

// Pure: the task name and the attempt directory of attempt `index`. Both carry
// the run token and the index, so no attempt can collide with another attempt
// of this run or with a leftover of an earlier run.
export function delegationAttemptIdentity({ scenario, pid, index, token }) {
  const suffix = `a${index}-${token}`;
  return {
    taskName: `ferryx-qa-${scenario}-${pid}-${suffix}`,
    attemptDirName: `attempt-${index}-${token}`,
  };
}

// The anti-stale precondition, as a pure decision so the runner unit suite can
// replay it: a marker path that ALREADY exists must never be read as this
// attempt's success, so the attempt is refused instead of started.
export function delegationMarkerPrecondition(markerPath, exists = existsSync) {
  if (exists(markerPath)) {
    return {
      ok: false,
      reason: `entry marker ${markerPath} already existed before this attempt started; refusing to read a stale marker as this attempt's success`,
    };
  }
  return { ok: true, reason: null };
}

// Single-quote one value for a PowerShell literal.
export function psSingleQuote(value) {
  return `'${String(value).replace(/'/g, "''")}'`;
}

// Identity evidence for "this attempt's own cmd.exe". The Schedule service
// creates the process, so this launcher never learns the PID at spawn time;
// what it DOES own is the exact bat path it just wrote inside a directory it
// just created (unique per attempt, recorded in the ledger before any signal).
// A candidate counts as this attempt's only when its command line carries that
// exact path (case-insensitive: the service may echo the action path with
// different case) AND its parent is the Schedule service host. A name or path
// PATTERN is never an ownership proof - a candidate that matches the path but
// not the parent is refused and reported, never killed.
//
// The payload states the interpreter that produced it (`powershell`), because a
// probe result is only readable when the reader knows which PowerShell ran it
// (the verifier's pass-17 lesson: `ArgumentList` read under 5.1 where it does
// not exist looked like a hang).
export function buildDelegationProcessProbeScript(batPath) {
  return [
    "$ErrorActionPreference = 'Continue';",
    `$bat = ${psSingleQuote(batPath)};`,
    "$candidates = @(Get-CimInstance Win32_Process -Filter \"Name='cmd.exe'\" -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -and $_.CommandLine.ToLower().Contains($bat.ToLower()) });",
    '$rows = @();',
    'foreach ($c in $candidates) {',
    '  $parent = Get-CimInstance Win32_Process -Filter ("ProcessId=" + $c.ParentProcessId) -ErrorAction SilentlyContinue;',
    '  $parentName = $null;',
    '  if ($parent) { $parentName = [string]$parent.Name }',
    '  $rows += [ordered]@{',
    '    pid = [int]$c.ProcessId;',
    '    parentPid = [int]$c.ParentProcessId;',
    '    parentName = $parentName;',
    '    creationDate = [string]$c.CreationDate;',
    '    commandLine = [string]$c.CommandLine;',
    '  };',
    '}',
    "$payload = [ordered]@{ probe = 'delegation-attempt-process'; batPath = $bat; powershell = [string]$PSVersionTable.PSVersion; matches = $rows };",
    'Write-Output ($payload | ConvertTo-Json -Compress -Depth 6);',
  ].join('\n');
}

// Pure: split the probe's matches into the processes this attempt may signal
// (`owned`) and those it must only report (`refused`).
export function classifyDelegationAttemptProcesses(payload, { batPath } = {}) {
  const wanted = typeof batPath === 'string' && batPath.length > 0 ? batPath.toLowerCase() : null;
  const rows = Array.isArray(payload?.matches) ? payload.matches : (payload?.matches ? [payload.matches] : []);
  const owned = [];
  const refused = [];
  for (const row of rows) {
    const pid = Number(row?.pid);
    const commandLine = typeof row?.commandLine === 'string' ? row.commandLine.toLowerCase() : '';
    if (!Number.isFinite(pid) || wanted === null || !commandLine.includes(wanted)) {
      refused.push({
        pid: Number.isFinite(pid) ? pid : null,
        reason: 'command line does not carry this attempt\'s exact bat path',
      });
      continue;
    }
    const parentName = typeof row?.parentName === 'string' ? row.parentName.toLowerCase() : null;
    if (parentName !== 'svchost.exe') {
      refused.push({
        pid,
        parentName: row?.parentName ?? null,
        reason: `parent is ${row?.parentName ?? 'unknown'}, not the Schedule service host (svchost.exe)`,
      });
      continue;
    }
    owned.push({
      pid,
      parentPid: Number.isFinite(Number(row?.parentPid)) ? Number(row.parentPid) : null,
      parentName: row.parentName,
      creationDate: row?.creationDate ?? null,
    });
  }
  return { owned, refused };
}

// The compact projection the run reports (stderr verdict line, admission
// evidence): how many attempts were needed, and what each one did.
export function summarizeDelegationLedger(ledger) {
  const attempts = Array.isArray(ledger?.attempts) ? ledger.attempts : [];
  return {
    ledgerPath: ledger?.ledgerPath ?? null,
    attemptsUsed: ledger?.attemptsUsed ?? 0,
    maxAttempts: ledger?.maxAttempts ?? null,
    entryMarkerBudgetMs: ledger?.entryMarkerBudgetMs ?? null,
    sequenceBudgetMs: ledger?.sequenceBudgetMs ?? null,
    stopReason: ledger?.stopReason ?? null,
    interpreter: ledger?.interpreter ?? null,
    stalled: ledger?.stalled === true,
    code: ledger?.code ?? null,
    attempts: attempts.map(attempt => ({
      index: attempt.index,
      taskName: attempt.taskName,
      entryMarkerPath: attempt.entryMarkerPath,
      markerAppeared: attempt.markerAppeared === true,
      markerWaitMs: attempt.markerWaitMs ?? null,
      markerWindowMs: attempt.markerWindowMs ?? null,
      exitBudgetMs: attempt.exitBudgetMs ?? null,
      outcome: attempt.outcome,
      exitCode: attempt.exitCode ?? null,
      exitFileBytes: attempt.exitFileBytes ?? null,
      kill: attempt.teardown?.kill
        ? {
          killed: (attempt.teardown.kill.killed ?? []).map(entry => entry.pid),
          refused: (attempt.teardown.kill.refused ?? []).map(entry => entry.pid),
          reason: attempt.teardown.kill.reason ?? null,
        }
        : null,
    })),
  };
}

// ---------------------------------------------------------------------------
// Process helpers.

function collect(child, timeoutMs) {
  return new Promise(resolvePromise => {
    let stdout = '';
    let stderr = '';
    let settled = false;
    const finish = (code, timedOut) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolvePromise({ code, stdout, stderr, timedOut });
    };
    const timer = setTimeout(() => {
      try { child.kill(); } catch { /* the timeout verdict is already decided */ }
      finish(null, true);
    }, timeoutMs);
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.stderr.on('data', chunk => { stderr += chunk; });
    child.once('error', error => { stderr += `\n${error.message}`; finish(-1, false); });
    child.once('exit', code => finish(code, false));
  });
}

function runPowerShell(script, timeoutMs = BUDGETS.windowsSessionProbeMs) {
  return collect(spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { stdio: ['ignore', 'pipe', 'pipe'] }), timeoutMs);
}

function runTool(file, args, timeoutMs = 15_000) {
  return collect(spawn(file, args, { stdio: ['ignore', 'pipe', 'pipe'] }), timeoutMs);
}

// The session probe is a PowerShell script: its array is newline-joined so each
// statement keeps its own line (same rule as native-driver.mjs - a space join
// would break any here-string added here later).
export function buildWindowsSessionProbeScript() {
  return [
    "$ErrorActionPreference = 'Continue';",
    '$sessions = @();',
    'try { $sessions = @(qwinsta 2>$null | ForEach-Object { "$_" }) } catch { $sessions = @() }',
    "$explorer = @(Get-CimInstance Win32_Process -Filter \"Name='explorer.exe'\" -ErrorAction SilentlyContinue | ForEach-Object { [int]$_.SessionId });",
    '$self = Get-Process -Id $PID;',
    '$active = $null;',
    'if ($explorer.Count -gt 0) { $active = ($explorer | Sort-Object -Descending | Select-Object -First 1) }',
    '$payload = [ordered]@{',
    "  probe = 'windows-session';",
    '  interactive = [bool][System.Environment]::UserInteractive;',
    '  sessionId = [int]$self.SessionId;',
    '  processId = [int]$PID;',
    '  activeConsoleSessionId = $active;',
    '  explorerSessions = $explorer;',
    '  computerName = $env:COMPUTERNAME;',
    '  userName = $env:USERNAME;',
    '  qwinsta = $sessions;',
    '};',
    'Write-Output ($payload | ConvertTo-Json -Compress -Depth 6);',
  ].join('\n');
}

export async function probeWindowsSession({ timeoutMs = BUDGETS.windowsSessionProbeMs } = {}) {
  const result = await runPowerShell(buildWindowsSessionProbeScript(), timeoutMs);
  if (result.timedOut) return { ok: false, reason: `session probe timed out after ${timeoutMs}ms`, stdout: result.stdout };
  if (result.code !== 0) return { ok: false, reason: `session probe powershell exited ${result.code}: ${result.stderr.trim()}`, stdout: result.stdout };
  return { ...parseWindowsProbeLine(result.stdout, 'windows-session'), stdout: result.stdout };
}

function readText(path) {
  try { return readFileSync(path, 'utf8'); } catch { return null; }
}

function deferredStop() {
  let resolveFn;
  const promise = new Promise(resolvePromise => { resolveFn = resolvePromise; });
  return { promise, stop: () => resolveFn() };
}

// The exit file's creation and its payload are TWO events: cmd.exe's
// `echo %ERRORLEVEL% > file` creates the file and then writes its content, so a
// watcher armed on the file's APPEARANCE can read it in between and see zero
// bytes. Pass 20 lost a COMPLETED inner run that way - the run carrying the first
// native scenario verdict of the effort - reporting `unreadable exit code ""`
// while the file on disk held `34 20 0D 0A` = "4 \r\n", four parseable bytes.
// Creation is not content (verifier trap 18).
//
// So this wait is for the CONTENT the caller needs: `waitForFile` gets the
// acceptance predicate below (its watcher stays armed across the create event and
// resolves on the write), and every armed segment is bounded by
// `EXIT_FILE_CONTENT_REARM_MS` so a missed filesystem event cannot hold the wait
// past one short segment. The total budget is the caller's own deadline - no
// unbounded loop and no fixed sleep decides anything here.
const EXIT_FILE_CONTENT_REARM_MS = 250;
// The fallback total budget when the caller passes none (the delegation always
// passes the attempt's remaining sequence budget explicitly).
const EXIT_FILE_CONTENT_BUDGET_MS = BUDGETS.interactiveRelaunchSequenceMs;
// Accepted only when the payload really parses as an exit code: a 0-byte or
// half-written read is "not yet written", never a verdict about the run.
const EXIT_FILE_PAYLOAD_ACCEPTED = text => parseRelaunchExitFile(text) !== null;

export async function awaitExitFile(exitPath, timeoutMs = EXIT_FILE_CONTENT_BUDGET_MS) {
  const budgetMs = Number.isFinite(timeoutMs) && timeoutMs > 0 ? timeoutMs : EXIT_FILE_CONTENT_BUDGET_MS;
  const deadlineAt = Date.now() + budgetMs;
  let appeared = false;
  let lastText = null;
  let lastWatchError = null;
  while (Date.now() < deadlineAt) {
    const remaining = deadlineAt - Date.now();
    const stopper = deferredStop();
    const segmentMs = Math.max(1, Math.min(remaining, EXIT_FILE_CONTENT_REARM_MS));
    const attempt = await withDeadline(waitForFile(exitPath, stopper.promise, { accept: EXIT_FILE_PAYLOAD_ACCEPTED }), segmentMs, 'interactive-relaunch', { onStop: stopper.stop });
    if (attempt.timedOut || attempt.error) {
      if (attempt.error) {
        // A watcher failure (a vanished path, or this segment's own cancellation)
        // re-arms while budget remains instead of ending the wait.
        lastWatchError = String(attempt.error?.message ?? attempt.error);
      }
      // Creation is not content, so the wait continues - but the file's
      // APPEARANCE is recorded here, independently of its payload, so a deadline
      // that expires on an empty file reports an appeared-but-unwritten exit file
      // (the caller's UNREADABLE_EXIT, naming the bytes observed) rather than a
      // missing one.
      if (readText(exitPath) !== null) appeared = true;
      continue;
    }
    const text = attempt.value;
    if (typeof text !== 'string') continue;
    appeared = true;
    lastText = text;
    if (EXIT_FILE_PAYLOAD_ACCEPTED(text)) {
      return { timedOut: false, empty: false, text, bytes: Buffer.byteLength(text, 'utf8'), path: exitPath, budgetMs, watchError: null };
    }
  }
  // One final observation at the deadline: the content the last segment may have
  // missed, and the appearance that decides which typed failure applies.
  const finalText = readText(exitPath);
  if (finalText !== null) {
    appeared = true;
    if (lastText === null) lastText = finalText;
  }
  // The deadline expired: either the file never appeared (the typed no-exit-file
  // outcome), or it appeared without ever carrying a parseable payload - reported
  // with the path, the bytes actually observed and the deadline, never a bare "".
  return {
    timedOut: !appeared,
    empty: appeared,
    text: lastText,
    bytes: typeof lastText === 'string' ? Buffer.byteLength(lastText, 'utf8') : 0,
    path: exitPath,
    budgetMs,
    watchError: lastWatchError,
  };
}

// ---------------------------------------------------------------------------
// Pass-18: one delegation attempt, and the bounded retry around it.

// Wait, event-driven and bounded, for the attempt's FIRST-LINE marker. No sleep
// and no polling: `waitForFile` checks for an existing file before it arms its
// watcher (so a marker written before this call is never missed) and the outer
// deadline cancels the watcher instead of leaking it.
async function awaitEntryMarker(markerPath, timeoutMs) {
  const startedAt = Date.now();
  const stopper = deferredStop();
  const attempt = await withDeadline(waitForFile(markerPath, stopper.promise), timeoutMs, 'interactive-relaunch-entry-marker', { onStop: stopper.stop });
  const waitedMs = Date.now() - startedAt;
  if (attempt.timedOut) return { appeared: false, waitedMs, text: null, watchError: null };
  if (attempt.error) return { appeared: false, waitedMs, text: null, watchError: String(attempt.error?.message ?? attempt.error) };
  // The marker's ASSERTION is its appearance - the bat's first line reached its
  // own redirect, which is exactly what proves the process executed its first
  // line - so the two-step create-then-write can never turn a started attempt
  // into a stall verdict here (that would end and kill a run that really
  // started). The read-once assumption still holds for the recorded TEXT though:
  // the same race can return "" for a marker whose line was written, so the
  // content is re-read within one bounded slice instead of being stored empty.
  const text = String(attempt.value);
  if (text.trim() !== '') return { appeared: true, waitedMs, text, watchError: null };
  const enrichStopper = deferredStop();
  const enriched = await withDeadline(
    waitForFile(markerPath, enrichStopper.promise, { accept: value => value.trim() !== '' }),
    EXIT_FILE_CONTENT_REARM_MS,
    'interactive-relaunch-entry-marker-content',
    { onStop: enrichStopper.stop },
  );
  if (enriched.timedOut || enriched.error) return { appeared: true, waitedMs, text, watchError: null };
  return { appeared: true, waitedMs, text: String(enriched.value), watchError: null };
}

// The stalled attempt's own task state, recorded RAW (see plan.queryArgs).
async function queryDelegationTaskState(plan, runToolFn) {
  try {
    const queried = await runToolFn('schtasks', plan.queryArgs);
    return { code: queried.code ?? null, raw: `${queried.stdout ?? ''}${queried.stderr ?? ''}`.trim().slice(0, 2000) };
  } catch (error) {
    return { code: null, raw: null, error: String(error?.message ?? error) };
  }
}

async function probeDelegationAttemptProcesses(batPath, deps = {}) {
  if (deps.probeAttemptProcesses) return deps.probeAttemptProcesses(batPath);
  const result = await runPowerShell(buildDelegationProcessProbeScript(batPath));
  if (result.timedOut) return { ok: false, reason: `attempt-process probe timed out after ${BUDGETS.windowsSessionProbeMs}ms` };
  if (result.code !== 0) return { ok: false, reason: `attempt-process probe exited ${result.code}: ${result.stderr.trim()}` };
  return parseWindowsProbeLine(result.stdout, 'delegation-attempt-process');
}

// End one stalled attempt: `/end` + `/delete` by task name, then an exact-PID
// kill of that attempt's own cmd.exe - but ONLY when that process is
// identity-checked against the unique bat path this attempt wrote (and its
// parent is the Schedule service host). A name or path pattern is never an
// ownership proof: a candidate that fails the check is reported as refused and
// left alone, and if nothing matches, nothing is killed.
async function endStalledDelegationAttempt({ plan, deps = {} }) {
  const runToolFn = deps.runTool ?? runTool;
  const teardown = { endCode: null, deleteCode: null, kill: { attempted: false, matched: [], refused: [], killed: [], reason: null } };
  const ended = await runToolFn('schtasks', plan.endArgs);
  teardown.endCode = ended.code ?? null;
  const deleted = await runToolFn('schtasks', plan.deleteArgs);
  teardown.deleteCode = deleted.code ?? null;
  const probed = await probeDelegationAttemptProcesses(plan.batPath, deps);
  teardown.kill.attempted = true;
  if (!probed.ok) {
    teardown.kill.reason = `could not identify this attempt's own cmd.exe (${probed.reason}); nothing was killed`;
    return teardown;
  }
  const classified = classifyDelegationAttemptProcesses(probed.probe, { batPath: plan.batPath });
  teardown.kill.matched = classified.owned;
  teardown.kill.refused = classified.refused;
  for (const owned of classified.owned) {
    const killed = await runToolFn('taskkill', ['/PID', String(owned.pid), '/F']);
    teardown.kill.killed.push({
      pid: owned.pid,
      parentPid: owned.parentPid,
      code: killed.code ?? null,
      output: `${killed.stderr ?? ''}${killed.stdout ?? ''}`.trim().slice(0, 400),
    });
  }
  if (classified.owned.length === 0) {
    teardown.kill.reason = "no cmd.exe carries this attempt's exact bat path; nothing was killed (a name or path pattern is not an ownership proof)";
  }
  return teardown;
}

// ONE delegation attempt: a fresh task name and a fresh attempt directory (and
// so a fresh bat path, exit path and entry-marker path) under the run's evidence
// dir, so no artifact of an earlier attempt can be read as this one's.
// Exported so the runner unit suite can drive the fail-closed branches (stale
// marker, stalled teardown) through the injected `deps` seam.
export async function runDelegationAttempt({ index, token, context, rawArgv, probe, cwd, delegationDir, budgetMs, entryMarkerMs, deps = {} }) {
  const runToolFn = deps.runTool ?? runTool;
  const exists = deps.exists ?? existsSync;
  // The stall window is a DECISION POINT inside the attempt, and never larger
  // than what is left of the retry sequence.
  const markerWindowMs = Math.max(1, Math.min(
    Number.isFinite(entryMarkerMs) ? entryMarkerMs : BUDGETS.interactiveRelaunchEntryMarkerMs,
    Number.isFinite(budgetMs) ? budgetMs : BUDGETS.interactiveRelaunchSequenceMs,
  ));
  const { taskName, attemptDirName } = delegationAttemptIdentity({ scenario: context.scenario, pid: process.pid, index, token });
  const batDir = join(delegationDir, attemptDirName);
  mkdirSync(batDir, { recursive: true, mode: 0o700 });
  const plan = buildInteractiveRelaunchPlan({
    taskName, batDir,
    nodePath: context.argv[0], runnerPath: context.argv[1], runnerArgs: rawArgv, cwd,
    timeoutMs: Number.isFinite(budgetMs) ? budgetMs : BUDGETS.interactiveRelaunchSequenceMs,
  });
  const record = {
    at: new Date().toISOString(),
    attemptIndex: index,
    taskName,
    mechanism: 'schtasks /create /it + /run bound to the interactive console user',
    outerProbe: probe,
    argv: [...context.argv, ...rawArgv],
    cwd,
    batPath: plan.batPath,
    entryMarkerPath: plan.entryMarkerPath,
    evidenceDir: context.evidenceDir,
  };
  const ledger = {
    index,
    taskName,
    attemptDir: batDir,
    batPath: plan.batPath,
    entryMarkerPath: plan.entryMarkerPath,
    recordPath: plan.recordPath,
    outPath: plan.outPath,
    errPath: plan.errPath,
    exitPath: plan.exitPath,
    markerPreexisting: null,
    markerAppeared: false,
    markerWaitMs: null,
    markerWindowMs,
    markerText: null,
    createCode: null,
    runCode: null,
    exitBudgetMs: plan.timeoutMs,
    exitFileBytes: null,
    exitFileContentBudgetMs: null,
    taskQuery: null,
    teardown: null,
    outcome: null,
    exitCode: null,
    detail: null,
  };
  const done = (result, detail) => ({
    ...result,
    detail: detail ?? null,
    report: { ...record, outPath: plan.outPath, errPath: plan.errPath, exitPath: plan.exitPath, ...(detail ? { detail } : {}) },
    ledger,
  });
  try {
    writeFileSync(plan.batPath, plan.batBody, { encoding: 'ascii', mode: 0o600 });
    writeFileSync(plan.recordPath, JSON.stringify(record, null, 2), { mode: 0o600 });
    mkdirSync(context.evidenceDir, { recursive: true, mode: 0o700 });
    writeFileSync(join(context.evidenceDir, WINDOWS_RELAUNCH_RECORD_FILE), JSON.stringify(record, null, 2), { mode: 0o600 });

    // Anti-stale precondition: an attempt whose marker path already exists is
    // REFUSED instead of started, so a stale marker can never be read as this
    // attempt's success.
    const precondition = delegationMarkerPrecondition(plan.entryMarkerPath, exists);
    ledger.markerPreexisting = precondition.ok !== true;
    if (!precondition.ok) {
      ledger.outcome = 'STALE_MARKER_REFUSED';
      return done({ outcome: 'STALE_MARKER_REFUSED', stalled: false, code: 'INTERACTIVE_RELAUNCH_FAILED', exitCode: null, innerStdout: null, innerStderr: null }, precondition.reason);
    }

    const created = await runToolFn('schtasks', plan.createArgs);
    ledger.createCode = created.code ?? null;
    if (created.code !== 0) {
      ledger.outcome = 'CREATE_FAILED';
      return done({ outcome: 'CREATE_FAILED', stalled: false, code: 'INTERACTIVE_RELAUNCH_FAILED', exitCode: null, innerStdout: null, innerStderr: null },
        `schtasks /create exited ${created.code}: ${(created.stderr || created.stdout).trim()}`);
    }
    const started = await runToolFn('schtasks', plan.runArgs);
    ledger.runCode = started.code ?? null;
    if (started.code !== 0) {
      ledger.outcome = 'RUN_FAILED';
      return done({ outcome: 'RUN_FAILED', stalled: false, code: 'INTERACTIVE_RELAUNCH_FAILED', exitCode: null, innerStdout: null, innerStderr: null },
        `schtasks /run exited ${started.code}: ${(started.stderr || started.stdout).trim()}`);
    }

    const marker = await awaitEntryMarker(plan.entryMarkerPath, markerWindowMs);
    ledger.markerAppeared = marker.appeared === true;
    ledger.markerWaitMs = marker.waitedMs;
    ledger.markerText = marker.text;
    if (marker.watchError) {
      ledger.outcome = 'MARKER_WATCH_FAILED';
      return done({ outcome: 'MARKER_WATCH_FAILED', stalled: false, code: 'INTERACTIVE_RELAUNCH_FAILED', exitCode: null, innerStdout: null, innerStderr: null },
        `could not watch for the entry marker ${plan.entryMarkerPath}: ${marker.watchError}`);
    }
    if (!ledger.markerAppeared) {
      // The window is a DECISION POINT, not a hard cutoff: re-measure once
      // before any signal, so a marker that landed between the deadline and here
      // is still read as this attempt having run its first line. An attempt that
      // really did execute must never be ended and retried as if it had stalled.
      const lateMarker = readText(plan.entryMarkerPath);
      if (lateMarker !== null) {
        ledger.markerAppeared = true;
        ledger.markerText = lateMarker;
      }
    }
    if (!ledger.markerAppeared) {
      // THE MEASURED STALL: the task exists and its cmd.exe was created, but the
      // bat's own first line never ran. End it, kill it by exact PID where it can
      // be identified, and report the attempt as stalled.
      ledger.taskQuery = await queryDelegationTaskState(plan, runToolFn);
      ledger.teardown = await endStalledDelegationAttempt({ plan, deps });
      ledger.outcome = 'STALLED';
      return done({ outcome: 'STALLED', stalled: true, code: null, exitCode: null, innerStdout: null, innerStderr: null },
        `attempt ${index} stalled before the bat's first line: no entry marker at ${plan.entryMarkerPath} within ${markerWindowMs}ms (task ${taskName} ended; its own cmd.exe was killed by exact PID where it could be identified)`);
    }

    const waited = await awaitExitFile(plan.exitPath, plan.timeoutMs);
    const innerStdout = readText(plan.outPath);
    const innerStderr = readText(plan.errPath);
    if (waited.timedOut) {
      const ended = await runToolFn('schtasks', plan.endArgs);
      ledger.teardown = { endCode: ended.code ?? null, deleteCode: null, kill: { attempted: false, matched: [], refused: [], killed: [], reason: 'the attempt ran its first line; no kill was attempted' } };
      ledger.outcome = 'NO_EXIT_FILE';
      return done({ outcome: 'NO_EXIT_FILE', stalled: false, code: 'INTERACTIVE_RELAUNCH_FAILED', exitCode: null, innerStdout, innerStderr },
        `delegated run in the interactive console session produced no exit file within ${plan.timeoutMs}ms (observed bytes=${waited.bytes ?? 0}: the file never appeared; task ${taskName} ended)`);
    }
    const exitCode = parseRelaunchExitFile(waited.text);
    if (exitCode === null) {
      ledger.outcome = 'UNREADABLE_EXIT';
      // Never a bare "": the typed failure names the path, the bytes actually
      // observed and the deadline that expired (pass 20: the file held 4 bytes -
      // "4 \r\n" - while the single read saw "", and a completed run carrying the
      // first native scenario verdict was discarded as unreadable).
      ledger.exitFileBytes = waited.bytes ?? 0;
      ledger.exitFileContentBudgetMs = waited.budgetMs ?? plan.timeoutMs;
      return done({ outcome: 'UNREADABLE_EXIT', stalled: false, code: 'INTERACTIVE_RELAUNCH_FAILED', exitCode: null, innerStdout, innerStderr },
        `delegated run wrote an unreadable exit code: path=${waited.path ?? plan.exitPath} bytesObserved=${waited.bytes ?? 0} content=${JSON.stringify(waited.text)} after the ${waited.budgetMs ?? plan.timeoutMs}ms exit-file content budget expired (the file appeared, so the run reached its exit line; a 0-byte observation is a create-before-write read, not a verdict about the run)`);
    }
    ledger.outcome = 'COMPLETED';
    ledger.exitCode = exitCode;
    return done({ outcome: 'COMPLETED', stalled: false, code: null, exitCode, innerStdout, innerStderr }, null);
  } finally {
    try { await runToolFn('schtasks', plan.deleteArgs); } catch { /* task teardown must never mask the verdict */ }
  }
}

// Relaunch this exact runner inside the active console session, retrying a
// STALLED attempt a bounded number of times. Returns the delegated exit code, or
// a typed failure - `DELEGATION_STALLED` when the attempts stall before their
// first line (fail-closed: never a pass, never an unbounded wait, never a silent
// session-0 fallback). A deterministic failure is returned on the FIRST attempt.
//
// Pass-19 budget shape: the SEQUENCE carries the budget, not the attempt. A
// stalled attempt costs only the 10s stall window plus its own teardown, and the
// loop keeps retrying (fresh task name, fresh dirs, fresh marker path) until the
// attempt cap or the sequence deadline stops it - so one burst cannot make the
// run spend three minutes on a single attempt, and the sequence outlasts the
// ~73s burst that was measured.
export async function relaunchIntoInteractiveSession({ context, rawArgv, probe, cwd, deps = {} }) {
  const runAttempt = deps.runAttempt ?? runDelegationAttempt;
  const maxAttempts = Number.isFinite(deps.maxAttempts) ? deps.maxAttempts : BUDGETS.interactiveRelaunchAttempts;
  const entryMarkerMs = Number.isFinite(deps.entryMarkerMs) ? deps.entryMarkerMs : BUDGETS.interactiveRelaunchEntryMarkerMs;
  const sequenceMs = Number.isFinite(deps.sequenceMs) ? deps.sequenceMs : BUDGETS.interactiveRelaunchSequenceMs;
  const now = deps.now ?? Date.now;
  const sequenceDeadlineAt = now() + sequenceMs;
  const token = deps.token ?? Math.random().toString(36).slice(2, 8);
  const delegationDir = join(context.evidenceDir, 'delegation');
  mkdirSync(delegationDir, { recursive: true, mode: 0o700 });
  const ledger = {
    at: new Date().toISOString(),
    scenario: context.scenario ?? null,
    mechanism: 'schtasks /create /it + /run bound to the interactive console user',
    // Which interpreter/version produced this ledger (the verifier's pass-17
    // rule: a result is only readable when the reader knows what ran it).
    interpreter: { node: process.version, platform: process.platform, arch: process.arch },
    entryMarkerFile: DELEGATION_ENTRY_MARKER_FILE,
    entryMarkerBudgetMs: entryMarkerMs,
    sequenceBudgetMs: sequenceMs,
    maxAttempts,
    attemptsUsed: 0,
    stopReason: null,
    stalled: false,
    code: null,
    ledgerPath: join(context.evidenceDir, WINDOWS_DELEGATION_LEDGER_FILE),
    attempts: [],
  };
  // Written after EVERY attempt: an interrupted run still carries the attempts
  // that really happened, and the run reports how many it needed.
  const persist = () => {
    try { writeFileSync(ledger.ledgerPath, JSON.stringify(ledger, null, 2), { mode: 0o600 }); } catch { /* the ledger must never mask the verdict */ }
  };
  persist();

  for (let index = 1; index <= maxAttempts; index += 1) {
    // The sequence budget is checked BEFORE each attempt: the loop can neither
    // outrun its own deadline nor leave an attempt with no budget at all.
    const remaining = sequenceDeadlineAt - now();
    if (remaining <= 0) {
      ledger.stopReason = 'sequence-budget-exhausted';
      persist();
      break;
    }
    const attempt = await runAttempt({ index, token, context, rawArgv, probe, cwd, delegationDir, budgetMs: remaining, entryMarkerMs, deps });
    ledger.attempts.push(attempt.ledger);
    ledger.attemptsUsed = index;
    ledger.code = attempt.code ?? null;
    ledger.stalled = attempt.stalled === true;
    persist();
    if (attempt.exitCode !== null) {
      ledger.code = null;
      ledger.stalled = false;
      ledger.stopReason = 'delegated-run-completed';
      persist();
      return {
        exitCode: attempt.exitCode,
        innerStdout: attempt.innerStdout ?? null,
        innerStderr: attempt.innerStderr ?? null,
        code: null,
        stalled: false,
        detail: null,
        report: { ...attempt.report, delegatedExitCode: attempt.exitCode, delegation: summarizeDelegationLedger(ledger) },
        ledger,
      };
    }
    if (attempt.stalled !== true) {
      // A deterministic failure is never retried: the retry loop must not be able
      // to hide one behind a second (or third) identical refusal.
      ledger.code = attempt.code ?? 'INTERACTIVE_RELAUNCH_FAILED';
      ledger.stalled = false;
      ledger.stopReason = 'deterministic-failure';
      persist();
      return {
        exitCode: null,
        code: ledger.code,
        stalled: false,
        detail: attempt.detail,
        innerStdout: attempt.innerStdout ?? null,
        innerStderr: attempt.innerStderr ?? null,
        report: { ...attempt.report, delegation: summarizeDelegationLedger(ledger) },
        ledger,
      };
    }
  }
  // The attempts stalled and nothing is left to try: fail closed and typed, with
  // the whole attempt log in the evidence.
  const last = ledger.attempts[ledger.attempts.length - 1] ?? null;
  ledger.stalled = true;
  ledger.code = 'DELEGATION_STALLED';
  if (ledger.stopReason === null) ledger.stopReason = 'attempts-exhausted';
  persist();
  return {
    exitCode: null,
    code: 'DELEGATION_STALLED',
    stalled: true,
    detail: `${ledger.attemptsUsed} delegation attempt(s) stalled before executing the bat's first line (no entry marker within ${entryMarkerMs}ms on any attempt; the ${sequenceMs}ms retry sequence ended by ${ledger.stopReason}; each attempt was ended and its own cmd.exe killed by exact PID where it could be identified) - refusing to fall back to a session-0 launch that could not show a window`,
    innerStdout: null,
    innerStderr: null,
    report: { ...(last?.report ?? {}), delegation: summarizeDelegationLedger(ledger) },
    ledger,
  };
}

// The delegated run reads this to record why it was relaunched.
export function readWindowsRelaunchRecord(env = process.env) {
  const path = env[WINDOWS_RELAUNCH_RECORD_ENV];
  if (!path) return null;
  const text = readText(path);
  if (text === null) return { path, record: null, reason: 'record file unreadable' };
  try { return { path, record: JSON.parse(text) }; } catch (error) { return { path, record: null, reason: error.message }; }
}

// Admission decision for the native lane. Returns null when this lane does not
// need an interactive desktop (non-win32, or the headless lane which never
// touches a GUI window), otherwise one of:
//   { mode: 'native-desktop' }        - this process is on the interactive desktop
//   { mode: 'blocked' }               - typed NO_INTERACTIVE_SESSION /
//                                       INTERACTIVE_RELAUNCH_FAILED /
//                                       DELEGATION_STALLED
//   { mode: 'delegated', exitCode }   - a relaunched run owns the evidence
// `deps` is injectable so the runner unit suite can replay every decision
// (probe, relaunch) deterministically without a Windows host, a scheduled task
// or a GUI: production always uses the real probe and the real relaunch.
export async function admitWindowsInteractiveDesktop({ invocation, context, rawArgv, cwd = process.cwd(), deps = {} }) {
  if (context.platform !== 'win32' || invocation.headless) return null;
  const probeSession = deps.probeSession ?? probeWindowsSession;
  const relaunch = deps.relaunch ?? relaunchIntoInteractiveSession;
  const probed = await probeSession();
  if (!probed.ok) {
    // Fail closed without claiming a session state that was never measured.
    return {
      mode: 'blocked', code: 'NATIVE_AUTOMATION_UNSUPPORTED',
      detail: `the Windows session probe produced no structured evidence (${probed.reason}); refusing to launch a GUI run whose desktop is unknown`,
      evidence: { probe: 'windows-session', unparsed: probed.reason, stdout: probed.stdout ?? null },
    };
  }
  const probe = probed.probe;
  const verdict = classifyWindowsSession(probe, {
    alreadyRelaunched: process.env[WINDOWS_INTERACTIVE_RELAUNCH_ENV] === '1',
  });
  const evidence = {
    probe: 'windows-session',
    ...probe,
    verdict: { interactive: verdict.interactive, code: verdict.code, detail: verdict.detail },
  };
  if (verdict.interactive) return { mode: 'native-desktop', evidence };
  if (verdict.code === 'NO_INTERACTIVE_SESSION') {
    return { mode: 'blocked', code: 'NO_INTERACTIVE_SESSION', detail: verdict.detail, evidence };
  }
  const relaunched = await relaunch({ context, rawArgv, probe, cwd, deps });
  const delegation = summarizeDelegationLedger(relaunched.ledger);
  if (relaunched.exitCode === null) {
    return {
      mode: 'blocked', code: relaunched.code ?? 'INTERACTIVE_RELAUNCH_FAILED', detail: relaunched.detail,
      evidence: { ...evidence, relaunch: relaunched.report, delegation, innerStdout: relaunched.innerStdout, innerStderr: relaunched.innerStderr },
      delegation,
    };
  }
  return {
    mode: 'delegated', exitCode: relaunched.exitCode, evidence,
    relaunch: relaunched.report, innerStdout: relaunched.innerStdout, innerStderr: relaunched.innerStderr,
    // How many scheduled-task attempts one of them needed before it ran its own
    // first line: the intermittency is measured and reported, never hidden.
    delegation,
  };
}
