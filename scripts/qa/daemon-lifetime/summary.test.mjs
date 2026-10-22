import test from 'node:test';
import assert from 'node:assert/strict';
import { SCENARIO_IDS, newSummary, selectScenarios, assertion, settle, block, applyQ00Gate, computeExitCode, validateSummary } from './lib/summary.mjs';
import { readFileSync } from 'node:fs';
import { RUNNERS, assertRunnersComplete, parseArgs, artifactPaths, selfTestExitCode, SELF_TEST_PATCH, teardownRun, runWithTeardown, prepareSelfTestWorktree, teardownExtras, releaseOwnerFile, releaseContinuity, prepareBaselineDist, recoverAfterScenarioException } from './run.mjs';
import { hostFor } from './lib/hosts.mjs';
import { confirmOwner, ownerStartMatches, ownerIdentity, parseStarting, awaitPendingStart, retireOwner, freshOwner, invalidateOwnerState, setAsideHeadCatalog, workspaceCatalogFile, recoverOwnerAfterException, STARTING_WINDOW_MS, seedLocalSession, seedShell, counterCommand, SHELL_READY_TIMEOUT_MS, COUNTER_TIMEOUT_MS, SEED_TEXT_TAIL } from './scenarios/common.mjs';
import { outputMatches, createOutputRoute } from './lib/agent.mjs';
import { DsrResponder, DSR_QUERY, DSR_REPLY, visibleText } from './lib/wire.mjs';
import { Q06, killLaunchedGui } from './scenarios/q04-q06.mjs';
import { Q07 } from './scenarios/q07-q09.mjs';
import { Q11, openSettings, nthReceiptPattern, SETTINGS_OPENED_RECEIPT, SETTINGS_OPEN_ATTEMPTS } from './scenarios/q10-q13.mjs';

const RUN = '20261019t064512z';

test('every todo 12 scenario exists with its platform scope and a runner', () => {
  assert.deepEqual(SCENARIO_IDS, ['Q00', 'Q01', 'Q02', 'Q03', 'Q04', 'Q04b', 'Q05', 'Q06', 'Q06w', 'Q07', 'Q08', 'Q08b', 'Q09', 'Q10', 'Q10b', 'Q11', 'Q12', 'Q13']);
  assert.ok(assertRunnersComplete(RUNNERS, SCENARIO_IDS));
  assert.throws(() => assertRunnersComplete({ Q00() {} }, ['Q00', 'Q01']), /no runner for Q01/);
  const by = (p) => Object.fromEntries(newSummary({ runId: RUN, platform: p, host: 'h' }).scenarios.map((s) => [s.id, s.applicable]));
  assert.equal(by('mac').Q04b, true);
  assert.equal(by('linux').Q04b, false);
  assert.equal(by('windows').Q06w, true);
  assert.equal(by('mac').Q06w, false);
  assert.equal(by('windows').Q08, false);
  assert.equal(by('windows').Q09, true);
  assert.equal(by('mac').Q09, false);
  assert.equal(by('windows').Q08b, true);
});

test('selection always includes the Q00 gate and rejects unknown ids', () => {
  assert.deepEqual(selectScenarios({ only: ['Q07'] }, 'linux'), ['Q00', 'Q07']);
  assert.equal(selectScenarios({ all: true }, 'mac').length, SCENARIO_IDS.length);
  assert.throws(() => selectScenarios({ only: ['Q99'] }, 'mac'), /unknown scenario/);
});

test('a scenario passes only with evidence and all-true assertions', () => {
  const s = newSummary({ runId: RUN, platform: 'linux', host: 'omarchy' });
  const [q00, q01, q02] = s.scenarios;
  settle(q00);
  assert.equal(q00.status, 'FAIL');
  assertion(q01, 'a', true);
  settle(q01);
  assert.equal(q01.status, 'FAIL');
  assertion(q02, 'a', true);
  q02.evidence.push('/e/q02.log');
  settle(q02);
  assert.equal(q02.status, 'PASS');
  const q04 = s.scenarios.find((x) => x.id === 'Q04');
  block(q04, 'HOOK_MISSING', 'listenerStall');
  settle(q04);
  assert.equal(q04.status, 'BLOCKED');
});

test('a non-PASS Q00 blocks the rest with an exact barrier', () => {
  const s = newSummary({ runId: RUN, platform: 'windows', host: 'maho-win' });
  s.scenarios[0].status = 'FAIL';
  assert.equal(applyQ00Gate(s), true);
  const q01 = s.scenarios.find((x) => x.id === 'Q01');
  assert.equal(q01.status, 'BLOCKED');
  assert.deepEqual(q01.barrier, { kind: 'Q00_PRECONDITION', detail: 'Q00 is FAIL' });
  assert.equal(s.scenarios.find((x) => x.id === 'Q08').status, 'NOT_APPLICABLE');
  assert.deepEqual(validateSummary(s), []);
});

test('exit code is 0 only with all PASS, clean teardown and an untouched release daemon', () => {
  const s = newSummary({ runId: RUN, platform: 'mac', host: 'mac' });
  const sel = ['Q00', 'Q01'];
  for (const r of s.scenarios.filter((x) => sel.includes(x.id))) { r.status = 'PASS'; r.assertions.push({ name: 'a', ok: true }); r.evidence.push('e'); }
  s.releaseDaemon.unchanged = true;
  assert.equal(computeExitCode(s, sel), 0);
  s.teardown.leftovers.push({ kind: 'job', name: 'x' });
  assert.equal(computeExitCode(s, sel), 1);
  s.teardown.leftovers.length = 0;
  s.releaseDaemon.unchanged = false;
  assert.equal(computeExitCode(s, sel), 1);
});

test('schema validation rejects a PASS without evidence and a BLOCKED without a barrier', () => {
  const s = newSummary({ runId: RUN, platform: 'linux', host: 'omarchy' });
  s.scenarios[0].status = 'PASS';
  s.scenarios[1].status = 'BLOCKED';
  const problems = validateSummary(s);
  assert.ok(problems.some((p) => p.startsWith('Q00: PASS without evidence')));
  assert.ok(problems.some((p) => p.startsWith('Q01: BLOCKED without')));
  assert.ok(validateSummary({ ...s, runId: 'x' }).includes('runId is not a harness run id'));
});

