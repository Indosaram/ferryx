import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { describe, expect, it, vi } from 'vitest';
import { joinFixtureChild, joinOwnedPty, capturePresentationFailure, admitOwnedPty, prearmPresentationBinding, connectNativeMahord, createConfiguredNativeContext, joinNativeAuditOwners, nativeProducerEvent, requestNativeSnapshot, rpcProcess, setupNativeTerminal, splitSecondPane, traceNativeLaunchPhase, withJoinedTargetIdentity } from './native-configured-adapters.mjs';
import { createLocalPtyOwnerRpc } from './native-remote-agent.mjs';

it.each(['success', 'screenshot-failure', 'snapshot-missing'])('captures presentation failure under a separate signal: %s', async mode => {
  const diagnostic = new AbortController();
  const entered = Promise.withResolvers();
  const screenshot = Promise.withResolvers();
  const artifact = vi.fn(async (_name, bytes) => {
    expect(JSON.parse(bytes.toString())).toEqual(_name.endsWith('child-windows.json')
      ? { windows: [] } : { producerComponent: 'native-frontend-snapshot' });
    return '/evidence/snapshot.json';
  });
  const publish = vi.fn();
  const pending = capturePresentationFailure({ profile: '/owned', makeSignal: () => diagnostic.signal,
    capture: async (phase, signal) => {
      expect(phase).toBe('setup-presentation-failed');
      expect(signal).toBe(diagnostic.signal);
      expect(signal.aborted).toBe(false);
      entered.resolve();
      await screenshot.promise;
      if (mode === 'screenshot-failure') throw new Error('capture failed');
    },
    agent: { call: async (method, args, signal) => {
      if (method === 'setup-child-windows') {
        expect(args).toEqual({ profile: '/owned', timeoutMs: 5_000 });
        return { windows: [] };
      }
      expect(method).toBe('snapshot'); expect(args).toEqual({ profile: '/owned' });
      expect(signal).toBe(diagnostic.signal);
      if (mode === 'snapshot-missing') throw new Error('producer absent');
      return { producerComponent: 'native-frontend-snapshot' };
    } }, artifact, publish });
  await entered.promise;
  expect(artifact).not.toHaveBeenCalled();
  screenshot.resolve();
  await pending;
  expect(artifact).toHaveBeenCalledTimes(mode === 'snapshot-missing' ? 1 : 2);
  expect(publish.mock.calls.filter(([event]) => event.kind === 'native-presentation-diagnostic-failed')).toHaveLength(mode === 'success' ? 0 : 1);
});

it.each(['success', 'rejection'])('inspects child windows after the earlier diagnostic signal expires: %s', async mode => {
  const earlier = new AbortController();
  const windows = new AbortController();
  const failure = Object.assign(new Error('window probe refused'), { code: 'NATIVE_SETUP_UNIMPLEMENTED' });
  const makeSignal = vi.fn().mockReturnValueOnce(earlier.signal).mockReturnValueOnce(windows.signal);
  const artifact = vi.fn(async () => '/evidence/windows.json');
  const publish = vi.fn();
  const result = { targetPid: 81, windows: [{ hwnd: 19 }] };
  await expect(capturePresentationFailure({ profile: '/owned', makeSignal, artifact, publish,
    capture: async () => { earlier.abort(new Error('capture budget expired')); throw earlier.signal.reason; },
    agent: { call: async (method, args, signal) => {
      if (method === 'snapshot') { expect(signal).toBe(earlier.signal); signal.throwIfAborted(); }
      expect(method).toBe('setup-child-windows');
      expect(args).toEqual({ profile: '/owned', timeoutMs: 5_000 });
      expect(signal).toBe(windows.signal);
      expect(signal.aborted).toBe(false);
      if (mode === 'rejection') throw failure;
      return result;
    } },
  })).resolves.toBeUndefined();
  expect(makeSignal).toHaveBeenCalledTimes(2);
  if (mode === 'success') {
    expect(artifact).toHaveBeenCalledWith('setup-presentation-failed-child-windows.json', Buffer.from(JSON.stringify(result)));
    expect(publish).toHaveBeenCalledWith({ kind: 'native-launch-artifact', phase: 'setup-presentation-failed-child-windows', path: '/evidence/windows.json' });
  } else {
    expect(artifact).not.toHaveBeenCalled();
    expect(publish).toHaveBeenCalledWith({ kind: 'native-presentation-diagnostic-failed', diagnostic: 'child-windows', code: failure.code });
  }
});

const cleanupDaemon = { role: 'daemon', status: 'ready', readiness: 'FERRYX_DAEMON_READY',
  pid: 77, executable: '/isolated/ferryx', processStart: '77:1',
  runId: 'run', operationId: 'op', candidate: 'candidate', hostId: 'host' };
const cleanupPty = { runId: 'run', operationId: 'op', producerPid: 77,
  producerComponent: 'terminal-qa-owned-pty-creation', role: 'pty', ownershipComplete: true,
  pid: 88, executable: '/isolated/shell', processStart: '88:1', backendSessionId: 'backend', incarnation: 'life' };

it.each(['foreign-run', 'foreign-operation', 'foreign-producer', 'stale', 'wrong-path', 'incomplete', 'reused-pid', 'wrong-executable'])(
  'rejects unsafe cleanup ownership: %s', async mode => {
    const raw = { ...cleanupPty,
      ...(mode === 'foreign-run' ? { runId: 'other' } : {}),
      ...(mode === 'foreign-operation' ? { operationId: 'other' } : {}),
      ...(mode === 'foreign-producer' ? { producerPid: 999 } : {}),
      ...(mode === 'incomplete' ? { ownershipComplete: false } : {}) };
    const recordOwnedPid = vi.fn();
    const probe = vi.fn(async () => ({ ...cleanupPty, exited: false,
      processStart: mode === 'reused-pid' ? '88:2' : cleanupPty.processStart,
      executable: mode === 'wrong-executable' ? '/foreign/shell' : cleanupPty.executable }));
    await expect(admitOwnedPty({ record: { raw, path: mode === 'wrong-path' ? '/foreign' : '/owned',
      lineIndex: mode === 'stale' ? 2 : 3 }, cursor: { path: '/owned', afterLine: 2 },
      runId: 'run', operationId: 'op', candidate: 'candidate', hostId: 'host', daemon: cleanupDaemon,
      probe, recordOwnedPid, signal: new AbortController().signal })).rejects.toThrow();
    expect(recordOwnedPid).not.toHaveBeenCalled();
    expect(probe).toHaveBeenCalledTimes(['reused-pid', 'wrong-executable'].includes(mode) ? 1 : 0);
  },
);

it.each(['before-receipt', 'during-probe', 'reused-pid'])('retains PTY cleanup ownership when presentation cancels %s', async mode => {
  const scenario = new AbortController();
  const cleanup = new AbortController();
  const probing = Promise.withResolvers();
  const admissionProbe = Promise.withResolvers();
  let agent;
  let probeId;
  const reply = (id, result) => agent.stdout.write(`${JSON.stringify({ id, result })}\n`);
  const spawnProcess = (_exe, argv) => {
    const child = new EventEmitter();
    child.stdout = new PassThrough(); child.stderr = new PassThrough();
    child.stdin = new EventEmitter(); child.stdin.end = vi.fn();
    const mcp = argv.includes('--mcp');
    if (!mcp) agent = child;
    child.stdin.write = (line, callback) => {
      const packet = JSON.parse(line); callback?.();
      if (mcp) { child.stdout.write(`${JSON.stringify({ id: packet.id, result: { tools: [] } })}\n`); return true; }
      if (packet.method === 'probe' && packet.args.pid === cleanupPty.pid) {
        probeId = packet.id; probing.resolve();
        void admissionProbe.promise.then(() => reply(probeId, { ...cleanupPty, exited: false,
          processStart: mode === 'reused-pid' ? 'reused' : cleanupPty.processStart }));
        return true;
      }
      if (packet.method === 'ownership-drain') child.stdout.write(`${JSON.stringify({ event: 'receipt',
        name: 'owned-pty-creation.receipt.jsonl', raw: cleanupPty, path: '/owned', lineIndex: 3 })}\n`);
      reply(packet.id, packet.method === 'prepare' ? { profile: '/isolated/profile', binary: cleanupDaemon.executable, env: {} }
        : packet.method === 'ownership-subscribe' ? { path: '/owned', afterLine: 2 }
        : packet.method === 'spawn-daemon' ? cleanupDaemon
        : packet.method === 'probe' ? { ...cleanupDaemon, exited: false }
        : packet.method === 'start-forwarder-proxy' ? { endpoint: { path: '/forwarder' } } : {});
      return true;
    };
    return child;
  };
  const release = vi.fn(async () => {});
  const owned = [];
  const ctx = await createConfiguredNativeContext({ sshDestination: 'isolated', remoteAgent: '/agent',
    worktree: '/isolated/worktree', canonicalClone: '/isolated/clone', worktreeRoot: '/isolated', bun: '/bun', binary: cleanupDaemon.executable,
    buildManifest: '/manifest', hostId: 'host', mahordExecutable: 'mahord', mahordAddress: 'isolated', pairingId: 'pair' },
  { candidate: { stamp: 'candidate' }, transport: { subscribe: () => () => {}, bindContext: vi.fn(), release },
    evidence: { artifact: vi.fn() }, runId: 'run', mutation: { id: 'op' }, signal: scenario.signal, spawnProcess });
  try {
    await ctx.spawn.prepare({ recordOwnedPid: owner => owned.push(owner), signal: scenario.signal });
    const binding = prearmPresentationBinding({ cursor: { path: '/presentation', afterLine: -1 },
      runId: 'run', operationId: 'op', subscribe: ctx.provider.subscribe, signal: scenario.signal });
    const cancelled = expect(binding.promise).rejects.toThrow('presentation cancelled');
    if (mode === 'before-receipt') scenario.abort(new Error('presentation cancelled'));
    const releasing = ctx.transport.release({ signal: cleanup.signal });
    const released = mode === 'reused-pid' ? expect(releasing).rejects.toThrow('exact PID') : releasing;
    await probing.promise;
    if (mode !== 'before-receipt') scenario.abort(new Error('presentation cancelled'));
    await cancelled;
    expect(owned.map(owner => owner.pid)).toEqual([77]);
    expect(release).not.toHaveBeenCalled();
    admissionProbe.resolve();
    await released;
    expect(owned.map(owner => owner.pid)).toEqual(mode === 'reused-pid' ? [77] : [77, 88]);
    if (mode !== 'reused-pid') expect(owned[1]).toMatchObject({ ...cleanupPty, ownershipPath: '/owned', ownershipLineIndex: 3 });
    expect(release).toHaveBeenCalledOnce();
  } finally { ctx.closeConnections(); }
});

