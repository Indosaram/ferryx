// Deterministic proofs for the pure parts of scripts/soak-pty-footprint.mjs.
//
// Nothing here spawns a process, reads a clock or touches a socket: the soak's runtime half needs
// real binaries and 60 wall-clock minutes, so what is pinned here is exactly the arithmetic and the
// argument shapes that decide whether a run can be believed. `node --test` only.

import test from 'node:test';
import assert from 'node:assert/strict';
import os from 'node:os';
import path from 'node:path';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import {
  GuardError,
  OUTPUT_RING_CAPACITY_BYTES,
  PLAN_DURATION_MIN,
  PLAN_INTERVAL_MS,
  PLAN_SESSIONS,
  SATURATION_DEADLINE_RATIO,
  TARGET_BYTES,
  TREND_FROM_MS,
  assertIsolated,
  assertSaturatesBeforeTrend,
  daemonArgv,
  decodeKeeperFrame,
  encodeKeeperFrame,
  evaluateSoak,
  expectedSampleElapsedMs,
  identityMatches,
  isFullSpec,
  isInside,
  isolatedEnv,
  keeperArgv,
  makeLayout,
  parseArgs,
  parsePsRow,
  productionRuntimeDir,
  reconcileRouting,
  ringFillMs,
  retainedBudgetFor,
  retainedFillMs,
  retainedTextCeilingBytes,
  scrollbackEnv,
  SOAK_MIN_SESSION_HISTORY_BYTES,
  SOAK_RETAINED_CEILING_BYTES,
  collectRetained,
  outputCommand,
  outputDelta,
  workloadInputBytes,
  workloadShortfall,
  MAX_INPUT_BYTES_PER_TICK,
  retainedProcess,
  shouldRemoveRoot,
  signalRecorded,
  survivedTeardown,
  stopRecorded,
  createSpawnGate,
  main,
  accountPanes,
  registerInterruptionHandlers,
  keeperShutdownGate,
  requestKeeperShutdown,
  preserveLogs,
  PS_ENV_PIN,
  processIdentity,
  probeProcess,
  censusPanesAfterShutdown,
  captureDescendants,
} from './soak-pty-footprint.mjs';

const SELF = fileURLToPath(import.meta.url);
const MIB = 1024 * 1024;

/** A short root so the derived socket paths stay well inside the unix sun_path limit. */
function shortRoot(suffix = 'a') {
  return path.join('/tmp', `fsoak-${suffix}`);
}

function fullSpecOptions(overrides = {}) {
  return {
    sessions: PLAN_SESSIONS,
    durationMin: PLAN_DURATION_MIN,
    intervalMs: PLAN_INTERVAL_MS,
    bytesPerTick: 12288,
    ...overrides,
  };
}

/** One sample per required instant, all healthy: the shape a truthful run produces. */
function healthySamples(options, totalBytes = 1000 * MIB) {
  return expectedSampleElapsedMs(options).map((elapsedMs, index) => ({
    at: `2026-11-16T00:${String(index % 60).padStart(2, '0')}:00.000Z`,
    elapsedMs,
    daemonRssBytes: Math.floor(totalBytes / 2),
    keeperRssBytes: totalBytes - Math.floor(totalBytes / 2),
    totalBytes,
    identityVerified: true,
    routingVerified: true,
  }));
}

// ---------------------------------------------------------------------------------------------
// the argv regression: spawn(file, args) must not repeat the executable path
// ---------------------------------------------------------------------------------------------

test('keeper argv is exactly the flags, never the executable path', () => {
  const argv = keeperArgv('/tmp/run/keeper.sock');
  assert.deepEqual(argv, ['--socket', '/tmp/run/keeper.sock']);
  // `spawn(file, args)` passes args as argv[1..]; repeating the binary would make argv[1] the
  // executable path, which the keeper's strict parser rejects as `unknown argument` and exits 2.
  assert.ok(
    !argv.some((arg) => path.isAbsolute(arg) && arg.endsWith('ferryx-ptykeeper')),
    `keeper argv must not carry the executable path: ${JSON.stringify(argv)}`,
  );
  assert.equal(argv.filter((arg) => arg === '--socket').length, 1);
});

test('daemon argv carries --daemon and no executable path', () => {
  assert.deepEqual(daemonArgv(), ['--daemon']);
  // parse_launch_mode scans every argument, so --daemon anywhere in argv selects the daemon.
  assert.ok(daemonArgv().includes('--daemon'));
});

// ---------------------------------------------------------------------------------------------
// argument parsing
// ---------------------------------------------------------------------------------------------

test('parseArgs requires both binaries explicitly and rejects name lookups', () => {
  assert.throws(() => parseArgs([], {}), GuardError);
  assert.throws(
    () => parseArgs(['--daemon-bin', SELF], {}),
    (error) => error instanceof GuardError && /--keeper-bin is required/.test(error.message),
  );
});

test('parseArgs defaults to the plan and accepts the documented flags', () => {
  const options = parseArgs(['--daemon-bin', SELF, '--keeper-bin', SELF], {});
  assert.equal(options.sessions, PLAN_SESSIONS);
  assert.equal(options.durationMin, PLAN_DURATION_MIN);
  assert.equal(options.intervalMs, PLAN_INTERVAL_MS);
  assert.equal(options.bytesPerTick, 12288);
  assert.equal(options.keepRoot, false);
  assert.ok(isFullSpec(options), 'the defaults must be the plan spec');
});

test('parseArgs rejects a non-integer session count and a sub-second interval', () => {
  const base = ['--daemon-bin', SELF, '--keeper-bin', SELF];
  assert.throws(() => parseArgs([...base, '--sessions', '7.5'], {}), GuardError);
  assert.throws(() => parseArgs([...base, '--sessions', '0'], {}), GuardError);
  assert.throws(() => parseArgs([...base, '--interval-ms', '999'], {}), GuardError);
});

test('parseArgs refuses the run14b rate before anything is spawned', () => {
  const base = ['--daemon-bin', SELF, '--keeper-bin', SELF];
  assert.throws(() => parseArgs([...base, '--bytes-per-tick', '4096'], {}), GuardError);
  const options = parseArgs([...base, '--bytes-per-tick', '12288'], {});
  assert.equal(options.bytesPerTick, 12288);
});

test('isFullSpec is false for any reduced run', () => {
  assert.equal(isFullSpec(fullSpecOptions()), true);
  assert.equal(isFullSpec(fullSpecOptions({ sessions: PLAN_SESSIONS - 1 })), false);
  assert.equal(isFullSpec(fullSpecOptions({ durationMin: 1 })), false);
  assert.equal(isFullSpec(fullSpecOptions({ intervalMs: 1000 })), false);
});

// ---------------------------------------------------------------------------------------------
// saturation guard: run14b measured a fill curve, not a leak
// ---------------------------------------------------------------------------------------------

test('a rate that cannot fill the OUTPUT ring before the trend window is refused', () => {
  // run14b: 4096 B per 30 s tick needs ceil(524288 / 4096) = 128 ticks = 64 min, i.e. the ring was
  // still filling at t=60, so the last 20 minutes could only trend upward.
  assert.equal(ringFillMs(fullSpecOptions({ bytesPerTick: 4096 })), 128 * PLAN_INTERVAL_MS);
  assert.throws(
    () => assertSaturatesBeforeTrend(fullSpecOptions({ bytesPerTick: 4096 })),
    (error) =>
      error instanceof GuardError &&
      /cannot fill the 524288 byte OUTPUT ring/.test(error.message),
  );
  // The shipped rate saturates in ceil(524288 / 12288) = 43 ticks = 21.5 min, inside the deadline.
  assert.equal(ringFillMs(fullSpecOptions({ bytesPerTick: 12288 })), 43 * PLAN_INTERVAL_MS);
  assert.doesNotThrow(() => assertSaturatesBeforeTrend(fullSpecOptions({ bytesPerTick: 12288 })));
  assert.ok(
    ringFillMs(fullSpecOptions({ bytesPerTick: 12288 })) <= TREND_FROM_MS * SATURATION_DEADLINE_RATIO,
    'the default rate must saturate before the trend deadline',
  );
  // A run shorter than TREND_FROM_MS has no trend window to protect, so the rate does not matter.
  assert.doesNotThrow(() =>
    assertSaturatesBeforeTrend(fullSpecOptions({ bytesPerTick: 4096, durationMin: 2 })),
  );
  // The deadline itself is the failing side: 8739 B/tick fills in exactly 60 ticks = 30 min, which
  // leaves no settled window, so it must be refused rather than accepted at the boundary.
  assert.equal(ringFillMs(fullSpecOptions({ bytesPerTick: 8739 })), 60 * PLAN_INTERVAL_MS);
  assert.throws(() => assertSaturatesBeforeTrend(fullSpecOptions({ bytesPerTick: 8739 })), GuardError);
  // The rate the error message advises must itself pass the guard: a hint that this same check then
  // refused would send the operator in a circle.
  let advised = null;
  try {
    assertSaturatesBeforeTrend(fullSpecOptions({ bytesPerTick: 4096 }));
  } catch (error) {
    advised = /--bytes-per-tick \(>= (\d+) at this interval\)/.exec(error.message);
  }
  assert.ok(advised, 'the guard must name the smallest accepted rate');
  const advisedBytes = Number(advised[1]);
  // 59 ticks fit before the deadline, so the smallest passing rate is ceil(524288 / 59) = 8887;
  // 8886 would still need a 60th tick, which lands exactly on the boundary and is refused above.
  assert.equal(advisedBytes, 8887);
  assert.doesNotThrow(() => assertSaturatesBeforeTrend(fullSpecOptions({ bytesPerTick: advisedBytes })));
  assert.ok(
    ringFillMs(fullSpecOptions({ bytesPerTick: advisedBytes })) <
      TREND_FROM_MS * SATURATION_DEADLINE_RATIO,
  );
  assert.equal(OUTPUT_RING_CAPACITY_BYTES, 512 * 1024);
});

