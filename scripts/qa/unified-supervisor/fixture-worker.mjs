#!/usr/bin/env bun
// Task-owned fixture worker for the `admission` scenario (POSIX only). The isolated daemon
// starts it as the pane's login shell (SHELL points at a runner-written /bin/sh wrapper that
// execs this file), so it is the worker the supervisor dispatcher launches. It reads the pane's
// input lines and, on the first non-empty one (the dispatched instruction), reports
// `agentState working` for its own pane to the daemon's agent-state socket, exactly as an agent
// extension does. Everything it sees is appended to <logDir>/worker-<pid>.jsonl, which the
// runner reads; it never decides PASS. It is a fixture standing in for an agent extension's
// self-report, not a real provider worker. It exits when its input closes (the pane is gone).
//   argv: <logDir> [--max-lifetime-ms=N] [--no-report] [--multi-turn]   (leak guard, default 600000)
// With --no-report it logs its input but never reports a state: a target that accepts the
// typed bytes and never shows a turn start (remote-delivery's suppressed receipt).
// With --multi-turn (autonomy) every delivered instruction is one turn: the worker reports
// `working`, writes result.txt in its cwd (`NOT READY` on its first turn, exactly `READY`
// after), runs the runner-committed verify.sh, prints its exit code to the pane, then reports
// `idle` so the run settles. It turns bracketed paste on (ESC[?2004h) at start, as an agent
// TUI does, so a delivery arrives as one paste whose lines are CR-joined; the following Enter
// ends the turn. A plain line outside a paste is one turn too.
// An instruction containing QA_NO_START_REPORT is logged but never reported: the `recovery`
// scenario uses it to hold a dispatch between "typed" and "start recorded".
import { spawnSync } from 'node:child_process';
import { appendFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { createConnection } from 'node:net';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

const [logDir, ...rest] = process.argv.slice(2);
const lifetimeArg = rest.find((arg) => arg.startsWith('--max-lifetime-ms='));
const lifetimeMs = Number(lifetimeArg?.slice('--max-lifetime-ms='.length) ?? 600_000);
const silent = rest.includes('--no-report');
const multiTurn = rest.includes('--multi-turn');
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

record({ kind: 'ready', sessionId, hasSocket: Boolean(socket), silent, multiTurn, cwd: process.cwd(), argv: process.argv.slice(2) });
setTimeout(() => { record({ kind: 'lifetime-exceeded', lifetimeMs }); process.exit(97); }, lifetimeMs);
process.stdout.write(multiTurn ? '\u001b[?2004hqa-worker ready\n' : 'qa-worker ready\n');

// One turn of --multi-turn, run strictly one after another.
let turns = 0;
let queue = Promise.resolve();
function turn(instruction) {
  queue = queue.then(async () => {
    turns += 1;
    const index = turns;
    record({ kind: 'instruction', turn: index, text: instruction });
    await report('working');
    const bytes = index === 1 ? 'NOT READY' : 'READY';
    writeFileSync('result.txt', bytes);
    const verify = spawnSync('/bin/sh', ['verify.sh'], { encoding: 'utf8', timeout: 20_000 });
    const code = verify.status ?? -1;
    record({ kind: 'turn', turn: index, wrote: bytes, verifyExit: code, stderr: verify.stderr ?? '' });
    process.stdout.write(`turn ${index}: wrote result.txt, verify.sh exited ${code}\n`);
    await report('idle');
  });
}

// Bracketed paste of the current delivery, when one is open.
let paste = null;
let reported = false;
const lines = createInterface({ input: process.stdin, terminal: false });
lines.on('line', (raw) => {
  if (multiTurn) {
    record({ kind: 'input', text: raw.replace(/\r/g, '\n') });
    let text = raw;
    if (text.includes('\u001b[200~')) { paste = ''; text = text.slice(text.indexOf('\u001b[200~') + 6); }
    if (paste !== null) {
      const end = text.indexOf('\u001b[201~');
      if (end < 0) { paste += `${text}\n`; return; }
      paste += text.slice(0, end);
      text = paste;
      paste = null;
    }
    const instruction = text.replace(/\r/g, '\n').trim();
    if (instruction !== '') turn(instruction);
    return;
  }
  const text = raw.replace(/\r/g, '');
  record({ kind: 'input', text });
  if (reported || text.trim() === '') return;
  reported = true;
  if (silent) { record({ kind: 'report-suppressed', state: 'working' }); return; }
  if (text.includes('QA_NO_START_REPORT')) {
    record({ kind: 'report-withheld', state: 'working' });
    return;
  }
  report('working');
});
lines.on('close', () => {
  record({ kind: 'stdin-closed' });
  process.exit(0);
});
// A read from a pty whose master is gone fails with EIO (Linux) instead of ending: that is the
// same terminal hang-up as SIGHUP, which may still be queued behind it. Any other input error,
// and anything uncaught, is recorded as what it is (not a clean exit) instead of dying silently.
process.stdin.on('error', (error) => {
  if (error?.code === 'EIO' || error?.code === 'ENXIO') {
    record({ kind: 'hangup', via: `stdin ${error.code}` });
    process.exit(0);
  }
  record({ kind: 'stdin-error', error: String(error?.code ?? error?.message ?? error) });
  process.exit(3);
});
process.on('uncaughtException', (error) => {
  record({ kind: 'crashed', error: String(error?.stack ?? error) });
  process.exit(4);
});
// The pane child is a session leader on its PTY: when the daemon exits the kernel hangs the
// terminal up and sends SIGHUP, usually before readline sees the input close. Record it and exit.
process.on('SIGHUP', () => { record({ kind: 'hangup' }); process.exit(0); });
process.on('SIGTERM', () => { record({ kind: 'terminated' }); process.exit(0); });
