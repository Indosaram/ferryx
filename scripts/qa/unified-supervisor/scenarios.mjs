// Scenario registry for scripts/qa/unified-supervisor.mjs.
//
// Every scenario named by the plan's "Scenario fixtures and exact observable
// contracts" table is registered here. A registered-but-unimplemented scenario
// reports NOT_IMPLEMENTED (exit 3) and can never PASS. Each later plan task
// replaces its `planned(...)` entry with `{ implemented: true, task, summary,
// deadlineMs, run(ctx) }` in the same increment that delivers the behaviour.
//
// The __selftest-* scenarios exist only for --self-test and are reachable only
// with the internal --internal-self-test-child flag.
import { existsSync, mkdirSync } from 'node:fs';
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

async function startSleeper(ctx, id, flags, graceMs) {
  const cursor = ctx.journal.cursor();
  const handle = await ctx.spawn(id, process.execPath, [SLEEPER, ...flags], { graceMs });
  const ready = await ctx.waitFor(`${id} ready line`, (e) => e.source === id && e.kind === 'stdout' && e.json?.ready === true, { after: cursor, deadlineMs: 20_000 });
  return { handle, ready: ready.json };
}

function envMs(name, fallback) {
  const value = Number(process.env[name]);
  return Number.isFinite(value) && value > 0 ? value : fallback;
}

export const SELF_TEST_SCENARIOS = Object.freeze({
  // Deliberately failing child: misleading success output, a planted secret,
  // a SIGTERM-ignoring owned process with a descendant, and missing evidence.
  '__selftest-fail': Object.freeze({
    implemented: true, task: '1', deadlineMs: 60_000, summary: 'Self-test: deliberately failing assertion with owned processes to tear down',
    async run(ctx) {
      const { handle, ready } = await startSleeper(ctx, 'sleeper', ['--ignore-sigterm', '--grandchild'], 1_000);
      ctx.announce({ selftest: 'armed', case: 'fail', sleeperPid: ready.pid, grandchildPid: ready.grandchildPid });
      ctx.log(`diagnostic line carrying a planted credential: ${process.env.UNIFIED_SUPERVISOR_SELFTEST_PLANT_SLACK ?? 'unset'}`);
      ctx.log(JSON.stringify({ apiKey: process.env.UNIFIED_SUPERVISOR_SELFTEST_PLANT_KEYED ?? 'unset' }));
      ctx.announce('PASS: all assertions passed (deliberately misleading output; assertions.json decides)');
      ctx.assert('sleeper-alive-before-teardown', handle.exitInfo === null, { pid: ready.pid });
      const required = join(ctx.paths.data, 'worker-result.txt');
      ctx.assert('worker-result-evidence-present', existsSync(required), { required, note: 'deliberately absent: missing evidence must fail the run' });
    },
  }),
  // Hangs on an event that never arrives: ends by step deadline or by SIGINT.
  '__selftest-hang': Object.freeze({
    implemented: true, task: '1', deadlineMs: 180_000, summary: 'Self-test: wait on a never-emitted event (deadline or interrupt)',
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
  // resolves, and that CLI calls are journaled with an isolated environment.
  '__selftest-hung-command': Object.freeze({
    implemented: true, task: '1', deadlineMs: 60_000, summary: 'Self-test positive control: bounded hung command, path barrier, isolated CLI env',
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

      const echo = await ctx.cli(['-e', 'process.stdout.write(JSON.stringify({ home: process.env.HOME, dataDir: process.env.FERRYX_DATA_DIR, inherited: Object.keys(process.env).filter((k) => k.startsWith("UNIFIED_SUPERVISOR_")) }))'], { label: 'echo-env', deadlineMs: 15_000 });
      const inside = (path) => typeof path === 'string' && isAbsolute(path) && !relative(ctx.paths.data, path).startsWith('..');
      ctx.assert('cli-env-is-isolated', echo.code === 0 && inside(echo.json?.home) && inside(echo.json?.dataDir) && echo.json?.inherited?.length === 0, { code: echo.code, json: echo.json, stderr: echo.stderr });
    },
  }),
  // Waits until the self-test parent SIGKILLs this runner, leaving an orphan
  // for --cleanup to reclaim by recorded identity.
  '__selftest-orphan': Object.freeze({
    implemented: true, task: '1', deadlineMs: 180_000, summary: 'Self-test: leave a ledgered orphan when the runner is SIGKILLed',
    async run(ctx) {
      const { ready } = await startSleeper(ctx, 'sleeper', [], 3_000);
      ctx.announce({ selftest: 'armed', case: 'orphan', sleeperPid: ready.pid });
      await ctx.waitFor('the self-test parent to SIGKILL this runner', () => false, { deadlineMs: 120_000 });
      ctx.assert('unreachable-after-orphan-wait', false);
    },
  }),
});

export function resolveScenario(name, internal = false) {
  if (Object.hasOwn(SCENARIOS, name)) return SCENARIOS[name];
  if (internal && Object.hasOwn(SELF_TEST_SCENARIOS, name)) return SELF_TEST_SCENARIOS[name];
  throw new HarnessError('UNKNOWN_SCENARIO', `unknown scenario ${name}; registered: ${Object.keys(SCENARIOS).join(', ')}`);
}