// ---------------------------------------------------------------------------------------------
// the retained-history bound: the ring was never the only bounded buffer
// ---------------------------------------------------------------------------------------------
// The ring filled in 21.5 min at the shipped rate, but the daemon's per-session retained ceiling at
// the plan's default budget (1536 MiB / 71 = 21.6 MiB) needed ~14.5 h. A guard that watched only the
// ring therefore passed a run whose retained history grew through the whole trend window.

test('the default run bounds retained history at 512 KiB per session', () => {
  const bound = retainedBudgetFor(PLAN_SESSIONS);
  assert.equal(bound.budgetBytes, PLAN_SESSIONS * SOAK_RETAINED_CEILING_BYTES);
  assert.equal(bound.budgetBytes, 37224448, '71 x 512 KiB');
  assert.equal(bound.minSessionHistoryBytes, SOAK_MIN_SESSION_HISTORY_BYTES);
  assert.equal(bound.minSessionHistoryBytes, 524288);
  assert.equal(bound.perSessionCeilingBytes, 524288, 'budget / sessions lands on the ceiling');
  // The daemon's validation (preferences.rs:60-66) rejects a minimum at or below libghostty's 10 KiB
  // default and one larger than the budget; both halves must survive it or the daemon silently falls
  // back to 1536 MiB / 8 MiB and every number here becomes fiction.
  assert.ok(bound.minSessionHistoryBytes > 10_000, 'must clear the libghostty floor');
  assert.ok(bound.minSessionHistoryBytes <= bound.budgetBytes, 'must not exceed the budget');

  // The split mirrors `RetentionBudget::from_total` (retained_history.rs:37-45): image = total / 17,
  // text = the remainder. Only text is produced by this workload, so text is what must fill.
  assert.equal(retainedTextCeilingBytes(524288), 524288 - Math.floor(524288 / 17));
  assert.equal(bound.retainedTextCeilingBytes, 493448);
  assert.equal(retainedTextCeilingBytes(3), 3, 'a ceiling below the divisor keeps all of it as text');
});

test('the retained ceiling follows --sessions so budget and floor can never disagree', () => {
  for (const sessions of [1, 8, 71, 200]) {
    const bound = retainedBudgetFor(sessions);
    assert.equal(bound.budgetBytes, sessions * SOAK_RETAINED_CEILING_BYTES);
    assert.equal(bound.perSessionCeilingBytes, SOAK_RETAINED_CEILING_BYTES);
    assert.ok(bound.minSessionHistoryBytes <= bound.budgetBytes);
  }
  // An unusable count is not a reason to emit a zero budget: it falls back to the plan's shape.
  assert.equal(retainedBudgetFor(0).budgetBytes, PLAN_SESSIONS * SOAK_RETAINED_CEILING_BYTES);
});

test('the default rate saturates both the ring and the retained history before the deadline', () => {
  const options = fullSpecOptions({ bytesPerTick: 12288 });
  // Ring: ceil(524288 / 12288) = 43 ticks = 21.5 min. Retained text: ceil(493448 / 12288) = 41 ticks
  // = 20.5 min. Both are inside the 30 min deadline, so the trend window opens on a settled footprint.
  assert.equal(ringFillMs(options), 43 * PLAN_INTERVAL_MS);
  assert.equal(retainedFillMs(options), 41 * PLAN_INTERVAL_MS);
  assert.doesNotThrow(() => assertSaturatesBeforeTrend(options));
  assert.ok(
    Math.max(ringFillMs(options), retainedFillMs(options)) <
      TREND_FROM_MS * SATURATION_DEADLINE_RATIO,
    'the slower of the two bounds must still settle before the deadline',
  );
});

test('a rate that fills the ring but not the retained history is refused', () => {
  // The run15 shape exactly: a deployment keeping the plan's default budget leaves each of 71 sessions
  // 1536 MiB / 71 = 22684686 B, so the text share is 21350293 B. At 12288 B/tick the 512 KiB ring is
  // full in 21.5 min but the retained history needs ceil(21350293 / 12288) = 1738 ticks = 869 min.
  const planDefaultCeiling = Math.floor((1536 * MIB) / PLAN_SESSIONS);
  const options = fullSpecOptions({ bytesPerTick: 12288, retainedCeilingBytes: planDefaultCeiling });
  assert.ok(
    ringFillMs(options) < TREND_FROM_MS * SATURATION_DEADLINE_RATIO,
    'the ring alone would look acceptable - that is what made the old guard blind',
  );
  assert.ok(
    retainedFillMs(options) >= TREND_FROM_MS * SATURATION_DEADLINE_RATIO,
    'the retained bound is the one that cannot settle',
  );
  assert.throws(
    () => assertSaturatesBeforeTrend(options),
    (error) =>
      error instanceof GuardError &&
      /retained-history text ceiling/.test(error.message) &&
      !/OUTPUT ring/.test(error.message),
    'the refusal must name the bound that actually binds',
  );
});

test('the withheld rate advice names the binding bound, and that advice is accepted', () => {
  const planDefaultCeiling = Math.floor((1536 * MIB) / PLAN_SESSIONS);
  let advised = null;
  try {
    assertSaturatesBeforeTrend(
      fullSpecOptions({ bytesPerTick: 12288, retainedCeilingBytes: planDefaultCeiling }),
    );
  } catch (error) {
    advised = /--bytes-per-tick \(>= (\d+) at this interval\)/.exec(error.message);
  }
  assert.ok(advised, 'the guard must still name a rate');
  // 59 ticks fit before the deadline, and the retained text ceiling (21350293 B) is the binding bound,
  // so the ceil is taken over it rather than over the 512 KiB ring.
  assert.equal(Number(advised[1]), Math.ceil(21350293 / 59));
  assert.doesNotThrow(() =>
    assertSaturatesBeforeTrend(
      fullSpecOptions({
        bytesPerTick: Number(advised[1]),
        retainedCeilingBytes: planDefaultCeiling,
      }),
    ),
  );
});

// ---------------------------------------------------------------------------------------------
// isolation guards
// ---------------------------------------------------------------------------------------------

test('assertIsolated refuses a root that is the production runtime', () => {
  if (typeof process.getuid !== 'function') return;
  const production = path.join('/tmp', `ferryx-${process.getuid()}`);
  assert.throws(() => assertIsolated(makeLayout(production), {}), GuardError);
});

test('assertIsolated accepts an ordinary temp root and refuses an over-long socket path', () => {
  assert.doesNotThrow(() => assertIsolated(makeLayout(shortRoot()), {}));
  // 103 bytes is the macOS sun_path ceiling; a deeper root must be refused before any bind.
  const deep = path.join('/tmp', 'f'.repeat(120), 'root');
  assert.throws(
    () => assertIsolated(makeLayout(deep), {}),
    (error) => error instanceof GuardError && /unix socket limit/.test(error.message),
  );
});

test('every isolated env location key points inside the run root', () => {
  const expected = { ...process.env };
  const layout = makeLayout(shortRoot('b'));
  const env = isolatedEnv(layout, 'run-1', expected);
  for (const key of [
    'FERRYX_RUNTIME_DIR',
    'FERRYX_LEGACY_RUNTIME_DIR',
    'FERRYX_DATA_DIR',
    'FERRYX_SESSION_DIR',
    'FERRYX_ACCOUNT_DATA_DIR',
    'FERRYX_RELAY_DATA_DIR',
    'HOME',
    'TMPDIR',
  ]) {
    assert.ok(env[key], `${key} must be set`);
    assert.ok(isInside(env[key], layout.root), `${key}=${env[key]} escaped ${layout.root}`);
  }
  assert.equal(env.FERRYX_QA_RUN_ID, 'run-1');
  // The supervisor name is what stops launchd.rs / supervisor_windows.rs installing a real job.
  assert.match(env.FERRYX_DAEMON_SUPERVISOR_NAME, /^ferryx-soak-run-1$/);
});

test('the daemon is given a retained-scrollback budget derived from the session count', () => {
  const layout = makeLayout(shortRoot('c'));
  // The daemon parses these as `usize` (preferences.rs:60-66) and silently falls back to 1536 MiB /
  // 8 MiB on anything unusable, so a wrong shape here would not fail - it would measure the default.
  const pair = scrollbackEnv(PLAN_SESSIONS);
  assert.deepEqual(pair, {
    FERRYX_SCROLLBACK_BUDGET_BYTES: '37224448',
    FERRYX_MIN_SESSION_HISTORY_BYTES: '524288',
  });
  for (const value of Object.values(pair)) {
    assert.equal(value, String(Number(value)), 'plain decimal, no separators or suffix');
  }

  const env = isolatedEnv(layout, 'run-1', {}, PLAN_SESSIONS);
  assert.equal(env.FERRYX_SCROLLBACK_BUDGET_BYTES, '37224448');
  assert.equal(env.FERRYX_MIN_SESSION_HISTORY_BYTES, '524288');
  // `--sessions` moves both, or a reduced run would be measured against the plan's budget.
  const reduced = isolatedEnv(layout, 'run-1', {}, 8);
  assert.equal(reduced.FERRYX_SCROLLBACK_BUDGET_BYTES, String(8 * SOAK_RETAINED_CEILING_BYTES));
  assert.equal(reduced.FERRYX_MIN_SESSION_HISTORY_BYTES, '524288');
  // An inherited pair from the operator's shell must never win over the run's own.
  const inherited = isolatedEnv(
    layout,
    'run-1',
    { FERRYX_SCROLLBACK_BUDGET_BYTES: '1', FERRYX_MIN_SESSION_HISTORY_BYTES: '1' },
    PLAN_SESSIONS,
  );
  assert.equal(inherited.FERRYX_SCROLLBACK_BUDGET_BYTES, '37224448');
  assert.equal(inherited.FERRYX_MIN_SESSION_HISTORY_BYTES, '524288');
});

