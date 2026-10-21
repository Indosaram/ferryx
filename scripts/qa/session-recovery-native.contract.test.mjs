import { expect, test, vi } from 'vitest';
import { runNativeRecoveryScenario } from './session-recovery-native.mjs';

function fixture() {
  let seq = 0;
  let receive;
  let recovered = false;
  const before = { backendSessionId: 'backend', incarnation: 'life', daemonEpoch: 'epoch1',
    frontendSessionId: 'frontend', paneIdentity: 'pane', bindingKey: 'binding', attemptGeneration: 1 };
  const after = { ...before, daemonEpoch: 'epoch2', attemptGeneration: 2 };
  const target = { pid: 20, processStart: 'start20', backendSessionId: 'backend', incarnation: 'life', daemonEpoch: 'epoch1' };
  const emit = event => receive({ ...event, seq: ++seq });
  const raw = { presented: true, producerPid: 10, attachTuple: after };
  const record = { raw, path: '/work/run/profile/presentation.receipt.jsonl', lineIndex: 6 };
  const ctx = {
    timeoutMs: 90000, runId: 'contract', launchOwnership: { gui: true, pty: true },
    mutation: { id: 'mutation', operation: 'write', payload: 'nonce' },
    candidate: { stamp: 'candidate', binarySha256: 'binary', uiSha256: 'ui' },
    ledger: { recordPid: (pid, executablePath, extra) => ({ pid, executablePath, extra }) },
    host: { id: 'host', remote: true, desktopTransport: 'mahord', platform: 'linux', worktreeRoot: '/work',
      probePid: vi.fn(async () => ({ exited: true })), subscribeExit: vi.fn(), terminateExact: vi.fn(),
      removeProfile: vi.fn(async () => {}), worktreeList: vi.fn(async () => []) },
    spawn: {
      prepare: vi.fn(async () => ({ candidate: 'candidate', binarySha256: 'binary', uiSha256: 'ui',
        productionUntouched: true, worktree: '/work/run', profile: '/work/run/profile', store: '/work/run/store', socket: '/work/run/socket' })),
      launch: vi.fn(async ({ recordOwnedPid }) => {
        for (const [pid, role] of [[10, 'gui'], [20, 'pty'], [30, 'daemon']]) recordOwnedPid({ pid, role, hostId: 'host',
          candidate: 'candidate', executable: '/fixture/app', processStart: `start${pid}` });
        emit({ kind: 'presentation', source: 'presentation.receipt.jsonl', receipt: { presented: true, attachTuple: before } });
      }),
    },
    provider: {
      subscribe: callback => { receive = callback; return () => { receive = undefined; }; },
      snapshot: async () => ({ attachTuple: recovered ? after : before,
        target: { ...target, daemonEpoch: recovered ? 'epoch2' : 'epoch1' }, focus: { paneIdentity: 'other' } }),
      capture: async ({ marker }) => ({ path: '/fixture/capture.png', sha256: 'capture', renderedText: marker, driver: 'mahord' }),
      sendUncertain: async ({ mutation }) => {
        emit({ kind: 'request', operation: 'write', mutationId: mutation.id, payload: mutation.payload });
        emit({ kind: 'delivery', operation: 'write', mutationId: mutation.id, payload: mutation.payload });
        return { outcome: 'unknown', mutationId: mutation.id };
      },
    },
    presentation: {
      cursor: async () => ({ path: record.path, afterLine: 5 }),
      awaitPresentation: vi.fn(async () => record),
    },
    transport: {
      beginAudit: async () => ({
        id: 'audit',
        complete: true,
        ingress: true,
        deliveries: true,
        scope: 'registered-isolated-owner-set',
        registeredOwners: [
          { pid: 10, executable: '/fixture/app', component: 'gui' },
          { pid: 20, executable: '/fixture/app', component: 'terminal-local-pty' },
          { pid: 30, executable: '/fixture/app', component: 'daemon' },
        ],
      }),
      endCurrentStream: vi.fn(), disconnect: vi.fn(), holdOutage: vi.fn(),
      changeOwnerEpoch: async () => { emit({ kind: 'authority', target: 'backend', daemonEpoch: 'epoch2' }); },
      restore: async () => {
        recovered = true;
        emit({ kind: 'status', outcome: 'retryable' });
        emit({ kind: 'binding-persisted', attachTuple: after });
        emit({ kind: 'listener', attachTuple: after, active: true });
        emit({ kind: 'attached', attachTuple: after });
        emit({ kind: 'status', attachTuple: after, outcome: 'ready' });
        emit({ kind: 'presentation', receipt: raw, source: 'presentation.receipt.jsonl', path: record.path, lineIndex: 6 });
      },
      emitOutput: async ({ marker }) => { emit({ kind: 'output', attachTuple: after, bytes: marker }); return target; },
      flush: async () => ({ complete: true, auditId: 'audit', throughSeq: seq }), release: vi.fn(async () => {}),
    },
  };
  return { ctx, record, before, after, emit };
}

