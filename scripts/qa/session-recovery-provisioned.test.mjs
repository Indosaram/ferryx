import { afterEach, expect, test, vi } from 'vitest';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { dirname, basename } from 'node:path';
import { readFile } from 'node:fs/promises';
import { EventEmitter } from 'node:events';
import { runProvisionedScenario, verifyCandidate } from './session-recovery-provisioned.mjs';
import { teardownOwnedChildren } from './session-recovery-consistency.mjs';
import { buildWebSelection } from './fixtures/session-recovery-web-config.mjs';

const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const here = fileURLToPath(import.meta.url);
const sourceHash = 'a'.repeat(64);
const receipts = vi.hoisted(() => new Map());
afterEach(() => vi.useRealTimers());

// No disk creation, real identity, global environment mutation, or browser launch.
async function options() {
  const bytes = await readFile(here);
  const sha256 = digest(bytes);
  return { scenario: 'confirmed-exit', evidenceDir: '/fixture/evidence', runId: 'row',
    candidate: { stamp: 'candidate', sourceHash,
      binary: { path: here, sha256, sourceHash },
      ui: { path: dirname(here), sourceHash, files: [{ path: basename(here), sha256 }], sha256 } },
    webSelection: { target: { sessionId: 'a' }, rebindTarget: { sessionId: 'b' } } };
}

// Artifact reads are the boundary; fixtures preserve actual hashing and inventory logic.
vi.mock('node:fs/promises', async importOriginal => {
  const original = await importOriginal();
  return { ...original,
    mkdir: async () => {},
    writeFile: async (path, body, options) => {
      if (options.flag !== 'wx' || receipts.has(path)) throw Object.assign(new Error('exists'), { code: 'EEXIST' });
      receipts.set(path, JSON.parse(body));
    },
    realpath: async path => path === '/fixture/ui' || path === '/fixture/ui/index.html' ? path : original.realpath(path),
    readdir: async path => path === '/fixture/ui' ? [{ name: 'index.html', isDirectory: () => false, isFile: () => true }] : original.readdir(path),
    readFile: async path => path === '/fixture/ui/index.html' ? Buffer.from('candidate-ui') : original.readFile(path),
  };
});

async function valid() {
  const value = await options();
  const sha256 = digest(Buffer.from('candidate-ui'));
  value.candidate.ui = { path: '/fixture/ui', sourceHash,
    files: [{ path: 'index.html', sha256 }], sha256: digest(Buffer.from(JSON.stringify([['index.html', sha256]]))) };
  return value;
}

test('verifies same-source binary and complete UI inventory', async () => {
  const input = await valid();
  const receipt = await verifyCandidate(input.candidate);
  expect(receipt.sourceHash).toBe(sourceHash);
  expect(receipt.binarySha256).toBe(input.candidate.binary.sha256);
  expect(receipt.uiSha256).toBe(input.candidate.ui.sha256);
});

test.each(['source', 'binary', 'ui'])('rejects changed %s provenance before dispatch', async kind => {
  const input = await valid();
  if (kind === 'source') input.candidate.ui.sourceHash = 'b'.repeat(64);
  if (kind === 'binary') input.candidate.binary.sha256 = '0'.repeat(64);
  if (kind === 'ui') input.candidate.ui.files[0].sha256 = '0'.repeat(64);
  const dispatchScenario = vi.fn();
  const result = await runProvisionedScenario(input, { dispatchScenario });
  expect(result.status).toBe('failed');
  expect(dispatchScenario).not.toHaveBeenCalled();
});

test('unsupported row does not impose native prerequisites on controlled web', async () => {
  const missing = await runProvisionedScenario({ scenario: 'local-outage' });
  const input = await valid();
  const dispatchScenario = vi.fn(async () => ({ status: 'passed' }));
  const result = await runProvisionedScenario(input, { dispatchScenario });
  expect(missing).toMatchObject({ status: 'incomplete', barrierCode: 'PROVISIONING_ADAPTER_UNSUPPORTED' });
  expect(result).toMatchObject({ status: 'passed', evidenceScope: 'controlled-ui', realProcessAcceptance: false });
  expect(dispatchScenario).toHaveBeenCalledOnce();
});

