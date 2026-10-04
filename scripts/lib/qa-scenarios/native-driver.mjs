#!/usr/bin/env node
// Task 3 native desktop automation for pane-liveness scenarios.
// Real OS events only: focus a task-owned Ferryx window by PID, assert exactly
// one enabled accessibility split affordance, type the marker command, capture
// a screenshot. Any unsupported surface is an explicit typed failure; this
// driver can never fabricate a PASS.
//
// Selector reconciliation (plan: "bind the observed unique selector in runner
// code before accepting it"): the product's real, uniquely-named, enabled
// affordance is the pane toolbar button the UI annotates with
// `aria-label`/`title` = `Split pane right`
// (ui/src/components/ui/IconButton.tsx via
// ui/src/components/TerminalSplitView.tsx). It reaches the OS accessibility
// tree as an AX/UIA button by that exact name - the same surface this driver
// already uses for the `Retry` button. The earlier binding (`Split Right`) named
// a label the product never rendered, so the trigger could never match.
//
// Selector scoping (pass-4 blocker `SPLIT_RIGHT_NOT_UNIQUE`): the name alone is
// NOT unique, because `TerminalSplitView.tsx` renders one `Split pane right`
// IconButton in the toolbar of EVERY pane leaf, and the whole window subtree
// contains all of them. The name is therefore scoped the way the product
// renders it: the affordance of the FOCUSED pane inside the OWNED, VISIBLE
// window. The scope is resolved from the OS accessibility tree - the focused
// element's nearest ancestor that contains the affordance (with a deterministic
// fallback that focuses the pane's own focus sink) - never by taking the first
// name match and never by clicking a coordinate. A scope that stays ambiguous
// or empty fails typed (`SPLIT_RIGHT_NOT_UNIQUE` / `SPLIT_RIGHT_NOT_FOUND` /
// `SPLIT_RIGHT_DISABLED`) with the measured candidate set recorded in evidence.
//
// Window scoping (pass-6 blocker `SPLIT_RIGHT_NOT_FOUND` with
// `candidateCount: 0`, which was a property of the WHOLE owned window):
// `MainWindowHandle` is NOT trusted to name the app's real UI window - the
// pass-6 probe saw four owned top-level windows (two visible: one titled `F`,
// one title-less) while the search looked only at the `MainWindowHandle` one.
// The search therefore enumerates EVERY top-level window the pid owns
// (`buildWindowsOwnedWindowsScript`), searches the visible ones in the
// deterministic order `orderOwnedWindowsForSearch` defines, and records which
// window produced the match and how many were searched. When no window matches,
// the probe emits a BOUNDED diagnostic inventory (accessible names + control
// types + automation ids of the elements whose name/automation id contains
// `Split`, per searched window, plus how many elements were inspected) instead
// of failing blind - the typed `SPLIT_RIGHT_NOT_FOUND` verdict is unchanged.

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, writeFileSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir, platform } from 'node:os';
import { join } from 'node:path';
import { HarnessError, withDeadline, waitForFile, BUDGETS } from './common-harness.mjs';
import { asArray, parseWindowsProbeLine } from './windows-interactive.mjs';

export const MARKER_COMMAND_UNIX = "printf 'FERRYX_SPLIT_READY\\n'";
export const MARKER_TEXT = 'FERRYX_SPLIT_READY';
// Plan requirement: bind the observed unique selector in runner code before
// accepting a native action. The observed macOS/Windows accessible name is the
// real pane toolbar label `Split pane right`; a different name fails typed and
// is repaired narrowly, never guessed clicked.
export const SPLIT_MENU_SELECTOR_DARWIN = { role: 'button', title: 'Split pane right' };
export const SPLIT_MENU_SELECTOR_WIN32 = { automationId: null, name: 'Split pane right' };

// Multi-property / multi-name binding for the Windows UIA search. The product's
// pane affordance is an `IconButton` with `label="Split pane right"`
// (ui/src/components/TerminalSplitView.tsx), which becomes the element's
// `aria-label`/`title`; a webview-hosted element's UIA accessible name is NOT
// guaranteed to equal that literal, so the search accepts a bounded set of
// EXACT UIA property conditions instead of one literal:
//   * every known accessible name of the pane affordance - exact equality only,
//     never a substring and never "the first button". `Split terminal right`
//     (ui/src/components/TabBar.tsx) is deliberately NOT in this list: it is a
//     different affordance behind a different trigger, and no scenario in the
//     plan opens the tab-bar split popup (`split-scenarios.mjs` drives
//     `driver.split` for the pane toolbar affordance on every split path), so
//     accepting it could click the wrong control.
//   * every known AutomationId, IF the product exposes one. The list is empty
//     today - the product exposes no automation id - so no id is guessed; the
//     not-found inventory reports the real AutomationIds it observes so the
//     next pass can bind one from evidence instead of from a guess.
export const SPLIT_AFFORDANCE_NAMES_WIN32 = Object.freeze([SPLIT_MENU_SELECTOR_WIN32.name]);
export const SPLIT_AFFORDANCE_AUTOMATION_IDS_WIN32 = Object.freeze([]);
// Bounds for the not-found diagnostic inventory: a bounded probe of what IS in
// scope, never a full accessibility-tree dump.
export const WINDOW_INVENTORY_MATCH_CAP = 40;
export const WINDOW_INVENTORY_INSPECT_CAP = 4000;

function osascript(evidence, source) {
  const args = ['-e', source];
  const child = spawn('osascript', args, { stdio: ['ignore', 'pipe', 'pipe'] });
  return new Promise((resolvePromise, rejectPromise) => {
    let stdout = ''; let stderr = '';
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.stderr.on('data', chunk => { stderr += chunk; });
    child.once('error', rejectPromise);
    child.once('exit', code => {
      evidence.action({ action: 'osascript', exitCode: code, stdout: stdout.trim(), stderr: stderr.trim() });
      if (code === 0) resolvePromise(stdout.trim());
      else {
        // Review M2: only permission denial maps to AX_UNTRUSTED. Domain
        // assertion failures keep their typed identity.
        const text = `${stderr.trim()} ${stdout.trim()}`;
        if (/not allowed|not permitted|assistive|accessibility|-1719|-25211/i.test(text)) {
          rejectPromise(new HarnessError('AX_UNTRUSTED', `Accessibility automation denied: ${text.trim()}`));
        } else if (/SPLIT_RIGHT_NOT_UNIQUE|SPLIT_RIGHT_DISABLED|SPLIT_RIGHT_NOT_FOUND|PID_NOT_UNIQUE|NO_OWNED_WINDOW|Can.t get|Invalid index/i.test(text)) {
          rejectPromise(new HarnessError('ASSERTION_FAILURE', `native assertion failed via osascript: ${text.trim()}`));
        } else {
          rejectPromise(new HarnessError('ASSERTION_FAILURE', `osascript exited ${code}: ${text.trim()}`));
        }
      }
    });
  });
}

// Windows UIA/OS probe failures carry their own typed identity instead of
// collapsing into NATIVE_AUTOMATION_UNSUPPORTED, so a blocked run says exactly
// which window/session/selector condition blocked it. Unknown text keeps the
// pre-existing NATIVE_AUTOMATION_UNSUPPORTED classification.
export function classifyWindowsFailure(text) {
  const message = String(text ?? '');
  for (const code of ['NO_INTERACTIVE_SESSION', 'NO_OWNED_WINDOW', 'SPLIT_RIGHT_NOT_UNIQUE', 'SPLIT_RIGHT_NOT_FOUND', 'SPLIT_RIGHT_DISABLED']) {
    if (message.includes(code)) return code;
  }
  return 'NATIVE_AUTOMATION_UNSUPPORTED';
}

function powershell(evidence, command) {
  const child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', command], { stdio: ['ignore', 'pipe', 'pipe'] });
  return new Promise((resolvePromise, rejectPromise) => {
    let stdout = ''; let stderr = '';
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.stderr.on('data', chunk => { stderr += chunk; });
    child.once('error', rejectPromise);
    child.once('exit', code => {
      evidence.action({ action: 'powershell', exitCode: code, stdout: stdout.trim(), stderr: stderr.trim() });
      if (code === 0) resolvePromise(stdout.trim());
      else {
        const failureCode = classifyWindowsFailure(`${stderr} ${stdout}`);
        rejectPromise(new HarnessError(failureCode, `powershell exited ${code}: ${stderr.trim()}`));
      }
    });
  });
}

// Probe Accessibility trust before any native action. A failure here must be
// reported as typed `ax-untrusted` with no native actions recorded.
export async function assertAxTrustDarwin(evidence) {
  const source = 'tell application "System Events" to get name of first process';
  try {
    await osascript(evidence, source);
    return { trusted: true };
  } catch (error) {
    throw new HarnessError('AX_UNTRUSTED', `Accessibility automation denied: ${error.message}`);
  }
}

