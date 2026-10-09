#!/usr/bin/env node
// Task 3 native desktop automation for pane-liveness scenarios.
// Real OS events only: focus a task-owned Ferryx window by PID, assert exactly
// one enabled accessibility menu item `Split Right`, type the marker command,
// capture a screenshot. Any unsupported surface is an explicit typed failure;
// this driver can never fabricate a PASS.

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, writeFileSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir, platform } from 'node:os';
import { join } from 'node:path';
import { HarnessError } from './common-harness.mjs';

export const MARKER_COMMAND_UNIX = "printf 'FERRYX_SPLIT_READY\\n'";
export const MARKER_TEXT = 'FERRYX_SPLIT_READY';
// Plan requirement: bind the observed unique selector in runner code before
// accepting a menu action. The observed macOS menu label is `Split Right`.
export const SPLIT_MENU_SELECTOR_DARWIN = { role: 'menu item', title: 'Split Right' };
export const SPLIT_MENU_SELECTOR_WIN32 = { automationId: null, name: 'Split Right' };

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
        } else if (/SPLIT_RIGHT_NOT_UNIQUE|SPLIT_RIGHT_DISABLED|PID_NOT_UNIQUE|NO_OWNED_WINDOW|Can.t get|Invalid index/i.test(text)) {
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
export async function assertScreenCapture(evidence) {
  const probe = join(mkdtempSync(join(tmpdir(), 'ferryx-qa-capture-')), 'probe.png');
  const child = spawn('screencapture', ['-x', probe]);
  const code = await new Promise(resolvePromise => {
    child.once('error', () => resolvePromise(-1));
    child.once('exit', c => resolvePromise(c));
  });
  evidence.action({ action: 'screencapture-probe', exitCode: code, path: probe });
  const ok = code === 0 && existsSync(probe);
  if (!ok) throw new HarnessError('CAPTURE_DENIED', 'screencapture probe failed or produced no image');
  return { captureGranted: true };
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

// Assert exactly one matching, enabled menu item named `Split Right`, then
// click it. A differing label/API fails explicitly.
export async function clickSplitRightDarwin(evidence, pid) {
  const selector = `menu item "Split Right" of menu 1 of menu bar item "Shell" of menu bar 1 of (first process whose unix id is ${pid})`;
  const source = [
    'tell application "System Events"',
    `  set matches to every menu item of menu 1 of menu bar item "Shell" of menu bar 1 of (first process whose unix id is ${pid}) whose name is "Split Right"`,
    '  if (count of matches) is not 1 then error "SPLIT_RIGHT_NOT_UNIQUE"',
    `  set theItem to ${selector}`,
    '  if enabled of theItem is not true then error "SPLIT_RIGHT_DISABLED"',
    '  click theItem',
    'end tell',
  ].join('\n');
  try {
    await osascript(evidence, source);
  } catch (error) {
    const message = error.message ?? '';
    if (message.includes('SPLIT_RIGHT_NOT_UNIQUE') || message.includes("Can't get") || message.includes('Invalid index')) {
      throw new HarnessError('ASSERTION_FAILURE', `actual menu label/API differs from bound selector: ${message}`);
    }
    throw error;
  }
  evidence.action({ action: 'click-menu-item', selector: SPLIT_MENU_SELECTOR_DARWIN, pid, assertedUniqueEnabled: true });
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
export async function windowsDriver(evidence, pid) {
  const command = [
    'Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes;',
    `$proc = Get-Process -Id ${pid} -ErrorAction Stop;`,
    '$root = [System.Windows.Automation.AutomationElement]::FromHandle($proc.MainWindowHandle);',
    'if (-not $root) { throw "NO_OWNED_WINDOW" }',
    '$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, "Split Right");',
    '$items = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond);',
    'if ($items.Count -ne 1) { throw "SPLIT_RIGHT_NOT_UNIQUE" }',
    'if (-not $items.Item(0).Current.IsEnabled) { throw "SPLIT_RIGHT_DISABLED" }',
    '$invoke = $items.Item(0).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern);',
    '$invoke.Invoke();',
    '"SPLIT_CLICKED"',
  ].join(' ');
  const result = await powershell(evidence, command);
  if (result !== 'SPLIT_CLICKED') throw new HarnessError('ASSERTION_FAILURE', `unexpected UIA result: ${result}`);
  evidence.action({ action: 'click-menu-item', selector: SPLIT_MENU_SELECTOR_WIN32, pid, assertedUniqueEnabled: true });
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
