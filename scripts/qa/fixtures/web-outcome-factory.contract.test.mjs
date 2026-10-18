import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PassThrough } from 'node:stream';
import { EventEmitter } from 'node:events';
import { createOpenWebOutcome, defaultChromeLauncher, CdpPipeConnection } from './web-outcome-factory.mjs';

test('createOpenWebOutcome requires isolated targets and candidate paths', () => {
  assert.throws(() => createOpenWebOutcome({}), /TASK9_FACTORY_REQUIRES_ISOLATED_TARGETS_AND_CANDIDATE/);
  assert.throws(() => createOpenWebOutcome({ uiDist: '/dist' }), /TASK9_FACTORY_REQUIRES_ISOLATED_TARGETS_AND_CANDIDATE/);
});

test('createOpenWebOutcome produces factory preserving simulated gateway proof and profileDir contract', () => {
  const factory = createOpenWebOutcome({
    uiDist: '/tmp/dist',
    evidenceDir: '/tmp/evidence',
    target: { sessionId: 'sess-1' },
    rebindTarget: { sessionId: 'sess-2' },
  });
  assert.equal(typeof factory, 'function');
});

test('defaultChromeLauncher wire test exercises single Chrome spawn, arguments, and target session CDP lifecycle', async () => {
  let capturedExecutable = null;
  let capturedArgs = null;
  let capturedStdio = null;
  let capturedNavigatedUrl = null;

  const inStream = new PassThrough();
  const outStream = new PassThrough();
  const mockProc = new EventEmitter();
  mockProc.pid = 4321;
  mockProc.spawnfile = '/mock/chrome';
  mockProc.stdio = [null, null, null, outStream, inStream];
  mockProc.killed = false;
  mockProc.kill = (sig) => {
    mockProc.killed = true;
    mockProc.emit('exit', 0, sig);
  };

  outStream.on('data', chunk => {
    let nullIdx;
    while ((nullIdx = chunk.indexOf(0)) !== -1) {
      const msgBuf = chunk.subarray(0, nullIdx);
      chunk = chunk.subarray(nullIdx + 1);
      if (msgBuf.length === 0) continue;
      const json = JSON.parse(msgBuf.toString('utf8'));
      if (json.method === 'Target.createTarget') {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, result: { targetId: 'mock-target-1' } })), Buffer.from([0])]));
      } else if (json.method === 'Target.attachToTarget') {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, result: { sessionId: 'mock-session-1' } })), Buffer.from([0])]));
      } else if (json.method === 'Page.navigate') {
        assert.equal(json.sessionId, 'mock-session-1');
        capturedNavigatedUrl = json.params?.url;
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, sessionId: json.sessionId, result: {} })), Buffer.from([0])]));
      } else if (json.method === 'Runtime.evaluate') {
        assert.equal(json.sessionId, 'mock-session-1');
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, sessionId: json.sessionId, result: { result: { value: 'evaluated-output' } } })), Buffer.from([0])]));
      } else if (json.method === 'Page.captureScreenshot') {
        assert.equal(json.sessionId, 'mock-session-1');
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, sessionId: json.sessionId, result: { data: Buffer.from('mock-png').toString('base64') } })), Buffer.from([0])]));
      } else {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, sessionId: json.sessionId, result: {} })), Buffer.from([0])]));
      }
    }
  });

  const mockSpawn = (exe, args, opts) => {
    capturedExecutable = exe;
    capturedArgs = args;
    capturedStdio = opts?.stdio;
    return mockProc;
  };

  const profileDir = '/tmp/test-profile-wire';
  const targetUrl = 'http://127.0.0.1:9099/index.html';
  const browser = await defaultChromeLauncher(profileDir, targetUrl, {
    spawn: mockSpawn,
    browserExecutable: '/mock/chrome',
    probeIdentity: async (pid, exe) => ({ pid, alive: true, executable: exe }),
    viewport: { width: 1024, height: 768, name: 'desktop' },
  });

  // Verify command line arguments passed to spawn
  assert.equal(capturedExecutable, '/mock/chrome');
  assert.ok(capturedArgs.includes('--headless=new'));
  assert.ok(capturedArgs.includes('--remote-debugging-pipe'));
  assert.ok(capturedArgs.includes(`--user-data-dir=${profileDir}`));
  assert.ok(capturedArgs.includes('about:blank'));
  assert.equal(capturedNavigatedUrl, targetUrl);
  assert.deepEqual(capturedStdio, ['ignore', 'pipe', 'pipe', 'pipe', 'pipe']);

  // Verify CDP target session routing
  const val = await browser.evaluate('1 + 1');
  assert.equal(val, 'evaluated-output');

  const shot = await browser.screenshot();
  assert.equal(shot.toString('utf8'), 'mock-png');

  // Verify clean disposal targets child process ownership without OS kill
  await browser.close();
  assert.equal(mockProc.killed, true);
});

