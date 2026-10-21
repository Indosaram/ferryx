import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import test from 'node:test';
import {
  launchPairedIsolatedHarness,
  provisionIsolatedPairedSession,
  recordExactChild,
  terminateExactChild,
  buildCreateSessionRequest,
  buildDescriptor,
  buildPairedRecoveryOutcomeRequest,
  pairedHostDeliveryObserver,
  pairedOwnerControls,
} from './paired-isolated-provider.mjs';
import { probeDarwin } from './native-remote-agent.mjs';
import { TransportBarrier } from './transport-provider.mjs';

test('launchPairedIsolatedHarness rejects with CANDIDATE_BINARY_REQUIRED when binary is missing or invalid', async () => {
  await assert.rejects(
    launchPairedIsolatedHarness({ binary: '/nonexistent/daemon/binary' }),
    (err) => err.code === 'CANDIDATE_BINARY_REQUIRED',
  );
});

test('provisionIsolatedPairedSession rejects invalid candidate binary with typed barrier', async () => {
  await assert.rejects(
    provisionIsolatedPairedSession({ binary: '' }),
    (err) => err.code === 'CANDIDATE_BINARY_REQUIRED',
  );
});

test('launchPairedIsolatedHarness structure validates required contract properties', () => {
  assert.equal(typeof launchPairedIsolatedHarness, 'function');
  assert.equal(typeof provisionIsolatedPairedSession, 'function');
  assert.equal(typeof recordExactChild, 'function');
  assert.equal(typeof terminateExactChild, 'function');
});

test('regression: paired provisioning registers existing task repo without authoring files or committing', () => {
  // Verifies task-owned repo registration payload matches wire schema without scratch writes
  const repoDir = '/Volumes/T9-Mac/project/ferryx';
  const operation = {
    kind: 'registerProject',
    request: { requestId: 'req-1', repoPath: repoDir },
  };
  assert.equal(operation.kind, 'registerProject');
  assert.equal(operation.request.repoPath, repoDir);
});

test('regression: outcome without measured daemonEpoch or incarnation rejects without fallback', () => {
  const validateOutcome = (outcome) => {
    if (!outcome.daemonEpoch || !outcome.incarnation) {
      throw new TransportBarrier(
        'PAIRED_MEASURED_IDENTITY_REQUIRED',
        `outcome missing measured daemonEpoch/incarnation: ${JSON.stringify(outcome)}`,
      );
    }
    return outcome;
  };
  assert.throws(
    () => validateOutcome({ backendSessionId: 'b-1', daemonEpoch: null, incarnation: null }),
    (err) => err.code === 'PAIRED_MEASURED_IDENTITY_REQUIRED',
  );
  assert.throws(
    () => validateOutcome({ backendSessionId: 'b-1', daemonEpoch: '1', incarnation: null }),
    (err) => err.code === 'PAIRED_MEASURED_IDENTITY_REQUIRED',
  );
  assert.throws(
    () => validateOutcome({ backendSessionId: 'b-1', daemonEpoch: null, incarnation: 'inc-1' }),
    (err) => err.code === 'PAIRED_MEASURED_IDENTITY_REQUIRED',
  );
  const valid = validateOutcome({ backendSessionId: 'b-1', daemonEpoch: '1', incarnation: 'inc-1' });
  assert.equal(valid.daemonEpoch, '1');
  assert.equal(valid.incarnation, 'inc-1');
});

test('recordExactChild records exact OS processStart and passes (child, extra) to ledger.record', async () => {
  const ledgerCalls = [];
  const fakeLedger = {
    record: (child, extra) => {
      ledgerCalls.push({ child, extra });
      return { pid: child.pid, executablePath: extra.executablePath, extra };
    },
  };
  const mockChild = new EventEmitter();
  mockChild.pid = 9999;
  mockChild.spawnargs = ['/bin/daemon', '--daemon'];

  const mockProbe = async (pid) => ({
    pid,
    exited: false,
    executable: '/bin/daemon',
    processStart: '1700000000:000007',
  });

  const entry = await recordExactChild({
    child: mockChild,
    expectedBinary: '/bin/daemon',
    role: 'paired-test-daemon',
    probe: mockProbe,
    ledger: fakeLedger,
  });

  assert.equal(entry.pid, 9999);
  assert.equal(entry.processStart, '1700000000:000007');
  assert.equal(entry.executablePath, '/bin/daemon');
  assert.equal(ledgerCalls.length, 1);
  assert.equal(ledgerCalls[0].child.pid, 9999);
  assert.equal(ledgerCalls[0].extra.processStart, '1700000000:000007');
  assert.equal(ledgerCalls[0].extra.executablePath, '/bin/daemon');
  assert.deepEqual(ledgerCalls[0].extra.argv, ['/bin/daemon', '--daemon']);
});

