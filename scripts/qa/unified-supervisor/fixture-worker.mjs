#!/usr/bin/env bun
// Task-owned fixture worker for the `admission` scenario (POSIX only). The isolated daemon
// starts it as the pane's login shell (SHELL points at a runner-written /bin/sh wrapper that
// execs this file), so it is the worker the supervisor dispatcher launches. It reads the pane's
// input lines and, on the first non-empty one (the dispatched instruction), reports
// `agentState working` for its own pane to the daemon's agent-state socket, exactly as an agent
// extension does. Everything it sees is appended to <logDir>/worker-<pid>.jsonl, which the
// runner reads; it never decides PASS. It is a fixture standing in for an agent extension's
// self-report, not a real provider worker. It exits when its input closes (the pane is gone).
//   argv: <logDir> [--max-lifetime-ms=N] [--no-report]   (leak guard, default 600000)
// With --no-report it logs its input but never reports a state: a target that accepts the
// typed bytes and never shows a turn start (remote-delivery's suppressed receipt).
// A later typed line containing QA-SETTLE (the runner types it) reports `agentState idle`: the
// turn ended, so the supervisor run settles (surfaces). Without that line it never reports idle.
import { appendFileSync, mkdirSync } from 'node:fs';
import { createConnection } from 'node:net';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

const [logDir, ...rest] = process.argv.slice(2);
const lifetimeArg = rest.find((arg) => arg.startsWith('--max-lifetime-ms='));
const lifetimeMs = Number(lifetimeArg?.slice('--max-lifetime-ms='.length) ?? 600_000);
const silent = rest.includes('--no-report');
if (!logDir) process.exit(2);
mkdirSync(logDir, { recursive: true });
const logFile = join(logDir, `worker-${process.pid}.jsonl`);
const sessionId = process.env.FERRYX_SESSION_ID ?? null;
const socket = process.env.FERRYX_AGENT_STATE_SOCKET ?? null;

function record(value) {
  try { appendFileSync(logFile, `${JSON.stringify({ at: new Date().toISOString(), pid: process.pid, ...value })}\n`); } catch { /* the run root may be gone */ }
}

function report(state) {
  return new Promise((resolveReport) => {
    if (!sessionId || !socket) {
      record({ kind: 'report-skipped', state, reason: 'no FERRYX_SESSION_ID or FERRYX_AGENT_STATE_SOCKET' });
      resolveReport(false);
      return;
    }
    const line = `${JSON.stringify({ type: 'agentState', sessionId, state })}\n`;
    const connection = createConnection(socket);
    const timer = setTimeout(() => { connection.destroy(); record({ kind: 'report-failed', state, reason: 'timeout' }); resolveReport(false); }, 5_000);
    connection.on('error', (error) => { clearTimeout(timer); record({ kind: 'report-failed', state, reason: String(error?.message ?? error) }); resolveReport(false); });
    connection.on('connect', () => connection.end(line, () => { clearTimeout(timer); record({ kind: 'reported', state }); resolveReport(true); }));
  });
}

record({ kind: 'ready', sessionId, hasSocket: Boolean(socket), silent, cwd: process.cwd(), argv: process.argv.slice(2) });
setTimeout(() => { record({ kind: 'lifetime-exceeded', lifetimeMs }); process.exit(97); }, lifetimeMs);
process.stdout.write('qa-worker ready\n');

let reported = false;
const lines = createInterface({ input: process.stdin, terminal: false });
lines.on('line', (raw) => {
  const text = raw.replace(/\r/g, '');
  record({ kind: 'input', text });
  if (reported && !silent && text.includes('QA-SETTLE')) { report('idle'); return; }
  if (reported || text.trim() === '') return;
  reported = true;
  if (silent) { record({ kind: 'report-suppressed', state: 'working' }); return; }
  report('working');
});
lines.on('close', () => {
  record({ kind: 'stdin-closed' });
  process.exit(0);
});
// The pane child is a session leader on its PTY: when the daemon exits the kernel hangs the
// terminal up and sends SIGHUP, usually before readline sees the input close. Record it and exit.
process.on('SIGHUP', () => { record({ kind: 'hangup' }); process.exit(0); });
process.on('SIGTERM', () => { record({ kind: 'terminated' }); process.exit(0); });
