import { isDeepStrictEqual } from 'node:util';
import { posix, win32 } from 'node:path';
import { ATTACH_TUPLE_FIELDS, HarnessError, requireSevenTupleReceipt } from '../lib/qa-scenarios/common-harness.mjs';
import { executablePathMatches } from './herdr-reference-fixtures.mjs';
import { driveNativeFaults, NATIVE_FAULTS } from './fixtures/native-recovery-transport.mjs';

export const NATIVE_SCENARIOS = Object.freeze(Object.keys(NATIVE_FAULTS));

// HarnessError intentionally normalizes codes outside the shared harness
// contract. Executor barriers belong to this contract, not that allowlist.
class NativeBarrier extends Error {
  constructor(code, detail) {
    super(`${code}: ${detail}`);
    this.code = code;
    this.detail = detail;
  }
}

function requireEvidence(condition, message) {
  if (!condition) throw new HarnessError('ASSERTION_FAILURE', message);
}

function requireMethod(adapter, method, label) {
  if (typeof adapter?.[method] !== 'function') {
    throw new NativeBarrier('NATIVE_ADAPTER_REQUIRED', `${label}.${method} is required; no native execution occurred`);
  }
}

function below(root, path, platform) {
  const paths = platform === 'win32' ? win32 : posix;
  if (typeof root !== 'string' || typeof path !== 'string' || !paths.isAbsolute(root) || !paths.isAbsolute(path)) return false;
  const delta = paths.relative(root, path);
  return delta !== '' && delta !== '..' && !delta.startsWith('../') && !delta.startsWith('..\\') && !paths.isAbsolute(delta);
}

// One subscription, installed before launch/snapshot/trigger. Waits consume a
// shared bounded attempt deadline, and can consume events already in this journal.
// seq is allocated by the receipt collector, not by separate producer clocks.
function observation(provider, signal, deadlineAt) {
  const events = [];
  const pending = new Set();
  let failure;
  const unsubscribe = provider.subscribe((event) => {
    if (!Number.isSafeInteger(event.seq) || event.seq <= (events.at(-1)?.seq ?? 0)) {
      failure = new HarnessError('ASSERTION_FAILURE', 'Native receipt sequence is not strictly increasing');
      for (const item of pending) item.reject(failure);
      return;
    }
    events.push(event);
    for (const item of pending) {
      if (event.seq > item.after && event.kind === item.kind && item.match(event)) item.resolve(event);
    }
  });
  const abort = () => {
    for (const item of pending) item.reject(signal.reason);
  };
  signal.addEventListener('abort', abort, { once: true });
  return {
    events,
    assertHealthy() { if (failure) throw failure; },
    cursor: () => events.at(-1)?.seq ?? 0,
    wait(kind, after, match = () => true) {
      if (signal.aborted) return Promise.reject(signal.reason);
      if (failure) return Promise.reject(failure);
      const found = events.find((event) => event.seq > after && event.kind === kind && match(event));
      if (found) return Promise.resolve(found);
      const promise = new Promise((resolve, reject) => {
        const timer = setTimeout(() => item.reject(new HarnessError('ASSERTION_FAILURE',
          `Missing native ${kind} receipt after sequence ${after}`)), Math.max(0, deadlineAt - performance.now()));
        const settle = (callback, value) => {
          clearTimeout(timer);
          pending.delete(item);
          callback(value);
        };
        const item = { kind, after, match, resolve: (value) => settle(resolve, value), reject: (error) => settle(reject, error) };
        pending.add(item);
        if (signal.aborted) item.reject(signal.reason);
      });
      // A trigger can fail before its armed promises are awaited. Keep the
      // original rejection observable without an unhandled rejection race.
      promise.catch(() => {});
      return promise;
    },
    close() {
      unsubscribe();
      signal.removeEventListener('abort', abort);
      for (const item of pending) item.reject(new Error('Native observation closed'));
    },
  };
}

