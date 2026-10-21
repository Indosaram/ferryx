import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, openSync, closeSync, readFileSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { runToFd, tailOfFile, LOG_TAIL_LINES } from './lib/proc.mjs';
import { createAgent } from './lib/agent.mjs';
import { scpSource, fetchHostFile } from './lib/remote.mjs';
import { runBuildStep, buildFailureDetail } from './run.mjs';
import { hostFor } from './lib/hosts.mjs';
import { findSshHelpers, sshHelperDigest } from './lib/sshd.mjs';

const RUN = '20261019t064512z';
const tmp = () => mkdtempSync(path.join(os.tmpdir(), 'qa-build-log-'));
// A child that prints n numbered lines to stdout and to stderr, then exits with code.
const chatty = (n, code) => [process.execPath, ['-e', `for (let i = 0; i < ${n}; i += 1) { process.stdout.write('out ' + i + String.fromCharCode(10)); process.stderr.write('err ' + i + String.fromCharCode(10)); } process.exitCode = ${code};`]];

test('runToFd writes stdout and stderr to the fd and reports the real exit code', async () => {
  const dir = tmp();
  const log = path.join(dir, 'b.log');
  const fd = openSync(log, 'w');
  const [cmd, args] = chatty(1000, 3);
  const r = await runToFd(cmd, args, { fd, deadlineMs: 60_000 });
  closeSync(fd);
  assert.equal(r.status, 3);
  assert.equal(r.signal, null);
  assert.equal(r.timedOut, false);
  assert.equal(r.stdout, undefined, 'no output is held by the runner');
  const text = readFileSync(log, 'utf8');
  assert.match(text, /^out 999$/m);
  assert.match(text, /^err 999$/m);
  rmSync(dir, { recursive: true, force: true });
});

test('runToFd reports the terminating signal of a killed child', { skip: process.platform === 'win32' }, async () => {
  const dir = tmp();
  const fd = openSync(path.join(dir, 's.log'), 'w');
  const r = await runToFd(process.execPath, ['-e', "process.kill(process.pid, 'SIGTERM')"], { fd, deadlineMs: 60_000 });
  closeSync(fd);
  assert.equal(r.status, null);
  assert.equal(r.signal, 'SIGTERM');
  rmSync(dir, { recursive: true, force: true });
});

test('runToFd kills a child past its deadline and says so', async () => {
  const dir = tmp();
  const fd = openSync(path.join(dir, 't.log'), 'w');
  const r = await runToFd(process.execPath, ['-e', 'setTimeout(() => {}, 600000)'], { fd, deadlineMs: 300 });
  closeSync(fd);
  assert.equal(r.timedOut, true);
  assert.notEqual(r.status, 0);
  rmSync(dir, { recursive: true, force: true });
});

test('runToFd resolves with an error for a missing command instead of throwing', async () => {
  const dir = tmp();
  const fd = openSync(path.join(dir, 'e.log'), 'w');
  const r = await runToFd(path.join(dir, 'no-such-command'), [], { fd, deadlineMs: 60_000 });
  closeSync(fd);
  assert.ok(r.error);
  assert.equal(r.status, null);
  rmSync(dir, { recursive: true, force: true });
});

test('tailOfFile returns the last n lines and drops a partial first line when it reads mid-file', () => {
  const dir = tmp();
  const file = path.join(dir, 'x.log');
  writeFileSync(file, Array.from({ length: 1000 }, (_, i) => `line ${i}`).join('\n') + '\n');
  const all = tailOfFile(file, 3);
  assert.equal(all.tail, 'line 997\nline 998\nline 999');
  assert.equal(all.bytes, readFileSync(file).length);
  // 20 bytes from the end start inside "line 997": that partial line is dropped.
  assert.equal(tailOfFile(file, 10, 20).tail, 'line 998\nline 999');
  writeFileSync(file, '');
  assert.equal(tailOfFile(file).tail, '');
  rmSync(dir, { recursive: true, force: true });
});