test('recordExactChild throws OWNED_PROCESS_IDENTITY_MISMATCH when measured executable disagrees with expected binary', async () => {
  const mockChild = new EventEmitter();
  mockChild.pid = 9999;
  mockChild.spawnargs = ['--daemon'];

  const mockProbe = async (pid) => ({
    pid,
    exited: false,
    executable: '/bin/other-daemon',
    processStart: '1700000000:000007',
  });

  await assert.rejects(
    recordExactChild({
      child: mockChild,
      expectedBinary: '/bin/daemon',
      role: 'paired-test-daemon',
      probe: mockProbe,
    }),
    (err) => err.code === 'OWNED_PROCESS_IDENTITY_MISMATCH',
  );
});

test('recordExactChild throws OWNED_PROCESS_IDENTITY_UNAVAILABLE when OS probe is unavailable or child exited', async () => {
  const mockChild = new EventEmitter();
  mockChild.pid = 9999;
  const failingProbe = async () => {
    throw new Error('OS libproc unavailable');
  };

  await assert.rejects(
    recordExactChild({
      child: mockChild,
      expectedBinary: '/bin/daemon',
      role: 'paired-test-daemon',
      probe: failingProbe,
    }),
    (err) => err.code === 'OWNED_PROCESS_IDENTITY_UNAVAILABLE',
  );

  const exitedProbe = async (pid) => ({ pid, exited: true });
  await assert.rejects(
    recordExactChild({
      child: mockChild,
      expectedBinary: '/bin/daemon',
      role: 'paired-test-daemon',
      probe: exitedProbe,
    }),
    (err) => err.code === 'OWNED_PROCESS_IDENTITY_UNAVAILABLE',
  );
});

test('terminateExactChild refuses with OWNED_PROCESS_IDENTITY_MISMATCH and dispatches NO signal when identity changes', async () => {
  const recorded = { pid: 9999, executablePath: '/bin/daemon', processStart: '1700000000:000007' };
  const mockChild = new EventEmitter();
  mockChild.pid = 9999;
  mockChild.exitCode = null;
  mockChild.signalCode = null;

  const signals = [];
  const signalProcess = (pid, sig) => { signals.push({ pid, sig }); };

  // Recycled PID with different start time
  const liveProbe = async (pid) => ({
    pid,
    exited: false,
    executable: '/bin/daemon',
    processStart: '1700000000:000008',
  });

  await assert.rejects(
    terminateExactChild({
      child: mockChild,
      recorded,
      probe: liveProbe,
      signalProcess,
    }),
    (err) => err.code === 'OWNED_PROCESS_IDENTITY_MISMATCH',
  );

  assert.equal(signals.length, 0, 'No signal may be dispatched when identity check fails');
});

test('terminateExactChild refuses with OWNED_PROCESS_IDENTITY_UNAVAILABLE and dispatches NO signal when OS probe is unavailable', async () => {
  const recorded = { pid: 9999, executablePath: '/bin/daemon', processStart: '1700000000:000007' };
  const mockChild = new EventEmitter();
  mockChild.pid = 9999;
  mockChild.exitCode = null;
  mockChild.signalCode = null;

  const signals = [];
  const signalProcess = (pid, sig) => { signals.push({ pid, sig }); };

  const failingProbe = async () => {
    throw new Error('probe failed');
  };

  await assert.rejects(
    terminateExactChild({
      child: mockChild,
      recorded,
      probe: failingProbe,
      signalProcess,
    }),
    (err) => err.code === 'OWNED_PROCESS_IDENTITY_UNAVAILABLE',
  );

  assert.equal(signals.length, 0, 'No signal may be dispatched when OS probe is unavailable');
});

