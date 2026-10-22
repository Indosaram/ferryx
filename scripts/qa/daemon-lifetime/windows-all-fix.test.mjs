// Windows --all repair lane (run-int2-all root causes): Q09 retire, Q10 tree, Q11 ConPTY echo,
// Q13 legacy task record, and the Windows teardown order.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { typeVerified, renderEcho, renderConptyInput } from './lib/typeverified.mjs';
import { createJobs } from './lib/jobs.mjs';
import { isolationLayout } from './lib/guards.mjs';
import { retireOwner } from './scenarios/common.mjs';
import { q11Marker } from './scenarios/q10-q13.mjs';
import { teardownRun } from './run.mjs';
import { newSummary } from './lib/summary.mjs';

const RUN = '20261022t120000z';
const ESC = '\x1b';

// ---- Q09: retireOwner speaks the owner's own data protocol and reports a refused connection ----

function retireWorld({ protocol = 6, refuse = false, resetOnRetire = false, exitsOnReset = true } = {}) {
  const w = { alive: new Set([7]), sessions: new Set(['s1']), opens: [], closes: [] };
  const agent = { call: async (method, p = {}) => {
    switch (method) {
      case 'readFile': return p.file.endsWith('owner.json') ? JSON.stringify({ pid: 7, startTime: 1007 }) : null;
      case 'probe': return w.alive.has(p.pid) ? { executable: '/qa/7', startTime: '1007' } : null;
      case 'controlOpen': return { handle: w.alive.has(7) ? 'ctl' : null };
      case 'dataOpen':
        w.opens.push(p);
        if (refuse || (p.version ?? 5) !== protocol) throw Object.assign(new Error('protocolMismatch expected ' + protocol + ' got ' + p.version), { code: 'HANDSHAKE_REFUSED' });
        return { handle: 'd', handshake: { type: 'handshakeOk', version: protocol } };
      case 'close': return true;
      case 'call': {
        const f = p.frame;
        if (f.op === 'Hello') return { ok: true, pid: 7, protocol_version: protocol };
        if (f.op === 'RetireWhenIdle') {
          if (w.sessions.size) return { ok: true, live: w.sessions.size };
          if (resetOnRetire) { if (exitsOnReset) w.alive.delete(7); throw Object.assign(new Error('read ECONNRESET'), { code: 'ECONNRESET' }); }
          w.alive.delete(7); return { ok: true, live: 0 };
        }
        if (f.type === 'describeLiveSessions') return { type: 'describeLiveSessionsOk', sessions: [...w.sessions].map((backendId) => ({ backendId })) };
        if (f.type === 'close') { w.closes.push(f.sessionId); w.sessions.delete(f.sessionId); return { type: 'closeOk' }; }
        return { type: 'unexpected' };
      }
      // As summary.test.mjs ownerWorld: an owner with open sessions never exits on retire.
      // waitExit is subscribed before RetireWhenIdle is sent: read the owner's liveness after it.
      case 'waitExit': await new Promise((r) => setImmediate(r)); return w.alive.has(p.pid) ? { timedOut: true } : { pid: p.pid, exitedAt: 1 };
      default: throw new Error('unexpected ' + method);
    }
  } };
  return { w, ctx: { agent, platform: 'windows', layout: { runtime: '/r/run' }, state: {} } };
}

test('Q09 retire: a protocol-6 owner (Q08b v1bump) is drained over protocol 6 and retires', async () => {
  const { w, ctx } = retireWorld({ protocol: 6 });
  const r = await retireOwner(ctx);
  assert.equal(w.opens[0].version, 6);
  assert.deepEqual(w.closes, ['s1']);
  assert.equal(r.retired, true);
});

test('Q09 retire: a refused data connection is reported in the result, never swallowed', async () => {
  const { ctx } = retireWorld({ protocol: 6, refuse: true });
  const r = await retireOwner(ctx);
  assert.equal(r.retired, false);
  assert.match(String(r.dataError), /protocolMismatch/);
});

test('Q09 retire: an idle owner that exits before its RetireWhenIdle reply is retired, never thrown (only2 ECONNRESET)', async () => {
  const gone = await retireOwner(retireWorld({ protocol: 6, resetOnRetire: true }).ctx);
  assert.equal(gone.retired, true);
  assert.match(String(gone.replyError), /ECONNRESET/);
  // A lost reply alone is not a retirement: the owner must also be seen to exit.
  const stuck = await retireOwner(retireWorld({ protocol: 6, resetOnRetire: true, exitsOnReset: false }).ctx);
  assert.equal(stuck.retired, false);
});