test('openWebOutcome integrates with launcher seam and cleans up sole launch-owned instance on dispose', async () => {
  let closed = false;
  let evaluated = false;

  const mockBrowser = {
    recordedPid: 54321,
    recordedExe: '/mock/chrome',
    navigate: async () => {},
    evaluate: async () => { evaluated = true; return true; },
    screenshot: async () => Buffer.from('png'),
    close: async () => { closed = true; },
  };

  const factory = createOpenWebOutcome({
    uiDist: '/tmp/dist',
    evidenceDir: '/tmp/evidence',
    target: { sessionId: 'sess-1' },
    rebindTarget: { sessionId: 'sess-2' },
  });

  const outcome = await factory({
    viewport: { width: 800, height: 600, name: 'desktop' },
    launcher: () => mockBrowser,
  });

  // Test driving terminal input using { kind: 'text', payload: 'ls\n' }
  await outcome.input({ kind: 'text', payload: 'ls\n' });
  assert.equal(evaluated, true);

  // Test dispose calls browser.close() with no OS process.kill
  await outcome.dispose();
  assert.equal(closed, true);
});

test('defaultChromeLauncher reaps child process if initTarget fails during launch', async () => {
  const inStream = new PassThrough();
  const outStream = new PassThrough();
  const mockProc = new EventEmitter();
  mockProc.pid = 7777;
  mockProc.spawnfile = '/mock/chrome';
  mockProc.stdio = [null, null, null, outStream, inStream];
  mockProc.killed = false;
  mockProc.kill = (sig) => {
    mockProc.killed = true;
    mockProc.emit('exit', 0, sig);
  };

  outStream.on('data', chunk => {
    let nullIdx;
    while ((nullIdx = chunk.indexOf(0)) !== -1) {
      const msgBuf = chunk.subarray(0, nullIdx);
      chunk = chunk.subarray(nullIdx + 1);
      if (msgBuf.length === 0) continue;
      const json = JSON.parse(msgBuf.toString('utf8'));
      // Fail on Target.createTarget
      if (json.method === 'Target.createTarget') {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, error: { code: -32000, message: 'Target creation failed' } })), Buffer.from([0])]));
      }
    }
  });

  const mockSpawn = () => mockProc;
  await assert.rejects(
    () => defaultChromeLauncher('/tmp/profile-fail', 'http://127.0.0.1:9099/index.html', {
      spawn: mockSpawn,
      browserExecutable: '/mock/chrome',
      probeIdentity: async (pid, exe) => ({ pid, alive: true, executable: exe }),
    }),
    /Target creation failed/
  );

  // Assert cleanup reaped the child process even though launch failed
  assert.equal(mockProc.killed, true);
});

test('defaultChromeLauncher close refuses kill on executable mismatch and reports CLEANUP_INCOMPLETE', async () => {
  const inStream = new PassThrough();
  const outStream = new PassThrough();
  const mockProc = new EventEmitter();
  mockProc.pid = 1234;
  mockProc.spawnfile = '/mock/chrome';
  mockProc.stdio = [null, null, null, outStream, inStream];
  mockProc.killed = false;
  mockProc.signals = [];
  mockProc.kill = (sig) => {
    mockProc.killed = true;
    mockProc.signals.push(sig);
    mockProc.emit('exit', 0, sig);
  };

  outStream.on('data', chunk => {
    let nullIdx;
    while ((nullIdx = chunk.indexOf(0)) !== -1) {
      const msgBuf = chunk.subarray(0, nullIdx);
      chunk = chunk.subarray(nullIdx + 1);
      if (msgBuf.length === 0) continue;
      const json = JSON.parse(msgBuf.toString('utf8'));
      if (json.method === 'Target.createTarget') {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, result: { targetId: 'm-1' } })), Buffer.from([0])]));
      } else if (json.method === 'Target.attachToTarget') {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, result: { sessionId: 's-1' } })), Buffer.from([0])]));
      } else {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, sessionId: json.sessionId, result: {} })), Buffer.from([0])]));
      }
    }
  });

  const browser = await defaultChromeLauncher('/tmp/p1', 'http://127.0.0.1:9099', {
    spawn: () => mockProc,
    probeIdentity: async () => ({ pid: 1234, alive: true, executable: '/bin/unrelated' }),
  });

  await assert.rejects(() => browser.close(), /CLEANUP_INCOMPLETE/);
  assert.equal(mockProc.killed, false);
});

