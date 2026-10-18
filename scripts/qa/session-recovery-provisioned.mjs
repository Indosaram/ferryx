import { createHash, randomUUID } from 'node:crypto';
import { readFile, realpath, readdir, mkdir, writeFile } from 'node:fs/promises';
import { resolve, relative, isAbsolute } from 'node:path';
import { createScenarioContext, dispatchScenario, teardownOwnedChildren, SCENARIOS } from './session-recovery-consistency.mjs';
import { OwnedProcessLedger } from './herdr-reference-fixtures.mjs';
import { WEB_OUTCOME_SCENARIOS } from './session-recovery-web-outcomes.mjs';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const failure = code => Object.assign(new Error(code), { code });

// Each artifact receipt must identify the same frozen source, not merely a label.
export async function verifyCandidate(candidate) {
  const { stamp, sourceHash, binary, ui } = candidate ?? {};
  if (!stamp || !/^[a-f0-9]{64}$/.test(sourceHash ?? '') ||
      binary?.sourceHash !== sourceHash || ui?.sourceHash !== sourceHash ||
      !binary.path || !ui.path || !Array.isArray(ui.files) || !ui.files.length) {
    throw failure('CANDIDATE_RECEIPT_REQUIRED');
  }
  if (hash(await readFile(binary.path)) !== binary.sha256) throw failure('BINARY_HASH_MISMATCH');
  const root = await realpath(ui.path);
  const actual = [];
  async function collect(dir, prefix = '') {
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      const name = prefix + entry.name;
      if (entry.isDirectory()) await collect(resolve(dir, entry.name), name + '/');
      else if (entry.isFile()) actual.push(name);
      else throw failure('UI_INVENTORY_INVALID');
    }
  }
  await collect(root);
  const seen = new Set();
  const inventory = [];
  for (const file of ui.files) {
    if (!file.path || isAbsolute(file.path) || seen.has(file.path)) throw failure('UI_INVENTORY_INVALID');
    seen.add(file.path);
    const path = await realpath(resolve(root, file.path));
    const rel = relative(root, path);
    if (rel === '..' || rel.startsWith('../') || isAbsolute(rel)) throw failure('UI_PATH_OUTSIDE_ROOT');
    if (hash(await readFile(path)) !== file.sha256) throw failure('UI_FILE_HASH_MISMATCH');
    inventory.push([file.path, file.sha256]);
  }
  if (!seen.has('index.html')) throw failure('UI_INDEX_REQUIRED');
  if (actual.length !== seen.size || actual.some(path => !seen.has(path))) throw failure('UI_INVENTORY_INCOMPLETE');
  inventory.sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0);
  if (hash(Buffer.from(JSON.stringify(inventory))) !== ui.sha256) throw failure('UI_HASH_MISMATCH');
  return { stamp, sourceHash, binarySha256: binary.sha256, uiSha256: ui.sha256 };
}