function currentPresentation(event, expected) {
  requireEvidence(event.receipt?.presented === true, 'Final native receipt must explicitly be presented === true');
  const tuple = requireSevenTupleReceipt(event.receipt, expected, 'native current presentation');
  requireEvidence(event.source === 'pane_presentation_receipt' || event.source === 'presentation.receipt.jsonl',
    'Presentation must come from the product producer, not inventory or an adapter-generated tuple');
  return tuple;
}

function assertReady(observe, after, presented, before, expectedEpoch, marker) {
  const tuple = currentPresentation(presented, {
    backendSessionId: before.backendSessionId, incarnation: before.incarnation,
    frontendSessionId: before.frontendSessionId, paneIdentity: before.paneIdentity,
    daemonEpoch: expectedEpoch,
  });
  requireEvidence(tuple.attemptGeneration > before.attemptGeneration, 'Recovery did not refresh native attempt generation');
  const sameTuple = (event) => ATTACH_TUPLE_FIELDS.every((field) => event.attachTuple?.[field] === tuple[field]);
  const cycle = observe.events.filter((event) => event.seq > after);
  const persisted = cycle.find((event) => event.kind === 'binding-persisted' && sameTuple(event));
  const listener = cycle.find((event) => event.kind === 'listener' && sameTuple(event) && event.active === true);
  const attached = cycle.find((event) => event.kind === 'attached' && sameTuple(event));
  const ready = cycle.find((event) => event.kind === 'status' && event.outcome === 'ready' && sameTuple(event));
  const output = cycle.find((event) => event.kind === 'output' && sameTuple(event) && event.bytes.includes(marker));
  requireEvidence(persisted && listener && attached && ready && output, 'Missing exact persisted/listener/attached/ready/output tuple receipts');
  requireEvidence(persisted.seq < listener.seq && listener.seq < attached.seq && attached.seq < ready.seq &&
    attached.seq < presented.seq && attached.seq < output.seq, 'Native readiness violated persist-before-listener/attach/ready ordering');
  return tuple;
}

function assertPayloads(events, after, mutation) {
  const cycle = events.filter((event) => event.seq > after);
  const requests = cycle.filter((event) => event.kind === 'request');
  requireEvidence(requests.length > 0, 'No actual recovery request ingress was captured');
  requireEvidence(requests.every((event) => !['createSession', 'spawn', 'spawnTerminalDetailed', 'ensureSessionBackends',
    'agentResume', 'newShell'].includes(event.operation)), 'Automatic process creation request during recovery');
  const deliveries = cycle.filter((event) => event.kind === 'delivery' &&
    ['write', 'input', 'signal', 'interrupt'].includes(event.operation));
  requireEvidence(deliveries.length === 1 && deliveries[0].mutationId === mutation.id &&
    deliveries[0].operation === mutation.operation && isDeepStrictEqual(deliveries[0].payload, mutation.payload),
  'Uncertain payload was absent, changed, duplicated, or another mutation was replayed');
  const sent = requests.filter((event) => ['write', 'input', 'signal', 'interrupt'].includes(event.operation));
  requireEvidence(sent.length === 1 && sent[0].mutationId === mutation.id &&
    isDeepStrictEqual(sent[0].payload, mutation.payload), 'Uncertain mutation was automatically resent at ingress');
}