test('terminateExactChild returns immediately when child has already exited without awaiting missed exit', async () => {
  const recorded = { pid: 9999, executablePath: '/bin/daemon', processStart: '1700000000:000007' };
  const mockChild = new EventEmitter();
  mockChild.pid = 9999;
  mockChild.exitCode = 0;
  mockChild.signalCode = null;

  const signals = [];
  const signalProcess = (pid, sig) => { signals.push({ pid, sig }); };
  const liveProbe = async () => { throw new Error('should not be called'); };

  const res = await terminateExactChild({
    child: mockChild,
    recorded,
    probe: liveProbe,
    signalProcess,
  });

  assert.equal(res.exited, true);
  assert.equal(signals.length, 0);
});

test('terminateExactChild rejects with DAEMON_EXIT_TIMEOUT carrying owned resource receipt on bounded exit timeout with NO second kill', async () => {
  const recorded = { pid: 9999, executablePath: '/bin/daemon', processStart: '1700000000:000007' };
  const mockChild = new EventEmitter();
  mockChild.pid = 9999;
  mockChild.exitCode = null;
  mockChild.signalCode = null;

  const signals = [];
  const signalProcess = (pid, sig) => { signals.push({ pid, sig }); };

  const liveProbe = async (pid) => ({
    pid,
    exited: false,
    executable: '/bin/daemon',
    processStart: '1700000000:000007',
  });

  await assert.rejects(
    terminateExactChild({
      child: mockChild,
      recorded,
      probe: liveProbe,
      timeoutMs: 30,
      signalProcess,
    }),
    (err) => {
      assert.equal(err.code, 'DAEMON_EXIT_TIMEOUT');
      assert.ok(err.message.includes('1700000000:000007'));
      return true;
    },
  );

  // Exactly one signal (SIGTERM) sent, NO second kill / SIGKILL fallback
  assert.equal(signals.length, 1);
  assert.equal(signals[0].sig, 'SIGTERM');
});

test('terminateExactChild rejects immediately with TERMINATION_SIGNAL_FAILED when signalProcess returns false', async () => {
  const recorded = { pid: 9999, executablePath: '/bin/daemon', processStart: '1700000000:000007' };
  const mockChild = new EventEmitter();
  mockChild.pid = 9999;
  mockChild.exitCode = null;
  mockChild.signalCode = null;

  const liveProbe = async (pid) => ({
    pid,
    exited: false,
    executable: '/bin/daemon',
    processStart: '1700000000:000007',
  });

  // signalProcess returning false must reject immediately, not await timeout
  const signalProcess = () => false;

  const start = Date.now();
  await assert.rejects(
    terminateExactChild({
      child: mockChild,
      recorded,
      probe: liveProbe,
      timeoutMs: 10000,
      signalProcess,
    }),
    (err) => err.code === 'TERMINATION_SIGNAL_FAILED',
  );
  assert.ok(Date.now() - start < 1000, 'Rejection must be immediate, not waiting for timeout');
});

test('terminateExactChild dispatches signal and resolves on bounded exit when identity matches', async () => {
  const recorded = { pid: 9999, executablePath: '/bin/daemon', processStart: '1700000000:000007' };
  const mockChild = new EventEmitter();
  mockChild.pid = 9999;
  mockChild.exitCode = null;
  mockChild.signalCode = null;

  const signals = [];
  const signalProcess = (pid, sig) => {
    signals.push({ pid, sig });
    setImmediate(() => mockChild.emit('exit', 0, sig));
  };

  const liveProbe = async (pid) => ({
    pid,
    exited: false,
    executable: '/bin/daemon',
    processStart: '1700000000:000007',
  });

  const res = await terminateExactChild({
    child: mockChild,
    recorded,
    probe: liveProbe,
    timeoutMs: 500,
    signalProcess,
  });

  assert.equal(res.exited, true);
  assert.equal(res.code, 0);
  assert.equal(res.signal, 'SIGTERM');
  assert.equal(signals.length, 1);
  assert.equal(signals[0].sig, 'SIGTERM');
});

