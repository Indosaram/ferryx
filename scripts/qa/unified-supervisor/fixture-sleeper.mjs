#!/usr/bin/env bun
// Task-owned process fixture for the unified-supervisor harness self-test.
// It prints one JSON ready line and then stays alive until it is signalled, so
// the runner can prove ownership, deadlines, escalation and descendant reaping.
//   --ignore-sigterm      survive SIGTERM (forces the runner to escalate to SIGKILL)
//   --grandchild          start one child in the same process group
//   --max-lifetime-ms=N   leak guard: exit 97 after N ms (default 300000)
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2);
const flags = new Set(args);
const lifetimeArg = args.find((arg) => arg.startsWith('--max-lifetime-ms='));
const lifetimeMs = Number(lifetimeArg?.slice('--max-lifetime-ms='.length) ?? 300_000);

process.stdout.on('error', () => { /* the reader may already be gone */ });
function emit(value) {
  try { process.stdout.write(`${JSON.stringify(value)}\n`); } catch { /* the reader may already be gone */ }
}

if (flags.has('--ignore-sigterm')) {
  process.on('SIGTERM', () => emit({ ignoredSignal: 'SIGTERM', pid: process.pid }));
}

let grandchildPid = null;
if (flags.has('--grandchild')) {
  const child = spawn(process.execPath, [fileURLToPath(import.meta.url), '--role=grandchild'], { stdio: 'ignore' });
  child.on('error', () => { /* reported as a null grandchildPid */ });
  grandchildPid = child.pid ?? null;
}

// Leak guard, not a synchronisation point: an orphaned fixture never outlives this bound.
setTimeout(() => process.exit(97), Number.isFinite(lifetimeMs) && lifetimeMs > 0 ? lifetimeMs : 300_000);

const plant = process.env.UNIFIED_SUPERVISOR_SELFTEST_PLANT_SLACK;
emit({
  ready: true,
  pid: process.pid,
  role: flags.has('--role=grandchild') ? 'grandchild' : 'sleeper',
  grandchildPid,
  diag: plant ? `token=${plant}` : null,
});