test('requires caller creation contract only for the cold-start row', async () => {
  const input = await valid();
  input.scenario = 'cold-start-unknown';
  const dispatchScenario = vi.fn();
  const result = await runProvisionedScenario(input, { dispatchScenario });
  expect(result.barrierCode).toBe('ACTUAL_CREATE_ROUTE_REQUIRED');
  expect(dispatchScenario).not.toHaveBeenCalled();
});

test('web builder preserves supplied codecs and local explicit creation schema', async () => {
  const input = await valid();
  const target = { sessionId: 'a', daemonEpoch: 'epoch', incarnation: 'one', pid: 41 };
  const rebindTarget = { ...target, sessionId: 'b', incarnation: 'two', pid: 42 };
  const exitFrame = vi.fn();
  const gapFrame = vi.fn();
  const selection = buildWebSelection(input.candidate, { target, rebindTarget, exitFrame, gapFrame });
  expect(selection.exitFrame).toBe(exitFrame);
  expect(selection.gapFrame).toBe(gapFrame);
  expect(selection.target).toEqual(target);
  expect(selection).toMatchObject({ evidenceScope: 'controlled-ui',
    identityEvidence: 'fixture-identity-proof-not-machine-pid',
    createRoute: { method: 'POST', path: '/api/v1/workspace/select' } });
  expect(selection.createRoute.response(target, { workspaceId: 'ws', createTerminal: true })).toEqual({ ok: true });
  expect(() => selection.createRoute.response(target, { workspaceId: 'ws', sessionId: 'a' }))
    .toThrow('WEB_EXPLICIT_CREATE_BODY_REQUIRED');
  expect(() => buildWebSelection(input.candidate, { target, rebindTarget }))
    .toThrow('WEB_CONSUMED_EXIT_GAP_CODECS_REQUIRED');
  input.scenario = 'cold-start-unknown';
  input.webSelection = selection;
  const dispatchScenario = vi.fn(async () => ({ status: 'unverified' }));
  const result = await runProvisionedScenario(input, { dispatchScenario });
  expect(dispatchScenario).toHaveBeenCalledOnce();
  expect(result).toMatchObject({ status: 'unverified', realProcessAcceptance: false });
});

test('each row receives a distinct ledger at existing context seam', async () => {
  const ledgers = [];
  const dispatchScenario = async (_, ctx) => { ledgers.push(ctx.ledger); return { status: 'passed' }; };
  const input = await valid();
  await runProvisionedScenario(input, { dispatchScenario });
  await runProvisionedScenario(input, { dispatchScenario });
  expect(ledgers).toHaveLength(2);
  expect(ledgers[0]).not.toBe(ledgers[1]);
  expect(ledgers.map(ledger => ledger.entries)).toEqual([[], []]);
});

test('cancellation disposes exactly the opened resource once', async () => {
  const input = await valid();
  const controller = new AbortController();
  input.signal = controller.signal;
  const entered = Promise.withResolvers();
  const dispose = vi.fn(async () => {});
  const pending = runProvisionedScenario(input, {
    createScenarioContext: (options, ledger) => ({ ...options, ledger, openWebOutcome: async () => ({ dispose }) }),
    dispatchScenario: async (_, ctx) => {
      await ctx.openWebOutcome({});
      entered.resolve();
      await new Promise(resolve => ctx.signal.addEventListener('abort', resolve, { once: true }));
      return { status: 'passed' };
    },
  });
  await entered.promise;
  controller.abort();
  const result = await pending;
  expect(result.status).toBe('incomplete');
  expect(dispose).toHaveBeenCalledOnce();
});

