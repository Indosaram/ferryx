import test from 'node:test';
import assert from 'node:assert/strict';
import { SCENARIO_IDS, newSummary, selectScenarios, assertion, settle, block, applyQ00Gate, computeExitCode, validateSummary } from './lib/summary.mjs';
import { readFileSync } from 'node:fs';
import { RUNNERS, assertRunnersComplete, parseArgs, artifactPaths, selfTestExitCode, SELF_TEST_PATCH, teardownRun, runWithTeardown, prepareSelfTestWorktree, teardownExtras, releaseOwnerFile, releaseContinuity } from './run.mjs';
import { hostFor } from './lib/hosts.mjs';
import { confirmOwner, ownerStartMatches, ownerIdentity, parseStarting, awaitPendingStart, retireOwner, freshOwner, STARTING_WINDOW_MS } from './scenarios/common.mjs';
import { Q06, killLaunchedGui } from './scenarios/q04-q06.mjs';

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
function ownerWorld({ owner = null, alive = [], starting = null, publish = [] } = {}) {
  const w = { owner, alive: new Set(alive), starting, publish: [...publish], calls: [], closed: [], quits: [], kills: [], launches: [], registryKills: [], killRefused: new Set() };
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
          if (f.op === 'RetireWhenIdle') { w.alive.delete(w.owner); return { ok: true, live: 0 }; }
          if (f.type === 'describeLiveSessions') return { type: 'describeLiveSessionsOk', sessions: [] };
          if (f.type === 'registerWorkspace') return { type: 'registerWorkspaceOk' };
          if (f.type === 'spawn') return { type: 'spawnOk', sessionId: 'S1' };
          if (f.type === 'attach') return { type: 'attachOk' };
          if (f.type === 'write') return { type: 'writeOk' };
          return { type: 'unexpected', frame: f };
        }
        case 'waitOutput': return { count: 1, text: 'QA_A_TICK_2' };
        case 'waitExit': return { pid: params.pid, exitedAt: 1 };
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