// The second argument exposes only existing runner seams for source-only contract tests.
export async function runProvisionedScenario(options, seams = {}) {
  const { scenario, candidate, evidenceDir, runId, signal, webSelection, timeoutMs = 30000 } = options;
  const ledger = new OwnedProcessLedger(`task9-${runId ?? 'row'}`);
  const base = { scenario, evidenceScope: 'controlled-ui', realProcessAcceptance: false };
  async function persist(row) {
    const receipt = { ...row, runId, scenario, invocation: options.invocation ?? {
      entrypoint: 'runProvisionedScenario', binary: candidate?.binary?.path ?? null,
      uiDist: candidate?.ui?.path ?? null,
    }, selection: webSelection ?? null, candidate: row.candidate ?? null,
      cleanup: row.cleanup ?? { allExited: !row.cleanupError && ledger.entries.length === 0,
        entries: ledger.entries, waitResults: [] } };
    if (typeof evidenceDir !== 'string' || !evidenceDir || typeof runId !== 'string' || !runId)
      return { ...receipt, status: 'incomplete', receiptError: 'ROW_RECEIPT_DESTINATION_REQUIRED' };
    const path = resolve(evidenceDir, `row-${randomUUID()}.json`);
    try {
      await (seams.mkdir ?? mkdir)(evidenceDir, { recursive: true });
      await (seams.writeFile ?? writeFile)(path, JSON.stringify(receipt, null, 2), { flag: 'wx' });
      return { ...receipt, receiptPath: path };
    } catch (error) {
      return { ...receipt, status: 'incomplete', barrierCode: 'ROW_RECEIPT_WRITE_FAILED',
        receiptError: error.code ?? error.name };
    }
  }
  if (!SCENARIOS.includes(scenario)) return persist({ ...base, status: 'error', barrierCode: 'UNKNOWN_SCENARIO' });
  if (!WEB_OUTCOME_SCENARIOS.includes(scenario)) return persist({ ...base, status: 'incomplete',
    barrierCode: 'PROVISIONING_ADAPTER_UNSUPPORTED', evidenceScope: 'none' });
  if (!evidenceDir || !runId || !Number.isFinite(timeoutMs) || timeoutMs <= 0)
    return persist({ ...base, status: 'incomplete', barrierCode: 'ROW_OPTIONS_REQUIRED' });
  if (!webSelection?.target || !webSelection?.rebindTarget)
    return persist({ ...base, status: 'incomplete', barrierCode: 'WEB_SELECTION_REQUIRED' });
  if (scenario === 'cold-start-unknown' && (!webSelection.createRoute?.method ||
      !webSelection.createRoute?.path || typeof webSelection.createRoute.response !== 'function'))
    return persist({ ...base, status: 'incomplete', barrierCode: 'ACTUAL_CREATE_ROUTE_REQUIRED' });

  const controller = new AbortController();
  const abort = () => controller.abort(signal.reason);
  signal?.addEventListener('abort', abort, { once: true });
  if (signal?.aborted) abort();
  const timer = setTimeout(() => controller.abort(failure('ROW_DEADLINE')), timeoutMs);
  const active = new Set();
  const openings = new Set();
  const cleanupFailures = [];
  let result;
  let provenance;
  let onAbort;
  const cancelled = new Promise((_, reject) => {
    onAbort = () => reject(failure('ROW_CANCELLED'));
    controller.signal.addEventListener('abort', onAbort, { once: true });
    if (controller.signal.aborted) onAbort();
  });
  cancelled.catch(() => {});
  try {
    provenance = await Promise.race([verifyCandidate(candidate), cancelled]);
    controller.signal.throwIfAborted();
    const ctx = (seams.createScenarioContext ?? createScenarioContext)({
      evidenceDir, runId, timeoutMs, signal: controller.signal,
      binary: candidate.binary.path, uiDist: candidate.ui.path,
      candidateStamp: candidate.stamp, webOutcomeFixture: webSelection,
    }, ledger);
    const open = ctx.openWebOutcome;
    ctx.openWebOutcome = async request => {
      controller.signal.throwIfAborted();
      const pending = open(request);
      openings.add(pending);
      try {
        const run = await pending;
        let disposed;
        const dispose = run.dispose.bind(run);
        run.dispose = () => {
          disposed ??= Promise.resolve().then(dispose).catch(error => {
            cleanupFailures.push(error.code ?? error.name);
            throw error;
          }).finally(() => active.delete(run));
          // A late open after cancellation may outlive dispatch; observe rejection now.
          disposed.catch(() => {});
          return disposed;
        };
        active.add(run);
        if (controller.signal.aborted) {
          try { await run.dispose(); } catch { throw failure('WEB_CLEANUP_UNPROVEN'); }
          throw failure('ROW_CANCELLED');
        }
        return run;
      } finally { openings.delete(pending); }
    };
    result = await Promise.race([(seams.dispatchScenario ?? dispatchScenario)(scenario, ctx), cancelled]);
  } catch (error) {
    result = { status: controller.signal.aborted ? 'incomplete' : 'failed',
      barrierCode: controller.signal.aborted ? (controller.signal.reason?.code ?? 'ROW_CANCELLED') :
        (error.code ?? 'PROVISIONING_FAILED') };
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', abort);
    controller.signal.removeEventListener('abort', onAbort);
    // Closing the owned browser/gateway also interrupts outstanding browser work.
    let cleanupTimer;
    const disposal = await Promise.race([
      Promise.allSettled([...active].map(run => run.dispose())),
      new Promise(resolveTimeout => {
        cleanupTimer = setTimeout(() => resolveTimeout([{ status: 'rejected' }]), timeoutMs);
      }),
    ]);
    clearTimeout(cleanupTimer);
    if (openings.size || cleanupFailures.length || disposal.some(item => item.status === 'rejected'))
      result = { ...result, status: 'incomplete', barrierCode: 'WEB_CLEANUP_UNPROVEN' };
    result = { ...result, webCleanup: { pendingOpens: openings.size,
      failures: [...cleanupFailures], completed: !openings.size && !cleanupFailures.length &&
        disposal.every(item => item.status === 'fulfilled') } };
    try {
      const cleanup = await (seams.teardownOwnedChildren ?? teardownOwnedChildren)(ledger);
      result = { ...result, cleanup };
      if (!cleanup.allExited) result = { ...result, status: 'incomplete', barrierCode: 'OWNED_EXIT_UNPROVEN' };
    } catch (error) {
      result = { ...result, status: 'incomplete', barrierCode: 'OWNED_CLEANUP_FAILED', cleanupError: error.code ?? error.name };
    }
  }
  return persist({ ...result, ...base, candidate: provenance });
}
