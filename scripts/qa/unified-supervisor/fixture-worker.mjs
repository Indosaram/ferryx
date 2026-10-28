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
// A later typed line containing QA-SETTLE (the runner types it) reports `agentState idle`: the
// turn ended, so the supervisor run settles (surfaces). Without that line it never reports idle.
// It applies only to the single-turn mode after a reported start (not --no-report, not a
// withheld QA_NO_START_REPORT start, not --multi-turn, which reports idle per turn itself).
// With --multi-turn (autonomy) every delivered instruction is one turn: the worker reports
// `working`, writes result.txt in its cwd (`NOT READY` on its first turn, exactly `READY`
// after), runs the runner-committed verify.sh, prints its exit code to the pane, then reports
// `idle` so the run settles. It turns bracketed paste on (ESC[?2004h) at start, as an agent
// TUI does, so a delivery arrives as one paste whose lines are CR-joined; the following Enter
// ends the turn. A plain line outside a paste is one turn too.
// An instruction containing QA_NO_START_REPORT is logged but never reported: the `recovery`
// scenario uses it to hold a dispatch between "typed" and "start recorded".
// With --hold-dir=DIR (--multi-turn only), a turn whose instruction names QA_HOLD:<token>
// reports `working`, then waits until DIR/<token> exists before it writes, verifies and reports
// `idle`: the `waiting` scenario decides when each run settles. A release file that already
// exists lets the turn go on at once. A turn that also names QA_ECHO:<token> waits, after its
// `idle`, until DIR/<token> exists and then reports `idle`, `working`, `idle` again: a
// duplicate and out-of-order report of a run that already ended.
// With --hold-dir, for the worker's whole life, every file DIR/report-<sessionId>-<n> (its own
// session only, written by rename so it is never seen half written) is one ordinary supervisor
// report: its content is the report id, sent as a `supervisorReport` line on the agent-state
// socket, the reply recorded as `supervisor-report`. The `burst` scenario drives its reports so.
import { spawnSync } from 'node:child_process';
import { appendFileSync, existsSync, mkdirSync, readdirSync, readFileSync, watch, writeFileSync } from 'node:fs';
import { createConnection } from 'node:net';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

const [logDir, ...rest] = process.argv.slice(2);
const lifetimeArg = rest.find((arg) => arg.startsWith('--max-lifetime-ms='));
const lifetimeMs = Number(lifetimeArg?.slice('--max-lifetime-ms='.length) ?? 600_000);
const silent = rest.includes('--no-report');
const multiTurn = rest.includes('--multi-turn');
const holdDir = rest.find((arg) => arg.startsWith('--hold-dir='))?.slice('--hold-dir='.length) || null;
if (!logDir) process.exit(2);
mkdirSync(logDir, { recursive: true });
if (holdDir) mkdirSync(holdDir, { recursive: true });
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

// Resolves once `token`'s release file exists: subscribed to the directory first, then checked.
function released(token) {
  const file = join(holdDir, token);
  return new Promise((resolveRelease) => {
    let watcher = null;
    const check = () => {
      if (!existsSync(file)) return;
      try { watcher?.close(); } catch { /* already closed */ }
      resolveRelease();
    };
    watcher = watch(holdDir, check);
    check();
  });
}

// One `supervisorReport` line; resolves with the daemon's JSON reply (or why there was none).
function supervisorReport(eventId, body) {
  return new Promise((resolveReport) => {
    if (!sessionId || !socket) { resolveReport({ sent: false, reason: 'no FERRYX_SESSION_ID or FERRYX_AGENT_STATE_SOCKET' }); return; }
    let buffer = '';
    let done = false;
    const connection = createConnection(socket);
    const finish = (value) => { if (done) return; done = true; clearTimeout(timer); connection.destroy(); resolveReport(value); };
    const timer = setTimeout(() => finish({ sent: true, reason: 'no reply within 5s' }), 5_000);
    connection.setEncoding('utf8');
    connection.on('error', (error) => finish({ sent: false, reason: String(error?.message ?? error) }));
    connection.on('connect', () => connection.write(`${JSON.stringify({ type: 'supervisorReport', sessionId, eventId, body })}\n`));
    connection.on('data', (chunk) => {
      buffer += chunk;
      const end = buffer.indexOf('\n');
      if (end < 0) return;
      let reply = null;
      try { reply = JSON.parse(buffer.slice(0, end)); } catch { reply = { unparsed: buffer.slice(0, end) }; }
      finish({ sent: true, reply });
    });
  });
}

if (holdDir && sessionId) {
  const prefix = `report-${sessionId}-`;
  const seen = new Set();
  let reports = Promise.resolve();
  const scan = () => {
    for (const name of readdirSync(holdDir).sort()) {
      if (!name.startsWith(prefix) || seen.has(name)) continue;
      seen.add(name);
      const eventId = readFileSync(join(holdDir, name), 'utf8').trim();
      reports = reports.then(async () => {
        const result = await supervisorReport(eventId, `qa report ${eventId}`);
        record({ kind: 'supervisor-report', file: name, eventId, ...result });
      });
    }
  };
  watch(holdDir, scan);
  scan();
}

// One turn of --multi-turn, run strictly one after another.
let turns = 0;
let queue = Promise.resolve();
function turn(instruction) {
  queue = queue.then(async () => {
    turns += 1;
    const index = turns;
    record({ kind: 'instruction', turn: index, text: instruction });
    await report('working');
    const hold = holdDir ? instruction.match(/QA_HOLD:([A-Za-z0-9_-]+)/)?.[1] ?? null : null;
    if (hold) {
      record({ kind: 'held', turn: index, token: hold });
      await released(hold);
      record({ kind: 'released', turn: index, token: hold });
    }
    const bytes = index === 1 ? 'NOT READY' : 'READY';
    writeFileSync('result.txt', bytes);
    const verify = spawnSync('/bin/sh', ['verify.sh'], { encoding: 'utf8', timeout: 20_000 });
    const code = verify.status ?? -1;
    record({ kind: 'turn', turn: index, wrote: bytes, verifyExit: code, stderr: verify.stderr ?? '' });
    process.stdout.write(`turn ${index}: wrote result.txt, verify.sh exited ${code}\n`);
    await report('idle');
    const echo = holdDir ? instruction.match(/QA_ECHO:([A-Za-z0-9_-]+)/)?.[1] ?? null : null;
    if (echo) {
      await released(echo);
      const states = ['idle', 'working', 'idle'];
      const delivered = [];
      for (const state of states) delivered.push(await report(state));
      record({ kind: 'echoed', turn: index, token: echo, states, delivered });
    }
  });
}

// Bracketed paste of the current delivery, when one is open.
let paste = null;
let reported = false;
// A `working` start was reported (not --no-report, not withheld): only then may QA-SETTLE end it.
let started = false;
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
  if (reported && started && text.includes('QA-SETTLE')) { report('idle'); return; }
  if (reported || text.trim() === '') return;
  reported = true;
  if (silent) { record({ kind: 'report-suppressed', state: 'working' }); return; }
  if (text.includes('QA_NO_START_REPORT')) {
    record({ kind: 'report-withheld', state: 'working' });
    return;
  }
  started = true;
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
