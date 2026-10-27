#!/usr/bin/env bun
// Isolated integration driver for the unified autonomous supervisor
// (.omo/plans/unified-autonomous-supervisor.md, task 1).
//
//   bun scripts/qa/unified-supervisor.mjs --scenario NAME --config <json> --evidence <dir>
//   bun scripts/qa/unified-supervisor.mjs --all          --config <json> --evidence <dir>
//   bun scripts/qa/unified-supervisor.mjs --cleanup      --config <json> --evidence <dir> [--force]
//   bun scripts/qa/unified-supervisor.mjs --self-test    --evidence <dir>
//
// Exit codes: 0 PASS, 1 FAIL, 2 usage/config/evidence refusal, 3 NOT_IMPLEMENTED,
// 4 deadline or harness error, 5 cleanup incomplete, 130 interrupted.
//
// Contracts: assertions decide PASS, never log text; a scenario that is only
// registered reports NOT_IMPLEMENTED; production paths (~/.ferryx, any
// hub.sqlite, mhc/ferryx app-data roots) and missing config fields are refused
// before anything is written; an evidence directory from a prior run is
// refused unless --force moves it aside; every spawned process is recorded and
// torn down by recorded identity, and cleanup.json is written on interrupts.
// Config fields: see scripts/qa/unified-supervisor/config.schema.json.
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import { randomUUID } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import {
  aggregateExit, cleanupMode, collectReceipts, errorInfo, EXIT, Evidence, exitCodeForError, HarnessError, identityVerdict,
  isSha256, Journal, Ledger, loadConfig, Owned, POSIX, probeProcess, reapMember, redact, redactValue, runScenario, SCHEMA, sha256, stamp, withDeadline,
} from './unified-supervisor/harness.mjs';
import { resolveScenario, SCENARIOS, SLEEPER } from './unified-supervisor/scenarios.mjs';

const SCRIPT = fileURLToPath(import.meta.url);
const QA_DIR = join(dirname(SCRIPT), 'unified-supervisor');
const REPO_ROOT = resolve(dirname(SCRIPT), '..', '..');
const SCRIPTS = [SCRIPT, join(QA_DIR, 'harness.mjs'), join(QA_DIR, 'scenarios.mjs'), join(QA_DIR, 'fixture-sleeper.mjs'), join(QA_DIR, 'config.schema.json')];
const USAGE = 'usage: bun scripts/qa/unified-supervisor.mjs (--scenario NAME | --all | --self-test | --cleanup) [--config <json>] --evidence <dir> [--force]';

function writeOut(stream, text) {
  return withDeadline(new Promise((resolvePromise) => { stream.write(text, () => resolvePromise(true)); }), 2_000, () => false);
}

export function parseCli(argv) {
  const opts = { mode: null, scenario: null, config: null, evidence: null, force: false, internal: false };
  const modes = [];
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const value = () => {
      const next = argv[index + 1];
      if (next === undefined || next.startsWith('--')) throw new HarnessError('USAGE', `${arg} needs a value`);
      index += 1;
      return next;
    };
    switch (arg) {
      case '--scenario': modes.push('scenario'); opts.scenario = value(); break;
      case '--all': modes.push('all'); break;
      case '--self-test': modes.push('self-test'); break;
      case '--cleanup': modes.push('cleanup'); break;
      case '--config': opts.config = value(); break;
      case '--evidence': opts.evidence = value(); break;
      case '--force': opts.force = true; break;
      case '--internal-self-test-child': opts.internal = true; break;
      case '--help': case '-h': return { ...opts, mode: 'help' };
      default: throw new HarnessError('USAGE', `unknown argument ${arg}`);
    }
  }
  if (modes.length !== 1) throw new HarnessError('USAGE', 'choose exactly one of --scenario NAME, --all, --self-test, --cleanup');
  opts.mode = modes[0];
  if (!opts.evidence) throw new HarnessError('USAGE', '--evidence <dir> is required');
  if (opts.mode === 'self-test' && opts.config) throw new HarnessError('USAGE', '--self-test builds its own fixture config; do not pass --config');
  if (opts.mode !== 'self-test' && !opts.config) throw new HarnessError('CONFIG_INVALID', '--config <json> is required for this mode');
  return opts;
}