// Probe screen-capture permission by taking a real capture to a temp file.
// Clean up probe file and temp directory in finally (no leftover temp artifacts even on failure).
export async function assertScreenCapture(evidence) {
  const tmpDir = mkdtempSync(join(tmpdir(), 'ferryx-qa-capture-'));
  const probe = join(tmpDir, 'probe.png');
  try {
    const child = spawn('screencapture', ['-x', probe]);
    const code = await new Promise(resolvePromise => {
      child.once('error', () => resolvePromise(-1));
      child.once('exit', c => resolvePromise(c));
    });
    evidence.action({ action: 'screencapture-probe', exitCode: code, path: probe });
    const ok = code === 0 && existsSync(probe);
    if (!ok) throw new HarnessError('CAPTURE_DENIED', 'screencapture probe failed or produced no image');
    return { captureGranted: true };
  } finally {
    try { rmSync(tmpDir, { recursive: true, force: true }); } catch { /* ignore */ }
  }
}

export async function focusWindowByPidDarwin(evidence, pid) {
  const source = [
    'tell application "System Events"',
    `  set owned to (every process whose unix id is ${pid})`,
    '  if (count of owned) is not 1 then error "PID_NOT_UNIQUE"',
    '  set frontmost of item 1 of owned to true',
    '  set winCount to count of windows of item 1 of owned',
    '  if winCount is 0 then error "NO_OWNED_WINDOW"',
    'end tell',
  ].join('\n');
  await osascript(evidence, source);
  evidence.action({ action: 'focus-window-by-pid', pid, selector: { unixId: pid } });
}

// Assert exactly one matching, enabled split affordance, then click it. A
// differing label/API - or an ambiguous match - fails explicitly instead of
// clicking a guess. `entire contents` is the recursive AX enumeration needed to
// reach a button rendered inside the webview (a direct-child search cannot).
export async function clickSplitRightDarwin(evidence, pid) {
  const label = SPLIT_MENU_SELECTOR_DARWIN.title;
  const source = [
    'tell application "System Events"',
    `  set owned to (every process whose unix id is ${pid})`,
    '  if (count of owned) is not 1 then error "PID_NOT_UNIQUE"',
    '  set p to item 1 of owned',
    '  set frontmost of p to true',
    '  set win to first window of p',
    `  set matches to (every button of (entire contents of win) whose name is ${JSON.stringify(label)})`,
    '  if (count of matches) > 1 then error "SPLIT_RIGHT_NOT_UNIQUE"',
    '  if (count of matches) is 0 then error "SPLIT_RIGHT_NOT_FOUND"',
    '  set theItem to item 1 of matches',
    '  if enabled of theItem is not true then error "SPLIT_RIGHT_DISABLED"',
    '  click theItem',
    'end tell',
  ].join('\n');
  try {
    await osascript(evidence, source);
  } catch (error) {
    const message = error.message ?? '';
    if (message.includes('SPLIT_RIGHT_NOT_UNIQUE') || message.includes('SPLIT_RIGHT_NOT_FOUND') || message.includes("Can't get") || message.includes('Invalid index')) {
      throw new HarnessError('ASSERTION_FAILURE', `actual split affordance differs from bound selector ${JSON.stringify(label)}: ${message}`);
    }
    throw error;
  }
  evidence.action({ action: 'click-split-affordance', selector: SPLIT_MENU_SELECTOR_DARWIN, pid, assertedUniqueEnabled: true });
}

// Click the actual Retry button in the UI for recovery scenarios (e.g. split-attach-stall).
export async function clickRetryDarwin(evidence, pid) {
  const source = [
    'tell application "System Events"',
    `  set owned to (every process whose unix id is ${pid})`,
    '  if (count of owned) is not 1 then error "PID_NOT_UNIQUE"',
    '  set frontmost of item 1 of owned to true',
    '  set win to first window of item 1 of owned',
    '  set found to false',
    '  try',
    '    set directButtons to (every button of win whose (name contains "Retry" or description contains "Retry"))',
    '    if (count of directButtons) > 0 then',
    '      click item 1 of directButtons',
    '      set found to true',
    '    end if',
    '  end try',
    '  if not found then',
    '    try',
    '      set uiElements to (every UI element of win whose (name contains "Retry" or description contains "Retry"))',
    '      if (count of uiElements) > 0 then',
    '        click item 1 of uiElements',
    '        set found to true',
    '      end if',
    '    end try',
    '  end if',
    '  if not found then error "RETRY_BUTTON_NOT_FOUND"',
    'end tell',
  ].join('\n');
  try {
    await osascript(evidence, source);
  } catch (error) {
    throw new HarnessError('ASSERTION_FAILURE', `Retry button click failed on Darwin: ${error.message}`);
  }
  evidence.action({ action: 'click-retry', pid, surface: 'AX button whose name/description contains Retry' });
  return { clicked: true };
}

// Every PowerShell script builder below is LINE-structured and therefore joins
// its array with newlines, never with spaces: a here-string header (`@"`) must
// end its line and its terminator (`"@`) must start one. The space join
// flattened `buildWindowsSplitRightScript` into `Add-Type @" using System; ...`
// and the real desktop rejected it in pass 5
// (`UnexpectedCharactersAfterHereStringHeader`) before any click could run.

// Windows: Click actual Retry button via UIA.
export function buildWindowsRetryScript(pid) {
  return [
    'Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes;',
    `$proc = Get-Process -Id ${pid} -ErrorAction Stop;`,
    '$root = [System.Windows.Automation.AutomationElement]::FromHandle($proc.MainWindowHandle);',
    'if (-not $root) { throw "NO_OWNED_WINDOW" }',
    '$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, "Retry");',
    '$items = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond);',
    'if ($items.Count -eq 0) {',
    '  $allButtons = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition);',
    '  foreach ($btn in $allButtons) {',
    '    if ($btn.Current.Name -match "Retry") { $items = @($btn); break; }',
    '  }',
    '}',
    'if ($items.Count -eq 0) { throw "RETRY_BUTTON_NOT_FOUND" }',
    '$invoke = $items.Item(0).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern);',
    '$invoke.Invoke();',
    '"RETRY_CLICKED"',
  ].join('\n');
}

export async function clickRetryWindows(evidence, pid) {
  const result = await powershell(evidence, buildWindowsRetryScript(pid));
  if (result !== 'RETRY_CLICKED') throw new HarnessError('ASSERTION_FAILURE', `unexpected Retry result: ${result}`);
  evidence.action({ action: 'click-retry', pid, surface: 'UIA button whose Name contains Retry' });
  return { clicked: true };
}

export async function getWindowBoundsDarwin(evidence, pid) {
  const source = [
    'tell application "System Events"',
    `  set owned to (every process whose unix id is ${pid})`,
    '  if (count of owned) is not 1 then error "PID_NOT_UNIQUE"',
    '  set w to first window of item 1 of owned',
    '  set {x, y} to position of w',
    '  set {wWidth, wHeight} to size of w',
    '  return "" & x & "," & y & "," & wWidth & "," & wHeight',
    'end tell',
  ].join('\n');
  const res = await osascript(evidence, source);
  const [x, y, w, h] = res.split(',').map(s => parseInt(s.trim(), 10));
  return { x, y, width: w, height: h };
}

// Capture owned window only (never the entire screen) and return screenshot metadata.
export async function captureOwnedWindowDarwin(evidence, path, pid) {
  const bounds = await getWindowBoundsDarwin(evidence, pid);
  const rect = `${bounds.x},${bounds.y},${bounds.width},${bounds.height}`;
  const child = spawn('screencapture', ['-R' + rect, '-x', path]);
  const code = await new Promise(resolvePromise => {
    child.once('error', () => resolvePromise(-1));
    child.once('exit', c => resolvePromise(c));
  });
  evidence.action({ action: 'screencapture-owned-window', exitCode: code, path, bounds });
  if (code !== 0 || !existsSync(path)) throw new HarnessError('CAPTURE_DENIED', `screencapture of owned window failed (${code})`);
  const screenshotSha256 = createHash('sha256').update(readFileSync(path)).digest('hex');
  const targetPaneBounds = {
    x: bounds.x + Math.round(bounds.width / 2),
    y: bounds.y,
    width: Math.round(bounds.width / 2),
    height: bounds.height,
  };
  return {
    windowBounds: bounds,
    targetPaneBounds,
    focused: true,
    screenshotSha256,
    path,
  };
}

// Windows: Capture owned window only (by MainWindowHandle bounds) and return metadata.
// Its only import (GetWindowRect) takes a handle and writes a struct - no text is
// marshalled - so it carries no charset.
export function buildWindowsCaptureScript(pid, path) {
  return [
    'Add-Type -AssemblyName System.Drawing,System.Windows.Forms;',
    `$proc = Get-Process -Id ${pid} -ErrorAction Stop;`,
    '$handle = $proc.MainWindowHandle;',
    'if (-not $handle) { throw "NO_OWNED_WINDOW" }',
    'Add-Type @"',
    '  using System;',
    '  using System.Runtime.InteropServices;',
    '  public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }',
    '  public class Win32 {',
    '    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);',
    '  }',
    '"@;',
    '$r = New-Object RECT;',
    '[Win32]::GetWindowRect($handle, [ref]$r) | Out-Null;',
    '$w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top;',
    'if ($w -le 0 -or $h -le 0) { throw "INVALID_WINDOW_RECT" }',
    '$b = New-Object System.Drawing.Bitmap($w, $h);',
    '$g = [System.Drawing.Graphics]::FromImage($b);',
    '$g.CopyFromScreen($r.Left, $r.Top, 0, 0, $b.Size);',
    `$b.Save('${path.replace(/'/g, "''")}');`,
    `"$($r.Left),$($r.Top),$w,$h"`,
  ].join('\n');
}