test('Q10 retire: an owner already retiring without a control endpoint is retired once it exits (only3 owner 23480)', async () => {
  // Without control there is no Hello, so the drain speaks protocol 5 (only3: HEAD owner 23480).
  const exiting = retireWorld({ protocol: 5 });
  // Retiring already: its endpoint is gone and it exits once its last session closes.
  exiting.ctx.agent.call = ((call) => async (m, p) => {
    if (m === 'controlOpen') return { handle: null };
    const r = await call(m, p);
    if (m === 'call' && p.frame.type === 'close') exiting.w.alive.delete(7);
    return r;
  })(exiting.ctx.agent.call);
  const r = await retireOwner(exiting.ctx);
  assert.equal(r.retired, true);
  assert.equal(r.reason, 'no control endpoint');
  // An owner without control that keeps running (Q09 part 3) is not retired.
  const kept = retireWorld({ protocol: 5 });
  kept.ctx.agent.call = ((call) => async (m, p) => (m === 'controlOpen' ? { handle: null } : call(m, p)))(kept.ctx.agent.call);
  assert.equal((await retireOwner(kept.ctx)).retired, false);
});

test('Q09 part 3 re-registers the bump task before the no-control launch and ends that owner', () => {
  const src = readFileSync(new URL('./scenarios/q07-q09.mjs', import.meta.url), 'utf8');
  const part3 = src.slice(src.indexOf('// Part 3:'), src.indexOf('\n}\n', src.indexOf('// Part 3:')));
  const removeBefore = part3.indexOf('ctx.jobs.remove(bumpTask)');
  assert.ok(removeBefore >= 0 && removeBefore < part3.indexOf("FERRYX_QA_DISABLE_CONTROL: '1'"), 'old bump task removed before the opted-out launch');
  const end = part3.lastIndexOf('ctx.jobs.remove(bumpTask)');
  assert.ok(end > removeBefore && end < part3.indexOf('killRecordedOwner(ctx, a3.pid)'), 'task removed before the recorded kill');
  assert.ok(part3.indexOf('killRecordedOwner(ctx, a3.pid)') < part3.indexOf('freshOwner(ctx, ctx.artifacts.A'), 'a controllable owner follows');
});

// ---- Q10 / Q09: the watchdog compared is the owner's own tree, read at the time ----

test('Q10 reads the owner tree before the toggle instead of reusing the Q00 watchdog', () => {
  const src = readFileSync(new URL('./scenarios/q10-q13.mjs', import.meta.url), 'utf8');
  const q10 = src.slice(src.indexOf('export async function Q10('), src.indexOf('export async function Q10b('));
  const before = q10.indexOf('windowsTree(ctx.stagedRoot, { ownerPid: owner.pid })');
  const toggle = q10.indexOf('await toggleAutostartOffThroughSettings(');
  assert.ok(before > 0 && before < toggle, 'tree captured before the toggle');
  assert.doesNotMatch(q10, /ctx\.state\.watchdog/);
});

// ---- Q11: the ConPTY repaint is rendered to the cursor line ----

const BANNER = 'Microsoft Windows [Version 10.0.26200.9711]' + ESC + '[K\r\n(c) Microsoft Corporation. All rights reserved.' + ESC + '[K\r\n' + ESC + '[K\r\n'
  + 'C:\\Users\\sook\\ferryx-wt\\qaruns\\r\\ssh-repo>powershell -NoProfile -Command "Write-Output (\'Q11APPEND\' + \'READY\'); while ($true) { $l = [Console]::ReadLine(); if ($null -eq $l) { break }; Add-Content -LiteralPath \'C:\\x\\ssh-input.log\' -Value $l }"' + ESC + '[K\r\n'
  + 'Q11APPENDREADY' + ESC + '[K\r\n';
const blanks = (n) => (ESC + '[K\r\n').repeat(n) + ESC + '[K';
/** One ConPTY full-screen repaint: input on the row after Q11APPENDREADY (screen row 8: the command wraps). */
const repaint = (input) => ESC + '[?25l' + ESC + '[H' + BANNER + (input ? input + ESC + '[K\r\n' + blanks(15) : blanks(16)) + ESC + '[8;' + (input.length + 1) + 'H' + ESC + '[?25h';

const conpty = (raw) => renderConptyInput(raw, 'Q11APPENDREADY');

