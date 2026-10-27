#!/usr/bin/env bun
// Isolated integration driver for the unified autonomous supervisor
// (.omo/plans/unified-autonomous-supervisor.md, task 1).
//
//   bun scripts/qa/unified-supervisor.mjs --scenario NAME --config <json> --evidence <dir> [--force-evidence]
//   bun scripts/qa/unified-supervisor.mjs --all          --config <json> --evidence <dir> [--force-evidence]
//   bun scripts/qa/unified-supervisor.mjs --cleanup      --config <json> --evidence <dir> [--force-evidence] [--force-owner]
//   bun scripts/qa/unified-supervisor.mjs --self-test    --evidence <dir> [--force-evidence]
//
// Exit codes: 0 PASS, 1 FAIL (including an empty or skipped run), 2 usage /
// config / evidence / stale-state / run-lock refusal, 3 NOT_IMPLEMENTED, 4
// deadline or harness error (including an uncaught fault anywhere in the
// process), 5 cleanup incomplete, 130 interrupted.
//
// Contracts: assertions decide PASS, never log text; a scenario that is only
// registered reports NOT_IMPLEMENTED; production paths (~/.ferryx, any
// hub.sqlite, Ferryx/mhc app-data, XDG and daemon runtime roots, symlinked or
// not) and missing config fields are refused before anything is written; a
// dataRoot/memoryRoot holding foreign state is refused; an evidence directory
// from a prior run is refused unless --force-evidence moves it aside; a run is
// refused while an earlier crashed run's owned processes may be alive
// (STALE_OWNED_STATE: run --cleanup first); a second concurrent run on the same
// dataRoot is refused (RUN_LOCKED); every spawned process is recorded
// and torn down by recorded identity; cleanup.json is written on interrupts and
// on uncaught faults. Interrupts: SIGINT/SIGTERM/SIGHUP (and Ctrl-Break, SIGBREAK,
// on Windows); with UNIFIED_SUPERVISOR_CONTROL=stdin each stdin line "interrupt"
// counts as one SIGINT, which is how a supervising process interrupts a run on
// Windows. Config fields: see unified-supervisor/config.schema.json.
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  acquireRunLock, aggregateExit, cleanupMode, collectReceipts, CONTROL_ENV, errorInfo, EXIT, Evidence, exitCodeForError, finalExitCode, HarnessError,
  isInside, loadConfig, PLATFORM, preflightOwnedState, probeProcess, redact, redactValue, releaseRunLock, runScenario, SCHEMA, withDeadline,
} from './unified-supervisor/harness.mjs';
import { resolveScenario, SCENARIOS } from './unified-supervisor/scenarios.mjs';
import { NONCE_ENV, NONCE_FILE, selfTest } from './unified-supervisor/self-test.mjs';

const SCRIPT = fileURLToPath(import.meta.url);
const QA_DIR = join(dirname(SCRIPT), 'unified-supervisor');
const REPO_ROOT = resolve(dirname(SCRIPT), '..', '..');
const SCRIPTS = [SCRIPT, ...['harness.mjs', 'scenarios.mjs', 'self-test.mjs', 'self-test-units.mjs', 'fixture-sleeper.mjs', 'fixture-worker.mjs', 'config.schema.json'].map((name) => join(QA_DIR, name))];
const USAGE = 'usage: bun scripts/qa/unified-supervisor.mjs (--scenario NAME | --all | --self-test | --cleanup) [--config <json>] --evidence <dir> [--force-evidence] [--force-owner]';

export function writeOut(stream, text) {
  return withDeadline(new Promise((resolvePromise) => { stream.write(text, () => resolvePromise(true)); }), 2_000, () => false);
}