it.each(['admit', 'join', 'child'])('normalizes Windows executable identity for %s without losing ownership scope', async kind => {
  const executable = 'C:\\Windows\\System32\\cmd.exe';
  const live = { exited: false, executable: 'c:/windows/system32/CMD.EXE', processStart: '88:1', ppid: 77 };
  const recorded = vi.fn();
  const tuple = { backendSessionId: 'backend', incarnation: 'life', daemonEpoch: 'epoch',
    frontendSessionId: 'frontend', paneIdentity: 'pane', bindingKey: 'binding', attemptGeneration: 1 };
  const spec = { record: { raw: { ...cleanupPty, executable }, path: '/owned', lineIndex: 3 },
    cursor: { path: '/owned', afterLine: 2 }, runId: 'run', operationId: 'op', candidate: 'candidate', hostId: 'host',
    daemon: cleanupDaemon, platform: 'win32', probe: async () => live, recordOwnedPid: recorded,
    gui: { ...cleanupDaemon, pid: 99, role: 'gui' }, binding: { runId: 'run', operationId: 'op', producerPid: 99, attachTuple: tuple } };
  if (kind === 'admit') await admitOwnedPty(spec);
  else if (kind === 'join') await joinOwnedPty(spec);
  else await joinFixtureChild({ target: { pid: 77, processStart: '77:1', hostId: 'host', candidate: 'candidate' },
    fixtureChild: { pid: 88, runId: 'run', operationId: 'op' }, platform: 'win32', expectedExecutable: executable,
    expectedRunId: 'run', expectedOperationId: 'op', probe: async () => live, recordOwnedPid: recorded });
  expect(recorded).toHaveBeenCalledWith(expect.objectContaining({ pid: 88, hostId: 'host', candidate: 'candidate' }));
  live.executable = 'C:/foreign/cmd.exe';
  if (kind === 'admit') await expect(admitOwnedPty(spec)).rejects.toThrow('exact PID');
  else if (kind === 'join') await expect(joinOwnedPty(spec)).rejects.toThrow('exact PID');
  else await expect(joinFixtureChild({ target: { pid: 77, processStart: '77:1', hostId: 'host', candidate: 'candidate' },
    fixtureChild: { pid: 88 }, platform: 'win32', expectedExecutable: executable,
    probe: async () => live, recordOwnedPid: recorded })).rejects.toThrow('does not match expected');
  expect(recorded).toHaveBeenCalledOnce();
});

it.each(['presented', 'focus-refused', 'occluded-before-click', 'click-only', 'foreign-presentation', 'incomplete-tuple'])(
  'sets up through MahoRD and requires fresh presentation: %s', async mode => {
    const controller = new AbortController();
    const order = [];
    const captureFailure = vi.fn(async () => {
      expect(controller.signal.aborted).toBe(true);
      order.push('failure-capture');
    });
    const gui = { pid: 81, executable: '/isolated/ferryx', processStart: '81:1' };
    const tuple = { backendSessionId: 'backend', incarnation: 'incarnation', daemonEpoch: '9007199254740993',
      frontendSessionId: 'frontend', paneIdentity: 'pane', bindingKey: 'binding', attemptGeneration: 1 };
    let receive;
    let hits = 0;
    const event = { kind: 'presentation', path: '/isolated/presentation', lineIndex: 3,
      receipt: { runId: 'run', operationId: 'op', presented: true, producerPid: gui.pid,
        producerComponent: 'surface-host-gpu-render', attachTuple: tuple } };
    const click = vi.fn(async point => {
      expect(point).toEqual({ x: 40, y: 60 });
      order.push('click');
      receive({ ...event, lineIndex: 2 });
      receive({ ...event, receipt: { ...event.receipt, operationId: 'foreign' } });
      receive({ ...event, receipt: { ...event.receipt, presented: false } });
      if (mode === 'click-only') { controller.abort(new Error('no presentation')); return; }
      receive({ ...event, receipt: { ...event.receipt,
        producerPid: mode === 'foreign-presentation' ? 999 : gui.pid,
        attachTuple: mode === 'incomplete-tuple' ? { backendSessionId: 'backend' } : tuple } });
    });
    const hotkey = vi.fn(async keys => { expect(keys).toEqual(['Alt', 'Escape']); order.push('focus'); });
    const pending = setupNativeTerminal({ profile: '/isolated/profile', gui, mahord: { click, hotkey },
      agent: { call: async (method, args) => {
        expect(method).toBe('setup-target');
        expect(args.profile).toBe('/isolated/profile');
        expect(args.gui).toBe(gui);
        order.push(args.point ? 'hit' : 'discover');
        if (!args.point) return { gui, hwnd: 19, point: { x: 40, y: 60 } };
        hits++;
        return { owned: mode !== 'focus-refused' && hits > 1 && !(mode === 'occluded-before-click' && hits === 3),
          foregroundPid: 81, foregroundHwnd: 19, hitHwnd: mode === 'occluded-before-click' && hits === 3 ? 99 : 19 };
      } },
      presentation: { cursor: async () => { order.push('cursor'); return { path: event.path, afterLine: 2 }; } },
      subscribe: callback => { receive = callback; order.push('subscribe'); return () => {}; },
      capture: async phase => { order.push(`capture:${phase}`); }, publish: vi.fn(),
      runId: 'run', operationId: 'op', signal: controller.signal, captureFailure });
    if (mode === 'presented') {
      expect((await pending).attachTuple).toBe(tuple);
      expect(order).toEqual(['discover', 'capture:setup-target', 'hit', 'focus', 'capture:setup-focus-0', 'hit',
        'discover', 'capture:setup-focused-target', 'cursor', 'subscribe', 'hit', 'click']);
    } else await expect(pending).rejects.toThrow();
    expect(click).toHaveBeenCalledTimes(['focus-refused', 'occluded-before-click'].includes(mode) ? 0 : 1);
    expect(captureFailure).toHaveBeenCalledTimes(mode === 'click-only' ? 1 : 0);
    expect(hotkey).toHaveBeenCalledTimes(mode === 'focus-refused' ? 8 : 1);
  },
);

it('publishes phase completion only after the operation settles without its payload', async () => {
  const events = [];
  const operation = Promise.withResolvers();
  const pending = traceNativeLaunchPhase(event => events.push(event), 'type-command', () => operation.promise);
  expect(events).toEqual([{ kind: 'native-launch-phase', phase: 'type-command', outcome: 'started' }]);
  operation.resolve({ secret: 'private tool response' });
  await expect(pending).resolves.toEqual({ secret: 'private tool response' });
  expect(events).toEqual([
    { kind: 'native-launch-phase', phase: 'type-command', outcome: 'started' },
    { kind: 'native-launch-phase', phase: 'type-command', outcome: 'completed' },
  ]);
});

it('preserves phase failure identity without recording error message or arguments', async () => {
  const events = [];
  const error = Object.assign(new Error('private command or credential'), { code: 'ABORT_ERR' });
  await expect(traceNativeLaunchPhase(event => events.push(event), 'press-enter', async () => { throw error; })).rejects.toBe(error);
  expect(events).toEqual([
    { kind: 'native-launch-phase', phase: 'press-enter', outcome: 'started' },
    { kind: 'native-launch-phase', phase: 'press-enter', outcome: 'failed', code: 'ABORT_ERR' },
  ]);
});