test.each(['release', 'worktrees'])('bounds non-cooperative cleanup %s and preserves initiating failure', async stage => {
  vi.useFakeTimers();
  const scheduled = vi.spyOn(globalThis, 'setTimeout');
  try {
    const { ctx } = fixture();
    const entered = Promise.withResolvers();
    const initiating = Object.assign(new Error('initiating presentation failure'), { code: 'PRESENTATION_FAILED' });
    ctx.presentation.awaitPresentation = async () => { throw initiating; };
    let cleanupSignal;
    const stall = ({ signal }) => {
      cleanupSignal = signal;
      entered.resolve();
      return new Promise(() => {});
    };
    if (stage === 'release') ctx.transport.release.mockImplementation(stall);
    else ctx.host.worktreeList.mockImplementation(stall);
    const pending = runNativeRecoveryScenario('stale-epoch', ctx);
    await entered.promise;
    const index = scheduled.mock.calls.findIndex(([, ms]) => ms === 15_000);
    expect(index).toBeGreaterThanOrEqual(0);
    expect(scheduled.mock.results[index].value.hasRef()).toBe(true);
    await vi.advanceTimersByTimeAsync(15_000);
    const result = await pending;
    expect(result).toMatchObject({ status: 'failed', barrierCode: initiating.code, detail: initiating.message });
    expect(cleanupSignal.aborted).toBe(true);
    expect(cleanupSignal.reason.code).toBe('NATIVE_CLEANUP_TIMEOUT');
    if (stage === 'release') {
      expect(result.cleanup).toContainEqual(expect.objectContaining({ code: 'NATIVE_CLEANUP_TIMEOUT', exited: false }));
      expect(ctx.host.terminateExact).not.toHaveBeenCalled();
      expect(result.profileRetained).toBe(true);
    } else expect(result.worktreeListError).toBe(cleanupSignal.reason.message);
    expect(vi.getTimerCount()).toBe(0);
  } finally {
    scheduled.mockRestore();
    vi.useRealTimers();
  }
});

test('release and one PID failure do not skip other exact cleanup or replace the initiating failure', async () => {
  const { ctx } = fixture();
  const initiating = Object.assign(new Error('presentation failed'), { code: 'PRESENTATION_FAILED' });
  const releaseError = Object.assign(new Error('ownership refused'), { code: 'OWNERSHIP_REFUSED' });
  const pidError = Object.assign(new Error('one PID probe failed'), { code: 'PROBE_FAILED' });
  ctx.presentation.awaitPresentation = async () => { throw initiating; };
  ctx.transport.release.mockRejectedValue(releaseError);
  ctx.host.probePid.mockImplementation(async ({ pid }) => {
    if (pid === 30) throw pidError;
    return { exited: false, processStart: `start${pid}`, executable: '/fixture/app' };
  });
  ctx.host.subscribeExit.mockImplementation(async ({ pid, processStart }) => ({ pid, processStart, exited: true }));
  ctx.host.terminateExact.mockResolvedValue({});
  const result = await runNativeRecoveryScenario('stale-epoch', ctx);
  expect(result).toMatchObject({ status: 'failed', barrierCode: initiating.code, detail: initiating.message, profileRetained: true });
  expect(ctx.host.probePid.mock.calls.map(([args]) => args.pid)).toEqual([30, 20, 10]);
  expect(ctx.host.terminateExact.mock.calls.map(([args]) => args.pid)).toEqual([20, 10]);
  for (const code of [releaseError.code, pidError.code]) expect(result.cleanup).toContainEqual(expect.objectContaining({ code, exited: false }));
  expect(ctx.host.removeProfile).not.toHaveBeenCalled();
  expect(ctx.host.worktreeList).toHaveBeenCalledOnce();
});

