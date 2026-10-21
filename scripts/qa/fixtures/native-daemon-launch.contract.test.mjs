import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { launchNativeDaemon } from './native-remote-agent.mjs';

function fixture() {
  const profile = '/worktree/.native-qa/run';
  const controller = new AbortController();
  const child = new EventEmitter();
  child.pid = 42; child.stdout = new PassThrough(); child.stderr = new PassThrough();
  child.kill = () => { throw new Error('Automatic cleanup forbidden'); };
  const published = [];
  const probes = [];
  const identity = { exited: false, executable: '/worktree/ferryx', processStart: '123:000007' };
  const setup = { worktree: '/worktree', profile, binary: identity.executable, binarySha256: 'hash', candidate: 'candidate',
    env: { FERRYX_QA_ISOLATED: '1', FERRYX_DATA_DIR: `${profile}/data`, FERRYX_RUNTIME_DIR: `${profile}/runtime`,
      FERRYX_SESSION_DIR: `${profile}/session`, FERRYX_QA_BARRIER_DIR: `${profile}/barriers`,
      FERRYX_QA_RUN_ID: 'run', FERRYX_QA_OPERATION_ID: 'op', HOME: `${profile}/home`, USERPROFILE: `${profile}/home` } };
  const spec = { setup, runId: 'run', operationId: 'op', hostId: 'host', timeoutMs: 1000, signal: controller.signal };
  const deps = { realpath: async (path) => path, hashFile: async () => 'hash',
    spawn: (binary, args, options) => {
      assert.equal(binary, setup.binary); assert.deepEqual(args, ['--daemon']);
      assert.equal(options.env.FERRYX_RUNTIME_DIR, `${profile}/runtime`); return child;
    },
    probe: async (pid) => {
      assert.equal(child.stdout.listenerCount('data'), 1);
      probes.push(pid);
      if (probes.length === 1) { child.stdout.write('noise\nFERRYX_DAEMON_'); child.stdout.write('READY\r\n'); }
      return identity;
    },
    persist: async (path, receipt) => published.push({ path, receipt }) };
  return { spec, deps, child, identity, published, probes, controller };
}

test('exact child readiness prearmed during first probe produces atomic ready receipt', async () => {
  const f = fixture();
  const receipt = await launchNativeDaemon(f.spec, f.deps);
  assert.deepEqual(f.probes, [42, 42]);
  assert.deepEqual(receipt, { runId: 'run', operationId: 'op', hostId: 'host', candidate: 'candidate',
    role: 'daemon', status: 'ready', pid: 42, executable: f.identity.executable,
    processStart: '123:000007', readiness: 'FERRYX_DAEMON_READY' });
  assert.deepEqual(f.published, [{ path: '/worktree/.native-qa/run/receipts/daemon-launch.json', receipt }]);
  assert.equal(f.child.stdout.listenerCount('data'), 0);
});

test('exit before readiness refuses without publication or cleanup', async () => {
  const f = fixture();
  f.deps.probe = async () => { f.child.emit('exit', 1, null); return f.identity; };
  await assert.rejects(launchNativeDaemon(f.spec, f.deps), /initial identity/);
  assert.deepEqual(f.published, []);
});

test('initial executable mismatch and replaced full start token refuse', async () => {
  for (const stage of ['initial', 'final']) {
    const f = fixture();
    let calls = 0;
    f.deps.probe = async () => {
      calls++;
      f.child.stdout.write('FERRYX_DAEMON_READY\n');
      return stage === 'initial' ? { ...f.identity, executable: '/foreign' }
        : { ...f.identity, processStart: calls === 1 ? f.identity.processStart : '123:000008' };
    };
    await assert.rejects(launchNativeDaemon(f.spec, f.deps), /identity/);
    assert.deepEqual(f.published, []);
  }
});

test('pending readiness cancellation rejects and removes event listener', async () => {
  const f = fixture();
  let entered;
  const initialProbe = new Promise((resolve) => { entered = resolve; });
  f.deps.probe = async () => { entered(); return f.identity; };
  const running = launchNativeDaemon(f.spec, f.deps);
  const rejected = assert.rejects(running, /cancelled/);
  await initialProbe;
  f.controller.abort(new Error('cancelled'));
  f.child.stdout.write('FERRYX_DAEMON_READY\n');
  await rejected;
  assert.deepEqual(f.published, []);
  assert.equal(f.child.stdout.listenerCount('data'), 0);
});

test('cancellation during final probe prevents late receipt publication', async () => {
  const f = fixture();
  let entered;
  let resolveFinal;
  let calls = 0;
  const finalProbe = new Promise((resolve) => { entered = resolve; });
  f.deps.probe = async () => {
    if (++calls === 1) { f.child.stdout.write('FERRYX_DAEMON_READY\n'); return f.identity; }
    entered(); return new Promise((resolve) => { resolveFinal = resolve; });
  };
  const rejected = assert.rejects(launchNativeDaemon(f.spec, f.deps), /cancelled/);
  await finalProbe;
  f.controller.abort(new Error('cancelled'));
  resolveFinal(f.identity);
  await rejected;
  assert.deepEqual(f.published, []);
});

test('publication collision is returned, never overwritten or cleaned up uncertainly', async () => {
  const f = fixture();
  f.deps.persist = async () => { throw Object.assign(new Error('receipt already exists'), { code: 'EEXIST' }); };
  await assert.rejects(launchNativeDaemon(f.spec, f.deps), { code: 'EEXIST' });
  assert.deepEqual(f.published, []);
});