it.each(['none', 'screenshot', 'artifact', 'missing-path', 'setup-refusal'])(
  'records launch screenshots in desktop action order with explicit %s failure', async failureAt => {
    const controller = new AbortController();
    const order = [];
    const children = [];
    const failure = Object.assign(new Error('diagnostic evidence failed'), { code: 'SCREENSHOT_FAILED' });
    const receiptFailure = Object.assign(new Error('launcher receipt failed'), { code: 'GUI_RECEIPT_FAILED' });
    const png = Buffer.from('actual fake screenshot bytes');
    let agent;
    let receiptId;
    const reply = (child, packet) => child.stdout.write(`${JSON.stringify(packet)}\n`);
    const spawnProcess = (_executable, argv, options) => {
      const child = new EventEmitter();
      child.stdout = new PassThrough(); child.stderr = new PassThrough();
      child.stdin = new EventEmitter(); child.stdin.end = vi.fn();
      options.signal.addEventListener('abort', () => child.emit('error', options.signal.reason), { once: true });
      const mcp = argv.includes('--mcp');
      if (!mcp) agent = child;
      child.stdin.write = (line, callback) => {
        const packet = JSON.parse(line);
        callback?.();
        let result = {};
        if (mcp) {
          if (packet.method === 'tools/list') result = { tools: [
            'remote_hotkey', 'remote_type_text', 'remote_key_press', 'remote_take_screenshot',
          ].map(name => ({ name })) };
          if (packet.method === 'tools/call') {
            const name = packet.params.name;
            order.push(name);
            if (name === 'remote_take_screenshot' && failureAt === 'screenshot') {
              reply(child, { jsonrpc: '2.0', id: packet.id,
                error: { code: failure.code, message: failure.message } });
              return true;
            }
            result = { content: name === 'remote_take_screenshot'
              ? [{ type: 'image', mimeType: 'image/png', data: png.toString('base64') }] : [] };
          }
        } else {
          if (packet.method === 'cancel') return true;
          if (packet.method === 'prepare') result = { profile: '/isolated/profile', binary: '/isolated/ferryx', env: {} };
          if (packet.method === 'ownership-subscribe' || packet.method === 'barrier-cursor') result = { path: '/isolated/receipts', afterLine: 0 };
          if (packet.method === 'spawn-daemon') result = { role: 'daemon', status: 'ready', readiness: 'FERRYX_DAEMON_READY',
            runId: 'run-1', operationId: 'op-1', candidate: 'candidate-1', hostId: 'host-1',
            pid: 77, executable: '/isolated/ferryx', processStart: '77:1' };
          if (packet.method === 'probe') result = { exited: false, executable: '/isolated/ferryx', processStart: '77:1' };
          if (packet.method === 'start-forwarder-proxy') result = { endpoint: { host: 'isolated', port: 1234 } };
          if (packet.method === 'provision-gui-launch') result = { launcherShortPath: 'private launcher path' };
          if (packet.method === 'setup-target') {
            order.push('setup-target');
            expect(packet.args.gui).toMatchObject({ pid: 81, processStart: '81:1', role: 'gui' });
            reply(child, { id: packet.id, error: { code: 'SETUP_TARGET_REFUSED', message: 'owned target unavailable' } });
            return true;
          }
          if (packet.method === 'await-gui-launch') {
            receiptId = packet.id;
            reply(child, { event: 'gui-launch-subscribed', profile: '/isolated/profile' });
            return true;
          }
        }
        reply(child, { id: packet.id, result });
        return true;
      };
      children.push(child);
      return child;
    };
    const artifact = vi.fn(async (path, bytes) => {
      expect(bytes).toEqual(png);
      order.push(path);
      if (failureAt === 'artifact') throw failure;
      return failureAt === 'missing-path' ? undefined : `/evidence/${path}`;
    });
    const ctx = await createConfiguredNativeContext({
      platform: 'win32', sshDestination: 'isolated-host', remoteAgent: '/isolated/agent.mjs',
      worktree: '/isolated/worktree', canonicalClone: '/isolated/clone', worktreeRoot: '/isolated',
      mahordExecutable: 'mahord', mahordAddress: 'isolated-host', pairingId: 'isolated-pair',
      bun: '/isolated/bun', binary: '/isolated/ferryx', buildManifest: '/isolated/manifest', hostId: 'host-1',
    }, { candidate: { stamp: 'candidate-1' }, transport: { subscribe: () => () => {}, bindContext() {} },
      evidence: { artifact }, runId: 'run-1', mutation: { id: 'op-1' }, signal: controller.signal, spawnProcess });
    ctx.provider.subscribe(event => {
      if (event.phase === 'gui-launch-receipt' && event.outcome === 'started') {
        order.push('await-gui-receipt');
        reply(agent, failureAt === 'setup-refusal' ? { id: receiptId, result: {
          runId: 'run-1', operationId: 'op-1', hostId: 'host-1', candidate: 'candidate-1',
          role: 'gui', status: 'spawned', executable: '/isolated/ferryx', pid: 81, processStart: '81:1',
        } } : { id: receiptId, error: { code: receiptFailure.code, message: receiptFailure.message } });
      }
    });
    try {
      await ctx.spawn.prepare({ recordOwnedPid() {}, signal: controller.signal });
      const result = await ctx.spawn.launch({ signal: controller.signal }).catch(error => error);
      if (failureAt === 'setup-refusal') {
        expect(result).toMatchObject({ code: 'SETUP_TARGET_REFUSED' });
        expect(order.slice(-2)).toEqual(['await-gui-receipt', 'setup-target']);
        expect(ctx.rawJournal).toContainEqual(expect.objectContaining({ phase: 'presentation-binding-receipt', outcome: 'failed' }));
      } else if (failureAt === 'none') {
        expect(result).toBeInstanceOf(Error);
        expect(result).toMatchObject({ code: 'GUI_RECEIPT_FAILED', message: 'launcher receipt failed' });
        expect(order).toEqual([
          'remote_hotkey', 'remote_take_screenshot', 'native-run-1-launch-hotkey.png',
          'remote_hotkey', 'remote_type_text', 'remote_take_screenshot', 'native-run-1-type-command.png',
          'remote_key_press', 'remote_take_screenshot', 'native-run-1-press-enter.png', 'await-gui-receipt',
        ]);
        expect(ctx.rawJournal.filter(event => event.kind === 'native-launch-artifact')).toEqual(
          ['launch-hotkey', 'type-command', 'press-enter'].map(phase => ({
            kind: 'native-launch-artifact', phase, path: `/evidence/native-run-1-${phase}.png`, seq: expect.any(Number),
          })));
      } else {
        if (failureAt === 'screenshot') {
          expect(result).toBeInstanceOf(Error);
          expect(result).toMatchObject({ code: 'SCREENSHOT_FAILED', message: 'diagnostic evidence failed' });
        } else if (failureAt === 'artifact') expect(result).toBe(failure);
        else expect(result.message).toContain('screenshot artifact path is missing');
        expect(order).not.toContain('remote_type_text');
        expect(ctx.rawJournal.some(event => event.kind === 'native-launch-artifact')).toBe(false);
        expect(ctx.rawJournal).toContainEqual(expect.objectContaining({
          phase: 'launch-hotkey-screenshot', outcome: 'failed',
        }));
      }
      expect(JSON.stringify(ctx.rawJournal)).not.toContain('private launcher path');
    } finally {
      controller.abort(new Error('fixture finished'));
      ctx.closeConnections();
      for (const child of children) { child.stdout.destroy(); child.stderr.destroy(); }
    }
  },
);

function rpcFixture() {
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  child.stdin = new EventEmitter();
  child.stdin.write = vi.fn((_, callback) => { callback?.(); return true; });
  child.stdin.end = vi.fn();
  const rpc = rpcProcess('unused', [], {
    signal: new AbortController().signal, spawnProcess: () => child,
  });
  return { child, rpc };
}

it.each(['close', 'EOF'])('rejects pending and later calls after child %s', async kind => {
  const { child, rpc } = rpcFixture();
  const pending = rpc.call('probe', { pid: 1 }).catch(error => error);
  if (kind === 'close') child.emit('close', 0);
  else {
    const closed = new Promise(resolve => child.stdout.once('end', resolve));
    child.stdout.end();
    await closed;
  }
  const error = await pending;
  expect(error).toBeInstanceOf(Error);
  await expect(rpc.call('unwatch', {})).rejects.toBe(error);
  expect(child.stdin.write).toHaveBeenCalledTimes(1);
  child.stdout.destroy(); child.stderr.destroy();
});

it('settles a real child error and refuses later calls', async () => {
  const { child, rpc } = rpcFixture();
  const failure = Object.assign(new Error('child transport failed'), { code: 'EPIPE' });
  const pending = expect(rpc.call('probe', {})).rejects.toBe(failure);
  child.emit('error', failure);
  await pending;
  await expect(rpc.call('unwatch', {})).rejects.toBe(failure);
  expect(child.stdin.write).toHaveBeenCalledTimes(1);
  child.stdout.destroy(); child.stderr.destroy();
});