test('isInside does not accept a sibling that merely shares a prefix', () => {
  assert.equal(isInside('/tmp/root/data', '/tmp/root'), true);
  assert.equal(isInside('/tmp/root', '/tmp/root'), true);
  assert.equal(isInside('/tmp/root-evil/x', '/tmp/root'), false);
});

// ---------------------------------------------------------------------------------------------
// process identity: pid reuse must not look like ours
// ---------------------------------------------------------------------------------------------

const PS_SAMPLE = '  4321   100  20480 Tue Nov 17 02:24:27 2026 /opt/ferryx/Ferryx\n';

test('parsePsRow reads pid, ppid, rss, start stamp and executable', () => {
  const row = parsePsRow(PS_SAMPLE);
  assert.equal(row.pid, 4321);
  assert.equal(row.ppid, 100);
  assert.equal(row.rssBytes, 20480 * 1024);
  assert.equal(row.startStamp, 'Tue Nov 17 02:24:27 2026');
  assert.equal(row.executable, '/opt/ferryx/Ferryx');
});

test('parsePsRow returns null for empty or unrecognised output', () => {
  assert.equal(parsePsRow(''), null);
  assert.equal(parsePsRow('   \n'), null);
  assert.equal(parsePsRow('garbage inputs here'), null);
});

test('identityMatches refuses a reused pid with a different start stamp or executable', () => {
  const recorded = parsePsRow(PS_SAMPLE);
  assert.equal(identityMatches(recorded, parsePsRow(PS_SAMPLE)), true);
  const restamped = parsePsRow(
    '  4321   100  20480 Wed Nov 18 09:00:00 2026 /opt/ferryx/Ferryx\n',
  );
  assert.equal(identityMatches(recorded, restamped), false, 'a later start stamp is a different process');
  const replaced = parsePsRow(
    '  4321   100  20480 Tue Nov 17 02:24:27 2026 /usr/bin/something-else\n',
  );
  assert.equal(identityMatches(recorded, replaced), false, 'a different executable is not ours');
  assert.equal(identityMatches(recorded, null), false);
  assert.equal(identityMatches(null, recorded), false);
});

// ---------------------------------------------------------------------------------------------
// keeper wire: the 60-byte header
// ---------------------------------------------------------------------------------------------

test('keeper frames round-trip and refuse a truncated buffer', () => {
  const requestId = '11111111-2222-3333-4444-555555555555';
  assert.equal(requestId.length, 36);
  const frame = encodeKeeperFrame(0x0001, requestId, 7, { protocolMajor: 1 });
  assert.equal(frame.length, 60 + Buffer.byteLength(JSON.stringify({ protocolMajor: 1 })));
  assert.equal(frame.subarray(0, 4).toString('ascii'), 'FXKP');

  assert.equal(decodeKeeperFrame(frame.subarray(0, 59)), null, 'a partial header is not a frame');

  const decoded = decodeKeeperFrame(frame);
  assert.equal(decoded.frame.kind, 0x0001);
  assert.equal(decoded.frame.requestId, requestId);
  assert.equal(decoded.frame.frameSequence, 7);
  assert.deepEqual(decoded.frame.payload, { protocolMajor: 1 });
  assert.equal(decoded.rest.length, 0);
});

test('encodeKeeperFrame refuses a request id that is not 36 bytes', () => {
  assert.throws(() => encodeKeeperFrame(0x0001, 'short', 1, {}), GuardError);
});

// ---------------------------------------------------------------------------------------------
// routing reconciliation: a session the keeper does not own is a false pass
// ---------------------------------------------------------------------------------------------

function routingFixture() {
  const ids = ['s-1', 's-2'];
  return {
    expectedIds: ids,
    liveSessions: ids.map((id) => ({ backendId: id, kind: 'local', localChildPid: 4242 })),
    keeperSessions: ids.map((id) => ({ id, owner: 'keeper' })),
    inventory: { liveLocal: 2, liveRemote: 0, livePaired: 0 },
    sessions: 2,
  };
}

test('reconcileRouting accepts a fully keeper-owned, fully local run', () => {
  const result = reconcileRouting(routingFixture());
  assert.equal(result.ok, true, result.reasons.join('; '));
  assert.equal(result.keeperOwned, 2);
  assert.equal(result.daemonOwned, 0);
  assert.equal(result.unknown, 0);
});

test('reconcileRouting refuses a run where the keeper is idle', () => {
  const fixture = routingFixture();
  const result = reconcileRouting({
    ...fixture,
    keeperSessions: fixture.expectedIds.map((id) => ({ id, owner: 'daemon' })),
  });
  assert.equal(result.ok, false);
  assert.equal(result.keeperOwned, 0);
  assert.equal(result.daemonOwned, 2);
  assert.ok(result.reasons.some((reason) => /not "keeper"/.test(reason)));
});

test('reconcileRouting refuses a missing session, a wrong inventory and a non-local pane', () => {
  const missing = reconcileRouting({ ...routingFixture(), keeperSessions: [{ id: 's-1', owner: 'keeper' }] });
  assert.equal(missing.ok, false);
  assert.ok(missing.reasons.some((reason) => /OwnershipList is missing/.test(reason)));

  const wrongInventory = reconcileRouting({
    ...routingFixture(),
    inventory: { liveLocal: 1, liveRemote: 1, livePaired: 0 },
  });
  assert.equal(wrongInventory.ok, false);
  assert.ok(wrongInventory.reasons.some((reason) => /live_local=1/.test(reason)));

  const remote = reconcileRouting({
    ...routingFixture(),
    liveSessions: [
      { backendId: 's-1', kind: 'ssh', localChildPid: 4242 },
      { backendId: 's-2', kind: 'local', localChildPid: 4242 },
    ],
  });
  assert.equal(remote.ok, false);
  assert.ok(remote.reasons.some((reason) => /not local/.test(reason)));
});

test('reconcileRouting refuses a local session with no child pid', () => {
  const result = reconcileRouting({
    ...routingFixture(),
    liveSessions: [
      { backendId: 's-1', kind: 'local', localChildPid: null },
      { backendId: 's-2', kind: 'local', localChildPid: 4242 },
    ],
  });
  assert.equal(result.ok, false);
  assert.ok(result.reasons.some((reason) => /no child pid/.test(reason)));
});

// ---------------------------------------------------------------------------------------------
// the rev-5 acceptance arithmetic
// ---------------------------------------------------------------------------------------------

test('expectedSampleElapsedMs covers t=0 through the final instant inclusively', () => {
  const instants = expectedSampleElapsedMs({ durationMin: 60, intervalMs: 30_000 });
  assert.equal(instants[0], 0);
  assert.equal(instants.at(-1), 60 * 60_000);
  assert.equal(instants.length, 121);
  assert.ok(instants.includes(TREND_FROM_MS), 'the t=40 min reference instant must be sampled');
});

test('a complete, flat run passes', () => {
  const options = fullSpecOptions();
  const evaluation = evaluateSoak({ samples: healthySamples(options), options, fullSpec: true });
  assert.equal(evaluation.verdict, 'pass', evaluation.unverified.join('; '));
  assert.equal(evaluation.exitCode, 0);
  assert.equal(evaluation.breaches.length, 0);
  assert.ok(evaluation.trendSamples > 0);
  assert.equal(evaluation.reference.elapsedMs, TREND_FROM_MS);
});

test('a run above 2.5 GiB combined breaches the hard target', () => {
  const options = fullSpecOptions();
  const samples = healthySamples(options);
  samples[90] = { ...samples[90], totalBytes: TARGET_BYTES + 1 };
  const evaluation = evaluateSoak({ samples, options, fullSpec: true });
  assert.equal(evaluation.verdict, 'breach');
  assert.equal(evaluation.exitCode, 1);
  assert.ok(evaluation.breaches.some((breach) => breach.kind === 'target-exceeded'));
});

test('exactly at the 2.5 GiB target is not a breach', () => {
  const options = fullSpecOptions();
  const samples = healthySamples(options, TARGET_BYTES);
  const evaluation = evaluateSoak({ samples, options, fullSpec: true });
  assert.equal(evaluation.verdict, 'pass', evaluation.unverified.join('; '));
});

test('an upward trend past +/-5 % of the t=40 min sample breaches', () => {
  const options = fullSpecOptions();
  const reference = 1000 * MIB;
  const samples = healthySamples(options, reference);
  // t=50 min is 10 % above the reference: outside the 5 % band.
  const index = samples.findIndex((sample) => sample.elapsedMs === 50 * 60_000);
  assert.ok(index > 0);
  samples[index] = { ...samples[index], totalBytes: Math.round(reference * 1.1) };
  const evaluation = evaluateSoak({ samples, options, fullSpec: true });
  assert.equal(evaluation.verdict, 'breach');
  assert.ok(evaluation.breaches.some((breach) => breach.kind === 'upward-trend'));
});

test('a fall past +/-5 % of the t=40 min sample also breaches (the band is symmetric)', () => {
  const options = fullSpecOptions();
  const reference = 1000 * MIB;
  const samples = healthySamples(options, reference);
  const index = samples.findIndex((sample) => sample.elapsedMs === 45 * 60_000);
  samples[index] = { ...samples[index], totalBytes: Math.round(reference * 0.9) };
  const evaluation = evaluateSoak({ samples, options, fullSpec: true });
  assert.equal(evaluation.verdict, 'breach');
  assert.ok(evaluation.breaches.some((breach) => breach.kind === 'upward-trend'));
});

