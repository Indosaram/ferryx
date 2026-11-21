/**
 * Focused tests for the p99 echo harness's pure pieces.
 *
 * Everything asserted here is a decision the harness makes before it measures anything: how a
 * percentile is ranked, how a payload is shaped, whether an arm order is balanced, whether keeper
 * ownership is *proven*, and whether a comparison is allowed to exist at all. Those are the parts
 * where a silent mistake would produce a confident number that means nothing, so they are pinned
 * here rather than left to the operator to eyeball in a report.
 *
 * Nothing in this file starts a process, opens a socket, writes a file, or sleeps. The harness is
 * imported for its helpers only: `pty-echo-p99-comparison.mjs` runs `main()` behind an argv guard, so
 * importing it has no side effects.
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  GAP,
  HarnessError,
  assertIsolation,
  blockPlan,
  comparisonOf,
  makePayload,
  parseArgs,
  percentile,
  planPaths,
  readActivation,
  renderText,
  summarize,
  verifyKeeperOwnershipEvidence,
} from './pty-echo-p99-comparison.mjs';

// -- percentiles -------------------------------------------------------------------------------

test('percentile uses nearest rank with no interpolation', () => {
  const sorted = [1, 2, 3, 4];
  // rank = ceil(p/100 * n), clamped to [1, n]; the reported value is always a real sample.
  assert.equal(percentile(sorted, 50), 2);
  assert.equal(percentile(sorted, 90), 4);
  assert.equal(percentile(sorted, 99), 4);
  assert.equal(percentile(sorted, 100), 4);
  // p <= 0 answers the smallest sample instead of throwing or returning undefined.
  assert.equal(percentile(sorted, 0), 1);
});

test('percentile on a single sample answers that sample for every p', () => {
  for (const p of [1, 50, 99, 100]) {
    assert.equal(percentile([7], p), 7);
  }
});

test('percentile of an empty array is null, not 0 and not NaN', () => {
  assert.equal(percentile([], 99), null);
});

test('summarize reports nulls for no samples and keeps the raw values it was given', () => {
  const empty = summarize([]);
  assert.equal(empty.count, 0);
  assert.equal(empty.p50Ms, null);
  assert.equal(empty.p99Ms, null);
  assert.equal(empty.minMs, null);
  assert.equal(empty.maxMs, null);
  assert.equal(empty.meanMs, null);
  assert.deepEqual(empty.samplesMs, []);

  const filled = summarize([4, 1, 3]);
  assert.equal(filled.count, 3);
  assert.equal(filled.minMs, 1);
  assert.equal(filled.maxMs, 4);
  // The raw samples travel with the summary, so a reader can recompute any percentile.
  assert.deepEqual(filled.samplesMs, [4, 1, 3]);
});

// -- payloads ----------------------------------------------------------------------------------

test('makePayload produces exactly the requested byte count and a trailing newline', () => {
  const { bytes } = makePayload(0, 64);
  assert.equal(bytes.length, 64);
  // Canonical mode delivers on newline, so a payload without one would never be echoed.
  assert.equal(bytes[bytes.length - 1], 0x0a);
});

test('makePayload embeds a nonce that differs per sample', () => {
  const first = makePayload(0, 64);
  const second = makePayload(1, 64);
  assert.notEqual(first.nonce, second.nonce);
  // The nonce must be findable in the bytes, or an echo could not be tied to its sample.
  assert.ok(first.bytes.toString('ascii').startsWith(first.nonce));
  // The index is encoded in the nonce, so a replayed echo from another sample is distinguishable.
  assert.ok(first.nonce.startsWith('fxe000000'));
  assert.ok(second.nonce.startsWith('fxe000001'));
});

test('makePayload refuses a size that cannot carry a unique nonce', () => {
  // Refusing is the point: a truncated nonce would make the echo match ambiguous, so the harness
  // must not quietly produce a payload it cannot verify.
  assert.throws(
    () => makePayload(0, 4),
    (error) => error instanceof HarnessError && error.code === 'USAGE',
  );
});

// -- interleaving ------------------------------------------------------------------------------

test('blockPlan alternates the arm order so drift lands on both arms', () => {
  assert.deepEqual(blockPlan(4, ['a', 'b']), [
    ['a', 'b'],
    ['b', 'a'],
    ['a', 'b'],
    ['b', 'a'],
  ]);
});

test('blockPlan never invents an arm that was not selected', () => {
  // A single-arm run is legal, and reversing it must stay a permutation of what was asked for.
  assert.deepEqual(blockPlan(3, ['only']), [['only'], ['only'], ['only']]);
  const three = blockPlan(2, ['a', 'b', 'c']);
  assert.deepEqual(three[0], ['a', 'b', 'c']);
  assert.deepEqual(three[1], ['c', 'b', 'a']);
  for (const block of three) {
    assert.deepEqual([...block].sort(), ['a', 'b', 'c']);
  }
});

// -- keeper ownership proof --------------------------------------------------------------------

/** The minimal evidence set that `verifyKeeperOwnershipEvidence` must accept. */
function provenEvidence() {
  return {
    sessionId: 'session-1',
    keeperSessions: [{ id: 'session-1', owner: 'keeper', pid: 4242, epoch: 7, sequenceOffset: 9 }],
    ownershipTable: {
      keeperOwnershipEnabled: true,
      records: { 'session-1': { owner: 'keeper', pid: 4242 } },
    },
    liveSession: { localChildPid: 4242 },
  };
}