it.each(['callback', 'throw', 'event'])('settles write %s failures and refuses later calls', async kind => {
  const { child, rpc } = rpcFixture();
  const failure = Object.assign(new Error('broken pipe'), { code: 'EPIPE' });
  child.stdin.write.mockImplementation((_, callback) => {
    if (kind === 'throw') throw failure;
    if (kind === 'callback') callback(failure);
    else child.stdin.emit('error', failure);
    return false;
  });
  await expect(rpc.call('probe', {})).rejects.toBe(failure);
  await expect(rpc.call('unwatch', {})).rejects.toBe(failure);
  expect(child.stdin.write).toHaveBeenCalledTimes(1);
  child.stdout.destroy(); child.stderr.destroy();
});

describe('native connection initialization failure ownership', () => {
  it.each([
    ['direct', 'initialize'], ['direct', 'tools/list'],
    ['factory', 'initialize'], ['factory', 'tools/list'],
  ])('closes owned connections for %s %s failure and preserves the error', async (entry, failedMethod) => {
    const failure = { message: 'Maho initialization refused', code: -32001 };
    const scenario = new AbortController();
    const children = [];
    const methods = [];
    const spawnProcess = (_executable, argv, options) => {
      const child = new EventEmitter();
      child.stdout = new PassThrough();
      child.stderr = new PassThrough();
      child.stdin = new EventEmitter();
      child.stdin.end = vi.fn();
      child.signal = options.signal;
      child.stdin.write = vi.fn((line, callback) => {
        const packet = JSON.parse(line);
        callback?.();
        methods.push(packet.method);
        if (packet.method === failedMethod) child.stdout.write(`${JSON.stringify({ id: packet.id, error: failure })}\n`);
        else child.stdout.write(`${JSON.stringify({ id: packet.id, result: {} })}\n`);
        return true;
      });
      options.signal.addEventListener('abort', () => child.emit('error', options.signal.reason), { once: true });
      children.push({ child, mcp: argv.includes('--mcp') });
      return child;
    };
    try {
      const pending = entry === 'direct'
        ? connectNativeMahord({ executable: 'mahord', address: 'isolated-host', pairingId: 'isolated-pair',
          signal: scenario.signal, spawnProcess })
        : createConfiguredNativeContext({
          sshDestination: 'isolated-host', remoteAgent: '/isolated/agent.mjs',
          worktree: '/isolated/worktree', canonicalClone: '/isolated/clone', worktreeRoot: '/isolated',
          mahordExecutable: 'mahord', mahordAddress: 'isolated-host', pairingId: 'isolated-pair',
          bun: '/isolated/bun', binary: '/isolated/ferryx', buildManifest: '/isolated/manifest', hostId: 'host-1',
        }, { candidate: { stamp: 'candidate-1' }, transport: {}, evidence: { artifact: vi.fn() },
          runId: 'run-1', mutation: { id: 'op-1' }, signal: scenario.signal, spawnProcess });
      const rejected = await pending.catch(error => error);
      expect(rejected).toBeInstanceOf(Error);
      expect(rejected).toMatchObject(failure);
      expect(methods).toEqual(failedMethod === 'initialize' ? ['initialize'] : ['initialize', 'tools/list']);
      expect(children).toHaveLength(entry === 'direct' ? 1 : 2);
      for (const { child, mcp } of children) {
        expect(child.stdin.end).toHaveBeenCalledTimes(1);
        if (!mcp) {
          expect(child.signal.aborted).toBe(true);
          expect(child.signal.reason).toBe(rejected);
          expect(child.stdin.write).not.toHaveBeenCalled();
        }
      }
      expect(scenario.signal.aborted).toBe(false);
    } finally {
      for (const { child } of children) { child.stdout.destroy(); child.stderr.destroy(); }
    }
  });
});

describe('configured native teardown connection lifetime', () => {
  it.each(['success', 'missing-exit', 'export-failure', 'collect-failure', 'missing-path', 'cancel', 'frontend-failure'])('survives scenario abort and preserves cleanup evidence: %s', async mode => {
    const cancelCleanup = mode === 'cancel';
    const scenario = new AbortController();
    const cleanup = new AbortController();
    const calls = [];
    const children = [];
    const owner = { pid: 77, executable: '/isolated/ferryx', processStart: '77:1' };
    const scenarioError = Object.assign(new Error('scenario deadline'), { code: 'ABORT_ERR' });
    const cleanupError = Object.assign(new Error('cleanup deadline'), { code: 'NATIVE_CLEANUP_TIMEOUT' });
    const exportError = new Error('artifact export failed');
    const raw = Buffer.from([0, 255, 13, 10, 32]);
    const names = ['receipts/gui-launch.json', 'receipts/gui-launch.json.exit.json',
      'evidence/launcher-entry.log', 'evidence/launcher-output.log', 'evidence/launcher-exit.log'];
    const exported = [];
    let exitId;
    const spawnProcess = (_executable, argv, options) => {
      const child = new EventEmitter();
      child.stdout = new PassThrough();
      child.stderr = new PassThrough();
      child.stdin = new EventEmitter();
      child.stdin.end = vi.fn();
      child.signal = options.signal;
      const mcp = argv.includes('--mcp');
      const reply = (id, result) => child.stdout.write(`${JSON.stringify({ id, result })}\n`);
      child.stdin.write = vi.fn((line, callback) => {
        const packet = JSON.parse(line);
        callback?.();
        if (mcp) reply(packet.id, packet.method === 'tools/list' ? { tools: [] } : {});
        else {
          calls.push(packet);
          if (packet.method === 'start-frontend') {
            child.stdout.write(`${JSON.stringify({ id: packet.id, error: { code: 'FRONTEND_NOT_SERVED', message: 'frontend not ready' } })}\n`);
            return true;
          }
          if (packet.method === 'snapshot' || packet.method === 'cancel') return true;
          if (packet.method === 'exit') { exitId = packet.id; return true; }
          if (packet.method === 'probe' && cancelCleanup) return true;
          if (packet.method === 'terminate') reply(exitId, { ...owner, exited: true });
          if (packet.method === 'collect-launch-evidence' && mode === 'collect-failure') {
            child.stdout.write(`${JSON.stringify({ id: packet.id, error: { code: 'EACCES', message: 'read denied' } })}\n`);
            return true;
          }
          reply(packet.id, packet.method === 'probe' ? { ...owner, exited: false }
            : packet.method === 'collect-launch-evidence' ? names.map(name => mode === 'missing-exit' && name.endsWith('.exit.json')
              ? { name, missing: true } : { name, base64: raw.toString('base64') })
            : packet.method === 'prepare' ? { profile: '/isolated/profile' }
            : packet.method === 'worktrees' ? ['/isolated/worktree'] : {});
        }
        return true;
      });
      // Model node:child_process's spawn signal, not just per-call cancellation.
      options.signal.addEventListener('abort', () => child.emit('error', options.signal.reason), { once: true });
      children.push(child);
      return child;
    };
    const release = vi.fn(async () => {});
    const ctx = await createConfiguredNativeContext({
      sshDestination: 'isolated-host', remoteAgent: '/isolated/agent.mjs',
      worktree: '/isolated/worktree', canonicalClone: '/isolated/clone', worktreeRoot: '/isolated',
      mahordExecutable: 'mahord', mahordAddress: 'isolated-host', pairingId: 'isolated-pair',
      bun: '/isolated/bun', binary: owner.executable, buildManifest: '/isolated/manifest', hostId: 'host-1',
      frontend: { mode: 'debug', url: 'http://127.0.0.1:5173' },
    }, { candidate: { stamp: 'candidate-1' }, transport: { subscribe: () => () => {}, release },
      evidence: { artifact: async (path, bytes) => {
        expect(calls.some(packet => packet.method === 'remove-profile')).toBe(false);
        expect(bytes).toEqual(raw);
        if (mode === 'export-failure') throw exportError;
        if (mode === 'missing-path') return undefined;
        exported.push(path);
        return `/evidence/${path}`;
      } }, runId: 'run-1', mutation: { id: 'op-1' },
      signal: scenario.signal, spawnProcess });
    try {
      if (mode === 'frontend-failure') {
        await expect(ctx.spawn.prepare({ recordOwnedPid: vi.fn(), signal: scenario.signal })).rejects.toMatchObject({ code: 'FRONTEND_NOT_SERVED' });
        expect(calls.map(packet => packet.method)).toEqual(['prepare', 'start-frontend']);
        expect(calls[1].args).toEqual({ profile: '/isolated/profile', frontend: { mode: 'debug', url: 'http://127.0.0.1:5173' } });
        await ctx.transport.release({ signal: cleanup.signal });
        expect(calls.map(packet => packet.method)).toEqual(['prepare', 'start-frontend', 'unwatch']);
        return;
      }
      const pending = ctx.host.subscribeExit({ pid: owner.pid, processStart: owner.processStart,
        signal: scenario.signal }).catch(error => error);
      scenario.abort(scenarioError);
      expect(await pending).toBe(scenarioError);
      expect(children[0].signal.aborted).toBe(false);
      expect(children[1].signal.aborted).toBe(false);
      await ctx.transport.release({ signal: cleanup.signal });
      expect(release).toHaveBeenCalledWith({ signal: cleanup.signal });
      const probe = ctx.host.probePid({ pid: owner.pid, signal: cleanup.signal });
      if (cancelCleanup) {
        const rejected = probe.catch(error => error);
        cleanup.abort(cleanupError);
        expect(await rejected).toBe(cleanupError);
        expect(children[0].signal.reason).toBe(cleanupError);
        await expect(ctx.host.worktreeList({ signal: new AbortController().signal })).rejects.toBe(cleanupError);
      } else {
        expect(await probe).toEqual({ ...owner, exited: false });
        const exit = ctx.host.subscribeExit({ pid: owner.pid, processStart: owner.processStart, signal: cleanup.signal });
        expect(() => ctx.host.terminateExact({ ...owner, tree: true, signal: cleanup.signal })).toThrow('Process tree termination is forbidden');
        await ctx.host.terminateExact({ ...owner, tree: false, signal: cleanup.signal });
        expect(await exit).toEqual({ ...owner, exited: true });
        const removal = ctx.host.removeProfile({ setup: { profile: '/isolated/profile' }, signal: cleanup.signal });
        const failed = ['export-failure', 'collect-failure', 'missing-path'].includes(mode);
        if (mode === 'export-failure') await expect(removal).rejects.toBe(exportError);
        else if (mode === 'collect-failure') await expect(removal).rejects.toMatchObject({ code: 'EACCES', message: 'read denied' });
        else if (mode === 'missing-path') await expect(removal).rejects.toThrow('artifact path is missing');
        else {
          await removal;
          expect(exported).toEqual(names.filter(name => mode !== 'missing-exit' || !name.endsWith('.exit.json'))
            .map(name => `native-run-1-${name.replaceAll('/', '-')}`));
        }
        expect(calls.some(packet => packet.method === 'remove-profile')).toBe(!failed);
        await expect(ctx.host.worktreeList({ signal: cleanup.signal })).resolves.toEqual(['/isolated/worktree']);
        expect(calls.filter(packet => packet.method === 'terminate')).toEqual([
          { id: expect.any(Number), method: 'terminate', args: owner },
        ]);
        expect(calls.map(packet => packet.method)).toEqual([
          'exit', 'cancel', 'unwatch', 'probe', 'exit', 'terminate', 'collect-launch-evidence',
          ...(failed ? [] : ['remove-profile']), 'worktrees',
        ]);
      }
      expect(scenario.signal.reason).toBe(scenarioError);
    } finally {
      ctx.closeConnections();
      expect(children[0].signal.aborted).toBe(true);
      for (const child of children) { child.stdout.destroy(); child.stderr.destroy(); }
    }
  });
});

