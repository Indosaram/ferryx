import test from 'node:test';
import assert from 'node:assert/strict';
import {
  makeRunId, assertRunId, qaSupervisorName, qaBundleIdentifier, tauriQaConfig, isQaJobName, assertBundleIdentifier,
  forbiddenRoots, assertQaExecutable, isolationLayout, isolationEnv, assertIsolatedEnv, classifyCommand, ProcessRegistry, GuardError,
} from './lib/guards.mjs';
import { probeIdentity, windowsProbeScript } from './lib/proc.mjs';
import { ownerStartMatches } from './scenarios/common.mjs';
import { hostFor } from './lib/hosts.mjs';
import { teardownRun } from './run.mjs';
import { newSummary } from './lib/summary.mjs';
import path from 'node:path';

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
  const split = isolationLayout('/home/q/qaruns/' + RUN, 'linux', { runtimeDir: '/tmp/qa/' + RUN + '/run' });
  assert.equal(split.runtime, '/tmp/qa/' + RUN + '/run');
  assert.equal(split.data, '/home/q/qaruns/' + RUN + '/data');
  assert.equal(split.logs, '/home/q/qaruns/' + RUN + '/logs');
  const forbidden = forbiddenRoots({ platform: 'mac', home: '/Users/q', uid: 501 });
  assert.ok(assertIsolatedEnv(env, { runId: RUN, forbidden, platform: 'mac' }));
  assert.throws(() => assertIsolatedEnv({ ...env, FERRYX_RUNTIME_DIR: '/tmp/rorca-501' }, { runId: RUN, forbidden, platform: 'mac' }), /ENV_NOT_ISOLATED/);
  assert.throws(() => assertIsolatedEnv({ ...env, FERRYX_DAEMON_SUPERVISOR_NAME: 'ferryx-daemon' }, { runId: RUN, forbidden, platform: 'mac' }), /ENV_NOT_ISOLATED/);
  const { FERRYX_SESSION_DIR, ...missing } = env;
  assert.ok(FERRYX_SESSION_DIR);
  assert.throws(() => assertIsolatedEnv(missing, { runId: RUN, forbidden, platform: 'mac' }), /ENV_NOT_ISOLATED/);
});

// lib.rs qa_primary_window_profile: the QA GUI panics unless FERRYX_QA_BARRIER_DIR is an existing
// dir named exactly `barriers` whose parent is named the run id. run.mjs builds runDir as
// <storageRoot ?? runRoot>/<runId> and mkdirs every layout dir before any GUI launch.
test('QA barriers dir: basename barriers, parent basename the run id, created with the layout, on every host', () => {
  for (const platform of ['mac', 'linux', 'windows']) {
    const host = hostFor(platform);
    const mod = platform === 'windows' ? path.win32 : path.posix;
    const runDir = mod.join(host.storageRoot ?? host.runRoot, RUN);
    const runtimeDir = host.storageRoot ? mod.join(host.runRoot, RUN, 'run') : null;
    const layout = isolationLayout(runDir, platform, runtimeDir ? { runtimeDir } : {});
    const dir = isolationEnv({ runId: RUN, layout, platform }).FERRYX_QA_BARRIER_DIR;
    assert.equal(dir, layout.barriers, platform);
    assert.equal(mod.basename(dir), 'barriers', platform);
    assert.equal(mod.basename(mod.dirname(dir)), RUN, platform);
    assert.ok(Object.values(layout).includes(dir), `${platform}: run.mjs creates every layout dir, so the barriers dir exists before launch`);
  }
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
  assert.ok(ok('systemctl', ['--user', 'kill', '--signal=SIGKILL', `${base}-gui-g3.scope`], 'linux'));
  assert.ok(!ok('systemctl', ['--user', 'kill', '--signal=SIGKILL', 'app-foo.scope'], 'linux'));
  assert.ok(!ok('systemctl', ['--user', 'kill', '--signal=SIGKILL'], 'linux'));
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
  assert.equal(await reg.check(10), 'reused');
  assert.equal(await reg.kill(10), false, 'a reused pid is never signalled');
  table.set(10, { executable: '/usr/bin/other', startTime: 'T1' }); // same start, other image: no exit proof
  assert.equal(await reg.check(10), 'mismatch');
  await assert.rejects(reg.kill(10), /KILL_REFUSED/);
  table.set(10, { executable: '/qa/ferryx', startTime: 'T1' });
  assert.equal(await reg.kill(10, 'SIGKILL'), true);
  table.delete(10);
  assert.equal(await reg.kill(10), false);
  assert.deepEqual(sent, [[10, 'SIGKILL']]);
});