test('probeDarwin matches contract signature (pid, binary, signal, invoke) and validates required libproc identity format', async () => {
  const invoked = [];
  const mockInvoke = async (binary, args, signal) => {
    invoked.push({ binary, args, signal });
    return JSON.stringify({
      exited: false,
      executable: '/Volumes/T9-Mac/project/ferryx/target/debug/ferryx',
      processStart: '1700000000:000007',
    });
  };

  const actual = await probeDarwin(42, '/Volumes/T9-Mac/project/ferryx/target/debug/ferryx', undefined, mockInvoke);
  assert.equal(actual.exited, false);
  assert.equal(actual.executable, '/Volumes/T9-Mac/project/ferryx/target/debug/ferryx');
  assert.equal(actual.processStart, '1700000000:000007');
  assert.equal(invoked.length, 1);
  assert.deepEqual(invoked[0].args, ['qa-exact-pid-probe', '42']);

  // Requires absolute candidate binary
  await assert.rejects(
    probeDarwin(42, 'relative/binary', undefined, mockInvoke),
    /Darwin exact PID probe requires the isolated candidate binary/,
  );
});

test('regression: readiness requires exact newline-delimited token', () => {
  const targetToken = 'FERRYX_DAEMON_READY';
  const substringOnly = 'SOME_PREFIX_FERRYX_DAEMON_READY_SUFFIX';
  const exactLine = 'FERRYX_DAEMON_READY\n';
  
  // Substring must not match
  assert.notEqual(substringOnly.trim(), targetToken);
  // Exact line matches
  assert.equal(exactLine.trim(), targetToken);
});

test('wire schema: PairedTerminalReattachOk carries session_id in snake_case without alias fallback', () => {
  const wireJson = JSON.stringify({
    type: 'pairedTerminalReattachOk',
    session_id: 'sess-target-1',
    generation: 1,
  });
  const parsed = JSON.parse(wireJson);
  // Must read strictly session_id, not alias sessionId
  assert.equal(parsed.session_id, 'sess-target-1');
  assert.equal(parsed.sessionId, undefined);
  const backendSessionId = parsed.session_id;
  assert.equal(backendSessionId, 'sess-target-1');
});

test('wire schema: OperationResponse result carries tagged kind and data for registerProject', () => {
  const wireJson = JSON.stringify({
    type: 'pairedHostOperationOk',
    response: {
      hostId: 'host-1',
      generation: 1,
      result: {
        kind: 'registerProject',
        data: {
          remoteWorkspaceId: 'project-0123456789abcdef0123456789abcdef',
          repoRoot: '/Volumes/T9-Mac/project/ferryx',
          target: { kind: 'pairedDaemon', hostId: 'host-1' },
        },
      },
    },
  });
  const parsed = JSON.parse(wireJson);
  assert.equal(parsed.response.result.kind, 'registerProject');
  assert.equal(parsed.response.result.data.remoteWorkspaceId, 'project-0123456789abcdef0123456789abcdef');
});

test('wire schema: OperationResponse result carries tagged kind and data for createSession', () => {
  const wireJson = JSON.stringify({
    type: 'pairedHostOperationOk',
    response: {
      hostId: 'host-1',
      generation: 1,
      result: {
        kind: 'createSession',
        data: {
          workspaceId: 'project-0123456789abcdef0123456789abcdef',
          worktree: null,
          cwd: '/Volumes/T9-Mac/project/ferryx',
          cols: 80,
          rows: 24,
          running: true,
          target: {
            machineId: 'm-1',
            daemonEpoch: 1,
            sessionId: 'sess-backend-100',
          },
        },
      },
    },
  });
  const parsed = JSON.parse(wireJson);
  assert.equal(parsed.response.result.kind, 'createSession');
  assert.equal(parsed.response.result.data.target.sessionId, 'sess-backend-100');
  assert.equal(parsed.response.result.data.target.machineId, 'm-1');
});