describe('configured native producer event adapter', () => {
  it('preserves actual producer fields and adds only the transport source', () => {
    const raw = {
      kind: 'binding-persisted',
      runId: 'run-1',
      operationId: 'mutation-1',
      attachTuple: {
        backendSessionId: 'backend-1',
        frontendSessionId: 'frontend-1',
        paneIdentity: 'pane-1',
        incarnation: 'inc-1',
        daemonEpoch: 'epoch-1',
        bindingKey: 'bind-1',
        attemptGeneration: 2,
      },
      producerPid: 42,
    };

    expect(nativeProducerEvent(raw)).toEqual({ ...raw, source: 'native-events.jsonl' });
    expect(raw).not.toHaveProperty('source');
  });

  it.each([null, [], {}, { kind: 1 }, { event: 'attached' }])(
    'rejects malformed or untyped event %j without manufacturing a receipt',
    (raw) => expect(nativeProducerEvent(raw)).toBeNull(),
  );
});

describe('registered native audit owner join', () => {
  const owners = [
    { ownerId: 'daemon-local', pid: 10, executable: '/isolated/daemon', processStart: '10:1', component: 'local-pty' },
    { ownerId: 'remote-forwarder', pid: 20, executable: '/isolated/forwarder', processStart: '20:1', component: 'remote-audit' },
  ];
  const packet = (owner, baselineSeq, endSeq) => ({ auditId: 'audit-1', owner,
    baselineSeq, endSeq, complete: true,
    requests: [{ kind: 'request', ownerId: owner.ownerId, pid: owner.pid, executable: owner.executable,
      processStart: owner.processStart, seq: baselineSeq + 1, operation: 'write' }],
    deliveries: [{ kind: 'delivery', ownerId: owner.ownerId, pid: owner.pid, executable: owner.executable,
      processStart: owner.processStart, seq: endSeq, operation: 'write' }],
  });

  it('joins complete owner-local intervals without collapsing sequence provenance', () => {
    const result = joinNativeAuditOwners({ auditId: 'audit-1', scope: 'registered-isolated-owner-set',
      registeredOwners: owners, packets: [packet(owners[0], 4, 8), packet(owners[1], 20, 24)] });
    expect(result).toMatchObject({ complete: true, ingress: true, deliveryCoverage: true,
      scope: 'registered-isolated-owner-set', throughSeq: 24 });
    expect(result.requests.map((event) => [event.ownerId, event.seq])).toEqual([['daemon-local', 5], ['remote-forwarder', 21]]);
    expect(result.deliveries.map((event) => [event.ownerId, event.seq])).toEqual([['daemon-local', 8], ['remote-forwarder', 24]]);
  });

  it.each([
    { name: 'missing owner', packets: [packet(owners[0], 0, 2)] },
    { name: 'duplicate owner', packets: [packet(owners[0], 0, 2), packet(owners[0], 2, 4)] },
    { name: 'wrong process identity', packets: [packet(owners[0], 0, 2), packet({ ...owners[1], pid: 21 }, 2, 4)] },
    { name: 'sequence before baseline', packets: [packet(owners[0], 0, 2), { ...packet(owners[1], 2, 4), requests: [{ ...packet(owners[1], 2, 4).requests[0], seq: 2 }] }] },
  ])('returns typed incomplete for $name', ({ packets }) => {
    expect(joinNativeAuditOwners({ auditId: 'audit-1', scope: 'registered-isolated-owner-set',
      registeredOwners: owners, packets })).toMatchObject({ complete: false, ingress: false,
      deliveryCoverage: false, scope: 'registered-isolated-owner-set' });
  });

  it('does not accept one owner as all-process completeness', () => {
    expect(joinNativeAuditOwners({ auditId: 'audit-1', scope: 'all-isolated-processes',
      registeredOwners: [owners[0]], packets: [packet(owners[0], 0, 2)] })).toMatchObject({ complete: false });
  });

  it('refuses when required producer channel is missing from manifest', () => {
    const result = joinNativeAuditOwners({
      auditId: 'audit-1',
      scope: 'registered-isolated-owner-set',
      registeredOwners: [owners[0]],
      packets: [packet(owners[0], 0, 2)],
      requiredComponents: ['local-pty', 'remote-audit'],
    });
    expect(result).toMatchObject({
      complete: false,
      ingress: false,
      deliveryCoverage: false,
      scope: 'registered-isolated-owner-set',
      detail: 'Missing required producer channel coverage for remote-audit',
    });
  });

  it('preserves empty valid intervals when instrumented channel coverage is explicit', () => {
    const emptyPacket = (owner, baselineSeq, endSeq) => ({
      auditId: 'audit-1',
      owner,
      baselineSeq,
      endSeq,
      complete: true,
      requests: [],
      deliveries: [],
    });
    const result = joinNativeAuditOwners({
      auditId: 'audit-1',
      scope: 'registered-isolated-owner-set',
      registeredOwners: owners,
      packets: [emptyPacket(owners[0], 0, 0), emptyPacket(owners[1], 0, 0)],
    });
    expect(result).toMatchObject({
      complete: true,
      ingress: true,
      deliveryCoverage: true,
      scope: 'registered-isolated-owner-set',
      requests: [],
      deliveries: [],
      throughSeq: 0,
    });
  });
});