test('mac teardown keeps a refused kill diagnosable: the pid is re-probed and its text mappings recorded', async () => {
  const refused = [{ pid: 4242, role: 'daemon', reason: 'KILL_REFUSED' }];
  const calls = [];
  const agent = { call: async (method, params = {}) => {
    calls.push([method, params]);
    if (method === 'exec' && params.command === 'node') return { status: 0, stdout: '[]', stderr: '' };
    if (method === 'exec') return { status: 0, stdout: '/tmp/qa/ferryx\n/usr/lib/libSystem.B.dylib\n', stderr: '' };
    if (method === 'teardownRecorded') return { killed: [], refused, gone: [] };
    if (method === 'probe') return { executable: '/tmp/qa/ferryx', startTime: 'Thu Nov 19 12:31:02 2026' };
    if (method === 'removeSshHelperRoots') return { removed: [], kept: [] };
    if (method === 'table') return [];
    if (method === 'exists') return false;
    return null;
  } };
  const summary = newSummary({ runId: '20261019t064512z', platform: 'mac', host: 'h' });
  await teardownRun({
    summary, agent, jobs: { listQaJobs: async () => [], remove: async () => {} }, host: { platform: 'mac' },
    layout: { logs: '/qa/logs' }, runDirs: ['/qa/run'], qaRoots: ['/qa/run'], localEvidence: '/nonexistent-evidence', prodSnapshot: async () => null,
  });
  assert.deepEqual(summary.teardown.errors, []);
  assert.equal(summary.teardown.refused.length, 1);
  assert.deepEqual(summary.teardown.refused[0].observedAfterRefusal, { executable: '/tmp/qa/ferryx', startTime: 'Thu Nov 19 12:31:02 2026' });
  assert.deepEqual(summary.teardown.refused[0].textMappings, { status: 0, stdout: '/tmp/qa/ferryx\n/usr/lib/libSystem.B.dylib\n', stderr: '' });
  const probe = calls.find(([m]) => m === 'probe');
  assert.deepEqual(probe, ['probe', { pid: 4242 }]);
  const lsof = calls.find(([m, p]) => m === 'exec' && p.command === '/usr/sbin/lsof');
  assert.deepEqual(lsof, ['exec', { command: '/usr/sbin/lsof', args: ['-nP', '-a', '-p', '4242', '-d', 'txt', '-Fn'], deadlineMs: 5000 }]);
});

test('a mac teardown refusal that cannot be probed is a teardown error, never a silent clean run', async () => {
  const refused = [{ pid: 4243, role: 'gui-g1' }];
  const agent = { call: async (method, params = {}) => {
    if (method === 'exec' && params.command === 'node') return { status: 0, stdout: '[]', stderr: '' };
    if (method === 'teardownRecorded') return { killed: [], refused };
    if (method === 'probe') throw new Error('PID_GONE');
    if (method === 'removeSshHelperRoots') return { removed: [], kept: [] };
    if (method === 'table') return [];
    if (method === 'exists') return false;
    return null;
  } };
  const summary = newSummary({ runId: '20261019t064512z', platform: 'mac', host: 'h' });
  await teardownRun({
    summary, agent, jobs: { listQaJobs: async () => [], remove: async () => {} }, host: { platform: 'mac' },
    layout: { logs: '/qa/logs' }, runDirs: ['/qa/run'], qaRoots: ['/qa/run'], localEvidence: '/nonexistent-evidence', prodSnapshot: async () => null,
  });
  assert.deepEqual(summary.teardown.errors, [{ step: 'kill-recorded', error: 'PID_GONE' }]);
  assert.equal(summary.teardown.refused.length, 1);
});

