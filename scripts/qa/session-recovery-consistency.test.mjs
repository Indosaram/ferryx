import { afterEach, expect, test, vi } from 'vitest';
import { EventEmitter } from 'node:events';
import { aggregateScenarioTeardown, requestedScenariosPassed, scenarioDurationMs, teardownOwnedChildren,
  createScenarioContext, dispatchScenario, parseArgs, SCENARIOS, SCENARIO_DISPATCH } from './session-recovery-consistency.mjs';
import { fileURLToPath } from 'node:url';

afterEach(() => vi.useRealTimers());

test('aggregates run10 native retained profile and cleanup failure despite an empty outer ledger', () => {
  const cleanup = { exited: false, action: 'retained-cleanup-error', code: 'ABORT_ERR', detail: 'The operation was aborted' };
  const result = { scenario: 'local-outage', status: 'failed', barrierCode: 'ABORT_ERR',
    cleanup: [cleanup], profileRetained: true, retainedProfile: 'C:\\isolated\\native-run',
    worktreeListError: 'The operation was aborted' };
  const aggregate = aggregateScenarioTeardown({ receipts: [], waitResults: [], allExited: true }, [result], []);
  expect(aggregate).toEqual({ retainedProfiles: [result.retainedProfile], teardown: {
    receipts: [], waitResults: [], allExited: false,
    nativeCleanup: [{ ...cleanup, scenario: result.scenario }],
    cleanupFailures: [{ ...cleanup, scenario: result.scenario }],
    worktreeListFailures: [{ scenario: result.scenario, detail: result.worktreeListError }],
  } });
  expect(result.barrierCode).toBe('ABORT_ERR');
});

test.each([
  { cleanup: [{ exited: false, action: 'retained-identity-mismatch', pid: 20 }] },
  { profileRetained: true },
  { worktreeListError: 'RPC unavailable' },
])('does not report native teardown success for $cleanup $profileRetained $worktreeListError', fields => {
  const result = { scenario: 'local-outage', status: 'passed', ...fields };
  expect(aggregateScenarioTeardown({ receipts: [], waitResults: [], allExited: true }, [result], []).teardown.allExited).toBe(false);
});

test('retains outer profiles and preserves successful native cleanup receipts', () => {
  const result = { scenario: 'stale-epoch', cleanup: [{ pid: 20, exited: true }], profileRetained: false };
  const outer = { receipts: [{ pid: 99 }], waitResults: [{ pid: 99, exited: false }], allExited: false };
  const aggregate = aggregateScenarioTeardown(outer, [result], ['/outer/profile']);
  expect(aggregate.retainedProfiles).toEqual(['/outer/profile']);
  expect(aggregate.teardown).toMatchObject({ ...outer, nativeCleanup: [{ pid: 20, exited: true, scenario: 'stale-epoch' }], cleanupFailures: [] });
  expect(aggregateScenarioTeardown({ ...outer, allExited: true }, [result], ['/outer/profile']).teardown.allExited).toBe(true);
});

test.each(['cli', 'fixturePath'])('loads an actual local transport module through %s options before real dispatch', async (source) => {
  const path = fileURLToPath(new URL('./fixtures/transport-loader-contract.mjs', import.meta.url));
  const options = parseArgs(['--scenario', 'ssh-ipc-failure', '--evidence-dir', '/fixture/evidence',
    ...(source === 'cli' ? ['--transport-fixture', path] : [])]);
  if (source === 'fixturePath') options.transportFixturePath = path;
  const ctx = createScenarioContext(options, { entries: [] });
  const result = await dispatchScenario(options.scenario, ctx);
  expect(ctx.transportLoaderCalls).toEqual(['ssh-ipc-failure']);
  expect(ctx.transportFixture).toEqual({ loaderContract: 'local-module-imported' });
  expect(result.status).toBe('unverified');
  expect(result.barrierCode).toBe('TRANSPORT_FIXTURE_REQUIRED');
  expect(requestedScenariosPassed([result], [options.scenario])).toBe(false);
});