describe('private local PTY owner command RPC', () => {
  it('requires correlated command acknowledgement and a drained close packet', async () => {
    const files = new Map();
    let notify;
    const rpc = createLocalPtyOwnerRpc({ profile: '/isolated/profile', runId: 'run-1', operationId: 'op-1',
      isProfileActive: () => true,
      ownerCommandId: (() => { let seq = 0; return () => `cmd-${++seq}`; })(),
      read: async (path) => {
        if (!files.has(path)) throw Object.assign(new Error('missing'), { code: 'ENOENT' });
        return files.get(path);
      },
      write: async (path, content) => { files.set(path, content); },
      renameFile: async (from, to) => {
        const value = files.get(from); files.delete(from);
        const request = JSON.parse(value);
        const filename = to.split('/').at(-1);
        const command = filename.startsWith('local-pty-owner-identity') ? 'identity'
          : filename.startsWith('local-pty-owner-open') ? 'open'
          : filename.startsWith('local-pty-owner-read') ? 'read'
          : filename.startsWith('local-pty-owner-close') ? 'close' : 'close';
        const owner = { ownerId: 'terminal-local-pty:run-1:77', pid: 77,
          executable: '/isolated/ferryx', processStart: '77:000001', component: 'terminal-local-pty' };
        const packet = { auditId: 'audit-1', runId: 'run-1', operationId: 'op-1', owner,
          baselineSeq: 2, endSeq: 4, complete: command === 'close', pending: command === 'close' ? 0 : 1,
          requests: [], deliveries: [] };
        if (command === 'read') files.set('/isolated/profile/barriers/local-pty-owner-read.receipt.json', JSON.stringify(packet));
        if (command === 'close') files.set('/isolated/profile/barriers/local-pty-owner-audit.json', JSON.stringify(packet));
        const commandFile = { identity: 'local-pty-owner-identity', open: 'local-pty-owner-open', read: 'local-pty-owner-read', close: 'local-pty-owner-close' }[command];
        files.set(`/isolated/profile/barriers/${commandFile}.ack.${request.commandId}.json`, JSON.stringify({
          commandId: request.commandId, runId: 'run-1', operationId: 'op-1', accepted: true,
          ...(command === 'identity' ? { commandSeq: request.commandSeq, owner } : {}),
          packetFile: command === 'read' ? 'local-pty-owner-read.receipt.json' : 'local-pty-owner-audit.json',
        }));
        notify('rename', `${commandFile}.ack.${request.commandId}.json`);
      },
      subscribe: (_path, listener) => { notify = listener; return { close() {} }; },
    });
    const identity = await rpc.identity({ runId: 'run-1', operationId: 'op-1' });
    expect(identity.owner.ownerId).toBe('terminal-local-pty:run-1:77');
    expect(identity.commandSeq).toBe(0);
    expect(rpc.nextCommandSeq).toBe(1);
    const opened = await rpc.open({ runId: 'run-1', operationId: 'op-1', auditId: 'audit-1' });
    expect(opened.ack.commandId).toBe('cmd-2');
    expect(opened.commandSeq).toBe(1);
    const snapshot = await rpc.read({ runId: 'run-1', operationId: 'op-1', auditId: 'audit-1' });
    expect(snapshot.packet.owner.ownerId).toBe('terminal-local-pty:run-1:77');
    expect(snapshot.commandSeq).toBe(2);
    const close = await rpc.close({ runId: 'run-1', operationId: 'op-1', auditId: 'audit-1' });
    expect(close.packet.complete).toBe(true);
    expect(close.commandSeq).toBe(3);
    await rpc.dispose();
  });

  it('does not accept an ack without its physical packet', async () => {
    const files = new Map();
    let notify;
    const rpc = createLocalPtyOwnerRpc({ profile: '/isolated/profile', runId: 'run-1', operationId: 'op-1',
      isProfileActive: () => true,
      ownerCommandId: (() => { let seq = 0; return () => `cmd-${++seq}`; })(),
      read: async (path) => {
        if (!files.has(path)) throw Object.assign(new Error('missing'), { code: 'ENOENT' });
        return files.get(path);
      }, write: async (path, content) => files.set(path, content),
      renameFile: async (from, to) => {
        const request = JSON.parse(files.get(from));
        files.delete(from);
        const name = to.split('/').at(-1).replace('.request.json', '');
        files.set(`/isolated/profile/barriers/${name}.ack.${request.commandId}.json`, JSON.stringify({
          commandId: request.commandId, runId: 'run-1', operationId: 'op-1', accepted: true,
          packetFile: 'local-pty-owner-audit.json',
        }));
        notify('rename', `${name}.ack.${request.commandId}.json`);
      }, subscribe: (_path, listener) => { notify = listener; return { close() {} }; },
    });
    await rpc.open({ runId: 'run-1', operationId: 'op-1', auditId: 'audit-1' });
    await expect(rpc.close({ runId: 'run-1', operationId: 'op-1', auditId: 'audit-1' })).rejects.toMatchObject({ code: 'ENOENT' });
    await expect(rpc.dispose()).rejects.toMatchObject({ code: 'ENOENT' });
  });
});

