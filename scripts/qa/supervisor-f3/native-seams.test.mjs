// Native adapter seams use in-memory JSONL RPC; no desktop or subprocess.
import { describe, expect, test } from 'bun:test';
import { EventEmitter } from 'node:events';
import { PassThrough, Writable } from 'node:stream';
import { join } from 'node:path';
import { connectNativeMahord, parseScreenChangeFrame, rpcProcess } from '../fixtures/native-configured-adapters.mjs';
import { launchInteractiveGuiTask, stopSetupProbeWorker, tolerableSetupProbeReadError } from '../fixtures/native-remote-agent.mjs';

const frame = (id) => ({ content: [{ type: 'text', text: `Screen changed: frame_id=${id}, timestamp_ms=123, age_ms=0` }] });
const timeout = { content: [{ type: 'text', text: 'Timeout: no change detected (last_frame_id=17)' }] };
const toolNames = ['remote_wait_for_screen_change', 'remote_mouse_move', 'remote_mouse_scroll'];

function wirePeer(handle) {
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  child.stdin = new Writable({ write(bytes, encoding, done) {
    const packet = JSON.parse(bytes.toString());
    try {
      const response = handle(packet);
      queueMicrotask(() => child.stdout.write(`${JSON.stringify({ id: packet.id, ...response })}\n`));
      done();
    } catch (error) { done(error); }
  } });
  return child;
}

async function mahordPeer(onTool, options = {}) {
  let spawned;
  const signal = new AbortController().signal;
  const adapter = await connectNativeMahord({ executable: 'mahord-fixture', address: 'fixture:7777', pairingId: 'pair', signal,
    ...options, spawnProcess: (executable, argv) => {
      spawned = { executable, argv };
      return wirePeer((packet) => {
        if (packet.method === 'initialize') return { result: {} };
        if (packet.method === 'tools/list') return { result: { tools: toolNames.map((name) => ({ name })) } };
        if (packet.method !== 'tools/call') throw new Error(`Unexpected RPC ${packet.method}`);
        return { result: onTool(packet.params.name, packet.params.arguments) };
      });
    } });
  return { adapter, spawned, signal };
}

describe('native frame observations', () => {
  test('parses only an actual advancing screen-change frame', () => {
    expect(parseScreenChangeFrame(frame(18), 17)).toBe(18);
    for (const invalid of ['-1', 'NaN', '9007199254740992', '17junk']) {
      expect(() => parseScreenChangeFrame(frame(invalid))).toThrow(expect.objectContaining({ code: 'INVALID_FRAME' }));
    }
    for (const id of [16, 17]) {
      expect(() => parseScreenChangeFrame(frame(id), 17)).toThrow(expect.objectContaining({ code: 'INVALID_FRAME' }));
    }
    expect(() => parseScreenChangeFrame(timeout)).toThrow(expect.objectContaining({ code: 'FRAME_TIMEOUT' }));
  });

  test('waitForFrame requests and returns the later recorded frame', async () => {
    const requests = [];
    const { adapter, signal } = await mahordPeer((name, args) => {
      requests.push(args);
      return frame(args.last_frame_id === undefined ? 17 : 23);
    });
    try {
      expect(await adapter.waitForFrame(signal)).toBe(23);
      expect(requests).toEqual([{ timeout_ms: 5000 }, { last_frame_id: 17, timeout_ms: 5000 }]);
    } finally { adapter.close(); }
  });

  for (const stage of ['initial', 'later']) {
    test(`waitForFrame rejects a plain-text ${stage} timeout`, async () => {
      const { adapter, signal } = await mahordPeer((name, args) => stage === 'initial' || args.last_frame_id !== undefined ? timeout : frame(17));
      try { await expect(adapter.waitForFrame(signal)).rejects.toMatchObject({ code: 'FRAME_TIMEOUT' }); }
      finally { adapter.close(); }
    });
  }

  test('waitForFrame rejects a nonadvancing second RPC frame', async () => {
    const { adapter, signal } = await mahordPeer(() => frame(17));
    try { await expect(adapter.waitForFrame(signal)).rejects.toMatchObject({ code: 'INVALID_FRAME' }); }
    finally { adapter.close(); }
  });
});

describe('native input transport', () => {
  test('wheel delivery uses the moved cursor and retains explicit video arguments', async () => {
    let cursor = { x: 0, y: 0 };
    let delivered;
    const { adapter, spawned, signal } = await mahordPeer((name, args) => {
      if (name === 'remote_mouse_move') cursor = { x: args.x, y: args.y, normalized: args.normalized };
      if (name === 'remote_mouse_scroll') delivered = { cursor, dy: args.dy };
      return { content: [] };
    }, { display: 0, codec: 'h264', nudgeMs: 50 });
    try {
      await adapter.scroll({ x: 0.4, y: 0.7, normalized: true, dy: 520 }, signal);
      expect(delivered).toEqual({ cursor: { x: 0.4, y: 0.7, normalized: true }, dy: 520 });
      expect(spawned.argv).toEqual(['--host', 'fixture:7777', '--pairing-id', 'pair', '--nudge-ms', '50', '--display', '0', '--codec', 'h264', '--mcp']);
    } finally { adapter.close(); }
  });

  test('unspecified video options add no overrides', async () => {
    const { adapter, spawned } = await mahordPeer(() => ({ content: [] }));
    try { expect(spawned.argv).toEqual(['--host', 'fixture:7777', '--pairing-id', 'pair', '--mcp']); }
    finally { adapter.close(); }
  });

  test('RPC failures retain the persisted task receipt for the cleanup ledger', async () => {
    const receipt = { created: true, deleted: false, taskName: 'retained-task' };
    const rpc = rpcProcess('fixture', [], { signal: new AbortController().signal,
      spawnProcess: () => wirePeer(() => ({ error: { code: 'RUN_FAILED', message: 'run failed', receipt } })) });
    try { await expect(rpc.call('launch-gui-interactive', {})).rejects.toMatchObject({ code: 'RUN_FAILED', receipt }); }
    finally { rpc.close(); }
  });
});