test('agent execLogged keeps the full output on the host and returns only exit, signal and a bounded tail', async () => {
  const runDir = tmp();
  const { methods } = createAgent({ platform: process.platform === 'win32' ? 'windows' : process.platform === 'darwin' ? 'mac' : 'linux', runId: RUN, runDir });
  const logFile = path.join(runDir, 'build-logs', 'build-C.log');
  const statusFile = path.join(runDir, 'build-logs', 'build-C.status.json');
  const [cmd, args] = chatty(5000, 7);
  const r = await methods.execLogged({ command: cmd, args, logFile, statusFile, deadlineMs: 60_000 });
  assert.equal(r.status, 7);
  assert.equal(r.signal, null);
  assert.equal(r.stdout, undefined);
  assert.equal(r.stderr, undefined);
  assert.ok(r.tail.split('\n').length <= LOG_TAIL_LINES);
  const full = readFileSync(logFile, 'utf8');
  assert.equal(full.split('\n').filter(Boolean).length, 10_000, 'every line of both streams is in the host log');
  assert.equal(r.bytes, Buffer.byteLength(full));
  const sidecar = JSON.parse(readFileSync(statusFile, 'utf8'));
  assert.equal(sidecar.status, 7);
  assert.equal(sidecar.signal, null);
  assert.equal(sidecar.timedOut, false);
  assert.equal(sidecar.logFile, logFile);
  // A JSON reply carrying the tail stays small whatever the build prints.
  assert.ok(JSON.stringify(r).length < 64 * 1024);
  rmSync(runDir, { recursive: true, force: true });
});

test('agent execLogged refuses log or status files outside the run dir', async () => {
  const runDir = tmp();
  const other = tmp();
  const { methods } = createAgent({ platform: 'linux', runId: RUN, runDir });
  await assert.rejects(methods.execLogged({ command: process.execPath, args: ['-e', ''], logFile: path.join(other, 'a.log'), statusFile: path.join(runDir, 's.json') }), { code: 'PATH_OUTSIDE_RUN' });
  await assert.rejects(methods.execLogged({ command: process.execPath, args: ['-e', ''], logFile: path.join(runDir, 'a.log'), statusFile: path.join(other, 's.json') }), { code: 'PATH_OUTSIDE_RUN' });
  assert.equal(existsSync(path.join(runDir, 'a.log')), false);
  rmSync(runDir, { recursive: true, force: true });
  rmSync(other, { recursive: true, force: true });
});

const fakeAgent = (reply) => {
  const calls = [];
  return { calls, call: async (method, params) => { calls.push({ method, params }); return { tail: 'last line', bytes: 9, pid: 1, timedOut: false, error: null, signal: null, ...reply, logFile: params.logFile, statusFile: params.statusFile }; } };
};
const step = { command: 'cargo', args: ['tauri', 'build', '--debug'], cwd: '/w', env: { CARGO_TARGET_DIR: '/t' } };

test('runBuildStep runs the build through execLogged into the run dir and copies log and status back as files', async () => {
  const ev = tmp();
  for (const [platform, runDir, sep] of [['linux', '/home/q/.qaruns/' + RUN, '/'], ['windows', 'C:\\Users\\sook\\ferryx-wt\\qaruns\\' + RUN, '\\']]) {
    const host = hostFor(platform);
    const agent = fakeAgent({ status: 0 });
    const fetched = [];
    const fetch = async (h, remote, local) => { fetched.push([h.platform, remote, local]); writeFileSync(local, 'copied'); return local; };
    const r = await runBuildStep({ host, agent, name: 'C', step, runDir, localEvidence: ev, fetch });
    assert.equal(agent.calls.length, 1);
    assert.equal(agent.calls[0].method, 'execLogged');
    assert.equal(agent.calls[0].params.logFile, [runDir, 'build-logs', 'build-C.log'].join(sep));
    assert.equal(agent.calls[0].params.statusFile, [runDir, 'build-logs', 'build-C.status.json'].join(sep));
    assert.equal(agent.calls[0].params.command, 'cargo');
    assert.deepEqual(fetched.map((f) => f[2]), [path.join(ev, 'build-C.log'), path.join(ev, 'build-C.status.json')]);
    assert.equal(r.ok, true);
    assert.deepEqual(r.fetchErrors, []);
  }
  rmSync(ev, { recursive: true, force: true });
});

test('runBuildStep: a signal, timeout or spawn error is a failure even when status looks clean', async () => {
  const ev = tmp();
  const fetch = async (h, remote, local) => { writeFileSync(local, 'x'); return local; };
  for (const [reply, detail] of [
    [{ status: 1 }, 'C exit=1 signal=none'],
    [{ status: null, signal: 'SIGKILL' }, 'C exit=null signal=SIGKILL'],
    [{ status: null, signal: 'SIGKILL', timedOut: true }, 'C exit=null signal=SIGKILL timedOut'],
    [{ status: null, error: 'spawn cargo ENOENT' }, 'C exit=null signal=none error=spawn cargo ENOENT'],
  ]) {
    const r = await runBuildStep({ host: hostFor('linux'), agent: fakeAgent(reply), name: 'C', step, runDir: '/r', localEvidence: ev, fetch });
    assert.equal(r.ok, false);
    assert.equal(buildFailureDetail('C', r), detail);
  }
  rmSync(ev, { recursive: true, force: true });
});