describe('configured native adapter snapshot request and validation', () => {
  const splitTuple = (pane, backend) => ({ backendSessionId: backend, incarnation: 'i', daemonEpoch: 'e', frontendSessionId: 'f-' + pane,
    paneIdentity: pane, bindingKey: 'k-' + pane, attemptGeneration: 1 });
  const splitHarness = (events) => {
    const listeners = new Set();
    const order = [];
    const mahord = { hotkey: vi.fn(async (keys) => { order.push(['hotkey', keys.join('+')]);
      for (const event of events) for (const listener of [...listeners]) listener(event); }) };
    const subscribe = (listener) => { order.push(['subscribe']); listeners.add(listener); return () => listeners.delete(listener); };
    return { mahord, subscribe, order, listeners };
  };
  const presented = (tuple, extra = {}) => ({ kind: 'presentation', path: '/p', lineIndex: 5,
    receipt: { runId: 'run', operationId: 'op', presented: true, producerComponent: 'surface-host-gpu-render', producerPid: 9,
      attachTuple: tuple, ...tuple, ...extra } });

  it('splits with the shell-safe chord, subscribes first and settles only on another presented pane', async () => {
    const target = splitTuple('pane-a', 'backend-a');
    const h = splitHarness([presented(target), presented(splitTuple('pane-b', 'backend-b'), { producerPid: 8 }),
      presented(splitTuple('pane-b', 'backend-b'))]);
    const receipt = await splitSecondPane({ mahord: h.mahord, platform: 'win32', presentation: { cursor: async () => ({ path: '/p', afterLine: 1 }) },
      subscribe: h.subscribe, target, gui: { pid: 9 }, runId: 'run', operationId: 'op', signal: new AbortController().signal });
    expect(h.order).toEqual([['subscribe'], ['hotkey', 'Control+Shift+d']]);
    expect(receipt.paneIdentity).toBe('pane-b');
    expect(receipt.producerPid).toBe(9);
    expect(h.listeners.size).toBe(0);
  });

  it('second pane wait is cancellable and never accepts the target pane', async () => {
    const target = splitTuple('pane-a', 'backend-a');
    const h = splitHarness([presented(target)]);
    const controller = new AbortController();
    const pending = splitSecondPane({ mahord: h.mahord, platform: 'linux', presentation: { cursor: async () => ({ path: '/p', afterLine: 1 }) },
      subscribe: h.subscribe, target, gui: { pid: 9 }, runId: 'run', operationId: 'op', signal: controller.signal });
    await vi.waitFor(() => expect(h.mahord.hotkey).toHaveBeenCalledOnce());
    controller.abort(new Error('cancelled'));
    await expect(pending).rejects.toThrow('cancelled');
    expect(h.listeners.size).toBe(0);
  });

  it('attaches spawn-joined PTY identity only for the same session, incarnation and epoch', () => {
    const tuple = { backendSessionId: 'backend-1', incarnation: 'inc-1', daemonEpoch: 'epoch-1' };
    const snapshot = { attachTuple: tuple, target: { inputPoint: { x: 10, y: 20 } } };
    const joined = { pid: 77, processStart: '77:1', executable: 'C:\\pwsh.exe', ...tuple };
    expect(withJoinedTargetIdentity(snapshot, joined).target).toEqual({ inputPoint: { x: 10, y: 20 }, pid: 77,
      processStart: '77:1', executable: 'C:\\pwsh.exe', ...tuple });
    for (const field of Object.keys(tuple)) {
      expect(withJoinedTargetIdentity(snapshot, { ...joined, [field]: 'other' }).target.pid).toBeUndefined();
    }
    expect(withJoinedTargetIdentity(snapshot, null).target.pid).toBeUndefined();
  });

  const targetTuple = {
    backendSessionId: 'backend-1', incarnation: 'inc-1', daemonEpoch: 'epoch-1',
    frontendSessionId: 'frontend-1', paneIdentity: 'pane-1', bindingKey: 'binding-1', attemptGeneration: 1,
  };
  const guiReceipt = { pid: 42, role: 'gui', processStart: '42:1' };
  const validSnapshot = {
    producerComponent: 'native-frontend-snapshot',
    attachTuple: { ...targetTuple },
    target: { inputPoint: { x: 10, y: 20 } },
    focus: { focused: true, paneIdentity: 'pane-1', inputPoint: { x: 10, y: 20 } },
    paneBounds: { x: 0, y: 0, width: 80, height: 24, scaleFactor: 1, inputPoint: { x: 10, y: 20 }, focusedInputPoint: { x: 10, y: 20 } },
  };

  it('sends correlated snapshot request with clamped timeout and returns validated snapshot', async () => {
    let capturedCall;
    const agent = {
      call: vi.fn(async (method, args) => {
        capturedCall = { method, args };
        return {
          runId: 'run-1',
          operationId: 'op-1',
          requestId: args.requestId,
          producer: 'ipc-qa-barrier',
          producerPid: 42,
          status: 'ok',
          snapshot: validSnapshot,
        };
      }),
    };
    const result = await requestNativeSnapshot({
      agent,
      profile: '/isolated/profile',
      runId: 'run-1',
      operationId: 'op-1',
      guiReceipt,
      targetTuple,
      timeoutMs: 10_000,
    });
    expect(result).toEqual(validSnapshot);
    expect(capturedCall.method).toBe('snapshot');
    expect(capturedCall.args).toMatchObject({
      profile: '/isolated/profile',
      runId: 'run-1',
      operationId: 'op-1',
      attachTuple: targetTuple,
      timeoutMs: 5_000,
    });
    expect(capturedCall.args.requestId).toMatch(/^[0-9a-f-]{36}$/);
  });

  it('refuses when target tuple or GUI receipt is missing/incomplete before remote call', async () => {
    const agent = { call: vi.fn() };
    await expect(requestNativeSnapshot({ agent, profile: '/p', runId: 'r', operationId: 'o', guiReceipt, targetTuple: null }))
      .rejects.toThrow('Exact target 7-tuple required');
    await expect(requestNativeSnapshot({ agent, profile: '/p', runId: 'r', operationId: 'o', guiReceipt, targetTuple: { backendSessionId: 'b' } }))
      .rejects.toThrow('presentation receipt missing 7-tuple field');
    await expect(requestNativeSnapshot({ agent, profile: '/p', runId: 'r', operationId: 'o', guiReceipt: null, targetTuple }))
      .rejects.toThrow('GUI launch receipt is required');
    await expect(requestNativeSnapshot({ agent, profile: '/p', runId: 'r', operationId: 'o', guiReceipt: { role: 'gui' }, targetTuple }))
      .rejects.toThrow('GUI launch receipt is required');
    expect(agent.call).not.toHaveBeenCalled();
  });

  it.each([
    ['foreign-run', { runId: 'foreign' }],
    ['foreign-operation', { operationId: 'foreign' }],
    ['foreign-request', { requestId: 'foreign-id' }],
    ['wrong-producer', { producer: 'wrong-producer' }],
    ['wrong-producer-pid', { producerPid: 999 }],
    ['refused-status', { status: 'refused', code: 'SNAPSHOT_TIMEOUT' }],
    ['missing-snapshot', { status: 'ok', snapshot: null }],
  ])('refuses response with %s', async (_name, patch) => {
    const agent = {
      call: vi.fn(async (_m, args) => ({
        runId: 'run-1',
        operationId: 'op-1',
        requestId: args.requestId,
        producer: 'ipc-qa-barrier',
        producerPid: 42,
        status: 'ok',
        snapshot: validSnapshot,
        ...patch,
      })),
    };
    await expect(requestNativeSnapshot({
      agent, profile: '/p', runId: 'run-1', operationId: 'op-1', guiReceipt, targetTuple,
    })).rejects.toThrow();
  });

  it.each([
    'backendSessionId', 'incarnation', 'daemonEpoch', 'frontendSessionId',
    'paneIdentity', 'bindingKey', 'attemptGeneration',
  ])('refuses response when returned snapshot tuple disagrees on %s', async field => {
    const agent = {
      call: vi.fn(async (_m, args) => ({
        runId: 'run-1',
        operationId: 'op-1',
        requestId: args.requestId,
        producer: 'ipc-qa-barrier',
        producerPid: 42,
        status: 'ok',
        snapshot: {
          ...validSnapshot,
          attachTuple: {
            ...targetTuple,
            [field]: field === 'attemptGeneration' ? 999 : 'discrepancy',
          },
        },
      })),
    };
    await expect(requestNativeSnapshot({
      agent, profile: '/p', runId: 'run-1', operationId: 'op-1', guiReceipt, targetTuple,
    })).rejects.toThrow(`Native snapshot target tuple mismatch on ${field}`);
  });

  it('keeps concurrent requests for distinct targets independent', async () => {
    const targetA = { ...targetTuple, paneIdentity: 'pane-A', bindingKey: 'key-A' };
    const targetB = { ...targetTuple, paneIdentity: 'pane-B', bindingKey: 'key-B' };
    const calls = [];
    const agent = {
      call: vi.fn(async (_m, args) => {
        calls.push(args);
        const isA = args.attachTuple.paneIdentity === 'pane-A';
        return {
          runId: 'run-1',
          operationId: 'op-1',
          requestId: args.requestId,
          producer: 'ipc-qa-barrier',
          producerPid: 42,
          status: 'ok',
          snapshot: {
            ...validSnapshot,
            attachTuple: isA ? targetA : targetB,
          },
        };
      }),
    };
    const [resA, resB] = await Promise.all([
      requestNativeSnapshot({ agent, profile: '/p', runId: 'run-1', operationId: 'op-1', guiReceipt, targetTuple: targetA }),
      requestNativeSnapshot({ agent, profile: '/p', runId: 'run-1', operationId: 'op-1', guiReceipt, targetTuple: targetB }),
    ]);
    expect(resA.attachTuple.paneIdentity).toBe('pane-A');
    expect(resB.attachTuple.paneIdentity).toBe('pane-B');
    expect(calls).toHaveLength(2);
    expect(calls[0].requestId).not.toBe(calls[1].requestId);
  });

  it('sendUncertain enforces client to screen conversion and throws NATIVE_SCREEN_GEOMETRY_REQUIRED when missing', async () => {
    const scenario = new AbortController();
    const children = [];
    const mahordCalls = [];
    let snapshotReply;
    const daemon = {
      role: 'daemon', status: 'ready', readiness: 'FERRYX_DAEMON_READY',
      runId: 'run-1', operationId: 'op-1', candidate: 'candidate-1', hostId: 'host-1',
      pid: 101, executable: '/isolated/ferryx', processStart: '101:100', port: 9999,
    };
    const setupReceipt = { profile: '/isolated/profile', binary: '/isolated/ferryx', env: {} };
    const spawnProcess = (_executable, argv, options) => {
      const child = new EventEmitter();
      child.stdout = new PassThrough();
      child.stderr = new PassThrough();
      child.stdin = new EventEmitter();
      child.stdin.end = vi.fn();
      child.signal = options.signal;
      const mcp = argv.includes('--mcp');
      const reply = (id, result) => child.stdout.write(`${JSON.stringify({ id, result })}\n`);
      child.stdin.write = vi.fn((line, callback) => {
        const packet = JSON.parse(line);
        callback?.();
        if (mcp) {
          mahordCalls.push(packet);
          reply(packet.id, packet.method === 'tools/list' ? { tools: [] } : { content: [{ type: 'text', text: 'ok' }] });
        } else {
          if (packet.method === 'snapshot') {
            reply(packet.id, {
              runId: 'run-1',
              operationId: 'op-1',
              requestId: packet.args.requestId,
              producer: 'ipc-qa-barrier',
              producerPid: 77,
              status: 'ok',
              snapshot: snapshotReply,
            });
          } else if (packet.method === 'barrier-bound') {
            reply(packet.id, { bound: true });
          } else if (packet.method === 'prepare') {
            reply(packet.id, setupReceipt);
          } else if (packet.method === 'spawn-daemon') {
            reply(packet.id, daemon);
          } else if (packet.method === 'probe') {
            reply(packet.id, { pid: 101, executable: '/isolated/ferryx', processStart: '101:100', exited: false });
          } else if (packet.method === 'start-forwarder-proxy') {
            reply(packet.id, { endpoint: { path: '/isolated/sock' } });
          } else {
            reply(packet.id, {});
          }
        }
        return true;
      });
      children.push(child);
      return child;
    };

    const ctx = await createConfiguredNativeContext({
      sshDestination: 'isolated-host', remoteAgent: '/isolated/agent.mjs',
      worktree: '/isolated/worktree', canonicalClone: '/isolated/clone', worktreeRoot: '/isolated',
      mahordExecutable: 'mahord', mahordAddress: 'isolated-host', pairingId: 'isolated-pair',
      bun: '/isolated/bun', binary: '/isolated/ferryx', buildManifest: '/isolated/manifest', hostId: 'host-1',
    }, {
      candidate: { stamp: 'candidate-1' },
      transport: { subscribe: () => () => {}, release: vi.fn(), armLostAcknowledgment: vi.fn(), bindContext: vi.fn() },
      evidence: { artifact: vi.fn() },
      runId: 'run-1',
      mutation: { id: 'op-1', operation: 'write', payload: 'test' },
      signal: scenario.signal,
      spawnProcess,
    });

    await ctx.spawn.prepare({ recordOwnedPid: vi.fn(), signal: scenario.signal });

    // When windowOrigin is missing from paneBounds, throws NATIVE_SCREEN_GEOMETRY_REQUIRED
    // The real sendUncertain reads geometry through this.snapshot(); stub only that read.
    ctx.provider.snapshot = async () => snapshotReply;
    snapshotReply = {
      attachTuple: targetTuple,
      paneBounds: { inputPoint: { x: 10, y: 20 }, focusedInputPoint: { x: 30, y: 40 }, scaleFactor: 2 },
    };
    await expect(ctx.provider.sendUncertain({ mutation: ctx.mutation, signal: scenario.signal }))
      .rejects.toMatchObject({ code: 'NATIVE_SCREEN_GEOMETRY_REQUIRED', missingField: 'windowOrigin' });
  });

  it('emitOutput probes processStart via host.probePid when omitted by agent', async () => {
    const scenario = new AbortController();
    const daemon = {
      role: 'daemon', status: 'ready', readiness: 'FERRYX_DAEMON_READY',
      runId: 'run-1', operationId: 'op-1', candidate: 'candidate-1', hostId: 'host-1',
      pid: 202, executable: '/isolated/ferryx', processStart: '202:200', port: 9999,
    };
    const setupReceipt = { profile: '/isolated/profile', binary: '/isolated/ferryx', env: {} };
    const spawnProcess = (_executable, argv, options) => {
      const child = new EventEmitter();
      child.stdout = new PassThrough();
      child.stderr = new PassThrough();
      child.stdin = new EventEmitter();
      child.stdin.end = vi.fn();
      child.signal = options.signal;
      const mcp = argv.includes('--mcp');
      const reply = (id, result) => child.stdout.write(`${JSON.stringify({ id, result })}\n`);
      child.stdin.write = vi.fn((line, callback) => {
        const packet = JSON.parse(line);
        callback?.();
        if (mcp) reply(packet.id, packet.method === 'tools/list' ? { tools: [] } : {});
        else {
          if (packet.method === 'fixture-emit-marker') reply(packet.id, { pid: 456, marker: 'm-1' });
          else if (packet.method === 'probe') {
            const pid = packet.args?.pid;
            if (pid === 456) reply(packet.id, { pid: 456, processStart: '456:12345' });
            else reply(packet.id, { pid: 202, executable: '/isolated/ferryx', processStart: '202:200', exited: false });
          } else if (packet.method === 'prepare') {
            reply(packet.id, setupReceipt);
          } else if (packet.method === 'spawn-daemon') {
            reply(packet.id, daemon);
          } else if (packet.method === 'start-forwarder-proxy') {
            reply(packet.id, { endpoint: { path: '/isolated/sock' } });
          } else reply(packet.id, {});
        }
        return true;
      });
      return child;
    };

    const ctx = await createConfiguredNativeContext({
      sshDestination: 'isolated-host', remoteAgent: '/isolated/agent.mjs',
      worktree: '/isolated/worktree', canonicalClone: '/isolated/clone', worktreeRoot: '/isolated',
      mahordExecutable: 'mahord', mahordAddress: 'isolated-host', pairingId: 'isolated-pair',
      bun: '/isolated/bun', binary: '/isolated/ferryx', buildManifest: '/isolated/manifest', hostId: 'host-1',
    }, {
      candidate: { stamp: 'candidate-1' }, transport: { subscribe: () => () => {}, release: vi.fn(), bindContext: vi.fn() },
      evidence: { artifact: vi.fn() }, runId: 'run-1', mutation: { id: 'op-1' }, signal: scenario.signal, spawnProcess,
    });

    await ctx.spawn.prepare({ recordOwnedPid: vi.fn(), signal: scenario.signal });

    const emitted = await ctx.transport.emitOutput({ marker: 'm-1', signal: scenario.signal });
    expect(emitted).toEqual({ pid: 456, marker: 'm-1', processStart: '456:12345' });
  });

  it('forwards remote agent forwarder-receipt events into the adapter timeline and maps lost-ack-suppressed to mutation-outcome unknown', async () => {
    const scenario = new AbortController();
    let agentStdout;
    const spawnProcess = (_executable, argv, options) => {
      const child = new EventEmitter();
      child.stdout = new PassThrough();
      child.stderr = new PassThrough();
      child.stdin = new EventEmitter();
      child.stdin.end = vi.fn();
      child.signal = options.signal;
      const mcp = argv.includes('--mcp');
      if (!mcp) agentStdout = child.stdout;
      const reply = (id, result) => child.stdout.write(`${JSON.stringify({ id, result })}\n`);
      child.stdin.write = vi.fn((line, callback) => {
        const packet = JSON.parse(line);
        callback?.();
        reply(packet.id, mcp && packet.method === 'tools/list' ? { tools: [] } : {});
        return true;
      });
      return child;
    };

    const ctx = await createConfiguredNativeContext({
      sshDestination: 'isolated-host', remoteAgent: '/isolated/agent.mjs',
      worktree: '/isolated/worktree', canonicalClone: '/isolated/clone', worktreeRoot: '/isolated',
      mahordExecutable: 'mahord', mahordAddress: 'isolated-host', pairingId: 'isolated-pair',
      bun: '/isolated/bun', binary: '/isolated/ferryx', buildManifest: '/isolated/manifest', hostId: 'host-1',
    }, {
      candidate: { stamp: 'candidate-1' }, transport: { subscribe: () => () => {}, release: vi.fn() },
      evidence: { artifact: vi.fn() }, runId: 'run-1', mutation: { id: 'op-1' }, signal: scenario.signal, spawnProcess,
    });

    const seen = [];
    ctx.provider.subscribe((e) => seen.push(e));
    agentStdout.write(`${JSON.stringify({ event: 'forwarder-receipt', profile: '/isolated/profile', kind: 'request', operation: 'write', mutationId: 'op-1', payload: 'abc', seq: 1 })}\n`);
    agentStdout.write(`${JSON.stringify({ event: 'forwarder-receipt', profile: '/isolated/profile', kind: 'delivery', operation: 'write', mutationId: 'op-1', payload: 'abc', seq: 2 })}\n`);
    agentStdout.write(`${JSON.stringify({ event: 'forwarder-receipt', profile: '/isolated/profile', kind: 'lost-ack-suppressed', mutationId: 'op-1', sessionId: 's-1', seq: 3 })}\n`);

    await new Promise((r) => setTimeout(r, 10));
    expect(seen.find((e) => e.kind === 'request' && e.mutationId === 'op-1')).toMatchObject({ operation: 'write', payload: 'abc', source: 'forwarder-receipt' });
    expect(seen.find((e) => e.kind === 'delivery' && e.mutationId === 'op-1')).toMatchObject({ operation: 'write', payload: 'abc', source: 'forwarder-receipt' });
    expect(seen.find((e) => e.kind === 'mutation-outcome' && e.mutationId === 'op-1')).toMatchObject({ outcome: 'unknown', source: 'forwarder-receipt' });
  });

  it('emitOutput on win32 maps verified fixtureChild PID to live PTY root identity', async () => {
    const scenario = new AbortController();
    const daemon = {
      role: 'daemon', status: 'ready', readiness: 'FERRYX_DAEMON_READY',
      runId: 'run-1', operationId: 'op-1', candidate: 'candidate-1', hostId: 'host-1',
      pid: 202, executable: 'C:\\isolated\\ferryx.exe', processStart: '202:200', port: 9999,
    };
    const setupReceipt = { profile: 'C:\\isolated\\profile', binary: 'C:\\isolated\\ferryx.exe', env: {} };
    const spawnProcess = (_executable, argv, options) => {
      const child = new EventEmitter();
      child.stdout = new PassThrough();
      child.stderr = new PassThrough();
      child.stdin = new EventEmitter();
      child.stdin.end = vi.fn();
      child.signal = options.signal;
      const mcp = argv.includes('--mcp');
      const reply = (id, result) => child.stdout.write(`${JSON.stringify({ id, result })}\n`);
      child.stdin.write = vi.fn((line, callback) => {
        const packet = JSON.parse(line);
        callback?.();
        if (mcp) reply(packet.id, packet.method === 'tools/list' ? { tools: [] } : {});
        else if (packet.method === 'fixture-emit-marker') reply(packet.id, { pid: 9324, marker: 'm-win' });
        else if (packet.method === 'probe') {
          const pid = packet.args?.pid;
          if (pid === 9324) reply(packet.id, { pid: 9324, ppid: 16424, processStart: '9324:555', exited: false });
          else if (pid === 16424) reply(packet.id, { pid: 16424, processStart: '16424:111', exited: false });
          else reply(packet.id, { pid: 202, executable: 'C:\\isolated\\ferryx.exe', processStart: '202:200', exited: false });
        } else if (packet.method === 'prepare') reply(packet.id, setupReceipt);
        else if (packet.method === 'spawn-daemon') reply(packet.id, daemon);
        else if (packet.method === 'start-forwarder-proxy') reply(packet.id, { endpoint: { host: '127.0.0.1', port: 9998 } });
        else reply(packet.id, {});
        return true;
      });
      return child;
    };

    const ctx = await createConfiguredNativeContext({
      platform: 'win32',
      sshDestination: 'isolated-host', remoteAgent: 'C:\\isolated\\agent.mjs',
      worktree: 'C:\\isolated\\worktree', canonicalClone: 'C:\\isolated\\clone', worktreeRoot: 'C:\\isolated',
      mahordExecutable: 'mahord', mahordAddress: 'isolated-host', pairingId: 'isolated-pair',
      bun: 'C:\\isolated\\bun.exe', binary: 'C:\\isolated\\ferryx.exe', buildManifest: 'C:\\isolated\\manifest', hostId: 'host-1',
    }, {
      candidate: { stamp: 'candidate-1' }, transport: { subscribe: () => () => {}, release: vi.fn(), bindContext: vi.fn() },
      evidence: { artifact: vi.fn() }, runId: 'run-1', mutation: { id: 'op-1' }, signal: scenario.signal, spawnProcess,
    });

    await ctx.spawn.prepare({ recordOwnedPid: vi.fn(), signal: scenario.signal });
    const target = {
      pid: 16424,
      processStart: '16424:111',
      fixtureChild: { pid: 9324, ppid: 16424, processStart: '9324:555' },
    };
    const emitted = await ctx.transport.emitOutput({ target, marker: 'm-win', signal: scenario.signal });
    expect(emitted).toEqual({
      pid: 16424,
      processStart: '16424:111',
      marker: 'm-win',
      fixtureChildPid: 9324,
      fixtureChildProcessStart: '9324:555',
    });
  });
});