test('a missing sample is unverified, never a pass', () => {
  const options = fullSpecOptions();
  const samples = healthySamples(options).filter((sample) => sample.elapsedMs !== 90_000);
  const evaluation = evaluateSoak({ samples, options, fullSpec: true });
  assert.equal(evaluation.verdict, 'unverified');
  assert.equal(evaluation.exitCode, 4);
  assert.ok(evaluation.unverified.some((reason) => /no sample within/.test(reason)));
});

test('a sample without both RSS readings or without verification is unverified', () => {
  const options = fullSpecOptions();
  const samples = healthySamples(options);
  samples[5] = { ...samples[5], keeperRssBytes: 0 };
  samples[6] = { ...samples[6], identityVerified: false };
  samples[7] = { ...samples[7], routingVerified: false };
  const evaluation = evaluateSoak({ samples, options, fullSpec: true });
  assert.equal(evaluation.verdict, 'unverified');
  assert.ok(evaluation.unverified.some((reason) => /no keeper RSS/.test(reason)));
  assert.ok(evaluation.unverified.some((reason) => /did not verify both process identities/.test(reason)));
  assert.ok(evaluation.unverified.some((reason) => /did not verify keeper ownership/.test(reason)));
});

test('a reduced run cannot pass even when its samples are perfect', () => {
  const options = fullSpecOptions({ sessions: 10, durationMin: 2 });
  const evaluation = evaluateSoak({
    samples: healthySamples(options),
    options,
    fullSpec: isFullSpec(options),
  });
  assert.equal(evaluation.verdict, 'unverified');
  assert.equal(evaluation.exitCode, 4);
  assert.ok(evaluation.unverified.some((reason) => /cannot pass/.test(reason)));
});

test('unverified outranks breach, so an unprovable run is never reported as a breach', () => {
  const options = fullSpecOptions();
  const samples = healthySamples(options);
  samples[30] = { ...samples[30], totalBytes: TARGET_BYTES + 1 };
  const withoutOne = samples.filter((sample) => sample.elapsedMs !== 600_000);
  const evaluation = evaluateSoak({ samples: withoutOne, options, fullSpec: true });
  assert.equal(evaluation.verdict, 'unverified');
  assert.equal(evaluation.exitCode, 4);
  assert.ok(evaluation.breaches.length > 0, 'the breach is still recorded, just not reported as the verdict');
});

// ---------------------------------------------------------------------------------------------
// scripted workload: a short command that makes the child print the volume
// ---------------------------------------------------------------------------------------------

test('outputCommand makes the child print the requested bytes and types almost nothing', () => {
  assert.equal(outputCommand(0), null, 'a zero budget writes nothing at all');
  assert.equal(outputCommand(12288), `head -c 12288 /dev/zero | tr '\\000' x`);
  // The whole point of the rework: the harness types a command, the child produces the payload.
  // run14d typed the 12 KiB itself, hit MAX_CANON plus tty echo, and took 139-366 s per tick.
  assert.ok(
    workloadInputBytes(12288) <= MAX_INPUT_BYTES_PER_TICK,
    `input ${workloadInputBytes(12288)} B must stay inside the budget`,
  );
  assert.ok(workloadInputBytes(12288) < 1024, 'the typed line must stay inside MAX_CANON on macOS');
  // No newline in the command's output: ONLCR would turn each produced byte into two and the
  // saturation arithmetic would no longer describe the ring.
  assert.ok(!outputCommand(12288).includes('\n'));
});

test('the typed workload stays tiny for every plausible output rate', () => {
  // The input budget is met by construction because the harness types a command, not the payload -
  // but it is checked rather than assumed, and parseArgs refuses a run above the budget. Pinned for
  // the rates an operator could plausibly choose, from a smoke rate to 1 MiB per tick.
  for (const bytes of [1, 4096, 12288, 8887, 65_536, 1_048_576]) {
    const typed = workloadInputBytes(bytes);
    assert.ok(typed <= MAX_INPUT_BYTES_PER_TICK, `${bytes} B/tick must type at most ${MAX_INPUT_BYTES_PER_TICK} B`);
    assert.ok(typed > 0, 'a non-zero rate must type a command');
  }
  // With this workload the guard cannot fire - a command line is bounded at ~40 B however large the
  // rate - and that is the point: it is a tripwire for a future workload that goes back to typing the
  // payload at the tty. `parseArgs` calls it, so such a regression is refused before a run starts.
  assert.doesNotThrow(() => parseArgs(['--daemon-bin', SELF, '--keeper-bin', SELF], {}));
});

test('produced output is measured from the keeper replay head, not from what was written', () => {
  const sessions = [
    { id: 's-1', sequenceOffset: 100 },
    { id: 's-2', sequenceOffset: 50 },
  ];
  const baseline = outputDelta(sessions, new Map(), ['s-1', 's-2']);
  assert.equal(baseline.bytes, 0, 'the first read is a baseline, never a delta');
  assert.equal(baseline.reason, null);
  const second = outputDelta(
    [
      { id: 's-1', sequenceOffset: 12_388 },
      { id: 's-2', sequenceOffset: 12_338 },
    ],
    baseline.offsets,
    ['s-1', 's-2'],
  );
  assert.equal(second.bytes, 12_388 - 100 + (12_338 - 50));
  assert.equal(second.reason, null);
  // A head that went backwards is a reset or a replaced session: reported, never counted as output.
  const backwards = outputDelta([{ id: 's-1', sequenceOffset: 10 }], baseline.offsets, ['s-1']);
  assert.equal(backwards.bytes, 0);
  assert.match(backwards.reason, /went backwards/);
  // An unreadable head is a failed measurement, not zero output.
  const unreadable = outputDelta([{ id: 's-1' }], baseline.offsets, ['s-1']);
  assert.match(unreadable.reason, /no readable sequenceOffset/);
});

test('a workload that produced nothing cannot pass, however flat the footprint is', () => {
  // The last tick is still in flight at the final read, so the floor is the completed ticks.
  assert.equal(workloadShortfall({ sessions: 71, bytesPerTick: 12288, ticksWritten: 0, producedBytes: 0 }), null);
  assert.equal(
    workloadShortfall({ sessions: 71, bytesPerTick: 12288, ticksWritten: 1, producedBytes: 0 }),
    null,
    'one written tick has no completed tick to demand output from',
  );
  assert.equal(
    workloadShortfall({ sessions: 71, bytesPerTick: 12288, ticksWritten: 121, producedBytes: 71 * 12288 * 120 }),
    null,
    'exactly the asked-for volume over the completed ticks satisfies the floor',
  );
  const short = workloadShortfall({ sessions: 71, bytesPerTick: 12288, ticksWritten: 121, producedBytes: 0 });
  assert.match(short, /produced 0 B of the 104693760 B/);
  assert.match(short, /OUTPUT ring was never filled/);
  // A shell that printed only a fraction of the volume is caught too, not just a silent one.
  assert.ok(workloadShortfall({ sessions: 71, bytesPerTick: 12288, ticksWritten: 121, producedBytes: 71 * 12288 * 60 }));
  // A zero budget is a deliberate no-output run and has no floor to miss.
  assert.equal(workloadShortfall({ sessions: 71, bytesPerTick: 0, ticksWritten: 120, producedBytes: 0 }), null);
});

test('a keeper socket path stays inside the per-run root', () => {
  const layout = makeLayout(shortRoot('c'));
  const argv = keeperArgv(layout.keeperSocket);
  assert.ok(isInside(argv[1], layout.root));
  // The keeper refuses a `--socket` whose parent is not a private, group/other-unwritable directory
  // (crates/ferryx-ptykeeper/src/main.rs::require_private_parent), which ensureLayout's 0o700 satisfies.
  assert.ok(isInside(layout.keeperSocket, layout.runtime));
});

// ---------------------------------------------------------------------------------------------
// teardown: no spawn-time identity means no signal, ever - the process is reported instead
// ---------------------------------------------------------------------------------------------

function fakeChild(pid) {
  return { pid };
}

/** Records every signal instead of sending one, and counts the `ps` reads a teardown performed. */
function spy() {
  const signals = [];
  let identityReads = 0;
  return {
    signals,
    get identityReads() { return identityReads; },
    readIdentity(pid) {
      identityReads += 1;
      return { pid, ppid: process.pid, rssBytes: 1, startStamp: 'Tue Nov 17 02:24:27 2026', executable: '/bin/x' };
    },
    kill(pid, signal) {
      signals.push([pid, signal]);
    },
  };
}

test('signalRecorded never signals when the spawn-time identity was never captured', () => {
  const s = spy();
  for (const missing of [null, undefined, {}, { pid: 4321 }]) {
    assert.equal(signalRecorded(missing, 'SIGKILL', s.readIdentity, s.kill), false);
  }
  assert.deepEqual(s.signals, [], 'no recorded identity must mean no signal, not a bare-pid guess');
  // A record without a start stamp is still not an identity: it must fail the match, not fall back.
  assert.equal(
    signalRecorded({ pid: 4321, executable: '/bin/x' }, 'SIGKILL', s.readIdentity, s.kill),
    false,
  );
  assert.deepEqual(s.signals, []);
});

test('signalRecorded refuses a reused pid whose start stamp or executable changed', () => {
  const s = spy();
  const recorded = parsePsRow(PS_SAMPLE);
  const restamped = (pid) => ({ pid, ppid: 100, rssBytes: 1, startStamp: 'Wed Nov 18 09:00:00 2026', executable: '/opt/ferryx/Ferryx' });
  const replaced = (pid) => ({ pid, ppid: 100, rssBytes: 1, startStamp: 'Tue Nov 17 02:24:27 2026', executable: '/usr/bin/other' });
  assert.equal(signalRecorded(recorded, 'SIGTERM', restamped, s.kill), false);
  assert.equal(signalRecorded(recorded, 'SIGTERM', replaced, s.kill), false);
  assert.deepEqual(s.signals, [], 'a pid that is no longer the recorded process is a stranger');
});

