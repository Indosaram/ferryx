// VERIFIER SCRATCH — accessibility-route experiment. Lives only in the verifier's
// staging tree; it is NOT part of the candidate. Runs inside the interactive
// Windows session and reuses the driver's OWN builders, so the measurement is
// the driver's measurement.
import { spawnSync } from 'node:child_process';
import { awaitOwnedWindowsWindows, buildWindowsSplitRightScript } from '../lib/qa-scenarios/native-driver.mjs';
import { parseWindowsProbeLine } from '../lib/qa-scenarios/windows-interactive.mjs';

const pid = Number(process.argv[2]);
const out = { pid, steps: [] };

function ps(script) {
  const r = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 });
  return { code: r.status, stdout: (r.stdout || '').trim(), stderr: (r.stderr || '').trim() };
}

const stub = { action: () => {} };

try {
  const enumeration = await awaitOwnedWindowsWindows(stub, pid);
  out.visibleWindowCount = enumeration.visibleWindowCount;
  out.mainWindowHandle = enumeration.mainWindowHandle;
  out.windows = enumeration.windows;
  out.searchOrder = enumeration.searchOrder;
  out.steps.push({ step: 'owned-windows-enumerated', ok: true, visibleWindowCount: enumeration.visibleWindowCount, searchOrder: enumeration.searchOrder });
} catch (error) {
  out.steps.push({ step: 'owned-windows-enumerated', ok: false, error: String(error && error.message ? error.message : error) });
  out.error = 'enumeration-failed';
  console.log(JSON.stringify(out));
  process.exit(0);
}

try {
  const script = buildWindowsSplitRightScript(pid, 4000, { windows: out.searchOrder });
  const res = ps(script);
  out.splitProbeExit = res.code;
  out.splitProbeStderr = res.stderr.slice(0, 800);
  const parsed = parseWindowsProbeLine(res.stdout, 'split-right');
  out.splitProbeParsed = parsed.ok;
  if (parsed.ok) {
    const p = parsed.probe;
    out.inventory = p.inventory ?? null;
    out.candidateCount = p.candidateCount ?? null;
    out.actionableCount = p.actionableCount ?? null;
    out.candidates = p.candidates ?? null;
    out.scopeDepth = p.scopeDepth ?? null;
    out.focusedFound = p.focusedFound ?? null;
    out.focusSource = p.focusSource ?? null;
    out.psFailure = p.failure ?? null;
    out.matchedWindowHwnd = p.matchedWindowHwnd ?? null;
    out.result = p.result ?? null;
    out.steps.push({ step: 'split-right-probe', ok: true, failure: p.failure ?? null, candidateCount: p.candidateCount ?? null, inventoryMatchCount: p.inventory ? p.inventory.matchCount : null });
  } else {
    out.splitProbeRawStdout = res.stdout.slice(0, 2000);
    out.steps.push({ step: 'split-right-probe', ok: false, reason: parsed.reason });
  }
} catch (error) {
  out.steps.push({ step: 'split-right-probe', ok: false, error: String(error && error.message ? error.message : error) });
}
console.log(JSON.stringify(out));
