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
// The macOS path never enters this module (admission returns null off win32 and
// for the headless lane, which never touches a GUI window).

import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { BUDGETS, waitForFile, withDeadline } from './common-harness.mjs';

export const WINDOWS_INTERACTIVE_RELAUNCH_ENV = 'FERRYX_QA_WINDOWS_INTERACTIVE_RELAUNCH';
export const WINDOWS_RELAUNCH_RECORD_ENV = 'FERRYX_QA_WINDOWS_RELAUNCH_RECORD';
export const WINDOWS_RELAUNCH_RECORD_FILE = 'windows-interactive-relaunch.json';

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
// the waiting parent can read.
export function buildInteractiveRelaunchPlan({
  taskName, batDir, nodePath, runnerPath, runnerArgs, cwd, timeoutMs = BUDGETS.interactiveRelaunchTimeoutMs,
}) {
  const batPath = join(batDir, `${taskName}.bat`);
  const outPath = join(batDir, 'relaunch.out');
  const errPath = join(batDir, 'relaunch.err');
  const exitPath = join(batDir, 'relaunch.exit');
  const recordPath = join(batDir, 'relaunch-record.json');
  const command = [quoteBatArg(nodePath), quoteBatArg(runnerPath), ...runnerArgs.map(quoteBatArg)].join(' ');
  const batBody = [
    '@echo off',
    `set ${WINDOWS_INTERACTIVE_RELAUNCH_ENV}=1`,
    `set ${WINDOWS_RELAUNCH_RECORD_ENV}=${recordPath}`,
    `cd /d ${quoteBatArg(cwd)}`,
    `${command} > ${quoteBatArg(outPath)} 2> ${quoteBatArg(errPath)}`,
    `echo %ERRORLEVEL% > ${quoteBatArg(exitPath)}`,
  ].join('\r\n');
  return {
    taskName, batPath, batBody, outPath, errPath, exitPath, recordPath, command, timeoutMs,
    createArgs: ['/create', '/tn', taskName, '/tr', batPath, '/sc', 'once', '/st', '00:00', '/f', '/it'],
    runArgs: ['/run', '/tn', taskName],
    endArgs: ['/end', '/tn', taskName],
    deleteArgs: ['/delete', '/tn', taskName, '/f'],
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
  ].join(' ');
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

// Await the delegated run's exit file with the outer budget as the only
// authority (the underlying watcher has its own shorter internal deadline, so
// a fresh bounded wait is re-armed while budget remains). Event-driven: no
// fixed sleep, no polling loop.
async function awaitExitFile(exitPath, timeoutMs) {
  const deadlineAt = Date.now() + timeoutMs;
  while (Date.now() < deadlineAt) {
    const remaining = deadlineAt - Date.now();
    const stopper = deferredStop();
    const attempt = await withDeadline(waitForFile(exitPath, stopper.promise), remaining, 'interactive-relaunch', { onStop: stopper.stop });
    if (attempt.timedOut) return { timedOut: true, text: null };
    if (attempt.error) continue;
    return { timedOut: false, text: attempt.value };
  }
  return { timedOut: true, text: null };
}

// Relaunch this exact runner inside the active console session.
export async function relaunchIntoInteractiveSession({ context, rawArgv, probe, cwd }) {
  const taskName = `ferryx-qa-${context.scenario}-${process.pid}-${Math.random().toString(36).slice(2, 8)}`;
  const batDir = mkdtempSync(join(tmpdir(), 'ferryx-qa-relaunch-'));
  const plan = buildInteractiveRelaunchPlan({
    taskName, batDir,
    nodePath: context.argv[0], runnerPath: context.argv[1], runnerArgs: rawArgv, cwd,
  });
  const record = {
    at: new Date().toISOString(),
    taskName,
    mechanism: 'schtasks /create /it + /run bound to the interactive console user',
    outerProbe: probe,
    argv: [...context.argv, ...rawArgv],
    cwd,
    batPath: plan.batPath,
    evidenceDir: context.evidenceDir,
  };
  const failure = detail => ({
    exitCode: null, innerStdout: null, innerStderr: null,
    detail, report: { ...record, outPath: plan.outPath, errPath: plan.errPath, exitPath: plan.exitPath, detail },
  });
  try {
    writeFileSync(plan.batPath, plan.batBody, { encoding: 'ascii', mode: 0o600 });
    writeFileSync(plan.recordPath, JSON.stringify(record, null, 2), { mode: 0o600 });
    mkdirSync(context.evidenceDir, { recursive: true, mode: 0o700 });
    writeFileSync(join(context.evidenceDir, WINDOWS_RELAUNCH_RECORD_FILE), JSON.stringify(record, null, 2), { mode: 0o600 });

    const created = await runTool('schtasks', plan.createArgs);
    if (created.code !== 0) return failure(`schtasks /create exited ${created.code}: ${(created.stderr || created.stdout).trim()}`);
    const started = await runTool('schtasks', plan.runArgs);
    if (started.code !== 0) return failure(`schtasks /run exited ${started.code}: ${(started.stderr || started.stdout).trim()}`);

    const waited = await awaitExitFile(plan.exitPath, plan.timeoutMs);
    const innerStdout = readText(plan.outPath);
    const innerStderr = readText(plan.errPath);
    if (waited.timedOut) {
      await runTool('schtasks', plan.endArgs);
      return {
        exitCode: null, innerStdout, innerStderr,
        detail: `delegated run in the interactive console session produced no exit file within ${plan.timeoutMs}ms (task ${taskName} ended)`,
        report: { ...record, outPath: plan.outPath, errPath: plan.errPath, exitPath: plan.exitPath },
      };
    }
    const exitCode = parseRelaunchExitFile(waited.text);
    if (exitCode === null) {
      return {
        exitCode: null, innerStdout, innerStderr,
        detail: `delegated run wrote an unreadable exit code ${JSON.stringify(waited.text)}`,
        report: { ...record, outPath: plan.outPath, errPath: plan.errPath, exitPath: plan.exitPath },
      };
    }
    return {
      exitCode, innerStdout, innerStderr,
      report: { ...record, outPath: plan.outPath, errPath: plan.errPath, exitPath: plan.exitPath, delegatedExitCode: exitCode },
    };
  } finally {
    try { await runTool('schtasks', plan.deleteArgs); } catch { /* task teardown must never mask the verdict */ }
    try { rmSync(batDir, { recursive: true, force: true }); } catch { /* temp dir cleanup is best effort */ }
  }
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
//   { mode: 'blocked' }               - typed NO_INTERACTIVE_SESSION / INTERACTIVE_RELAUNCH_FAILED
//   { mode: 'delegated', exitCode }   - a relaunched run owns the evidence
export async function admitWindowsInteractiveDesktop({ invocation, context, rawArgv, cwd = process.cwd() }) {
  if (context.platform !== 'win32' || invocation.headless) return null;
  const probed = await probeWindowsSession();
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
  const relaunched = await relaunchIntoInteractiveSession({ context, rawArgv, probe, cwd });
  if (relaunched.exitCode === null) {
    return {
      mode: 'blocked', code: 'INTERACTIVE_RELAUNCH_FAILED', detail: relaunched.detail,
      evidence: { ...evidence, relaunch: relaunched.report, innerStdout: relaunched.innerStdout, innerStderr: relaunched.innerStderr },
    };
  }
  return {
    mode: 'delegated', exitCode: relaunched.exitCode, evidence,
    relaunch: relaunched.report, innerStdout: relaunched.innerStdout, innerStderr: relaunched.innerStderr,
  };
}
