import test from 'node:test';
import assert from 'node:assert/strict';
import { selectWindowsTree } from './lib/os-jobs.mjs';
import { createJobs } from './lib/jobs.mjs';
import { sshdArgs, sshdConfig, startSshd, BANNER_PROBE_SCRIPT } from './lib/sshd.mjs';
import { createOutputRoute, waitForContent, readLogText, LOG_NOT_YET_CODES } from './lib/agent.mjs';
import { createGuiDriver, windowsLaunchCmdContent, windowsLaunchFiles, windowsCloseFiles, lastObserverRecord, isExplorerDialogForeground, GO_WAIT_SECONDS } from './lib/gui.mjs';
import { isolationLayout } from './lib/guards.mjs';
import { requireOwner, nonceRoundTrip, hasShownPrompt, FIRST_PROMPT_MS } from './scenarios/common.mjs';
import { legacyDaemonRows, recordLegacyJobProcess, SESSION_EXIT_TIMEOUT_MS } from './scenarios/q10-q13.mjs';
import { recoverAfterScenarioException, endScenarioGuis, recordedGuiPids } from './run.mjs';
import { Q07 } from './scenarios/q07-q09.mjs';
import { Q12 } from './scenarios/q10-q13.mjs';
import { readFileSync } from 'node:fs';

const RUN = '20261021t120000z';
const STAGED = 'C:\\s';
const row = (pid, ppid, args) => ({ pid, ppid, executable: 'C:\\s\\k\\ferryx.exe', commandLine: `"C:\\s\\k\\ferryx.exe" ${args}` });

test('Q04: the watchdog is the owner pid parent, never the first --daemon-supervisor found', () => {
  // Two watchdogs under the staged root; the first one has no daemon child (run-q00fix-all hypothesis).
  const rows = [row(10, 1, '--daemon-supervisor'), row(20, 1, '--daemon-supervisor'), row(21, 20, '--daemon')];
  const t = selectWindowsTree(rows, { ownerPid: 21 });
  assert.equal(t.watchdog.pid, 20);
  assert.equal(t.daemon.pid, 21);
  assert.deepEqual(selectWindowsTree(rows, {}).daemon.pid, 21, 'one watchdog with a daemon child is unambiguous');
  const gone = selectWindowsTree(rows, { ownerPid: 99 });
  assert.equal(gone.daemon, null);
  assert.equal(gone.watchdogs.length, 2);
  assert.match(gone.reason, /owner pid is not a daemon/);
  const two = selectWindowsTree([...rows, row(11, 10, '--daemon')], {});
  assert.equal(two.daemon, null);
  assert.match(two.reason, /ambiguous/);
  // A daemon whose parent is not a watchdog (a GUI-spawned fallback) is not the job tree.
  assert.equal(selectWindowsTree([row(30, 5, '--daemon'), ...rows], { ownerPid: 30 }).daemon, null);
});

test('jobs.windowsTree passes the owner pid and keeps the staged-root rows', async () => {
  const rows = [row(10, 1, '--daemon-supervisor'), row(20, 1, '--daemon-supervisor'), row(21, 20, '--daemon'), { pid: 3, ppid: 1, executable: 'C:\\other\\x.exe', commandLine: 'x' }];
  const jobs = createJobs({ host: { platform: 'windows' }, agent: { call: async () => rows }, runId: RUN, layout: isolationLayout('C:\\r', 'windows') });
  const t = await jobs.windowsTree(STAGED, { ownerPid: 21 });
  assert.equal(t.watchdog.pid, 20);
  assert.equal(t.rows.length, 3);
});

