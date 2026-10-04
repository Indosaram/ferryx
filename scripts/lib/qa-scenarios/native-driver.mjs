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

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, writeFileSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir, platform } from 'node:os';
import { join } from 'node:path';
import { HarnessError, withDeadline, waitForFile, BUDGETS } from './common-harness.mjs';

export const MARKER_COMMAND_UNIX = "printf 'FERRYX_SPLIT_READY\\n'";
export const MARKER_TEXT = 'FERRYX_SPLIT_READY';
// Plan requirement: bind the observed unique selector in runner code before
// accepting a native action. The observed macOS/Windows accessible name is the
// real pane toolbar label `Split pane right`; a different name fails typed and
// is repaired narrowly, never guessed clicked.
export const SPLIT_MENU_SELECTOR_DARWIN = { role: 'button', title: 'Split pane right' };
export const SPLIT_MENU_SELECTOR_WIN32 = { automationId: null, name: 'Split pane right' };

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
      else rejectPromise(new HarnessError('NATIVE_AUTOMATION_UNSUPPORTED', `powershell exited ${code}: ${stderr.trim()}`));
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

// Windows: Click actual Retry button via UIA.
export async function clickRetryWindows(evidence, pid) {
  const command = [
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
  ].join(' ');
  const result = await powershell(evidence, command);
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
export async function captureOwnedWindowWindows(evidence, path, pid) {
  const command = [
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
  ].join(' ');
  const res = await powershell(evidence, command);
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

// Windows: System.Windows.Automation (UIA) is the real native API. Marker uses
// a PowerShell Write-Output shell line, recorded in the scenario manifest.
// UIA `Descendants` is already recursive, so it reaches the webview-rendered
// pane toolbar button by its real accessible name.
export async function windowsDriver(evidence, pid) {
  const command = [
    'Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes;',
    `$proc = Get-Process -Id ${pid} -ErrorAction Stop;`,
    '$root = [System.Windows.Automation.AutomationElement]::FromHandle($proc.MainWindowHandle);',
    'if (-not $root) { throw "NO_OWNED_WINDOW" }',
    '$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, "Split pane right");',
    '$items = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond);',
    'if ($items.Count -ne 1) { throw "SPLIT_RIGHT_NOT_UNIQUE" }',
    'if (-not $items.Item(0).Current.IsEnabled) { throw "SPLIT_RIGHT_DISABLED" }',
    '$invoke = $items.Item(0).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern);',
    '$invoke.Invoke();',
    '"SPLIT_CLICKED"',
  ].join(' ');
  const result = await powershell(evidence, command);
  if (result !== 'SPLIT_CLICKED') throw new HarnessError('ASSERTION_FAILURE', `unexpected UIA result: ${result}`);
  evidence.action({ action: 'click-split-affordance', selector: SPLIT_MENU_SELECTOR_WIN32, pid, assertedUniqueEnabled: true });
  // Review H5: this driver only clicks the menu. Marker typing is performed
  // by typeMarkerWindows on every marker path - never logged as an action
  // that did not happen.
  return { clicked: true };
}

// Review blocker 5: the Windows marker must be typed on EVERY marker path,
// not only inside the split flow. Focus the task-owned process window, then
// send the marker command through SendKeys (real OS input events).
export async function focusWindowWindows(evidence, pid) {
  const command = [
    `$proc = Get-Process -Id ${pid} -ErrorAction Stop;`,
    "Add-Type -AssemblyName Microsoft.VisualBasic;",
    '[Microsoft.VisualBasic.Interaction]::AppActivate($proc.Id) | Out-Null;',
    "'FOCUSED'",
  ].join(' ');
  const result = await powershell(evidence, command);
  if (result !== 'FOCUSED') throw new HarnessError('ASSERTION_FAILURE', `unexpected AppActivate result: ${result}`);
  evidence.action({ action: 'focus-window-by-pid', pid, selector: { processId: pid }, surface: 'AppActivate' });
}

export async function typeMarkerWindows(evidence, pid) {
  const markerShellCommand = "Write-Output 'FERRYX_SPLIT_READY'";
  const command = [
    "Add-Type -AssemblyName System.Windows.Forms;",
    "[System.Windows.Forms.SendKeys]::SendWait('Write-Output ''FERRYX_SPLIT_READY''{ENTER}');",
    "'TYPED'",
  ].join(' ');
  const result = await powershell(evidence, command);
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