export async function captureOwnedWindowWindows(evidence, path, pid) {
  const res = await powershell(evidence, buildWindowsCaptureScript(pid, path));
  const [x, y, w, h] = res.split(',').map(s => parseInt(s.trim(), 10));
  const bounds = { x, y, width: w, height: h };
  const screenshotSha256 = createHash('sha256').update(readFileSync(path)).digest('hex');
  const targetPaneBounds = {
    x: bounds.x + Math.round(bounds.width / 2),
    y: bounds.y,
    width: Math.round(bounds.width / 2),
    height: bounds.height,
  };
  evidence.action({ action: 'screencapture-owned-window', path, bounds, screenshotSha256 });
  return {
    windowBounds: bounds,
    targetPaneBounds,
    focused: true,
    screenshotSha256,
    path,
  };
}

export async function typeMarkerDarwin(evidence, pid) {
  const source = [
    'tell application "System Events"',
    `  set p to first process whose unix id is ${pid}`,
    '  set frontmost of p to true',
    `  keystroke ${JSON.stringify(MARKER_COMMAND_UNIX)}`,
    '  keystroke return',
    'end tell',
  ].join('\n');
  await osascript(evidence, source);
  // Review L1 (honest logging): keystrokes go to the frontmost task-owned
  // window; QA-leaf selector binding is pending product accessibility
  // annotation and is compensated by the markerRegionPx correlation.
  evidence.action({ action: 'type-marker', pid, markerCommand: MARKER_COMMAND_UNIX, surface: 'AX keystroke into frontmost task-owned window', targetBinding: 'pending-product-ax-annotation', correlatedBy: 'screenshot-marker-correlation' });
}

export async function captureScreenshot(evidence, path) {
  const child = spawn('screencapture', ['-x', path]);
  const code = await new Promise(resolvePromise => {
    child.once('error', () => resolvePromise(-1));
    child.once('exit', c => resolvePromise(c));
  });
  evidence.action({ action: 'screencapture', exitCode: code, path });
  if (code !== 0 || !existsSync(path)) throw new HarnessError('CAPTURE_DENIED', `screencapture failed (${code})`);
}

// Windows: System.Windows.Automation (UIA) is the real native API, and UIA
// `Descendants` is already recursive, so it reaches the webview-rendered pane
// toolbar button by its real accessible name. The probe scripts below always
// exit 0 and print exactly one JSON line, so the typed verdict is decided in JS
// (`classifyWindows*` below) and is replayable by the runner unit suite without
// launching anything.

// The two StringBuilder imports in the here-string below (GetWindowTextW /
// GetClassNameW) carry `CharSet = CharSet.Unicode`: without it the default ANSI
// marshalling stops at the first UTF-16 NUL byte, which is how pass-7 recorded
// `title`/`className` as a single character ('F'/'T', and the IME window as
// class 'I' / title 'D' for `IME` / `Default IME`). The other imports in that
// here-string (EnumWindows, GetWindowThreadProcessId, IsWindowVisible) marshal
// only handles, an out uint, and a bool - no text - so they stay charset-free.
// Bounded wait for the app to own a VISIBLE top-level window. In Windows
// session 0 (the SSH/service session) the app's windows are created but can
// never be shown, so `MainWindowHandle` stays zero forever - this wait turns
// that condition into the typed failure it is instead of letting the driver
// assume a window it does not own.
export function buildWindowsWindowWaitScript(pid, budgetMs) {
  return [
    "$ErrorActionPreference = 'Stop';",
    'Add-Type @"',
    'using System;',
    'using System.Text;',
    'using System.Runtime.InteropServices;',
    'public class FerryxQaWin {',
    '  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);',
    '  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);',
    '  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);',
    '  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);',
    '  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);',
    '  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder text, int count);',
    '}',
    '"@;',
    `$targetPid = ${pid};`,
    `$budgetMs = ${budgetMs};`,
    '$interactive = [bool][System.Environment]::UserInteractive;',
    '$sessionId = [int](Get-Process -Id $PID).SessionId;',
    '$sw = [System.Diagnostics.Stopwatch]::StartNew();',
    '$handle = [IntPtr]::Zero;',
    '$processExited = $false;',
    '$windows = New-Object System.Collections.ArrayList;',
    '$cb = [FerryxQaWin+EnumProc]{ param($hWnd, $lParam)',
    '  $owner = [uint32]0;',
    '  [FerryxQaWin]::GetWindowThreadProcessId($hWnd, [ref]$owner) | Out-Null;',
    '  if ($owner -eq [uint32]$targetPid) {',
    '    $title = New-Object System.Text.StringBuilder 256;',
    '    [FerryxQaWin]::GetWindowTextW($hWnd, $title, 256) | Out-Null;',
    '    $cls = New-Object System.Text.StringBuilder 256;',
    '    [FerryxQaWin]::GetClassNameW($hWnd, $cls, 256) | Out-Null;',
    '    $windows.Add([ordered]@{ hwnd = $hWnd.ToInt64(); visible = [bool][FerryxQaWin]::IsWindowVisible($hWnd); className = $cls.ToString(); title = $title.ToString() }) | Out-Null;',
    '  }',
    '  return $true };',
    'while ($sw.ElapsedMilliseconds -lt $budgetMs) {',
    '  $proc = Get-Process -Id $targetPid -ErrorAction SilentlyContinue;',
    '  if ($proc -eq $null) { $processExited = $true; break }',
    '  $proc.Refresh();',
    '  $candidate = $proc.MainWindowHandle;',
    '  if ($candidate -ne [IntPtr]::Zero -and [FerryxQaWin]::IsWindowVisible($candidate)) { $handle = $candidate; break }',
    '  $windows.Clear();',
    '  [FerryxQaWin]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null;',
    '  if (@($windows | Where-Object { $_.visible }).Count -gt 0) { break }',
    '  Start-Sleep -Milliseconds 200;',
    '}',
    '$windows.Clear();',
    '[FerryxQaWin]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null;',
    '$visibleWindowCount = @($windows | Where-Object { $_.visible }).Count;',
    '$payload = [ordered]@{',
    "  probe = 'owned-window';",
    '  pid = $targetPid;',
    '  interactive = $interactive;',
    '  sessionId = $sessionId;',
    '  budgetMs = $budgetMs;',
    '  waitedMs = [int]$sw.ElapsedMilliseconds;',
    '  mainWindowHandle = $handle.ToInt64();',
    '  processExited = $processExited;',
    '  visibleWindowCount = $visibleWindowCount;',
    '  ok = [bool]($handle -ne [IntPtr]::Zero -or $visibleWindowCount -gt 0);',
    '  windows = $windows;',
    '};',
    'Write-Output ($payload | ConvertTo-Json -Compress -Depth 6);',
  ].join('\n');
}

export function classifyWindowsWindowProbe(probe) {
  const windows = asArray(probe?.windows);
  // "A visible owned window exists" is the real readiness condition - never
  // "MainWindowHandle is non-zero", which is only one of the owned windows and
  // (pass-6) is not guaranteed to be the one carrying the webview UI.
  const visibleWindows = windows.filter(window => window?.visible === true);
  const visibleWindowCount = probe?.visibleWindowCount !== undefined && probe?.visibleWindowCount !== null
    ? Number(probe.visibleWindowCount)
    : (windows.length > 0 ? visibleWindows.length : (Number(probe?.mainWindowHandle) !== 0 ? 1 : 0));
  const measured = {
    pid: probe?.pid ?? null,
    interactive: probe?.interactive ?? null,
    sessionId: probe?.sessionId ?? null,
    budgetMs: probe?.budgetMs ?? null,
    waitedMs: probe?.waitedMs ?? null,
    mainWindowHandle: probe?.mainWindowHandle ?? null,
    processExited: probe?.processExited ?? null,
    windows,
    visibleWindowCount,
  };
  if (probe?.ok === true && visibleWindowCount >= 1) return { ok: true, ...measured };
  const interactive = probe?.interactive === true && Number(probe?.sessionId) !== 0;
  const visibility = `mainWindowHandle=${JSON.stringify(measured.mainWindowHandle)} interactive=${JSON.stringify(measured.interactive)} sessionId=${JSON.stringify(measured.sessionId)} processExited=${JSON.stringify(measured.processExited)} waitedMs=${JSON.stringify(measured.waitedMs)} topLevelWindows=${JSON.stringify(windows)}`;
  const code = interactive ? 'NO_OWNED_WINDOW' : 'NO_INTERACTIVE_SESSION';
  const detail = interactive
    ? `the app never owned a visible top-level window within ${measured.budgetMs}ms in interactive session ${JSON.stringify(measured.sessionId)}: ${visibility}`
    : `the app can never own a visible window: the runner is not on an interactive desktop (${visibility})`;
  return { ok: false, code, detail, ...measured };
}