test('preserves an injected transport object and typed module import refusal', async () => {
  const absent = fileURLToPath(new URL('./fixtures/transport-loader-does-not-exist.mjs', import.meta.url));
  const fixture = { loaderContract: 'injected' };
  const injected = createScenarioContext({ transportFixture: fixture, transportFixturePath: absent }, { entries: [] });
  const result = await dispatchScenario('ssh-ipc-failure', injected);
  expect(injected.transportFixture).toBe(fixture);
  expect(result.barrierCode).toBe('TRANSPORT_FIXTURE_REQUIRED');
  const missing = createScenarioContext({ transportFixture: absent }, { entries: [] });
  const refused = await dispatchScenario('ssh-ipc-failure', missing);
  expect(refused.status).toBe('unverified');
  expect(refused.barrierCode).toBe('TRANSPORT_FIXTURE_INVALID');
  expect(refused.detail).toEqual(expect.any(String));
});

test('passes only every requested scenario in order', () => {
  const requested = ['first', 'second'];
  const results = requested.map(scenario => ({ scenario, status: 'passed' }));
  expect(requestedScenariosPassed(results, requested)).toBe(true);
  for (const status of ['failed', 'blocked', 'incomplete', 'unverified', 'error', 'unknown', undefined]) {
    expect(requestedScenariosPassed([results[0], { scenario: 'second', status }], requested)).toBe(false);
  }
  expect(requestedScenariosPassed(results.slice(0, 1), requested)).toBe(false);
  expect(requestedScenariosPassed([...results].reverse(), requested)).toBe(false);
  expect(requestedScenariosPassed([...results, results[0]], requested)).toBe(false);
});

test('computes scenario duration from explicit monotonic endpoints', () => {
  expect(scenarioDurationMs(100, 31100)).toBe(31000);
  expect(scenarioDurationMs(100, 100.6)).toBe(1);
  expect(scenarioDurationMs(100, 99)).toBe(0);
});

test('dispatch accounts for every scenario and refuses absent native and transport fixtures', async () => {
  expect(Object.keys(SCENARIO_DISPATCH)).toEqual(SCENARIOS);
  const ctx = createScenarioContext({ timeoutMs: 1000 }, { entries: [] });
  for (const scenario of ['local-outage', 'stale-epoch', 'native-stream-ends', 'native-remote-mahord',
    'ssh-ipc-failure', 'paired-relay-loss', 'paired-backend-unavailable', 'host-down-descriptor-restore']) {
    const result = await SCENARIO_DISPATCH[scenario](ctx);
    expect(result.scenario).toBe(scenario);
    expect(result.status).toBe('unverified');
    expect(result.required).toEqual(expect.any(String));
    expect(requestedScenariosPassed([result], [scenario])).toBe(false);
  }
  expect((await dispatchScenario('unknown', ctx)).status).toBe('error');
});

test('web wrappers use deferred real factory and preserve missing fixture requirement', async () => {
  const ctx = createScenarioContext({ timeoutMs: 1000, evidenceDir: '/fixture/evidence' }, { entries: [] });
  expect(ctx.openWebOutcome).toEqual(expect.any(Function));
  for (const scenario of ['confirmed-exit', 'auth-revocation', 'cold-start-unknown',
    'close-rebind-pending-dial', 'output-replay-gap', 'no-focus-steal']) {
    const result = await SCENARIO_DISPATCH[scenario](ctx);
    expect(result.status).toBe('unverified');
    expect(result.barrierCode).toBe('TASK9_WEB_FIXTURE_UNVERIFIED');
    expect(result.required).toEqual(expect.any(String));
  }
});

