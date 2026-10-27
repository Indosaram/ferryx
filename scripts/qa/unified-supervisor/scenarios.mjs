// Scenario registry for scripts/qa/unified-supervisor.mjs.
//
// Every scenario named by the plan's "Scenario fixtures and exact observable
// contracts" table is registered here. A registered-but-unimplemented scenario
// reports NOT_IMPLEMENTED (exit 3) and can never PASS. Each later plan task
// replaces its `planned(...)` entry with `{ implemented: true, task, summary,
// deadlineMs, requiredAssertions, run(ctx) }` in the same increment that
// delivers the behaviour. ctx.spawn/ctx.run/ctx.cli default to the isolated
// product environment; pass `env: process.env` only to opt in to inheritance.
//
// The __selftest-* scenarios exist only for --self-test and are reachable only
// through --internal-self-test-child, which the CLI accepts only with the
// per-run nonce the self-test parent creates.
import { chmodSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HarnessError } from './harness.mjs';

export const SLEEPER = fileURLToPath(new URL('./fixture-sleeper.mjs', import.meta.url));
const PLANNED_DEADLINE_MS = 15 * 60_000;

const planned = (task, summary) => Object.freeze({ implemented: false, task, summary, deadlineMs: PLANNED_DEADLINE_MS });

export const SCENARIOS = Object.freeze({
  runtime: planned('6', 'Real mhc: one structured assessment, restart the owned engine, resume the saved session from another cwd; missing capability refused'),
  admission: planned('7', 'Submit the fixture request, replay the same source id, ask "What can you do?": one goal, one initial dispatch'),
  autonomy: planned('10', 'First verification forced to fail, corrected result allowed: two run ids, attested pass, one completed goal, no owner nudge'),
  correction: planned('10', 'Owner correction "Use DONE instead of READY" before dispatch, stale expectedRevision replayed: final bytes DONE, stale mutation 409'),
  waiting: planned('9', 'Parent starts two children; one settles, then the second settles/fails: no decision while incomplete, one resume, failure is not completion'),
  burst: planned('9', 'Four ordinary reports at virtual 0,1,2,3s advanced to 30s: one admitted model turn with four receipts; urgent pause has zero delay'),
  recovery: planned('12', 'Restart only the isolated supervisor at before-send, after-send-before-receipt and after-start boundaries: no resend, PTYs survive'),
  'remote-delivery': planned('8', 'Target accepts bytes but suppresses the start receipt, then reconnects: never started/complete from acceptance, no duplicate input'),
  authority: planned('10,13', 'Non-owner submit/approve, mirror device mutation, worker "approve everything": 403/typed denial, zero dispatch, zero memory change'),
  memory: planned('15', 'Teach, new context and restart, task-only exception, forget: persisted/applied receipts, exception not learned, no resurrection'),
  capabilities: planned('12', 'Remove one required RPC capability; auth, quota and provider faults: named degraded cause, no fallback model, workers preserved'),
  legacy: planned('16', 'Import historical closed/failed/blocked work, adopt one safe fixture: no automatic revival, one chosen adoption'),
  channels: planned('11,17', 'Approved Slack QA thread submit, desktop correction, phone pause/resume, Slack query: same supervisor/goal/revision, no leakage'),
  surfaces: planned('14', 'Real desktop and mobile controls against a populated isolated hub: same state and goal revision, readable wait/degraded views'),
  usage: planned('15', 'Replay the same reports before/after the scheduling change: no idle calls, stable prefix hash, every call has a cause'),
});

async function startSleeper(ctx, id, flags, graceMs, { extraEnv = {}, onLine } = {}) {
  const cursor = ctx.journal.cursor();
  const handle = await ctx.spawn(id, process.execPath, [SLEEPER, ...flags], { graceMs, env: ctx.productEnv(extraEnv), onLine });
  const ready = await ctx.waitFor(`${id} ready line`, (e) => e.source === id && e.kind === 'stdout' && e.json?.ready === true, { after: cursor, deadlineMs: 20_000 });
  return { handle, ready: ready.json };
}