function installInterruptHandlers() {
  const controller = new AbortController();
  const state = { signal: controller.signal, count: 0, duringCleanup: 0, phase: 'run', received: [] };
  for (const name of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
    try {
      process.on(name, () => {
        state.count += 1;
        state.received.push({ signal: name, phase: state.phase, at: new Date().toISOString() });
        if (state.phase === 'cleanup') state.duringCleanup += 1;
        if (!controller.signal.aborted) controller.abort(new HarnessError('INTERRUPTED', `received ${name}`));
      });
    } catch { /* signal unsupported on this platform */ }
  }
  process.on('unhandledRejection', (reason) => {
    try { process.stderr.write(`unified-supervisor: unhandled rejection: ${redact(String(reason?.stack ?? reason))}\n`); } catch { /* stderr gone */ }
  });
  return state;
}

async function main(argv) {
  const interrupts = installInterruptHandlers();
  try {
    const opts = parseCli(argv);
    if (opts.mode === 'help') {
      await writeOut(process.stdout, `${USAGE}\nscenarios: ${Object.keys(SCENARIOS).join(', ')}\n`);
      return EXIT.PASS;
    }
    if (opts.mode === 'self-test') return await selfTest(opts, interrupts);
    const scenario = opts.mode === 'scenario' ? resolveScenario(opts.scenario, opts.internal) : null;
    // Refusals happen before any directory is created or any process started.
    const configInfo = loadConfig(opts.config);
    const evidence = Evidence.prepare(opts.evidence, { force: opts.force });
    evidence.log(`unified-supervisor mode=${opts.mode} pid=${process.pid} argv=${JSON.stringify(argv)}`);
    if (opts.mode === 'cleanup') {
      const exitCode = await cleanupMode({ configInfo, evidence, interrupts, force: opts.force });
      await writeOut(process.stdout, `${JSON.stringify({ unifiedSupervisor: { mode: 'cleanup', exitCode, evidence: evidence.dir } })}\n`);
      return exitCode;
    }
    const receipts = await collectReceipts({ repoRoot: REPO_ROOT, configInfo, scripts: SCRIPTS });
    if (opts.mode === 'scenario') {
      const result = await runScenario({ name: opts.scenario, scenario, configInfo, evidence, receipts, interrupts, argv });
      await writeOut(process.stdout, `${JSON.stringify({ unifiedSupervisor: { mode: 'scenario', ...result, evidence: evidence.dir } })}\n`);
      return result.exitCode;
    }
    evidence.writeJson('receipts.json', receipts);
    const results = [];
    const skipped = [];
    for (const [name, entry] of Object.entries(SCENARIOS)) {
      // The self-test exercises --all only over planned scenarios; real ones need real binaries.
      if (opts.internal && entry.implemented) { skipped.push({ name, status: 'SKIPPED_SELF_TEST' }); continue; }
      if (interrupts.count > 0) { results.push({ name, status: 'INTERRUPTED', exitCode: EXIT.INTERRUPTED, skipped: 'after interrupt' }); continue; }
      results.push(await runScenario({ name, scenario: entry, configInfo, evidence: evidence.child(name), receipts, interrupts, argv }));
    }
    const { status, exitCode } = aggregateExit(results);
    evidence.writeJson('summary.json', { schema: SCHEMA, mode: 'all', status, exitCode, results, skipped, finishedAt: new Date().toISOString() });
    await writeOut(process.stdout, `${JSON.stringify({ unifiedSupervisor: { mode: 'all', status, exitCode, evidence: evidence.dir } })}\n`);
    return exitCode;
  } catch (error) {
    const info = errorInfo(error);
    await writeOut(process.stderr, `${JSON.stringify(redactValue({ error: info }))}\nunified-supervisor: ${redact(info.message)}\n${USAGE}\n`);
    return exitCodeForError(error);
  }
}