test('renderConptyInput reads the ConPTY input row, not the repainted screen (run-int2-all Q11 echoed.text)', () => {
  // Measured: the frame before the first key, then the key echoed at the cursor; then a repaint
  // holding that key on its row followed by a second echo.
  const measured1 = repaint('') + 'N';
  const measured2 = repaint('N') + 'N';
  assert.equal(conpty(measured1), 'N');
  assert.equal(conpty(measured1 + measured2), 'NN');
  assert.equal(conpty(repaint('')), '');
  assert.equal(conpty(repaint('unc')), 'unc');
  // The screen text ("C:\Users ...") never satisfies a prefix: a dropped first key reads as empty.
  assert.notEqual(renderEcho(repaint('')), '', 'the old reading flattened the screen');
  assert.equal(conpty(repaint('')), '');
  // A later cursor move on the same row, then an erase to the end of the line.
  assert.equal(conpty(repaint('un') + ESC + '[8;3Hc'), 'unc');
  assert.equal(conpty(repaint('unx') + ESC + '[8;3H' + ESC + '[K'), 'un');
  // No repaint yet: plain echo. A plain canonical tty echo (omarchy) is unchanged.
  assert.equal(conpty('ab'), 'ab');
  // No repaint but the anchor printed: only what follows the anchor's line is input.
  assert.equal(conpty('C:\\Users\\x>powershell Q11APPEND+READY\r\nQ11APPENDREADY\r\nun'), 'un');
  assert.equal(conpty('C:\\Users\\x>Q11APPENDREADY'), '');
  // Rows below the input row (a status or wrapped line in the repaint) are never part of the input.
  const below = ESC + '[H' + BANNER + 'un' + ESC + '[K\r\n' + 'zzzzzzzz' + ESC + '[K\r\n' + blanks(14) + ESC + '[8;3H';
  assert.equal(conpty(below), 'un');
  assert.equal(renderEcho('UNCERRRRR\b \b\b \b\b \b\b \bT'), 'UNCERT');
  assert.equal(renderEcho(ESC + '[?25lAB' + ESC + '[0mC'), 'ABC');
});

/** maho-win: every key repaints the whole ConPTY screen; Shift+letter presses are lost. */
function conptyTerminal() {
  let raw = '';
  let input = '';
  const show = () => { raw += repaint(input); };
  const mahord = {
    key: async (k) => { if (k === 'Backspace') input = input.slice(0, -1); else input += k; show(); },
    hotkey: async () => {},
  };
  return { mahord, echo: async () => raw, waitMore: async (length) => ({ timedOut: raw.length <= length }), input: () => input };
}

test('Q11 a lowercase marker typed through ConPTY repaints converges; Windows uses no Shift', async () => {
  const t = conptyTerminal();
  const r = await typeVerified({ ...t, text: 'uncertain653f0e33d74a', render: conpty });
  assert.equal(r.line, 'uncertain653f0e33d74a');
  assert.equal(t.input(), 'uncertain653f0e33d74a');
  const src = readFileSync(new URL('./scenarios/q10-q13.mjs', import.meta.url), 'utf8');
  assert.match(src, /const marker = q11Marker\(ctx\.platform\)/);
  const win = q11Marker('windows');
  assert.match(win, /^uncertain[0-9a-f]{12}$/, 'Windows marker has no Shift key');
  assert.match(q11Marker('linux'), /^UNCERTAIN[0-9A-F]{12}$/, 'other platforms unchanged');
});

// ---- Q13: the Windows legacy task has the definition record the product migrates by ----

function windowsJobsWorld() {
  const calls = [];
  const record = 'C:\\Users\\sook\\AppData\\Local\\Ferryx\\supervisor\\Ferryx-ferryx-qa-' + RUN + '.xml';
  const agent = { call: async (method, p) => {
    calls.push([method, p]);
    if (method === 'exec' && p.command === 'powershell.exe' && p.args.includes('-File')) return { status: 0, stdout: record + '\r\n', stderr: '' };
    if (method === 'exec' && p.command === 'whoami') return { status: 0, stdout: 'desktop-1lapjmp\\sook\r\n', stderr: '' };
    if (method === 'waitPath') return true;
    if (method === 'readFile' && p.file.endsWith('legacy-task-create.json')) return JSON.stringify({ create: 0, run: 0, stdout: 'ok' });
    return { status: 0, stdout: '', stderr: '' };
  } };
  const layout = isolationLayout('C:\\q\\' + RUN, 'windows');
  return { calls, record, jobs: createJobs({ host: { platform: 'windows' }, agent, runId: RUN, layout }) };
}

