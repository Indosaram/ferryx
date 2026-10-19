import test from 'node:test';
import assert from 'node:assert/strict';
import {
  makeRunId, assertRunId, qaSupervisorName, qaBundleIdentifier, tauriQaConfig, isQaJobName, assertBundleIdentifier,
  forbiddenRoots, assertQaExecutable, isolationLayout, isolationEnv, assertIsolatedEnv, classifyCommand, ProcessRegistry, GuardError,
} from './lib/guards.mjs';

const RUN = '20261019t064512z';
const OTHER = '20261019t064513z';

test('run id is lowercase UTC and valid in job names', () => {
  assert.equal(makeRunId(new Date('2026-10-19T06:45:12.345Z')), RUN);
  assert.equal(assertRunId(RUN), RUN);
  assert.throws(() => assertRunId('20261019T064512Z'), GuardError);
  assert.throws(() => qaSupervisorName('ferryx-daemon'), GuardError);
});

test('QA bundle identity never equals the release identity', () => {
  assert.equal(qaBundleIdentifier(RUN), `com.ferryx.app.qa-${RUN}`);
  assert.equal(tauriQaConfig(RUN).identifier, qaBundleIdentifier(RUN));
  assert.equal(tauriQaConfig(RUN).plugins.updater.active, false);
  assert.equal(assertBundleIdentifier(` com.ferryx.app.qa-${RUN}\n`, RUN), qaBundleIdentifier(RUN));
  assert.throws(() => assertBundleIdentifier('com.ferryx.app', RUN), /BUNDLE_ID_MISMATCH/);
  assert.throws(() => assertBundleIdentifier(qaBundleIdentifier(OTHER), RUN), /BUNDLE_ID_MISMATCH/);
});

test('job names match only this run', () => {
  const base = `ferryx-qa-${RUN}`;
  assert.ok(isQaJobName(base, RUN, 'mac'));
  assert.ok(isQaJobName(`${base}.2026.1019.1-abcdef01`, RUN, 'mac'));
  assert.ok(!isQaJobName('com.ferryx.daemon', RUN, 'mac'));
  assert.ok(!isQaJobName(`ferryx-qa-${OTHER}.x`, RUN, 'mac'));
  assert.ok(!isQaJobName(`${base}x`, RUN, 'mac'));
  assert.ok(isQaJobName(`${base}-2026.1019.1-abcdef01.service`, RUN, 'linux'));
  assert.ok(isQaJobName(`${base}-gui-g1.scope`, RUN, 'linux'));
  assert.ok(!isQaJobName('ferryx-daemon.service', RUN, 'linux'));
  assert.ok(isQaJobName(`\\Ferryx\\${base}-k`, RUN, 'windows'));
  assert.ok(!isQaJobName('Ferryx\\Daemon', RUN, 'windows'));
});

test('executables: QA roots allowed, release and live-daemon roots refused', () => {
  const forbidden = forbiddenRoots({ platform: 'mac', home: '/Users/q', uid: 501 });
  const allowedRoots = ['/tmp/ferryx-qa-daemon-lifetime/' + RUN, '/tmp/ferryx-t12-target'];
  assert.ok(assertQaExecutable('/tmp/ferryx-t12-target/debug/ferryx', { allowedRoots, forbidden, platform: 'mac' }));
  assert.throws(() => assertQaExecutable('/Applications/Ferryx.app/Contents/MacOS/ferryx', { allowedRoots, forbidden, platform: 'mac' }), /RELEASE_PATH/);
  assert.throws(() => assertQaExecutable('/tmp/rorca-501/x', { allowedRoots: ['/tmp'], forbidden, platform: 'mac' }), /RELEASE_PATH/);
  assert.throws(() => assertQaExecutable('/usr/local/bin/ferryx', { allowedRoots, forbidden, platform: 'mac' }), /NOT_A_QA_ARTIFACT/);
  const wf = forbiddenRoots({ platform: 'windows', home: 'C:\\Users\\sook' });
  assert.throws(() => assertQaExecutable('c:\\users\\SOOK\\appdata\\local\\Programs\\Ferryx\\ferryx.exe', { allowedRoots: ['C:\\'], forbidden: wf, platform: 'windows' }), /RELEASE_PATH/);
});