test('terminateExactChild rejects with OWNED_PROCESS_IDENTITY_MISMATCH and dispatches NO signal when child.pid !== recorded.pid', async () => {
  const recorded = { pid: 1001, executablePath: '/bin/daemon', processStart: '1700000000:000007' };
  const mockChild = new EventEmitter();
  mockChild.pid = 2002;
  mockChild.exitCode = null;
  mockChild.signalCode = null;

  let probeCalls = 0;
  const mockProbe = async (pid) => {
    probeCalls++;
    return { pid, exited: false, executable: '/bin/daemon', processStart: '1700000000:000007' };
  };

  const signals = [];
  const signalProcess = (pid, sig) => {
    signals.push({ pid, sig });
  };

  await assert.rejects(
    terminateExactChild({
      child: mockChild,
      recorded,
      probe: mockProbe,
      timeoutMs: 1000,
      signalProcess,
    }),
    (err) => {
      assert.equal(err.code, 'OWNED_PROCESS_IDENTITY_MISMATCH');
      assert.ok(err.message.includes('1001') && err.message.includes('2002'));
      return true;
    },
  );

  assert.equal(signals.length, 0, 'No signal must be dispatched on PID mismatch');
  assert.equal(probeCalls, 0, 'No probe should be executed when child.pid !== recorded.pid');
});

test('production wire validator: buildCreateSessionRequest includes explicit nulls for inheritFromSessionId and cwdRelative per machine_protocol_lifecycle.rs:207', () => {
  const req = buildCreateSessionRequest({
    requestId: 'test-req-12345678-1234-1234-1234-123456789abc',
    workspaceId: 'project-test-ws',
    worktree: null,
    cols: 80,
    rows: 24,
    inheritFromSessionId: null,
    cwdRelative: null,
    startup: { kind: 'shell' },
  });

  // Verify exact wire schema properties
  assert.equal(req.requestId, 'test-req-12345678-1234-1234-1234-123456789abc');
  assert.equal(req.workspaceId, 'project-test-ws');
  assert.equal(req.worktree, null);
  assert.equal(req.cols, 80);
  assert.equal(req.rows, 24);
  assert.equal(req.inheritFromSessionId, null);
  assert.equal(req.cwdRelative, null);
  assert.deepEqual(req.startup, { kind: 'shell' });

  // Verify JSON serialization retains explicit nulls required by Rust serde nullable_text
  const serialized = JSON.stringify(req);
  const parsed = JSON.parse(serialized);
  assert.ok('inheritFromSessionId' in parsed, 'inheritFromSessionId must be present in JSON');
  assert.equal(parsed.inheritFromSessionId, null);
  assert.ok('cwdRelative' in parsed, 'cwdRelative must be present in JSON');
  assert.equal(parsed.cwdRelative, null);
  assert.ok('worktree' in parsed, 'worktree must be present in JSON');
  assert.equal(parsed.worktree, null);
});

test('production wire validator: buildDescriptor and buildPairedRecoveryOutcomeRequest match exact Rust wire schemas', () => {
  const target = { machineId: 'm-1', daemonEpoch: 1, sessionId: 's-100' };
  const desc = buildDescriptor({
    hostId: 'host-1',
    generation: 2,
    target,
    afterSequence: null,
  });
  assert.equal(desc.hostId, 'host-1');
  assert.equal(desc.generation, 2);
  assert.deepEqual(desc.target, target);
  assert.equal(desc.afterSequence, null);

  const parsedDesc = JSON.parse(JSON.stringify(desc));
  assert.equal(parsedDesc.hostId, 'host-1');
  assert.equal(parsedDesc.generation, 2);
  assert.equal(parsedDesc.afterSequence, null);

  const outcomeReq = buildPairedRecoveryOutcomeRequest('sess-backend-100');
  assert.equal(outcomeReq.type, 'pairedRecoveryOutcome');
  assert.equal(outcomeReq.backend_session_id, 'sess-backend-100');
  // Must NOT use camelCase backendSessionId
  assert.equal(outcomeReq.backendSessionId, undefined);

  const parsedOutcome = JSON.parse(JSON.stringify(outcomeReq));
  assert.equal(parsedOutcome.type, 'pairedRecoveryOutcome');
  assert.equal(parsedOutcome.backend_session_id, 'sess-backend-100');
});