function ownedFixture() {
  const child = Object.assign(new EventEmitter(), { pid: 12345, exitCode: null, signalCode: null });
  const entry = { pid: child.pid, executablePath: '/fixture/ferryx', extra: { profileRoot: '/fixture/profile' } };
  Object.defineProperty(entry, 'child', { value: child });
  // Preserve ledger callback failure capture without invoking its real OS identity probe.
  const ledger = {
    entries: [entry], receipts: [],
    teardown(kill) {
      try { kill(entry.pid); this.receipts.push({ pid: entry.pid, action: 'killed' }); }
      catch (error) { this.receipts.push({ pid: entry.pid, action: 'kill-failed', error: String(error) }); }
      return { entries: this.entries, receipts: this.receipts };
    },
  };
  const probeIdentity = vi.fn(() => ({ alive: true, executable: entry.executablePath }));
  const signalPid = vi.fn();
  return { child, entry, ledger, probeIdentity, signalPid };
}

test('subscribes before exact Unix signal and awaits observed exit', async () => {
  const f = ownedFixture();
  f.signalPid.mockImplementation((pid, signal) => {
    expect(pid).toBe(f.child.pid);
    expect(signal).toBe('SIGTERM');
    expect(f.child.listenerCount('exit')).toBe(1);
  });
  let settled = false;
  const pending = teardownOwnedChildren(f.ledger, { ...f, currentPlatform: 'linux' }).then(r => { settled = true; return r; });
  await Promise.resolve();
  expect(settled).toBe(false);
  f.child.signalCode = 'SIGTERM';
  f.child.emit('exit', null, 'SIGTERM');
  const result = await pending;
  expect(result.allExited).toBe(true);
  expect(f.probeIdentity).toHaveBeenCalledWith(f.child.pid);
  expect(f.child.listenerCount('exit')).toBe(0);
  expect(JSON.parse(JSON.stringify(result)).entries[0]).not.toHaveProperty('child');
});

test('Windows termination excludes unrecorded descendants', async () => {
  const f = ownedFixture();
  f.entry.executablePath = 'C:\\fixture\\ferryx.exe';
  const runTaskkill = vi.fn((command, args, options) => {
    expect(command).toBe('taskkill');
    expect(args).toEqual(['/PID', String(f.child.pid), '/F']);
    expect(options).toEqual({ encoding: 'utf8' });
    expect(f.child.listenerCount('exit')).toBe(1);
    f.child.exitCode = 1;
    f.child.emit('exit', 1, null);
    return { status: 0 };
  });
  expect((await teardownOwnedChildren(f.ledger, { ...f, currentPlatform: 'win32', runTaskkill })).allExited).toBe(true);
  expect(runTaskkill).toHaveBeenCalledTimes(1);
  expect(f.signalPid).not.toHaveBeenCalled();
});

test('identity mismatch never signals and timeout retains exit uncertainty', async () => {
  vi.useFakeTimers();
  const f = ownedFixture();
  f.probeIdentity.mockReturnValue({ alive: true, executable: '/foreign/program' });
  const pending = teardownOwnedChildren(f.ledger, { ...f, currentPlatform: 'linux' });
  await vi.advanceTimersByTimeAsync(5000);
  const result = await pending;
  expect(f.signalPid).not.toHaveBeenCalled();
  expect(result.allExited).toBe(false);
  expect(result.waitResults).toEqual([{ pid: f.child.pid, exited: false, reason: 'OWNED_CHILD_EXIT_TIMEOUT' }]);
  expect(f.child.listenerCount('exit')).toBe(0);
  f.child.exitCode = 0;
  f.child.emit('exit', 0, null);
  expect(result.allExited).toBe(false);
});

test('missing child handle cannot prove profile cleanup safety', async () => {
  const f = ownedFixture();
  f.ledger.entries = [{ pid: f.child.pid, executablePath: f.entry.executablePath }];
  const result = await teardownOwnedChildren(f.ledger, { ...f, currentPlatform: 'linux' });
  expect(result.allExited).toBe(false);
  expect(result.waitResults[0].reason).toBe('DIRECT_CHILD_EXIT_HANDLE_MISSING');
  expect(f.signalPid).not.toHaveBeenCalled();
});