test('Q13 Windows legacy install writes the UTF-16 definition record before /Create /XML (493fae45 ensure_with)', async () => {
  const { calls, record, jobs } = windowsJobsWorld();
  await jobs.installLegacy({ executable: 'C:\\t\\ferryx.exe', env: { FERRYX_DATA_DIR: 'C:\\d' } });
  const writes = calls.filter(([m]) => m === 'writeFile').map(([, p]) => p);
  const xml = writes.find((p) => p.file.endsWith('legacy-task.xml'))?.content;
  assert.ok(xml, 'task XML rendered into the run dir');
  assert.match(xml, /<LogonTrigger>/);
  // The account whoami names (an SSH logon's USERDOMAIN is WORKGROUP, which schtasks cannot map).
  assert.match(xml, /<UserId>desktop-1lapjmp\\sook<\/UserId>/);
  // Headless: no console window in front of the QA GUI launches (only3 Q13 g15/g16).
  assert.match(xml, /<Command>conhost\.exe<\/Command>\s*<Arguments>--headless cmd\.exe \/c &quot;[^<]*legacy-daemon\.cmd&quot;<\/Arguments>/);
  const script = writes.find((p) => p.file.endsWith('legacy-task-record.ps1'))?.content;
  assert.match(script, /\[Text\.Encoding\]::Unicode/);
  assert.ok(script.includes('Ferryx-ferryx-qa-' + RUN + '.xml'));
  const execs = calls.filter(([m]) => m === 'exec').map(([, p]) => [p.command, ...p.args]);
  const ps = execs.findIndex((a) => a[0] === 'powershell.exe' && a.includes('-File'));
  // The legacy task is created and run from the non-elevated logon token by a helper task: the
  // elevated SSH session never creates it, so the non-elevated GUI can re-import it.
  const legacy = 'Ferryx\\ferryx-qa-' + RUN;
  assert.ok(!execs.some((a) => a[0] === 'schtasks' && a.includes(legacy)), 'the SSH session never creates or runs the legacy task');
  const create = writes.find((p) => p.file.endsWith('legacy-task-create.ps1'))?.content;
  assert.ok(create.includes(`schtasks /Create /TN '${legacy}' /XML '${record}' /F`), create);
  assert.ok(create.indexOf('/Create') < create.indexOf('/Run'), 'run only after create');
  const helper = 'Ferryx\\ferryx-qa-' + RUN + '-legacy-create';
  const made = execs.findIndex((a) => a[0] === 'schtasks' && a[1] === '/Create' && a[3] === helper);
  assert.ok(ps >= 0 && ps < made, 'record written before the helper creates the task');
  assert.ok(!execs[made].includes('/RU'), 'helper runs with the interactive (limited) token');
  assert.deepEqual(execs.slice(made + 1), [['schtasks', '/Run', '/TN', helper], ['schtasks', '/Delete', '/TN', helper, '/F']]);
});

test('Q13 a failed non-elevated /Create is reported and the helper task is still deleted', async () => {
  const c2 = [];
  const record = 'C:\\r.xml';
  const agent = { call: async (method, p) => {
    c2.push([method, p]);
    if (method === 'exec' && p.command === 'powershell.exe') return { status: 0, stdout: record, stderr: '' };
    if (method === 'exec' && p.command === 'whoami') return { status: 0, stdout: 'd\\s', stderr: '' };
    if (method === 'waitPath') return true;
    if (method === 'readFile') return JSON.stringify({ create: 1, run: null, stdout: 'ERROR: access denied' });
    return { status: 0, stdout: '', stderr: '' };
  } };
  const j = createJobs({ host: { platform: 'windows' }, agent, runId: RUN, layout: isolationLayout('C:\\q\\' + RUN, 'windows') });
  const r = await j.installLegacy({ executable: 'C:\\t\\ferryx.exe', env: {} });
  assert.equal(r.status, 1);
  assert.match(r.stdout, /access denied/);
  assert.deepEqual(c2.filter(([m]) => m === 'exec').map(([, p]) => [p.command, ...p.args]).at(-1), ['schtasks', '/Delete', '/TN', 'Ferryx\\ferryx-qa-' + RUN + '-legacy-create', '/F']);
});

// ---- Teardown: on Windows the tasks end before the recorded daemon is killed ----

test('Windows teardown ends and deletes the QA tasks before killing recorded pids (only-q11 leftover 27232)', async () => {
  for (const platform of ['windows', 'linux']) {
    const order = [];
    const agent = { call: async (method) => {
      if (method === 'teardownRecorded') { order.push('kill'); return { killed: [], refused: [] }; }
      if (method === 'exec') return { status: 0, stdout: '[]', stderr: '' };
      if (method === 'table') return [];
      if (method === 'remove') return true;
      if (method === 'exists') return false;
      return null;
    } };
    let listed = false;
    const jobs = { listQaJobs: async () => { if (listed) return []; listed = true; return ['Ferryx\\ferryx-qa-x-k']; }, remove: async () => order.push('remove-job') };
    const summary = newSummary({ runId: RUN, platform, host: 'h' });
    await teardownRun({ summary, agent, jobs, host: { platform }, layout: { logs: '/s/logs' }, runDirs: [], qaRoots: [], localEvidence: '/nonexistent', prodSnapshot: async () => null });
    assert.deepEqual(order, platform === 'windows' ? ['remove-job', 'kill'] : ['kill', 'remove-job'], platform);
  }
});