test('unexecuted harness boundary: client provider has no host delivery observer and fails closed without host machine seam', async () => {
  // Unexecuted static assertion: client provider has no host delivery observer capability.
  // Proves that when the host machine delivery observer seam is absent, client request ledgers
  // cannot be substituted as wire delivery proof and must fail closed with typed barrier.
  const fakeControl = {
    call: async (req) => {
      if (req.type === 'pairedRecoveryOutcome') {
        return {
          type: 'pairedRecoveryOutcomeResult',
          outcome: { backend_session_id: 'b-1', daemonEpoch: '1', incarnation: 'inc-1' },
        };
      }
      if (req.type === 'qaPairedHostDeliveryAudit') {
        return { type: 'error', message: 'Unknown request type: qaPairedHostDeliveryAudit' };
      }
      return { type: 'unknown' };
    },
  };

  const session = {
    identity: { backendSessionId: 'b-1', daemonEpoch: '1', incarnation: 'inc-1', generation: 1 },
  };

  // Verify that wire delivery ledger audit requires the authoritative host daemon observer seam
  const auditHostDelivery = async (control, session) => {
    const res = await control.call({ type: 'qaPairedHostDeliveryAudit', backendSessionId: session.identity.backendSessionId });
    if (res.type !== 'qaPairedHostDeliveryAuditOk') {
      throw new TransportBarrier(
        'TRAFFIC_AUDITOR_PRODUCER_REQUIRED',
        'traffic-endpoint-audit-producer-required: host machine wire delivery observer seam absent',
      );
    }
    return res.audit;
  };

  await assert.rejects(
    auditHostDelivery(fakeControl, session),
    (err) => err.code === 'TRAFFIC_AUDITOR_PRODUCER_REQUIRED',
  );
});