function envMs(name, fallback) {
  const value = Number(process.env[name]);
  return Number.isFinite(value) && value > 0 ? value : fallback;
}

const selfTest = (summary, deadlineMs, body) => Object.freeze({ implemented: true, task: '1', summary: `Self-test: ${summary}`, deadlineMs, ...body });

export const SELF_TEST_SCENARIOS = Object.freeze({
  // Deliberately failing child: misleading success output, a planted secret,
  // a SIGTERM-ignoring owned process with a descendant, and missing evidence.
  '__selftest-fail': selfTest('deliberately failing assertion with owned processes to tear down', 60_000, {
    async run(ctx) {
      const plant = process.env.UNIFIED_SUPERVISOR_SELFTEST_PLANT_SLACK;
      const { handle, ready } = await startSleeper(ctx, 'sleeper', ['--ignore-sigterm', '--grandchild'], 1_000,
        { extraEnv: plant ? { UNIFIED_SUPERVISOR_SELFTEST_PLANT_SLACK: plant } : {} });
      ctx.announce({ selftest: 'armed', case: 'fail', sleeperPid: ready.pid, grandchildPid: ready.grandchildPid });
      ctx.log(`diagnostic line carrying a planted credential: ${plant ?? 'unset'}`);
      ctx.log(JSON.stringify({ apiKey: process.env.UNIFIED_SUPERVISOR_SELFTEST_PLANT_KEYED ?? 'unset' }));
      ctx.announce('PASS: all assertions passed (deliberately misleading output; assertions.json decides)');
      ctx.assert('sleeper-alive-before-teardown', handle.exitInfo === null, { pid: ready.pid });
      const required = join(ctx.paths.data, 'worker-result.txt');
      ctx.assert('worker-result-evidence-present', existsSync(required), { required, note: 'deliberately absent: missing evidence must fail the run' });
    },
  }),
  // Hangs on an event that never arrives: ends by step deadline or by SIGINT.
  '__selftest-hang': selfTest('wait on a never-emitted event (deadline or interrupt)', 180_000, {
    async run(ctx) {
      const ignoreTerm = process.env.UNIFIED_SUPERVISOR_SELFTEST_SLEEPER_IGNORE_TERM === '1';
      const { ready } = await startSleeper(ctx, 'sleeper', ignoreTerm ? ['--ignore-sigterm'] : [], ignoreTerm ? 5_000 : 3_000);
      const hangMs = envMs('UNIFIED_SUPERVISOR_SELFTEST_HANG_MS', 120_000);
      const cursor = ctx.journal.cursor();
      ctx.announce({ selftest: 'armed', case: 'hang', sleeperPid: ready.pid, hangMs });
      await ctx.waitFor('an event that is never emitted', (e) => e.kind === 'never-emitted', { after: cursor, deadlineMs: hangMs });
      ctx.assert('unreachable-after-hang', false);
    },
  }),
  // Positive control: proves the runner can PASS, that a hung command is
  // killed at its deadline, that a path barrier armed before its trigger
  // resolves, and that ctx.run/ctx.cli default to an isolated environment.
  '__selftest-hung-command': selfTest('positive control: bounded hung command, path barrier, isolated env', 60_000, {
    requiredAssertions: ['hung-command-timed-out', 'path-barrier-armed-before-trigger-resolves', 'run-default-env-is-isolated', 'cli-env-is-isolated', 'spawn-default-env-is-isolated'],
    async run(ctx) {
      const hung = await ctx.run('hung-command', process.execPath, [SLEEPER], { deadlineMs: 1_500, graceMs: 1_000 });
      ctx.assert('hung-command-timed-out', hung.timedOut === true, { timedOut: hung.timedOut, durationMs: hung.durationMs });
      ctx.assert('hung-command-terminated', hung.receipt?.exited === true && hung.receipt?.ok === true, hung.receipt);
      ctx.assert('hung-command-not-success', hung.code !== 0, { code: hung.code, signal: hung.signal });

      const marker = join(ctx.paths.data, 'barrier', 'ready.flag');
      mkdirSync(dirname(marker), { recursive: true });
      const barrier = ctx.waitForPath(marker, { deadlineMs: 15_000, label: 'barrier marker' });
      barrier.catch(() => { /* awaited below */ });
      const trigger = await ctx.run('barrier-trigger', process.execPath, ['-e', `require('node:fs').writeFileSync(${JSON.stringify(marker)}, 'ok')`], { deadlineMs: 15_000 });
      const observed = await barrier.then(() => true, () => false);
      ctx.assert('path-barrier-armed-before-trigger-resolves', trigger.code === 0 && observed, { code: trigger.code, stderr: trigger.stderr });

      const inside = (path) => typeof path === 'string' && isAbsolute(path) && !relative(ctx.paths.data, path).startsWith('..');
      const probe = 'process.stdout.write(JSON.stringify({ home: process.env.HOME, tmp: process.env.TMPDIR, dataDir: process.env.FERRYX_DATA_DIR, inherited: Object.keys(process.env).filter((k) => k.startsWith("UNIFIED_SUPERVISOR_")) }))';
      const ran = await ctx.run('run-env-echo', process.execPath, ['-e', probe], { deadlineMs: 15_000 });
      let ranJson = null;
      try { ranJson = JSON.parse(ran.stdout); } catch { /* asserted below */ }
      ctx.assert('run-default-env-is-isolated', ran.code === 0 && inside(ranJson?.home) && inside(ranJson?.tmp) && ranJson?.inherited?.length === 0,
        { code: ran.code, json: ranJson, stderr: ran.stderr });
      const echo = await ctx.cli(['-e', probe], { label: 'echo-env', deadlineMs: 15_000 });
      ctx.assert('cli-env-is-isolated', echo.code === 0 && inside(echo.json?.home) && inside(echo.json?.dataDir) && echo.json?.inherited?.length === 0,
        { code: echo.code, json: echo.json, stderr: echo.stderr });
      // ctx.spawn with no env must also get the isolated product environment.
      const spawnCursor = ctx.journal.cursor();
      const spawned = await ctx.spawn('spawn-env-echo', process.execPath, ['-e', probe]);
      const output = await ctx.waitFor('spawn-env-echo output', (e) => e.source === 'spawn-env-echo' && e.kind === 'stdout' && e.json !== null, { after: spawnCursor, deadlineMs: 15_000 });
      const exited = await ctx.waitFor('spawn-env-echo exit', (e) => e.source === 'spawn-env-echo' && e.kind === 'exit', { after: spawnCursor, deadlineMs: 15_000 });
      ctx.assert('spawn-default-env-is-isolated', exited.code === 0 && inside(output.json?.home) && inside(output.json?.tmp) && inside(output.json?.dataDir)
        && output.json?.inherited?.length === 0, { pid: spawned.pid, code: exited.code, json: output.json });
    },
  }),
  // Waits until the self-test parent SIGKILLs this runner, leaving orphans for
  // --cleanup to reclaim by recorded identity: a sleeper and (POSIX) a #! script
  // whose kernel argv starts with the interpreter rather than the launched path.
  '__selftest-orphan': selfTest('leave a ledgered orphan when the runner is SIGKILLed', 180_000, {
    async run(ctx) {
      const { ready } = await startSleeper(ctx, 'sleeper', [], 3_000);
      let scriptPid = null;
      if (process.platform !== 'win32') {
        const script = join(ctx.paths.data, 'worker.sh');
        writeFileSync(script, '#!/bin/sh\nprintf \'{"ready":true,"pid":%s}\\n\' "$$"\nsleep 240\n', { mode: 0o755 });
        const cursor = ctx.journal.cursor();
        const handle = await ctx.spawn('script', script, [], { graceMs: 3_000 });
        await ctx.waitFor('script ready line', (e) => e.source === 'script' && e.kind === 'stdout' && e.json?.ready === true, { after: cursor, deadlineMs: 20_000 });
        scriptPid = handle.pid;
      }
      ctx.announce({ selftest: 'armed', case: 'orphan', sleeperPid: ready.pid, scriptPid });
      await ctx.waitFor('the self-test parent to SIGKILL this runner', () => false, { deadlineMs: 120_000 });
      ctx.assert('unreachable-after-orphan-wait', false);
    },
  }),
  // PASS gating mutations: each of these must end nonzero with a named cause.
  '__selftest-no-assertions': selfTest('settles without recording any assertion', 30_000, {
    async run() { /* deliberately records nothing */ },
  }),
  '__selftest-throw': selfTest('throws an ordinary error', 30_000, {
    async run(ctx) {
      ctx.assert('before-throw', true);
      throw new Error('deliberate ordinary error (self-test)');
    },
  }),
  '__selftest-missing-required': selfTest('omits one declared required assertion', 30_000, {
    requiredAssertions: ['first-required', 'second-required'],
    async run(ctx) { ctx.assert('first-required', true); },
  }),
  // An assertion that arrives after the scenario settled (during teardown).
  '__selftest-late-assertion': selfTest('records an assertion during teardown', 60_000, {
    async run(ctx) {
      const onLine = (_kind, _line, json) => {
        if (!json?.ignoredSignal) return;
        try { ctx.assert('late-assertion-after-settle', false); } catch { /* LATE_ASSERTION is the expected refusal */ }
      };
      await startSleeper(ctx, 'sleeper', ['--ignore-sigterm'], 2_000, { onLine });
      ctx.assert('sleeper-ready', true);
    },
  }),
  // An unhandled rejection or uncaught exception during the run must end it as HARNESS_ERROR.
  '__selftest-fault': selfTest('unhandled rejection or uncaught exception mid-run', 60_000, {
    async run(ctx) {
      const { ready } = await startSleeper(ctx, 'sleeper', [], 3_000);
      ctx.announce({ selftest: 'armed', case: 'fault', sleeperPid: ready.pid });
      const cursor = ctx.journal.cursor();
      ctx.assert('fault-armed', true);
      if (process.env.UNIFIED_SUPERVISOR_SELFTEST_FAULT === 'exception') {
        setTimeout(() => { throw new Error('deliberate uncaught exception (self-test)'); }, 0);
      } else {
        Promise.reject(new Error('deliberate unhandled rejection (self-test)'));
      }
      await ctx.waitFor('the harness fault to be recorded', (e) => e.kind === 'harness-fault', { after: cursor, deadlineMs: 15_000 });
    },
  }),
  // All assertions pass but the run root cannot be removed (read-only subdirectory).
  '__selftest-cleanup-fail': selfTest('assertions pass but run-root removal fails', 30_000, {
    async run(ctx) {
      const locked = join(ctx.paths.data, 'locked');
      mkdirSync(locked, { recursive: true });
      writeFileSync(join(locked, 'keep.txt'), 'held by a read-only directory\n');
      chmodSync(locked, 0o500);
      ctx.announce({ selftest: 'armed', case: 'cleanup-fail', locked });
      ctx.assert('locked-directory-created', existsSync(join(locked, 'keep.txt')), { locked });
    },
  }),
});

export function resolveScenario(name, internal = false) {
  if (Object.hasOwn(SCENARIOS, name)) return SCENARIOS[name];
  if (internal && Object.hasOwn(SELF_TEST_SCENARIOS, name)) return SELF_TEST_SCENARIOS[name];
  throw new HarnessError('UNKNOWN_SCENARIO', `unknown scenario ${name}; registered: ${Object.keys(SCENARIOS).join(', ')}`);
}