test('already exited child refuses recycled same-executable PID before probe', async () => {
  const f = ownedFixture();
  f.child.exitCode = 0;
  const result = await teardownOwnedChildren(f.ledger, { ...f, currentPlatform: 'linux' });
  expect(result.allExited).toBe(true);
  expect(result.waitResults[0].code).toBe(0);
  expect(f.probeIdentity).not.toHaveBeenCalled();
  expect(f.signalPid).not.toHaveBeenCalled();
  expect(result.receipts[0].action).toBe('kill-failed');
});

test('parseArgs parses native, transport, and web fixture flags', async () => {
  const { parseArgs } = await import('./session-recovery-consistency.mjs');
  const parsed = parseArgs([
    '--evidence-dir', '/evidence/path',
    '--native-config', '/path/to/native.json',
    '--transport-fixture', '/path/to/transport.mjs',
    '--web-fixture', '/path/to/web.mjs',
  ]);
  expect(parsed.evidenceDir).toBe('/evidence/path');
  expect(parsed.nativeConfig).toBe('/path/to/native.json');
  expect(parsed.transportFixture).toBe('/path/to/transport.mjs');
  expect(parsed.webFixture).toBe('/path/to/web.mjs');
});

test('dispatchScenario isolates native configuration failure per scenario without aborting web outcomes', async () => {
  const ctx = createScenarioContext({
    timeoutMs: 1000,
    evidenceDir: '/fixture/evidence',
    nativeConfig: '/nonexistent/native/config.json',
  }, { entries: [] });

  const nativeResult = await dispatchScenario('local-outage', ctx);
  expect(nativeResult.scenario).toBe('local-outage');
  expect(nativeResult.status).toBe('unverified');
  expect(nativeResult.barrierCode).toBe('NATIVE_CONFIG_INVALID');

  const webResult = await dispatchScenario('confirmed-exit', ctx);
  expect(webResult.scenario).toBe('confirmed-exit');
  expect(webResult.status).toBe('unverified');
  expect(webResult.barrierCode).toBe('TASK9_WEB_FIXTURE_UNVERIFIED');
});

test('dispatchScenario with native config refuses missing candidate hashes', async () => {
  const { writeFileSync, unlinkSync } = await import('node:fs');
  const tempManifestPath = '/tmp/test-manifest-nohash.json';
  const tempConfigPath = '/tmp/test-native-config-nohash.json';
  writeFileSync(tempManifestPath, JSON.stringify({
    stamp: 'mock-stamp',
    // Missing binarySha256 and uiSha256
  }));
  writeFileSync(tempConfigPath, JSON.stringify({
    sshDestination: 'mock-host',
    remoteAgent: '/mock/agent.mjs',
    worktree: '/mock/wt',
    canonicalClone: '/mock/clone',
    worktreeRoot: '/mock/root',
    mahordExecutable: '/mock/mahord',
    mahordAddress: '127.0.0.1:9999',
    pairingId: 'mock-pair',
    bun: '/mock/bun',
    binary: '/mock/ferryx',
    buildManifest: tempManifestPath,
    hostId: 'mock-host',
  }));

  try {
    const ctx = createScenarioContext({
      timeoutMs: 1000,
      evidenceDir: '/fixture/evidence',
      nativeConfig: tempConfigPath,
      candidateStamp: 'mock-stamp',
    }, { entries: [] });

    const result = await dispatchScenario('local-outage', ctx);
    expect(result.scenario).toBe('local-outage');
    expect(result.status).toBe('unverified');
    expect(result.barrierCode).toBe('NATIVE_CONFIG_INVALID');
  } finally {
    try { unlinkSync(tempManifestPath); } catch (_) {}
    try { unlinkSync(tempConfigPath); } catch (_) {}
  }
});