test('keeper ownership is accepted only when all three views agree', () => {
  const proven = verifyKeeperOwnershipEvidence(provenEvidence());
  assert.equal(proven.sessionId, 'session-1');
  assert.equal(proven.keeperPid, 4242);
  assert.equal(proven.tablePid, 4242);
  assert.equal(proven.livePid, 4242);
  // The epoch and offset come from the keeper's own registry, which is what later samples fence on.
  assert.equal(proven.keeperEpoch, 7);
  assert.equal(proven.keeperSequenceOffset, 9);
  assert.equal(proven.provedBy.length, 4);
});

/** Asserts the evidence is rejected with the typed ownership gap. */
function assertUnproven(evidence, mention) {
  assert.throws(
    () => verifyKeeperOwnershipEvidence(evidence),
    (error) => {
      assert.ok(error instanceof HarnessError, `expected a HarnessError, got ${error}`);
      assert.equal(error.code, GAP.KEEPER_OWNERSHIP_UNPROVEN);
      if (mention) {
        assert.ok(
          error.message.includes(mention),
          `expected the message to mention ${JSON.stringify(mention)}, got ${error.message}`,
        );
      }
      return true;
    },
  );
}

test('a session the keeper does not hold is refused', () => {
  // This is the silent-degradation case: the daemon came up, spawned, and owned the PTY itself.
  assertUnproven({ ...provenEvidence(), keeperSessions: [] }, 'does not hold session');
});

test('a session the keeper holds under the daemon owner is refused', () => {
  assertUnproven(
    {
      ...provenEvidence(),
      keeperSessions: [{ id: 'session-1', owner: 'daemon', pid: 4242 }],
    },
    'not "keeper"',
  );
});

test('a missing or disabled ownership table is refused', () => {
  assertUnproven({ ...provenEvidence(), ownershipTable: null }, 'never read');
  assertUnproven(
    {
      ...provenEvidence(),
      ownershipTable: { keeperOwnershipEnabled: false, records: provenEvidence().ownershipTable.records },
    },
    'keeper ownership enabled',
  );
});

test('a durable record with the wrong owner is refused', () => {
  assertUnproven(
    {
      ...provenEvidence(),
      ownershipTable: {
        keeperOwnershipEnabled: true,
        records: { 'session-1': { owner: 'daemon', pid: 4242 } },
      },
    },
    'records owner',
  );
});

test('a pid disagreement anywhere in the three views is refused', () => {
  // A disagreement means one view describes a different process, so no sample from it is trustworthy.
  assertUnproven(
    {
      ...provenEvidence(),
      ownershipTable: {
        keeperOwnershipEnabled: true,
        records: { 'session-1': { owner: 'keeper', pid: 9999 } },
      },
    },
    'durable table records 9999',
  );
  assertUnproven({ ...provenEvidence(), liveSession: { localChildPid: 8888 } }, 'describeLiveSessions');
});

test('evidence with no pid at all is refused rather than accepted vacuously', () => {
  // Absence of evidence must not read as agreement: two nulls are not a matching pid.
  assertUnproven(
    {
      sessionId: 'session-1',
      keeperSessions: [{ id: 'session-1', owner: 'keeper' }],
      ownershipTable: { keeperOwnershipEnabled: true, records: { 'session-1': { owner: 'keeper' } } },
      liveSession: null,
    },
    'reported a child pid',
  );
});

// -- activation log ----------------------------------------------------------------------------

test('readActivation reads the outcome the daemon logged', () => {
  assert.equal(
    readActivation({ stdout: '', stderr: 'INFO keeper ownership activation outcome=NotProven\n' }).outcome,
    'NotProven',
  );
  assert.equal(
    readActivation({
      stdout: '',
      stderr: 'INFO keeper ownership activation outcome=Activated { keeper_pid: Some(123) }\n',
    }).outcome,
    'Activated',
  );
  assert.equal(
    readActivation({ stdout: 'INFO keeper ownership activation outcome=AlreadyActive\n', stderr: '' })
      .outcome,
    'AlreadyActive',
  );
});