test('Q04 jobPid saves the task tree as evidence whenever the daemon resolves to null', () => {
  const src = readFileSync(new URL('./scenarios/q04-q06.mjs', import.meta.url), 'utf8');
  assert.match(src, /windowsTree\(ctx\.stagedRoot, \{ ownerPid \}\)/);
  assert.match(src, /if \(!tree\.daemon && result\) await evidence\(ctx, result, `\$\{label\}-task-tree\.json`/);
});

test('Q11: no UsePAM on Windows sshd, unchanged elsewhere', () => {
  const base = { port: 2222, hostKey: '/k', authorizedKeys: '/a', pidFile: '/p', user: 'qa' };
  assert.doesNotMatch(sshdConfig({ ...base, platform: 'windows' }), /UsePAM/);
  for (const platform of ['linux', 'mac']) assert.match(sshdConfig({ ...base, platform }), /^UsePAM no$/m);
});

function sshdWorld({ testStatus = 0, exitsEarly = false, listens = true } = {}) {
  const calls = [];
  const never = new Promise(() => {});
  const agent = { call: async (m, p = {}) => {
    calls.push([m, p.command ?? p.file ?? p.pid, p.args?.[0]]);
    switch (m) {
      case 'exists': case 'mkdir': case 'remove': case 'writeFile': return true;
      case 'exec':
        if (p.command === 'node') return exitsEarly ? never : (listens ? { status: 0, stdout: 'SSH-2.0-OpenSSH_for_Windows_9.5\r\n', stderr: '' } : { status: 1, stdout: '', stderr: '' });
        return p.args?.[0] === '-t' ? { status: testStatus, stdout: '', stderr: testStatus ? 'line 9: Unsupported option UsePAM' : '' } : { status: 0, stdout: '', stderr: '' };
      case 'readFile': return String(p.file).endsWith('sshd.log') ? 'sshd_config line 9: Unsupported option UsePAM\r\n' : 'ssh-ed25519 AAAA qa';
      case 'spawn': return { pid: 4711 };
      case 'waitExit': return exitsEarly ? { pid: 4711, exitedAt: 1 } : (listens ? never : { timedOut: true });
      case 'waitLog': return exitsEarly ? never : (listens ? { matched: 'Server listening' } : { matched: null, timedOut: true });
      default: return null;
    }
  } };
  const ctx = { agent, platform: 'windows', layout: isolationLayout('C:\\r', 'windows'), hostUser: 'sook', sshPort: 2222, repoDir: 'C:\\repo', state: {} };
  return { ctx, calls };
}

test('Q11: sshd -t rejects the config before anything is spawned', async () => {
  const { ctx, calls } = sshdWorld({ testStatus: 1 });
  const r = await startSshd(ctx);
  assert.match(r.unavailable, /^sshd -t rejected the config: line 9: Unsupported option UsePAM/);
  assert.ok(!calls.some((c) => c[0] === 'spawn'));
});

test('Q11: an sshd that exits before listening is reported with its log, not after the 15 s wait', async () => {
  const { ctx } = sshdWorld({ exitsEarly: true });
  const r = await startSshd(ctx);
  assert.match(r.unavailable, /^sshd exited before listening: sshd_config line 9/);
  assert.equal(r.pid, 4711);
  const silent = await startSshd(sshdWorld({ listens: false }).ctx);
  assert.equal(silent.unavailable, 'sshd did not report listening');
});

test('Q11: Windows sshd logs to a file (-E), never to piped stderr (-e); elsewhere unchanged', async () => {
  const { ctx, calls } = sshdWorld();
  await startSshd(ctx);
  const spawned = calls.find((c) => c[0] === 'spawn');
  assert.ok(spawned, 'sshd spawned');
  assert.deepEqual(sshdArgs({ config: 'C:\\c', logFile: 'C:\\l\\sshd.log', platform: 'windows' }), ['-D', '-E', 'C:\\l\\sshd.log', '-f', 'C:\\c']);
  for (const platform of ['linux', 'mac']) assert.deepEqual(sshdArgs({ config: '/c', logFile: '/l', platform }), ['-D', '-e', '-f', '/c']);
});

test('Q11: Windows readiness reads the SSH banner (sshd -E locks its log), never waitLog on it', async () => {
  const { ctx, calls } = sshdWorld();
  const r = await startSshd(ctx);
  assert.equal(r.unavailable, undefined);
  assert.ok(calls.some((c) => c[0] === 'exec' && c[1] === 'node'));
  assert.ok(!calls.some((c) => c[0] === 'waitLog'));
});

test('Q11: the banner probe script exits 0 with the banner, 1 when nothing answers in time', async () => {
  const { createServer } = await import('node:net');
  const { execFile } = await import('node:child_process');
  const server = createServer((s) => s.end('SSH-2.0-QA\r\n'));
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const port = server.address().port;
  const run = (p, ms) => new Promise((resolve) => execFile(process.execPath, ['-e', BANNER_PROBE_SCRIPT, String(p), String(ms)], (e, out) => resolve({ code: e ? e.code : 0, out: out.trim() })));
  assert.deepEqual(await run(port, 3000), { code: 0, out: 'SSH-2.0-QA' });
  server.close();
  await new Promise((r) => server.once('close', r));
  assert.equal((await run(port, 300)).code, 1);
});

test('Q11 teardown: a log locked by its live writer (sshd -E) is copied after the kill, not an error', async () => {
  const { mkdtempSync, readFileSync: rf } = await import('node:fs');
  const { tmpdir } = await import('node:os');
  const { newSummary } = await import('./lib/summary.mjs');
  const { teardownRun } = await import('./run.mjs');
  const ev = mkdtempSync(`${tmpdir()}/qa-r6e-`);
  let killed = false;
  const order = [];
  const agent = { call: async (m, p = {}) => {
    if (m === 'exec') return { status: 0, stdout: JSON.stringify(['gui-g1.log', 'sshd.log']), stderr: '' };
    if (m === 'readFile') { order.push(`read:${p.file.split(/[\\/]/).pop()}:${killed}`); if (p.file.endsWith('sshd.log') && !killed) throw new Error("EBUSY: resource busy or locked, open 'sshd.log'"); return 'log'; }
    if (m === 'teardownRecorded') { killed = true; return { killed: [{ pid: 9, role: 'sshd' }], refused: [] }; }
    if (m === 'remove') return true;
    if (m === 'table') return [];
    return null;
  } };
  const s = newSummary({ runId: '20261021t000000z', platform: 'windows', host: 'h' });
  await teardownRun({ summary: s, agent, jobs: { listQaJobs: async () => [], remove: async () => {} }, host: { platform: 'windows' }, layout: { logs: 'C:\\r\\logs' }, runDirs: ['C:\\r'], qaRoots: ['C:\\r'], localEvidence: ev, prodSnapshot: async () => null });
  assert.deepEqual(order, ['read:gui-g1.log:false', 'read:sshd.log:false', 'read:sshd.log:true']);
  assert.deepEqual(s.teardown.errors, []);
  assert.equal(rf(`${ev}/runlog-sshd.log`, 'utf8'), 'log');
});

const fsError = (code) => Object.assign(new Error(`${code}: x`), { code });

test('waitLog read: EBUSY, EPERM and ENOENT read as not yet written', () => {
  assert.deepEqual([...LOG_NOT_YET_CODES], ['EBUSY', 'EPERM', 'ENOENT']);
  for (const code of LOG_NOT_YET_CODES) assert.equal(readLogText('C:\\r\\logs\\sshd.log', () => { throw fsError(code); }), '', code);
  assert.equal(readLogText('f', () => 'Server listening'), 'Server listening');
});

test('waitLog read: any other error is rethrown, never a silent timeout', () => {
  for (const code of ['EACCES', 'EISDIR', 'EMFILE', undefined]) {
    assert.throws(() => readLogText('f', () => { throw code ? fsError(code) : new TypeError('bad encoding'); }), (e) => e.code === code || e instanceof TypeError, String(code));
  }
});

test('waitLog read: a real missing file is not yet written, a real directory is rethrown', async () => {
  const { mkdtempSync } = await import('node:fs');
  const { tmpdir } = await import('node:os');
  const dir = mkdtempSync(`${tmpdir()}/qa-r6e-log-`);
  assert.equal(readLogText(`${dir}/absent.log`), '');
  assert.throws(() => readLogText(dir), (e) => e.code === 'EISDIR');
});

test('Q11 teardown: a Windows helper whose --root is a verbatim \\\\?\\ path is still this run\'s helper', async () => {
  const { findSshHelpers, sshHelperDigest } = await import('./lib/sshd.mjs');
  const host = 'qa-sshd-20261021t172546z';
  const exe = 'C:\\Users\\sook\\.ferryx\\versions\\2026.1017.1\\bin\\ferryx-remote-helper.exe';
  const root = `C:\\Users\\sook\\.ferryx\\r\\2026.1017.1\\${sshHelperDigest(host)}`;
  const row = { pid: 4040, executable: exe, commandLine: `"${exe}" "daemon" "--root" "\\\\?\\${root}" "--host-id" "${host}"` };
  assert.deepEqual(findSshHelpers([row], { platform: 'windows', home: 'C:\\Users\\sook', hostId: host }), [{ pid: 4040, executable: exe, root }]);
  assert.deepEqual(findSshHelpers([{ ...row, commandLine: row.commandLine.replace(host, 'omarchy') }], { platform: 'windows', home: 'C:\\Users\\sook', hostId: host }), []);
});

test('Q12: the stream Exit message is recorded per session and wakes its waiters', () => {
  const route = createOutputRoute({ platform: 'linux', output: new Map(), waiters: new Set(), send: async () => {} });
  const woke = [];
  route.exitWaiters.add({ sessionId: 'S1', resolve: (v) => woke.push(['S1', v]) });
  route.exitWaiters.add({ sessionId: 'S2', resolve: (v) => woke.push(['S2', v]) });
  route.route({ type: 'exit', sessionId: 'S1', exitCode: null });
  assert.deepEqual(woke, [['S1', { exitCode: null }]]);
  assert.deepEqual(route.exits.get('S1'), { exitCode: null });
  assert.equal(route.exitWaiters.size, 1, 'S2 still waits');
  assert.equal(SESSION_EXIT_TIMEOUT_MS, 10_000);
});

function q12World({ exitEvent = { exitCode: null }, promptTimedOut = false } = {}) {
  const order = [];
  const conn = {
    call: async (f) => { order.push(f.type); return f.type === 'spawn' ? { type: 'spawnOk', sessionId: 'S9' } : { type: `${f.type}Ok` }; },
    describe: async () => { order.push('describe'); return { type: 'describeLiveSessionsOk', sessions: order.includes('exitEvent') ? [] : [{ backendId: 'S9', kind: 'local', localChildPid: 55, readerOpen: true }] }; },
    waitOutput: async () => { order.push('prompt'); return promptTimedOut ? { timedOut: true, text: '' } : { text: 'PS C:\\> ' }; },
    waitSessionExit: async () => { await Promise.resolve(); order.push('exitEvent'); return exitEvent; },
  };
  const agent = { call: async (m) => {
    if (m === 'waitExit') return { pid: 55, exitedAt: 1 };
    if (m === 'controlOpen') return { handle: 'c' };
    if (m === 'call') return { ok: true, live_local: 0 };
    return true;
  } };
  const ctx = { agent, platform: 'windows', workspaceId: 'ws', repoDir: 'C:\\r', layout: { runtime: 'C:\\r\\run' }, evidenceDir: '/e', writeEvidence: async () => {}, hooks: {}, state: { conn, daemon: { pid: 1 } } };
  return { ctx, order };
}

test('Q12 waits for the prompt before exit and asserts the inventory only after the Exit event', async () => {
  const { ctx, order } = q12World();
  const result = { id: 'Q12', assertions: [], evidence: [] };
  await Q12(ctx, result);
  assert.ok(order.indexOf('prompt') < order.indexOf('write'), 'prompt before exit is typed');
  const firstDescribeAfterExit = order.indexOf('describe', order.indexOf('exitEvent'));
  assert.ok(firstDescribeAfterExit > order.indexOf('exitEvent'));
  const by = Object.fromEntries(result.assertions.map((a) => [a.name, a.ok]));
  assert.equal(by['attached stream delivered the session Exit message'], true);
  assert.equal(by['exited local PTY reports readerOpen=false (or is gone)'], true);
  const late = q12World({ exitEvent: { timedOut: true } });
  const r2 = { id: 'Q12', assertions: [], evidence: [] };
  await Q12(late.ctx, r2);
  assert.equal(r2.assertions.find((a) => a.name === 'attached stream delivered the session Exit message').ok, false);
});

test('Q13: waitLog re-reads the content on a bounded interval even when no change event ever arrives', async () => {
  let content = '';
  const timers = [];
  const pending = waitForContent({
    read: () => content, re: /^FERRYX_DAEMON_READY$/m, deadlineMs: 60_000, pollMs: 250,
    subscribe: () => () => {}, // a watcher that never fires
    setTimer: (fn, ms) => { timers.push({ fn, ms }); return timers.length; }, clearTimer: (id) => { timers[id - 1].cleared = true; },
  });
  assert.equal(timers.length, 1);
  assert.equal(timers[0].ms, 250);
  content = 'FERRYX_DAEMON_READY\n';
  timers[0].fn();
  assert.equal(await pending, 'FERRYX_DAEMON_READY');
  assert.equal(timers[0].cleared, true, 'the poll stops at the match');
});

test('Q13: waitLog still resolves on a change event and returns null at its deadline', async () => {
  let fire;
  let content = '';
  const p = waitForContent({ read: () => content, re: /x/, deadlineMs: 60_000, pollMs: 60_000, subscribe: (t) => { fire = t; return () => {}; } });
  content = 'x';
  fire();
  assert.equal(await p, 'x');
  assert.equal(await waitForContent({ read: () => '', re: /x/, deadlineMs: 5, pollMs: 60_000 }), null);
});

test('Q13: the legacy daemon is recorded at job start: the one new baseline --daemon row, identity-checked', async () => {
  const exe = 'C:\\b\\ferryx.exe';
  const rows = [
    { pid: 1, executable: exe, commandLine: `"${exe}" --daemon` },
    { pid: 2, executable: exe, commandLine: `"${exe}" --daemon-supervisor` },
    { pid: 3, executable: 'C:\\other\\ferryx.exe', commandLine: 'ferryx.exe --daemon' },
  ];
  assert.deepEqual(legacyDaemonRows(rows, exe).map((r) => r.pid), [1]);
  let table = rows;
  const recorded = [];
  const ctx = { agent: { call: async (m, p) => {
    if (m === 'table') { const t = table; table = [...rows, { pid: 7, executable: exe.toUpperCase(), commandLine: `"${exe}" --daemon` }]; return t; }
    if (m === 'record') { recorded.push(p); return { pid: p.pid, role: p.role }; }
    throw new Error(m);
  } } };
  const r = await recordLegacyJobProcess(ctx, exe, new Set([1]), { deadlineMs: 1_000, intervalMs: 1 });
  assert.equal(r.recorded.pid, 7);
  assert.deepEqual(recorded, [{ pid: 7, role: 'daemon-legacy', executable: exe }]);
  const none = await recordLegacyJobProcess({ agent: { call: async () => rows } }, exe, new Set([1]), { deadlineMs: 5, intervalMs: 1 });
  assert.equal(none.recorded, null);
  assert.match(none.reason, /no new/);
});

test('Q13 records the legacy pid between the install and the readiness wait', () => {
  const src = readFileSync(new URL('./scenarios/q10-q13.mjs', import.meta.url), 'utf8');
  const install = src.indexOf('ctx.jobs.installLegacy(');
  const rec = src.indexOf('recordLegacyJobProcess(ctx, legacyExecutable');
  const ready = src.indexOf('await awaitLegacyReady(ctx, { retiredPid');
  assert.ok(install > 0 && install < rec && rec < ready);
});

test('requireOwner blocks OWNER_UNAVAILABLE with the recorded reason; Q07 then never dereferences null', async () => {
  const result = { id: 'Q07', assertions: [], evidence: [] };
  const ctx = { state: { conn: null, daemon: null, ownerLost: 'Q06 threw and owner recovery failed: OWNER_GONE' } };
  assert.equal(requireOwner(ctx, result), false);
  assert.equal(result.status, 'BLOCKED');
  assert.equal(result.barrier.kind, 'OWNER_UNAVAILABLE');
  assert.match(result.barrier.detail.reason, /Q06 threw/);
  const q07 = { id: 'Q07', assertions: [], evidence: [] };
  await Q07({ state: {} }, q07);
  assert.equal(q07.status, 'BLOCKED');
  assert.equal(requireOwner({ state: { conn: {}, daemon: { pid: 1 } } }, { id: 'x', assertions: [], evidence: [] }), true);
});

test('a failed owner recovery invalidates the state so later scenarios block', async () => {
  const closed = [];
  const ctx = { artifacts: { A: {} }, state: { conn: { close: async () => closed.push('c') }, daemon: { pid: 1 }, localA: 'S' } };
  const result = { id: 'Q06', assertions: [], evidence: [] };
  await recoverAfterScenarioException(ctx, result, { writeFileSync: () => {}, localEvidence: '/e', recover: async () => ({ ok: false, code: 'OWNER_GONE' }) });
  assert.equal(ctx.state.conn, null);
  assert.deepEqual(closed, ['c']);
  assert.match(ctx.state.ownerLost, /^Q06 threw and owner recovery failed: OWNER_GONE/);
  const ok = { artifacts: { A: {} }, state: { conn: { close: async () => {} } } };
  await recoverAfterScenarioException(ok, { id: 'Q01', assertions: [], evidence: [] }, { writeFileSync: () => {}, localEvidence: '/e', recover: async () => ({ ok: true }) });
  assert.ok(ok.state.conn, 'a recovered owner is kept');
});

test('per-scenario cleanup kills only GUIs the scenario recorded, alive, and not the current GUI', async () => {
  const entries = [{ pid: 1, role: 'gui-g1' }, { pid: 2, role: 'gui-g2' }, { pid: 3, role: 'gui-g3' }, { pid: 4, role: 'gui-g4' }, { pid: 9, role: 'daemon-A' }];
  const killed = [];
  const agent = { call: async (m, p) => {
    if (m === 'recorded') return entries;
    if (m === 'check') return p.pid === 4 ? 'gone' : 'match';
    if (m === 'kill') { killed.push(p); return true; }
    if (m === 'waitExit') return { pid: p.pid, exitedAt: 1 };
    throw new Error(m);
  } };
  const before = await recordedGuiPids({ call: async () => entries.slice(0, 1) });
  assert.deepEqual([...before], [1]);
  const written = [];
  const result = { id: 'Q10', assertions: [], evidence: [] };
  const ended = await endScenarioGuis({ agent, state: { gui: { pid: 3 } } }, result, before, { writeFileSync: (f, t) => written.push([f, t]), localEvidence: '/e' });
  assert.deepEqual(ended.map((e) => e.pid), [2]);
  assert.deepEqual(killed, [{ pid: 2, signal: 'SIGKILL' }]);
  assert.deepEqual(result.evidence, ['/e/Q10-leftover-guis.json']);
  const none = { id: 'Q11', assertions: [], evidence: [] };
  await endScenarioGuis({ agent, state: { gui: { pid: 2 } } }, none, new Set([1, 3]), { writeFileSync: () => {}, localEvidence: '/e' });
  assert.deepEqual(none.evidence, []);
});

test('run.mjs ends scenario GUIs in a finally after the exception recovery', () => {
  const src = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8');
  const loop = src.slice(src.indexOf('await RUNNERS[result.id](ctx, result);'));
  assert.ok(loop.indexOf('recoverAfterScenarioException(ctx, result') < loop.indexOf('} finally {'));
  assert.ok(loop.indexOf('} finally {') < loop.indexOf('endScenarioGuis(ctx, result, guisBefore'));
});

// ---- Windows GUI driver: input mutex, go file, Escape, one retry, close request, null safety ----

const focus = (o = {}) => JSON.stringify({ kind: 'focus', focusHwnd: 0x5012c, rootHwnd: 0x5012a, processName: 'explorer.exe', rootClass: '#32770', focusedClass: 'Edit', parentClass: 'ComboBox', foreground: true, ...o });

/**
 * One interactive desktop: each tag has its own observer events list, and every MahoRD call is
 * logged with the tag that currently holds the input section. Scripts decide outcomes per attempt.
 */
function desktop({ winR = () => [focus()], afterEscape = () => [focus({ processName: 'WindowsTerminal.exe', rootClass: 'CASCADIA', focusedClass: 'CASCADIA' })], typedMatch = () => true, cmdStarted = () => true, gate = null } = {}) {
  const layout = isolationLayout('C:\\r', 'windows');
  const events = new Map();
  const written = new Map();
  const exists = new Set();
  const log = [];
  const attempts = new Map();
  let active = null;
  let seq = 0;
  const push = (tag, line) => { const s = ++seq; const rec = JSON.parse(line); events.get(tag).push(JSON.stringify({ ...rec, seq: s })); };
  const tagOf = (file) => /observer-([a-z0-9]+)\./i.exec(file)?.[1] ?? /launch-([a-z0-9]+)\./i.exec(file)?.[1] ?? null;
  const agent = { call: async (m, p = {}) => {
    if (m === 'writeFile') {
      written.set(p.file, p.content);
      const tag = tagOf(p.file);
      // The args file is written first inside the input section: from then on input belongs to tag.
      if (/observer-[a-z0-9]+\.args\.json$/i.test(p.file)) { active = tag; events.set(tag, []); attempts.set(tag, (attempts.get(tag) ?? 0) + 1); }
      if (/\.arm$/.test(p.file)) { log.push(['arm', tag]); events.get(tag).push(JSON.stringify({ kind: 'armed', pid: 9100, seq: 0 })); }
      const t = /\.target-(\d+)$/.exec(p.file);
      if (t) events.get(tag).push(JSON.stringify({ kind: 'latched', focusHwnd: Number(t[1]), seq: 0 }));
      if (/\.stop$/.test(p.file)) { log.push(['stop', tag]); if (active === tag) active = null; }
      if (/\.go$/.test(p.file)) log.push(['go']);
      return true;
    }
    if (m === 'exec' && p.command === 'schtasks') return { status: 0, stdout: 'SUCCESS', stderr: '' };
    if (m === 'exec') return { status: 0, stdout: 'C:\\R\\LAUNCH.CMD\r\n', stderr: '' };
    if (m === 'waitPath') {
      if (/ready\.json$/.test(p.file)) return true;
      if (/\.err$/.test(p.file)) return new Promise(() => {});
      if (/cmd\.started$/.test(p.file)) {
        const tag = tagOf(p.file);
        const ok = cmdStarted(tag, attempts.get(tag));
        if (ok) exists.add(p.file);
        log.push(['cmdStarted', tag, ok]);
        return ok;
      }
      if (/launcher-([a-z0-9]+)\.started$/i.test(p.file) && gate) return gate();
      return true;
    }
    if (m === 'exists') return exists.has(p.file);
    if (m === 'remove') { const tag = tagOf(p.target); if (/events\.jsonl$/.test(p.target)) events.set(tag, []); return true; }
    if (m === 'readFile') {
      if (/ready\.json$/.test(p.file)) { const tag = tagOf(p.file); return JSON.stringify({ pid: 9100, sessionId: 1, activeConsoleSessionId: 1, nonce: JSON.parse(written.get(windowsLaunchFiles(layout, tag).observerArgs)).nonce, hooks: 3, start: '1' }); }
      if (/events\.jsonl$/.test(p.file)) return (events.get(tagOf(p.file)) ?? []).join('\n');
      if (/gui-([a-z0-9]+)\.json$/i.test(p.file)) return JSON.stringify({ pid: 4000 + seq, jobAssigned: true, noBreakaway: false, launcherPid: 7 });
      if (/closed\.json$/.test(p.file)) return '{"closeMainWindow":true}';
      return null;
    }
    if (m === 'probe') return { executable: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe', startTime: '1' };
    if (m === 'waitLog') {
      const re = new RegExp(p.pattern, p.flags);
      const hit = (events.get(tagOf(p.file)) ?? []).map((l) => re.exec(l)).find(Boolean);
      return hit ? { matched: hit[0] } : { matched: null, timedOut: true };
    }
    if (m === 'waitExit') return { pid: p.pid, exitedAt: 1 };
    if (m === 'record') return { pid: p.pid, startTime: 1 };
    if (m === 'kill') return true;
    throw new Error(`unexpected ${m}`);
  } };
  const mahord = {
    click: async () => log.push(['click', active]),
    hotkey: async (keys) => {
      log.push(['hotkey', active, keys.join('+')]);
      if (keys.join('+') === 'Meta+r') for (const l of winR(active, attempts.get(active))) push(active, l);
    },
    type: async (text) => {
      log.push(['type', active]);
      push(active, JSON.stringify({ kind: 'text', match: typedMatch(active, attempts.get(active)), length: text.length }));
    },
    key: async (key) => {
      log.push(['key', active, key]);
      if (key === 'Escape') for (const l of afterEscape(active, attempts.get(active))) push(active, l);
    },
    screenshot: async () => Buffer.from('png'),
  };
  const gui = createGuiDriver({ host: { platform: 'windows' }, agent, runId: RUN, layout, allowedRoots: ['C:\\r'], forbidden: [], mahord });
  return { gui, log, written, layout, artifact: { guiExecutable: 'C:\\r\\ferryx.exe' } };
}

test('Windows launches never interleave their keyboard sections, even when started concurrently', async () => {
  const d = desktop();
  await Promise.all([d.gui.launch(d.artifact, { tag: 'a' }), d.gui.launch(d.artifact, { tag: 'b' })]);
  const input = d.log.filter((l) => ['click', 'hotkey', 'type', 'key'].includes(l[0]));
  assert.deepEqual(input.map((l) => l[1]), ['a', 'a', 'a', 'a', 'a', 'b', 'b', 'b', 'b', 'b']);
  const cmd = d.log.findIndex((l) => l[0] === 'cmdStarted' && l[1] === 'a');
  const armB = d.log.findIndex((l) => l[0] === 'arm' && l[1] === 'b');
  assert.ok(cmd < armB, 'the next launch arms only after the previous one executed its .cmd');
});

test('launchBatch types each launch alone, writes one go file after every .cmd executed, then finishes all', async () => {
  const d = desktop({ gate: () => false });
  const order = [];
  const real = d.gui.launchBatch;
  const settled = await real([{ artifact: d.artifact, options: { tag: 'a' } }, { artifact: d.artifact, options: { tag: 'b' } }, { artifact: d.artifact, options: { tag: 'c', env: { X: '1' } } }].map((x) => { order.push(x.options.tag); return x; }));
  // gate() always returns false (launcher-not-started): all three still waited for their launcher
  // after the single go write, which proves the go precedes every finish.
  const go = d.log.findIndex((l) => l[0] === 'go');
  assert.equal(d.log.filter((l) => l[0] === 'go').length, 1);
  assert.ok(d.log.filter((l) => l[0] === 'cmdStarted').every((l) => d.log.indexOf(l) < go));
  assert.equal(settled.length, 3);
  const cmds = [...d.written.keys()].filter((f) => /launch[abc]\.cmd$/.test(f)).map((f) => d.written.get(f));
  assert.equal(cmds.length, 3);
  for (const c of cmds) assert.match(c, /if exist "C:\\r\\launchbatch1\.go" goto go/);
});

test('launchBatch resolves like allSettled with the GUIs once go is written', async () => {
  const d = desktop();
  const settled = await d.gui.launchBatch([{ artifact: d.artifact, options: { tag: 'a' } }, { artifact: d.artifact, options: { tag: 'b' } }]);
  assert.deepEqual(settled.map((s) => s.status), ['fulfilled', 'fulfilled']);
  assert.deepEqual(settled.map((s) => s.value.tag), ['a', 'b']);
});

test('a batch .cmd waits, bounded, for its go file before starting the launcher; a plain .cmd does not', () => {
  const plain = windowsLaunchCmdContent({ cmdStarted: 'C:\\r\\s', command: 'powershell x' });
  assert.equal(plain, '@echo off\r\necho started> "C:\\r\\s"\r\nstart "" powershell x\r\n');
  const batch = windowsLaunchCmdContent({ cmdStarted: 'C:\\r\\s', command: 'powershell x', goFile: 'C:\\r\\b.go' });
  const lines = batch.split('\r\n');
  assert.equal(lines[1], 'echo started> "C:\\r\\s"', 'receipt first');
  assert.ok(lines.indexOf('if exist "C:\\r\\b.go" goto go') < lines.indexOf('start "" powershell x'));
  assert.ok(lines.includes(`if %n% geq ${GO_WAIT_SECONDS} exit /b 1`));
});

test('a failed Run dialog is dismissed with Escape only while an explorer #32770 holds the foreground, then retried once', async () => {
  // Attempt 1: Win+R lands on the explorer error box (#32770, Button focused); attempt 2 succeeds.
  const d = desktop({ winR: (tag, n) => (n === 1 ? [focus({ focusedClass: 'Button' })] : [focus()]) });
  const g = await d.gui.launch(d.artifact, { tag: 'a' });
  assert.equal(g.tag, 'a');
  const keys = d.log.filter((l) => l[0] === 'key').map((l) => l[2]);
  assert.deepEqual(keys, ['Escape', 'Enter']);
  const receipt = JSON.parse(d.written.get(windowsLaunchFiles(d.layout, 'a').receipt));
  assert.equal(receipt.attempt, 2);
  assert.equal(receipt.priorAttempts[0].failedStage, 'run-dialog-not-identified');
  assert.equal(receipt.priorAttempts[0].dismissal.clear, true);
});

test('no Escape when the foreground is not an explorer dialog, and no retry when the dismissal is unconfirmed', async () => {
  const d = desktop({ winR: () => [focus({ processName: 'WindowsTerminal.exe', rootClass: 'CASCADIA', focusedClass: 'CASCADIA' })] });
  await assert.rejects(d.gui.launch(d.artifact, { tag: 'a' }), (e) => e.stage === 'run-dialog-not-identified' && e.receipt.attempt === 2);
  assert.ok(!d.log.some((l) => l[0] === 'key'), 'never a blind key');
  // Escape that the observer never follows: not clear, so no second attempt.
  const stuck = desktop({ winR: () => [focus({ focusedClass: 'Button' })], afterEscape: () => [] });
  await assert.rejects(stuck.gui.launch(stuck.artifact, { tag: 'a' }), (e) => e.stage === 'run-dialog-not-identified' && e.receipt.attempt === 1 && e.receipt.dismissal.clear === false);
  assert.equal(stuck.log.filter((l) => l[0] === 'key').length, 1);
});

test('exactly one retry, and none once the .cmd executed or after a later stage', async () => {
  const twice = desktop({ winR: () => [focus({ focusedClass: 'Button' })] });
  await assert.rejects(twice.gui.launch(twice.artifact, { tag: 'a' }), (e) => e.receipt.attempt === 2);
  assert.equal(twice.log.filter((l) => l[0] === 'arm').length, 2);
  const typed = desktop({ typedMatch: (tag, n) => n === 2 });
  assert.equal((await typed.gui.launch(typed.artifact, { tag: 'a' })).tag, 'a');
  const noCmd = desktop({ cmdStarted: () => false });
  await assert.rejects(noCmd.gui.launch(noCmd.artifact, { tag: 'a' }), (e) => e.stage === 'cmd-not-executed' && e.receipt.attempt === 1);
  assert.equal(noCmd.log.filter((l) => l[0] === 'arm').length, 1, 'cmd-not-executed is never retried');
});

test('lastObserverRecord / isExplorerDialogForeground read the newest focus record', () => {
  const text = [JSON.stringify({ kind: 'armed', seq: 0 }), focus({ seq: 1 }), JSON.stringify({ kind: 'text', match: true, seq: 2 }), '{"kind":"fo'].join('\n');
  assert.equal(lastObserverRecord(text).seq, 1);
  assert.ok(isExplorerDialogForeground(lastObserverRecord(text)));
  assert.ok(!isExplorerDialogForeground({ ...JSON.parse(focus()), foreground: false }));
  assert.equal(lastObserverRecord(''), null);
});

test('Windows quit asks the launcher to close the recorded GUI; no hotkey, and the close receipt is returned', async () => {
  const d = desktop();
  const q = await d.gui.quit({ tag: 'g7', pid: 77 });
  assert.equal(d.written.get(windowsCloseFiles(d.layout, 'g7').request), '1');
  assert.ok(!d.log.some((l) => l[0] === 'hotkey'));
  assert.equal(q.pid, 77);
  assert.equal(q.close, '{"closeMainWindow":true}');
  assert.deepEqual(windowsCloseFiles({ root: 'C:\\r' }, 'g7'), { request: 'C:\\r\\gui-g7.close.request', receipt: 'C:\\r\\gui-g7.closed.json' });
  const launcher = readFileSync(new URL('./fixtures/windows-gui-launcher.ps1', import.meta.url), 'utf8');
  assert.match(launcher, /"gui-\$Tag\.close\.request"/);
  assert.match(launcher, /\$proc\.CloseMainWindow\(\)/);
  assert.match(launcher, /"gui-\$Tag\.closed\.json"/);
});

test('quit, kill and forceTerminate of no GUI are skipped, not a null dereference', async () => {
  const d = desktop();
  for (const fn of ['quit', 'kill', 'forceTerminate']) {
    assert.deepEqual(await d.gui[fn](null), { skipped: 'no GUI' }, fn);
    assert.deepEqual(await d.gui[fn](undefined), { skipped: 'no GUI' }, fn);
  }
});

test('Q01: a GUI that outlives its quit is killed by recorded pid before the relaunch', () => {
  const src = readFileSync(new URL('./scenarios/q00-q03.mjs', import.meta.url), 'utf8');
  const body = src.slice(src.indexOf('export async function surviveGuiEnd'));
  assert.ok(body.indexOf("ctx.agent.call('kill', { pid: ctx.state.gui.pid") < body.indexOf('ctx.gui.launch(artifact)'));
});

// ---- dialog sweep preflight ----

const win = (o) => ({ hwnd: 101, processName: 'explorer.exe', className: '#32770', topLevel: true, title: '실행', foreground: false, ...o });

test('sweep selects only explorer.exe top-level #32770 windows titled as the Run dialog or an explorer error box', async () => {
  const { selectSweepTargets, sweepPathPrefixes } = await import('./lib/gui.mjs');
  const prefixes = sweepPathPrefixes('C:\\Users\\sook\\ferryx-wt\\qaruns', 'C:\\Users\\sook\\FERRYX~1\\qaruns');
  const windows = [
    win({ hwnd: 1, title: '실행' }),
    win({ hwnd: 2, title: '위치를 사용할 수 없습니다.' }),
    win({ hwnd: 3, title: 'Run' }),
    win({ hwnd: 4, title: 'C:\\Users\\sook\\FERRYX~1\\qaruns\\202610~3\\LAUNCH~2.CMD' }),
    win({ hwnd: 5, title: '실행', processName: 'notepad.exe' }), // a foreign app's dialog with the same title
    win({ hwnd: 6, title: '파일 삭제' }), // an explorer dialog that is not ours
    win({ hwnd: 7, title: 'C:\\Windows\\x.cmd' }), // a path outside the QA run roots
    win({ hwnd: 8, title: '실행', className: 'ApplicationFrameWindow' }),
    win({ hwnd: 9, title: '실행', topLevel: false }),
    win({ hwnd: 0, title: '실행' }),
  ];
  const { close, kept } = selectSweepTargets(windows, { pathPrefixes: prefixes });
  assert.deepEqual(close.map((c) => [c.hwnd, c.kind]), [[1, 'run-dialog'], [2, 'error-box'], [3, 'run-dialog'], [4, 'error-box-path']]);
  assert.deepEqual(kept.map((k) => k.hwnd), [5, 6, 7, 8, 9, 0]);
  assert.equal(kept.find((k) => k.hwnd === 5).reason, 'not explorer.exe');
  assert.deepEqual(selectSweepTargets([], {}), { close: [], kept: [] });
});

function sweepWorld({ windows = [], sessionId = 1, result = null, listed = true, sweepErr = null } = {}) {
  const calls = [];
  const written = new Map();
  const agent = { call: async (m, p = {}) => {
    calls.push([m, p.command ?? p.file, p.args?.[0]]);
    if (m === 'writeFile') { written.set(p.file, p.content); return true; }
    if (m === 'exec' && p.command === 'schtasks') return { status: 0, stdout: 'SUCCESS', stderr: '' };
    if (m === 'exec') return { status: 0, stdout: 'C:\\Users\\sook\\FERRYX~1\\qaruns\\202610~4\\DIALOG~1.PS1\r\n', stderr: '' };
    if (m === 'waitPath') {
      if (/candidates\.json$/.test(p.file)) return listed;
      if (/dialog-sweep\.err$/.test(p.file)) return sweepErr ? true : new Promise(() => {});
      if (/result\.json$/.test(p.file)) return true;
    }
    if (m === 'readFile') {
      if (/candidates\.json$/.test(p.file)) return JSON.stringify({ pid: 3, sessionId, activeConsoleSessionId: 1, windows });
      if (/dialog-sweep\.err$/.test(p.file)) return sweepErr;
      if (/result\.json$/.test(p.file)) {
        const asked = JSON.parse(written.get([...written.keys()].find((f) => /close\.json$/.test(f)))).hwnds;
        return JSON.stringify(result ?? { requested: true, closed: asked.map((hwnd) => ({ hwnd, verified: true, posted: true, closed: true })) });
      }
    }
    throw new Error(`unexpected ${m}`);
  } };
  const layout = isolationLayout('C:\\Users\\sook\\ferryx-wt\\qaruns\\' + RUN, 'windows');
  const mahord = { click: async () => calls.push(['mahord']), hotkey: async () => calls.push(['mahord']), key: async () => calls.push(['mahord']), type: async () => calls.push(['mahord']), screenshot: async () => null };
  const gui = createGuiDriver({ host: { platform: 'windows' }, agent, runId: RUN, layout, allowedRoots: ['C:\\'], forbidden: [], mahord });
  return { gui, calls, written };
}

test('sweep closes exactly the selected hwnds through a per-run QA task, records them, and removes the task', async () => {
  const { isQaJobName } = await import('./lib/guards.mjs');
  const w = sweepWorld({ windows: [win({ hwnd: 11, title: '실행', foreground: true }), win({ hwnd: 12, title: '위치를 사용할 수 없습니다.' }), win({ hwnd: 13, processName: 'Code.exe', title: 'Run' })] });
  const r = await w.gui.sweepExplorerDialogs();
  assert.equal(r.error, null);
  assert.ok(isQaJobName(r.task, RUN, 'windows'), r.task);
  const closeFile = [...w.written.keys()].find((f) => /dialog-sweep\.close\.json$/.test(f));
  assert.deepEqual(JSON.parse(w.written.get(closeFile)), { hwnds: [11, 12] });
  assert.deepEqual(r.selected.map((s) => s.hwnd), [11, 12]);
  assert.deepEqual(r.kept.map((k) => k.hwnd), [13]);
  assert.deepEqual(r.result.closed.map((c) => [c.hwnd, c.closed]), [[11, true], [12, true]]);
  const tasks = w.calls.filter((c) => c[1] === 'schtasks').map((c) => c[2]);
  assert.deepEqual(tasks, ['/Create', '/Run', '/End', '/Delete']);
  assert.ok([...w.written.keys()].some((f) => /dialog-sweep\.receipt\.json$/.test(f)), 'receipt written');
  assert.ok(!w.calls.some((c) => c[0] === 'mahord'), 'no keyboard or mouse input');
});

test('sweep never closes anything when it cannot list, runs in the wrong session, or the fixture errors; the task is still removed', async () => {
  for (const [opts, re] of [
    [{ listed: false }, /no .*candidates\.json within/],
    [{ sessionId: 0, windows: [win({ hwnd: 11 })] }, /sweep ran in session 0/],
    [{ sweepErr: 'Add-Type failed', listed: new Promise(() => {}) }, /sweep failed: Add-Type failed/],
  ]) {
    const w = sweepWorld(opts);
    const r = await w.gui.sweepExplorerDialogs();
    assert.match(r.error, re);
    assert.ok(![...w.written.keys()].some((f) => /close\.json$/.test(f)), 'no close request');
    assert.deepEqual(w.calls.filter((c) => c[1] === 'schtasks').map((c) => c[2]).slice(-2), ['/End', '/Delete']);
  }
});

test('sweep fixture posts WM_CLOSE only to a re-verified explorer #32770 hwnd with the same title', () => {
  const ps = readFileSync(new URL('./fixtures/windows-dialog-sweep.ps1', import.meta.url), 'utf8');
  assert.match(ps, /PostMessage\(h, 0x0010,/, 'WM_CLOSE');
  assert.match(ps, /\$verified = \(\$null -ne \$was\) -and \[DialogSweep\]::IsWindow\(\$h\) -and \(\[DialogSweep\]::Cls\(\$h\) -eq '#32770'\) -and \(\[DialogSweep\]::Proc\(\$h\) -ieq 'explorer\.exe'\) -and \(\[DialogSweep\]::Title\(\$h\) -eq \$was\.title\)/);
  assert.match(ps, /if \(\$verified\) \{\s*\$posted = \[DialogSweep\]::Close\(\$h\)/);
  assert.equal((ps.match(/::Close\(/g) ?? []).length, 1, 'one close site');
  const run = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8');
  assert.ok(run.indexOf('summary.dialogSweep = await gui.sweepExplorerDialogs()') < run.indexOf('const ctx = {'), 'sweep before any scenario launches');
});

test('dialog sweep records foreground-stealing non-QA windows report-only', async () => {
  const { foreignWindows } = await import('./lib/gui.mjs');
  const out = foreignWindows({ visibleWindows: [
    { hwnd: 1, processName: 'Aside.exe', className: 'Chrome_WidgetWin_1', title: 'New Tab', foreground: false },
    { hwnd: 2, processName: 'RobloxStudioBeta.exe', className: 'Qt5QWindowIcon', title: 'robango-island', foreground: true },
    { hwnd: 3, processName: 'ferryx.exe', className: 'X', title: 'Ferryx', foreground: false },
  ] });
  assert.deepEqual(out.map((w) => w.hwnd), [2, 1]);
  assert.equal(foreignWindows({}).length, 0);
  const ps1 = readFileSync(new URL('./fixtures/windows-dialog-sweep.ps1', import.meta.url), 'utf8');
  assert.equal((ps1.match(/::Close\(/g) ?? []).length, 1, 'WM_CLOSE only in the verified explorer-dialog path');
  assert.ok(!/Stop-Process|taskkill|\.Kill\(/i.test(ps1), 'never kills');
});

test('Q11 Windows SSH project record carries platform windows (projects.rs resolve defaults posix)', async () => {
  const { sshInventory } = await import('./lib/sshd.mjs');
  const root = 'C:\\Users\\sook\\ferryx-wt\\qaruns\\x\\ssh-repo';
  const win = sshInventory({ hostId: 'h', port: 1, user: 'u', identityFile: 'k', repoRoot: root, platform: 'windows' });
  assert.equal(win.projects[win.workspaceId].platform, 'windows');
  const lin = sshInventory({ hostId: 'h', port: 1, user: 'u', identityFile: 'k', repoRoot: '/tmp/x' });
  assert.equal('platform' in lin.projects[lin.workspaceId], false, 'posix records unchanged');
});

test('Q06w launcher keeps the -NoBreakaway switch distinct from its result (PowerShell names are case-insensitive)', () => {
  const ps1 = readFileSync(new URL('./fixtures/windows-gui-launcher.ps1', import.meta.url), 'utf8');
  const assigned = [...ps1.matchAll(/^\s*\$(\w+)\s*=/gm)].map((m) => m[1].toLowerCase());
  assert.ok(!assigned.includes('nobreakaway'), 'no assignment to $NoBreakaway / $noBreakaway');
  assert.match(ps1, /if \(\$NoBreakaway\) \{ \$forbidden = \[QaJob\]::ForbidBreakaway\(\$job\) \}/);
  assert.match(ps1, /noBreakaway = \$forbidden;/);
});

test('windows typed lines end with a single CR (ConPTY maps LF to Ctrl+Enter)', async () => {
  const { enterKey, counterCommand, echoCommand } = await import('./scenarios/common.mjs');
  assert.equal(enterKey('windows'), '\r');
  assert.equal(enterKey('linux'), '\n');
  for (const line of [counterCommand('windows', 'T'), echoCommand('windows', 'NONCE_ABCD1234')]) {
    assert.ok(line.endsWith('\r') && !line.includes('\n'), JSON.stringify(line));
  }
  assert.ok(counterCommand('linux', 'T').endsWith('done\n'));
  const src = ['scenarios/q00-q03.mjs', 'scenarios/q10-q13.mjs', 'scenarios/common.mjs'].map((f) => readFileSync(new URL('./' + f, import.meta.url), 'utf8')).join('');
  assert.ok(!src.includes("\\r\\n'") && !src.includes('\\r\\n`'), 'no CR+LF typed into a session');
  // run-r6c Q01 threw ReferenceError: q00-q03 called counterCommand without importing it.
  for (const [file, names] of [['scenarios/q00-q03.mjs', ['counterCommand']], ['scenarios/q10-q13.mjs', ['enterKey']]]) {
    const text = readFileSync(new URL('./' + file, import.meta.url), 'utf8');
    const imports = text.slice(0, text.indexOf("from './common.mjs'"));
    for (const name of names) assert.ok(new RegExp(`\\b${name}\\b`).test(imports), `${file} imports ${name}`);
  }
});

test('endsAtPowerShellPrompt: fresh prompt yes, continuation or typed text no (maho-win probe bytes)', async () => {
  const { endsAtPowerShellPrompt, outputMatches } = await import('./lib/agent.mjs');
  const fresh = 'QA_A_TICK_9\r\n\x1b[?25lPS C:\\Users\\sook\\AppData\\Local\\Temp> \x1b[?25h';
  const cont = 'PS C:\\Users\\sook\\AppData\\Local\\Temp> \x1b[?25l\x1b[37m\r\n>> \x1b[11;38H\x1b[?25h';
  assert.equal(endsAtPowerShellPrompt(fresh), true);
  assert.equal(endsAtPowerShellPrompt(cont), false);
  assert.equal(endsAtPowerShellPrompt('PS C:\\x> Write-Output 1'), false);
  assert.equal(endsAtPowerShellPrompt('QA_A_TICK_9\r\n'), false);
  // promptAfter only looks past the offset: a prompt before Ctrl+C does not count.
  const before = 'PS C:\\x> for(...)\r\nQA_A_TICK_1\r\n';
  assert.equal(outputMatches(before, '', false, before.length), false);
  assert.equal(outputMatches(before + 'QA_A_TICK_2\r\nPS C:\\x> ', '', false, before.length), true);
});

test('Q13: hasShownPrompt sees a PowerShell prompt line, not a bare DSR or banner', () => {
  assert.equal(hasShownPrompt('\x1b[6n'), false);
  assert.equal(hasShownPrompt('\x1b[6n\x1b[?9001h\x1b[m\x1b]0;C:\\pwsh.exe\x07PowerShell 7.6.7\r\n'), false);
  assert.equal(hasShownPrompt('PowerShell 7.6.7\r\n\x1b]0;x\x07PS C:\\repo> '), true);
  assert.equal(hasShownPrompt('PS C:\\repo> while ($true) {}\r\nQA_TICK_1\r\n'), true);
});

function roundTripWorld({ history, promptArrives = true }) {
  const order = [];
  let text = history;
  const conn = {
    call: async (m) => { order.push(m.type === 'write' ? (Buffer.from(m.data, 'base64').toString() === '\u0003' ? 'ctrlC' : 'write') : m.type); if (m.type === 'write' && Buffer.from(m.data, 'base64').toString() !== '\u0003') text += 'NONCE_X\r\n'; return m.type === 'attach' ? { type: 'attachOk' } : { type: 'writeOk' }; },
    outputOf: async () => text,
    waitOutput: async (_sid, marker, deadlineMs, { promptAfter = null } = {}) => {
      if (promptAfter === 0) { order.push(`firstPrompt:${deadlineMs}`); if (!promptArrives) return { timedOut: true, text }; text += 'PS C:\\repo> '; return { text }; }
      if (promptAfter !== null) { order.push('freshPrompt'); text += '^C\r\nPS C:\\repo> '; return { text }; }
      return { text, count: 1 };
    },
    describe: async () => ({}),
  };
  return { conn, order };
}

test('Q13: a never-prompted Windows session waits for its first prompt, then types without Ctrl+C', async () => {
  const { conn, order } = roundTripWorld({ history: '\x1b[6n\x1b[?9001h' });
  const result = { assertions: [] };
  await nonceRoundTrip({ platform: 'windows' }, conn, 's1', result, 'legacy');
  assert.deepEqual(order.slice(0, 3), ['attach', `firstPrompt:${FIRST_PROMPT_MS}`, 'write']);
  assert.ok(!order.includes('ctrlC') && !order.includes('freshPrompt'));
  assert.ok(result.assertions.find((a) => a.name === 'legacy: first prompt before Ctrl+C')?.ok);
  assert.ok(result.assertions.every((a) => a.ok));
});

test('Q13: a session that already prompted (counter running) goes straight to Ctrl+C', async () => {
  const { conn, order } = roundTripWorld({ history: 'PS C:\\repo> while(1){}\r\nQA_TICK_2\r\n' });
  const result = { assertions: [] };
  await nonceRoundTrip({ platform: 'windows' }, conn, 's1', result, 'reattached');
  assert.deepEqual(order.slice(0, 2), ['attach', 'ctrlC']);
  assert.ok(!result.assertions.some((a) => /first prompt/.test(a.name)));
});

test('Q13: no first prompt is a failed assertion, never a hang; Linux never waits', async () => {
  const w = roundTripWorld({ history: '\x1b[6n', promptArrives: false });
  const result = { assertions: [] };
  await nonceRoundTrip({ platform: 'windows' }, w.conn, 's1', result, 'legacy');
  assert.equal(result.assertions.find((a) => a.name === 'legacy: first prompt before Ctrl+C')?.ok, false);
  const l = roundTripWorld({ history: '' });
  await nonceRoundTrip({ platform: 'linux' }, l.conn, 's1', { assertions: [] }, 'x');
  assert.ok(!l.order.some((o) => String(o).startsWith('firstPrompt')));
});



