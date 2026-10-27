#!/usr/bin/env bun
// Task-owned fixture worker for the `admission` scenario (POSIX only). The isolated daemon
// starts it as the pane's login shell (SHELL points at a runner-written /bin/sh wrapper that
// execs this file), so it is the worker the supervisor dispatcher launches. It reads the pane's
// input lines and, on the first non-empty one (the dispatched instruction), reports
// `agentState working` for its own pane to the daemon's agent-state socket, exactly as an agent
// extension does. Everything it sees is appended to <logDir>/worker-<pid>.jsonl, which the
// runner reads; it never decides PASS. It is a fixture standing in for an agent extension's
// self-report, not a real provider worker. It exits when its input closes (the pane is gone).
//   argv: <logDir> [--max-lifetime-ms=N]   (leak guard, default 600000)
import { appendFileSync, mkdirSync } from 'node:fs';
import { createConnection } from 'node:net';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

const [logDir, ...rest] = process.argv.slice(2);
const lifetimeArg = rest.find((arg) => arg.startsWith('--max-lifetime-ms='));
const lifetimeMs = Number(lifetimeArg?.slice('--max-lifetime-ms='.length) ?? 600_000);
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

record({ kind: 'ready', sessionId, hasSocket: Boolean(socket), cwd: process.cwd(), argv: process.argv.slice(2) });
setTimeout(() => { record({ kind: 'lifetime-exceeded', lifetimeMs }); process.exit(97); }, lifetimeMs);
process.stdout.write('qa-worker ready\n');

let reported = false;
const lines = createInterface({ input: process.stdin, terminal: false });
lines.on('line', (raw) => {
  const text = raw.replace(/\r/g, '');
  record({ kind: 'input', text });
  if (reported || text.trim() === '') return;
  reported = true;
  report('working');
});
lines.on('close', () => {
  record({ kind: 'stdin-closed' });
  process.exit(0);
});