export function parseCli(argv) {
  const opts = { mode: null, scenario: null, config: null, evidence: null, forceEvidence: false, forceOwner: false, internal: false };
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
      case '--force-evidence': opts.forceEvidence = true; break;
      case '--force-owner': opts.forceOwner = true; break;
      case '--force': throw new HarnessError('USAGE', '--force is ambiguous: use --force-evidence (move a prior evidence dir aside) or --force-owner (--cleanup may reclaim processes of a runner that still looks alive)');
      case '--internal-self-test-child': opts.internal = true; break;
      case '--help': case '-h': return { ...opts, mode: 'help' };
      default: throw new HarnessError('USAGE', `unknown argument ${arg}`);
    }
  }
  if (modes.length !== 1) throw new HarnessError('USAGE', 'choose exactly one of --scenario NAME, --all, --self-test, --cleanup');
  opts.mode = modes[0];
  if (!opts.evidence) throw new HarnessError('USAGE', '--evidence <dir> is required');
  if (opts.forceOwner && opts.mode !== 'cleanup') throw new HarnessError('USAGE', '--force-owner only applies to --cleanup');
  if (opts.mode === 'self-test' && opts.config) throw new HarnessError('USAGE', '--self-test builds its own fixture config; do not pass --config');
  if (opts.mode === 'self-test' && opts.internal) throw new HarnessError('USAGE', '--self-test cannot be nested');
  if (opts.mode !== 'self-test' && !opts.config) throw new HarnessError('CONFIG_INVALID', '--config <json> is required for this mode');
  return opts;
}

// --internal-self-test-child only runs for a child of --self-test: the per-run
// nonce in the environment must equal the nonce file beside the fixture config,
// and evidence must stay inside that self-test's children directory.
function assertSelfTestChild(opts) {
  const nonce = process.env[NONCE_ENV] ?? '';
  if (nonce.length < 32 || !opts.config) {
    throw new HarnessError('INTERNAL_FLAG_REFUSED', '--internal-self-test-child is reserved for children started by --self-test');
  }
  const configDir = dirname(resolve(opts.config));
  let expected = '';
  try { expected = readFileSync(join(configDir, NONCE_FILE), 'utf8').trim(); } catch { /* refused below */ }
  if (expected !== nonce) throw new HarnessError('INTERNAL_FLAG_REFUSED', 'the self-test nonce does not match this config; the internal flag only runs against the live self-test fixture');
  if (!isInside(resolve(opts.evidence), join(dirname(configDir), 'children'))) {
    throw new HarnessError('INTERNAL_FLAG_REFUSED', 'self-test child evidence must live under the self-test children directory');
  }
}

function installProcessHandlers() {
  const controller = new AbortController();
  const state = { signal: controller.signal, count: 0, duringCleanup: 0, phase: 'run', received: [], faults: [], faultListeners: new Set() };
  const interrupt = (name) => {
    state.count += 1;
    state.received.push({ signal: name, phase: state.phase, at: new Date().toISOString() });
    if (state.phase === 'cleanup') state.duringCleanup += 1;
    if (!controller.signal.aborted) controller.abort(new HarnessError('INTERRUPTED', `received ${name}`));
  };
  for (const name of ['SIGINT', 'SIGTERM', 'SIGHUP', ...(PLATFORM === 'win32' ? ['SIGBREAK'] : [])]) {
    try {
      process.on(name, () => interrupt(name));
    } catch { /* signal unsupported on this platform */ }
  }
  // Opt-in control channel. On Windows a parent cannot deliver SIGINT to a child
  // (child.kill('SIGINT') is TerminateProcess), so a supervising process writes
  // "interrupt" lines instead; each one is handled exactly like SIGINT.
  if (process.env[CONTROL_ENV] === 'stdin') {
    let buffer = '';
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', (chunk) => {
      buffer += chunk;
      for (let index = buffer.indexOf('\n'); index >= 0; index = buffer.indexOf('\n')) {
        const line = buffer.slice(0, index).trim();
        buffer = buffer.slice(index + 1);
        if (line === 'interrupt') interrupt('control:interrupt');
      }
      if (buffer.length > 4096) buffer = '';
    });
    process.stdin.on('error', () => { /* the channel is optional; signals still work */ });
  }
  // A fault aborts the active run as HARNESS_ERROR; teardown still runs and cleanup.json records it.
  const onFault = (kind) => (reason) => {
    const message = redact(String(reason?.stack ?? reason));
    state.faults.push({ kind, message, phase: state.phase, at: new Date().toISOString() });
    try { process.stderr.write(`unified-supervisor: ${kind}: ${message}\n`); } catch { /* stderr gone */ }
    for (const listener of [...state.faultListeners]) {
      try { listener(kind, message); } catch { /* a listener must not mask the fault */ }
    }
  };
  process.on('unhandledRejection', onFault('unhandledRejection'));
  process.on('uncaughtException', onFault('uncaughtException'));
  return state;
}