test('signalRecorded signals only the exactly matching recorded process', () => {
  const s = spy();
  const recorded = parsePsRow(PS_SAMPLE);
  assert.equal(signalRecorded(recorded, 'SIGTERM', s.readIdentity, s.kill), false, 'fixture executable differs');
  const matching = (pid) => ({ pid, ppid: 100, rssBytes: 1, startStamp: recorded.startStamp, executable: recorded.executable });
  assert.equal(signalRecorded(recorded, 'SIGTERM', matching, s.kill), true);
  assert.deepEqual(s.signals, [[recorded.pid, 'SIGTERM']]);
});

test('a retained child is reported by pid and label and never signalled', () => {
  const s = spy();
  const entry = retainedProcess(fakeChild(4321), 'keeper');
  assert.deepEqual(entry, { label: 'keeper', pid: 4321, reason: 'spawn-time-identity-unreadable' });
  assert.deepEqual(s.signals, [], 'reporting a retained process must not signal it');
});

test('collectRetained lists only children whose identity was never recorded', () => {
  const keeper = fakeChild(4321);
  const daemon = fakeChild(4322);
  const both = collectRetained({ keeper, keeperRecorded: null, daemon, daemonRecorded: null });
  assert.deepEqual(both.map((e) => [e.label, e.pid]), [['keeper', 4321], ['daemon', 4322]]);
  // A recorded child is stopped by stopRecorded, so reporting it would keep every run's root forever.
  assert.deepEqual(collectRetained({ keeper, keeperRecorded: { pid: 4321 }, daemon, daemonRecorded: { pid: 4322 } }), []);
  assert.deepEqual(
    collectRetained({ keeper, keeperRecorded: { pid: 4321 }, daemon, daemonRecorded: null }).map((e) => e.label),
    ['daemon'],
  );
  assert.deepEqual(collectRetained({ keeper: null, keeperRecorded: null, daemon: null, daemonRecorded: null }), []);
});

test('a child that survives teardown is detected only while its identity still matches', () => {
  const recorded = parsePsRow(PS_SAMPLE);
  const matching = (pid) => ({ pid, ppid: 100, rssBytes: 1, startStamp: recorded.startStamp, executable: recorded.executable });
  assert.equal(survivedTeardown(recorded, matching), true, 'same start stamp and executable = still alive');
  assert.equal(survivedTeardown(recorded, () => null), false, 'gone');
  // A pid now held by a different process means our child exited; that stranger is not ours to report.
  assert.equal(
    survivedTeardown(recorded, (pid) => ({ pid, ppid: 1, rssBytes: 1, startStamp: 'Wed Nov 18 09:00:00 2026', executable: '/bin/other' })),
    false,
  );
  assert.equal(survivedTeardown(null, matching), false);
});

test('a survivor is reported with its own reason, not the unreadable-identity one', () => {
  assert.deepEqual(retainedProcess(fakeChild(4321), 'daemon', 'survived-teardown'), {
    label: 'daemon',
    pid: 4321,
    reason: 'survived-teardown',
  });
});

test('a retained report carries no runtime, environment or socket detail', () => {
  const entry = retainedProcess(fakeChild(4321), 'daemon');
  const leaked = JSON.stringify(entry);
  for (const forbidden of ['/run', 'sock', 'FERRYX', 'HOME', 'runtime', '/Users', '/Volumes', 'TMPDIR']) {
    assert.equal(leaked.includes(forbidden), false, `retained report must not contain ${forbidden}`);
  }
  assert.deepEqual(Object.keys(entry).sort(), ['label', 'pid', 'reason']);
});

test('an absent child yields no retained record', () => {
  assert.equal(retainedProcess(null, 'keeper'), null);
  assert.equal(retainedProcess(fakeChild(undefined), 'keeper'), null);
  assert.equal(retainedProcess(fakeChild(0), 'keeper'), null);
});

// ---------------------------------------------------------------------------------------------
// run-root lifetime: never deleted while a child we declined to signal may still be using it
// ---------------------------------------------------------------------------------------------

test('the run root survives every non-pass, so a breach keeps the logs that explain it', () => {
  for (const verdict of ['breach', 'unverified', 'interrupted', undefined]) {
    assert.equal(shouldRemoveRoot({ keepRoot: false, retained: [], verdict }), false, `verdict ${verdict} keeps logs`);
  }
  assert.equal(
    shouldRemoveRoot({ keepRoot: false, retained: [], verdict: 'pass', panesConfirmedGone: true, subtreeAbsence: 'proven' }),
    true,
    'a pass with panes and subtree proven gone may reclaim the root',
  );
});

test('a retained child keeps the root even on a pass', () => {
  assert.equal(
    shouldRemoveRoot({ keepRoot: false, retained: [{ label: 'keeper', pid: 4321 }], verdict: 'pass' }),
    false,
    'a retained child owns its cwd/HOME/sockets inside the root',
  );
  assert.equal(shouldRemoveRoot({ keepRoot: true, retained: [], verdict: 'pass' }), false, '--keep-root always keeps it');
});

test('the root survives for each retained child independently', () => {
  for (const label of ['keeper', 'daemon']) {
    const entry = retainedProcess(fakeChild(4321), label);
    assert.equal(shouldRemoveRoot({ keepRoot: false, retained: [entry] }), false, `${label} keeps the root`);
  }
});

test('shouldRemoveRoot is total, and errs toward keeping the root on odd input', () => {
  // No verdict is not a pass: an unknown outcome must not be allowed to delete evidence.
  assert.equal(shouldRemoveRoot(), false);
  assert.equal(shouldRemoveRoot({}), false);
  assert.equal(shouldRemoveRoot({ retained: [] }), false);
  assert.equal(shouldRemoveRoot({ retained: [{ pid: 1 }], verdict: 'pass' }), false);
  assert.equal(shouldRemoveRoot({ retained: [], verdict: 'pass' }), false, 'unproven panes keep the root');
  assert.equal(
    shouldRemoveRoot({ retained: [], verdict: 'pass', panesConfirmedGone: true }),
    false,
    'pane proof alone never licenses removal: the subtree is still unproven',
  );
  assert.equal(
    shouldRemoveRoot({ retained: [], verdict: 'pass', panesConfirmedGone: true, subtreeAbsence: 'proven' }),
    true,
    'only both proofs together license removal',
  );
});

// ---------------------------------------------------------------------------------------------
// B1: interruption reaches bounded teardown, exactly once, and states what it cannot cover
// ---------------------------------------------------------------------------------------------

/** A fake emitter: no real signal is ever sent, and listener wiring is inspectable. */
function fakeEmitter(supported = ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  const listeners = new Map();
  return {
    on(signal, fn) {
      if (!supported.includes(signal)) throw new Error(`unsupported signal ${signal}`);
      if (!listeners.has(signal)) listeners.set(signal, []);
      listeners.get(signal).push(fn);
    },
    removeListener(signal, fn) {
      listeners.set(signal, (listeners.get(signal) ?? []).filter((entry) => entry !== fn));
    },
    emit(signal) {
      for (const fn of [...(listeners.get(signal) ?? [])]) fn(signal);
    },
    count(signal) {
      return (listeners.get(signal) ?? []).length;
    },
  };
}

test('an interrupt triggers teardown once, even if several signals arrive', () => {
  const target = fakeEmitter();
  const fired = [];
  const registration = registerInterruptionHandlers({ target, onInterrupt: (signal) => fired.push(signal) });
  assert.deepEqual(registration.installed, ['SIGINT', 'SIGTERM', 'SIGHUP']);
  target.emit('SIGINT');
  target.emit('SIGINT');
  target.emit('SIGTERM');
  assert.deepEqual(fired, ['SIGINT'], 'a second signal must not start a competing teardown');
});

test('interrupt handlers are disposed, so teardown cannot re-enter after it finished', () => {
  const target = fakeEmitter();
  const fired = [];
  const registration = registerInterruptionHandlers({ target, onInterrupt: (signal) => fired.push(signal) });
  registration.dispose();
  assert.deepEqual(['SIGINT', 'SIGTERM', 'SIGHUP'].map((signal) => target.count(signal)), [0, 0, 0]);
  target.emit('SIGINT');
  assert.deepEqual(fired, []);
});

test('a platform without SIGHUP still installs the signals it has', () => {
  const target = fakeEmitter(['SIGINT', 'SIGTERM']);
  const registration = registerInterruptionHandlers({ target, onInterrupt: () => {} });
  assert.deepEqual(registration.installed, ['SIGINT', 'SIGTERM']);
});

// ---------------------------------------------------------------------------------------------
// B5: stopRecorded - the escalation is bounded, and a lost identity is never escalated
// ---------------------------------------------------------------------------------------------

/** A clock and `ps` stub that advance only when the code sleeps, so no real time is spent. */
function fakeWorld({ rows, advanceMs = 100 }) {
  let clock = 0;
  const signals = [];
  return {
    signals,
    now: () => clock,
    sleep: async () => { clock += advanceMs; },
    run: (_cmd, args) => {
      const pid = Number(String(args[1]));
      const row = rows(clock, pid);
      return row ? { status: 0, stdout: `${row}\n` } : { status: 1, stdout: '' };
    },
    kill: (pid, signal) => { signals.push([pid, signal]); },
  };
}

const PS_ROW = '4321 100 2048 Tue Nov 17 02:24:27 2026 /opt/ferryx/Ferryx';
const recordedKeeper = parsePsRow(PS_ROW);