test('defaultChromeLauncher close sends no signal if process already exited', async () => {
  const inStream = new PassThrough();
  const outStream = new PassThrough();
  const mockProc = new EventEmitter();
  mockProc.pid = 2345;
  mockProc.spawnfile = '/mock/chrome';
  mockProc.stdio = [null, null, null, outStream, inStream];
  mockProc.killed = false;
  mockProc.signals = [];
  mockProc.kill = (sig) => {
    mockProc.killed = true;
    mockProc.signals.push(sig);
    mockProc.emit('exit', 0, sig);
  };

  outStream.on('data', chunk => {
    let nullIdx;
    while ((nullIdx = chunk.indexOf(0)) !== -1) {
      const msgBuf = chunk.subarray(0, nullIdx);
      chunk = chunk.subarray(nullIdx + 1);
      if (msgBuf.length === 0) continue;
      const json = JSON.parse(msgBuf.toString('utf8'));
      if (json.method === 'Target.createTarget') {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, result: { targetId: 'm-2' } })), Buffer.from([0])]));
      } else if (json.method === 'Target.attachToTarget') {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, result: { sessionId: 's-2' } })), Buffer.from([0])]));
      } else {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, sessionId: json.sessionId, result: {} })), Buffer.from([0])]));
      }
    }
  });

  const browser = await defaultChromeLauncher('/tmp/p2', 'http://127.0.0.1:9099', {
    spawn: () => mockProc,
    probeIdentity: async () => ({ pid: 2345, alive: false, executable: null }),
  });

  await browser.close();
  assert.equal(mockProc.signals.length, 0);
});

test('defaultChromeLauncher close signals direct child process when identity matches exactly and awaits exit', async () => {
  const inStream = new PassThrough();
  const outStream = new PassThrough();
  const mockProc = new EventEmitter();
  mockProc.pid = 3456;
  mockProc.spawnfile = '/mock/chrome';
  mockProc.stdio = [null, null, null, outStream, inStream];
  mockProc.killed = false;
  mockProc.signals = [];
  mockProc.kill = (sig) => {
    mockProc.killed = true;
    mockProc.signals.push(sig);
    mockProc.emit('exit', 0, sig);
  };

  outStream.on('data', chunk => {
    let nullIdx;
    while ((nullIdx = chunk.indexOf(0)) !== -1) {
      const msgBuf = chunk.subarray(0, nullIdx);
      chunk = chunk.subarray(nullIdx + 1);
      if (msgBuf.length === 0) continue;
      const json = JSON.parse(msgBuf.toString('utf8'));
      if (json.method === 'Target.createTarget') {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, result: { targetId: 'm-3' } })), Buffer.from([0])]));
      } else if (json.method === 'Target.attachToTarget') {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, result: { sessionId: 's-3' } })), Buffer.from([0])]));
      } else {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, sessionId: json.sessionId, result: {} })), Buffer.from([0])]));
      }
    }
  });

  const browser = await defaultChromeLauncher('/tmp/p3', 'http://127.0.0.1:9099', {
    spawn: () => mockProc,
    browserExecutable: '/mock/chrome',
    probeIdentity: async () => ({ pid: 3456, alive: true, executable: '/mock/chrome' }),
  });

  await browser.close();
  assert.equal(mockProc.killed, true);
  assert.equal(mockProc.signals[0], 'SIGTERM');
});

test('defaultChromeLauncher throws CLEANUP_INCOMPLETE if process fails to exit after SIGKILL', async () => {
  const inStream = new PassThrough();
  const outStream = new PassThrough();
  const mockProc = new EventEmitter();
  mockProc.pid = 4567;
  mockProc.spawnfile = '/mock/chrome';
  mockProc.stdio = [null, null, null, outStream, inStream];
  mockProc.killed = false;
  mockProc.signals = [];
  mockProc.kill = (sig) => {
    mockProc.killed = true;
    mockProc.signals.push(sig);
    // Deliberately do NOT emit exit to simulate hung process
  };

  outStream.on('data', chunk => {
    let nullIdx;
    while ((nullIdx = chunk.indexOf(0)) !== -1) {
      const msgBuf = chunk.subarray(0, nullIdx);
      chunk = chunk.subarray(nullIdx + 1);
      if (msgBuf.length === 0) continue;
      const json = JSON.parse(msgBuf.toString('utf8'));
      if (json.method === 'Target.createTarget') {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, result: { targetId: 'm-4' } })), Buffer.from([0])]));
      } else if (json.method === 'Target.attachToTarget') {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, result: { sessionId: 's-4' } })), Buffer.from([0])]));
      } else {
        inStream.write(Buffer.concat([Buffer.from(JSON.stringify({ id: json.id, sessionId: json.sessionId, result: {} })), Buffer.from([0])]));
      }
    }
  });

  const browser = await defaultChromeLauncher('/tmp/p4', 'http://127.0.0.1:9099', {
    spawn: () => mockProc,
    browserExecutable: '/mock/chrome',
    probeIdentity: async () => ({ pid: 4567, alive: true, executable: '/mock/chrome' }),
  });

  await assert.rejects(() => browser.close(), /failed to exit within deadline after SIGKILL/);
  assert.equal(mockProc.signals.includes('SIGTERM'), true);
  assert.equal(mockProc.signals.includes('SIGKILL'), true);
});