// ---------------------------------------------------------------------------
// --self-test: drive the runner as a child process and audit what it wrote.

const CHILD_DEADLINE_MS = 120_000;
const ARM_DEADLINE_MS = 60_000;

function readJson(path) {
  try { return JSON.parse(readFileSync(path, 'utf8')); } catch { return null; }
}

function readText(path) {
  try { return readFileSync(path, 'utf8'); } catch { return ''; }
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

// Direct signal to one owned child runner (never its group): the runner's own
// handlers and teardown are what is under test.
function signalChild(handle, signal) {
  if (handle.exitInfo) return;
  try { handle.child.kill(signal); } catch { /* exited between the check and the signal */ }
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

function makeFixture(evidence) {
  const root = join(evidence.dir, 'fixture');
  const dataRoot = join(root, 'data');
  const memoryRoot = join(root, 'memory');
  const projectRoot = join(root, 'project');
  mkdirSync(projectRoot, { recursive: true });
  const good = {
    $comment: 'Self-test fixture: the binary fields point at the Bun runtime because self-test scenarios exercise the harness, not Ferryx or mhc.',
    ferryxCli: process.execPath, ferryxApp: process.execPath, mhcBin: process.execPath,
    dataRoot, memoryRoot, projectRoot, ownerPrincipal: 'qa-owner',
  };
  const write = (name, value) => {
    const path = join(root, name);
    writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
    return path;
  };
  const missing = { ...good };
  delete missing.mhcBin;
  return {
    root, dataRoot, memoryRoot, ledger: Ledger.forDataRoot(dataRoot),
    good: write('config.good.json', good),
    refusals: [
      ['config-missing-field', write('config.missing-field.json', missing), 'CONFIG_INVALID'],
      ['config-unknown-field', write('config.unknown-field.json', { ...good, engine: 'other' }), 'CONFIG_INVALID'],
      ['config-production-data-root', write('config.production-data-root.json', { ...good, dataRoot: join(homedir(), '.ferryx') }), 'PRODUCTION_PATH_REFUSED'],
      ['config-home-memory-root', write('config.home-memory-root.json', { ...good, memoryRoot: homedir() }), 'PRODUCTION_PATH_REFUSED'],
      ['config-hub-sqlite', write('config.hub-sqlite.json', { ...good, projectRoot: join(root, 'hub', 'hub.sqlite') }), 'PRODUCTION_PATH_REFUSED'],
    ],
  };
}

function redactionUnitChecks(check) {
  const samples = [
    `token xoxb-${'1'.repeat(12)}-abcdef`,
    'xapp-1-A0123-4567890123',
    'Authorization: Bearer abcdefghijklmnop',
    'key sk-ant-abcdefghijklmnopqrstuv',
    `ghp_${'a'.repeat(36)}`,
    '{"botToken":"plain-value-123"}',
    'SLACK_BOT_TOKEN=plainvalue123',
  ];
  const redacted = samples.map((sample) => redact(sample));
  check('redaction-patterns-mask-secrets', redacted.every((value, index) => value !== samples[index] && value.includes('[REDACTED')), { redacted });
  const nested = redactValue({ password: 'hunter2-hunter2', nested: { apiKey: 'abc', list: ['Bearer abcdefghijkl'] }, plain: 'kept' });
  check('redaction-of-structured-values', nested.password === '[REDACTED]' && nested.nested.apiKey === '[REDACTED]' && nested.nested.list[0].includes('[REDACTED') && nested.plain === 'kept', nested);
  let parsed = null;
  try { parsed = JSON.parse(redact(JSON.stringify({ message: 'auth Bearer abcdefghijklmnop', t: `xoxb-${'2'.repeat(10)}`, s: 'MY_TOKEN=abc\\"def' }))); } catch { parsed = null; }
  check('redaction-keeps-json-valid', parsed !== null && !JSON.stringify(parsed).includes('2222222222'), parsed);
}

async function selfTest(opts, interrupts) {
  const evidence = Evidence.prepare(opts.evidence, { force: opts.force });
  const runId = `selftest-${stamp()}-${process.pid}`;
  const journal = new Journal(evidence);
  const ledger = new Ledger(join(evidence.dir, 'parent-ledger'));
  const owned = new Owned({ runId, ledger, evidence, journal, runnerStamp: (await probeProcess(process.pid)).startStamp ?? null });
  const checks = [];
  const safetyNet = [];
  const plants = { slack: `xoxb-${randomUUID()}`, keyed: `plain-${randomUUID()}` };
  const check = (name, ok, detail = null) => {
    const entry = { name, ok: ok === true, detail };
    checks.push(entry);
    evidence.log(`check ${name} ${entry.ok ? 'ok' : 'FAILED'}`);
    return entry.ok;
  };
  const step = async (name, fn) => {
    try { await fn(); } catch (error) {
      check(`${name}-completed`, false, errorInfo(error));
      if (error?.code === 'INTERRUPTED') throw error;
    }
  };
  const runChild = async (label, args, { env = {}, onArmed } = {}) => {
    const id = `child-${label}`;
    const cursor = journal.cursor();
    const handle = await owned.spawn(id, process.execPath, [SCRIPT, ...args], { env: { ...process.env, ...env }, graceMs: 20_000, cwd: REPO_ROOT });
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
  // A child that failed its own teardown must not leak a fixture process: kill
  // it here by recorded start stamp, and report that it was needed.
  const sweep = async (label, pid, startStamp) => {
    if (!Number.isSafeInteger(pid) || !startStamp) return;
    const { verdict } = identityVerdict({ pid, startStamp }, await probeProcess(pid));
    if (verdict === 'match') safetyNet.push({ label, ...(await reapMember({ pid, startStamp })) });
  };

  try {
    const receipts = await collectReceipts({ repoRoot: REPO_ROOT, scripts: SCRIPTS });
    evidence.writeJson('receipts.json', receipts);
    check('receipt-git-revision', /^[0-9a-f]{40}$/.test(receipts.git?.rev ?? ''), { rev: receipts.git?.rev ?? null, error: receipts.git?.error ?? null });
    check('receipt-runner-hashes', Object.values(receipts.runner.scripts).length === SCRIPTS.length && Object.values(receipts.runner.scripts).every(isSha256), receipts.runner.scripts);
    redactionUnitChecks(check);
    const fx = makeFixture(evidence);
    const children = join(evidence.dir, 'children');
    const internal = ['--internal-self-test-child'];

    await step('config-refusals', async () => {
      for (const [label, path, code] of fx.refusals) {
        const dir = join(children, label);
        const result = await runChild(label, ['--scenario', 'runtime', '--config', path, '--evidence', dir]);
        check(`refuses-${label}`, result.code === EXIT.USAGE && result.lines.some((line) => line.includes(code)) && !existsSync(dir), { code: result.code, expected: code, evidenceCreated: existsSync(dir), tail: result.lines.slice(-3) });
      }
    });

    await step('unknown-scenarios', async () => {
      const unknown = await runChild('unknown-scenario', ['--scenario', 'no-such-scenario', '--config', fx.good, '--evidence', join(children, 'unknown-scenario')]);
      check('refuses-unknown-scenario', unknown.code === EXIT.USAGE && unknown.lines.some((line) => line.includes('UNKNOWN_SCENARIO')), { code: unknown.code });
      const hidden = await runChild('hidden-scenario', ['--scenario', '__selftest-fail', '--config', fx.good, '--evidence', join(children, 'hidden-scenario')]);
      check('self-test-scenarios-need-internal-flag', hidden.code === EXIT.USAGE && hidden.lines.some((line) => line.includes('UNKNOWN_SCENARIO')), { code: hidden.code });
    });

    let positiveDir = null;
    await step('not-implemented', async () => {
      const dir = join(children, 'not-implemented');
      const result = await runChild('not-implemented', ['--scenario', 'runtime', '--config', fx.good, '--evidence', dir]);
      const assertions = readJson(join(dir, 'assertions.json'));
      const receipts = readJson(join(dir, 'receipts.json'));
      check('unimplemented-scenario-is-not-implemented', result.code === EXIT.NOT_IMPLEMENTED && assertions?.status === 'NOT_IMPLEMENTED'
        && assertions.assertions?.some((a) => a.name === 'scenario-implemented' && a.ok === false), { code: result.code, status: assertions?.status ?? null });
      check('unimplemented-scenario-writes-cleanup-json', readJson(join(dir, 'cleanup.json'))?.ok === true, null);
      check('receipts-hash-binaries-config-and-revision', ['ferryxCli', 'ferryxApp', 'mhcBin'].every((key) => isSha256(receipts?.binaries?.[key]?.sha256))
        && receipts?.config?.sha256 === sha256(readFileSync(fx.good)) && /^[0-9a-f]{40}$/.test(receipts?.git?.rev ?? ''),
        { binaries: receipts?.binaries ?? null, config: receipts?.config ?? null, git: receipts?.git ?? null });
    });

    await step('all-mode', async () => {
      const dir = join(children, 'all');
      const result = await runChild('all', [...internal, '--all', '--config', fx.good, '--evidence', dir]);
      const summary = readJson(join(dir, 'summary.json'));
      const plannedCount = Object.values(SCENARIOS).filter((entry) => !entry.implemented).length;
      const ok = plannedCount === 0
        ? result.code === EXIT.PASS
        : result.code === EXIT.NOT_IMPLEMENTED && summary?.results?.length === plannedCount && summary.results.every((r) => r.status === 'NOT_IMPLEMENTED');
      check('all-mode-never-passes-unimplemented', ok, { code: result.code, plannedCount, statuses: summary?.results?.map((r) => `${r.name}:${r.status}`) ?? null });
    });

    await step('positive-control', async () => {
      positiveDir = join(children, 'positive-control');
      const result = await runChild('positive-control', [...internal, '--scenario', '__selftest-hung-command', '--config', fx.good, '--evidence', positiveDir]);
      const assertions = readJson(join(positiveDir, 'assertions.json'));
      const cleanup = readJson(join(positiveDir, 'cleanup.json'));
      const hung = cleanup?.processes?.find((p) => p.id === 'hung-command');
      const requests = readText(join(positiveDir, 'requests.jsonl')).split('\n').filter(Boolean);
      const responses = readText(join(positiveDir, 'responses.jsonl')).split('\n').filter(Boolean).map((line) => JSON.parse(line));
      check('positive-control-passes', result.code === EXIT.PASS && assertions?.status === 'PASS' && assertions.assertions?.length > 0 && assertions.assertions.every((a) => a.ok),
        { code: result.code, status: assertions?.status ?? null, failed: assertions?.assertions?.filter((a) => !a.ok) ?? null, error: assertions?.error ?? null });
      check('hung-command-killed-at-deadline', hung?.reason === 'deadline' && hung.exited === true && hung.ok === true, hung ?? null);
      check('cli-requests-and-responses-journaled', requests.length === 1 && responses.length === 1 && responses[0].code === 0 && responses[0].json !== null, { requests: requests.length, responses: responses.length });
    });

    await step('failing-child', async () => {
      const dir = join(children, 'failing-child');
      const result = await runChild('failing-child', [...internal, '--scenario', '__selftest-fail', '--config', fx.good, '--evidence', dir],
        { env: { UNIFIED_SUPERVISOR_SELFTEST_PLANT_SLACK: plants.slack, UNIFIED_SUPERVISOR_SELFTEST_PLANT_KEYED: plants.keyed } });
      const assertions = readJson(join(dir, 'assertions.json'));
      const cleanup = readJson(join(dir, 'cleanup.json'));
      const armed = result.armed;
      const receipt = cleanup?.processes?.find((p) => p.id === 'sleeper') ?? null;
      const grandchild = receipt?.descendants?.find((d) => d.pid === armed?.grandchildPid) ?? null;
      check('failing-child-exits-nonzero', result.code === EXIT.FAIL && assertions?.status === 'FAIL', { code: result.code, status: assertions?.status ?? null });
      check('misleading-success-output-does-not-pass', result.lines.some((line) => line.includes('PASS: all assertions passed')) && assertions?.status !== 'PASS', null);
      check('missing-evidence-assertion-fails', assertions?.assertions?.some((a) => a.name === 'worker-result-evidence-present' && a.ok === false) === true, assertions?.assertions ?? null);
      check('cleanup-json-records-child-termination-receipt', Boolean(armed) && receipt?.pid === armed.sleeperPid && receipt.identity === 'child-handle'
        && receipt.exited === true && receipt.signals?.some((s) => s.signal === 'SIGKILL') && receipt.ok === true, { armed, receipt });
      check('cleanup-json-records-descendant-reap', grandchild?.finalState === 'gone', receipt?.descendants ?? null);
      const sleeperNow = identityVerdict({ pid: armed?.sleeperPid, startStamp: receipt?.startStamp ?? null, args: receipt?.args ?? [] }, await probeProcess(armed?.sleeperPid ?? -1)).verdict;
      const grandNow = identityVerdict({ pid: armed?.grandchildPid, startStamp: grandchild?.startStamp ?? null }, await probeProcess(armed?.grandchildPid ?? -1)).verdict;
      check('terminated-processes-are-gone', ['gone', 'mismatch'].includes(sleeperNow) && ['gone', 'mismatch'].includes(grandNow), { sleeperNow, grandNow });
      await sweep('failing-child sleeper', armed?.sleeperPid, receipt?.startStamp ?? null);
      await sweep('failing-child grandchild', armed?.grandchildPid, grandchild?.startStamp ?? null);
      const runnerLog = readText(join(dir, 'logs', 'runner.log'));
      const sleeperLog = readText(join(dir, 'logs', 'sleeper.stdout.log'));
      check('planted-secrets-reach-logs-only-redacted', runnerLog.includes('[REDACTED') && sleeperLog.includes('[REDACTED')
        && ![runnerLog, sleeperLog].some((text) => text.includes(plants.slack) || text.includes(plants.keyed)), null);
    });

    await step('deadline', async () => {
      const dir = join(children, 'deadline');
      const result = await runChild('deadline', [...internal, '--scenario', '__selftest-hang', '--config', fx.good, '--evidence', dir], { env: { UNIFIED_SUPERVISOR_SELFTEST_HANG_MS: '1500' } });
      const assertions = readJson(join(dir, 'assertions.json'));
      const receipt = readJson(join(dir, 'cleanup.json'))?.processes?.find((p) => p.id === 'sleeper') ?? null;
      check('step-deadline-fails-the-run', result.code === EXIT.HARNESS && assertions?.status === 'DEADLINE_EXCEEDED' && assertions?.error?.code === 'DEADLINE_EXCEEDED',
        { code: result.code, status: assertions?.status ?? null });
      check('deadline-run-tears-down-owned-process', receipt?.pid === result.armed?.sleeperPid && receipt?.exited === true && receipt.ok === true, receipt);
      await sweep('deadline sleeper', result.armed?.sleeperPid, receipt?.startStamp ?? null);
    });

    await step('interrupt', async () => {
      const dir = join(children, 'interrupt');
      const result = await runChild('interrupt', [...internal, '--scenario', '__selftest-hang', '--config', fx.good, '--evidence', dir], {
        env: { UNIFIED_SUPERVISOR_SELFTEST_SLEEPER_IGNORE_TERM: '1' },
        onArmed: async (_armed, handle, cursor) => {
          signalChild(handle, 'SIGINT');
          // Repeated interruption once teardown is under way must not abort it.
          await journal.waitFor('interrupt child cleanup phase', (e) => e.source === handle.id && e.json?.unifiedSupervisor?.phase === 'cleanup', { after: cursor, deadlineMs: ARM_DEADLINE_MS, signal: interrupts.signal });
          signalChild(handle, 'SIGINT');
          signalChild(handle, 'SIGINT');
        },
      });
      const assertions = readJson(join(dir, 'assertions.json'));
      const cleanup = readJson(join(dir, 'cleanup.json'));
      const receipt = cleanup?.processes?.find((p) => p.id === 'sleeper') ?? null;
      check('interrupted-run-exits-130', result.code === EXIT.INTERRUPTED && assertions?.status === 'INTERRUPTED', { code: result.code, status: assertions?.status ?? null });
      check('interrupted-run-still-writes-cleanup-json', receipt?.pid === result.armed?.sleeperPid && receipt?.exited === true && receipt.ok === true && cleanup?.ok === true, receipt);
      check('repeated-interrupts-during-cleanup-do-not-abort-it', (cleanup?.interruptsDuringCleanup ?? 0) >= 1 && receipt?.signals?.some((s) => s.signal === 'SIGKILL') === true,
        { interrupts: cleanup?.interrupts ?? null, duringCleanup: cleanup?.interruptsDuringCleanup ?? null });
      await sweep('interrupt sleeper', result.armed?.sleeperPid, receipt?.startStamp ?? null);
    });

    await step('stale-evidence', async () => {
      if (!positiveDir || !existsSync(join(positiveDir, 'assertions.json'))) {
        check('refuses-stale-evidence-dir', false, 'positive-control evidence missing');
        return;
      }
      const before = sha256(readFileSync(join(positiveDir, 'assertions.json')));
      const result = await runChild('stale-evidence', [...internal, '--scenario', '__selftest-hung-command', '--config', fx.good, '--evidence', positiveDir]);
      const after = sha256(readFileSync(join(positiveDir, 'assertions.json')));
      check('refuses-stale-evidence-dir', result.code === EXIT.USAGE && result.lines.some((line) => line.includes('EVIDENCE_DIR_NOT_EMPTY')) && before === after, { code: result.code, unchanged: before === after });
    });

    await step('orphan-cleanup', async () => {
      const dir = join(children, 'orphan');
      const result = await runChild('orphan', [...internal, '--scenario', '__selftest-orphan', '--config', fx.good, '--evidence', dir], {
        onArmed: async (_armed, handle) => { signalChild(handle, 'SIGKILL'); },
      });
      const orphanPid = result.armed?.sleeperPid ?? null;
      check('orphan-runner-killed-without-teardown', result.signal === 'SIGKILL' && !existsSync(join(dir, 'cleanup.json')), { signal: result.signal });
      const record = fx.ledger.open().find((r) => r.type === 'spawn' && r.pid === orphanPid) ?? null;
      const survived = record ? identityVerdict(record, await probeProcess(orphanPid)) : { verdict: 'no-ledger-record' };
      check('orphan-survives-and-is-ledgered', survived.verdict === 'match', { orphanPid, survived });

      const cursor = journal.cursor();
      const foreign = await owned.spawn('foreign-process', process.execPath, [SLEEPER], { graceMs: 2_000 });
      await journal.waitFor('foreign process ready', (e) => e.source === 'foreign-process' && e.json?.ready === true, { after: cursor, deadlineMs: 20_000, signal: interrupts.signal });
      const forgedKey = `forged-${runId}/proc/foreign#1`;
      fx.ledger.append({ type: 'spawn', key: forgedKey, runId: `forged-${runId}`, id: 'forged-foreign', pid: foreign.pid, executable: process.execPath,
        args: ['--not-the-recorded-launch'], detached: POSIX, graceMs: 1_000, startStamp: 'forged:0', runnerPid: null, runnerStartStamp: null });

      const cleanDir = join(children, 'cleanup');
      const clean = await runChild('cleanup', ['--cleanup', '--config', fx.good, '--evidence', cleanDir]);
      const report = readJson(join(cleanDir, 'cleanup.json'));
      const orphanEntry = report?.entries?.find((e) => e.kind === 'process' && e.pid === orphanPid) ?? null;
      const forgedEntry = report?.entries?.find((e) => e.key === forgedKey) ?? null;
      const pathEntries = report?.entries?.filter((e) => e.kind === 'path') ?? [];
      check('cleanup-mode-terminates-verified-orphan', clean.code === EXIT.PASS && orphanEntry?.status === 'terminated' && orphanEntry.identity?.verdict === 'match' && orphanEntry.ok === true,
        { code: clean.code, orphanEntry });
      check('cleanup-mode-refuses-stale-identity', forgedEntry?.status === 'identity-mismatch' && forgedEntry.killed === false && foreign.exitInfo === null, { forgedEntry, foreignAlive: foreign.exitInfo === null });
      const orphanNow = record ? identityVerdict(record, await probeProcess(orphanPid)).verdict : 'no-ledger-record';
      check('orphan-gone-after-cleanup', ['gone', 'mismatch'].includes(orphanNow), { orphanNow });
      check('cleanup-mode-removes-orphaned-run-roots', pathEntries.length === 2 && pathEntries.every((e) => e.removed === true), pathEntries);
      if (record) await sweep('orphan sleeper', orphanPid, record.startStamp);
      await owned.terminate(foreign, 'self-test-foreign-release');
    });

    await step('ledger-audit', async () => {
      const open = fx.ledger.open();
      check('no-owned-resource-left-open', open.length === 0, open);
      const leftovers = [fx.dataRoot, fx.memoryRoot].flatMap((base) => {
        const runs = join(base, 'runs');
        return existsSync(runs) ? readdirSync(runs).map((name) => join(runs, name)) : [];
      });
      check('no-owned-run-root-left', leftovers.length === 0, leftovers);
    });
  } catch (error) {
    check('self-test-completed', false, errorInfo(error));
  }

  interrupts.phase = 'cleanup';
  const processes = await owned.teardown(interrupts.count > 0 ? 'interrupted' : 'teardown');
  const cleanup = {
    schema: SCHEMA, mode: 'self-test', runId, ledger: ledger.file, finishedAt: new Date().toISOString(), interrupts: interrupts.count,
    interruptsDuringCleanup: interrupts.duringCleanup, received: interrupts.received, processes, safetyNet,
    ok: processes.every((entry) => entry.ok) && safetyNet.every((entry) => entry.finalState === 'gone'),
  };
  evidence.writeJson('cleanup.json', cleanup);
  check('self-test-cleanup-ok', cleanup.ok, { processes: processes.length });
  check('no-safety-net-kill-needed', safetyNet.length === 0, safetyNet);
  const leaks = scanForPlants(evidence.dir, [plants.slack, plants.keyed]);
  check('no-planted-secret-anywhere-in-evidence', leaks.length === 0, leaks);

  const ok = interrupts.count === 0 && checks.length > 0 && checks.every((entry) => entry.ok);
  const status = ok ? 'PASS' : interrupts.count > 0 ? 'INTERRUPTED' : 'FAIL';
  const exitCode = ok ? EXIT.PASS : interrupts.count > 0 ? EXIT.INTERRUPTED : EXIT.FAIL;
  evidence.writeJson('self-test.json', {
    schema: SCHEMA, status, exitCode, runId, checks, failed: checks.filter((entry) => !entry.ok).map((entry) => entry.name),
    plantSha256: [sha256(plants.slack), sha256(plants.keyed)], finishedAt: new Date().toISOString(),
  });
  const failed = checks.filter((entry) => !entry.ok).map((entry) => entry.name);
  await writeOut(process.stdout, `${JSON.stringify({ unifiedSupervisor: { mode: 'self-test', status, exitCode, checks: checks.length, failed, evidence: evidence.dir } })}\n`);
  return exitCode;
}

if (import.meta.main ?? resolve(process.argv[1] ?? '') === SCRIPT) {
  let code;
  try { code = await main(process.argv.slice(2)); } catch (error) {
    await writeOut(process.stderr, `unified-supervisor: fatal ${redact(String(error?.stack ?? error))}\n`);
    code = EXIT.HARNESS;
  }
  process.exit(code);
}