test('dispatchScenario with native config refuses manifest mismatch', async () => {
  const { writeFileSync, unlinkSync } = await import('node:fs');
  const tempManifestPath = '/tmp/test-manifest-mismatch.json';
  const tempConfigPath = '/tmp/test-native-config-mismatch.json';
  writeFileSync(tempManifestPath, JSON.stringify({
    stamp: 'stamp-manifest-1234',
    binarySha256: 'bin-sha256-abcdef0123456789',
    uiSha256: 'ui-sha256-abcdef0123456789',
  }));
  writeFileSync(tempConfigPath, JSON.stringify({
    sshDestination: 'mock-host',
    remoteAgent: '/mock/agent.mjs',
    worktree: '/mock/wt',
    canonicalClone: '/mock/clone',
    worktreeRoot: '/mock/root',
    mahordExecutable: '/mock/mahord',
    mahordAddress: '127.0.0.1:9999',
    pairingId: 'mock-pair',
    bun: '/mock/bun',
    binary: '/mock/ferryx',
    buildManifest: tempManifestPath,
    candidate: {
      stamp: 'stamp-different-9999',
      binarySha256: 'bin-sha256-abcdef0123456789',
      uiSha256: 'ui-sha256-abcdef0123456789',
    },
    mutation: { id: 'm1', operation: 'write', payload: 'nonce' },
    hostId: 'mock-host',
  }));

  try {
    const ctx = createScenarioContext({
      timeoutMs: 1000,
      evidenceDir: '/fixture/evidence',
      nativeConfig: tempConfigPath,
    }, { entries: [] });

    const result = await dispatchScenario('local-outage', ctx);
    expect(result.scenario).toBe('local-outage');
    expect(result.status).toBe('unverified');
    expect(result.barrierCode).toBe('NATIVE_CONFIG_INVALID');
  } finally {
    try { unlinkSync(tempManifestPath); } catch (_) {}
    try { unlinkSync(tempConfigPath); } catch (_) {}
  }
});

test('dispatchScenario with complete native config reports unconfigured transport when no provider is supplied', async () => {
  const { writeFileSync, unlinkSync } = await import('node:fs');
  const tempManifestPath = '/tmp/test-manifest-complete.json';
  const tempConfigPath = '/tmp/test-native-config-complete.json';
  writeFileSync(tempManifestPath, JSON.stringify({
    stamp: 'stamp-1234567890abcdef',
    binarySha256: 'bin-sha256-abcdef0123456789',
    uiSha256: 'ui-sha256-abcdef0123456789',
  }));
  writeFileSync(tempConfigPath, JSON.stringify({
    sshDestination: 'mock-host',
    remoteAgent: '/mock/agent.mjs',
    worktree: '/mock/wt',
    canonicalClone: '/mock/clone',
    worktreeRoot: '/mock/root',
    mahordExecutable: '/mock/mahord',
    mahordAddress: '127.0.0.1:9999',
    pairingId: 'mock-pair',
    bun: '/mock/bun',
    binary: '/mock/ferryx',
    buildManifest: tempManifestPath,
    candidate: {
      stamp: 'stamp-1234567890abcdef',
      binarySha256: 'bin-sha256-abcdef0123456789',
      uiSha256: 'ui-sha256-abcdef0123456789',
    },
    mutation: { id: 'm1', operation: 'write', payload: 'nonce' },
    hostId: 'mock-host',
  }));

  try {
    const ctx = createScenarioContext({
      timeoutMs: 1000,
      evidenceDir: '/fixture/evidence',
      nativeConfig: tempConfigPath,
    }, { entries: [] });

    const result = await dispatchScenario('local-outage', ctx);
    expect(result.scenario).toBe('local-outage');
    expect(result.status).toBe('unverified');
    expect(result.barrierCode).toBe('NATIVE_CONFIG_INVALID');
    expect(result.detail).toContain('Existing isolated forwarding transport provider is not configured');
  } finally {
    try { unlinkSync(tempManifestPath); } catch (_) {}
    try { unlinkSync(tempConfigPath); } catch (_) {}
  }
});