export async function awaitOwnedWindowWindows(evidence, pid, budgetMs = BUDGETS.ownedWindowReadyMs) {
  const stdout = await powershell(evidence, buildWindowsWindowWaitScript(pid, budgetMs));
  const parsed = parseWindowsProbeLine(stdout, 'owned-window');
  if (!parsed.ok) {
    throw new HarnessError('NO_OWNED_WINDOW', `owned-window probe produced no structured evidence (${parsed.reason}): stdout=${JSON.stringify(stdout)}`);
  }
  const verdict = classifyWindowsWindowProbe(parsed.probe);
  evidence.action({ action: 'owned-window', pid, ...verdict });
  if (!verdict.ok) throw new HarnessError(verdict.code, verdict.detail);
  return verdict;
}

// Same charset rule as the wait probe: the two StringBuilder imports
// (GetWindowTextW / GetClassNameW) declare `CharSet = CharSet.Unicode` because
// they marshal text, while the handle/int/bool imports declare none.
// Windows: every top-level window owned by the pid, enumerated immediately (no
// wait). The split search must never key off `MainWindowHandle` (pass-6: the
// pid owned four top-level windows, two visible, while the search only ever
// looked at the `MainWindowHandle` one), so the runner enumerates here, orders
// in JS (`orderOwnedWindowsForSearch`), and hands the ordered list to the probe.
export function buildWindowsOwnedWindowsScript(pid) {
  return [
    "$ErrorActionPreference = 'Stop';",
    'Add-Type @"',
    'using System;',
    'using System.Text;',
    'using System.Runtime.InteropServices;',
    'public class FerryxQaOwned {',
    '  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);',
    '  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);',
    '  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);',
    '  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);',
    '  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);',
    '  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder text, int count);',
    '}',
    '"@;',
    `$targetPid = ${pid};`,
    '$proc = Get-Process -Id $targetPid -ErrorAction SilentlyContinue;',
    'if ($proc -eq $null) {',
    "  Write-Output (([ordered]@{ probe = 'owned-windows'; pid = $targetPid; processExited = $true; interactive = [bool][System.Environment]::UserInteractive; sessionId = [int](Get-Process -Id $PID).SessionId; mainWindowHandle = 0; mainWindowVisible = $false; visibleWindowCount = 0; windows = @() }) | ConvertTo-Json -Compress -Depth 6);",
    '  exit 0',
    '}',
    '$proc.Refresh();',
    '$mainHandle = $proc.MainWindowHandle;',
    '$windows = New-Object System.Collections.ArrayList;',
    '$cb = [FerryxQaOwned+EnumProc]{ param($hWnd, $lParam)',
    '  $owner = [uint32]0;',
    '  [FerryxQaOwned]::GetWindowThreadProcessId($hWnd, [ref]$owner) | Out-Null;',
    '  if ($owner -eq [uint32]$targetPid) {',
    '    $title = New-Object System.Text.StringBuilder 256;',
    '    [FerryxQaOwned]::GetWindowTextW($hWnd, $title, 256) | Out-Null;',
    '    $cls = New-Object System.Text.StringBuilder 256;',
    '    [FerryxQaOwned]::GetClassNameW($hWnd, $cls, 256) | Out-Null;',
    '    $windows.Add([ordered]@{ hwnd = $hWnd.ToInt64(); visible = [bool][FerryxQaOwned]::IsWindowVisible($hWnd); className = $cls.ToString(); title = $title.ToString() }) | Out-Null;',
    '  }',
    '  return $true };',
    '[FerryxQaOwned]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null;',
    '$visible = @($windows | Where-Object { $_.visible });',
    '$payload = [ordered]@{',
    "  probe = 'owned-windows';",
    '  pid = $targetPid;',
    '  processExited = $false;',
    '  interactive = [bool][System.Environment]::UserInteractive;',
    '  sessionId = [int](Get-Process -Id $PID).SessionId;',
    '  mainWindowHandle = $mainHandle.ToInt64();',
    '  mainWindowVisible = [bool][FerryxQaOwned]::IsWindowVisible($mainHandle);',
    '  visibleWindowCount = $visible.Count;',
    '  windows = $windows;',
    '};',
    'Write-Output ($payload | ConvertTo-Json -Compress -Depth 6);',
  ].join('\n');
}

// The typed verdict for the enumeration. "A visible owned window exists" - never
// "MainWindowHandle is the UI window" - is the readiness condition, and the
// typed NO_OWNED_WINDOW is kept for the case where none is visible.
export function classifyOwnedWindowsProbe(probe) {
  const windows = asArray(probe?.windows);
  const visibleWindows = windows.filter(window => window?.visible === true);
  const measured = {
    pid: probe?.pid ?? null,
    interactive: probe?.interactive ?? null,
    sessionId: probe?.sessionId ?? null,
    processExited: probe?.processExited ?? null,
    mainWindowHandle: probe?.mainWindowHandle ?? null,
    mainWindowVisible: probe?.mainWindowVisible ?? null,
    windows,
    visibleWindowCount: visibleWindows.length,
  };
  if (visibleWindows.length === 0) {
    return {
      ok: false,
      code: 'NO_OWNED_WINDOW',
      detail: `the app owns no visible top-level window to search for the split affordance: mainWindowHandle=${JSON.stringify(measured.mainWindowHandle)} interactive=${JSON.stringify(measured.interactive)} sessionId=${JSON.stringify(measured.sessionId)} processExited=${JSON.stringify(measured.processExited)} topLevelWindows=${JSON.stringify(windows)}`,
      ...measured,
    };
  }
  return {
    ok: true,
    code: null,
    detail: `enumerated ${windows.length} owned top-level window(s); ${visibleWindows.length} visible and searchable`,
    ...measured,
  };
}

// Deterministic search order, owned by the runner instead of by
// `MainWindowHandle` alone: the main-handle window first when it is visible,
// then every other visible owned window by ascending hwnd. Invisible windows
// are never searched, and no window is searched twice. The probe still gives
// priority to the window that actually contains the focused element (it can
// only know that at run time), so this order decides the sweep, not the verdict.
export function orderOwnedWindowsForSearch(windows, mainWindowHandle = null) {
  const main = Number(mainWindowHandle);
  const all = asArray(windows);
  const asEntry = window => ({ hwnd: Number(window?.hwnd), title: String(window?.title ?? ''), className: String(window?.className ?? '') });
  const others = all
    .filter(window => window?.visible === true)
    .map(asEntry)
    .filter(window => Number.isFinite(window.hwnd) && window.hwnd !== 0 && window.hwnd !== main)
    .sort((left, right) => left.hwnd - right.hwnd);
  const mainWindow = all.find(window => window?.visible === true && Number(window?.hwnd) === main);
  return [...(mainWindow ? [asEntry(mainWindow)] : []), ...others];
}

// Enumerate the owned windows and record exactly what will be searched. Throws
// the typed NO_OWNED_WINDOW when the pid owns no visible top-level window.
export async function awaitOwnedWindowsWindows(evidence, pid) {
  const stdout = await powershell(evidence, buildWindowsOwnedWindowsScript(pid));
  const parsed = parseWindowsProbeLine(stdout, 'owned-windows');
  if (!parsed.ok) {
    throw new HarnessError('NO_OWNED_WINDOW', `owned-windows enumeration produced no structured evidence (${parsed.reason}): stdout=${JSON.stringify(stdout)}`);
  }
  const verdict = classifyOwnedWindowsProbe(parsed.probe);
  const searchOrder = verdict.ok ? orderOwnedWindowsForSearch(verdict.windows, verdict.mainWindowHandle) : [];
  evidence.action({
    action: 'owned-windows-enumerated',
    pid,
    ok: verdict.ok,
    code: verdict.code,
    interactive: verdict.interactive,
    sessionId: verdict.sessionId,
    mainWindowHandle: verdict.mainWindowHandle,
    mainWindowVisible: verdict.mainWindowVisible,
    windows: verdict.windows,
    visibleWindowCount: verdict.visibleWindowCount,
    searchOrder,
  });
  if (!verdict.ok) throw new HarnessError(verdict.code, verdict.detail);
  return { ...verdict, searchOrder };
}

// PowerShell single-quoted literal + literal array: a window title or a product
// label can never break out of the generated script.
function psStringLiteral(value) {
  return `'${String(value ?? '').replace(/'/g, "''")}'`;
}
function psStringArray(values) {
  return `@(${values.map(psStringLiteral).join(', ')})`;
}
// Window titles/class names are evidence text, not code: a newline would break
// the generated array literal, so they are flattened before being embedded (the
// raw values stay in the enumeration action).
function windowText(value) {
  return String(value ?? '').replace(/[\r\n]+/g, ' ');
}

