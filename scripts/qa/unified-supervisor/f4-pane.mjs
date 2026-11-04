#!/usr/bin/env bun
// F4 fixture agent (docs/plans/SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md, F4). The F4 driver
// types `<bun> <runRoot>/bin/claude.js <role> <logDir>` into a real bash pane; claude.js only
// imports this file, so the daemon's foreground observer recognizes the process as agent
// `claude` (interpreter entry point named claude.js). It reports its own state on the daemon's
// agent-state socket with agent `claude` and appends every step to <logDir>/pane-<pid>.jsonl.
// It never decides PASS.
// Roles:
//   killable:  reports working and draws one line, then waits; the driver SIGKILLs it mid-turn.
//   upstream:  draws a distinct Claude-shaped 502 line every 5 s and reports idle, until a typed
//              line (the supervisor's resume) arrives; then it draws completion output.
//   stoppable: reports working in raw mode; on Escape or Ctrl-C (the provider interrupt) it
//              reports idle and stays alive at its prompt.
import { appendFileSync, mkdirSync } from 'node:fs';
import { createConnection } from 'node:net';
import { join } from 'node:path';

const [role, logDir] = process.argv.slice(2);
if (!role || !logDir) process.exit(2);
mkdirSync(logDir, { recursive: true });
const logFile = join(logDir, `pane-${process.pid}.jsonl`);
const sessionId = process.env.FERRYX_SESSION_ID ?? null;
const socket = process.env.FERRYX_AGENT_STATE_SOCKET ?? null;

function record(value) {
  try {
    appendFileSync(logFile, `${JSON.stringify({ at: new Date().toISOString(), atMs: Date.now(), pid: process.pid, role, sessionId, ...value })}\n`);
  } catch { /* the run root may be gone */ }
}

function report(state) {
  return new Promise((resolveReport) => {
    if (!sessionId || !socket) { record({ kind: 'report-skipped', state }); resolveReport(false); return; }
    const line = `${JSON.stringify({ type: 'agentState', sessionId, state, agent: 'claude' })}\n`;
    const connection = createConnection(socket);
    const timer = setTimeout(() => { connection.destroy(); record({ kind: 'report-failed', state, reason: 'timeout' }); resolveReport(false); }, 5_000);
    connection.on('error', (error) => { clearTimeout(timer); record({ kind: 'report-failed', state, reason: String(error?.message ?? error) }); resolveReport(false); });
    connection.on('connect', () => connection.end(line, () => { clearTimeout(timer); record({ kind: 'reported', state }); resolveReport(true); }));
  });
}

const draw = (text) => process.stdout.write(`${text}\r\n`);
process.on('SIGHUP', () => { record({ kind: 'hangup' }); process.exit(0); });
process.on('SIGTERM', () => { record({ kind: 'terminated' }); process.exit(0); });
record({ kind: 'ready', cwd: process.cwd() });

if (role === 'killable') {
  await report('working');
  draw('● Working on the migration plan (turn 1)');
  record({ kind: 'mid-turn' });
  setInterval(() => {}, 60_000);
} else if (role === 'upstream') {
  let attempt = 0;
  let resumed = false;
  const fail = () => {
    if (resumed) return;
    attempt += 1;
    // The attempt number keeps each line distinct, so recovery reads it as fresh output.
    draw(`  ⎿  API Error: 502 Bad Gateway · upstream connect error (attempt ${attempt})`);
    record({ kind: 'error-drawn', attempt });
  };
  await report('idle');
  fail();
  const timer = setInterval(fail, 5_000);
  let buffer = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', async (chunk) => {
    buffer += chunk;
    const lines = buffer.split(/\r?\n/);
    buffer = lines.pop() ?? '';
    for (const raw of lines) {
      const text = raw.replace(/\r/g, '').trim();
      record({ kind: 'input', raw, text });
      if (resumed || text === '') continue;
      resumed = true;
      clearInterval(timer);
      await report('working');
      draw('● Resuming: applying the remaining steps');
      draw('● Finished: all steps applied.');
      draw('TASK COMPLETE upstream');
      record({ kind: 'done' });
      await report('idle');
    }
  });
} else if (role === 'stoppable') {
  if (process.stdin.isTTY) process.stdin.setRawMode(true);
  await report('working');
  draw('● Working on the long refactor (Esc to interrupt)');
  record({ kind: 'mid-turn' });
  let interrupted = false;
  process.stdin.on('data', async (chunk) => {
    const bytes = Buffer.from(chunk);
    record({ kind: 'input-bytes', hex: bytes.toString('hex') });
    if (interrupted || !(bytes.includes(0x1b) || bytes.includes(0x03))) return;
    interrupted = true;
    draw('  ⎿  Interrupted by user');
    record({ kind: 'interrupted' });
    await report('idle');
  });
} else {
  record({ kind: 'unknown-role' });
  process.exit(2);
}