// Explicit adapters own platform implementation. No shell, GUI, spawn, build or
// network activity occurs on import. No fallback driver touches the local Mac.
export async function runNativeRecoveryScenario(scenario, ctx) {
  const plan = NATIVE_FAULTS[scenario];
  if (!plan) throw new NativeBarrier('NATIVE_SCENARIO_UNKNOWN', scenario);
  if (ctx.signal?.aborted) return { scenario, status: 'incomplete', barrierCode: 'NATIVE_CANCELLED', executed: false };
  const { host, provider, spawn: spawner, transport, ledger, candidate, presentation } = ctx;
  const needed = [
    [provider, ['subscribe', 'snapshot', 'capture', 'sendUncertain'], 'provider'],
    [spawner, ['prepare', 'launch'], 'spawn'],
    [transport, ['beginAudit', 'endCurrentStream', 'disconnect', 'holdOutage', 'restore', 'changeOwnerEpoch',
      'emitOutput', 'flush', 'release'], 'transport'],
    [host, ['probePid', 'subscribeExit', 'terminateExact', 'removeProfile', 'worktreeList'], 'host'],
    [ledger, ['recordPid'], 'ledger'],
    [presentation, ['cursor', 'awaitPresentation'], 'presentation'],
  ];
  try {
    for (const [adapter, methods, label] of needed) for (const method of methods) requireMethod(adapter, method, label);
    if (ctx.launchOwnership?.gui !== true || ctx.launchOwnership?.pty !== true)
      throw new NativeBarrier('NATIVE_OWNERSHIP_REQUIRED', 'Actual GUI and PTY spawn-time ownership producers must be established before launch');
    requireEvidence(host.remote === true && host.desktopTransport === 'mahord', 'Native GUI requires a remote MahoRD host');
    requireEvidence(candidate?.stamp && candidate.binarySha256 && candidate.uiSha256, 'Frozen candidate artifact hashes are required');
    requireEvidence(typeof ctx.runId === 'string' && ctx.runId.length > 0, 'Unique runId is required');
    requireEvidence(ctx.timeoutMs >= 90_000 && ctx.timeoutMs <= 300_000, 'Native scenario requires a 90-300 second total deadline');
    requireEvidence(ctx.mutation?.id && ['write', 'signal'].includes(ctx.mutation.operation) &&
      ctx.mutation.payload !== undefined, 'Explicit real uncertain write or signal payload is required');
  } catch (error) {
    return { scenario, status: ['NATIVE_ADAPTER_REQUIRED', 'NATIVE_OWNERSHIP_REQUIRED'].includes(error.code) ? 'incomplete' : 'failed',
      barrierCode: error.code, detail: error.message, executed: false };
  }
  const controller = new AbortController();
  const externalAbort = () => controller.abort(new NativeBarrier('NATIVE_CANCELLED', 'External native scenario cancellation'));
  ctx.signal?.addEventListener('abort', externalAbort, { once: true });
  if (ctx.signal?.aborted) externalAbort();
  const timer = setTimeout(() => controller.abort(new Error('Native scenario deadline exceeded')), ctx.timeoutMs);
  const signal = controller.signal;
  const deadlineAt = performance.now() + ctx.timeoutMs;
  let observe;
  let setup;
  let result;
  const owned = [];
  const recordOwnedPid = (receipt) => {
    requireEvidence(receipt.hostId === host.id && receipt.candidate === candidate.stamp &&
      Number.isSafeInteger(receipt.pid) && receipt.pid > 0 && receipt.executable && receipt.processStart,
    'Spawn receipt needs exact host/candidate/PID/executable/process-start identity');
    const entry = ledger.recordPid(receipt.pid, receipt.executable, { ...receipt, nativeScenario: scenario });
    owned.push(entry);
  };
  try {
    signal.throwIfAborted();
    observe = observation(provider, signal, deadlineAt);
    // prepare must not launch; it resolves isolated roots and fixture providers.
    setup = await spawner.prepare({ scenario, host, candidate, recordOwnedPid, signal });
    requireEvidence(setup.candidate === candidate.stamp && setup.binarySha256 === candidate.binarySha256 &&
      setup.uiSha256 === candidate.uiSha256, 'Native app and UI are not the same frozen candidate');
    requireEvidence(setup.productionUntouched === true && below(host.worktreeRoot, setup.worktree, host.platform) &&
      [setup.profile, setup.store, setup.socket].every((path) => below(setup.worktree, path, host.platform)),
    'Native runtime/profile/socket/store must be isolated under the canonical remote worktree root');
    // Two producers publish kind 'presentation': the frontend lifecycle event
    // (native-events.jsonl, no receipt) and the product receipt. Their arrival
    // order is not fixed, so the baseline waits for the product receipt only.
    const initial = observe.wait('presentation', 0, (event) => event.receipt !== undefined);
    signal.throwIfAborted();
    await spawner.launch({ setup, recordOwnedPid, signal });
    const first = await initial;
    const before = currentPresentation(first, {});
    const baseline = await provider.snapshot({ setup, signal });
    requireEvidence(ATTACH_TUPLE_FIELDS.every((field) => baseline.attachTuple?.[field] === before[field]),
      'Initial snapshot disagrees with the product presentation tuple');
    const target = baseline.target;
    requireEvidence(target?.pid > 0 && target.processStart && target.backendSessionId === before.backendSessionId &&
      target.incarnation === before.incarnation && target.daemonEpoch === before.daemonEpoch,
    'Measured target PID/incarnation/epoch identity is required');
    requireEvidence(owned.some((entry) => entry.pid === target.pid && entry.extra.processStart === target.processStart),
      'Target PID was not recorded at spawn time');
    requireEvidence(typeof baseline.focus?.paneIdentity === 'string' && baseline.focus.paneIdentity.trim().length > 0 &&
      baseline.focus.paneIdentity !== before.paneIdentity,
      'Setup must leave another real pane focused before recovery');
    const audit = await transport.beginAudit({ setup, target, signal });
    const scopeVerified = audit.scope === 'all-isolated-processes' ||
      (audit.scope === 'registered-isolated-owner-set' && Array.isArray(audit.registeredOwners) &&
       ['gui', 'daemon', 'terminal-local-pty'].every((c) => audit.registeredOwners.some((o) => o.component === c)) &&
       audit.registeredOwners.every((o) => owned.some((entry) => entry.pid === o.pid && entry.executablePath === o.executable)));
    if (audit.complete !== true || audit.ingress !== true || audit.deliveries !== true ||
      !scopeVerified) throw new NativeBarrier('NATIVE_AUDIT_REQUIRED',
      'Complete request/delivery producer is missing; zero-create/replay acceptance is incomplete');
    const after = observe.cursor();
    const mutation = ctx.mutation;
    const delivered = observe.wait('delivery', after, (event) => event.mutationId === mutation.id);
    // The fixture forwards this exactly once but drops its acknowledgment.
    // sendUncertain drives the product input path, not a direct daemon write.
    const uncertain = await provider.sendUncertain({ setup, target, mutation, signal });
    requireEvidence(uncertain.outcome === 'unknown' && uncertain.mutationId === mutation.id,
      'Mutation must actually cross the product path with a lost acknowledgment');
    await delivered;
    // Deliberate input targeting restores the prior pane before fault injection;
    // focus-steal assertions cover automatic recovery, not our explicit input.
    const recoveryCursor = observe.cursor();
    const physicalCursor = await presentation.cursor({ signal });
    requireEvidence(typeof physicalCursor.path === 'string' && Number.isSafeInteger(physicalCursor.afterLine) &&
      physicalCursor.afterLine >= -1, 'Presentation cursor must identify a physical receipt file and line');
    const faultCursor = await driveNativeFaults(scenario, { transport, observe, target, signal });
    const marker = `NATIVE_RECOVERY_${ctx.runId}_${scenario}`;
    const output = observe.wait('output', faultCursor, (event) => event.bytes.includes(marker));
    const ready = observe.wait('status', faultCursor, (event) => event.outcome === 'ready');
    await transport.restore({ target, signal });
    // Fixture-side output generation is not client input and must run in the
    // retained target process; emitOutput returns its measured PID/start identity.
    const emitted = await transport.emitOutput({ target, marker, signal });
    requireEvidence(emitted.pid === target.pid && emitted.processStart === target.processStart,
      'Output marker did not originate in the retained process');
    await output;
    await ready;
    const current = await provider.snapshot({ setup, signal });
    const expected = requireSevenTupleReceipt(current.attachTuple, {}, 'current frontend snapshot');
    const gui = owned.find(entry => entry.extra.role === 'gui');
    if (!gui) throw new NativeBarrier('NATIVE_OWNERSHIP_REQUIRED', 'No actual GUI spawn receipt is recorded');
    signal.throwIfAborted();
    const rawPresentation = await presentation.awaitPresentation({ sessionId: before.backendSessionId,
      expected, afterLine: physicalCursor.afterLine, producerPid: gui.pid,
      timeoutMs: Math.max(1, deadlineAt - performance.now()), signal });
    requireEvidence(rawPresentation.path === physicalCursor.path &&
      Number.isSafeInteger(rawPresentation.lineIndex) && rawPresentation.lineIndex > physicalCursor.afterLine &&
      rawPresentation.raw?.producerPid === gui.pid, 'Presentation is not after the physical cursor from the owned GUI');
    requireSevenTupleReceipt(rawPresentation.raw, expected, 'raw current presentation');
    const presented = await observe.wait('presentation', faultCursor, event =>
      event.path === rawPresentation.path && event.lineIndex === rawPresentation.lineIndex &&
      isDeepStrictEqual(event.receipt, rawPresentation.raw));
    const flushed = await transport.flush({ signal });
    requireEvidence(flushed.complete === true && flushed.auditId === audit.id && flushed.throughSeq >= observe.cursor(),
      'Request/delivery audit did not flush through final recovery receipts');
    observe.assertHealthy();
    const final = await provider.snapshot({ setup, signal });
    const tuple = assertReady(observe, after, presented, before, final.target.daemonEpoch, marker);
    requireEvidence(ATTACH_TUPLE_FIELDS.every((field) => final.attachTuple?.[field] === tuple[field]),
      'Final presented tuple is not the current frontend binding');
    requireEvidence(final.target.pid === target.pid && final.target.processStart === target.processStart &&
      final.target.incarnation === target.incarnation && final.target.backendSessionId === target.backendSessionId,
    'Recovery replaced the target PID/incarnation');
    const cycle = observe.events.filter((event) => event.seq > after);
    requireEvidence(cycle.filter((event) => event.kind === 'stream-ended').length >= plan.streamEnds,
      'Insufficient measured native stream ends');
    if (plan.minimumFailures) requireEvidence(cycle.filter((event) => event.kind === 'attempt' &&
      event.outcome === 'retryable').length >= plan.minimumFailures, 'Local outage did not exceed five failed attempts');
    if (plan.staleEpoch) requireEvidence(tuple.daemonEpoch !== before.daemonEpoch, 'Stale epoch was never refreshed');
    requireEvidence(cycle.some((event) => event.kind === 'status' && event.outcome === 'retryable'),
      'No actual recovering status was observed');
    requireEvidence(!cycle.some((event) => event.kind === 'status' && ['exited', 'cancelled'].includes(event.outcome)),
      'Unknown outage falsely settled the live session');
    requireEvidence(isDeepStrictEqual(final.focus, baseline.focus) && cycle.filter((event) => event.seq > recoveryCursor && event.kind === 'focus')
      .every((event) => isDeepStrictEqual(event.focus, baseline.focus)), 'Recovery stole focus');
    assertPayloads(observe.events, after, mutation);
    const capture = await provider.capture({ setup, marker, attachTuple: tuple, signal });
    requireEvidence(capture.path && capture.sha256 && capture.renderedText.includes(marker) &&
      capture.driver === 'mahord', 'MahoRD rendered native marker capture is missing');
    result = { scenario, status: 'passed', executed: true, candidate, targetBefore: target, targetAfter: final.target,
      attachTuple: tuple, presentation: rawPresentation, capture, timeline: [...observe.events], assertions: ['same-process', 'current-presented-seven-tuple',
        'persist-before-ready', 'output-listener-attached', 'no-focus-steal', 'zero-create', 'zero-uncertain-replay'] };
  } catch (error) {
    result = { scenario, status: ['NATIVE_ADAPTER_REQUIRED', 'NATIVE_OWNERSHIP_REQUIRED', 'NATIVE_CANCELLED',
      'NATIVE_SETUP_UNIMPLEMENTED', 'NATIVE_AUDIT_REQUIRED'].includes(error.code) ? 'incomplete' : 'failed', executed: true,
      barrierCode: error.code ?? 'NATIVE_EXECUTION_FAILED', detail: error.message, timeline: [...(observe?.events ?? [])] };
  } finally {
    controller.abort(new Error('Native scenario finished'));
    clearTimeout(timer);
    ctx.signal?.removeEventListener('abort', externalAbort);
    observe?.close();
    // Teardown has a separate bounded budget; an aborted scenario signal must
    // not prevent cleanup. Unconfirmed exits retain profile and report evidence.
    const cleanupController = new AbortController();
    const cleanupSignal = cleanupController.signal;
    let cleanupTimer;
    const cleanupDeadline = new Promise((_, reject) => {
      cleanupTimer = setTimeout(() => {
        const error = Object.assign(new Error('Native cleanup deadline exceeded'), { code: 'NATIVE_CLEANUP_TIMEOUT' });
        reject(error);
        cleanupController.abort(error);
      }, 15_000);
      cleanupTimer.ref();
    });
    const bounded = (operation) => {
      cleanupSignal.throwIfAborted();
      return Promise.race([operation(), cleanupDeadline]);
    };
    const cleanup = [];
    const cleanupError = error => cleanup.push({ exited: false, action: 'retained-cleanup-error', code: error.code, detail: error.message,
      ...(error instanceof AggregateError ? { errors: error.errors.map(item => ({ code: item.code, detail: item.message })) } : {}) });
    try { await bounded(() => transport.release({ signal: cleanupSignal })); }
    catch (error) { cleanupError(error); }
    for (const entry of [...owned].reverse()) {
      try {
        const live = await bounded(() => host.probePid({ pid: entry.pid, signal: cleanupSignal }));
        if (live.exited === true) { cleanup.push({ pid: entry.pid, exited: true, action: 'already-exited' }); continue; }
        if (live.processStart !== entry.extra.processStart ||
          !executablePathMatches(live.executable, entry.executablePath, host.platform)) {
          cleanup.push({ pid: entry.pid, exited: false, action: 'retained-identity-mismatch', live });
          continue;
        }
        const exit = host.subscribeExit({ pid: entry.pid, processStart: entry.extra.processStart, signal: cleanupSignal });
        exit.catch(() => {});
        await bounded(() => host.terminateExact({ pid: entry.pid, executable: entry.executablePath,
          processStart: entry.extra.processStart, tree: false, signal: cleanupSignal }));
        const receipt = await bounded(() => exit);
        requireEvidence(receipt.pid === entry.pid && receipt.processStart === entry.extra.processStart && receipt.exited === true,
          'Cleanup exit receipt does not identify the spawn-recorded process');
        cleanup.push({ ...receipt, action: 'terminated-exact-pid' });
      } catch (error) { cleanupError(error); }
    }
    try {
      if (setup && owned.length > 0 && cleanup.every((entry) => entry.exited)) {
        await bounded(() => host.removeProfile({ setup, signal: cleanupSignal }));
      }
    } catch (error) {
      cleanupError(error);
    }
    result.cleanup = cleanup;
    result.profileRetained = !!setup && (owned.length === 0 || cleanup.some((entry) => !entry.exited));
    if (result.profileRetained) result.retainedProfile = setup.profile;
    try { result.worktrees = await bounded(() => host.worktreeList({ signal: cleanupSignal })); }
    catch (error) { result.worktreeListError = error.message; }
    finally { clearTimeout(cleanupTimer); }
    if (result.status === 'passed' && (cleanup.some((entry) => !entry.exited) || result.profileRetained || result.worktreeListError)) result.status = 'failed';
  }
  return result;
}

export const runNativeStreamEndsScenario = (ctx) => runNativeRecoveryScenario('native-stream-ends', ctx);
export const runNativeRemoteMahordScenario = (ctx) => runNativeRecoveryScenario('native-remote-mahord', ctx);
export const runNativeLocalOutageScenario = (ctx) => runNativeRecoveryScenario('local-outage', ctx);
export const runNativeStaleEpochScenario = (ctx) => runNativeRecoveryScenario('stale-epoch', ctx);