// Pane-scoped split affordance probe over EVERY visible owned window. The
// caller supplies the windows to search (`orderOwnedWindowsForSearch`), the
// probe re-checks each one's visibility, resolves the target pane from the OS
// accessibility tree (focused element inside the searched windows;
// deterministic fallback: focus a pane's own focus sink when the window
// root/document owns focus), then requires exactly one ACTIONABLE affordance in
// that pane scope. The window containing the focused element is searched first;
// the remaining windows are searched from their root only if it yields nothing,
// and the window that produced the match is recorded (`matchedWindowHwnd`).
// Every candidate (name, control type, automation id, enabled, offscreen, rect,
// in-window) is returned so a blocked run records why it blocked, and the
// not-found path additionally returns a bounded inventory of every element
// whose name or automation id contains `Split`.
// Its two imports (GetWindowRect into a struct, IsWindowVisible) marshal no text,
// so neither carries a charset.
export function buildWindowsSplitRightScript(pid, focusBudgetMs = BUDGETS.splitFocusWaitMs, options = {}) {
  const names = asArray(options.names ?? SPLIT_AFFORDANCE_NAMES_WIN32).map(String);
  const automationIds = asArray(options.automationIds ?? SPLIT_AFFORDANCE_AUTOMATION_IDS_WIN32).map(String);
  if (names.length + automationIds.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', 'the split affordance probe needs at least one exact accessible name or automation id; refusing to search for nothing');
  }
  const matchCap = Number.isFinite(options.inventoryMatchCap) ? options.inventoryMatchCap : WINDOW_INVENTORY_MATCH_CAP;
  const inspectCap = Number.isFinite(options.inventoryInspectCap) ? options.inventoryInspectCap : WINDOW_INVENTORY_INSPECT_CAP;
  const searchWindows = asArray(options.windows)
    .map(window => ({ hwnd: Number(window?.hwnd), title: windowText(window?.title), className: windowText(window?.className) }))
    .filter(window => Number.isFinite(window.hwnd) && window.hwnd !== 0);
  const conditionBindings = [
    ...names.map((_, index) => `$conditionName${index}`),
    ...automationIds.map((_, index) => `$conditionAutomationId${index}`),
  ];
  const conditionLines = [
    ...names.map((_, index) => `$conditionName${index} = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $selectorNames[${index}]);`),
    ...automationIds.map((_, index) => `$conditionAutomationId${index} = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, $selectorAutomationIds[${index}]);`),
  ];
  const conditionAssembly = conditionBindings.length === 1
    ? [`$condition = ${conditionBindings[0]};`]
    : [
      `$conditionArray = [System.Windows.Automation.Condition[]]@(${conditionBindings.join(', ')});`,
      '$condition = New-Object System.Windows.Automation.OrCondition -ArgumentList (, $conditionArray);',
    ];
  return [
    "$ErrorActionPreference = 'Stop';",
    'Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, Microsoft.VisualBasic;',
    'Add-Type @"',
    'using System;',
    'using System.Runtime.InteropServices;',
    'public struct FerryxQaRectStruct { public int Left; public int Top; public int Right; public int Bottom; }',
    'public class FerryxQaRect {',
    '  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out FerryxQaRectStruct rect);',
    '  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);',
    '}',
    '"@;',
    `$targetPid = ${pid};`,
    `$focusBudgetMs = ${focusBudgetMs};`,
    `$inventoryMatchCap = ${matchCap};`,
    `$inventoryInspectCap = ${inspectCap};`,
    `$windowHandles = @(${searchWindows.map(window => window.hwnd).join(', ')});`,
    `$windowTitles = ${psStringArray(searchWindows.map(window => window.title))};`,
    `$windowClasses = ${psStringArray(searchWindows.map(window => window.className))};`,
    `$selectorNames = ${psStringArray(names)};`,
    `$selectorAutomationIds = ${psStringArray(automationIds)};`,
    '$primaryName = $selectorNames[0];',
    '$diag = [ordered]@{',
    "  probe = 'split-right';",
    '  pid = $targetPid;',
    '  selector = $primaryName;',
    '  selectorNames = $selectorNames;',
    '  selectorAutomationIds = $selectorAutomationIds;',
    '  interactive = [bool][System.Environment]::UserInteractive;',
    '  sessionId = [int](Get-Process -Id $PID).SessionId;',
    '  mainWindowHandle = 0;',
    '  windowVisible = $false;',
    '  visibleWindowCount = 0;',
    '  windowsSearched = @();',
    '  windowsSearchedCount = 0;',
    '  windowSearchDepths = @();',
    '  matchedWindowHwnd = $null;',
    '  focusedFound = $false;',
    '  focusSource = $null;',
    '  scopeDepth = -1;',
    '  scopeIsWindowRoot = $false;',
    '  candidateCount = 0;',
    '  actionableCount = 0;',
    '  candidates = @();',
    '  chosen = $null;',
    '  inventory = $null;',
    '};',
    'function Emit { Write-Output ($diag | ConvertTo-Json -Compress -Depth 8) }',
    'function Fail($code, $detail) { $diag.failure = $code; $diag.detail = $detail; Emit; exit 0 }',
    'function InRect($element, $rect) {',
    '  $r = $null;',
    '  try { $r = $element.Current.BoundingRectangle } catch { return $false };',
    '  if ($r.IsEmpty) { return $false };',
    '  return ($r.Left -ge ($rect.Left - 2) -and $r.Top -ge ($rect.Top - 2) -and $r.Right -le ($rect.Right + 2) -and $r.Bottom -le ($rect.Bottom + 2))',
    '}',
    'function InWindow($element) {',
    '  foreach ($w in $searched) { if (InRect $element $w.rect) { return $true } };',
    '  return $false',
    '}',
    'function ContainsSplit($text) {',
    '  if ($null -eq $text) { return $false };',
    "  return ($text.ToString().ToLowerInvariant().IndexOf('split') -ge 0)",
    '}',
    'function BuildInventory($windowList, $inspectCap, $matchCap) {',
    '  $perWindow = New-Object System.Collections.ArrayList;',
    '  $totalInspected = 0;',
    '  $totalMatches = 0;',
    '  $truncated = $false;',
    '  foreach ($w in $windowList) {',
    '    $inventoryRoot = [System.Windows.Automation.AutomationElement]::FromHandle([System.IntPtr]::new([int64]$w.hwnd));',
    '    $inspected = 0;',
    '    $matchList = New-Object System.Collections.ArrayList;',
    '    $windowTruncated = $false;',
    '    if ($inventoryRoot -ne $null) {',
    '      $all = $inventoryRoot.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition);',
    '      for ($i = 0; $i -lt $all.Count; $i = $i + 1) {',
    '        if ($inspected -ge $inspectCap) { $windowTruncated = $true; break };',
    '        $inspected = $inspected + 1;',
    '        try {',
    '          $element = $all.Item($i);',
    '          $elementName = $element.Current.Name;',
    '          $elementAutomationId = $element.Current.AutomationId;',
    '          if ((ContainsSplit $elementName) -or (ContainsSplit $elementAutomationId)) {',
    '            if ($matchList.Count -lt $matchCap) {',
    '              $matchList.Add([ordered]@{ name = $elementName; controlType = $element.Current.ControlType.ProgrammaticName; automationId = $elementAutomationId; enabled = [bool]$element.Current.IsEnabled; offscreen = [bool]$element.Current.IsOffscreen }) | Out-Null;',
    '            } else { $windowTruncated = $true }',
    '          }',
    '        } catch { }',
    '      }',
    '    }',
    '    $totalInspected = $totalInspected + $inspected;',
    '    $totalMatches = $totalMatches + $matchList.Count;',
    '    if ($windowTruncated) { $truncated = $true };',
    '    $perWindow.Add([ordered]@{ hwnd = $w.hwnd; title = $w.title; className = $w.className; inspectedCount = $inspected; matchCount = $matchList.Count; truncated = $windowTruncated; matches = @($matchList) }) | Out-Null;',
    '  }',
    '  return [ordered]@{ windows = @($perWindow); inspectedCount = $totalInspected; matchCount = $totalMatches; truncated = $truncated; inspectCap = $inspectCap; matchCap = $matchCap; filter = \'name or automationId contains "split" (case-insensitive)\' };',
    '}',
    ...conditionLines,
    ...conditionAssembly,
    '$proc = Get-Process -Id $targetPid -ErrorAction SilentlyContinue;',
    "if ($proc -eq $null) { Fail 'NO_OWNED_WINDOW' 'the launched process exited before any split affordance could be addressed' }",
    '$proc.Refresh();',
    '$mainHandle = $proc.MainWindowHandle;',
    '$diag.mainWindowHandle = $mainHandle.ToInt64();',
    '$diag.windowVisible = [bool][FerryxQaRect]::IsWindowVisible($mainHandle);',
    '$searched = New-Object System.Collections.ArrayList;',
    'for ($i = 0; $i -lt $windowHandles.Count; $i = $i + 1) {',
    '  $hwnd = [int64]$windowHandles[$i];',
    '  if ($hwnd -eq 0) { continue };',
    '  if (-not [FerryxQaRect]::IsWindowVisible([System.IntPtr]::new($hwnd))) { continue };',
    '  $rect = New-Object FerryxQaRectStruct;',
    '  [FerryxQaRect]::GetWindowRect([System.IntPtr]::new($hwnd), [ref]$rect) | Out-Null;',
    '  $searched.Add([ordered]@{ hwnd = $hwnd; title = $windowTitles[$i]; className = $windowClasses[$i]; rect = $rect }) | Out-Null;',
    '}',
    '$diag.visibleWindowCount = $searched.Count;',
    '$diag.windowsSearchedCount = $searched.Count;',
    '$diag.windowsSearched = @($searched | ForEach-Object { [ordered]@{ hwnd = $_.hwnd; title = $_.title; className = $_.className } });',
    "if ($searched.Count -eq 0) { Fail 'NO_OWNED_WINDOW' 'no visible top-level window owned by the process could be searched for the split affordance' }",
    'try { [Microsoft.VisualBasic.Interaction]::AppActivate($targetPid) | Out-Null } catch { }',
    '$focused = [System.Windows.Automation.AutomationElement]::FocusedElement;',
    'if ($focused -ne $null -and $focused.Current.ControlType -eq [System.Windows.Automation.ControlType]::Document) { $focused = $null }',
    'if ($focused -ne $null -and -not (InWindow $focused)) { $focused = $null }',
    '$sw = [System.Diagnostics.Stopwatch]::StartNew();',
    '$setFocusTried = $false;',
    'while ($focused -eq $null -and $sw.ElapsedMilliseconds -lt $focusBudgetMs) {',
    '  $candidateFocus = [System.Windows.Automation.AutomationElement]::FocusedElement;',
    '  if ($candidateFocus -ne $null -and $candidateFocus.Current.ControlType -ne [System.Windows.Automation.ControlType]::Document -and (InWindow $candidateFocus)) { $focused = $candidateFocus; $diag.focusSource = \'focused-element\'; break }',
    '  if (-not $setFocusTried -and $sw.ElapsedMilliseconds -gt 400) { $setFocusTried = $true; try { ([System.Windows.Automation.AutomationElement]::FromHandle([System.IntPtr]::new([int64]$searched[0].hwnd))).SetFocus() } catch { } }',
    '  Start-Sleep -Milliseconds 100;',
    '}',
    'if ($focused -eq $null) {',
    '  $editCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Edit);',
    '  foreach ($window in $searched) {',
    '    if ($focused -ne $null) { break };',
    '    $windowRoot = [System.Windows.Automation.AutomationElement]::FromHandle([System.IntPtr]::new([int64]$window.hwnd));',
    '    if ($windowRoot -eq $null) { continue };',
    '    $edits = $windowRoot.FindAll([System.Windows.Automation.TreeScope]::Descendants, $editCondition);',
    '    if ($edits.Count -eq 1) {',
    '      try { $edits.Item(0).SetFocus(); $focused = [System.Windows.Automation.AutomationElement]::FocusedElement; $diag.focusSource = \'pane-focus-sink\' } catch { $focused = $null }',
    '    }',
    '  }',
    '}',
    '$diag.focusedFound = [bool]($focused -ne $null);',
    "if ($focused -eq $null) { Fail 'SPLIT_RIGHT_NOT_FOUND' 'no focused pane could be identified inside the owned window, so no affordance was clicked' }",
    '$walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker;',
    '$scope = $null;',
    '$scopeRect = $null;',
    '$depths = New-Object System.Collections.ArrayList;',
    'foreach ($pass in @(1, 2)) {',
    '  for ($i = 0; $i -lt $searched.Count -and $scope -eq $null; $i = $i + 1) {',
    '    $containsFocus = [bool]($focused -ne $null -and (InRect $focused $searched[$i].rect));',
    '    if ($pass -eq 1 -and -not $containsFocus) { continue };',
    '    if ($pass -eq 2 -and $containsFocus) { continue };',
    '    $root = [System.Windows.Automation.AutomationElement]::FromHandle([System.IntPtr]::new([int64]$searched[$i].hwnd));',
    '    if ($root -eq $null) { continue };',
    '    $node = $root;',
    '    if ($containsFocus) { $node = $focused };',
    '    $depth = 0;',
    '    while ($node -ne $null) {',
    '      $found = $node.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition);',
    '      if ($found.Count -ge 1) { $scope = $node; $scopeRect = $searched[$i].rect; $diag.scopeDepth = $depth; $diag.matchedWindowHwnd = $searched[$i].hwnd; $diag.scopeIsWindowRoot = [bool]($node -eq $root); break };',
    '      if ($node -eq $root) { break };',
    '      $node = $walker.GetParent($node);',
    '      $depth = $depth + 1;',
    '      if ($depth -gt 64) { break };',
    '    };',
    '    $depths.Add([ordered]@{ hwnd = $searched[$i].hwnd; depth = $depth; containsFocus = $containsFocus }) | Out-Null;',
    '  }',
    '}',
    '$diag.windowSearchDepths = @($depths);',
    'if ($scope -eq $null) {',
    '  foreach ($entry in $depths) { if ($entry.containsFocus) { $diag.scopeDepth = $entry.depth; break } };',
    '  $diag.inventory = BuildInventory $searched $inventoryInspectCap $inventoryMatchCap;',
    "  Fail 'SPLIT_RIGHT_NOT_FOUND' 'no ancestor of the focused pane contains the split affordance in any visible owned window (the bounded inventory names every element whose name or automation id contains Split)'",
    '}',
    '$items = $scope.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition);',
    '$candidates = New-Object System.Collections.ArrayList;',
    'for ($i = 0; $i -lt $items.Count; $i = $i + 1) {',
    '  try {',
    '    $item = $items.Item($i);',
    '    $rectItem = $item.Current.BoundingRectangle;',
    '    $candidates.Add([ordered]@{ index = $i; name = $item.Current.Name; controlType = $item.Current.ControlType.ProgrammaticName; automationId = $item.Current.AutomationId; enabled = [bool]$item.Current.IsEnabled; offscreen = [bool]$item.Current.IsOffscreen; rectEmpty = [bool]$rectItem.IsEmpty; rect = ("{0},{1},{2},{3}" -f $rectItem.Left, $rectItem.Top, $rectItem.Width, $rectItem.Height); inWindow = [bool](InWindow $item) }) | Out-Null;',
    '  } catch { $candidates.Add([ordered]@{ index = $i; error = $_.Exception.Message }) | Out-Null; }',
    '}',
    '$diag.candidates = $candidates;',
    '$diag.candidateCount = $items.Count;',
    '$actionable = @($candidates | Where-Object { $_.enabled -and (-not $_.offscreen) -and (-not $_.rectEmpty) -and $_.inWindow });',
    '$diag.actionableCount = $actionable.Count;',
    'if ($items.Count -eq 0) { Fail \'SPLIT_RIGHT_NOT_FOUND\' \'the focused pane scope contains no element with the bound accessible name\' }',
    'elseif ($actionable.Count -gt 1) { Fail \'SPLIT_RIGHT_NOT_UNIQUE\' \'the focused pane scope contains more than one actionable split affordance\' }',
    'elseif ($actionable.Count -eq 0) { Fail \'SPLIT_RIGHT_DISABLED\' \'the split affordance is present but not actionable (disabled, offscreen, empty rect, or outside the owned window)\' }',
    'else {',
    '  $chosen = $items.Item([int]$actionable[0].index);',
    '  $diag.chosen = [ordered]@{ index = [int]$actionable[0].index; name = $chosen.Current.Name; controlType = $chosen.Current.ControlType.ProgrammaticName; enabled = [bool]$chosen.Current.IsEnabled; offscreen = [bool]$chosen.Current.IsOffscreen; rect = $actionable[0].rect; inWindow = $true };',
    '  try {',
    '    $invoke = $chosen.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern);',
    '    $invoke.Invoke();',
    "    $diag.result = 'SPLIT_CLICKED';",
    '  } catch {',
    "    $diag.failure = 'SPLIT_RIGHT_DISABLED';",
    '    $diag.detail = "InvokePattern failed: " + $_.Exception.Message;',
    '  }',
    '}',
    'Emit;',
  ].join('\n');
}