test('runBuildStep keeps the agent tail in build-<name>.log when the full copy fails', async () => {
  const ev = tmp();
  const fetch = async () => { throw new Error('scp exited 1'); };
  const r = await runBuildStep({ host: hostFor('linux'), agent: fakeAgent({ status: 0, tail: 'Finished dev profile' }), name: 'A', step, runDir: '/r', localEvidence: ev, fetch });
  assert.equal(r.fetchErrors.length, 2);
  assert.match(readFileSync(path.join(ev, 'build-A.log'), 'utf8'), /full log copy failed: scp exited 1[\s\S]*Finished dev profile/);
  const status = JSON.parse(readFileSync(path.join(ev, 'build-A.status.json'), 'utf8'));
  assert.equal(status.status, 0);
  assert.equal(status.copyFailed, true);
  assert.equal(status.tail, undefined);
  rmSync(ev, { recursive: true, force: true });
});

test('scp source: posix paths as-is, Windows paths with forward slashes (C:\\ fails on maho-win)', () => {
  assert.equal(scpSource(hostFor('linux'), '/home/indo/x/build-C.log'), 'omarchy:/home/indo/x/build-C.log');
  assert.equal(scpSource(hostFor('windows'), 'C:\\Users\\sook\\ferryx-wt\\qaruns\\r\\build-logs\\build-C.log'), 'maho-win:C:/Users/sook/ferryx-wt/qaruns/r/build-logs/build-C.log');
});

test('fetchHostFile: scp for SSH hosts, a plain copy for the local host, a failed scp throws', async () => {
  const dir = tmp();
  const calls = [];
  const exec = async (cmd, args) => { calls.push([cmd, args]); return { status: 0, stdout: '', stderr: '' }; };
  await fetchHostFile(hostFor('linux'), '/r/build-logs/build-B.log', path.join(dir, 'sub', 'build-B.log'), { exec });
  assert.equal(calls[0][0], 'scp');
  assert.deepEqual(calls[0][1].slice(-2), ['omarchy:/r/build-logs/build-B.log', path.join(dir, 'sub', 'build-B.log')]);
  assert.ok(calls[0][1].includes('BatchMode=yes'));
  await assert.rejects(fetchHostFile(hostFor('linux'), '/r/x', path.join(dir, 'y'), { exec: async () => ({ status: 1, stdout: '', stderr: 'no such file' }) }), /no such file/);
  const src = path.join(dir, 'src.log');
  writeFileSync(src, 'local bytes');
  await fetchHostFile(hostFor('mac'), src, path.join(dir, 'dst.log'));
  assert.equal(readFileSync(path.join(dir, 'dst.log'), 'utf8'), 'local bytes');
  rmSync(dir, { recursive: true, force: true });
});

test('ssh helper match: exact executable, root and host id of this run only (posix and Windows)', () => {
  const host = 'qa-sshd-20261021t123034z';
  assert.equal(sshHelperDigest(host), '5dd1ed31c397244416c027ffe07264fd'); // measured on omarchy, pid 470221
  const exe = '/home/indo/.ferryx/versions/2026.1017.1/bin/ferryx-remote-helper';
  const root = '/home/indo/.ferryx/r/2026.1017.1/5dd1ed31c397244416c027ffe07264fd';
  const row = (pid, commandLine, executable = exe) => ({ pid, executable, commandLine });
  const rows = [
    row(470221, `${exe} daemon --root ${root} --host-id ${host}`),
    row(2, `${exe} daemon --root /home/indo/.ferryx/r/2026.1017.1/e81eaed965c4a3e95680bfbfff3af0c7 --host-id omarchy`),
    row(3, `${exe} daemon --root ${root} --host-id qa-sshd-20261021t000000z`),
    row(4, `${exe} daemon --root /home/indo/.ferryx/r/2026.1017.1/${'0'.repeat(32)} --host-id ${host}`),
    row(5, `/tmp/fake daemon --root ${root} --host-id ${host}`, '/tmp/fake'),
    row(6, `${exe} daemon --root /home/indo/.ferryx/r/../r/2026.1017.1/5dd1ed31c397244416c027ffe07264fd --host-id ${host}`),
  ];
  assert.deepEqual(findSshHelpers(rows, { platform: 'linux', home: '/home/indo/', hostId: host }), [{ pid: 470221, executable: exe, root }]);
  const wexe = 'C:\\Users\\sook\\.ferryx\\versions\\2026.1017.1\\bin\\ferryx-remote-helper.exe';
  const wroot = 'C:\\Users\\sook\\.ferryx\\r\\2026.1017.1\\5dd1ed31c397244416c027ffe07264fd';
  const wrow = { pid: 9, executable: wexe.toLowerCase(), commandLine: `"${wexe}" "daemon" "--root" "${wroot}" "--host-id" "${host}"` };
  assert.deepEqual(findSshHelpers([wrow], { platform: 'windows', home: 'C:\\Users\\sook', hostId: host }), [{ pid: 9, executable: wexe.toLowerCase(), root: wroot }]);
});