test('cleanup refuses an owned PID whose executable changed', async () => {
  vi.useFakeTimers();
  const child = Object.assign(new EventEmitter(), { pid: 12345, exitCode: null, signalCode: null });
  const entry = { pid: child.pid, child, executablePath: '/fixture/ferryx' };
  const ledger = { entries: [entry], teardown(kill) {
    try { kill(entry.pid); } catch (error) { return { refused: true }; }
    return { refused: false };
  } };
  const signalPid = vi.fn();
  const pending = teardownOwnedChildren(ledger, { currentPlatform: 'linux', signalPid,
    probeIdentity: () => ({ alive: true, executable: '/foreign/app' }) });
  await vi.advanceTimersByTimeAsync(5000);
  const result = await pending;
  expect(signalPid).not.toHaveBeenCalled();
  expect(result).toMatchObject({ refused: true, allExited: false });
});

test('persists verified digests and executor evidence without claiming real PID', async () => {
  const input = await valid();
  input.invocation = { command: ['bun', 'caller.mjs'], entrypoint: 'runProvisionedScenario' };
  const evidence = [{ target: { sessionId: 'a', pid: 42 }, timeline: [{ event: 'attached' }],
    captures: [{ path: '/fixture/capture.png' }] }];
  const result = await runProvisionedScenario(input, { dispatchScenario: async () => ({ status: 'passed', evidence }) });
  expect(receipts.get(result.receiptPath)).toMatchObject({ runId: 'row', scenario: 'confirmed-exit',
    candidate: { sourceHash, binarySha256: input.candidate.binary.sha256, uiSha256: input.candidate.ui.sha256 },
    invocation: input.invocation, evidenceScope: 'controlled-ui', realProcessAcceptance: false,
    evidence, cleanup: { allExited: true } });
});

test('unsupported rows persist independent non-overwriting receipts', async () => {
  const input = { scenario: 'local-outage', evidenceDir: '/fixture/evidence', runId: 'same' };
  const first = await runProvisionedScenario(input);
  const second = await runProvisionedScenario(input);
  expect(first.receiptPath).not.toBe(second.receiptPath);
  expect(receipts.get(first.receiptPath)).toMatchObject({ status: 'incomplete', candidate: null,
    barrierCode: 'PROVISIONING_ADAPTER_UNSUPPORTED', realProcessAcceptance: false });
  expect(receipts.has(second.receiptPath)).toBe(true);
});

test.each(['EACCES', 'EEXIST'])('receipt write failure %s cannot return passed', async code => {
  const input = await valid();
  const result = await runProvisionedScenario(input, {
    dispatchScenario: async () => ({ status: 'passed' }),
    writeFile: async (_, __, options) => {
      expect(options.flag).toBe('wx');
      throw Object.assign(new Error(code), { code });
    },
  });
  expect(result).toMatchObject({ status: 'incomplete', barrierCode: 'ROW_RECEIPT_WRITE_FAILED', receiptError: code });
  expect(result.receiptPath).toBeUndefined();
});

test('late open disposal rejection is observed and cleanup stays unproven', async () => {
  const input = await valid();
  const controller = new AbortController();
  input.signal = controller.signal;
  const opening = Promise.withResolvers();
  const entered = Promise.withResolvers();
  const disposed = Promise.withResolvers();
  const dispatchDone = Promise.withResolvers();
  const pending = runProvisionedScenario(input, {
    createScenarioContext: (options, ledger) => ({ ...options, ledger, openWebOutcome: () => {
      entered.resolve(); return opening.promise;
    } }),
    dispatchScenario: async (_, ctx) => {
      try { await ctx.openWebOutcome({}); } finally { dispatchDone.resolve(); }
    },
  });
  await entered.promise;
  controller.abort();
  const result = await pending;
  opening.resolve({ dispose: async () => {
    disposed.resolve(); throw Object.assign(new Error('dispose'), { code: 'DISPOSE_FAILED' });
  } });
  await disposed.promise;
  await dispatchDone.promise;
  expect(result).toMatchObject({ status: 'incomplete', barrierCode: 'WEB_CLEANUP_UNPROVEN',
    webCleanup: { completed: false, pendingOpens: 1 } });
  expect(receipts.get(result.receiptPath).webCleanup.completed).toBe(false);
});