test('readActivation reports silence as no outcome, which the caller treats as unproven', () => {
  // Silence is not success: a daemon that never logged activation never proved a keeper.
  const silent = readActivation({ stdout: 'FERRYX_DAEMON_READY\n', stderr: '' });
  assert.equal(silent.outcome, null);
  assert.equal(silent.line, null);
});

test('readActivation flags the failure warning without inventing an outcome', () => {
  const warned = readActivation({
    stdout: '',
    stderr: 'WARN keeper ownership could not be activated: payload absent\n',
  });
  assert.equal(warned.outcome, null);
  assert.equal(warned.sawFailureWarning, true);
});

// -- comparison is fail-closed -----------------------------------------------------------------

/** One arm summary as `buildReport` shapes it. */
function armReport(p99Ms, socketPath) {
  return { count: 10, p50Ms: p99Ms / 2, p90Ms: p99Ms, p99Ms, maxMs: p99Ms, ownership: { daemonSocketPath: socketPath } };
}

test('a comparison is obtained only for the two daemon arms on the same endpoint', () => {
  const comparison = comparisonOf({
    arms: {
      'direct-daemon': armReport(2, '/rt/daemon.sock'),
      'keeper-daemon': armReport(3, '/rt/daemon.sock'),
    },
  });
  assert.equal(comparison.obtained, true);
  assert.equal(comparison.comparable, true);
  assert.deepEqual(comparison.comparedArms, ['direct-daemon', 'keeper-daemon']);
  // The delta is keeper minus direct, so a positive number means the keeper hop cost more.
  assert.equal(comparison.p99DeltaMs, 1);
  // armReport sets p50 to p99/2, so 3/2 - 2/2 = 0.5. Asserting the derived value keeps this honest
  // about the arithmetic rather than about one hand-picked number.
  assert.equal(comparison.p50DeltaMs, 0.5);
  // No threshold is invented: the plan sets none, so the verdict stays report-only.
  assert.equal(comparison.passFailThreshold, null);
});

test('the keeper-uds arm is never substituted for the comparable arm', () => {
  // This is the central refusal: keeper-uds has no daemon hop, so its boundary is shorter and
  // subtracting it would compute a number describing two different quantities.
  const comparison = comparisonOf({
    arms: {
      'direct-daemon': armReport(2, '/rt/daemon.sock'),
      'keeper-uds': armReport(1, null),
    },
  });
  assert.equal(comparison.obtained, false);
  assert.equal(comparison.comparable, false);
  assert.equal(comparison.comparedArms, null);
  assert.equal(comparison.p99DeltaMs, null);
  // The diagnostic arm is named, so the report says what it had instead of staying silent.
  assert.equal(comparison.diagnosticArm, 'keeper-uds');
  assert.match(comparison.verdict, /NO COMPARISON OBTAINED/);
});

test('two arms that entered different daemon sockets are refused', () => {
  const comparison = comparisonOf({
    arms: {
      'direct-daemon': armReport(2, '/a/daemon.sock'),
      'keeper-daemon': armReport(3, '/b/daemon.sock'),
    },
  });
  assert.equal(comparison.obtained, false);
  assert.equal(comparison.comparable, false);
  assert.equal(comparison.boundaryKind, 'mismatched-daemon-endpoints');
  assert.equal(comparison.p99DeltaMs, null);
});

test('a missing arm yields no comparison rather than a partial one', () => {
  assert.equal(comparisonOf({ arms: {} }).obtained, false);
  assert.equal(comparisonOf({ arms: { 'direct-daemon': armReport(2, '/rt/daemon.sock') } }).obtained, false);
  assert.equal(
    comparisonOf({ arms: { 'keeper-daemon': armReport(2, '/rt/daemon.sock') } }).obtained,
    false,
  );
});

// -- isolation ---------------------------------------------------------------------------------

test('planned paths all live inside the fixture root and pass the isolation check', () => {
  const root = '/tmp/ferryx-p99-fixture-unit-test';
  const paths = planPaths(root);
  // Pure path arithmetic: this asserts the layout, and touches no directory.
  assert.ok(paths.daemonSocketPath.startsWith(root));
  assert.ok(paths.bundledDaemonPath.startsWith(root));
  assert.ok(paths.bundledKeeperPath.startsWith(root));
  assert.equal(assertIsolation(paths), undefined);
});