test('isolation env points every daemon location into the run dir', () => {
  const layout = isolationLayout('/tmp/qa/' + RUN, 'mac');
  const env = isolationEnv({ runId: RUN, layout, platform: 'mac' });
  assert.equal(env.FERRYX_RUNTIME_DIR, layout.runtime);
  assert.equal(env.FERRYX_DAEMON_PLIST_PATH, layout.launchAgents);
  assert.equal(env.HOME, layout.home);
  const linux = isolationEnv({ runId: RUN, layout: isolationLayout('/tmp/qa/' + RUN, 'linux'), platform: 'linux' });
  assert.equal(linux.HOME, undefined);
  const forbidden = forbiddenRoots({ platform: 'mac', home: '/Users/q', uid: 501 });
  assert.ok(assertIsolatedEnv(env, { runId: RUN, forbidden, platform: 'mac' }));
  assert.throws(() => assertIsolatedEnv({ ...env, FERRYX_RUNTIME_DIR: '/tmp/rorca-501' }, { runId: RUN, forbidden, platform: 'mac' }), /ENV_NOT_ISOLATED/);
  assert.throws(() => assertIsolatedEnv({ ...env, FERRYX_DAEMON_SUPERVISOR_NAME: 'ferryx-daemon' }, { runId: RUN, forbidden, platform: 'mac' }), /ENV_NOT_ISOLATED/);
  const { FERRYX_SESSION_DIR, ...missing } = env;
  assert.ok(FERRYX_SESSION_DIR);
  assert.throws(() => assertIsolatedEnv(missing, { runId: RUN, forbidden, platform: 'mac' }), /ENV_NOT_ISOLATED/);
});

test('OS-job commands: reads allowed, mutations only on QA jobs, kills never', () => {
  const ctx = (platform) => ({ runId: RUN, platform });
  const base = `ferryx-qa-${RUN}`;
  const ok = (c, a, p) => classifyCommand(c, a, ctx(p)).allowed;
  assert.ok(ok('launchctl', ['print', 'gui/501/com.ferryx.daemon'], 'mac'));
  assert.ok(ok('launchctl', ['kickstart', `gui/501/${base}.k`], 'mac'));
  assert.ok(!ok('launchctl', ['kickstart', '-k', `gui/501/${base}.k`], 'mac'));
  assert.ok(!ok('launchctl', ['bootout', 'gui/501/com.ferryx.daemon'], 'mac'));
  assert.ok(ok('launchctl', ['bootstrap', 'gui/501', `/tmp/LaunchAgents/${base}.k.plist`], 'mac'));
  assert.ok(!ok('launchctl', ['bootstrap', 'gui/501', '/Users/q/Library/LaunchAgents/com.ferryx.daemon.plist'], 'mac'));
  assert.ok(!ok('launchctl', ['remove', `${base}.k`], 'mac'));
  assert.ok(ok('systemctl', ['--user', 'show', 'ferryx-daemon.service'], 'linux'));
  assert.ok(ok('systemctl', ['--user', 'start', '--no-block', `${base}-k.service`], 'linux'));
  assert.ok(!ok('systemctl', ['--user', 'stop', 'ferryx-daemon.service'], 'linux'));
  assert.ok(!ok('systemctl', ['stop', `${base}-k.service`], 'linux'));
  assert.ok(!ok('systemctl', ['--user', 'disable', '--now', `${base}-k.service`], 'linux'));
  assert.ok(ok('schtasks', ['/Query', '/TN', 'Ferryx\\Daemon'], 'windows'));
  assert.ok(ok('schtasks', ['/Run', '/TN', `Ferryx\\${base}-k`], 'windows'));
  assert.ok(!ok('schtasks', ['/End', '/TN', 'Ferryx\\Daemon'], 'windows'));
  for (const k of ['kill', 'pkill', 'killall', 'taskkill.exe']) assert.ok(!ok(k, ['-9', '1'], 'mac'));
});

test('ProcessRegistry kills only a recorded pid whose identity still matches', async () => {
  const table = new Map([[10, { executable: '/qa/ferryx', startTime: 'T1' }], [11, { executable: '/qa/ferryx', startTime: 'T1' }]]);
  const sent = [];
  const reg = new ProcessRegistry({ probe: async (pid) => table.get(pid) ?? null, signal: async (pid, sig) => sent.push([pid, sig]), platform: 'mac' });
  await reg.record(10, { role: 'daemon', executable: '/qa/ferryx' });
  await assert.rejects(reg.record(11, { role: 'gui', executable: '/Applications/Ferryx.app/ferryx' }), /IDENTITY_MISMATCH/);
  await assert.rejects(reg.record(99, { role: 'gone', executable: '/qa/ferryx' }), /PID_GONE/);
  await assert.rejects(reg.kill(11), /KILL_REFUSED/);
  table.set(10, { executable: '/qa/ferryx', startTime: 'T2' }); // pid reused
  assert.equal(await reg.check(10), 'mismatch');
  await assert.rejects(reg.kill(10), /KILL_REFUSED/);
  table.set(10, { executable: '/qa/ferryx', startTime: 'T1' });
  assert.equal(await reg.kill(10, 'SIGKILL'), true);
  table.delete(10);
  assert.equal(await reg.kill(10), false);
  assert.deepEqual(sent, [[10, 'SIGKILL']]);
});