test('windows teardown never probes or maps text for a refused kill', async () => {
  const gui = 'C:\\run\\artifacts\\A\\ferryx.exe';
  const table = new Map([[7716, { executable: gui, startTime: '134371448369187860' }]]);
  const sent = [];
  const reg = new ProcessRegistry({ probe: async (pid) => table.get(pid) ?? null, signal: async (pid, sig) => sent.push([pid, sig]), platform: 'windows' });
  await reg.record(7716, { role: 'gui-g9', executable: gui });
  // g9 exited; nine minutes later g28's powershell launcher got pid 7716.
  table.set(7716, { executable: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe', startTime: '134371453993520000' });
  assert.equal(await reg.check(7716), 'reused');
  assert.equal(await reg.kill(7716), false);
  // A recorded start that is unknown proves nothing: the image difference is still refused.
  const noStart = new ProcessRegistry({ probe: async (pid) => table.get(pid) ?? null, signal: async (pid, sig) => sent.push([pid, sig]), platform: 'windows' });
  table.set(7717, { executable: gui, startTime: null });
  await noStart.record(7717, { role: 'gui-g10', executable: gui });
  table.set(7717, { executable: 'C:\\Windows\\System32\\cmd.exe', startTime: '1' });
  await assert.rejects(noStart.kill(7717), /KILL_REFUSED/);
  assert.deepEqual(sent, []);
});

test('mutable macOS listener titles never authorize a kill; only the kernel image and recorded start do', async () => {
  const start = 'Thu Nov 19 12:30:39 2026';
  let title = '/usr/sbin/sshd', image = '/usr/sbin/sshd';
  const sent = [];
  const exec = async (cmd, args) => {
    assert.equal(args.at(-1), '42');
    if (cmd === '/bin/ps') return { status: 0, stdout: `${start} ${title}\n` };
    assert.equal(cmd, '/usr/bin/python3');
    return { status: 0, stdout: image };
  };
  const r = new ProcessRegistry({ platform: 'mac', probe: pid => probeIdentity(pid, 'mac', { exec }), signal: async pid => sent.push(pid) });
  await r.record(42, { role: 'sshd', executable: image });
  title = 'sshd: /usr/sbin/sshd -D -e -f /qa/sshd_config [listener]';
  image = '/tmp/other-program';
  await assert.rejects(r.kill(42), /KILL_REFUSED/);
  image = '';
  await assert.rejects(r.kill(42));
  assert.deepEqual(sent, []);
  image = '/usr/sbin/sshd';
  assert.equal(await r.kill(42), true);
  assert.deepEqual(sent, [42]);
});

test('windows probe keeps the FILETIME start exact through script, JSON and parse (> 2^53)', async () => {
  const filetime = 134048390123456789n; // owner.json u64; Number(filetime) would round to ...800
  assert.notEqual(String(Number(filetime)), String(filetime));
  // Emulates ConvertTo-Json for the script actually sent: [string] cast -> JSON string, else number.
  const calls = [];
  const exec = async (command, args) => {
    calls.push([command, args]);
    const script = args.at(-1);
    const start = /start = \[string\]\$p\.StartTime\.ToFileTimeUtc\(\)/.test(script) ? JSON.stringify(String(filetime)) : String(filetime);
    return { status: 0, stdout: `{"path":"C:\\\\qa\\\\ferryx.exe","start":${start}}\r\n`, stderr: '' };
  };
  const live = await probeIdentity(42, 'windows', { exec });
  assert.equal(calls[0][0], 'powershell.exe');
  assert.equal(calls[0][1].at(-1), windowsProbeScript(42));
  assert.deepEqual(live, { executable: 'C:\\qa\\ferryx.exe', startTime: String(filetime) });
  assert.equal(ownerStartMatches('windows', String(filetime), live.startTime), true);
  assert.equal(ownerStartMatches('windows', String(filetime + 1n), live.startTime), false);
  // A numeric start (the old producer shape) is refused, never silently rounded.
  const numeric = async () => ({ status: 0, stdout: `{"path":"C:\\\\x.exe","start":${filetime}}`, stderr: '' });
  await assert.rejects(probeIdentity(42, 'windows', { exec: numeric }), /not an exact decimal string/);
  assert.equal(await probeIdentity(42, 'windows', { exec: async () => ({ status: 0, stdout: '', stderr: '' }) }), null);
});