test('external abort before launch performs zero spawn', async () => {
  const { ctx } = fixture();
  const controller = new AbortController(); controller.abort(); ctx.signal = controller.signal;
  const result = await runNativeRecoveryScenario('stale-epoch', ctx);
  expect(result).toMatchObject({ status: 'incomplete', barrierCode: 'NATIVE_CANCELLED', executed: false });
  expect(ctx.spawn.prepare).not.toHaveBeenCalled();
  expect(ctx.spawn.launch).not.toHaveBeenCalled();
});

test.each(['presentation', 'ownership'])('missing %s prerequisites refuse launch', async missing => {
  const { ctx } = fixture();
  if (missing === 'presentation') delete ctx.presentation;
  else delete ctx.launchOwnership;
  const result = await runNativeRecoveryScenario('stale-epoch', ctx);
  expect(result.status).toBe('incomplete');
  expect(result).toMatchObject({ executed: false,
    barrierCode: missing === 'presentation' ? 'NATIVE_ADAPTER_REQUIRED' : 'NATIVE_OWNERSHIP_REQUIRED' });
  expect(ctx.spawn.prepare).not.toHaveBeenCalled();
  expect(ctx.spawn.launch).not.toHaveBeenCalled();
});

test.each(['missing', 'empty', 'whitespace', 'same', 'distinct'])('requires an actual distinct baseline focus identity: %s', async mode => {
  const { ctx, before } = fixture();
  const changeOwnerEpoch = vi.spyOn(ctx.transport, 'changeOwnerEpoch');
  const snapshot = ctx.provider.snapshot;
  ctx.provider.snapshot = async (...args) => ({ ...await snapshot(...args), focus: {
    ...(mode === 'missing' ? {} : { paneIdentity: mode === 'empty' ? '' : mode === 'whitespace' ? '   '
      : mode === 'same' ? before.paneIdentity : 'other' }),
  } });
  const result = await runNativeRecoveryScenario('stale-epoch', ctx);
  if (mode === 'distinct') {
    expect(result.status).toBe('passed');
    expect(ctx.presentation.awaitPresentation).toHaveBeenCalledOnce();
  } else {
    expect(result).toMatchObject({ status: 'failed', barrierCode: 'ASSERTION_FAILURE' });
    expect(ctx.presentation.awaitPresentation).not.toHaveBeenCalled();
    expect(changeOwnerEpoch).not.toHaveBeenCalled();
  }
});

test('current raw seven tuple is awaited after physical cursor unchanged', async () => {
  const { ctx, record, after } = fixture();
  const result = await runNativeRecoveryScenario('stale-epoch', ctx);
  expect(result.status).toBe('passed');
  expect(result.presentation).toBe(record);
  expect(result.attachTuple).toEqual(after);
  expect(ctx.presentation.awaitPresentation).toHaveBeenCalledWith(expect.objectContaining({
    sessionId: 'backend', expected: after, afterLine: 5, producerPid: 10,
  }));
});

test('baseline skips a receipt-less frontend presentation event published before the product receipt', async () => {
  const { ctx, emit } = fixture();
  const launch = ctx.spawn.launch.getMockImplementation();
  // Producer order measured on maho-win (run38): the native-events.jsonl lifecycle
  // 'presentation' event (no receipt) lands before the presentation.receipt.jsonl one.
  ctx.spawn.launch.mockImplementation(async (args) => {
    emit({ kind: 'presentation', source: 'native-events.jsonl', producerComponent: 'native-frontend-lifecycle' });
    return launch(args);
  });
  const result = await runNativeRecoveryScenario('stale-epoch', ctx);
  expect(result.status).toBe('passed');
});

