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
//   focus     draws one idle line naming its title and reports `idle`, so the panel lists it
//             as a plain session row; every key it receives is recorded, which proves the row
//             click focused this pane's terminal (F3-D6).
//
// Every received byte and every state report goes to <logDir>/agent-<title>.jsonl, which the
// runner copies as evidence; this file never decides PASS. Ctrl-C (the Stop path) is recorded
// and ends the process with 130. A line F3-REPORT:<token> sends one long supervisor report
// (a `supervisorReport` line on the agent-state socket, as fixture-worker.mjs does) whose body
// carries <token> in its heading and in its last line, so the Viewer check can prove the whole
// report was opened.
//   argv: <logDir> <decision|held|upstream|focus> <title>
import { appendFileSync, mkdirSync } from 'node:fs';
import { createConnection } from 'node:net';
import { join } from 'node:path';
import { classifyFixtureInput } from './seams.mjs';

const [logDir, mode, title] = process.argv.slice(2);
if (!logDir || !['decision', 'held', 'upstream', 'focus'].includes(mode) || !/^[a-z0-9]+$/.test(title ?? '')) {
  process.stderr.write('usage: fixture-agent.mjs <logDir> <decision|held|upstream|focus> <title [a-z0-9]+>\n');
  process.exit(5);
}
mkdirSync(logDir, { recursive: true });
const logFile = join(logDir, `agent-${title}.jsonl`);
const record = (entry) => appendFileSync(logFile, `${JSON.stringify({ at: new Date().toISOString(), pid: process.pid, mode, title, ...entry })}\n`);
let nonce = 1;
let drawn = mode !== 'held';
let controlInput = '';

function report(state) {
  const socket = process.env.FERRYX_AGENT_STATE_SOCKET;
  const port = Number(process.env.FERRYX_AGENT_STATE_PORT);
  const token = process.env.FERRYX_AGENT_STATE_TOKEN;
  const sessionId = process.env.FERRYX_SESSION_ID;
  if ((!socket && !(port && token)) || !sessionId) { record({ kind: 'report-skipped', state, reason: 'no agent-state endpoint or FERRYX_SESSION_ID' }); return; }
  const connection = createConnection(port && token ? { host: '127.0.0.1', port } : socket);
  connection.on('connect', () => connection.end(`${JSON.stringify({ type: 'agentState', sessionId, state, agent: 'claude', ...(port && token ? { token } : {}) })}\n`));
  connection.on('data', (reply) => record({ kind: 'report-reply', state, reply: String(reply).trim() }));
  connection.on('error', (error) => record({ kind: 'report-failed', state, error: error.message }));
}

function sendReport(token) {
  const socket = process.env.FERRYX_AGENT_STATE_SOCKET;
  const port = Number(process.env.FERRYX_AGENT_STATE_PORT);
  const auth = process.env.FERRYX_AGENT_STATE_TOKEN;
  const sessionId = process.env.FERRYX_SESSION_ID;
  const sections = Array.from({ length: 40 }, (_, i) => `## Section ${i + 1}\n\nMigration check ${i + 1} for ${title}: schema, data and rollback reviewed.`);
  const body = [`# F3 long report ${token}`, '', ...sections, '', `End of report ${token}`].join('\n');
  if ((!socket && !(port && auth)) || !sessionId) { record({ kind: 'report-skipped', token, reason: 'no agent-state endpoint or FERRYX_SESSION_ID' }); return; }
  const eventId = `f3-report-${token}`;
  const connection = createConnection(port && auth ? { host: '127.0.0.1', port } : socket);
  let reply = '';
  connection.on('connect', () => {
    connection.end(`${JSON.stringify({ type: 'supervisorReport', sessionId, eventId, body, ...(port && auth ? { token: auth } : {}) })}\n`, () =>
      record({ kind: 'supervisor-report-written', token, eventId, bytes: body.length, transport: port && auth ? 'tcp' : 'socket', sessionId }));
  });
  connection.on('data', (chunk) => {
    reply += chunk.toString('utf8');
    const newline = reply.indexOf('\n');
    if (newline !== -1) {
      record({ kind: 'supervisor-report', token, eventId, bytes: body.length, reply: reply.slice(0, newline).trim() });
      reply = reply.slice(newline + 1);
    }
  });
  connection.on('error', (error) => record({ kind: 'supervisor-report-failed', token, error: error.message }));
}

function drawDecision() {
  const command = `sqlx migrate run --database-url postgres://prod.example.com/${title}${nonce}`;
  process.stdout.write('\x1b[2J\x1b[H');
  process.stdout.write([
    '', `● Running the ${title} migration`, `  ⎿  $ ${command}`, '────────────────────────────────────────',
    ' Bash command', `   ${command}`, `   Apply migration ${nonce} to the production database`,
    // The question names its own migration: the card paints it, so the screen can tell one
    // answered card from the next one the fixture draws (F3-D5). Every other line is unchanged.
    ` Do you want to proceed with migration ${nonce}?`, ' ❯ 1. Yes', '   2. Yes, and always allow access to this database from this project',
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

function drawFocus() {
  process.stdout.write('\x1b[2J\x1b[H');
  process.stdout.write(`\r\n● ${title} is idle. Waiting for input.\r\n`);
  record({ kind: 'focus-drawn' });
  report('idle');
}

if (process.stdin.isTTY) process.stdin.setRawMode(true);
process.stdin.on('data', (chunk) => {
  const classified = classifyFixtureInput({ bytes: chunk, controlInput, drawn, mode });
  controlInput = classified.controlInput;
  const kind = classified.kind;
  record({ kind: 'received', classified: kind, hex: chunk.toString('hex'), nonce });
  if (kind === 'stop') { record({ kind: 'stopped' }); process.exit(130); }
  if (kind === 'draw') { drawn = true; drawDecision(); return; }
  if (kind.startsWith('report:')) { sendReport(kind.slice('report:'.length)); return; }
  // The focus pane echoes printable input, so the typed token is visible in its terminal.
  if (mode === 'focus') { process.stdout.write(chunk.toString('utf8').replace(/[^\x20-\x7e]/g, '')); return; }
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
else if (mode === 'focus') drawFocus();