test('a path that escapes the fixture root is refused', () => {
  const paths = planPaths('/tmp/ferryx-p99-fixture-unit-test');
  assert.throws(
    () => assertIsolation({ ...paths, dataDir: '/somewhere/else/data' }),
    (error) => error instanceof HarnessError && /escapes the fixture root/.test(error.message),
  );
});

test('a fixture root inside a production runtime location is refused', () => {
  // String arithmetic only. The point is that a root placed under the real runtime directory cannot
  // pass as isolated, because every path it derives would sit in production.
  const uid = typeof process.getuid === 'function' ? process.getuid() : 0;
  const paths = planPaths(`/tmp/ferryx-${uid}/nested-fixture`);
  assert.throws(
    () => assertIsolation(paths),
    (error) => error instanceof HarnessError && /overlaps the production runtime location/.test(error.message),
  );
});

// -- argument validation -----------------------------------------------------------------------

test('the comparable daemon pair is the default selection', () => {
  // Both binaries are supplied because the default selection includes two daemon arms, and each
  // requires its binary. Supplying them keeps this test about the default arm *set*, not about
  // validation.
  const options = parseArgs(['--daemon-binary', '/bin/true', '--keeper-binary', '/bin/true']);
  assert.deepEqual(options.arms, ['direct-daemon', 'keeper-daemon']);
  // The default selection must be exactly the comparable pair, never the incomparable diagnostic.
  assert.ok(!options.arms.includes('keeper-uds'));
});

test('the keeper-daemon arm requires both binaries', () => {
  assert.throws(
    () => parseArgs(['--arms', 'keeper-daemon', '--daemon-binary', '/bin/true']),
    (error) => error instanceof HarnessError && /--keeper-binary is required/.test(error.message),
  );
  assert.throws(
    () => parseArgs(['--arms', 'keeper-daemon', '--keeper-binary', '/bin/true']),
    (error) => error instanceof HarnessError && /--daemon-binary is required/.test(error.message),
  );
});

test('an unknown arm name is refused with the known names listed', () => {
  assert.throws(
    () => parseArgs(['--arms', 'keeper-through-daemon']),
    (error) =>
      error instanceof HarnessError &&
      /unknown arm/.test(error.message) &&
      error.message.includes('keeper-daemon'),
  );
});

test('an empty arm selection is refused', () => {
  assert.throws(
    () => parseArgs(['--arms', ' , ']),
    (error) => error instanceof HarnessError && /selected no arm/.test(error.message),
  );
});

// -- rendering ---------------------------------------------------------------------------------

/** The smallest report `renderText` will accept, so rendering can be tested without a run. */
function minimalReport(overrides = {}) {
  return {
    schema: 'ferryx-pty-echo-p99/v1',
    plan: { file: 'plan.md', revision: 'rev 5' },
    host: { platform: 'darwin', arch: 'arm64', release: 'r', nodeVersion: 'v' },
    isolation: { root: '/tmp/x' },
    baseline: { kind: 'measured-now-direct-owner', historical: false, sameBinaryAsKeeperArm: false },
    parameters: { samples: 1, warmup: 0, payloadBytes: 64, repeats: 1 },
    arms: {},
    boundary: { arms: {} },
    comparison: null,
    workload: { residualAsymmetry: null, residualAsymmetryNote: null },
    gaps: [],
    limitations: [],
    ...overrides,
  };
}

test('renderText prints the residual asymmetry field, including when it is null', () => {
  // Rendering must distinguish "checked and absent" from "never checked", so the null case still
  // produces a line rather than silently omitting it.
  const plain = renderText(minimalReport());
  assert.match(plain, /residual asymmetry: none/);
});

test('renderText surfaces a residual asymmetry and its note when one is recorded', () => {
  // A unique token per field, so this asserts the field is actually consumed rather than that the
  // renderer emits some particular sentence.
  const text = renderText(
    minimalReport({
      workload: {
        residualAsymmetry: 'ASYMMETRY_TOKEN_7f3a',
        residualAsymmetryNote: 'NOTE_TOKEN_9c1d',
      },
    }),
  );
  assert.ok(text.includes('ASYMMETRY_TOKEN_7f3a'));
  assert.ok(text.includes('NOTE_TOKEN_9c1d'));
});

test('renderText reports a refused comparison without printing a delta', () => {
  const text = renderText(
    minimalReport({
      comparison: comparisonOf({
        arms: {
          'direct-daemon': armReport(2, '/rt/daemon.sock'),
          'keeper-uds': armReport(1, null),
        },
      }),
    }),
  );
  assert.match(text, /comparable: false/);
  assert.match(text, /p50 delta: null ms {3}p99 delta: null ms/);
  assert.match(text, /NO COMPARISON OBTAINED/);
});