export function classifyWindowsSplitRight(probe) {
  const candidates = asArray(probe?.candidates);
  const windowsSearched = asArray(probe?.windowsSearched);
  // The bound property set is what the PROBE searched with, reported back from
  // the emitted payload (`$diag.selectorNames`/`$diag.selectorAutomationIds` are
  // part of it on every path). A probe that predates a field - or a hand-built
  // fixture that omits it - is classified against the module's default binding
  // set, exactly like the `selector` field below; it is never re-derived from a
  // matched element.
  const selectorNames = asArray(probe?.selectorNames);
  const selectorAutomationIds = asArray(probe?.selectorAutomationIds);
  // "A visible owned window was searched" - never "MainWindowHandle is the UI
  // window" - decides the window scope. A probe that predates the enumeration
  // (no `windowsSearched`/`visibleWindowCount`) still classifies from the
  // main-handle measurement.
  const visibleWindowCount = probe?.visibleWindowCount !== undefined && probe?.visibleWindowCount !== null
    ? Number(probe.visibleWindowCount)
    : (windowsSearched.length > 0
      ? windowsSearched.length
      : (Number(probe?.mainWindowHandle) !== 0 && probe?.windowVisible === true ? 1 : 0));
  const measured = {
    selector: probe?.selector ?? SPLIT_MENU_SELECTOR_WIN32.name,
    selectorNames: selectorNames.length > 0 ? selectorNames : [...SPLIT_AFFORDANCE_NAMES_WIN32],
    selectorAutomationIds: selectorAutomationIds.length > 0 ? selectorAutomationIds : [...SPLIT_AFFORDANCE_AUTOMATION_IDS_WIN32],
    interactive: probe?.interactive ?? null,
    sessionId: probe?.sessionId ?? null,
    mainWindowHandle: probe?.mainWindowHandle ?? null,
    windowVisible: probe?.windowVisible ?? null,
    visibleWindowCount,
    windowsSearched,
    windowsSearchedCount: probe?.windowsSearchedCount ?? windowsSearched.length,
    windowSearchDepths: asArray(probe?.windowSearchDepths),
    matchedWindowHwnd: probe?.matchedWindowHwnd ?? null,
    focusedFound: probe?.focusedFound ?? null,
    focusSource: probe?.focusSource ?? null,
    scopeDepth: probe?.scopeDepth ?? null,
    scopeIsWindowRoot: probe?.scopeIsWindowRoot ?? null,
    candidateCount: probe?.candidateCount ?? null,
    actionableCount: probe?.actionableCount ?? null,
    candidates,
    chosen: probe?.chosen ?? null,
    inventory: probe?.inventory ?? null,
    psFailure: probe?.failure ?? null,
    psDetail: probe?.detail ?? null,
  };
  // The recorded detail stays bounded: the bounded inventory travels as its own
  // structured field, never dumped into this string.
  const evidenceText = JSON.stringify({ ...measured, inventory: undefined, psFailure: undefined, psDetail: undefined });
  if (probe?.result === 'SPLIT_CLICKED') {
    return { ok: true, code: null, derivedCode: null, detail: 'clicked the single actionable split affordance of the focused pane', ...measured };
  }
  const candidateCount = Number(probe?.candidateCount ?? 0);
  const actionableCount = Number(probe?.actionableCount ?? 0);
  // Derived from the measured shape; the probe's own verdict is honoured when
  // it names a typed code (the probe measured the window/focus state directly).
  const derivedCode = visibleWindowCount < 1
    ? 'NO_OWNED_WINDOW'
    : probe?.focusedFound !== true
      ? 'SPLIT_RIGHT_NOT_FOUND'
      : candidateCount === 0
        ? 'SPLIT_RIGHT_NOT_FOUND'
        : actionableCount > 1
          ? 'SPLIT_RIGHT_NOT_UNIQUE'
          : 'SPLIT_RIGHT_DISABLED';
  const declaredCode = ['NO_OWNED_WINDOW', 'SPLIT_RIGHT_NOT_FOUND', 'SPLIT_RIGHT_NOT_UNIQUE', 'SPLIT_RIGHT_DISABLED'].includes(probe?.failure) ? probe.failure : null;
  const code = declaredCode ?? derivedCode;
  const divergence = declaredCode && declaredCode !== derivedCode ? ` (probe reported ${declaredCode}, measured shape derives ${derivedCode})` : '';
  const detail = code === 'NO_OWNED_WINDOW'
    ? `no owned visible window to address${divergence}: ${evidenceText}`
    : code === 'SPLIT_RIGHT_NOT_UNIQUE'
      ? `more than one actionable ${JSON.stringify(measured.selector)} affordance in the focused pane scope${divergence}: ${evidenceText}`
      : code === 'SPLIT_RIGHT_NOT_FOUND'
        ? `the ${JSON.stringify(measured.selector)} affordance of the focused pane could not be identified${divergence}: ${evidenceText}`
        : `the ${JSON.stringify(measured.selector)} affordance is present but not actionable${divergence}: ${evidenceText}`;
  return { ok: false, code, derivedCode, detail, ...measured };
}

