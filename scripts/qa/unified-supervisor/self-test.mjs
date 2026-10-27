// --self-test: drives the runner as child processes against a throwaway fixture
// config and audits what it wrote. Every guard in harness.mjs and the CLI has a
// case here that fails when that guard is removed.
import { randomUUID } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join, relative } from 'node:path';
import {
  collectReceipts, CONTROL_ENV, errorInfo, EXIT, Evidence, identityVerdict, isSha256, Journal, Ledger, Owned, PLATFORM, POSIX, probeProcess,
  reapMember, sameStartStamp, SCHEMA, sha256, withDeadline,
} from './harness.mjs';
import { SCENARIOS, SLEEPER } from './scenarios.mjs';
import { unitChecks } from './self-test-units.mjs';

export const NONCE_ENV = 'UNIFIED_SUPERVISOR_SELFTEST_NONCE';
export const NONCE_FILE = 'selftest-nonce';
const CHILD_DEADLINE_MS = 120_000;
const ARM_DEADLINE_MS = 60_000;
// The refusal, not-implemented, run-lock and stale-evidence steps need a registered scenario
// that is still only planned: the first one, so implementing a scenario never silently runs
// it against the self-test's fake binaries.
const PLANNED = Object.entries(SCENARIOS).find(([, scenario]) => !scenario.implemented)?.[0];
if (!PLANNED) throw new Error('self-test needs at least one planned scenario');

function readJson(path) {
  try { return JSON.parse(readFileSync(path, 'utf8')); } catch { return null; }
}

function readLines(path) {
  try { return readFileSync(path, 'utf8').split('\n').filter(Boolean); } catch { return []; }
}

function armedFrom(lines) {
  for (const line of lines) {
    try {
      const value = JSON.parse(line);
      if (value?.selftest === 'armed') return value;
    } catch { /* plain text */ }
  }
  return null;
}

// Signal one owned child runner: its pid only, or its whole process group as a
// terminal Ctrl-C would. The runner's handlers and teardown are under test.
function signalChild(handle, signal, { group = false } = {}) {
  if (handle.exitInfo) return;
  try {
    if (group && POSIX) process.kill(-handle.pid, signal);
    else handle.child.kill(signal);
  } catch { /* exited between the check and the signal */ }
}

// Interrupt one owned child runner. POSIX: SIGINT to its pid or (group) its
// whole process group. Windows cannot deliver SIGINT to a child
// (child.kill('SIGINT') is TerminateProcess), so the runner's stdin control
// channel carries the interrupt there; the child must run with control: true.
function interruptChild(handle, { group = false } = {}) {
  if (handle.exitInfo) return;
  if (POSIX) { signalChild(handle, 'SIGINT', { group }); return; }
  try { handle.child.stdin?.write('interrupt\n'); } catch { /* exited between the check and the write */ }
}

function scanForPlants(root, plants) {
  const hits = [];
  const needles = plants.map((plant) => Buffer.from(plant, 'utf8'));
  const walk = (dir) => {
    let entries;
    try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
    for (const entry of entries) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.isFile()) {
        try {
          if (statSync(full).size > 64 * 1024 * 1024) { hits.push({ file: relative(root, full), reason: 'too large to scan' }); continue; }
          const bytes = readFileSync(full);
          needles.forEach((needle, index) => { if (bytes.includes(needle)) hits.push({ file: relative(root, full), plant: index }); });
        } catch (error) { hits.push({ file: relative(root, full), reason: String(error?.message ?? error) }); }
      }
    }
  };
  walk(root);
  return hits;
}

// Config files and the nonce live in evidence; state roots live under a short
// /tmp path so the daemon-socket length preflight holds for the fixture too.
function makeFixture(evidence, nonce) {
  const configDir = join(evidence.dir, 'fixture');
  mkdirSync(configDir, { recursive: true });
  writeFileSync(join(configDir, NONCE_FILE), `${nonce}\n`);
  const stateRoot = join(POSIX ? '/tmp' : tmpdir(), `usqa-${randomUUID().slice(0, 8)}`);
  mkdirSync(stateRoot, { recursive: true, mode: 0o700 });
  const dataRoot = join(stateRoot, 'data');
  const memoryRoot = join(stateRoot, 'memory');
  const projectRoot = join(stateRoot, 'project');
  const foreignRoot = join(stateRoot, 'foreign-data');
  mkdirSync(projectRoot, { recursive: true });
  mkdirSync(foreignRoot, { recursive: true });
  writeFileSync(join(foreignRoot, 'state.json'), '{}\n');
  const good = {
    $comment: 'Self-test fixture: the binary fields point at the Bun runtime because self-test scenarios exercise the harness, not Ferryx or mhc.',
    ferryxCli: process.execPath, ferryxApp: process.execPath, mhcBin: process.execPath,
    dataRoot, memoryRoot, projectRoot, ownerPrincipal: 'qa-owner',
  };
  const write = (name, value) => {
    const path = join(configDir, name);
    writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
    return path;
  };
  const missing = { ...good };
  delete missing.mhcBin;
  // [label, config path, expected error code, extra stderr substring]
  const refusals = [
    ['config-missing-field', write('config.missing-field.json', missing), 'CONFIG_INVALID'],
    ['config-unknown-field', write('config.unknown-field.json', { ...good, engine: 'other' }), 'CONFIG_INVALID'],
    ['config-production-data-root', write('config.production-data-root.json', { ...good, dataRoot: join(homedir(), '.ferryx') }), 'PRODUCTION_PATH_REFUSED'],
    ['config-home-memory-root', write('config.home-memory-root.json', { ...good, memoryRoot: homedir() }), 'PRODUCTION_PATH_REFUSED'],
    ['config-hub-sqlite', write('config.hub-sqlite.json', { ...good, projectRoot: join(stateRoot, 'hub', 'hub.sqlite') }), 'PRODUCTION_PATH_REFUSED'],
    // POSIX: the daemon socket's sun_path limit. Windows: the daemon.port endpoint under the MAX_PATH budget.
    ['config-socket-too-long', write('config.socket-too-long.json', { ...good, dataRoot: POSIX ? join(stateRoot, 'd'.repeat(90)) : join(stateRoot, 'd'.repeat(100), 'd'.repeat(100)) }),
      'CONFIG_INVALID', POSIX ? 'daemon socket' : 'daemon endpoint'],
    ['config-foreign-data-root', write('config.foreign-data-root.json', { ...good, dataRoot: foreignRoot }), 'FOREIGN_STATE_ROOT'],
  ];
  const uid = process.getuid?.();
  if (POSIX && Number.isInteger(uid)) {
    refusals.push(['config-daemon-runtime-root', write('config.daemon-runtime-root.json', { ...good, dataRoot: join('/tmp', `ferryx-${uid}`, 'qa') }), 'PRODUCTION_PATH_REFUSED']);
  }
  return { configDir, stateRoot, dataRoot, memoryRoot, ledger: Ledger.forDataRoot(dataRoot), good: write('config.good.json', good), refusals, lockedDirs: [] };
}