describe('setup worker cleanup', () => {
  const profile = '/fixture/profile';
  const worker = { owner: { pid: 123, processStart: '456' }, completionPath: '/fixture/completion.json',
    wrapperMarkerPath: '/fixture/wrapper-exit.marker', deleteArgs: ['/delete', '/tn', 'owned-task', '/f'] };

  for (const first of ['process', 'wrapper']) {
    test(`requires both exit proofs when ${first} finishes first`, async () => {
      const processExit = Promise.withResolvers();
      const wrapperExit = Promise.withResolvers();
      const requested = Promise.withResolvers();
      const proofs = new Set();
      let deleted = false;
      let closed = false;
      const operation = stopSetupProbeWorker({ profile, worker, signal: AbortSignal.timeout(1000) }, {
        subscribe: () => ({ promise: wrapperExit.promise, close: () => { closed = true; } }),
        waitExit: (pid, start) => {
          expect({ pid, start }).toEqual({ pid: 123, start: '456' });
          return processExit.promise;
        },
        persist: async () => { requested.resolve(); },
        invoke: async () => {
          expect([...proofs].sort()).toEqual(['process', 'wrapper']);
          deleted = true;
        },
      });
      await requested.promise;
      const finish = (name) => {
        proofs.add(name);
        (name === 'process' ? processExit : wrapperExit).resolve();
      };
      finish(first);
      // Flush the first proof's continuations, not a wall-clock delay.
      await Promise.resolve();
      await Promise.resolve();
      expect(deleted).toBe(false);
      finish(first === 'process' ? 'wrapper' : 'process');
      await operation;
      expect(deleted).toBe(true);
      expect(closed).toBe(true);
    });
  }

  for (const failure of ['process', 'wrapper', 'delete']) {
    test(`preserves cleanup failure from ${failure}`, async () => {
      const error = new Error(`${failure} failed`);
      let deleted = false;
      let closed = false;
      await expect(stopSetupProbeWorker({ profile, worker, signal: AbortSignal.timeout(1000) }, {
        subscribe: () => ({ promise: failure === 'wrapper' ? Promise.reject(error) : Promise.resolve(), close: () => { closed = true; } }),
        waitExit: async () => { if (failure === 'process') throw error; },
        persist: async () => {},
        invoke: async () => { deleted = true; throw error; },
      })).rejects.toBe(error);
      expect(deleted).toBe(failure === 'delete');
      expect(closed).toBe(true);
    });
  }
});

describe('interactive task lifecycle', () => {
  for (const failures of [[], ['whoami'], ['/create'], ['/run'], ['/delete'], ['/run', '/delete']]) {
    test(`persists receipt when ${failures.join(' and ') || 'all operations succeed'}`, async () => {
      const saved = [];
      const operations = [];
      const launchError = Object.assign(new Error('launch failed'), { code: 'LAUNCH_FAILED' });
      const deleteError = Object.assign(new Error('delete failed'), { code: 'DELETE_FAILED' });
      const operation = launchInteractiveGuiTask({ profile: '/fixture/profile', signal: new AbortController().signal }, {
        invoke: async (executable, argv) => {
          const action = executable === 'whoami' ? 'whoami' : argv[0];
          operations.push(action);
          if (failures.includes(action)) throw action === '/delete' ? deleteError : launchError;
          return 'fixture-user\n';
        }, persist: async (path, receipt) => saved.push({ path, receipt: structuredClone(receipt) }),
      });
      let receipt;
      if (failures.some((action) => action !== '/delete')) {
        await expect(operation).rejects.toBe(launchError);
        receipt = launchError.receipt;
      } else receipt = await operation;
      const created = !failures.includes('whoami') && !failures.includes('/create');
      expect(receipt).toMatchObject({ created, started: created && !failures.includes('/run'), deleted: created && !failures.includes('/delete') });
      expect(saved).toEqual([{ path: join('/fixture/profile', 'evidence', 'gui-interactive-task.json'), receipt }]);
      expect(operations.includes('/delete')).toBe(created);
      if (failures.includes('/delete')) expect(receipt.deleteError).toEqual({ code: 'DELETE_FAILED', message: 'delete failed' });
    });
  }

  test('persistence failure preserves the launch failure and receipt', async () => {
    const launchError = new Error('create failed');
    const persistenceError = new Error('disk full');
    const operation = launchInteractiveGuiTask({ profile: '/fixture/profile', signal: new AbortController().signal }, {
      invoke: async (executable) => { if (executable === 'whoami') return 'fixture-user'; throw launchError; },
      persist: async () => { throw persistenceError; },
    });
    await expect(operation).rejects.toMatchObject({ errors: [launchError, persistenceError], receipt: { created: false, deleted: false } });
  });

  test('probe completion tolerates only missing files and partial JSON', () => {
    expect(tolerableSetupProbeReadError(new SyntaxError('partial JSON'))).toBe(true);
    expect(tolerableSetupProbeReadError(Object.assign(new Error('missing'), { code: 'ENOENT' }))).toBe(true);
    expect(tolerableSetupProbeReadError(Object.assign(new Error('denied'), { code: 'EACCES' }))).toBe(false);
    expect(tolerableSetupProbeReadError(new Error('SyntaxError'))).toBe(false);
  });
});