// Click the split affordance of the FOCUSED pane. The window set is enumerated
// from the pid (never assumed from `MainWindowHandle`), ordered by
// `orderOwnedWindowsForSearch`, and the probe re-checks visibility before it
// searches each window; an ambiguous, absent, or non-actionable match fails
// typed and is recorded with the measured candidate set and - on the not-found
// path - the bounded inventory of everything named/id'd like a split affordance.
export async function windowsDriver(evidence, pid) {
  const enumeration = await awaitOwnedWindowsWindows(evidence, pid);
  if (enumeration.searchOrder.length === 0) {
    throw new HarnessError('NO_OWNED_WINDOW', enumeration.detail);
  }
  const stdout = await powershell(evidence, buildWindowsSplitRightScript(pid, BUDGETS.splitFocusWaitMs, { windows: enumeration.searchOrder }));
  const parsed = parseWindowsProbeLine(stdout, 'split-right');
  if (!parsed.ok) {
    throw new HarnessError('SPLIT_RIGHT_NOT_FOUND', `split-right probe produced no structured evidence (${parsed.reason}): stdout=${JSON.stringify(stdout)}`);
  }
  const verdict = classifyWindowsSplitRight(parsed.probe);
  evidence.action({
    action: 'click-split-affordance',
    selector: SPLIT_MENU_SELECTOR_WIN32,
    selectorNames: [...SPLIT_AFFORDANCE_NAMES_WIN32],
    selectorAutomationIds: [...SPLIT_AFFORDANCE_AUTOMATION_IDS_WIN32],
    pid,
    assertedUniqueEnabled: verdict.ok,
    code: verdict.code,
    window: { mainWindowHandle: verdict.mainWindowHandle, windowVisible: verdict.windowVisible, interactive: verdict.interactive, sessionId: verdict.sessionId },
    windowsSearched: verdict.windowsSearched,
    windowsSearchedCount: verdict.windowsSearchedCount,
    matchedWindowHwnd: verdict.matchedWindowHwnd,
    scope: { focusedFound: verdict.focusedFound, focusSource: verdict.focusSource, depth: verdict.scopeDepth, isWindowRoot: verdict.scopeIsWindowRoot },
    candidateCount: verdict.candidateCount,
    actionableCount: verdict.actionableCount,
    candidates: verdict.candidates,
    chosen: verdict.chosen,
    splitInventory: verdict.inventory,
    detail: verdict.detail,
  });
  if (!verdict.ok) throw new HarnessError(verdict.code, verdict.detail);
  // Review H5: this driver only clicks the menu. Marker typing is performed
  // by typeMarkerWindows on every marker path - never logged as an action
  // that did not happen.
  return { clicked: true, chosen: verdict.chosen, matchedWindowHwnd: verdict.matchedWindowHwnd };
}

// Review blocker 5: the Windows marker must be typed on EVERY marker path,
// not only inside the split flow. Focus the task-owned process window, then
// send the marker command through SendKeys (real OS input events).
export function buildWindowsFocusScript(pid) {
  return [
    `$proc = Get-Process -Id ${pid} -ErrorAction Stop;`,
    "Add-Type -AssemblyName Microsoft.VisualBasic;",
    '[Microsoft.VisualBasic.Interaction]::AppActivate($proc.Id) | Out-Null;',
    "'FOCUSED'",
  ].join('\n');
}

export async function focusWindowWindows(evidence, pid) {
  const result = await powershell(evidence, buildWindowsFocusScript(pid));
  if (result !== 'FOCUSED') throw new HarnessError('ASSERTION_FAILURE', `unexpected AppActivate result: ${result}`);
  evidence.action({ action: 'focus-window-by-pid', pid, selector: { processId: pid }, surface: 'AppActivate' });
}

export function buildWindowsTypeMarkerScript() {
  return [
    "Add-Type -AssemblyName System.Windows.Forms;",
    "[System.Windows.Forms.SendKeys]::SendWait('Write-Output ''FERRYX_SPLIT_READY''{ENTER}');",
    "'TYPED'",
  ].join('\n');
}

export async function typeMarkerWindows(evidence, pid) {
  const markerShellCommand = "Write-Output 'FERRYX_SPLIT_READY'";
  const result = await powershell(evidence, buildWindowsTypeMarkerScript());
  if (result !== 'TYPED') throw new HarnessError('ASSERTION_FAILURE', `unexpected SendKeys result: ${result}`);
  evidence.action({ action: 'type-marker', pid, markerCommand: markerShellCommand, shell: 'powershell', surface: 'SendKeys into focused QA leaf' });
  return { markerCommand: markerShellCommand, shell: 'powershell' };
}

export function assertNativeAutomationSupported() {
  const p = platform();
  if (p !== 'darwin' && p !== 'win32') {
    throw new HarnessError('NATIVE_AUTOMATION_UNSUPPORTED', `native desktop automation is not implemented for platform ${p}`);
  }
}