test('CLI arguments', () => {
  assert.deepEqual(parseArgs(['--platform', 'linux', '--only', 'Q01, Q08']).only, ['Q01', 'Q08']);
  assert.equal(parseArgs(['--platform', 'mac', '--self-check']).selfCheck, true);
  assert.throws(() => parseArgs(['--platform', 'mac']), /--all or --only/);
  assert.throws(() => parseArgs(['--platform', 'bsd', '--all']), /--platform/);
  assert.throws(() => parseArgs(['--platform', 'mac', '--all', '--release']), /unknown argument/);
  assert.equal(parseArgs(['--platform', 'linux', '--all']).worktreeSlug, 'daemon-lifetime-w5-t12-20261018');
  assert.equal(parseArgs(['--platform', 'windows', '--all', '--worktree-slug', 'w5qa-r3-20261020']).worktreeSlug, 'w5qa-r3-20261020');
  assert.throws(() => parseArgs(['--platform', 'windows', '--all', '--worktree-slug', '../x']), /--worktree-slug/);
  const run = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8');
  assert.ok(!/worktreePaths\(host, '/.test(run), 'every remote worktree path honors --worktree-slug');
});

test('--self-test-reload runs Q04 against the reload patch and passes only when Q04 fails', () => {
  const opts = parseArgs(['--platform', 'linux', '--self-test-reload']);
  assert.equal(opts.selfTestReload, true);
  assert.deepEqual(opts.only, ['Q04']);
  assert.throws(() => parseArgs(['--platform', 'mac', '--self-test-reload', '--all']), /takes no --all/);
  const summary = (status) => ({ scenarios: [{ id: 'Q00', status: 'PASS' }, { id: 'Q04', status }] });
  assert.equal(selfTestExitCode(summary('FAIL')), 0);
  for (const status of ['PASS', 'BLOCKED', 'NOT_RUN']) assert.equal(selfTestExitCode(summary(status)), 1, status);
  // The patch re-adds supervisor::reload on the silent-handshake path Q04's stalled but connectable
  // owner takes (client.rs classify_silent_handshake, Alive/Unknown arm), not the owner-wait path
  // the fast-path connect never reaches, and restores a reload that restarts the job holding the
  // live daemon (launchctl kickstart -k / systemctl restart).
  const patch = readFileSync(new URL(`./${SELF_TEST_PATCH}`, import.meta.url), 'utf8');
  assert.match(patch, /\+\+\+ b\/src-tauri\/src\/daemon\/client\.rs/);
  assert.match(patch, /^\+\s+crate::daemon::supervisor::reload\(\)/m);
  assert.match(patch, /^\+.*"kickstart", "-k"/m);
  assert.match(patch, /^\+.*"--user", "restart"/m);
  const clientHunk = patch.split(/^diff --git /m).find((d) => d.startsWith('a/src-tauri/src/daemon/client.rs'));
  assert.match(clientHunk, /^@@ [^\n]*@@ async fn classify_silent_handshake\(/m);
  assert.match(clientHunk, /^-\s+"handshakeSilent",$/m);
  assert.match(clientHunk, /^ \s+Ok\(owner_admission::OwnerState::Absent\) => \{$/m);
  assert.doesNotMatch(clientHunk, /wait_for_owner_endpoint|wait_for_supervised_daemon/);
});

test('teardown runs every step after a pre-scenario failure and never reports a failed step as clean', async () => {
  const fakeAgent = ({ failTable = false } = {}) => {
    const calls = [];
    return { calls, call: async (method, params) => {
      calls.push([method, params]);
      if (method === 'exec') return { status: 0, stdout: '[]', stderr: '' };
      if (method === 'teardownRecorded') return { killed: [{ pid: 7, role: 'gui-g1' }], refused: [] };
      if (method === 'table') { if (failTable) throw new Error('table failed'); return []; }
      if (method === 'remove') return true;
      if (method === 'exists') return false;
      return null;
    } };
  };
  const jobs = { removed: [], listQaJobs: async () => (jobs.removed.length ? [] : ['ferryx-qa-x.service']), remove: async (n) => jobs.removed.push(n) };
  const base = { host: { platform: 'linux' }, layout: { logs: '/s/r/logs' }, runDirs: ['/s/r', '/t/r'], qaRoots: ['/s/r'], localEvidence: '/nonexistent-evidence', prodSnapshot: async () => 'owner' };
  const ok = newSummary({ runId: RUN, platform: 'linux', host: 'h' });
  ok.releaseDaemon.before = 'owner';
  const order = [];
  const agent = fakeAgent();
  await teardownRun({ ...base, summary: ok, agent, jobs, extra: [['mahord', async () => order.push('mahord')]] });
  assert.deepEqual(ok.teardown.killed, [{ pid: 7, role: 'gui-g1' }]);
  assert.deepEqual(ok.teardown.jobsRemoved, ['ferryx-qa-x.service']);
  assert.deepEqual(ok.teardown.dirsRemoved, ['/s/r', '/t/r']);
  assert.deepEqual([ok.teardown.leftovers, ok.teardown.errors, ok.releaseDaemon.unchanged, order], [[], [], true, ['mahord']]);
  // A failing step is recorded, later steps still run, and the run dirs stay for inspection.
  jobs.removed = [];
  const bad = newSummary({ runId: RUN, platform: 'linux', host: 'h' });
  bad.releaseDaemon.before = 'owner';
  const badAgent = fakeAgent({ failTable: true });
  await teardownRun({ ...base, summary: bad, agent: badAgent, jobs });
  assert.deepEqual(bad.teardown.errors, [{ step: 'leftovers', error: 'table failed' }]);
  assert.deepEqual(bad.teardown.dirsRemoved, []);
  assert.ok(!badAgent.calls.some(([m]) => m === 'remove'));
  assert.equal(bad.releaseDaemon.unchanged, true);
  assert.equal(computeExitCode(bad, ['Q00']), 1);
});

test('a build or setup error goes through teardown and stop, and the original error is rethrown with exit 2', async () => {
  const build = new Error('BUILD_FAILED: A exited 101');
  const order = [];
  const writes = [];
  const s = newSummary({ runId: RUN, platform: 'linux', host: 'h' });
  const run = runWithTeardown({
    summary: s,
    work: async () => { order.push('work'); throw build; },
    teardown: async () => { order.push('teardown'); throw new Error('teardown broke'); },
    stop: async () => { order.push('stop'); throw new Error('stop broke'); },
    writeSummary: () => writes.push(s.exitCode),
  });
  await assert.rejects(run, (e) => e === build);
  assert.deepEqual(order, ['work', 'teardown', 'stop']);
  assert.equal(s.exitCode, 2);
  assert.match(s.error, /BUILD_FAILED: A exited 101/);
  assert.match(s.teardownError, /teardown broke/);
  assert.match(s.stopError, /stop broke/);
  assert.equal(writes.at(-1), 2, 'the exit code is persisted before the rethrow');
  // A clean run tears down once and returns; a teardown failure without a run error is not swallowed.
  const ok = newSummary({ runId: RUN, platform: 'linux', host: 'h' });
  const seen = [];
  await runWithTeardown({ summary: ok, work: async () => seen.push('work'), teardown: async () => seen.push('teardown'), stop: async () => seen.push('stop'), writeSummary: () => {} });
  assert.deepEqual([seen, ok.exitCode, ok.error], [['work', 'teardown', 'stop'], null, undefined]);
  await assert.rejects(runWithTeardown({ summary: ok, work: async () => {}, teardown: async () => { throw new Error('leak'); }, stop: async () => {}, writeSummary: () => {} }), /leak/);
});

test('a self-test setup failure after worktree add still hands teardown the worktree to remove', async () => {
  const host = hostFor('linux');
  const calls = [];
  const agent = { call: async (method, p) => {
    calls.push(p.args.join(' '));
    if (p.args.includes('rev-parse')) return { status: 0, stdout: 'e4bc115f\n', stderr: '' };
    if (p.args.includes('apply')) return { status: 1, stdout: '', stderr: 'error: patch failed: src-tauri/src/daemon/client.rs:188' };
    return { status: 0, stdout: '', stderr: '' };
  } };
  const live = { selfTest: null, mahord: null };
  const s = newSummary({ runId: RUN, platform: 'linux', host: 'h' });
  const register = (h) => { live.selfTest = h; s.selfTestReload = { worktree: h.worktree, removed: false }; };
  await assert.rejects(prepareSelfTestWorktree({ host, agent, runId: RUN, source: '/src', register }), /SELF_TEST_SETUP_FAILED: git .* apply .*patch failed/);
  const wt = `${host.worktreeRoot}/selftest-reload-${RUN}`;
  assert.equal(live.selfTest?.worktree, wt);
  assert.ok(!calls.some((c) => c.includes('install')), 'setup stops at the failed apply');
  const teardown = newSummary({ runId: RUN, platform: 'linux', host: 'h' });
  teardown.releaseDaemon.before = null;
  const tAgent = { call: async (m) => (m === 'exec' ? { status: 0, stdout: '[]', stderr: '' } : m === 'teardownRecorded' ? { killed: [], refused: [] } : m === 'table' ? [] : m === 'exists' ? false : null) };
  await teardownRun({ summary: teardown, agent: tAgent, jobs: { listQaJobs: async () => [], remove: async () => {} }, host: { platform: 'linux' }, layout: { logs: '/s/r/logs' }, runDirs: ['/s/r'], qaRoots: ['/s/r'], localEvidence: '/nonexistent-evidence', prodSnapshot: async () => null, extra: teardownExtras(live, s) });
  const n = calls.length;
  assert.deepEqual(calls.slice(n - 2), [`-C ${host.canonical} worktree remove --force ${wt}`, `-C ${host.canonical} worktree prune`]);
  assert.equal(s.selfTestReload.removed, true);
  assert.deepEqual(teardown.teardown.leftovers, []);
  // A failing removal is a teardown error and a leftover, never a clean run.
  const bad = newSummary({ runId: RUN, platform: 'linux', host: 'h' });
  const failing = { selfTest: { worktree: wt, remove: async () => { throw new Error('worktree remove exited 128'); } }, mahord: null };
  await teardownRun({ summary: bad, agent: tAgent, jobs: { listQaJobs: async () => [], remove: async () => {} }, host: { platform: 'linux' }, layout: { logs: '/s/r/logs' }, runDirs: ['/s/r'], qaRoots: ['/s/r'], localEvidence: '/nonexistent-evidence', prodSnapshot: async () => null, extra: teardownExtras(failing, bad) });
  assert.deepEqual(bad.teardown.errors, [{ step: 'self-test-worktree', error: 'worktree remove exited 128' }]);
  assert.equal(computeExitCode(bad, ['Q00']), 1);
});

test('release owner record: product runtime path per platform, honest absent vs continuity vs change', async () => {
  assert.equal(releaseOwnerFile('linux', { uid: 1000 }), '/tmp/rorca-1000/owner.json');
  assert.equal(releaseOwnerFile('mac', { uid: 501 }), '/tmp/rorca-501/owner.json');
  assert.equal(releaseOwnerFile('windows', { localAppData: 'C:\\Users\\sook\\AppData\\Local' }), 'C:\\Users\\sook\\AppData\\Local\\Ferryx\\runtime\\owner.json');
  assert.throws(() => releaseOwnerFile('windows', { localAppData: '' }), /RELEASE_OWNER_PATH/);
  assert.deepEqual(releaseContinuity(null, null), { state: 'absent', unchanged: true });
  assert.deepEqual(releaseContinuity('{"pid":1}', '{"pid":1}'), { state: 'continuous', unchanged: true });
  for (const [b, a] of [[null, '{"pid":2}'], ['{"pid":1}', null], ['{"pid":1}', '{"pid":2}']]) assert.deepEqual(releaseContinuity(b, a), { state: 'changed', unchanged: false });
  // teardownRun: a snapshot that cannot be read leaves the release receipt unverified, not unchanged.
  const tAgent = { call: async (m) => (m === 'exec' ? { status: 0, stdout: '[]', stderr: '' } : m === 'teardownRecorded' ? { killed: [], refused: [] } : m === 'table' ? [] : m === 'exists' ? false : null) };
  const base = { agent: tAgent, jobs: { listQaJobs: async () => [], remove: async () => {} }, host: { platform: 'windows' }, layout: { logs: 'C:\\r\\logs' }, runDirs: ['C:\\r'], qaRoots: ['C:\\r'], localEvidence: '/nonexistent-evidence' };
  const absent = newSummary({ runId: RUN, platform: 'windows', host: 'h' });
  await teardownRun({ ...base, summary: absent, prodSnapshot: async () => null });
  assert.deepEqual([absent.releaseDaemon.state, absent.releaseDaemon.unchanged], ['absent', true]);
  const unreadable = newSummary({ runId: RUN, platform: 'windows', host: 'h' });
  await teardownRun({ ...base, summary: unreadable, prodSnapshot: async () => { throw new Error('EACCES owner.json'); } });
  assert.deepEqual([unreadable.releaseDaemon.state, unreadable.releaseDaemon.unchanged], ['unverified', null]);
  assert.equal(computeExitCode(unreadable, ['Q00']), 1);
  const changed = newSummary({ runId: RUN, platform: 'windows', host: 'h' });
  changed.releaseDaemon.before = '{"pid":1}';
  await teardownRun({ ...base, summary: changed, prodSnapshot: async () => '{"pid":2}' });
  assert.deepEqual([changed.releaseDaemon.state, changed.releaseDaemon.unchanged], ['changed', false]);
});

test('artifact paths per platform layout', () => {
  const mac = artifactPaths({ platform: 'mac', targetDir: '/t', productName: 'Ferryx QA x', name: 'A' });
  assert.equal(mac.guiExecutable, '/t/debug/bundle/macos/Ferryx QA x.app/Contents/MacOS/ferryx');
  const win = artifactPaths({ platform: 'windows', targetDir: 'C:\\t', productName: 'p', name: 'A' });
  assert.equal(win.guiExecutable, 'C:\\t\\debug\\ferryx.exe');
  assert.equal(win.cliExecutable, 'C:\\t\\debug\\ferryx-cli.exe');
});

/**
 * In-memory host for the owner-state helpers: owner.json, starting.json, live pids and the
 * owner change a waitOwnerChange observes are plain fields; no clock, sleep or real process.
 */
function ownerWorld({ owner = null, alive = [], starting = null, publish = [], sessions = [], stuck = false } = {}) {
  // sessions: backend ids the owner still serves. ListSessionsOk carries them as bare strings
  // (protocol.rs ListSessionsOk { sessions: Vec<String> }); RetireWhenIdle refuses while any is
  // open, so the owner exits only after every one is closed. stuck: close never ends a session.
  const w = { owner, alive: new Set(alive), starting, publish: [...publish], sessions: new Set(sessions), stuck, sessionCloses: [], calls: [], closed: [], quits: [], kills: [], launches: [], registryKills: [], killRefused: new Set() };
  const RT = '/r/run';
  // Linux units: owner.json startTime and the probe are the same /proc starttime ticks.
  const startOf = (pid) => 1000 + pid;
  const agent = {
    call: async (method, params = {}) => {
      w.calls.push(method);
      switch (method) {
        case 'readFile':
          if (params.file === `${RT}/owner.json`) return w.owner === null ? null : JSON.stringify({ pid: w.owner, startTime: w.ownerStartTime ?? startOf(w.owner) });
          if (params.file === `${RT}/starting.json`) return w.starting;
          return null;
        case 'probe': return w.alive.has(params.pid) ? { executable: `/qa/${params.pid}`, startTime: String(startOf(params.pid)) } : null;
        case 'kill':
          // ProcessRegistry.kill: refuses (throws) a pid whose recorded identity no longer matches.
          w.registryKills.push({ pid: params.pid, signal: params.signal });
          if (w.killRefused.has(params.pid)) throw Object.assign(new Error(`pid ${params.pid} no longer runs its recorded executable`), { code: 'KILL_REFUSED' });
          w.alive.delete(params.pid);
          return true;
        case 'controlOpen': return { handle: w.alive.has(w.owner) ? 'ctl' : null };
        case 'dataOpen': return { handle: 'data2', handshake: { type: 'handshakeOk' } };
        case 'close': w.closed.push(params.handle); return true;
        case 'call': {
          const f = params.frame;
          if (f.op === 'Hello') return { ok: true, pid: w.owner };
          if (f.op === 'Inventory') return { ok: true, live_local: 0, live_remote: 0, live_paired: 0 };
          if (f.op === 'RetireWhenIdle') {
            if (w.sessions.size > 0) return { ok: true, live: w.sessions.size };
            w.alive.delete(w.owner); return { ok: true, live: 0 };
          }
          if (f.type === 'listSessions') return { type: 'listSessionsOk', epoch: 1, sessions: [...w.sessions] };
          if (f.type === 'close') {
            w.sessionCloses.push(f.sessionId);
            if (!w.sessions.has(f.sessionId)) return { type: 'error', code: 'SESSION_NOT_FOUND' };
            if (!w.stuck) w.sessions.delete(f.sessionId);
            return { type: 'closeOk' };
          }
          if (f.type === 'describeLiveSessions') return { type: 'describeLiveSessionsOk', sessions: [] };
          if (f.type === 'registerWorkspace') return { type: 'registerWorkspaceOk' };
          if (f.type === 'spawn') return { type: 'spawnOk', sessionId: 'S1' };
          if (f.type === 'attach') return { type: 'attachOk' };
          if (f.type === 'write') return { type: 'writeOk' };
          return { type: 'unexpected', frame: f };
        }
        case 'waitOutput': return { count: 1, text: 'QA_A_TICK_2' };
        // An owner with open sessions never exits on retire; a killed one is gone.
        case 'waitExit': return w.alive.has(params.pid) && w.sessions.size > 0 ? { pid: params.pid, timedOut: true } : { pid: params.pid, exitedAt: 1 };
        case 'waitOwnerChange': {
          const next = w.publish.shift();
          if (!next) return { owner: null, timedOut: true, seen: [] };
          w.owner = next.pid;
          if (next.alive) w.alive.add(next.pid);
          if (next.clearStarting) w.starting = null;
          return { owner: { pid: next.pid }, seen: [next.pid] };
        }
        case 'waitLog': return { matched: 'attached' };
        case 'table': return [...w.alive].map((pid) => ({ pid, executable: `/qa/${pid}`, commandLine: `/qa/${pid} --daemon` }));
        case 'record': return { pid: params.pid };
        case 'writeFile': case 'remove': return true;
        default: throw new Error(`unexpected agent call ${method}`);
      }
    },
  };
  let nextGui = 20;
  const gui = {
    launch: async (artifact, { tag = `g${nextGui}` } = {}) => {
      if (w.failLaunch?.includes(tag)) throw new Error(`PID_GONE: gui-${tag} exited before it was recorded`);
      const pid = ++nextGui;
      const g = { tag, pid, startTime: String(startOf(pid)) };
      w.launches.push(g);
      return g;
    },
    quit: async (g) => { if (g) w.quits.push(g.pid); return { pid: g?.pid, exitedAt: 1 }; },
    kill: async (g) => { w.kills.push(g.pid); return { pid: g.pid, exitedAt: 1 }; },
  };
  gui.launchBatch = (items) => Promise.allSettled(items.map(({ artifact, options }) => gui.launch(artifact, options)));
  const evidenceFiles = [];
  const ctx = {
    agent, gui, platform: 'linux', runId: RUN, layout: { runtime: RT, logs: '/r/logs' }, evidenceDir: '/e',
    writeEvidence: async (file) => { evidenceFiles.push(file); }, workspaceId: 'ws', repoDir: '/repo',
    qaRoots: ['/qa'], artifacts: { A: { name: 'A' } }, state: {},
  };
  return { w, ctx, evidenceFiles };
}

const startingText = (launcherPid, startedAtUnixMs) => JSON.stringify({ launcherPid, launcherStartTime: 1, generationKey: 'k', startedAtUnixMs });

test('owner confirmation: a recorded pid without a live process, or with a Hello for another pid, is never adoptable', () => {
  assert.equal(confirmOwner({ pid: null, hello: null, os: null }).code, 'OWNER_GONE');
  assert.equal(confirmOwner({ pid: 7, hello: null, os: null }).code, 'OWNER_GONE');
  assert.equal(confirmOwner({ pid: 7, hello: { pid: 7 }, os: null }).code, 'OWNER_GONE');
  // owner.json is published before control binds: a live pid without a Hello yet is adoptable.
  assert.equal(confirmOwner({ pid: 7, hello: null, os: { executable: '/x', startTime: '1' } }).ok, true);
  assert.equal(confirmOwner({ pid: 7, hello: { pid: 8 }, os: { executable: '/x', startTime: '1' } }).code, 'OWNER_UNCONFIRMED');
  assert.equal(confirmOwner({ pid: 7, hello: { pid: 7 }, os: { executable: '/x', startTime: '1' } }).ok, true);
  assert.equal(STARTING_WINDOW_MS, 60_000);
  assert.deepEqual(parseStarting(null), { state: 'none' });
  assert.equal(parseStarting('{oops').state, 'unreadable');
  assert.equal(parseStarting('{"launcherPid":1}').state, 'unreadable');
  assert.equal(parseStarting(startingText(5, 100)).key, '5:100');
});

test('pending start: absent, published owner, expired once, replaced and unreadable are distinct', async () => {
  const none = ownerWorld({ owner: 1 });
  assert.deepEqual(await awaitPendingStart(none.ctx, 1), { state: 'absent' });
  assert.ok(!none.w.calls.includes('waitOwnerChange'), 'no pending start: nothing to wait for');

  const published = ownerWorld({ owner: 1, starting: startingText(5, 100), publish: [{ pid: 9, alive: true, clearStarting: true }] });
  assert.equal((await awaitPendingStart(published.ctx, 1)).pid, 9);

  // Same record after a full window without an owner change: older than the window, so Absent,
  // and a later call does not wait on that record again.
  const stuck = ownerWorld({ owner: 1, starting: startingText(5, 100) });
  assert.deepEqual(await awaitPendingStart(stuck.ctx, 1), { state: 'absent', expired: '5:100' });
  assert.deepEqual(await awaitPendingStart(stuck.ctx, 1), { state: 'absent', expired: '5:100' });
  assert.equal(stuck.w.calls.filter((m) => m === 'waitOwnerChange').length, 1);

  const replaced = ownerWorld({ owner: 1, starting: startingText(5, 100) });
  const realCall = replaced.ctx.agent.call;
  replaced.ctx.agent.call = async (m, p) => { const r = await realCall(m, p); if (m === 'waitOwnerChange') replaced.w.starting = startingText(6, 200); return r; };
  assert.equal((await awaitPendingStart(replaced.ctx, 1)).state, 'unknown');

  assert.equal((await awaitPendingStart(ownerWorld({ owner: 1, starting: '{bad' }).ctx, 1)).state, 'unknown');
});

test('retireOwner never reports a stale owner as retired while a start is pending', async () => {
  const clean = ownerWorld({ owner: 1 });
  assert.equal((await retireOwner(clean.ctx)).retired, true);

  const unreadable = ownerWorld({ owner: 1, starting: '{bad' });
  const r = await retireOwner(unreadable.ctx);
  assert.equal(r.retired, false);
  assert.match(r.reason, /starting record unreadable/);

  // The pending start publishes owner 9; that owner is retired through control, not skipped.
  const pending = ownerWorld({ owner: 1, starting: startingText(5, 100), publish: [{ pid: 9, alive: true, clearStarting: true }] });
  const p = await retireOwner(pending.ctx);
  assert.equal(p.retired, true);
  assert.equal(p.pid, 9);
  assert.equal(pending.w.alive.has(9), false);

  // rerun2 Q08b: ctx.state.conn named a generation that had handed over; the owner's sessions are
  // closed through a fresh connection to the current owner, never through that stale one.
  const stale = ownerWorld({ owner: 1, alive: [1], sessions: ['live'] });
  const staleCloses = [];
  stale.ctx.state.conn = { describe: async () => ({ sessions: [{ backendId: 'old' }] }), call: async (f) => { staleCloses.push(f.sessionId); }, close: async () => {} };
  const real = stale.ctx.agent.call;
  stale.ctx.agent.call = async (m, p) => (m === 'call' && p.frame?.type === 'describeLiveSessions' ? { type: 'describeLiveSessionsOk', sessions: [...stale.w.sessions].map((backendId) => ({ backendId })) } : real(m, p));
  const retiredStale = await retireOwner(stale.ctx);
  assert.deepEqual(staleCloses, [], 'nothing is closed through the stale connection');
  assert.deepEqual(stale.w.sessionCloses, ['live']);
  assert.equal(retiredStale.retired, true);
  assert.equal(stale.ctx.state.conn, null);
});

test('freshOwner with expectKey refuses an owner of another generation (run-9a5db442 Q13 got B)', async () => {
  const withKey = (key) => {
    const world = ownerWorld({ owner: 1, publish: [{ pid: 9, alive: true }] });
    const real = world.ctx.agent.call;
    world.ctx.agent.call = async (m, p) => (m === 'call' && p.frame?.op === 'Hello' ? { ok: true, pid: world.w.owner, generation_key: key } : real(m, p));
    return world;
  };
  const stale = withKey('KEY_B');
  await assert.rejects(freshOwner(stale.ctx, { name: 'A' }, { expectKey: 'KEY_A' }), (e) => e.code === 'OWNER_WRONG_GENERATION' && e.detail.expected === 'KEY_A' && e.detail.actual === 'KEY_B');
  assert.equal(stale.ctx.state.conn, undefined, 'no session seeded on the wrong generation');
  const right = withKey('KEY_A');
  assert.equal((await freshOwner(right.ctx, { name: 'A' }, { expectKey: 'KEY_A' })).pid, 9);
});

test('Q07 removes the B generation job it installed, by its recorded name only', async () => {
  const removed = [];
  const units = new Set(['ferryx-qa-x.service', 'ferryx-qa-x-KEY_A.service', 'ferryx-qa-x-KEY_B.service']);
  const jobs = {
    jobName: (key) => `ferryx-qa-x-${key}`,
    inspect: async () => ({ state: 'inactive' }),
    listQaJobs: async () => [...units],
    remove: async (name) => { removed.push(name); units.delete(`${name}.service`); return true; },
  };
  const session = { kind: 'local', backendId: 'L1', readerOpen: true, localChildPid: 5 };
  const conn = { describe: async () => ({ type: 'describeLiveSessionsOk', sessions: [session] }), call: async (f) => ({ type: f.type === 'attach' ? 'attachOk' : 'writeOk' }), waitOutput: async (_s, marker) => ({ count: 1, text: marker }), close: async () => {} };
  const call = async (method, params = {}) => {
    if (method === 'dataOpen') return { handle: 'd', handshake: { type: 'handshakeOk' } };
    if (method === 'controlOpen') return { handle: 'c' };
    if (method === 'call') return params.handle === 'd' ? conn.call(params.frame).then((r) => (params.frame.type === 'describeLiveSessions' ? conn.describe() : r)) : { ok: true, pid: 4, generation_key: 'KEY_A' };
    if (method === 'waitOutput') return { count: 1, text: params.marker };
    if (method === 'probe') return { executable: '/qa/4', startTime: '1' };
    if (method === 'readFile') return null;
    if (method === 'exists') return true;
    return true;
  };
  const ctx = {
    agent: { call }, platform: 'linux', runId: RUN, keys: { A: 'KEY_A', B: 'KEY_B' }, stagedRoot: '/s', jobs,
    layout: { runtime: '/r/run', barriers: '/r/barriers' }, evidenceDir: '/e', writeEvidence: async () => {},
    artifacts: { A: { name: 'A' }, B: { name: 'B' } }, replaceAppOnDisk: async (_from, to) => ({ artifact: to }),
    gui: { quit: async () => ({}), launch: async () => ({ pid: 3 }) }, state: { conn, gui: { pid: 2 }, localA: 'L1' },
  };
  ctx.agent.call = async (method, params) => (method === 'dataOpen' ? { handle: 'd', handshake: { type: 'handshakeOk' } } : call(method, params));
  const result = { id: 'Q07', assertions: [], evidence: [] };
  await Q07(ctx, result);
  assert.deepEqual(removed, ['ferryx-qa-x-KEY_B']);
  assert.ok(units.has('ferryx-qa-x-KEY_A.service') && units.has('ferryx-qa-x.service'), 'only the B unit is removed');
  assert.ok(result.assertions.some((a) => a.name === 'B generation job removed by its recorded name after the replacement check' && a.ok));
});

test('freshOwner throws a structured OWNER_GONE instead of dereferencing a dead owner', async () => {
  const { ctx } = ownerWorld({ owner: 1, publish: [{ pid: 9, alive: false }] });
  await assert.rejects(freshOwner(ctx, { name: 'v1bump' }), (e) => e.code === 'OWNER_GONE' && e.detail.pid === 9 && e.detail.artifact === 'v1bump');
  assert.equal(ctx.state.conn, undefined, 'no connection seeded to a dead owner');
});

test('Q06 launch failure: original error propagates, every launched GUI ends, retired connection dropped, confirmed owner restored', async () => {
  const { w, ctx } = ownerWorld({ owner: 1, alive: [1], publish: [{ pid: 40, alive: true }] });
  w.failLaunch = ['q06c'];
  const oldConn = { describe: async () => ({ type: 'describeLiveSessionsOk', sessions: [] }), call: async () => ({}), close: async () => { w.closed.push('old'); } };
  ctx.state = { gui: { tag: 'g0', pid: 2 }, conn: oldConn, localA: 'OLD' };
  const result = { id: 'Q06', assertions: [], evidence: [] };
  await assert.rejects(Q06(ctx, result), /PID_GONE: gui-q06c/);
  const q06Guis = w.launches.filter((g) => g.tag === 'q06a' || g.tag === 'q06b').map((g) => g.pid);
  // Failure cleanup never sends the focused-window quit hotkey: only registry kills of recorded pids.
  assert.ok(q06Guis.every((pid) => !w.quits.includes(pid)), 'no quit for q06a/q06b');
  assert.deepEqual(w.registryKills.map((k) => k.pid), q06Guis);
  assert.ok(w.registryKills.every((k) => k.signal === 'SIGKILL'));
  assert.ok(w.closed.includes('old'), 'retired connection closed');
  assert.equal(ctx.state.daemon.pid, 40);
  assert.equal(ctx.state.conn.handle, 'data2');
  assert.equal(ctx.state.localA, 'S1');
  const byName = Object.fromEntries(result.assertions.map((a) => [a.name, a.ok]));
  assert.equal(byName['Q06 GUIs ended after the failure'], true);
  assert.equal(byName['a confirmed QA owner restored for the following scenarios'], true);
});

test('Q06 launch failure with no live replacement owner: restore fails as its own assertion, state stays empty', async () => {
  const { w, ctx } = ownerWorld({ owner: 1, alive: [1], publish: [{ pid: 40, alive: false }] });
  w.failLaunch = ['q06c'];
  ctx.state = { gui: { tag: 'g0', pid: 2 }, conn: { describe: async () => ({ type: 'describeLiveSessionsOk', sessions: [] }), call: async () => ({}), close: async () => {} } };
  const result = { id: 'Q06', assertions: [], evidence: [] };
  await assert.rejects(Q06(ctx, result), /PID_GONE: gui-q06c/);
  const restore = result.assertions.find((a) => a.name === 'a confirmed QA owner restored for the following scenarios');
  assert.equal(restore.ok, false);
  assert.equal(restore.detail.code, 'OWNER_GONE');
  assert.equal(ctx.state.conn, null);
  assert.equal(ctx.state.localA, null);
});

test('Q06 failure cleanup: a registry kill refusal is evidence, the GUI is not reported gone and the original error still propagates', async () => {
  const { w, ctx } = ownerWorld({ owner: 1, alive: [1], publish: [{ pid: 40, alive: true }] });
  w.failLaunch = ['q06c'];
  // q06a (pid 21) is still running but its pid now runs another executable: the registry refuses.
  w.alive.add(21);
  w.killRefused.add(21);
  ctx.state = { gui: { tag: 'g0', pid: 2 }, conn: { describe: async () => ({ type: 'describeLiveSessionsOk', sessions: [] }), call: async () => ({}), close: async () => {} } };
  const result = { id: 'Q06', assertions: [], evidence: [] };
  await assert.rejects(Q06(ctx, result), /PID_GONE: gui-q06c/);
  assert.equal(w.quits.length, 1, 'only the pre-Q06 GUI was quit, before the launches');
  const ended = result.assertions.find((a) => a.name === 'Q06 GUIs ended after the failure');
  assert.equal(ended.ok, false);
  const a = ended.detail.find((e) => e.tag === 'q06a');
  assert.deepEqual([a.signalled, a.refused.code, a.gone], [null, 'KILL_REFUSED', false]);
  assert.ok(w.alive.has(21), 'refused pid not signalled');
  assert.equal(ended.detail.find((e) => e.tag === 'q06b').gone, true);
  assert.ok(result.evidence.includes('/e/Q06-cleanup.json'));
});

test('killLaunchedGui ends only the recorded pid through the registry and reads gone from the process', async () => {
  const { w, ctx } = ownerWorld({ alive: [30] });
  const out = await killLaunchedGui(ctx, { tag: 'x', pid: 30, startTime: '1030' });
  assert.deepEqual([out.signalled, out.refused, out.gone], [true, null, true]);
  assert.deepEqual(w.registryKills, [{ pid: 30, signal: 'SIGKILL' }]);
  assert.ok(!w.calls.includes('exec'), 'no hotkey or shell path');
});

test('owner start time: owner.json startTime must match the probed start in platform units', () => {
  // linux: /proc starttime ticks on both sides.
  assert.equal(ownerStartMatches('linux', '123456', '123456'), true);
  assert.equal(ownerStartMatches('linux', '123456', '123457'), false);
  // windows: FILETIME above 2^53 compared exactly (no float rounding).
  assert.equal(ownerStartMatches('windows', '134048390123456789', '134048390123456789'), true);
  assert.equal(ownerStartMatches('windows', '134048390123456789', '134048390123456790'), false);
  // mac: record in microseconds since epoch, probe is ps lstart in local time at 1s resolution.
  const local = new Date(2026, 9, 5, 7, 8, 9);
  const us = BigInt(local.getTime()) * 1000n + 654_321n;
  assert.equal(ownerStartMatches('mac', String(us), 'Mon Oct  5 07:08:09 2026'), true);
  assert.equal(ownerStartMatches('mac', String(us), 'Mon Oct  5 07:08:10 2026'), false);
  // Missing or unparseable values are never a match.
  for (const [p, r, o] of [['linux', null, '1'], ['linux', '1', null], ['linux', 'x', '1'], ['mac', '1', 'garbage'], ['bsd', '1', '1']]) assert.equal(ownerStartMatches(p, r, o), false);
});

test('owner confirmation rejects a reused pid whose live start time differs from owner.json', async () => {
  const os = { executable: '/x', startTime: '500' };
  assert.equal(confirmOwner({ pid: 7, hello: null, owner: { pid: 7 }, ownerStartTime: '500', os, platform: 'linux' }).ok, true);
  const reused = confirmOwner({ pid: 7, hello: { pid: 7 }, owner: { pid: 7 }, ownerStartTime: '499', os, platform: 'linux' });
  assert.equal(reused.code, 'OWNER_UNCONFIRMED');
  assert.deepEqual([reused.detail.recordedStartTime, reused.detail.probedStartTime], ['499', '500']);
  assert.equal(confirmOwner({ pid: 7, hello: null, owner: { pid: 7 }, ownerStartTime: null, os, platform: 'linux' }).code, 'OWNER_UNCONFIRMED');
  // End to end through ownerIdentity: the raw serialized field, not a rounded JSON number.
  const { w, ctx } = ownerWorld({ owner: 7, alive: [7] });
  assert.equal(confirmOwner(await ownerIdentity(ctx)).ok, true);
  w.ownerStartTime = 999;
  const id = await ownerIdentity(ctx);
  assert.equal(id.ownerStartTime, '999');
  assert.equal(confirmOwner(id).code, 'OWNER_UNCONFIRMED');
  // freshOwner refuses to seed an owner whose pid was reused.
  const fresh = ownerWorld({ owner: 1, publish: [{ pid: 9, alive: true }] });
  fresh.w.ownerStartTime = 1;
  await assert.rejects(freshOwner(fresh.ctx, { name: 'A' }), (e) => e.code === 'OWNER_UNCONFIRMED' && e.detail.pid === 9);
});

test('baseline step gets an empty ui/dist in its own worktree, idempotently, on every platform', async () => {
  const { mkdtempSync, writeFileSync: wf, readdirSync, readFileSync: rf, rmSync, statSync } = await import('node:fs');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const { spawnSync } = await import('node:child_process');
  // Host paths: each platform joins ui/dist under the given worktree with its own separator.
  const seen = [];
  const fake = { call: async (op, a) => { seen.push({ op, ...a }); return { status: 0, stdout: '', stderr: '' }; } };
  assert.equal(await prepareBaselineDist({ host: hostFor('windows'), agent: fake, worktree: 'C:\\wt\\base' }), 'C:\\wt\\base\\ui\\dist');
  assert.equal(await prepareBaselineDist({ host: hostFor('linux'), agent: fake, worktree: '/wt/base' }), '/wt/base/ui/dist');
  assert.equal(await prepareBaselineDist({ host: hostFor('mac'), agent: fake, worktree: '/wt/base' }), '/wt/base/ui/dist');
  assert.deepEqual(seen.map((c) => [c.op, c.command, c.args.at(-1)]), [['exec', 'node', 'C:\\wt\\base\\ui\\dist'], ['exec', 'node', '/wt/base/ui/dist'], ['exec', 'node', '/wt/base/ui/dist']]);
  // A failing mkdir is a typed refusal, not a silent pass into the cargo step.
  const failing = { call: async () => ({ status: 1, stdout: '', stderr: 'EACCES' }) };
  await assert.rejects(prepareBaselineDist({ host: hostFor('linux'), agent: failing, worktree: '/wt/base' }), (e) => e.code === 'BASELINE_DIST_FAILED');
  // The exact command it sends really creates the dir, twice in a row, and leaves an existing dist untouched.
  const root = mkdtempSync(join(tmpdir(), 'qa-baseline-dist-'));
  try {
    const local = { call: async (op, a) => { const r = spawnSync(process.execPath, a.args, { encoding: 'utf8' }); return { status: r.status, stdout: r.stdout, stderr: r.stderr }; } };
    const host = hostFor(process.platform === 'win32' ? 'windows' : 'linux');
    const dir = await prepareBaselineDist({ host, agent: local, worktree: root });
    assert.equal(statSync(dir).isDirectory(), true);
    assert.deepEqual(readdirSync(dir), []);
    wf(join(dir, 'index.html'), 'kept');
    await prepareBaselineDist({ host, agent: local, worktree: root });
    assert.equal(rf(join(dir, 'index.html'), 'utf8'), 'kept');
    assert.deepEqual(readdirSync(root), ['ui']);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

/** A scripted data connection: each waitOutput answers from `waits` in order. */
function seedWorld({ platform = 'windows', waits }) {
  const calls = [];
  const files = new Map();
  const conn = {
    call: async (frame, deadlineMs) => {
      calls.push({ kind: frame.type, frame, deadlineMs });
      if (frame.type === 'registerWorkspace') return { type: 'registerWorkspaceOk' };
      if (frame.type === 'spawn') return { type: 'spawnOk', sessionId: 'S1' };
      if (frame.type === 'attach') return { type: 'attachOk' };
      if (frame.type === 'write') return { type: 'writeOk' };
      return { type: 'unexpected' };
    },
    waitOutput: async (sessionId, marker, deadlineMs, opts = {}) => { calls.push({ kind: 'waitOutput', sessionId, marker, deadlineMs, visible: Boolean(opts.visible) }); return waits.shift(); },
    describe: async () => { calls.push({ kind: 'describe' }); return { type: 'describeLiveSessionsOk', sessions: [{ backendId: 'S1', readerOpen: true, localChildPid: 77 }] }; },
  };
  const ctx = { platform, workspaceId: 'ws', repoDir: 'C:\\r\\repo', workspaceRegistered: false, evidenceDir: '/e', writeEvidence: async (f, c) => { files.set(f, c); } };
  return { conn, ctx, calls, files };
}

test('seed: Windows spawns powershell explicitly and writes the counter only after the shell prompt', async () => {
  assert.equal(seedShell('windows'), 'powershell');
  assert.equal(seedShell('linux'), null);
  assert.equal(seedShell('mac'), null);
  const { conn, ctx, calls, files } = seedWorld({ waits: [{ text: 'PS C:\\r\\repo> ', count: 0 }, { text: 'QA_A_TICK_2', count: 1 }] });
  assert.equal(await seedLocalSession(ctx, conn, 'QA_A'), 'S1');
  assert.deepEqual(calls.map((c) => c.kind), ['registerWorkspace', 'spawn', 'attach', 'waitOutput', 'write', 'waitOutput']);
  assert.equal(calls[1].frame.shell, 'powershell');
  // The readiness wait is "any visible output" (a prompt, not ConPTY's query), bounded, and
  // strictly before the counter write.
  assert.deepEqual([calls[3].marker, calls[3].deadlineMs, calls[3].visible], ['', SHELL_READY_TIMEOUT_MS, true]);
  assert.equal(Buffer.from(calls[4].frame.data, 'base64').toString(), counterCommand('windows', 'QA_A'));
  assert.deepEqual([calls[5].marker, calls[5].deadlineMs], ['QA_A_TICK_2', COUNTER_TIMEOUT_MS]);
  assert.equal(files.size, 0);
  // Linux keeps the daemon default shell (no shell field on the wire).
  const linux = seedWorld({ platform: 'linux', waits: [{ text: '$ ', count: 0 }, { text: 'QA_A_TICK_2', count: 1 }] });
  await seedLocalSession(linux.ctx, linux.conn, 'QA_A');
  assert.equal('shell' in linux.calls[1].frame, false);
});

test('seed: a silent shell fails at shell-ready, never writes the counter, and keeps describe-live evidence', async () => {
  const { conn, ctx, calls, files } = seedWorld({ waits: [{ timedOut: true, text: '', count: 0 }] });
  await assert.rejects(seedLocalSession(ctx, conn, 'QA_A'), (e) => {
    assert.equal(e.code, 'SEED_FAILED');
    assert.equal(e.detail.stage, 'shell-ready');
    assert.equal(e.detail.receivedBytes, 0);
    assert.deepEqual(e.detail.describe.sessions.map((s) => s.backendId), ['S1']);
    assert.match(e.message, /shell-ready timed out/);
    return true;
  });
  assert.ok(!calls.some((c) => c.kind === 'write'), 'no counter is typed into a shell that never answered');
  assert.deepEqual([...files.keys()], ['/e/seed-QA_A-shell-ready.json']);
  assert.equal(JSON.parse(files.get('/e/seed-QA_A-shell-ready.json')).stage, 'shell-ready');
});

test('seed: a counter timeout carries the bounded received-text tail and describe-live', async () => {
  const noise = 'x'.repeat(SEED_TEXT_TAIL * 2) + 'TAIL_END';
  const { conn, ctx, files } = seedWorld({ waits: [{ text: 'PS> ', count: 0 }, { timedOut: true, text: noise, count: 0 }] });
  await assert.rejects(seedLocalSession(ctx, conn, 'QA_A'), (e) => {
    assert.equal(e.detail.stage, 'counter');
    assert.equal(e.detail.marker, 'QA_A_TICK_2');
    assert.equal(e.detail.receivedBytes, noise.length);
    assert.equal(e.detail.receivedTail.length, SEED_TEXT_TAIL);
    assert.ok(e.detail.receivedTail.endsWith('TAIL_END'));
    assert.equal(e.detail.shell, 'powershell');
    assert.equal(e.detail.evidence, '/e/seed-QA_A-counter.json');
    return true;
  });
  const saved = JSON.parse(files.get('/e/seed-QA_A-counter.json'));
  assert.equal(saved.receivedTail.length, SEED_TEXT_TAIL);
  assert.equal(saved.describe.sessions[0].localChildPid, 77);
});

test('agent output matching: an empty marker means any output, otherwise a substring', () => {
  assert.equal(outputMatches('', ''), false);
  assert.equal(outputMatches('P', ''), true);
  assert.equal(outputMatches('abc QA_A_TICK_2', 'QA_A_TICK_2'), true);
  assert.equal(outputMatches('QA_A_TICK_1', 'QA_A_TICK_2'), false);
  // visible: a bare terminal query is not a prompt; a prompt behind VT noise is.
  assert.equal(outputMatches(DSR_QUERY, '', true), false);
  assert.equal(outputMatches('\x1b[?25l\x1b[2J\x1b[H\x1b]0;Windows PowerShell\x07\r\n', '', true), false);
  assert.equal(outputMatches('\x1b[6n\x1b[?25lPS C:\\r> ', '', true), true);
  assert.equal(visibleText('\x1b[1;1H\x1b[32mok\x1b[0m\r\n'), 'ok\n');
  assert.equal(visibleText('\x1b['), '');
  assert.equal(visibleText('ok\x1b]0;tit'), 'ok');
});

test('DSR responder: one reply per query, split across frames, two queries, never twice for one query', () => {
  const r = new DsrResponder();
  // Query split across frames: nothing owed until it completes, then exactly one.
  assert.equal(r.feed('S1', 'hello\x1b[', 1), 0);
  assert.equal(r.feed('S1', '6n', 2), 1);
  // Two queries in one frame, and one split three ways.
  assert.equal(r.feed('S1', '\x1b[6nPS> \x1b[6n', 3), 2);
  assert.equal(r.feed('S1', '\x1b', 4), 0);
  assert.equal(r.feed('S1', '[', 5), 0);
  assert.equal(r.feed('S1', '6n tail', 6), 1);
  // The same frame seen again (a re-attach replaying a sequence) is never answered twice.
  assert.equal(r.feed('S1', '\x1b[6n', 6), 0);
  assert.equal(r.feed('S1', '\x1b[6n', 5), 0);
  assert.equal(r.replies('S1'), 4);
  // A completed query leaves no tail that could complete a second time.
  assert.equal(r.feed('S1', 'n', 7), 0);
  // Near-misses are not queries; sessions are independent.
  assert.equal(r.feed('S2', '\x1b[5n \x1b[6 n \x1b6n', 1), 0);
  assert.equal(r.feed('S2', '\x1b[', 2), 0);
  assert.equal(r.feed('S1', '6n', 8), 0, 'S2 tail never completes a query in S1');
  assert.equal(r.feed('S2', '6n', 3), 1);
  assert.deepEqual([r.replies('S1'), r.replies('S2')], [4, 1]);
});

test('DSR responder: the query in the first attach history is answered once; a re-attach replay never again', () => {
  const r = new DsrResponder();
  // ConPTY asks at spawn, before the harness attaches: the query is in the first history.
  assert.equal(r.feedHistory('S1', '\x1b[6n', 1), 1);
  // Live output continues after the history's end sequence.
  assert.equal(r.feed('S1', 'PS C:\\r> ', 2), 0);
  // A re-attach (same or another connection) replays the history including the old query.
  assert.equal(r.feedHistory('S1', '\x1b[6nPS C:\\r> ', 2), 0);
  assert.equal(r.feedHistory('S1', '\x1b[6nPS C:\\r> more', 5), 0);
  // ...and the sequence it advanced to keeps the live replay of those frames unanswered too.
  assert.equal(r.feed('S1', '\x1b[6n', 4), 0);
  // A genuinely new live query after that is answered.
  assert.equal(r.feed('S1', '\x1b[6n', 6), 1);
  assert.equal(r.replies('S1'), 2);
  // An empty first history leaves a later live query answerable.
  assert.equal(r.feedHistory('S2', '', null), 0);
  assert.equal(r.feed('S2', '\x1b[6n', 1), 1);
});

test('output route answers each live ConPTY query with exactly one ESC[1;1R write to that session, on Windows only', async () => {
  const frame = (sessionId, text, sequence) => ({ type: 'output', sessionId, sequence, data: Buffer.from(text).toString('base64') });
  const sent = [];
  const output = new Map();
  const waiters = new Set();
  const { route, replyErrors } = createOutputRoute({ platform: 'windows', output, waiters, send: async (f) => { sent.push(f); return { type: 'writeOk' }; } });
  let prompt;
  waiters.add({ sessionId: 'S1', marker: '', visible: true, resolve: (t) => { prompt = t; } });
  route(frame('S1', '\x1b[', 1));
  route(frame('S1', '6n', 2));
  assert.equal(prompt, undefined, 'the query alone never satisfies a visible wait');
  route(frame('S1', '\x1b[6n', 2)); // duplicate sequence
  route(frame('S2', '\x1b[6n', 1));
  route(frame('S1', 'PS C:\\r> ', 3));
  await Promise.resolve();
  assert.deepEqual(sent.map((f) => [f.type, f.sessionId, Buffer.from(f.data, 'base64').toString()]), [['write', 'S1', DSR_REPLY], ['write', 'S2', DSR_REPLY]]);
  // Output storage is unchanged (every received frame is kept); only the replies are deduplicated.
  assert.equal(prompt, '\x1b[6n\x1b[6nPS C:\\r> ');
  assert.deepEqual(replyErrors, []);
  // A failing reply write is kept, not swallowed.
  const failing = createOutputRoute({ platform: 'windows', output: new Map(), waiters: new Set(), send: async () => { throw new Error('closed'); } });
  failing.route(frame('S9', '\x1b[6n', 1));
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(failing.replyErrors, [{ sessionId: 'S9', error: 'closed' }]);
  // Attach history goes through answer({ history: true }): answered once per session.
  const h = createOutputRoute({ platform: 'windows', output: new Map(), waiters: new Set(), send: async (f) => { sent.push(f); } });
  sent.length = 0;
  assert.equal(h.answer('H1', '\x1b[6n', 1, { history: true }), 1);
  assert.equal(h.answer('H1', '\x1b[6n', 1, { history: true }), 0);
  assert.deepEqual(sent.map((f) => f.sessionId), ['H1']);
  // Linux and mac: no replies (the harness is not the terminal of a Unix PTY start-up).
  for (const platform of ['linux', 'mac']) {
    const unix = [];
    createOutputRoute({ platform, output: new Map(), waiters: new Set(), send: async (f) => { unix.push(f); } }).route(frame('S1', '\x1b[6n', 1));
    assert.deepEqual(unix, []);
  }
});

/** Records every data frame type the fake agent of an ownerWorld receives. */
function frameLog(ctx) {
  const frames = [];
  const real = ctx.agent.call;
  ctx.agent.call = async (method, params = {}) => {
    if (method === 'call') frames.push(params.frame.type ?? params.frame.op);
    return real(method, params);
  };
  return frames;
}

test('a retired owner forgets the workspace registration, so the next owner is registered again (Q08b/Q13)', async () => {
  const { ctx } = ownerWorld({ owner: 1, publish: [{ pid: 9, alive: true }, { pid: 10, alive: true }] });
  const frames = frameLog(ctx);
  ctx.workspaceRegistered = false;
  await freshOwner(ctx, { name: 'A' });
  assert.equal(ctx.workspaceRegistered, true);
  await invalidateOwnerState(ctx);
  assert.equal(ctx.workspaceRegistered, false, 'the next owner has not seen registerWorkspace');
  await freshOwner(ctx, { name: 'v1bump' });
  assert.equal(frames.filter((f) => f === 'registerWorkspace').length, 2, 'one registration per owner');
});

test('an older owner never inherits the HEAD version-2 catalog: it is set aside as evidence and registration is redone', async () => {
  const files = new Map([['/r/data/remote/machine-workspaces.v1.json', '{"version":2,"workspaces":{}}']]);
  const ops = [];
  const ctx = {
    layout: { data: '/r/data', root: '/r' }, workspaceRegistered: true,
    agent: { call: async (m, a) => {
      ops.push([m, a.file ?? a.target]);
      if (m === 'readFile') return files.get(a.file) ?? null;
      if (m === 'writeFile') { files.set(a.file, a.content); return true; }
      if (m === 'remove') { files.delete(a.target); return true; }
      throw new Error(m);
    } },
  };
  assert.equal(workspaceCatalogFile(ctx), '/r/data/remote/machine-workspaces.v1.json');
  const out = await setAsideHeadCatalog(ctx, 'Q08b');
  assert.deepEqual([out.setAside, out.version], ['/r/set-aside/Q08b-machine-workspaces.v1.json', 2]);
  assert.equal(files.has(workspaceCatalogFile(ctx)), false);
  assert.equal(files.get(out.setAside), '{"version":2,"workspaces":{}}', 'kept byte for byte');
  assert.equal(ctx.workspaceRegistered, false);
  // No catalog: nothing moved, registration still redone.
  ctx.workspaceRegistered = true;
  assert.equal((await setAsideHeadCatalog(ctx, 'Q13')).setAside, null);
  assert.equal(ctx.workspaceRegistered, false);
});

test('a scenario exception retires the leftover owner and starts a fresh A owner before the next scenario (Q08b -> Q10)', async () => {
  // Owner 1 is the failed scenario's (v1bump) owner; its GUI 5 is still up.
  const { w, ctx } = ownerWorld({ owner: 1, alive: [1], publish: [{ pid: 40, alive: true }], sessions: ['SSH-left', 'L-left'] });
  ctx.state = { gui: { tag: 'Q08B', pid: 5 }, conn: { close: async () => { w.closed.push('old'); } }, localA: 'OLD' };
  ctx.workspaceRegistered = true;
  const frames = frameLog(ctx);
  const out = await recoverOwnerAfterException(ctx, ctx.artifacts.A);
  assert.equal(out.ok, true, JSON.stringify(out));
  assert.deepEqual(w.kills, [5], 'the failed scenario GUI is killed by recorded pid, never a focused-window quit');
  assert.deepEqual(w.quits, []);
  assert.ok(frames.includes('RetireWhenIdle'));
  // run-r5c: the list carries bare id strings; each leftover session is closed by its id, so the
  // owner retires on its own and is never killed.
  assert.deepEqual(w.sessionCloses, ['SSH-left', 'L-left']);
  assert.deepEqual(out.closed.map((c) => [c.id, c.resp.type]), [['SSH-left', 'closeOk'], ['L-left', 'closeOk']]);
  assert.equal(w.sessions.size, 0);
  assert.deepEqual(w.registryKills, [], 'a retired owner is not killed');
  assert.equal(w.alive.has(1), false, 'leftover owner retired');
  assert.deepEqual([out.fresh, ctx.state.daemon.pid], [40, 40]);
  assert.ok(frames.includes('registerWorkspace'), 'the fresh owner is registered again');
  // run.mjs records the recovery on the failed scenario and keeps the original failure.
  const result = { id: 'Q08b', assertions: [{ name: 'scenario completed without an exception', ok: false }], evidence: [] };
  const written = [];
  const rec = await recoverAfterScenarioException(ctx, result, { writeFileSync: (f, t) => written.push([f, t]), localEvidence: '/ev', recover: async () => ({ ok: false, code: 'OWNER_STATE_UNRECOVERABLE' }) });
  assert.equal(rec.ok, false);
  assert.deepEqual(result.assertions.map((a) => [a.name, a.ok]), [['scenario completed without an exception', false], ['owner of the failed scenario retired and a fresh QA owner started', false]]);
  assert.deepEqual(result.evidence, ['/ev/Q08b-exception-recovery.json']);
  assert.equal(written[0][0], '/ev/Q08b-exception-recovery.json');
});

test('an owner that keeps live sessions past its retire deadline is killed by its recorded pid, then a fresh A starts', async () => {
  const { w, ctx } = ownerWorld({ owner: 1, alive: [1], publish: [{ pid: 40, alive: true }], sessions: ['S-held'], stuck: true });
  ctx.state = { gui: null };
  const out = await recoverOwnerAfterException(ctx, ctx.artifacts.A);
  assert.equal(out.ok, true, JSON.stringify(out));
  assert.deepEqual(w.sessionCloses, ['S-held']);
  assert.deepEqual(w.registryKills, [{ pid: 1, signal: 'SIGTERM' }], 'only the recorded owner pid, through the registry');
  assert.deepEqual([out.killed.exited, out.retired.retired, out.fresh], [true, true, 40]);
});

test('a kill the registry refuses leaves the owner unrecoverable, never thrown', async () => {
  const { w, ctx } = ownerWorld({ owner: 1, alive: [1], sessions: ['S-held'], stuck: true });
  ctx.state = { gui: null };
  w.killRefused.add(1);
  const out = await recoverOwnerAfterException(ctx, ctx.artifacts.A);
  assert.deepEqual([out.ok, out.code, out.killed.exited, out.killed.steps[0].refused], [false, 'OWNER_STATE_UNRECOVERABLE', false, 'KILL_REFUSED']);
  assert.equal(out.fresh, null);
});

test('a dead owner that cannot be retired is reported, never thrown', async () => {
  const { ctx } = ownerWorld({ owner: 1, alive: [1], starting: '{bad' });
  ctx.state = { gui: null };
  // Owner 1 alive but retirement through control fails: RetireWhenIdle answers not ok.
  const real = ctx.agent.call;
  ctx.agent.call = async (m, p = {}) => (m === 'call' && p.frame?.op === 'RetireWhenIdle' ? { ok: false } : real(m, p));
  const out = await recoverOwnerAfterException(ctx, ctx.artifacts.A);
  assert.deepEqual([out.ok, out.code], [false, 'OWNER_STATE_UNRECOVERABLE']);
});

/** GUI log + MahoRD whose Ctrl+, opens Settings only from the press numbered in \`opensOn\`. */
function settingsWorld({ opensOn, prior = 0 }) {
  let log = `${SETTINGS_OPENED_RECEIPT}\n`.repeat(prior);
  const presses = [];
  const ctx = {
    mahord: { hotkey: async (keys) => { presses.push(keys.join('+')); if (opensOn.includes(presses.length)) log += `x ${SETTINGS_OPENED_RECEIPT} supported=true\n`; } },
    agent: { call: async (m, a) => {
      if (m === 'readFile') return log;
      if (m === 'waitLog') { const r = new RegExp(a.pattern).exec(log); return r ? { matched: r[0] } : { matched: null, timedOut: true }; }
      throw new Error(m);
    } },
  };
  return { ctx, presses };
}

test('Settings counts as open only on a new cmd_daemon_supervisor_get receipt, with a bounded retry (Q10)', async () => {
  // An earlier GUI session's receipt in the log never counts for this press.
  assert.ok(new RegExp(nthReceiptPattern(SETTINGS_OPENED_RECEIPT, 2)).test(`${SETTINGS_OPENED_RECEIPT}\n${SETTINGS_OPENED_RECEIPT}`));
  assert.equal(new RegExp(nthReceiptPattern(SETTINGS_OPENED_RECEIPT, 2)).test(SETTINGS_OPENED_RECEIPT), false);
  const first = settingsWorld({ opensOn: [1], prior: 1 });
  const ok = await openSettings(first.ctx, '/l/gui.log');
  assert.deepEqual([ok.opened, first.presses.length, ok.attempts[0].before], [true, 1, 1]);
  // run-9a5db442: the first press reached a webview that had not rendered yet.
  const late = settingsWorld({ opensOn: [2] });
  const retried = await openSettings(late.ctx, '/l/gui.log');
  assert.deepEqual([retried.opened, late.presses], [true, ['Control+,', 'Control+,']]);
  const never = settingsWorld({ opensOn: [] });
  const failed = await openSettings(never.ctx, '/l/gui.log');
  assert.deepEqual([failed.opened, never.presses.length], [false, SETTINGS_OPEN_ATTEMPTS]);
});

/**
 * Q11 fake world. The remote log records every write that reaches the SSH session from any
 * connection (M12: a resend on reconnect through another handle lands in it too), and the GUI's
 * MahoRD keystrokes are the only path that commits the marker line. Variants inject the defects
 * each assertion exists to catch.
 */
function q11World({ replayOnRelaunch = false, resendOnReconnect = false, releaseBeforeQuit = false, wrongAttach = false, setupWizard = false, behindConsole = false, platform = 'linux', noWizardEvent = false } = {}) {
  const sid = 'SSH1';
  const w = { log: '', line: '', echo: '', receipts: [], events: [], sessionFile: null, launches: [], quits: [], quitWhileHeld: null, writesByHandle: {}, typedBy: [], held: false, escapes: 0, wizardOpen: false, pendingAttach: null, focused: true, focusCalls: 0, lostKeys: 0, appender: false, debug: [], debugDir: null, wizardArmed: false };
  const call = async (method, params = {}) => {
    switch (method) {
      case 'writeFile':
        if (params.file.endsWith('write-ack-hold.request.json')) { w.receipts.push(`{"event":"armed","sessionId":"${sid}"}`); w.held = true; }
        if (params.file.endsWith('write-ack-hold.release.json')) { w.receipts.push(`{"event":"released","sessionId":"${sid}"}`); w.held = false; }
        if (params.file.endsWith('session_state.json')) w.sessionFile = params.content;
        return true;
      case 'readFile':
        if (params.file.endsWith('write-ack-hold.receipt.jsonl')) return w.receipts.join('\n');
        if (params.file.endsWith('session_state.json')) return w.sessionFile;
        // Windows Add-Content writes CRLF; the remote file is read where it lies.
        if (params.file.endsWith('ssh-input.log')) return platform === 'windows' ? w.log.replace(/\n/g, '\r\n') : w.log;
        return null;
      case 'mkdir': return true;
      case 'waitLog': {
        // The GUI's switch-debug log, written under its TEMP (Windows): absent unless Q11 redirected TEMP there.
        if (params.file.endsWith('ferryx-switch-debug.jsonl')) {
          const text = w.debugDir && params.file.startsWith(w.debugDir) ? w.debug.join('\n') : '';
          const d = new RegExp(params.pattern, params.flags ?? '').exec(text);
          return d ? { matched: d[0] } : { matched: null, timedOut: true };
        }
        const text = params.file.endsWith('native-events.jsonl') ? w.events.join('\n') : w.receipts.join('\n');
        const r = new RegExp(params.pattern, params.flags ?? '').exec(text);
        return r ? { matched: r[0] } : { matched: null, timedOut: true };
      }
      case 'dataOpen': return { handle: `h${Object.keys(w.writesByHandle).length + 1}`, handshake: { type: 'handshakeOk' } };
      case 'call': {
        const f = params.frame;
        if (f.type === 'describeLiveSessions') return { type: 'describeLiveSessionsOk', sessions: [{ kind: 'ssh', backendId: sid, remotePid: 77, localChildPid: null, readerOpen: true, connected: true }] };
        if (f.type === 'spawn') return { type: 'spawnOk', sessionId: sid, epoch: 3, session: { sessionId: sid, incarnation: 'qa-q11-inc' } };
        if (f.type === 'write') {
          const text = Buffer.from(f.data, 'base64').toString();
          (w.writesByHandle[params.handle] ??= []).push(text);
          if (text === '\u0003') {
            w.line = '';
            // A harness resend of the marker on the reconnected handle reaches the session too.
            if (resendOnReconnect && w.lastMarker) w.log += `${w.lastMarker}\n`;
          } else if (text.startsWith('cat >> ')) {
            // The SSH session's shell: a POSIX shell runs cat; Windows cmd.exe has none (run-q11 r4).
            w.appender = platform !== 'windows';
          } else if (text.startsWith('powershell ')) {
            w.appender = platform === 'windows' && text.endsWith('\r');
            if (w.appender) w.echo += 'Q11APPENDREADY\r\n';
          } else {
            w.log += text;
          }
          return { type: 'writeOk' };
        }
        return { type: `${f.type}Ok` };
      }
      case 'waitOutput': { const text = w.echo.slice(params.since ?? 0); const n = text.split(params.marker).length - 1; return n > 0 ? { count: n, text } : { timedOut: true, count: 0 }; }
      case 'outputOf': return w.echo;
      case 'close': return true;
      case 'exec': return { status: 0, stdout: w.log, stderr: '' };
      // build-log branch: Q11 records this run's SSH remote-helper pids for teardown.
      case 'recordSshHelper': return [];
      default: throw new Error(`unexpected agent call ${method}`);
    }
  };
  const conn = { handle: 'h0', call: (frame) => call('call', { handle: 'h0', frame }), describe: () => call('call', { handle: 'h0', frame: { type: 'describeLiveSessions' } }), waitOutput: (s, marker, _ms, { since = 0 } = {}) => call('waitOutput', { marker, since }), outputOf: () => call('outputOf', {}), outputLength: async () => (await call('outputOf', {})).length, close: async () => {} };
  const restoredTab = () => {
    const doc = w.sessionFile ? JSON.parse(w.sessionFile) : null;
    const ws = doc && Object.values(doc.workspaces).find((x) => x.target?.kind === 'ssh');
    return ws && Object.values(ws.terminalSessions).find((s) => s.backendSessionId === sid);
  };
  const typeChar = (ch) => { if (!w.typedBy.includes(w.launches.at(-1))) w.typedBy.push(w.launches.at(-1)); w.line += ch; w.echo += ch; };
  // Windows first run (run-q11 all): the wizard opens from a 1.2 s timer, after the tab attached, and
  // covers the pane: a key that arrives while it is up never reaches the terminal.
  const coveredByWizard = () => { if (!w.wizardArmed) return false; w.lostKeys += 1; return true; };
  // Windows first run (run-q11 Q11-attach-screenshot.png): the setup wizard is a role="dialog" that
  // hides native surfaces off macOS, so the restored tab attaches only after Escape closes it.
  const flushAttach = () => { if (w.pendingAttach) { w.events.push(w.pendingAttach); w.pendingAttach = null; } };
  const ctx = {
    agent: { call }, runId: RUN, platform, hooks: { writeAckHoldArm: { present: true } },
    sshd: { hostId: 'qa-sshd-x', workspaceId: 'ssh:x', hostStorePath: '/d/ssh_hosts.json', repoRoot: '/r/ssh-repo', clientKey: '/k', knownHosts: '/kh', port: 2, user: 'u' },
    layout: { root: '/r', runtime: '/r/run', barriers: '/r/barriers', sessions: '/r/sessions' }, evidenceDir: '/e', writeEvidence: async () => {},
    // MahoRD keys one at a time (lib/typeverified.mjs): each printable key reaches the line and its
    // echo; Enter commits the line. A whole-text burst is not used (MahoRD drops keys from it).
    mahord: {
      // A GUI behind the console that started it (Windows run-q11 r2) gets no key until focused.
      hotkey: async (keys) => { assert.equal(keys[0], 'Shift'); if (!w.focused) { w.lostKeys += 1; return; } if (coveredByWizard()) return; typeChar(keys[1].toUpperCase()); },
      key: async (key) => {
        if (!w.focused) { w.lostKeys += 1; return; }
        if (key === 'Escape') {
          w.escapes += 1;
          w.wizardOpen = false;
          if (w.wizardArmed) { w.wizardArmed = false; w.debug.push(`{"event":"terminal.surface.input.gate.state","details":{"paneIdentity":"q11-pane-${RUN}","visible":true}}`); }
          flushAttach();
          return;
        }
        if (coveredByWizard()) return;
        assert.equal(w.wizardOpen, false, 'a key reached the GUI while the setup wizard owned the input');
        if (key !== 'Enter') { typeChar(key); return; }
        w.lastMarker = w.line;
        // The GUI writes the Enter; the line reaches cat's log and the daemon parks the ack.
        if (w.appender) w.log += `${w.line}\n`;
        w.line = '';
        w.receipts.push(`{"event":"write-applied","sessionId":"${sid}","seq":1}`);
        if (releaseBeforeQuit) w.receipts.push(`{"event":"released","sessionId":"${sid}"}`);
      },
    },
    gui: {
      focus: async () => { w.focusCalls += 1; w.focused = true; return { focused: true }; },
      quit: async (g) => { w.quits.push(g.pid); if (w.receipts.some((r) => r.includes('write-applied'))) w.quitWhileHeld ??= w.held; return { code: 0 }; },
      launch: async (_artifact, { env } = {}) => {
        const pid = 10 + w.launches.length;
        w.launches.push(pid);
        w.focused = !behindConsole;
        const tab = restoredTab();
        if (tab && env?.FERRYX_QA_OPERATION_ID) {
          const tuple = { backendSessionId: wrongAttach ? 'OTHER' : tab.backendSessionId, incarnation: tab.incarnation, daemonEpoch: tab.daemonEpoch, frontendSessionId: tab.localSessionId, paneIdentity: tab.localSessionId, bindingKey: 'k', attemptGeneration: 0 };
          w.pendingAttach = JSON.stringify({ runId: RUN, operationId: env.FERRYX_QA_OPERATION_ID, kind: 'attached', attachTuple: tuple });
          w.wizardOpen = setupWizard;
          if (!setupWizard) flushAttach();
          // Windows: the wizard always opens on a fresh QA profile; the pane logs it to the GUI's TEMP.
          if (platform === 'windows') {
            w.wizardArmed = true;
            w.debugDir = env.TEMP ?? null;
            if (!noWizardEvent) w.debug.push(`{"event":"terminal.surface.skipped","details":{"localSessionId":"${tab.localSessionId}","reason":"surfaceVisible=false yielding=1 which=dialog:Welcome to Ferryx"}}`);
          }
        }
        // A product that replays the uncertain write after relaunch.
        if (replayOnRelaunch && w.launches.length > 1 && w.lastMarker) w.log += `${w.lastMarker}\n`;
        return { tag: `g${pid}`, pid };
      },
    },
    artifacts: { A: { name: 'A' } }, state: { conn, gui: { tag: 'g8', pid: 8 } },
  };
  return { w, ctx, sid };
}

async function runQ11(opts) {
  const world = q11World(opts);
  const result = { id: 'Q11', assertions: [], evidence: [] };
  await Q11(world.ctx, result);
  return { ...world, result, failed: result.assertions.filter((a) => !a.ok).map((a) => a.name) };
}

test('Q11 types the marker through a GUI tab restored onto the SSH session and quits while the ack is held', async () => {
  const { w, result, failed, sid } = await runQ11();
  assert.deepEqual(failed, [], JSON.stringify(result.assertions.filter((a) => !a.ok)));
  // The tab came from the GUI's own restore path: a saved SSH workspace bound to this backend.
  const restored = JSON.parse(w.sessionFile).workspaces['ssh:x'];
  assert.deepEqual(restored.target, { kind: 'ssh', hostId: 'qa-sshd-x' });
  assert.equal(Object.values(restored.terminalSessions)[0].backendSessionId, sid);
  assert.deepEqual(w.quits, [8, 10], 'the old GUI quits before the seeded restore, the restored GUI quits while held');
  assert.deepEqual(w.typedBy, [10], 'the marker is typed into the GUI that attached the restored tab');
  assert.equal(w.quitWhileHeld, true);
  assert.match(w.log, /^UNCERTAIN[0-9A-F]{12}\n$/);
  assert.equal(w.escapes, 0, 'no Escape when the tab attached without a setup wizard');
  // No data connection writes the marker: only the cat command and the closing interrupt.
  for (const writes of Object.values(w.writesByHandle)) for (const text of writes) assert.ok(!text.includes('UNCERTAIN'), text);
});

test('Q11 fails when the uncertain write is replayed after the relaunch', async () => {
  const { failed } = await runQ11({ replayOnRelaunch: true });
  assert.ok(failed.includes('remote log contains the marker exactly once (no replay of the uncertain write)'), failed.join('; '));
});

test('Q11 fails when the marker is resent through a reconnected data connection (M12)', async () => {
  const { failed } = await runQ11({ resendOnReconnect: true });
  assert.ok(failed.includes('remote log contains the marker exactly once (no replay of the uncertain write)'), failed.join('; '));
});

test('Q11 fails when the ack is no longer held at the GUI quit (M13)', async () => {
  const { failed } = await runQ11({ releaseBeforeQuit: true });
  assert.ok(failed.includes('the ack was still held when the GUI quit'), failed.join('; '));
});

test('Q11 never types when no GUI tab is attached to the SSH session', async () => {
  const { w, failed } = await runQ11({ wrongAttach: true });
  assert.deepEqual(failed, ['a GUI tab is attached to the SSH session (native attach receipt)']);
  assert.deepEqual(w.typedBy, []);
  assert.equal(w.log, '');
});

test('Q11 closes a first-run setup wizard with one Escape, then the restored tab attaches (Windows run-q11)', async () => {
  const { w, result, failed } = await runQ11({ setupWizard: true });
  assert.deepEqual(failed, [], JSON.stringify(result.assertions.filter((a) => !a.ok)));
  assert.equal(w.escapes, 1);
  assert.match(w.log, /^UNCERTAIN[0-9A-F]{12}\n$/);
  assert.equal(result.assertions.find((a) => a.name.startsWith('a GUI tab is attached')).detail.attached.escapeSent, true);
});

test('Q11 focuses the GUI before its Escape and before typing when it opened behind another window (Windows run-q11 r2)', async () => {
  const { w, result, failed } = await runQ11({ setupWizard: true, behindConsole: true });
  assert.deepEqual(failed, [], JSON.stringify(result.assertions.filter((a) => !a.ok)));
  assert.deepEqual([w.escapes, w.lostKeys], [1, 0]);
  assert.match(w.log, /^UNCERTAIN[0-9A-F]{12}\n$/);
});

test('Q11 focuses a GUI that attached behind another window before typing the marker, without an Escape', async () => {
  const { w, result, failed } = await runQ11({ behindConsole: true });
  assert.deepEqual(failed, [], JSON.stringify(result.assertions.filter((a) => !a.ok)));
  assert.deepEqual([w.escapes, w.lostKeys], [0, 0]);
  assert.match(w.log, /^UNCERTAIN[0-9A-F]{12}\n$/);
});

test('Q11 on Windows appends through a PowerShell line loop (cmd.exe has no cat) and reads the CRLF file (run-q11 r4)', async () => {
  const { w, result, failed } = await runQ11({ platform: 'windows' });
  assert.deepEqual(failed, [], JSON.stringify(result.assertions.filter((a) => !a.ok)));
  assert.match(w.log, /^UNCERTAIN[0-9A-F]{12}\n$/);
  assert.ok(result.assertions.some((a) => a.name === 'the remote line appender is running in the SSH session' && a.ok));
});

test('Q11 on Windows closes the setup wizard that opens after the tab attached before typing a key (run-q11 all)', async () => {
  const { w, result, failed } = await runQ11({ platform: 'windows' });
  assert.deepEqual(failed, [], JSON.stringify(result.assertions.filter((a) => !a.ok)));
  assert.deepEqual([w.escapes, w.lostKeys], [1, 0], 'one Escape, and no key was swallowed by the wizard');
  const wizard = result.assertions.find((a) => a.name === 'the first-run setup wizard opened and was closed before any key was typed');
  assert.deepEqual([wizard?.ok, wizard?.detail.opened, wizard?.detail.closed], [true, true, true]);
  assert.ok(wizard.detail.focus.length === 1, 'the GUI is focused before its Escape');
});

test('Q11 on Windows never types when the setup wizard event never shows', async () => {
  const { w, failed } = await runQ11({ platform: 'windows', noWizardEvent: true });
  assert.deepEqual(failed, ['the first-run setup wizard opened and was closed before any key was typed']);
  assert.deepEqual([w.typedBy, w.escapes], [[], 0]);
});

test('Q11 on Windows focuses a GUI that opened behind the console before the wizard Escape (run-q11 r2)', async () => {
  const { w, result, failed } = await runQ11({ platform: 'windows', behindConsole: true });
  assert.deepEqual(failed, [], JSON.stringify(result.assertions.filter((a) => !a.ok)));
  assert.deepEqual([w.escapes, w.lostKeys], [1, 0], 'the Escape reached the GUI, not the console');
});

test('Q11 waits for the setup wizard only on Windows', async () => {
  const { expectsSetupWizard, wizardOpenPattern, wizardClosedPattern } = await import('./scenarios/q10-q13.mjs');
  assert.deepEqual(['windows', 'linux', 'mac'].map(expectsSetupWizard), [true, false, false]);
  // Lines as the GUI wrote them on maho-win (run-q11 all/Q11-switch-debug.jsonl).
  const pane = 'q11-pane-20261021t220833z';
  const skipped = `{"event":"terminal.surface.skipped","details":{"localSessionId":"${pane}","reason":"surfaceVisible=false element=true tauri=true target=true yielding=1 which=dialog:Welcome to Ferryx"}}`;
  const visible = `{"event":"terminal.surface.input.gate.state","details":{"paneIdentity":"${pane}","backendSessionId":"x","visible":true,"interactive":true}}`;
  assert.ok(new RegExp(wizardOpenPattern(pane)).test(skipped));
  assert.ok(!new RegExp(wizardOpenPattern('other-pane')).test(skipped));
  assert.ok(!new RegExp(wizardClosedPattern(pane)).test(`${visible}\n${skipped}`), 'a visible gate before the wizard is not its close');
  assert.ok(new RegExp(wizardClosedPattern(pane)).test(`${visible}\n${skipped}\n${visible}`));
});