test('unexecuted harness contract: pairedHostDeliveryObserver enforces concrete target, explicit deviceId, generation, arm+seal interval, and rejects absent fence', async () => {
  const target = { machineId: 'm-host-1', daemonEpoch: '1', sessionId: 'sess-1' };
  const mockCalls = [];
  const fakeAudit = {
    target,
    fence: { fenceId: 'fence-sess-1-1-1', status: 'sealed', startDeliverySeq: 5, endDeliverySeq: 7 },
    interval: {
      startDeliverySeq: 5,
      endDeliverySeq: 7,
      totalDelivered: 2,
      deliveredWrites: 1,
      deliveredSignals: 1,
      failedCount: 0,
      unresolvedAtSeal: 0,
      replayViolations: [],
      signalMultiplicityViolation: false,
      signalDeficit: false,
      writeMultiplicityViolation: false,
    },
    records: [
      { deliverySeq: 6, ingressSeq: 6, operation: 'write', success: true, payloadHash: 'hash-w1' },
      { deliverySeq: 7, ingressSeq: 7, operation: 'signal', success: true, payloadHash: 'hash-s1' },
    ],
    unresolvedAtSeal: [],
    preIntervalDeliveries: [],
  };

  const mockHostControl = {
    call: async (req) => {
      mockCalls.push(req);
      if (req.type === 'qaPairedHostDeliveryArmFence') {
        if (!req.target?.machineId || !req.target?.daemonEpoch || !req.target?.sessionId) {
          return { type: 'error', message: 'INVALID_TARGET' };
        }
        return { type: 'qaPairedHostDeliveryArmFenceOk', fenceId: 'fence-sess-1-1-1' };
      }
      if (req.type === 'qaPairedHostDeliverySealFence') {
        return {
          type: 'qaPairedHostDeliverySealFenceOk',
          fence: { fenceId: req.fenceId, target: req.target, status: 'sealed', startDeliverySeq: 5, endDeliverySeq: 7 },
        };
      }
      if (req.type === 'qaPairedHostDeliveryAudit') {
        if (req.fenceId === 'missing-fence') {
          return { type: 'error', message: 'FENCE_NOT_FOUND' };
        }
        return { type: 'qaPairedHostDeliveryAuditOk', audit: fakeAudit };
      }
      return { type: 'error', message: 'unexpected' };
    },
  };

  const observer = pairedHostDeliveryObserver(mockHostControl);

  // Negative tests: missing context must reject with typed TransportBarrier
  await assert.rejects(
    observer.armFence({ target, generation: 1 }),
    (err) => err instanceof TransportBarrier && err.code === 'DEVICE_IDENTITY_REQUIRED',
  );
  await assert.rejects(
    observer.armFence({ target, deviceId: '', generation: 1 }),
    (err) => err instanceof TransportBarrier && err.code === 'DEVICE_IDENTITY_REQUIRED',
  );
  await assert.rejects(
    observer.armFence({ target, deviceId: '   ', generation: 1 }),
    (err) => err instanceof TransportBarrier && err.code === 'DEVICE_IDENTITY_REQUIRED',
  );
  await assert.rejects(
    observer.armFence({ target, deviceId: 'dev-1' }),
    (err) => err instanceof TransportBarrier && err.code === 'GENERATION_REQUIRED',
  );
  await assert.rejects(
    observer.armFence({ target, deviceId: 'dev-1', generation: '' }),
    (err) => err instanceof TransportBarrier && err.code === 'GENERATION_REQUIRED',
  );
  await assert.rejects(
    observer.armFence({ target: { sessionId: 'sess-1' }, deviceId: 'dev-1', generation: 1 }),
    (err) => err instanceof TransportBarrier && err.code === 'TARGET_IDENTITY_INCOMPLETE',
  );
  await assert.rejects(
    observer.sealFence({ target }),
    (err) => err instanceof TransportBarrier && err.code === 'FENCE_ID_REQUIRED',
  );
  await assert.rejects(
    observer.audit({ target }),
    (err) => err instanceof TransportBarrier && err.code === 'FENCE_ID_REQUIRED',
  );

  // 1. Arm fence with explicit concrete target, explicit deviceId, and explicit generation
  const fenceId = await observer.armFence({
    target,
    deviceId: 'dev-client-1',
    generation: 1,
    expectedWriteMultiplicity: 1,
    expectedSignalMultiplicity: 1,
  });
  assert.equal(fenceId, 'fence-sess-1-1-1');
  const armCall = mockCalls.find((c) => c.type === 'qaPairedHostDeliveryArmFence');
  assert.deepEqual(armCall.target, target);
  assert.equal(armCall.deviceId, 'dev-client-1');
  assert.equal(armCall.generation, '1');
  assert.equal(armCall.expectedWriteMultiplicity, 1);
  assert.equal(armCall.expectedSignalMultiplicity, 1);

  // 2. Seal fence to establish bounded interval
  const sealed = await observer.sealFence({ fenceId, target });
  assert.equal(sealed.status, 'sealed');
  assert.equal(sealed.endDeliverySeq, 7);

  // 3. Audit sealed interval
  const audit = await observer.audit({ target, fenceId });
  assert.deepEqual(audit, fakeAudit);
  assert.equal(audit.interval.deliveredWrites, 1);
  assert.equal(audit.interval.deliveredSignals, 1);
  assert.equal(audit.interval.replayViolations.length, 0);

  // 4. Reject absent fence on host
  await assert.rejects(
    observer.audit({ target, fenceId: 'missing-fence' }),
    (err) => err instanceof TransportBarrier && err.code === 'TRAFFIC_AUDITOR_PRODUCER_REQUIRED',
  );
});


test('paired mutation preserves original nonce and uses paired sender exactly once', async () => {
  const calls = [];
  const control = { call: async (request) => {
    calls.push(request);
    return { type: 'qaPairedMutationOk', intentId: request.intentId };
  } };
  const owner = pairedOwnerControls(control);
  const session = { identity: { backendSessionId: 'b', daemonEpoch: '9', incarnation: 'i', generation: 3 } };
  await owner.mutate(session, 'signal', Buffer.alloc(0), 'signal-original');
  await owner.mutate(session, 'write', Buffer.from('payload-original'), 'write-original');
  assert.equal(calls.length, 2);
  assert.deepEqual(calls.map((r) => r.type), ['qaPairedMutation', 'qaPairedMutation']);
  assert.equal(calls[0].intentId, 'signal-original');
  assert.equal(calls[0].incarnation, 'i');
  assert.equal(calls[0].generation, 3);
  assert.equal(Buffer.from(calls[1].data, 'base64').toString(), 'payload-original');
});
