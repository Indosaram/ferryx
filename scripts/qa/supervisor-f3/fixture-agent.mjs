#!/usr/bin/env bun
// Task-owned fixture agent for the F3 acceptance run. The session host's isolated daemon starts
// it as a pane shell (host-helper.mjs writes the per-mode wrapper the spawn names). It stands in
// for a Claude Code session; it is not a real provider worker:
//
//   decision  draws a Claude approval prompt for a migration against a non-local database (D1a:
//             the supervisor never answers it, it is always a decision card), records every key
//             the pane receives, and after an answer draws a fresh prompt (next nonce).
//   held      a decision session that draws nothing until the pane receives a line F3-DRAW, so
//             the runner decides when its card (and its push) appears.
//   upstream  draws a provider 502 retry screen and reports `working`, so the recovery
//             scheduler lists it under Handling with Stop.
//
// Every received byte and every state report goes to <logDir>/agent-<title>.jsonl, which the
// runner copies as evidence; this file never decides PASS. Ctrl-C (the Stop path) is recorded
// and ends the process with 130. A line F3-REPORT:<token> sends one long supervisor report
// (a `supervisorReport` line on the agent-state socket, as fixture-worker.mjs does) whose body
// carries <token> in its heading and in its last line, so the Viewer check can prove the whole
// report was opened.
//   argv: <logDir> <decision|held|upstream> <title>
import { appendFileSync, mkdirSync } from 'node:fs';
import { createConnection } from 'node:net';
import { join } from 'node:path';

const [logDir, mode, title] = process.argv.slice(2);
if (!logDir || !['decision', 'held', 'upstream'].includes(mode) || !/^[a-z0-9]+$/.test(title ?? '')) {
  process.stderr.write('usage: fixture-agent.mjs <logDir> <decision|held|upstream> <title [a-z0-9]+>\n');
  process.exit(5);
}
mkdirSync(logDir, { recursive: true });
const logFile = join(logDir, `agent-${title}.jsonl`);
const record = (entry) => appendFileSync(logFile, `${JSON.stringify({ at: new Date().toISOString(), pid: process.pid, mode, title, ...entry })}\n`);
let nonce = 1;
let drawn = mode !== 'held';

function report(state) {
  const socket = process.env.FERRYX_AGENT_STATE_SOCKET;
  const sessionId = process.env.FERRYX_SESSION_ID;
  if (!socket || !sessionId) { record({ kind: 'report-skipped', state, reason: 'no FERRYX_AGENT_STATE_SOCKET or FERRYX_SESSION_ID' }); return; }
  const connection = createConnection(socket);
  connection.on('connect', () => connection.end(`${JSON.stringify({ type: 'agentState', sessionId, state, agent: 'claude' })}\n`));
  connection.on('data', (reply) => record({ kind: 'report-reply', state, reply: String(reply).trim() }));
  connection.on('error', (error) => record({ kind: 'report-failed', state, error: error.message }));
}

function sendReport(token) {
  const socket = process.env.FERRYX_AGENT_STATE_SOCKET;
  const sessionId = process.env.FERRYX_SESSION_ID;
  const sections = Array.from({ length: 40 }, (_, i) => `## Section ${i + 1}\n\nMigration check ${i + 1} for ${title}: schema, data and rollback reviewed.`);
  const body = [`# F3 long report ${token}`, '', ...sections, '', `End of report ${token}`].join('\n');
  if (!socket || !sessionId) { record({ kind: 'report-skipped', token, reason: 'no FERRYX_AGENT_STATE_SOCKET or FERRYX_SESSION_ID' }); return; }
  const eventId = `f3-report-${token}`;
  const connection = createConnection(socket);
  connection.on('connect', () => connection.end(`${JSON.stringify({ type: 'supervisorReport', sessionId, eventId, body })}\n`));
  connection.on('data', (reply) => record({ kind: 'supervisor-report', token, eventId, bytes: body.length, reply: String(reply).trim() }));
  connection.on('error', (error) => record({ kind: 'supervisor-report-failed', token, error: error.message }));
}

function drawDecision() {
  const command = `sqlx migrate run --database-url postgres://prod.example.com/${title}${nonce}`;
  process.stdout.write('\x1b[2J\x1b[H');
  process.stdout.write([
    '', `● Running the ${title} migration`, `  ⎿  $ ${command}`, '────────────────────────────────────────',
    ' Bash command', `   ${command}`, `   Apply migration ${nonce} to the production database`,
    ' Do you want to proceed?', ' ❯ 1. Yes', '   2. Yes, and always allow access to this database from this project',
    '   3. Yes, and switch to auto mode · auto mode handles these prompts for you', '   4. No', ' Esc to cancel · Tab to amend', '',
  ].join('\r\n'));
  record({ kind: 'prompt-drawn', nonce, command });
  report('blocked');
}

function drawUpstream() {
  process.stdout.write('\x1b[2J\x1b[H');
  process.stdout.write(`\r\n● Working on ${title}\r\n  ⎿  API Error: 502 Bad Gateway · upstream proxy error · Retrying in 30s\r\n`);
  record({ kind: 'upstream-drawn' });
  report('working');
}

function classify(bytes) {
  if (bytes.includes(0x03)) return 'stop';
  const text = bytes.toString('utf8');
  const report = /F3-REPORT:([a-z0-9]+)/.exec(text);
  if (report) return `report:${report[1]}`;
  if (!drawn) return text.includes('F3-DRAW') ? 'draw' : 'input';
  if (mode === 'upstream') return 'input';
  // The option the keys select: a digit, or Enter after N down-arrows from option 1; Esc alone
  // cancels. Option 1 is Yes, option 4 is No.
  const digit = /^([1-4])/.exec(text);
  const arrows = text.split('\x1b[B').length - 1;
  const index = digit ? Number(digit[1]) : text.endsWith('\r') ? arrows + 1 : text === '\x1b' ? 4 : null;
  if (index === 1) return 'approve';
  if (index === 4) return 'reject';
  return index === null ? 'input' : `option${index}`;
}

if (process.stdin.isTTY) process.stdin.setRawMode(true);
process.stdin.on('data', (chunk) => {
  const kind = classify(chunk);
  record({ kind: 'received', classified: kind, hex: chunk.toString('hex'), nonce });
  if (kind === 'stop') { record({ kind: 'stopped' }); process.exit(130); }
  if (kind === 'draw') { drawn = true; drawDecision(); return; }
  if (kind.startsWith('report:')) { sendReport(kind.slice('report:'.length)); return; }
  if (kind === 'approve' || kind === 'reject' || kind.startsWith('option')) {
    process.stdout.write(`\r\nAnswered ${kind} for migration ${nonce}.\r\n`);
    record({ kind: 'answered', answer: kind, nonce });
    nonce += 1;
    drawDecision();
  }
});
process.stdin.on('end', () => { record({ kind: 'input-closed' }); process.exit(0); });
record({ kind: 'ready', sessionId: process.env.FERRYX_SESSION_ID ?? null, cwd: process.cwd() });
if (mode === 'decision') drawDecision();
else if (mode === 'upstream') drawUpstream();