test('ssh helper teardown: recorded by exact identity, killed by recorded pid, root removed only after it exited', async () => {
  const runDir = tmp();
  const home = tmp();
  const host = `qa-sshd-${RUN}`;
  const tag = '2026.1017.1';
  const exe = path.join(home, '.ferryx', 'versions', tag, 'bin', 'ferryx-remote-helper');
  const root = path.join(home, '.ferryx', 'r', tag, sshHelperDigest(host));
  const other = path.join(home, '.ferryx', 'r', tag, 'e81eaed965c4a3e95680bfbfff3af0c7');
  mkdirSync(root, { recursive: true });
  mkdirSync(other, { recursive: true });
  writeFileSync(path.join(root, 'endpoint.json'), '{}');
  const live = new Map([[41, exe], [42, exe]]);
  const signals = [];
  let exitCb = null;
  const agent = createAgent({
    platform: 'linux', runId: RUN, runDir, home,
    table: async () => [
      { pid: 41, executable: exe, commandLine: `${exe} daemon --root ${root} --host-id ${host}` },
      { pid: 42, executable: exe, commandLine: `${exe} daemon --root ${other} --host-id omarchy` },
    ],
    probe: async (pid) => (live.has(pid) ? { executable: live.get(pid), startTime: '7' } : null),
    signal: async (pid, sig) => { signals.push([pid, sig]); live.delete(pid); exitCb?.(); },
  });
  await assert.rejects(agent.methods.recordSshHelper({ hostId: 'qa-sshd-20261021t000000z' }), /SSH_HELPER_NOT_OURS/);
  assert.deepEqual((await agent.methods.recordSshHelper({ hostId: host })).map((h) => h.pid), [41]);
  assert.deepEqual((await agent.methods.recordSshHelper({ hostId: host })).map((h) => h.pid), [41], 'recording twice is idempotent');
  assert.deepEqual(agent.methods.recorded ? (await agent.methods.recorded()).map((e) => [e.pid, e.role]) : [], [[41, 'ssh-helper']]);
  const t = await agent.methods.teardownRecorded();
  assert.deepEqual(t.killed.map((e) => e.pid), [41]);
  assert.deepEqual(signals, [[41, 'SIGTERM']], 'the foreign helper 42 is never signalled');
  const r = await agent.methods.removeSshHelperRoots({ deadlineMs: 1000 });
  assert.deepEqual(r, { removed: [{ root, pid: 41 }], kept: [] });
  assert.equal(existsSync(root), false);
  assert.equal(existsSync(other), true, 'a foreign helper root stays');
  // A recorded helper that is still running keeps its root and is reported as kept.
  const runDir2 = tmp();
  const live2 = new Map([[51, exe]]);
  mkdirSync(root, { recursive: true });
  const stuck = createAgent({
    platform: 'linux', runId: RUN, runDir: runDir2, home,
    table: async () => [{ pid: 51, executable: exe, commandLine: `${exe} daemon --root ${root} --host-id ${host}` }],
    probe: async (pid) => (live2.has(pid) ? { executable: exe, startTime: '8' } : null),
    signal: async () => {},
  });
  await stuck.methods.recordSshHelper({ hostId: host });
  const kept = await stuck.methods.removeSshHelperRoots({ deadlineMs: 200 });
  assert.deepEqual(kept.removed, []);
  assert.equal(kept.kept[0].reason, 'helper pid still running');
  assert.equal(existsSync(root), true);
  for (const d of [runDir, runDir2, home]) rmSync(d, { recursive: true, force: true });
});