test.each(['old-line', 'stale-tuple'])('rejects %s rather than settling ready', async invalid => {
  const { ctx, record, before } = fixture();
  ctx.presentation.awaitPresentation = async () => invalid === 'old-line' ? { ...record, lineIndex: 5 } :
    { ...record, raw: { ...record.raw, attachTuple: before } };
  const result = await runNativeRecoveryScenario('stale-epoch', ctx);
  expect(result.status).toBe('failed');
  expect(result.barrierCode).toBe('ASSERTION_FAILURE');
});

test.each(['already-exited', 'owned-live', 'reused-pid', 'changed-executable'])(
  'external abort cancels pending presentation and performs cleanup: %s', async identity => {
  const { ctx } = fixture();
  const exit = Promise.withResolvers();
  if (identity !== 'already-exited') {
    ctx.host.probePid.mockImplementation(async ({ pid }) => (pid === 10 || pid === 30) ? { exited: true } : {
      processStart: identity === 'reused-pid' ? 'replacement' : 'start20',
      executable: identity === 'changed-executable' ? '/foreign/app' : '/fixture/app',
    });
    ctx.host.subscribeExit.mockReturnValue(exit.promise);
    ctx.host.terminateExact.mockImplementation(async ({ pid, processStart }) => {
      expect(ctx.host.subscribeExit).toHaveBeenCalledWith(expect.objectContaining({ pid, processStart }));
      exit.resolve({ pid, processStart, exited: true });
    });
  }
  const controller = new AbortController(); ctx.signal = controller.signal;
  const entered = Promise.withResolvers();
  let presentationSignal;
  ctx.presentation.awaitPresentation = ({ signal }) => new Promise((_, reject) => {
    presentationSignal = signal;
    signal.addEventListener('abort', () => reject(signal.reason), { once: true });
    entered.resolve();
  });
  const pending = runNativeRecoveryScenario('stale-epoch', ctx);
  await entered.promise;
  controller.abort();
  const result = await pending;
  expect(result).toMatchObject({ status: 'incomplete', barrierCode: 'NATIVE_CANCELLED' });
  expect(presentationSignal.aborted).toBe(true);
  expect(presentationSignal.reason.code).toBe('NATIVE_CANCELLED');
  expect(ctx.transport.release).toHaveBeenCalledOnce();
  expect(ctx.transport.release.mock.calls[0][0].signal.aborted).toBe(false);
  if (identity === 'already-exited' || identity === 'owned-live') {
    expect(ctx.host.removeProfile).toHaveBeenCalledOnce();
    expect(result.profileRetained).toBe(false);
  } else {
    expect(ctx.host.removeProfile).not.toHaveBeenCalled();
    expect(result).toMatchObject({ profileRetained: true, retainedProfile: '/work/run/profile' });
    expect(result.cleanup).toContainEqual(expect.objectContaining({ pid: 20, action: 'retained-identity-mismatch' }));
  }
  if (identity === 'owned-live') {
    expect(ctx.host.terminateExact).toHaveBeenCalledOnce();
    expect(ctx.host.terminateExact).toHaveBeenCalledWith(expect.objectContaining({
      pid: 20, processStart: 'start20', executable: '/fixture/app', tree: false,
    }));
    expect(result.cleanup).toContainEqual({ pid: 20, processStart: 'start20', exited: true, action: 'terminated-exact-pid' });
  } else {
    expect(ctx.host.subscribeExit).not.toHaveBeenCalled();
    expect(ctx.host.terminateExact).not.toHaveBeenCalled();
  }
});

test('incomplete audit keeps its native prerequisite code and cleans up', async () => {
  const { ctx } = fixture();
  ctx.transport.beginAudit = async () => ({ complete: false });
  const result = await runNativeRecoveryScenario('stale-epoch', ctx);
  expect(result).toMatchObject({ status: 'incomplete', barrierCode: 'NATIVE_AUDIT_REQUIRED', executed: true });
  expect(ctx.transport.release).toHaveBeenCalledOnce();
  expect(ctx.host.removeProfile).toHaveBeenCalledOnce();
  expect(ctx.presentation.awaitPresentation).not.toHaveBeenCalled();
});