test('stopRecorded sends SIGTERM and stops at SIGTERM once the identity is gone', async () => {
  const world = fakeWorld({ rows: (clock) => (clock === 0 ? PS_ROW : null) });
  const result = await stopRecorded([recordedKeeper], {
    waitMs: 5_000, now: world.now, sleep: world.sleep, run: world.run, kill: world.kill,
  });
  assert.deepEqual(world.signals, [[4321, 'SIGTERM']], 'no SIGKILL once the process is gone');
  assert.deepEqual(result.escalated, []);
});

test('stopRecorded escalates to SIGKILL only after the bounded wait', async () => {
  const world = fakeWorld({ rows: () => PS_ROW }); // never dies
  const result = await stopRecorded([recordedKeeper], {
    waitMs: 5_000, now: world.now, sleep: world.sleep, run: world.run, kill: world.kill,
  });
  assert.deepEqual(world.signals, [[4321, 'SIGTERM'], [4321, 'SIGKILL']]);
  assert.deepEqual(result.escalated.map((entry) => entry.pid), [4321]);
  assert.ok(world.now() <= 5_000 + 100, `wait overran its bound: ${world.now()} ms`);
});

test('stopRecorded refuses to escalate a pid that was reused while waiting', async () => {
  // Ours at t=0 (so SIGTERM is sent), then a different process holds the number: same pid, new stamp.
  const world = fakeWorld({
    rows: (clock) => (clock === 0 ? PS_ROW : '4321 100 2048 Wed Nov 18 09:00:00 2026 /usr/bin/other'),
  });
  const result = await stopRecorded([recordedKeeper], {
    waitMs: 5_000, now: world.now, sleep: world.sleep, run: world.run, kill: world.kill,
  });
  assert.deepEqual(world.signals, [[4321, 'SIGTERM']], 'a stranger with the same pid is never SIGKILLed');
  assert.deepEqual(result.escalated, []);
});

test('stopRecorded signals nothing when no identity was recorded', async () => {
  const world = fakeWorld({ rows: () => PS_ROW });
  const result = await stopRecorded([null, undefined, {}], {
    now: world.now, sleep: world.sleep, run: world.run, kill: world.kill,
  });
  assert.deepEqual(world.signals, []);
  assert.deepEqual(result, { signalled: [], escalated: [] });
});

test('stopRecorded tolerates an empty or absent list', async () => {
  const world = fakeWorld({ rows: () => PS_ROW });
  const opts = { now: world.now, sleep: world.sleep, run: world.run, kill: world.kill };
  assert.deepEqual(await stopRecorded([], opts), { signalled: [], escalated: [] });
  assert.deepEqual(await stopRecorded(undefined, opts), { signalled: [], escalated: [] });
  assert.deepEqual(world.signals, []);
});

// ---------------------------------------------------------------------------------------------
// B4: pane pids are accounting metadata only, never signal authority
// ---------------------------------------------------------------------------------------------

test('all 71 panes are accounted when the daemon reports a positive pid for each', () => {
  const ids = Array.from({ length: 71 }, (_, index) => `s-${index}`);
  const live = ids.map((id, index) => ({ backendId: id, kind: 'local', localChildPid: 5000 + index }));
  const state = accountPanes(live, ids);
  assert.equal(state.accounted, true);
  assert.equal(state.panes.length, 71);
  assert.deepEqual(state.unaccounted, []);
  assert.deepEqual(state.panes[0], { sessionId: 's-0', panePid: 5000 });
});

test('a missing, zero, negative or non-integer pane pid is unaccounted, not silently accepted', () => {
  const ids = ['a', 'b', 'c', 'd'];
  const live = [
    { backendId: 'a', localChildPid: 0 },
    { backendId: 'b', localChildPid: -1 },
    { backendId: 'c', localChildPid: '1234' },
    { backendId: 'd' },
  ];
  const state = accountPanes(live, ids);
  assert.equal(state.accounted, false);
  assert.deepEqual(state.unaccounted, ['a', 'b', 'c', 'd']);
  assert.deepEqual(state.panes, []);
});

test('a session the daemon never described is unaccounted', () => {
  const state = accountPanes([{ backendId: 'a', localChildPid: 42 }], ['a', 'b']);
  assert.equal(state.accounted, false);
  assert.deepEqual(state.unaccounted, ['b']);
});

test('pane accounting is a pure read: it never signals and never mutates its input', () => {
  const live = [{ backendId: 'a', localChildPid: 42 }];
  const before = JSON.stringify(live);
  accountPanes(live, ['a']);
  assert.equal(JSON.stringify(live), before);
  assert.equal(accountPanes([], []).accounted, true);
});

// ---------------------------------------------------------------------------------------------
// B3 + keeper Shutdown: only the run-owned keeper may be asked, and only when isolation is proved
// ---------------------------------------------------------------------------------------------

const LAYOUT = makeLayout('/private/tmp/ferryx-soak-abc');
const GOOD_PROOF = { pid: 4321, spawnedPid: 4321, socketPath: LAYOUT.keeperSocket };

test('Shutdown is allowed only for the run-owned keeper on a socket inside the run root', () => {
  assert.deepEqual(keeperShutdownGate({ proof: GOOD_PROOF, layout: LAYOUT, env: {} }), { allowed: true });
});

test('Shutdown is refused when no HELLO proof exists', () => {
  const gate = keeperShutdownGate({ proof: null, layout: LAYOUT, env: {} });
  assert.equal(gate.allowed, false);
  // `reason` is the token that lands in `summary.keeperShutdown.reason`, so it is pinned exactly
  // rather than by a prose fragment: a refusal that starts reading as allowed must fail here.
  assert.equal(gate.reason, 'keeper-identity-never-verified');
});

test('Shutdown is refused when the keeper answers as a pid this run did not spawn', () => {
  const gate = keeperShutdownGate({ proof: { ...GOOD_PROOF, pid: 9999 }, layout: LAYOUT, env: {} });
  assert.equal(gate.allowed, false);
  assert.equal(gate.reason, 'keeper-hello-pid-does-not-match-the-pid-this-run-spawned');
});

test('Shutdown is refused for a socket outside the run root', () => {
  const foreign = keeperShutdownGate({
    proof: { ...GOOD_PROOF, socketPath: '/tmp/somebody-else/run/keeper.sock' },
    layout: LAYOUT,
    env: {},
  });
  assert.equal(foreign.allowed, false);
  assert.equal(foreign.reason, 'keeper-socket-is-outside-the-run-root');
  // A sibling directory whose name merely starts with the root's is still outside it.
  const sibling = keeperShutdownGate({
    proof: { ...GOOD_PROOF, socketPath: `${LAYOUT.root}-evil/run/keeper.sock` },
    layout: LAYOUT,
    env: {},
  });
  assert.equal(sibling.allowed, false);
  assert.equal(sibling.reason, 'keeper-socket-is-outside-the-run-root');
});

test('Shutdown is refused when the socket is inside the production runtime', () => {
  // A layout that was itself built wrong must not be able to authorise a production Shutdown. The
  // production dir is derived exactly as the gate derives it, so the case does not silently stop
  // being a production case when the host's uid is not 501.
  const production = productionRuntimeDir({});
  assert.ok(production, 'this platform must have a production runtime dir for this case to exist');
  const badLayout = makeLayout(production);
  const gate = keeperShutdownGate({
    proof: { pid: 4321, spawnedPid: 4321, socketPath: badLayout.keeperSocket },
    layout: badLayout,
    env: {},
  });
  assert.equal(gate.allowed, false);
  assert.equal(gate.reason, 'keeper-socket-is-inside-the-production-runtime');
});

test('requestKeeperShutdown never touches the socket when the gate refuses', async () => {
  let called = 0;
  const connection = { call: async () => { called += 1; return { kind: 0x00f1 }; } };
  const result = await requestKeeperShutdown(connection, { proof: null, layout: LAYOUT, env: {} });
  assert.equal(result.requested, false);
  assert.equal(result.refused, true);
  assert.equal(called, 0, 'a refused Shutdown must never reach the keeper');
});

test('requestKeeperShutdown reports acknowledgement, refusal and transport failure distinctly', async () => {
  const ok = await requestKeeperShutdown(
    { call: async () => ({ kind: 0x00f1 }) },
    { proof: GOOD_PROOF, layout: LAYOUT, env: {} },
  );
  assert.deepEqual(ok, { requested: true, acknowledged: true });

  const refused = await requestKeeperShutdown(
    { call: async () => ({ kind: 0x00f0, payload: { code: 'ShuttingDown' } }) },
    { proof: GOOD_PROOF, layout: LAYOUT, env: {} },
  );
  assert.equal(refused.requested, true);
  assert.equal(refused.acknowledged, false);
  assert.match(refused.reason, /ShuttingDown/);

  const dropped = await requestKeeperShutdown(
    { call: async () => { throw new Error('connection to keeper closed'); } },
    { proof: GOOD_PROOF, layout: LAYOUT, env: {} },
  );
  assert.equal(dropped.acknowledged, false);
  assert.match(dropped.reason, /closed/);
});

test('preserveLogs copies each existing log into the evidence dir and never throws', () => {
  const copied = [];
  const result = preserveLogs(
    '/evidence',
    LAYOUT,
    (from, to) => copied.push([from, to]),
    (candidate) => !candidate.endsWith('keeper.stderr.log'),
  );
  assert.deepEqual(result.copied, ['daemon.stdout.log', 'daemon.stderr.log']);
  assert.deepEqual(copied[0], [path.join(LAYOUT.logs, 'daemon.stdout.log'), path.join('/evidence', 'daemon.stdout.log')]);

  const failed = preserveLogs('/evidence', LAYOUT, () => { throw new Error('ENOSPC'); }, () => true);
  assert.deepEqual(failed.copied, []);
  assert.match(failed.error, /ENOSPC/);
});