export async function selfTest(opts, interrupts, { script, repoRoot, scripts, writeOut }) {
  const evidence = Evidence.prepare(opts.evidence, { force: opts.forceEvidence });
  const runId = `selftest-${Date.now().toString(36)}-${process.pid.toString(36)}`;
  const journal = new Journal(evidence);
  const ledger = new Ledger(join(evidence.dir, 'parent-ledger'));
  const owned = new Owned({ runId, ledger, evidence, journal, runnerStamp: (await probeProcess(process.pid)).startStamp ?? null });
  const checks = [];
  const safetyNet = [];
  const plants = { slack: `xoxb-${randomUUID()}`, keyed: `plain-${randomUUID()}` };
  const nonce = `${randomUUID()}${randomUUID()}`.replace(/-/g, '');
  let fx = null;
  const check = (name, ok, detail = null) => {
    const entry = { name, ok: ok === true, detail };
    checks.push(entry);
    evidence.log(`check ${name} ${entry.ok ? 'ok' : 'FAILED'}`);
    return entry.ok;
  };
  const runChild = async (label, args, { env = {}, onArmed, control = false } = {}) => {
    const id = `child-${label}`;
    const cursor = journal.cursor();
    const childEnv = { ...process.env, [NONCE_ENV]: nonce, ...env };
    if (control) childEnv[CONTROL_ENV] = 'stdin'; else delete childEnv[CONTROL_ENV];
    const handle = await owned.spawn(id, process.execPath, [script, ...args], { env: childEnv, graceMs: 20_000, cwd: repoRoot, stdin: control ? 'pipe' : 'ignore' });
    handle.child.stdin?.on('error', () => { /* the exit event decides */ });
    let armed = null;
    if (onArmed) {
      const event = await journal.waitFor(`${label} armed marker`, (e) => e.source === id && e.kind === 'stdout' && e.json?.selftest === 'armed', { after: cursor, deadlineMs: ARM_DEADLINE_MS, signal: interrupts.signal });
      armed = event.json;
      await onArmed(armed, handle, cursor);
    }
    const exit = await journal.waitFor(`${label} exit`, (e) => e.source === id && e.kind === 'exit', { after: cursor, deadlineMs: CHILD_DEADLINE_MS, signal: interrupts.signal });
    await withDeadline(handle.closed, 5_000, () => false);
    const lines = journal.events.filter((e) => e.source === id && e.seq > cursor && (e.kind === 'stdout' || e.kind === 'stderr')).map((e) => e.line);
    return { code: exit.code, signal: exit.signal, lines, armed: armed ?? armedFrom(lines), handle };
  };
  const readChild = (dir) => ({ assertions: readJson(join(dir, 'assertions.json')), cleanup: readJson(join(dir, 'cleanup.json')), run: readJson(join(dir, 'run.json')) });
  const goneNow = async (pid, startStamp) => Number.isSafeInteger(pid) && Boolean(startStamp)
    && ['gone', 'mismatch'].includes(identityVerdict({ pid, startStamp }, await probeProcess(pid)).verdict);
  const receiptFor = (cleanup, id) => cleanup?.processes?.find((entry) => entry.id === id) ?? null;
  // A child that failed its own teardown must not leak a fixture process into
  // later steps: reap it here by recorded start stamp and report that it was needed.
  const sweepLedger = async (label) => {
    if (!fx) return;
    for (const record of fx.ledger.open().filter((r) => r.type === 'spawn' && !String(r.key).startsWith('forged-'))) {
      const probe = await probeProcess(record.pid);
      if (sameStartStamp(record, probe) && identityVerdict(record, probe).verdict === 'match') {
        safetyNet.push({ label, key: record.key, ...(await reapMember({ pid: record.pid, startStamp: record.startStamp })) });
      }
    }
  };
  const step = async (name, fn, { sweep = true } = {}) => {
    try { await fn(); } catch (error) {
      check(`${name}-completed`, false, errorInfo(error));
      if (error?.code === 'INTERRUPTED') throw error;
    }
    if (sweep) await sweepLedger(name);
  };

  try {
    const receipts = await collectReceipts({ repoRoot, scripts });
    evidence.writeJson('receipts.json', receipts);
    fx = makeFixture(evidence, nonce);
    const children = join(evidence.dir, 'children');
    const internal = ['--internal-self-test-child'];
    const childDir = (name) => join(children, name);
    const scenarioArgs = (scenario, dir) => [...internal, '--scenario', scenario, '--config', fx.good, '--evidence', dir];
    const stderrOf = (result) => result.lines.join('\n');

    await step('unit-checks', () => unitChecks(check, fx.stateRoot), { sweep: false });

    await step('refusals', async () => {
      for (const [label, config, code, needle] of fx.refusals) {
        const dir = childDir(label);
        const result = await runChild(label, ['--scenario', PLANNED, '--config', config, '--evidence', dir]);
        const text = stderrOf(result);
        check(`refuses-${label}`, result.code === EXIT.USAGE && text.includes(code) && (!needle || text.includes(needle)) && !existsSync(dir),
          { code: result.code, expected: code, evidenceCreated: existsSync(dir), tail: result.lines.slice(-3) });
      }
      const cases = [
        ['missing-config', ['--scenario', PLANNED, '--evidence', childDir('missing-config')], 'CONFIG_INVALID', {}],
        ['ambiguous-force', ['--scenario', PLANNED, '--config', fx.good, '--evidence', childDir('ambiguous-force'), '--force'], 'USAGE', {}],
        ['force-owner-outside-cleanup', ['--scenario', PLANNED, '--config', fx.good, '--evidence', childDir('force-owner'), '--force-owner'], 'USAGE', {}],
        ['unknown-scenario', ['--scenario', 'no-such-scenario', '--config', fx.good, '--evidence', childDir('unknown')], 'UNKNOWN_SCENARIO', {}],
        ['hidden-scenario-without-internal-flag', ['--scenario', '__selftest-fail', '--config', fx.good, '--evidence', childDir('hidden')], 'UNKNOWN_SCENARIO', {}],
        ['internal-flag-without-nonce', scenarioArgs('__selftest-no-assertions', childDir('no-nonce')), 'INTERNAL_FLAG_REFUSED', { [NONCE_ENV]: '' }],
        ['internal-flag-wrong-nonce', scenarioArgs('__selftest-no-assertions', childDir('wrong-nonce')), 'INTERNAL_FLAG_REFUSED', { [NONCE_ENV]: 'f'.repeat(64) }],
        ['internal-flag-evidence-outside-children', [...internal, '--scenario', '__selftest-no-assertions', '--config', fx.good, '--evidence', join(evidence.dir, 'outside-children')], 'INTERNAL_FLAG_REFUSED', {}],
      ];
      for (const [label, args, code, env] of cases) {
        const dir = args[args.indexOf('--evidence') + 1];
        const result = await runChild(label, args, { env });
        check(`refuses-${label}`, result.code === EXIT.USAGE && stderrOf(result).includes(code) && !existsSync(dir),
          { code: result.code, expected: code, evidenceCreated: existsSync(dir), tail: result.lines.slice(-3) });
      }
    });

    await step('not-implemented', async () => {
      const dir = childDir('not-implemented');
      const result = await runChild('not-implemented', ['--scenario', PLANNED, '--config', fx.good, '--evidence', dir]);
      const { assertions, cleanup, run } = readChild(dir);
      check('planned-scenario-reports-not-implemented', result.code === EXIT.NOT_IMPLEMENTED && assertions?.status === 'NOT_IMPLEMENTED' && cleanup?.ok === true,
        { code: result.code, status: assertions?.status ?? null });
      check('run-id-is-short', typeof run?.runId === 'string' && run.runId.length <= 24, { runId: run?.runId ?? null });
      const r = readJson(join(dir, 'receipts.json'));
      const binaries = Object.values(r?.binaries ?? {});
      check('receipts-record-git-binaries-and-config', /^[0-9a-f]{40}$/.test(r?.git?.rev ?? '') && isSha256(r?.git?.diffSha256) && isSha256(r?.git?.untrackedSha256)
        && isSha256(r?.git?.statusSha256) && binaries.length === 3 && binaries.every((b) => isSha256(b.sha256)) && Object.keys(r?.binaryRepos ?? {}).length === 3
        && isSha256(r?.config?.sha256) && isSha256(r?.runner?.scripts?.['scripts/qa/unified-supervisor.mjs']),
      { git: r?.git ?? null, binaryRepos: r?.binaryRepos ?? null, config: r?.config ?? null });
      const expected = ['run.json', 'receipts.json', 'config.redacted.json', 'requests.jsonl', 'responses.jsonl', 'events.jsonl', 'owned.jsonl', 'assertions.json', 'cleanup.json', 'logs/runner.log'];
      const missingFiles = expected.filter((file) => file !== 'owned.jsonl' && !existsSync(join(dir, file)));
      check('per-scenario-evidence-layout', missingFiles.length === 0, { missingFiles });
    });

    await step('all', async () => {
      const dir = childDir('all');
      const result = await runChild('all', [...internal, '--all', '--config', fx.good, '--evidence', dir]);
      const summary = readJson(join(dir, 'summary.json'));
      const results = summary?.results ?? [];
      check('all-mode-never-passes-registered-only-scenarios', result.code !== EXIT.PASS && summary?.status !== 'PASS'
        && results.length === Object.keys(SCENARIOS).length && results.every((entry) => entry.status !== 'PASS'),
      { code: result.code, status: summary?.status ?? null, count: results.length });
    });

    await step('run-lock', async () => {
      const lockDir = join(fx.dataRoot, '.qa-unified-supervisor');
      const lockFile = join(lockDir, 'run.lock');
      mkdirSync(lockDir, { recursive: true });
      // A live holder (this self-test parent) must refuse a second run on the same dataRoot.
      writeFileSync(lockFile, `${JSON.stringify({ pid: process.pid, startStamp: owned.runnerStamp, mode: 'self-test-holder', at: new Date().toISOString(), nonce: 'held' })}\n`);
      const heldDir = childDir('run-lock-held');
      const held = await runChild('run-lock-held', ['--scenario', PLANNED, '--config', fx.good, '--evidence', heldDir]);
      check('concurrent-run-refused-by-run-lock', held.code === EXIT.USAGE && stderrOf(held).includes('RUN_LOCKED') && !existsSync(heldDir) && existsSync(lockFile),
        { code: held.code, tail: held.lines.slice(-3) });
      // A holder that no longer exists is taken over, recorded, and the lock is released afterwards.
      writeFileSync(lockFile, `${JSON.stringify({ pid: 2_000_000_000, startStamp: null, mode: 'crashed-holder', at: new Date().toISOString(), nonce: 'dead' })}\n`);
      const staleDir = childDir('run-lock-stale');
      const taken = await runChild('run-lock-stale', ['--scenario', PLANNED, '--config', fx.good, '--evidence', staleDir]);
      check('stale-run-lock-taken-over-and-released', taken.code === EXIT.NOT_IMPLEMENTED && existsSync(join(staleDir, 'run-lock-takeover.json')) && !existsSync(lockFile),
        { code: taken.code, tail: taken.lines.slice(-3) });
    });

    await step('positive-control', async () => {
      const dir = childDir('positive');
      const result = await runChild('positive', scenarioArgs('__selftest-hung-command', dir));
      const { assertions, cleanup } = readChild(dir);
      check('positive-control-passes', result.code === EXIT.PASS && assertions?.status === 'PASS' && cleanup?.ok === true,
        { code: result.code, status: assertions?.status ?? null, failed: assertions?.assertions?.filter((a) => !a.ok) ?? null, error: assertions?.error ?? null });
      check('cli-requests-and-responses-recorded', readLines(join(dir, 'requests.jsonl')).length === 1 && readLines(join(dir, 'responses.jsonl')).length === 1, null);
    });

    await step('failing-child', async () => {
      const dir = childDir('failing');
      const result = await runChild('failing', scenarioArgs('__selftest-fail', dir), {
        env: { UNIFIED_SUPERVISOR_SELFTEST_PLANT_SLACK: plants.slack, UNIFIED_SUPERVISOR_SELFTEST_PLANT_KEYED: plants.keyed },
      });
      const { assertions, cleanup } = readChild(dir);
      const receipt = receiptFor(cleanup, 'sleeper');
      const grandchild = receipt?.descendants?.find((entry) => entry.pid === result.armed?.grandchildPid) ?? null;
      check('failing-assertion-fails-the-run', result.code === EXIT.FAIL && assertions?.status === 'FAIL'
        && assertions?.assertions?.some((a) => a.name === 'worker-result-evidence-present' && a.ok === false) === true, { code: result.code, status: assertions?.status ?? null });
      check('misleading-success-output-does-not-pass', result.lines.some((line) => line.includes('PASS: all assertions passed')) && result.code !== EXIT.PASS, { code: result.code });
      check('sigterm-ignoring-process-escalated-and-reaped', receipt?.ok === true && receipt.exited === true && receipt.signals?.some((s) => s.signal === 'SIGKILL') === true
        && grandchild?.finalState === 'gone', { receipt });
      check('failing-run-processes-gone', await goneNow(result.armed?.sleeperPid, receipt?.startStamp) && await goneNow(result.armed?.grandchildPid, grandchild?.startStamp),
        { sleeperPid: result.armed?.sleeperPid ?? null, grandchildPid: result.armed?.grandchildPid ?? null });
    });

    await step('pass-gating', async () => {
      const gating = [
        ['no-assertions', '__selftest-no-assertions', EXIT.FAIL, 'FAIL', 'NO_ASSERTIONS'],
        ['throw', '__selftest-throw', EXIT.HARNESS, 'HARNESS_ERROR', 'deliberate ordinary error'],
        ['missing-required', '__selftest-missing-required', EXIT.FAIL, 'FAIL', 'MISSING_ASSERTIONS'],
        ['late-assertion', '__selftest-late-assertion', EXIT.FAIL, 'FAIL', 'LATE_ASSERTION'],
      ];
      for (const [label, scenario, exitCode, status, needle] of gating) {
        const dir = childDir(label);
        const result = await runChild(label, scenarioArgs(scenario, dir));
        const { assertions, cleanup } = readChild(dir);
        check(`gating-${label}`, result.code === exitCode && assertions?.status === status && JSON.stringify(assertions?.error ?? null).includes(needle) && cleanup?.ok === true,
          { code: result.code, status: assertions?.status ?? null, error: assertions?.error ?? null, cleanupOk: cleanup?.ok ?? null });
      }
    });

    await step('deadline', async () => {
      const dir = childDir('deadline');
      const result = await runChild('deadline', scenarioArgs('__selftest-hang', dir), { env: { UNIFIED_SUPERVISOR_SELFTEST_HANG_MS: '3000' } });
      const { assertions, cleanup } = readChild(dir);
      const receipt = receiptFor(cleanup, 'sleeper');
      check('step-deadline-ends-run', result.code === EXIT.HARNESS && assertions?.status === 'DEADLINE_EXCEEDED' && assertions?.error?.code === 'DEADLINE_EXCEEDED',
        { code: result.code, status: assertions?.status ?? null });
      check('deadline-run-cleaned-up', cleanup?.ok === true && await goneNow(result.armed?.sleeperPid, receipt?.startStamp), { receipt });
    });

    await step('interrupt', async () => {
      const dir = childDir('interrupt');
      const result = await runChild('interrupt', scenarioArgs('__selftest-hang', dir), {
        control: !POSIX,
        env: { UNIFIED_SUPERVISOR_SELFTEST_SLEEPER_IGNORE_TERM: '1', UNIFIED_SUPERVISOR_SELFTEST_HANG_MS: '120000' },
        onArmed: async (_armed, handle, cursor) => {
          const cleaning = journal.waitFor('interrupt child cleanup phase', (e) => e.source === handle.id && e.json?.unifiedSupervisor?.phase === 'cleanup',
            { after: cursor, deadlineMs: ARM_DEADLINE_MS, signal: interrupts.signal });
          cleaning.catch(() => { /* awaited below */ });
          interruptChild(handle);
          await cleaning;
          // Ctrl-C again (POSIX: to the whole process group) while teardown is under way.
          interruptChild(handle, { group: true });
          interruptChild(handle, { group: true });
        },
      });
      const { assertions, cleanup } = readChild(dir);
      const receipt = receiptFor(cleanup, 'sleeper');
      check('interrupted-run-exits-130', result.code === EXIT.INTERRUPTED && assertions?.status === 'INTERRUPTED', { code: result.code, status: assertions?.status ?? null });
      check('group-interrupt-during-cleanup-does-not-abort-it', cleanup?.ok === true && receipt?.ok === true && receipt.signals?.some((s) => s.signal === 'SIGKILL') === true
        && (cleanup?.interruptsDuringCleanup ?? 0) >= 1, { interrupts: cleanup?.interrupts ?? null, duringCleanup: cleanup?.interruptsDuringCleanup ?? null, receipt });
      check('interrupted-run-sleeper-really-gone', await goneNow(result.armed?.sleeperPid, receipt?.startStamp), { sleeperPid: result.armed?.sleeperPid ?? null });
    });

    await step('stale-evidence', async () => {
      const dir = childDir('not-implemented');
      const before = existsSync(join(dir, 'run.json')) ? sha256(readFileSync(join(dir, 'run.json'))) : null;
      const refused = await runChild('stale-evidence', ['--scenario', PLANNED, '--config', fx.good, '--evidence', dir]);
      const after = existsSync(join(dir, 'run.json')) ? sha256(readFileSync(join(dir, 'run.json'))) : null;
      check('stale-evidence-dir-refused', refused.code === EXIT.USAGE && refused.lines.join('\n').includes('EVIDENCE_DIR_NOT_EMPTY') && before !== null && before === after,
        { code: refused.code, unchanged: before === after });
      const forced = await runChild('stale-evidence-forced', ['--scenario', PLANNED, '--config', fx.good, '--evidence', dir, '--force-evidence']);
      const moved = readdirSync(children).some((name) => name.startsWith('not-implemented.prior-'));
      check('force-evidence-moves-prior-run-aside', forced.code === EXIT.NOT_IMPLEMENTED && moved, { code: forced.code, moved });
    });

    await step('cleanup-fail', async () => {
      const dir = childDir('cleanup-fail');
      const result = await runChild('cleanup-fail', scenarioArgs('__selftest-cleanup-fail', dir));
      if (result.armed?.locked) fx.lockedDirs.push(result.armed.locked);
      const { assertions, cleanup } = readChild(dir);
      const effective = POSIX && process.getuid?.() !== 0;
      const failedPath = cleanup?.paths?.find((entry) => entry.label === 'data' && entry.status === 'remove-failed') ?? null;
      check('cleanup-failure-overrides-pass', effective
        ? result.code === EXIT.CLEANUP && assertions?.status === 'CLEANUP_FAILED' && cleanup?.ok === false && failedPath !== null
        : [EXIT.PASS, EXIT.CLEANUP].includes(result.code),
      { effective, code: result.code, status: assertions?.status ?? null, failedPath, note: effective ? null : 'read-only directories do not block removal for root or on win32' });
    });

    await step('faults', async () => {
      for (const kind of ['rejection', 'exception']) {
        const dir = childDir(`fault-${kind}`);
        const result = await runChild(`fault-${kind}`, scenarioArgs('__selftest-fault', dir), { env: { UNIFIED_SUPERVISOR_SELFTEST_FAULT: kind } });
        const { assertions, cleanup } = readChild(dir);
        const receipt = receiptFor(cleanup, 'sleeper');
        check(`uncaught-${kind}-is-harness-error-with-teardown`, result.code === EXIT.HARNESS && assertions?.status === 'HARNESS_ERROR'
          && (cleanup?.faults?.length ?? 0) >= 1 && cleanup?.ok === true && await goneNow(result.armed?.sleeperPid, receipt?.startStamp),
        { code: result.code, status: assertions?.status ?? null, faults: cleanup?.faults ?? null, receipt });
      }
    });

    await step('orphan-cleanup', async () => {
      const dir = childDir('orphan');
      const result = await runChild('orphan', scenarioArgs('__selftest-orphan', dir), {
        onArmed: async (_armed, handle) => { signalChild(handle, 'SIGKILL'); },
      });
      const orphanPid = result.armed?.sleeperPid ?? null;
      const scriptPid = result.armed?.scriptPid ?? null;
      check('orphan-runner-killed-without-teardown', result.signal === 'SIGKILL' && !existsSync(join(dir, 'cleanup.json')), { signal: result.signal });
      const record = fx.ledger.open().find((r) => r.type === 'spawn' && r.pid === orphanPid) ?? null;
      const survived = record ? identityVerdict(record, await probeProcess(orphanPid)) : { verdict: 'no-ledger-record' };
      check('orphan-survives-and-is-ledgered-with-stamp', survived.verdict === 'match' && Boolean(record?.startStamp), { orphanPid, survived });
      const scriptRecord = scriptPid === null ? null : fx.ledger.open().find((r) => r.type === 'spawn' && r.pid === scriptPid) ?? null;
      if (POSIX) {
        // The kernel shows /bin/sh first for a #! script; the argv observed at spawn must still identify it.
        const scriptSurvived = scriptRecord ? identityVerdict(scriptRecord, await probeProcess(scriptPid)) : { verdict: 'no-ledger-record' };
        check('orphan-shebang-script-identified-by-observed-argv', scriptSurvived.verdict === 'match' && Boolean(scriptRecord?.startStamp)
          && (Array.isArray(scriptRecord?.observedArgv) || typeof scriptRecord?.observedCommand === 'string'),
        { scriptPid, scriptSurvived, observedArgv: scriptRecord?.observedArgv ?? null, observedCommand: scriptRecord?.observedCommand ?? null });
      }

      // A foreign process the ledger only claims through forged records.
      const cursor = journal.cursor();
      const foreign = await owned.spawn('foreign-process', process.execPath, [SLEEPER], { env: { PATH: process.env.PATH ?? '' }, graceMs: 2_000 });
      await journal.waitFor('foreign process ready', (e) => e.source === 'foreign-process' && e.json?.ready === true, { after: cursor, deadlineMs: 20_000, signal: interrupts.signal });
      const base = { type: 'spawn', pid: foreign.pid, executable: process.execPath, detached: POSIX, graceMs: 1_000, runnerPid: null, runnerStartStamp: null, runId: `forged-${runId}` };
      const realStamp = foreign.record.startStamp;
      const forged = {
        // Reused pid: a different start stamp (and argv) is provably another process: released untouched.
        argvMismatch: { ...base, key: `forged-${runId}/proc/argv#1`, id: 'forged-argv', args: ['--not-the-recorded-launch'], startStamp: `${realStamp}-earlier`, spawnedAtMs: foreign.record.spawnedAtMs },
        // Same process (equal stamp) whose argv no longer matches: possibly ours after exec, so never signalled and never released.
        argvChanged: { ...base, key: `forged-${runId}/proc/argvchanged#2`, id: 'forged-argvchanged', args: ['--not-the-recorded-launch'], startStamp: realStamp, spawnedAtMs: foreign.record.spawnedAtMs },
        noStampRecent: { ...base, key: `forged-${runId}/proc/nostamp#3`, id: 'forged-nostamp', args: [SLEEPER], startStamp: null, spawnedAtMs: foreign.record.spawnedAtMs },
        noStampOld: { ...base, key: `forged-${runId}/proc/old#4`, id: 'forged-old', args: [SLEEPER], startStamp: null, spawnedAtMs: foreign.record.spawnedAtMs - 3_600_000 },
        // Verified identity, but its owning runner (this self-test parent) is alive.
        liveOwner: { ...base, key: `forged-${runId}/proc/liveowner#5`, id: 'forged-liveowner', args: [SLEEPER], startStamp: realStamp, spawnedAtMs: foreign.record.spawnedAtMs,
          runnerPid: process.pid, runnerStartStamp: owned.runnerStamp },
        // Its owning runner saw this pid exit before a stamp could be read; the live process now at that pid is another one.
        exitedEarly: { ...base, key: `forged-${runId}/proc/exitedearly#6`, id: 'forged-exitedearly', args: [SLEEPER], startStamp: null, spawnedAtMs: foreign.record.spawnedAtMs },
      };
      for (const value of Object.values(forged)) fx.ledger.append(value);
      fx.ledger.append({ type: 'exited-before-stamp', key: forged.exitedEarly.key, startStamp: null, probe: 'gone', exit: { code: 0, signal: null, at: new Date().toISOString() } });
      const decoyBase = join(fx.stateRoot, 'decoy');
      const decoy = { type: 'path', key: 'forged-decoy/path/data', runId: 'qa-decoy-x-1', label: 'decoy', base: decoyBase, path: join(decoyBase, 'runs', 'qa-decoy-x-1') };
      mkdirSync(decoy.path, { recursive: true });
      writeFileSync(join(decoy.path, 'keep.txt'), 'not owned by the configured roots\n');
      fx.ledger.append(decoy);

      const staleDir = childDir('stale-preflight');
      const stale = await runChild('stale-preflight', scenarioArgs('__selftest-no-assertions', staleDir));
      check('stale-owned-state-refused-before-run', stale.code === EXIT.USAGE && stale.lines.join('\n').includes('STALE_OWNED_STATE') && !existsSync(staleDir)
        && !stale.lines.join('\n').includes(forged.exitedEarly.key), { code: stale.code });

      for (const locked of fx.lockedDirs) { try { chmodSync(locked, 0o700); } catch { /* already removed */ } }
      const firstDir = childDir('cleanup-1');
      const first = await runChild('cleanup-1', ['--cleanup', '--config', fx.good, '--evidence', firstDir]);
      const report = readJson(join(firstDir, 'cleanup.json'));
      const entryFor = (key) => report?.entries?.find((entry) => entry.key === key) ?? null;
      const orphanEntry = report?.entries?.find((entry) => entry.kind === 'process' && entry.pid === orphanPid && entry.key === record?.key) ?? null;
      check('cleanup-terminates-verified-orphan', orphanEntry?.status === 'terminated' && orphanEntry.ok === true && orphanEntry.identity?.verdict === 'match'
        && await goneNow(orphanPid, record?.startStamp), { orphanEntry });
      const argvEntry = entryFor(forged.argvMismatch.key);
      const changedEntry = entryFor(forged.argvChanged.key);
      const recentEntry = entryFor(forged.noStampRecent.key);
      const oldEntry = entryFor(forged.noStampOld.key);
      const ownerEntry = entryFor(forged.liveOwner.key);
      const scriptEntry = scriptRecord ? entryFor(scriptRecord.key) : null;
      if (POSIX) {
        check('cleanup-terminates-owned-shebang-script', scriptEntry?.status === 'terminated' && scriptEntry.ok === true && scriptEntry.killed === true
          && await goneNow(scriptPid, scriptRecord?.startStamp), { scriptEntry });
      }
      check('cleanup-never-signals-argv-mismatch', argvEntry?.status === 'identity-mismatch' && argvEntry.killed === false && argvEntry.ok === true, { argvEntry });
      check('cleanup-leaves-equal-stamp-argv-change-open', changedEntry?.status === 'argv-changed' && changedEntry.killed === false && changedEntry.ok === false
        && (changedEntry.signals?.length ?? 1) === 0, { changedEntry });
      check('cleanup-never-signals-without-both-start-stamps', recentEntry?.status === 'unverifiable' && recentEntry.killed === false && recentEntry.ok === false, { recentEntry });
      check('cleanup-releases-pid-started-outside-spawn-window', oldEntry?.status === 'identity-mismatch' && oldEntry.killed === false, { oldEntry });
      check('cleanup-respects-live-owner-runner', ownerEntry?.status === 'owner-runner-alive' && ownerEntry.killed === false && ownerEntry.ok === false, { ownerEntry });
      const earlyEntry = entryFor(forged.exitedEarly.key);
      check('cleanup-releases-exited-before-stamp-record-untouched', earlyEntry?.status === 'already-gone' && earlyEntry.killed === false
        && (earlyEntry.signals?.length ?? 1) === 0 && earlyEntry.ok === true && foreign.exitInfo === null, { earlyEntry });
      check('foreign-process-survives-cleanup', foreign.exitInfo === null && identityVerdict(foreign.record, await probeProcess(foreign.pid)).verdict === 'match', null);
      const decoyEntry = entryFor(decoy.key);
      const pathEntries = (report?.entries ?? []).filter((entry) => entry.kind === 'path' && entry.key !== decoy.key);
      check('cleanup-refuses-path-outside-configured-roots', decoyEntry?.status === 'refused-outside-configured-roots' && existsSync(join(decoy.path, 'keep.txt')), { decoyEntry });
      check('cleanup-removes-orphaned-run-roots', pathEntries.length >= 2 && pathEntries.every((entry) => entry.status === 'removed'), pathEntries);
      check('incomplete-cleanup-exits-5', first.code === EXIT.CLEANUP && report?.ok === false, { code: first.code });

      fx.ledger.append({ type: 'released', key: forged.noStampRecent.key, reason: 'self-test: forged record withdrawn' });
      fx.ledger.append({ type: 'path-released', key: decoy.key });
      // Only the argv-changed record (dead-owner, still alive) and the live-owner record remain open.
      const changedDir = childDir('stale-preflight-argv-changed');
      const changedRun = await runChild('stale-preflight-argv-changed', ['--scenario', PLANNED, '--config', fx.good, '--evidence', changedDir]);
      const changedText = stderrOf(changedRun);
      check('preflight-counts-argv-changed-as-stale', changedRun.code === EXIT.USAGE && changedText.includes('STALE_OWNED_STATE') && changedText.includes(forged.argvChanged.key)
        && !changedText.includes(forged.liveOwner.key) && !existsSync(changedDir), { code: changedRun.code, tail: changedRun.lines.slice(-3) });
      fx.ledger.append({ type: 'released', key: forged.argvChanged.key, reason: 'self-test: forged record withdrawn' });

      const postDir = childDir('post-cleanup');
      const post = await runChild('post-cleanup', ['--scenario', PLANNED, '--config', fx.good, '--evidence', postDir]);
      check('preflight-skips-records-of-a-live-runner', post.code === EXIT.NOT_IMPLEMENTED && foreign.exitInfo === null, { code: post.code, tail: post.lines.slice(-3) });

      const secondDir = childDir('cleanup-2');
      const second = await runChild('cleanup-2', ['--cleanup', '--config', fx.good, '--evidence', secondDir]);
      const secondOwner = readJson(join(secondDir, 'cleanup.json'))?.entries?.find((entry) => entry.key === forged.liveOwner.key) ?? null;
      check('cleanup-without-force-owner-exits-5-and-spares-live-owner', second.code === EXIT.CLEANUP && secondOwner?.status === 'owner-runner-alive'
        && secondOwner.killed === false && secondOwner.ok === false && foreign.exitInfo === null, { code: second.code, secondOwner });

      const thirdDir = childDir('cleanup-3-force-owner');
      const third = await runChild('cleanup-3-force-owner', ['--cleanup', '--config', fx.good, '--evidence', thirdDir, '--force-owner']);
      const thirdReport = readJson(join(thirdDir, 'cleanup.json'));
      const thirdOwner = thirdReport?.entries?.find((entry) => entry.key === forged.liveOwner.key) ?? null;
      const foreignExit = await withDeadline(foreign.exited, 10_000, () => null);
      check('force-owner-reclaims-live-owner-record', third.code === EXIT.PASS && thirdReport?.ok === true && thirdOwner?.status === 'terminated'
        && thirdOwner.killed === true && foreignExit !== null, { code: third.code, thirdOwner, foreignExit });
      const foreignReceipt = await owned.terminate(foreign, 'self-test');
      check('foreign-process-torn-down-by-its-owner', foreignReceipt.ok === true, foreignReceipt);
      rmSync(decoyBase, { recursive: true, force: true });
    }, { sweep: true });

    await step('ledger-audit', async () => {
      const { records, invalidLines } = fx.ledger.read();
      const open = fx.ledger.open();
      const stamped = new Set(records.filter((r) => r.type === 'stamp' && r.startStamp).map((r) => r.key));
      // A spawn without a stamp is honest only when its own exit event was ledgered before the probe could read it.
      const exitedEarly = new Set(records.filter((r) => r.type === 'exited-before-stamp' && r.exit && (r.exit.code !== null || r.exit.signal !== null)).map((r) => r.key));
      const spawns = records.filter((r) => r.type === 'spawn' && !String(r.key).startsWith('forged-'));
      const unstamped = spawns.filter((r) => !stamped.has(r.key) && !exitedEarly.has(r.key));
      check('fixture-ledger-fully-released', invalidLines === 0 && open.length === 0, { invalidLines, open });
      check('every-spawn-recorded-before-probe-then-stamped', spawns.length > 0 && spawns.some((r) => stamped.has(r.key))
        && spawns.every((r) => r.startStamp === null) && unstamped.length === 0,
      { spawns: spawns.length, unstamped: unstamped.map((r) => r.key), exitedBeforeStamp: spawns.filter((r) => exitedEarly.has(r.key)).map((r) => r.key) });
    }, { sweep: false });
  } catch (error) {
    if (error?.code !== 'INTERRUPTED') check('self-test-completed', false, errorInfo(error));
  }

  interrupts.phase = 'cleanup';
  const processes = await owned.teardown(interrupts.count > 0 ? 'interrupted' : 'teardown');
  let fixtureState = null;
  if (fx) {
    await sweepLedger('final');
    for (const locked of fx.lockedDirs) { try { chmodSync(locked, 0o700); } catch { /* already removed */ } }
    if (existsSync(fx.ledger.file)) evidence.writeText('fixture-ledger.jsonl', readFileSync(fx.ledger.file, 'utf8'));
    let error = null;
    try { rmSync(fx.stateRoot, { recursive: true, force: true }); } catch (caught) { error = errorInfo(caught); }
    fixtureState = { path: fx.stateRoot, removed: !existsSync(fx.stateRoot), error };
  }
  const cleanup = {
    schema: SCHEMA, mode: 'self-test', runId, ledger: ledger.file, finishedAt: new Date().toISOString(), interrupts: interrupts.count,
    interruptsDuringCleanup: interrupts.duringCleanup, received: interrupts.received, faults: interrupts.faults, processes, safetyNet, fixtureState,
    ok: processes.every((entry) => entry.ok) && safetyNet.every((entry) => entry.finalState === 'gone') && (fixtureState === null || fixtureState.removed),
  };
  evidence.writeJson('cleanup.json', cleanup);
  check('self-test-cleanup-ok', cleanup.ok, { processes: processes.length, fixtureState });
  check('no-safety-net-kill-needed', safetyNet.length === 0, safetyNet);
  check('no-uncaught-fault-in-self-test-parent', interrupts.faults.length === 0, interrupts.faults);
  const leaks = scanForPlants(evidence.dir, [plants.slack, plants.keyed]);
  check('no-planted-secret-anywhere-in-evidence', leaks.length === 0, leaks);

  const failed = checks.filter((entry) => !entry.ok).map((entry) => entry.name);
  const ok = interrupts.count === 0 && checks.length > 0 && failed.length === 0;
  const status = ok ? 'PASS' : interrupts.count > 0 ? 'INTERRUPTED' : 'FAIL';
  const exitCode = ok ? EXIT.PASS : interrupts.count > 0 ? EXIT.INTERRUPTED : EXIT.FAIL;
  evidence.writeJson('self-test.json', {
    schema: SCHEMA, status, exitCode, runId, platform: PLATFORM, checks, failed,
    plantSha256: [sha256(plants.slack), sha256(plants.keyed)], finishedAt: new Date().toISOString(),
  });
  await writeOut(process.stdout, `${JSON.stringify({ unifiedSupervisor: { mode: 'self-test', status, exitCode, checks: checks.length, failed, evidence: evidence.dir } })}\n`);
  return exitCode;
}