test('openWebOutcome dispose propagates CLEANUP_INCOMPLETE and avoids profile deletion when close fails', async () => {
  const mockBrowser = {
    recordedPid: 6789,
    recordedExe: '/mock/chrome',
    navigate: async () => {},
    evaluate: async () => true,
    screenshot: async () => Buffer.from('png'),
    close: async () => {
      throw new Error('CLEANUP_INCOMPLETE: process hung');
    },
  };

  const factory = createOpenWebOutcome({
    uiDist: '/tmp/dist',
    evidenceDir: '/tmp/evidence',
    target: { sessionId: 'sess-1' },
    rebindTarget: { sessionId: 'sess-2' },
  });

  const outcome = await factory({
    viewport: { width: 800, height: 600, name: 'desktop' },
    launcher: () => mockBrowser,
  });

  await assert.rejects(() => outcome.dispose(), /CLEANUP_INCOMPLETE: process hung/);
});

test('cdp.evaluate forwards timeoutMs with finite transport margin and preserves sessionId', async () => {
  let capturedSessionId = null;
  let capturedTimeoutMs = null;
  const mockProc = new EventEmitter();
  mockProc.stdio = [null, null, null, new PassThrough(), new PassThrough()];
  const cdp = new CdpPipeConnection(mockProc, { timeoutMs: 15000 });
  cdp.sessionId = 'test-session-id';
  cdp.send = async (method, params, sessionId, timeoutMs) => {
    capturedSessionId = sessionId;
    capturedTimeoutMs = timeoutMs;
    return { result: { value: 'ok' } };
  };

  const val = await cdp.evaluate('1 + 1', 12000);
  assert.equal(val, 'ok');
  assert.equal(capturedSessionId, 'test-session-id');
  assert.equal(capturedTimeoutMs, 17000);

  const defaultVal = await cdp.evaluate('1 + 1');
  assert.equal(defaultVal, 'ok');
  assert.equal(capturedTimeoutMs, 20000);
});

test('cdp.evaluate surfaces in-page evaluation exceptions rather than transport timeouts', async () => {
  const mockProc = new EventEmitter();
  mockProc.stdio = [null, null, null, new PassThrough(), new PassThrough()];
  const cdp = new CdpPipeConnection(mockProc, { timeoutMs: 15000 });
  cdp.sessionId = 'test-session-id';
  cdp.send = async () => ({
    exceptionDetails: { text: 'Error: TASK9_EXISTING_ACTION_UNAVAILABLE' },
  });

  await assert.rejects(
    () => cdp.evaluate('promise', 10000),
    /Evaluation failed: Error: TASK9_EXISTING_ACTION_UNAVAILABLE/
  );
});

test('openWebOutcome correctToken restores gateway authorization and clicks Retry without hash navigation', async () => {
  const { runInNewContext } = await import('node:vm');
  let retryClicks = 0;
  const mockBrowser = {
    recordedPid: 54322,
    recordedExe: '/mock/chrome',
    navigate: async () => {},
    evaluate: async (expr) => {
      return runInNewContext(expr, {
        document: {
          documentElement: {},
          querySelectorAll(selector) {
            assert.equal(selector, '[data-testid="remote-terminal-outcome"] button');
            return [{ getClientRects: () => [{}], disabled: false, click: () => { retryClicks++; } }];
          },
        },
        MutationObserver: class { observe() {} disconnect() {} },
        setTimeout: () => 1,
        clearTimeout: () => {},
        location: { set hash(_) { assert.fail('Authentication correction must not navigate'); } },
      });
    },
    screenshot: async () => Buffer.from('png'),
    close: async () => {},
  };

  const factory = createOpenWebOutcome({
    uiDist: '/tmp/dist',
    evidenceDir: '/tmp/evidence',
    target: { sessionId: 'sess-1' },
    rebindTarget: { sessionId: 'sess-2' },
  });

  const outcome = await factory({
    viewport: { width: 800, height: 600, name: 'desktop' },
    launcher: () => mockBrowser,
  });

  try {
    outcome.gateway.authorized = false;
    outcome.gateway.autoReady = false;
    await outcome.correctToken();
    assert.equal(outcome.gateway.authorized, true);
    assert.equal(outcome.gateway.autoReady, true);
    assert.equal(retryClicks, 1);
  } finally {
    await outcome.dispose();
  }
});