// ---------------------------------------------------------------------------------------------
// B6: identity reads are locale-pinned, because `lstart` is localised output
// ---------------------------------------------------------------------------------------------

test('the ps identity read pins the locale, so lstart parses on a localised host', () => {
  const seen = [];
  const run = (_cmd, _args, options) => {
    seen.push(options);
    return { status: 0, stdout: `${PS_ROW}\n` };
  };
  const identity = processIdentity(4321, run);
  assert.equal(seen.length, 1);
  assert.equal(seen[0].env.LC_ALL, 'C');
  assert.equal(seen[0].env.LANG, 'C');
  assert.equal(identity.pid, 4321);
  assert.equal(identity.startStamp, 'Tue Nov 17 02:24:27 2026');
});

test('the pinned locale is exported and covers every variable ps consults', () => {
  assert.deepEqual({ ...PS_ENV_PIN }, { LC_ALL: 'C', LANG: 'C', LC_TIME: 'C' });
  assert.ok(Object.isFrozen(PS_ENV_PIN));
});

test('a localised lstart is still rejected rather than misparsed', () => {
  // Pinning fixes the host; a translated row must still never become a bogus identity.
  assert.equal(parsePsRow('4321 100 2048 Di 17 Nov 02:24:27 2026 /opt/ferryx/Ferryx'), null);
});

// ---------------------------------------------------------------------------------------------
// R1: no child may be spawned after teardown begins - including SIGTERM in the startup window
// ---------------------------------------------------------------------------------------------

/** A spawn stub that records calls and returns a fake child. Starts no process. */
function fakeSpawner() {
  const calls = [];
  const spawnFn = (file, args, options) => {
    calls.push({ file, args, options });
    return { pid: 9000 + calls.length, fake: true };
  };
  return { calls, spawnFn };
}

test('SIGTERM after handler registration but before the daemon spawn leaves no daemon behind', () => {
  // This reproduces R1's exact sequence inside one test: register, deliver the signal, then attempt
  // the spawn that used to run unconditionally. The gate must refuse it.
  const target = fakeEmitter();
  const { calls, spawnFn } = fakeSpawner();
  const gate = createSpawnGate({ spawnFn });
  let teardownBegan = null;

  registerInterruptionHandlers({
    target,
    onInterrupt: (signal) => {
      // Teardown begins here, synchronously, exactly as `finish` does it before its first await.
      gate.close('interrupted');
      teardownBegan = signal;
    },
  });

  target.emit('SIGTERM');
  assert.equal(teardownBegan, 'SIGTERM', 'the signal must reach the handler');

  const daemonAdmission = gate.admit('daemon', '/bin/false', ['--daemon'], {});
  assert.equal(daemonAdmission.ok, false, 'no daemon may be spawned after teardown began');
  assert.equal(daemonAdmission.child, null);
  assert.equal(daemonAdmission.reason, 'interrupted');
  assert.deepEqual(calls, [], 'the spawn function must never have been called');
});

test('a signal during an await does not reopen a gate that teardown already shut', async () => {
  // The interleaving the gate exists for: shutdown happens while async work is in flight, and the
  // code resumes afterwards and reaches a spawn site. It must find the gate closed.
  const { calls, spawnFn } = fakeSpawner();
  const gate = createSpawnGate({ spawnFn });
  const before = gate.admit('keeper', '/bin/true', [], {});
  assert.equal(before.ok, true);

  const inFlight = (async () => {
    await Promise.resolve();
    gate.close('interrupted'); // teardown decided on this tick
    await Promise.resolve();
    return gate.admit('daemon', '/bin/false', ['--daemon'], {});
  })();

  const after = await inFlight;
  assert.equal(after.ok, false, 'the spawn after the await must be refused');
  assert.equal(calls.length, 1, 'only the pre-signal spawn reached the spawn function');
  assert.equal(calls[0].file, '/bin/true');
});

test('the gate is monotonic: closing keeps the first reason and refuses every later spawn', () => {
  const { calls, spawnFn } = fakeSpawner();
  const gate = createSpawnGate({ spawnFn });
  assert.equal(gate.closed, false);
  gate.close('interrupted');
  gate.close('sample-error');
  assert.equal(gate.closed, true);
  for (const label of ['keeper', 'daemon']) {
    const result = gate.admit(label, '/bin/false', [], {});
    assert.equal(result.ok, false);
    assert.equal(result.reason, 'interrupted', 'the first close wins');
  }
  assert.deepEqual(calls, []);
});

test('admission is synchronous, so no await can slip between the check and the spawn', () => {
  const { spawnFn } = fakeSpawner();
  const gate = createSpawnGate({ spawnFn });
  const result = gate.admit('keeper', '/bin/true', [], {});
  assert.equal(result.ok, true);
  assert.equal(typeof result.then, 'undefined', 'admit must not return a promise');
  assert.ok(!(result instanceof Promise));
});

test('the gate records what it started, so teardown can see a spawn that raced it', () => {
  const { spawnFn } = fakeSpawner();
  const gate = createSpawnGate({ spawnFn, clock: () => 1234 });
  gate.admit('keeper', '/bin/true', [], {});
  gate.admit('daemon', '/bin/true', [], {});
  const spawned = gate.spawned;
  assert.deepEqual(spawned.map((entry) => entry.label), ['keeper', 'daemon']);
  assert.deepEqual(spawned.map((entry) => entry.at), [1234, 1234]);
  // The snapshot is a copy: mutating it cannot rewrite the gate's own record.
  spawned.pop();
  assert.equal(gate.spawned.length, 2);
});

test('a gate with no spawn function refuses instead of throwing inside teardown', () => {
  const gate = createSpawnGate({});
  const result = gate.admit('keeper', '/bin/true', [], {});
  assert.equal(result.ok, false);
  assert.equal(result.reason, 'no-spawn-function');
});

// ---------------------------------------------------------------------------------------------
// R1-a: a handler must already exist when the FIRST child is spawned
// ---------------------------------------------------------------------------------------------

/**
 * A signal target that records listener registration interleaved with a spawn log, so the assertion
 * is about observed ordering rather than about where a line sits in the source.
 */
function orderingTarget(log) {
  const listeners = new Map();
  return {
    on(signal, listener) {
      log.push(`on:${signal}`);
      listeners.set(signal, listener);
    },
    removeListener(signal) {
      listeners.delete(signal);
    },
    emit(signal) {
      const listener = listeners.get(signal);
      if (listener) listener(signal);
      return Boolean(listener);
    },
    hasListener(signal) {
      return listeners.has(signal);
    },
  };
}

test('signal handlers are registered before the keeper is spawned, so an early SIGTERM still reports', async () => {
  // The regression R1-a names: registration used to sit after the keeper spawn, so a SIGTERM in that
  // window killed the harness with no summary and an orphaned keeper. This drives the real `main`
  // through its real startup path and delivers the signal during the very first spawn attempt, then
  // asserts a summary was still written and the keeper was never started.
  const log = [];
  const target = orderingTarget(log);
  // Cleanup is scoped to this exact root: the path is created here, resolved once with `realpathSync`,
  // and the `finally` below removes only that resolved path, and only while it still resolves to
  // itself. No glob, no prefix sweep, no shared parent. A throwing assertion or a rejected `main`
  // therefore cannot leak the root, and a symlink swapped in underneath cannot redirect the removal.
  const outDir = realpathSync(mkdtempSync(path.join(os.tmpdir(), 'soak-order-')));
  // A fixture that exists on every host. `/bin/true` is Linux-only (macOS keeps it at
  // `/usr/bin/true`), so hardcoding it made this test fail in `parseArgs` before it ever reached the
  // ordering assertion. `process.execPath` is a real executable on both platforms, and the fixture is
  // placed under a `Contents/MacOS` parent so `stageSoakDaemon` short-circuits on darwin instead of
  // trying to build a real app bundle, which needs a keeper protocol this test does not supply.
  const fixtureDir = path.join(outDir, 'fixture', 'Ferryx.app', 'Contents', 'MacOS');
  mkdirSync(fixtureDir, { recursive: true });
  const fixtureBin = path.join(fixtureDir, 'ferryx');
  symlinkSync(process.execPath, fixtureBin);
  const exits = [];
  let summaryWritten = null;
  let failed = false;

  const deps = {
    signalTarget: target,
    exit: (code) => exits.push(code),
    spawnFn: (file, args, options) => {
      log.push(`spawn:${path.basename(file)}`);
      // Deliver the signal at the instant of the first spawn: the worst case for R1-a, because it is
      // exactly the boundary where the old code had no handler installed yet.
      assert.equal(target.hasListener('SIGTERM'), true, 'a SIGTERM handler must exist before any spawn');
      target.emit('SIGTERM');
      // Deliberately no pid: teardown is already decided by the signal above, so identity must not be
      // read. That keeps this test free of a real `ps` call (and of any dependence on pid 4242 being
      // unused on the host), which is what makes the ordering assertion deterministic.
      return { stderr: { on() {} }, once() {}, on() {} };
    },
  };

  try {
    const code = await main(
      ['--daemon-bin', fixtureBin, '--keeper-bin', fixtureBin, '--out', outDir, '--keep-root'],
      process.env,
      deps,
    );

    const spawns = log.filter((entry) => entry.startsWith('spawn:'));
    const registrations = log.filter((entry) => entry.startsWith('on:'));
    const firstSpawnIndex = log.findIndex((entry) => entry.startsWith('spawn:'));
    const lastRegistrationIndex = log.map((e, i) => (e.startsWith('on:') ? i : -1)).reduce((a, b) => Math.max(a, b), -1);

    assert.ok(registrations.length > 0, 'handlers must be registered at all');
    assert.ok(lastRegistrationIndex >= 0 && firstSpawnIndex >= 0);
    assert.ok(
      lastRegistrationIndex < firstSpawnIndex,
      `every handler registration must precede the first spawn (last on:${lastRegistrationIndex} < spawn:${firstSpawnIndex})`,
    );
    assert.deepEqual(
      spawns,
      [`spawn:${path.basename(fixtureBin)}`],
      'the signal at spawn time must stop the run before a second spawn',
    );
    assert.equal(code, 4, 'an interrupted run is unverified');

    const summaryPath = path.join(outDir, 'footprint-summary.json');
    assert.equal(existsSync(summaryPath), true, 'R1-a: an interrupted run must still write a summary');
    summaryWritten = JSON.parse(readFileSync(summaryPath, 'utf8'));
    assert.equal(summaryWritten.verdict, 'unverified');
    assert.equal(summaryWritten.interruption.signal, 'SIGTERM');
  } catch (error) {
    failed = true;
    throw error;
  } finally {
    // A failed run keeps its root and names it: the summary and logs inside are the only explanation
    // of the failure, and deleting the evidence is the one outcome this cleanup must never produce.
    if (failed) {
      console.error(`R1-a test root retained for inspection: ${outDir}`);
    } else {
      // Re-verify the path is still the one this test created before removing it, and stay silent if
      // it is already gone: cleanup must never throw, or it would mask the result of the test itself.
      let stillOurs = false;
      try {
        stillOurs = realpathSync(outDir) === outDir;
      } catch {
        stillOurs = false;
      }
      if (!stillOurs) {
        console.error(`R1-a test root not removed: ${outDir} no longer resolves to itself`);
      } else {
        rmSync(outDir, { recursive: true, force: true });
      }
    }
  }
});

