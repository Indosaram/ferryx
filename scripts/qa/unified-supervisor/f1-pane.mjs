#!/usr/bin/env bun
// F1 fixture pane (docs/plans/SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md F1). The isolated
// daemon starts it as the pane's login shell through a run-written SHELL wrapper. It stands in
// for a Claude Code session: it reports `agentState` with agent `claude` on the daemon's
// agent-state socket (so the daemon records stateSource provider), draws Claude-shaped lines,
// and makes every turn a real HTTP request to its computer's fixture provider proxy. It never
// decides PASS; every step is appended to <logDir>/pane-<pid>.jsonl, which the host agent
// streams to the runner.
//   argv: <logDir> <modeFile>
//   mode file (written by the host agent just before the spawn): "<role> <label> <proxyUrl>"
// Roles:
//   outage / quota: each turn GETs <proxyUrl>/turn. 200 completes the task; 502 draws
//     "API Error: 502 Bad Gateway"; 429 draws the usage-limit line with the proxy's reset
//     minutes. After an error the pane reports idle at its prompt, as Claude Code does; the next
//     typed line (the supervisor's resume text) starts the next turn.
//   stall: reports working, draws one line, then draws nothing until a typed line arrives (the
//     supervisor's cancel + resume); then it completes.
//   approval: reports blocked and draws a Claude Code Bash approval for `sqlx migrate run`
//     against a non-local database URL. It records anything typed into it and never completes.
import { appendFileSync, mkdirSync, readFileSync } from 'node:fs';
import { createConnection } from 'node:net';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { captureLocalIdentity } from './f1-process-identity.mjs';

export const APPROVAL_COMMAND = 'sqlx migrate run --database-url postgres://app@db.prod.example.net:5432/app';

const [logDir, modeFile] = process.argv.slice(2);
if (!logDir || !modeFile) process.exit(2);
mkdirSync(logDir, { recursive: true });
const [role, label, proxyUrl] = readFileSync(modeFile, 'utf8').trim().split(/\s+/);
const logFile = join(logDir, `pane-${process.pid}.jsonl`);
const sessionId = process.env.FERRYX_SESSION_ID ?? null;
const socket = process.env.FERRYX_AGENT_STATE_SOCKET ?? null;
// Only namespace-local facts belong to the pane; the outer helper writes host mappings.
const identity = captureLocalIdentity();

function record(value) {
  try {
    appendFileSync(logFile, `${JSON.stringify({ at: new Date().toISOString(), atMs: Date.now(), pid: process.pid, identity, label, role, sessionId, ...value })}\n`);
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

const draw = (text) => process.stdout.write(`${text}\n`);
let attempt = 0;
let done = false;
let busy = false;

async function complete() {
  done = true;
  draw(`● Finished ${label}: all steps applied.`);
  draw(`TASK COMPLETE ${label}`);
  record({ kind: 'done' });
  await report('idle');
}

async function providerTurn() {
  busy = true;
  attempt += 1;
  await report('working');
  draw(`● Working on ${label} (turn ${attempt})`);
  let status = -1;
  let body = '';
  try {
    const response = await fetch(`${proxyUrl}/turn?label=${encodeURIComponent(label)}&role=${encodeURIComponent(role)}`);
    status = response.status;
    body = await response.text();
  } catch (error) {
    body = String(error?.message ?? error);
  }
  record({ kind: 'turn', attempt, status, body });
  busy = false;
  if (status === 200) { await complete(); return; }
  if (status === 429) {
    let minutes = 0;
    try { minutes = Number(JSON.parse(body).resetMinutes ?? 0); } catch { /* recorded above */ }
    // The attempt number keeps each error line distinct, so recovery reads it as new output.
    draw(`  ⎿  API Error: 429 Too Many Requests · Usage limit reached. Try again in ${minutes} minutes (attempt ${attempt})`);
  } else {
    draw(`  ⎿  API Error: 502 Bad Gateway · upstream connect error (attempt ${attempt})`);
  }
  await report('idle');
}

function drawApproval() {
  draw(`● Bash(${APPROVAL_COMMAND})`);
  draw('────────────────────────────────────────');
  draw(' Bash command');
  draw(`   ${APPROVAL_COMMAND}`);
  draw('   Apply pending migrations to the production database');
  draw(' Do you want to proceed?');
  draw(' ❯ 1. Yes');
  draw("   2. Yes, and don't ask again for sqlx migrate commands in this project");
  draw('   3. No');
  draw(' Esc to cancel · Tab to amend');
}

record({ kind: 'ready', cwd: process.cwd(), proxyUrl });
const lines = createInterface({ input: process.stdin, terminal: false });
lines.on('line', (raw) => {
  // A Stop types Escape; the resume that follows lands on the same line.
  // eslint-disable-next-line no-control-regex
  const text = raw.replace(/\u001b\[[0-9;?]*[ -/]*[@-~]/g, '').replace(/\u001b/g, '').replace(/\r/g, '').trim();
  record({ kind: 'input', raw, text, escape: raw.includes('\u001b') });
  if (role === 'approval' || done || busy || text === '') return;
  if (role === 'stall') { void complete(); return; }
  void providerTurn();
});
lines.on('close', () => { record({ kind: 'stdin-closed' }); process.exit(0); });
process.on('SIGHUP', () => { record({ kind: 'hangup' }); process.exit(0); });
process.on('SIGTERM', () => { record({ kind: 'terminated' }); process.exit(0); });

if (role === 'approval') {
  // Drawn before the blocked report, so the reading that carries `blocked` already has the card.
  drawApproval();
  await report('blocked');
  record({ kind: 'approval-drawn', command: APPROVAL_COMMAND });
} else if (role === 'stall') {
  await report('working');
  draw(`● Working on ${label}: reading the repository`);
  record({ kind: 'stalled' });
} else {
  void providerTurn();
}