async function main(argv) {
  const interrupts = installProcessHandlers();
  try {
    const opts = parseCli(argv);
    if (opts.mode === 'help') {
      await writeOut(process.stdout, `${USAGE}\nscenarios: ${Object.keys(SCENARIOS).join(', ')}\n`);
      return EXIT.PASS;
    }
    if (opts.internal) assertSelfTestChild(opts);
    if (opts.mode === 'self-test') {
      return await selfTest(opts, interrupts, { script: SCRIPT, repoRoot: REPO_ROOT, scripts: SCRIPTS, writeOut });
    }
    const scenario = opts.mode === 'scenario' ? resolveScenario(opts.scenario, opts.internal) : null;
    // Refusals happen before any directory is created or any process started.
    const configInfo = loadConfig(opts.config);
    const lock = await acquireRunLock(configInfo.config.dataRoot, { runnerStamp: (await probeProcess(process.pid)).startStamp ?? null, mode: opts.mode });
    try {
      return await runLocked(opts, interrupts, { scenario, configInfo, argv, lock });
    } finally {
      releaseRunLock(lock);
    }
  } catch (error) {
    const info = errorInfo(error);
    await writeOut(process.stderr, `${JSON.stringify(redactValue({ error: info }))}\nunified-supervisor: ${redact(info.message)}\n${USAGE}\n`);
    return exitCodeForError(error);
  }
}

// Runs while this process holds the dataRoot run lock.
async function runLocked(opts, interrupts, { scenario, configInfo, argv, lock }) {
  if (opts.mode !== 'cleanup') await preflightOwnedState(configInfo);
  const evidence = Evidence.prepare(opts.evidence, { force: opts.forceEvidence });
  evidence.log(`unified-supervisor mode=${opts.mode} pid=${process.pid} argv=${JSON.stringify(argv)}`);
  if (lock.takenOver.length > 0) evidence.writeJson('run-lock-takeover.json', lock.takenOver);
  if (opts.mode === 'cleanup') {
    const exitCode = finalExitCode(await cleanupMode({ configInfo, evidence, interrupts, forceOwner: opts.forceOwner }), interrupts.faults);
    await writeOut(process.stdout, `${JSON.stringify({ unifiedSupervisor: { mode: 'cleanup', exitCode, evidence: evidence.dir } })}\n`);
    return exitCode;
  }
  const receipts = await collectReceipts({ repoRoot: REPO_ROOT, configInfo, scripts: SCRIPTS });
  if (opts.mode === 'scenario') {
    const result = await runScenario({ name: opts.scenario, scenario, configInfo, evidence, receipts, interrupts, argv });
    const exitCode = finalExitCode(result.exitCode, interrupts.faults);
    if (exitCode !== result.exitCode) evidence.writeJson('faults.json', { exitCode, faults: interrupts.faults });
    await writeOut(process.stdout, `${JSON.stringify({ unifiedSupervisor: { mode: 'scenario', ...result, exitCode, evidence: evidence.dir } })}\n`);
    return exitCode;
  }
  evidence.writeJson('receipts.json', receipts);
  const results = [];
  for (const [name, entry] of Object.entries(SCENARIOS)) {
    // A self-test child exercises --all over planned scenarios only; a skipped one is still not a PASS.
    if (opts.internal && entry.implemented) { results.push({ name, status: 'SKIPPED', reason: 'self-test child runs planned scenarios only' }); continue; }
    if (interrupts.count > 0) { results.push({ name, status: 'INTERRUPTED', exitCode: EXIT.INTERRUPTED, reason: 'not started after an interrupt' }); continue; }
    results.push(await runScenario({ name, scenario: entry, configInfo, evidence: evidence.child(name), receipts, interrupts, argv }));
  }
  const aggregate = aggregateExit(results);
  const exitCode = finalExitCode(aggregate.exitCode, interrupts.faults);
  const status = exitCode === aggregate.exitCode ? aggregate.status : 'HARNESS_ERROR';
  evidence.writeJson('summary.json', { schema: SCHEMA, mode: 'all', status, exitCode, results, faults: interrupts.faults, finishedAt: new Date().toISOString() });
  await writeOut(process.stdout, `${JSON.stringify({ unifiedSupervisor: { mode: 'all', status, exitCode, evidence: evidence.dir } })}\n`);
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