// ---------------------------------------------------------------------------------------------
// The census must never read a failed `ps` as an absent process: that inference deleted the root of
// a run whose panes were still alive. Every case below is driven by a stub `run`, so it touches no
// real pid, spawns nothing, and waits on nothing.
// ---------------------------------------------------------------------------------------------

/** A `ps` stand-in with a fixed result. No process is consulted. */
function stubRun(result) {
  const calls = [];
  const run = (file, args) => {
    calls.push({ file, args });
    if (result instanceof Error) throw result;
    return result;
  };
  run.calls = calls;
  return run;
}

test('probeProcess: a missing pid is the only shape that reads as gone', () => {
  // `ps -p <gone>` exits 1 with no stdout and no stderr. That silence is the entire evidence of
  // absence, so it must be exactly this shape and nothing looser.
  assert.deepEqual(probeProcess(4242, stubRun({ status: 1, stdout: '', stderr: '' })), { state: 'gone' });
});

test('probeProcess: an unreadable table is unknown, never gone', () => {
  const unreadable = [
    ['stderr diagnostic', { status: 1, stdout: '', stderr: 'ps: cannot read process table' }],
    ['unexpected status', { status: 3, stdout: '', stderr: '' }],
    ['non-string stdout', { status: 0, stdout: null, stderr: '' }],
    ['unparseable row', { status: 0, stdout: 'garbage output row', stderr: '' }],
  ];
  for (const [label, result] of unreadable) {
    const probe = probeProcess(4242, stubRun(result));
    assert.equal(probe.state, 'unknown', `${label} must not be evidence of absence`);
  }
  // A thrown ps (ENOENT, EAGAIN) is a failed read, not a dead process.
  assert.equal(probeProcess(4242, stubRun(new Error('spawn ps ENOENT'))).state, 'unknown');
});

test('probeProcess: a live pid is alive and carries its identity', () => {
  // Uses the file's existing PS_ROW (pid 4321) rather than a second fixture of its own.
  const probe = probeProcess(4321, stubRun({ status: 0, stdout: PS_ROW, stderr: '' }));
  assert.equal(probe.state, 'alive');
  assert.equal(probe.identity.pid, 4321);
  assert.equal(probe.identity.executable, '/opt/ferryx/Ferryx');
});

test('probeProcess: an invalid pid is unknown, and no ps is run for it', () => {
  for (const pid of [0, -1, 1.5, null, undefined, '4242']) {
    const run = stubRun({ status: 1, stdout: '', stderr: '' });
    const probe = probeProcess(pid, run);
    assert.equal(probe.state, 'unknown', `pid ${String(pid)} is not a positive integer`);
    assert.equal(run.calls.length, 0, 'an invalid pid must not reach ps at all');
  }
});

test('processIdentity keeps its two-state contract for teardown callers', () => {
  assert.equal(processIdentity(4242, stubRun({ status: 1, stdout: '', stderr: '' })), null);
  assert.equal(processIdentity(4242, stubRun({ status: 1, stdout: '', stderr: 'boom' })), null);
  assert.equal(processIdentity(4321, stubRun({ status: 0, stdout: PS_ROW, stderr: '' })).pid, 4321);
  assert.equal(processIdentity(0, stubRun({ status: 0, stdout: PS_ROW, stderr: '' })), null);
});

/** A probe returning a fixed state, so the census can be driven without any `ps` at all. */
function stubProbe(stateByPid) {
  const calls = [];
  const probe = (pid) => {
    calls.push(pid);
    const entry = stateByPid[pid];
    if (entry instanceof Error) throw entry;
    return entry ?? { state: 'gone' };
  };
  probe.calls = calls;
  return probe;
}

test('census: a pane proven gone with no other evidence confirms the panes only', () => {
  const census = censusPanesAfterShutdown(
    [{ sessionId: 'a', panePid: 11 }, { sessionId: 'b', panePid: 12 }],
    stubProbe({ 11: { state: 'gone' }, 12: { state: 'gone' } }),
  );
  assert.equal(census.gone.length, 2);
  assert.equal(census.alive.length, 0);
  assert.equal(census.unknown.length, 0);
  assert.equal(census.confirmedGone, true);
});

test('census: an unreadable pane keeps the root, even when every other pane is gone', () => {
  // The exact defect: `null` used to mean both "gone" and "could not read". Unreadable must block.
  const census = censusPanesAfterShutdown(
    [{ sessionId: 'a', panePid: 11 }, { sessionId: 'b', panePid: 12 }],
    stubProbe({ 11: { state: 'gone' }, 12: { state: 'unknown', reason: 'ps-stderr: cannot read' } }),
  );
  assert.equal(census.unknown.length, 1);
  assert.equal(census.unknown[0].reason.includes('ps-stderr'), true);
  assert.equal(census.confirmedGone, false, 'an unreadable pane is not proof of absence');
});

test('census: an alive pane keeps the root', () => {
  const census = censusPanesAfterShutdown([{ sessionId: 'a', panePid: 11 }], stubProbe({ 11: { state: 'alive' } }));
  assert.equal(census.alive.length, 1);
  assert.equal(census.confirmedGone, false);
});

test('census: an empty census is never confirmation', () => {
  // A run that never learned its pane pids has proven nothing at all.
  const census = censusPanesAfterShutdown([], stubProbe({}));
  assert.equal(census.gone.length, 0);
  assert.equal(census.confirmedGone, false, 'nothing proven is not proof');
});

test('census: a rejected or malformed probe result is unknown, never gone', () => {
  const thrown = censusPanesAfterShutdown([{ sessionId: 'a', panePid: 11 }], () => {
    throw new Error('probe exploded');
  });
  assert.equal(thrown.unknown.length, 1);
  assert.equal(thrown.confirmedGone, false);

  const malformed = censusPanesAfterShutdown([{ sessionId: 'a', panePid: 11 }], () => ({ state: 'exploded' }));
  assert.equal(malformed.unknown.length, 1, 'an unrecognised state is not permission to delete');
  assert.equal(malformed.confirmedGone, false);
});

test('census: a surviving descendant blocks the pass, and its absence proves nothing', () => {
  const blocking = censusPanesAfterShutdown(
    [{ sessionId: 'a', panePid: 11 }],
    stubProbe({ 11: { state: 'gone' }, 99: { state: 'alive' } }),
    [{ pid: 99, line: 'descendant' }],
  );
  assert.equal(blocking.alive.length, 1);
  assert.equal(blocking.confirmedGone, false, 'a live descendant keeps the root in use');

  // Even with every pane and every known descendant gone, the subtree is still not proven: a child
  // reparented after its pane died, or forked after the last sample, was never observed.
  const clean = censusPanesAfterShutdown(
    [{ sessionId: 'a', panePid: 11 }],
    stubProbe({ 11: { state: 'gone' }, 99: { state: 'gone' } }),
    [{ pid: 99, line: 'descendant' }],
  );
  assert.equal(clean.confirmedGone, true, 'every known pid is gone');
  assert.equal(clean.subtreeAbsence, 'unproven', 'but absence of the subtree is never claimed');
  assert.equal(clean.descendantsChecked, 1);
});

test('captureDescendants: reads tree by parentage, needs no waiting, and never throws', () => {
  const table = '   10     1\n   11    10\n   12    11\n   77     1\n';
  const found = captureDescendants([10], stubRun({ status: 0, stdout: table, stderr: '' }));
  assert.deepEqual(found.map((entry) => entry.pid).sort((a, b) => a - b), [11, 12], 'transitive, excluding the root');

  assert.deepEqual(captureDescendants([], stubRun({ status: 0, stdout: table, stderr: '' })), [], 'no roots, no ps');
  assert.deepEqual(captureDescendants([10], stubRun(new Error('no ps'))), [], 'a failed read is empty, not a throw');
  assert.deepEqual(captureDescendants([10], stubRun({ status: 1, stdout: table, stderr: 'x' })), [], 'a failed status is empty');
  // A cycle in the table must terminate rather than spin.
  const cycle = '   10    12\n   12    10\n';
  assert.deepEqual(captureDescendants([10], stubRun({ status: 0, stdout: cycle, stderr: '' })).map((e) => e.pid), [12]);
});