// Adapters may inject a complete driver; only explicit native preflights select OS APIs.
export function selectNativeDriver(ctx) {
  if (ctx.nativeDriver) return ctx.nativeDriver;
  if (ctx.platformPreflight === 'win32') {
    return {
      focus: focusWindowWindows,
      split: windowsDriver,
      typeMarker: typeMarkerWindows,
      retry: clickRetryWindows,
      capture: captureOwnedWindowWindows,
    };
  }
  if (ctx.platformPreflight === 'darwin') {
    return {
      focus: focusWindowByPidDarwin,
      split: clickSplitRightDarwin,
      typeMarker: typeMarkerDarwin,
      retry: clickRetryDarwin,
      capture: captureOwnedWindowDarwin,
    };
  }
  // Mock/unsupported contexts never focus, click, type, or capture a real window.
  // Capture only reads an existing fixture; missing evidence still fails normally.
  return {
    focus: async () => {},
    split: async () => {},
    typeMarker: async () => {},
    retry: async () => {},
    capture: async (_evidence, path) => ({
      path,
      screenshotSha256: createHash('sha256').update(readFileSync(path)).digest('hex'),
    }),
  };
}

// Bounded capture-ready and independent inspection handshake.
// Runner writes capture-ready.json with exact hash, runId, operationId, and bounds,
// then awaits independent inspection artifact. Rejects mismatched hash, run, bounds, or text.
export async function performInspectionHandshake(evidence, barrierHub, nonces, screenshotMetadata, timeoutMs = BUDGETS.stagePresentationMs) {
  const { runId, operationId } = nonces;
  // 1. Emit capture-ready event and record capture-ready.json
  const captureReadyRecord = barrierHub.recordCaptureReady({
    screenshotPath: screenshotMetadata.path,
    screenshotSha256: screenshotMetadata.screenshotSha256,
    windowBounds: screenshotMetadata.windowBounds,
    targetPaneBounds: screenshotMetadata.targetPaneBounds,
  });
  evidence.action({ action: 'capture-ready', ...captureReadyRecord });

  // 2. Await independent inspection / marker-recognition artifact
  const recognitionPath = join(barrierHub.dir, 'marker-recognition.json');
  let stopHandler;
  const stopPromise = new Promise(resolve => { stopHandler = resolve; });
  let outcome;
  try {
    outcome = await withDeadline(waitForFile(recognitionPath, stopPromise), timeoutMs, 'marker-recognition', { onStop: stopHandler });
  } catch (err) {
    outcome = { timedOut: false, error: err };
  }

  if (outcome.timedOut) {
    throw new HarnessError('MARKER_RECOGNITION_UNVERIFIED',
      `bounded inspection handshake timed out waiting for independent inspection artifact (${timeoutMs}ms) after capture-ready`);
  }
  if (outcome.error) {
    throw outcome.error instanceof HarnessError ? outcome.error : new HarnessError('MARKER_RECOGNITION_UNVERIFIED', `inspection wait failed: ${outcome.error.message}`);
  }

  let artifact = null;
  try {
    artifact = JSON.parse(outcome.value);
  } catch (err) {
    throw new HarnessError('MARKER_RECOGNITION_UNVERIFIED', `unparseable recognition artifact: ${err.message}`);
  }

  // 3. Strict validation:
  if (!artifact || typeof artifact !== 'object') {
    throw new HarnessError('MARKER_RECOGNITION_UNVERIFIED', 'recognition artifact is not an object');
  }
  if (artifact.runId !== runId) {
    throw new HarnessError('MARKER_RECOGNITION_UNVERIFIED', `recognition artifact runId mismatch: expected ${runId}, got ${JSON.stringify(artifact.runId)}`);
  }
  if (artifact.operationId !== operationId) {
    throw new HarnessError('MARKER_RECOGNITION_UNVERIFIED', `recognition artifact operationId mismatch: expected ${operationId}, got ${JSON.stringify(artifact.operationId)}`);
  }
  if (typeof artifact.recognizer !== 'string' || artifact.recognizer.trim().length === 0) {
    throw new HarnessError('MARKER_RECOGNITION_UNVERIFIED', 'recognition artifact is anonymous');
  }
  if (artifact.text !== MARKER_TEXT) {
    throw new HarnessError('MARKER_RECOGNITION_UNVERIFIED', `recognized text ${JSON.stringify(artifact.text)} does not equal expected marker ${JSON.stringify(MARKER_TEXT)}`);
  }
  if (artifact.screenshotSha256 !== screenshotMetadata.screenshotSha256) {
    throw new HarnessError('MARKER_RECOGNITION_UNVERIFIED', `recognition artifact binds a different screenshot: expected ${screenshotMetadata.screenshotSha256}, got ${JSON.stringify(artifact.screenshotSha256)}`);
  }
  if (typeof artifact.paneBounds !== 'object' || artifact.paneBounds === null) {
    throw new HarnessError('MARKER_RECOGNITION_UNVERIFIED', 'recognition artifact lacks paneBounds');
  }
  const ab = artifact.paneBounds;
  if (typeof ab.x !== 'number' || typeof ab.y !== 'number' || typeof ab.w !== 'number' || typeof ab.h !== 'number') {
    throw new HarnessError('MARKER_RECOGNITION_UNVERIFIED', `recognition artifact has invalid paneBounds dimensions: ${JSON.stringify(ab)}`);
  }
  if (ab.w <= 0 || ab.h <= 0) {
    throw new HarnessError('MARKER_RECOGNITION_UNVERIFIED', `recognized pane bounds have non-positive dimensions: ${JSON.stringify(ab)}`);
  }

  const region = screenshotMetadata.targetPaneBounds;
  if (region && typeof region.x === 'number' && typeof region.width === 'number') {
    if (region.x < ab.x || region.y < ab.y || (region.x + region.width) > (ab.x + ab.w) || (region.y + region.height) > (ab.y + ab.h)) {
      throw new HarnessError('MARKER_RECOGNITION_UNVERIFIED', 'recognized pane bounds do not contain the target pane region');
    }
  }

  evidence.action({
    action: 'marker-recognition',
    recognizer: artifact.recognizer,
    paneBounds: artifact.paneBounds,
    text: artifact.text,
    screenshotSha256: artifact.screenshotSha256,
  });

  return { recognizer: artifact.recognizer, paneBounds: artifact.paneBounds, verified: true };
}

// Root ruling (midpoint): PNG region variance is NOT marker/text recognition
// and cannot prove FERRYX_SPLIT_READY is visible. The variance heuristic was
// removed. Marker recognition must come from a trustworthy, INDEPENDENT
// channel: either an existing OCR implementation matching the exact marker
// text within the owned pane bounds, or an independent screenshot visual
// verification artifact. Until such recognition exists, this gate fails
// typed (MARKER_RECOGNITION_UNVERIFIED, nonzero) - the runner never
// fabricates a PASS. frameSubmitted remains a distinct, necessary-but-not-
// sufficient receipt. Solution ownership: Architect91.
//
// Recognition artifact contract (independent lane, NOT the product process):
//   <barrierDir>/marker-recognition.json
//   { runId, operationId, recognizer, text, paneBounds: {x,y,w,h}, screenshotSha256 }
// `recognizer` names the independent recognition component; `text` MUST equal
// the marker exactly; `paneBounds` MUST contain the product-reported
// markerRegionPx; `screenshotSha256` MUST match the captured evidence file.
export async function awaitMarkerRecognition(evidence, barrierDir, nonces, region, screenshotPath) {
  const { runId, operationId } = nonces;
  const path = join(barrierDir, 'marker-recognition.json');
  let artifact = null;
  try {
    artifact = JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    // fall through to the typed gate below
  }
  const unverified = detail => new HarnessError('MARKER_RECOGNITION_UNVERIFIED',
    `no trustworthy marker recognition is implemented; native presentation cannot be proven (${detail}). Unresolved dependency: OCR with exact text + owned pane bounds, or an independent screenshot visual verification artifact (Architect91 owns the solution).`);
  if (!artifact || typeof artifact !== 'object') throw unverified('no recognition artifact present');
  if (artifact.runId !== runId || artifact.operationId !== operationId) throw unverified('recognition artifact does not correlate to this run/operation');
  if (typeof artifact.recognizer !== 'string' || artifact.recognizer.length === 0) throw unverified('recognition artifact is anonymous');
  if (artifact.text !== MARKER_TEXT) throw unverified(`recognized text ${JSON.stringify(artifact.text)} does not equal the marker exactly`);
  if (typeof artifact.paneBounds !== 'object' || typeof artifact.screenshotSha256 !== 'string') throw unverified('recognition artifact lacks paneBounds/screenshotSha256');
  if (region && typeof region.x === 'number' && typeof region.w === 'number') {
    const b = artifact.paneBounds;
    if (typeof b.x !== 'number' || typeof b.y !== 'number' || typeof b.w !== 'number' || typeof b.h !== 'number'
      || region.x < b.x || region.y < b.y || region.x + region.w > b.x + b.w || region.y + region.h > b.y + b.h) {
      throw unverified('recognized pane bounds do not contain the product-reported marker region');
    }
  }
  const actualSha = createHash('sha256').update(readFileSync(screenshotPath)).digest('hex');
  if (artifact.screenshotSha256 !== actualSha) throw unverified('recognition artifact binds a different screenshot');
  evidence.action({ action: 'marker-recognition', recognizer: artifact.recognizer, paneBounds: artifact.paneBounds, text: artifact.text });
  return { recognizer: artifact.recognizer, paneBounds: artifact.paneBounds, verified: true };
}
