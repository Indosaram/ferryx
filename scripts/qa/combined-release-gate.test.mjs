// Deterministic proofs for the fail-closed rules in scripts/qa/combined-release-gate.mjs.
//
// Nothing here spawns a process, reads a clock or touches a socket: the runtime half needs shipped
// binaries, a release run directory and live harnesses, so what is pinned is exactly the arithmetic
// that decides whether a run can be believed.
//
// The rule under test, stated once: exit 0 is NOT acceptance. A harness that exits 0 while reporting
// a blocked case, a missing summary or a self-check-only selection must be rejected, and the reason
// must be a typed code rather than prose.
//
// The Phase 1 Test B soak is ADVISORY since rev 5.1 (2026-11-20): its predicates are still pinned
// below (`evaluateSoakEvidence`, because a judge that reports nothing is worthless), but no soak
// result - passing, failing or absent - may move the verdict. The aggregation section pins that.
//
// `node --test` only. This file has never been executed in the authoring session (the task forbids
// running tests on that Mac); it is recorded as an unverified gap in the handoff.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  ADVISORY_STEP_IDS,
  GATE_SCHEMA,
  PHASE1_BUDGET_TESTS,
  PHASE2_SUPERVISOR_TESTS,
  PHASE3_REQUIRED_CASES,
  PHASE4_REQUIRED_CASES,
  PLAN_SOAK,
  candidateDigestOf,
  canonicalJson,
  cargoTestNamed,
  evaluateBundleSeal,
  evaluateCargoStep,
  evaluateGate,
  evaluateHarnessStep,
  evaluateCandidateSnapshot,
  evaluateHostConsistency,
  evaluateKeeperPayload,
  evaluateProcedureDocs,
  evaluateP99Evidence,
  evaluatePhase0Evidence,
  evaluateRollbackFloor,
  evaluateSoakAdvisory,
  evaluateSoakEvidence,
  evaluateSoakSamples,
  lastJsonDocument,
  parseCargoTestLog,
  renderGateReport,
  requiredStepIds,
  verifyRecordedStep,
} from './combined-release-gate.mjs';
import { SYMBOLICATION_SCHEMA } from '../lib/release-symbolication.mjs';
import { sha256Hex } from '../lib/dsym-symbolication.mjs';

const codesOf = (evaluation) => evaluation.problems.map((problem) => problem.code);

/** A harness summary whose cases all passed, shaped exactly as phase3/phase4 print it. */
function passingHarness({ requiredCases, extra = {} }) {
  const outcomes = requiredCases.map((id) => ({ id, outcome: 'pass', result: { note: 'ok' } }));
  return {
    summary: {
      total: outcomes.length,
      passed: outcomes.length,
      failed: 0,
      blocked: 0,
      accepted: true,
      ...extra,
    },
    outcomes,
  };
}

// ---------------------------------------------------------------------------------------------
// exit 0 is not acceptance
// ---------------------------------------------------------------------------------------------

test('a harness that exits 0 while reporting a blocked case is rejected', () => {
  const requiredCases = ['a-successor-dies-before-decision', 'b-successor-dies-after-decision'];
  const summary = passingHarness({ requiredCases });
  // The blocked case is real and the counts says so, while `accepted` was left at true: this is the
  // exact contradiction a raw exit-code check calls a pass.
  summary.outcomes[1] = { id: requiredCases[1], outcome: 'blocked', gap: 'KEEPER_BINARY_ABSENT' };
  summary.summary.passed = 1;
  summary.summary.blocked = 1;

  const evaluated = evaluateHarnessStep({ id: 'phase4-handover-failure', exitCode: 0, summary, requiredCases });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_CASE_BLOCKED'));
  assert.ok(codesOf(evaluated).includes('GATE_NOT_ACCEPTED'));
});

test('a blocked count with accepted:false and exit 0 is rejected on the count alone', () => {
  const requiredCases = ['mixed-ownership-drain'];
  const summary = {
    summary: { total: 1, passed: 0, failed: 0, blocked: 1, accepted: false },
    outcomes: [{ id: 'mixed-ownership-drain', outcome: 'blocked', gap: 'NO_BINARIES' }],
  };
  const evaluated = evaluateHarnessStep({ id: 'phase3-crash-survival', exitCode: 0, summary, requiredCases });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_CASE_BLOCKED'));
});

test('a summary naming fewer cases than the gate selected is rejected', () => {
  const requiredCases = ['identity-60-session-sigkill', 'stale-epoch-fenced'];
  const summary = passingHarness({ requiredCases: [requiredCases[0]] });
  const evaluated = evaluateHarnessStep({ id: 'phase3-crash-survival', exitCode: 0, summary, requiredCases });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_CASE_ABSENT'));
  assert.ok(codesOf(evaluated).includes('GATE_CASE_COUNT_MISMATCH'));
});

test('a missing or unparsable summary fails even at exit 0', () => {
  for (const summary of [null, undefined, 'not-an-object', 42]) {
    const evaluated = evaluateHarnessStep({
      id: 'phase3-crash-survival',
      exitCode: 0,
      summary,
      requiredCases: PHASE3_REQUIRED_CASES,
    });
    assert.equal(evaluated.status, 'fail');
    assert.deepEqual(codesOf(evaluated), ['GATE_HARNESS_SUMMARY_MISSING']);
  }
});

test('a self-check-only selection is never acceptance', () => {
  const summary = passingHarness({ requiredCases: ['isolation-guard'] });
  const evaluated = evaluateHarnessStep({
    id: 'phase4-handover-failure',
    exitCode: 0,
    summary,
    requiredCases: ['isolation-guard'],
  });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_SELF_CHECK_ONLY'));
});

test('a full passing harness is accepted and reports its counts', () => {
  const summary = passingHarness({ requiredCases: PHASE4_REQUIRED_CASES });
  const evaluated = evaluateHarnessStep({
    id: 'phase4-handover-failure',
    exitCode: 0,
    summary,
    requiredCases: PHASE4_REQUIRED_CASES,
  });
  assert.equal(evaluated.status, 'pass');
  assert.deepEqual(evaluated.problems, []);
  assert.equal(evaluated.counts.total, PHASE4_REQUIRED_CASES.length);
});

// ---------------------------------------------------------------------------------------------
// cargo steps: a named test must have run
// ---------------------------------------------------------------------------------------------

test('cargo log parsing records real test rows and suite totals', () => {
  const log = [
    'running 2 tests',
    'test terminal::scrollback_budget_tests::inactive_evicted_before_active ... ok',
    'test terminal::scrollback_budget_tests::floor_exhaustion_refuses_typed ... FAILED',
    '',
    'test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out;',
  ].join('\n');
  const parsed = parseCargoTestLog(log);
  assert.equal(parsed.tests.length, 2);
  assert.deepEqual(parsed.suites, [{ status: 'FAILED', passed: 1, failed: 1, ignored: 0 }]);
  assert.deepEqual(parsed.totals, { passed: 1, failed: 1 });
});

test('a required test that never ran is a failure, not an assumed pass', () => {
  const log = [
    'running 1 test',
    'test terminal::scrollback_budget_tests::inactive_evicted_before_active ... ok',
    '',
    'test result: ok. 1 passed; 0 failed; 0 ignored;',
  ].join('\n');
  const evaluated = evaluateCargoStep({
    id: 'phase1-budget-pressure',
    exitCode: 0,
    logText: log,
    requiredTests: PHASE1_BUDGET_TESTS,
  });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_CARGO_TEST_ABSENT'));
  // The count mismatch is reported through the missing names, never as a silent partial pass.
  assert.equal(evaluated.counts.required, PHASE1_BUDGET_TESTS.length);
});

test('a cargo run with no result line proves nothing', () => {
  const evaluated = evaluateCargoStep({
    id: 'phase2-supervisor',
    exitCode: 0,
    logText: 'Compiling ferryx v0.1.0\nFinished test profile\n',
    requiredTests: PHASE2_SUPERVISOR_TESTS,
  });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_CARGO_NO_RESULT_LINE'));
});

test('a fully passing cargo run satisfies every required name', () => {
  const rows = PHASE2_SUPERVISOR_TESTS.map((name) => `test ${name} ... ok`);
  const log = [...rows, '', `test result: ok. ${rows.length} passed; 0 failed; 0 ignored;`].join('\n');
  const evaluated = evaluateCargoStep({
    id: 'phase2-supervisor',
    exitCode: 0,
    logText: log,
    requiredTests: PHASE2_SUPERVISOR_TESTS,
  });
  assert.equal(evaluated.status, 'pass');
  assert.deepEqual(evaluated.problems, []);
});

test('cargoTestNamed matches a plain name and a module path, never a prefix', () => {
  assert.equal(cargoTestNamed('exit_zero_never_restarts', 'exit_zero_never_restarts'), true);
  assert.equal(cargoTestNamed('terminal::scrollback_budget_tests::exit_zero_never_restarts', 'exit_zero_never_restarts'), true);
  assert.equal(cargoTestNamed('exit_zero_never_restarts_extra', 'exit_zero_never_restarts'), false);
});

// ---------------------------------------------------------------------------------------------
// soak: schemaVersion 3 with the plan's exact numbers
//
// The JUDGE is unchanged by rev 5.1; what changed is who may act on its answer. These pin the
// arithmetic so an advisory finding is a real finding rather than a rubber stamp.
// ---------------------------------------------------------------------------------------------

function passingSoak(overrides = {}) {
  return {
    schemaVersion: 3,
    verdict: 'pass',
    exitCode: 0,
    fullSpec: true,
    targetBytes: PLAN_SOAK.targetBytes,
    // v3 names the basis its `totalBytes` came from (`soak-pty-footprint.mjs:1812-1816`); the gate
    // requires it, so the fixture carries it.
    measurement: 'phys_footprint',
    plan: {
      sessions: PLAN_SOAK.sessions,
      durationMin: PLAN_SOAK.durationMin,
      intervalMs: PLAN_SOAK.intervalMs,
      trendFromMs: PLAN_SOAK.trendFromMs,
      tolerance: PLAN_SOAK.tolerance,
    },
    sampleCount: PLAN_SOAK.minimumSamples,
    peak: { totalBytes: PLAN_SOAK.targetBytes - 1 },
    ...overrides,
  };
}

test('a passing soak summary is accepted', () => {
  const evaluated = evaluateSoakEvidence({ exitCode: 0, summary: passingSoak() });
  assert.equal(evaluated.status, 'pass');
  assert.deepEqual(evaluated.problems, []);
});

test('a survival-style footprint document is refused on its schema', () => {
  // The older daemon-lifetime gate reads `survival/footprint.json`, a different document. Handing it
  // here must not be reinterpreted as soak evidence.
  const legacy = { footprint: { totalBytes: 1 }, scenarios: [] };
  const evaluated = evaluateSoakEvidence({ exitCode: 0, summary: legacy });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_SOAK_SCHEMA'));
  assert.ok(codesOf(evaluated).includes('GATE_SOAK_VERDICT'));
});

test('a reduced-spec soak cannot report the plan run', () => {
  const evaluated = evaluateSoakEvidence({ exitCode: 0, summary: passingSoak({ fullSpec: false }) });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_SOAK_REDUCED_SPEC'));
});

test('a soak with the wrong plan numbers is refused', () => {
  const short = passingSoak({
    plan: { sessions: 71, durationMin: 5, intervalMs: 30_000, trendFromMs: 40 * 60_000, tolerance: 0.05 },
  });
  const evaluated = evaluateSoakEvidence({ exitCode: 0, summary: short });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_SOAK_PLAN_MISMATCH'));
});

test('a soak over the 2.5 GiB target is a breach', () => {
  const over = passingSoak({ peak: { totalBytes: PLAN_SOAK.targetBytes + 1 } });
  const evaluated = evaluateSoakEvidence({ exitCode: 0, summary: over });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_SOAK_TARGET_EXCEEDED'));
});

test('a soak with too few samples is refused', () => {
  const sparse = passingSoak({ sampleCount: PLAN_SOAK.minimumSamples - 1 });
  const evaluated = evaluateSoakEvidence({ exitCode: 0, summary: sparse });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_SOAK_SAMPLES'));
});

test('a v2 soak summary is refused, so a pre-v3 peak is never judged against a v3 target', () => {
  // The v3 header states a v2 `totalBytes` is an RSS sum and must never be compared with a v3
  // number. Accepting schemaVersion 2 here would do exactly that.
  const evaluated = evaluateSoakEvidence({ exitCode: 0, summary: passingSoak({ schemaVersion: 2 }) });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_SOAK_SCHEMA'));
});

test('a soak summary that names no measurement basis is refused', () => {
  const unnamed = passingSoak({ measurement: undefined });
  const evaluated = evaluateSoakEvidence({ exitCode: 0, summary: unnamed });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_SOAK_MEASUREMENT'));
});

test('a soak summary naming a basis v3 does not emit is refused', () => {
  const invented = passingSoak({ measurement: 'peak-rss-sum' });
  const evaluated = evaluateSoakEvidence({ exitCode: 0, summary: invented });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_SOAK_MEASUREMENT'));
});

// ---------------------------------------------------------------------------------------------
// p99: measured, with the absent threshold recorded as a gap rather than invented
// ---------------------------------------------------------------------------------------------

function passingP99() {
  return {
    schema: 'ferryx-pty-echo-p99/v1',
    arms: {
      'direct-daemon': { count: 200, p50Ms: 1.1, p99Ms: 2.2 },
      'keeper-daemon': { count: 200, p50Ms: 1.4, p99Ms: 2.9 },
    },
    comparison: {
      obtained: true,
      comparable: true,
      p50DeltaMs: 0.3,
      p99DeltaMs: 0.7,
      verdict: 'REPORT ONLY.',
    },
  };
}

test('a p99 comparison that was measured passes and records the missing threshold', () => {
  const evaluated = evaluateP99Evidence({ exitCode: 0, report: passingP99() });
  assert.equal(evaluated.status, 'pass');
  assert.deepEqual(codesOf(evaluated), []);
  assert.deepEqual(
    evaluated.gaps.map((gapEntry) => gapEntry.code),
    ['GATE_P99_NO_THRESHOLD'],
  );
});

test('an unobtained p99 comparison fails and is never replaced by the incomparable arm', () => {
  const report = {
    schema: 'ferryx-pty-echo-p99/v1',
    arms: { 'keeper-uds': { count: 200, p99Ms: 1.0 } },
    comparison: { obtained: false, comparable: false, whyNotDirectlyComparable: 'only one arm ran' },
  };
  const evaluated = evaluateP99Evidence({ exitCode: 0, report });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_P99_NO_COMPARISON'));
  assert.ok(codesOf(evaluated).includes('GATE_P99_ARM_ABSENT'));
});

// ---------------------------------------------------------------------------------------------
// phase 0: forced crash, named frame, refused mismatched dSYM, shipped bytes
// ---------------------------------------------------------------------------------------------

function passingPhase0() {
  return {
    schema: SYMBOLICATION_SCHEMA,
    verdict: 'PASS',
    forcedCrash: { pid: 4242, signal: 'SIGSEGV' },
    symbolicated: { frames: 6, binaryFrames: 3, namedFrames: 3, anonymousFrames: 3 },
    negative: { gateCode: 'DSYM_UUID_MISMATCH', namedFrames: 0 },
    // `capturedBeforeCrash` / `verifiedUnchangedAfterCrash` are the provenance half of the bytes
    // binding: the harness records both (runSymbolicationChecks) and the gate refuses a digest read
    // at some unknown time. A fixture without them describes no provenance at all.
    binary: {
      sha256: 'a'.repeat(64),
      sha256AfterCrash: 'a'.repeat(64),
      uuids: ['1111'],
      capturedBeforeCrash: true,
      verifiedUnchangedAfterCrash: true,
    },
    packagedBinary: { sha256: 'a'.repeat(64) },
    collector: { daemonLogFromCrashedRoot: true },
    // The harness's own named checks, exactly the names it records; the gate reads every one.
    checks: {
      crashReportNamesBinary: { ok: true, detail: 'crash report names ferryx' },
      namedFramePresent: { ok: true, detail: '3 named ferryx frame(s)' },
      gateRefusedMismatch: { ok: true, detail: 'release gate refused with DSYM_UUID_MISMATCH' },
      mismatchNamesNothing: { ok: true, detail: 'no named ferryx frame under the mismatched dSYM' },
      collectorCopiedCrashReport: { ok: true, detail: 'bundle holds ferryx-1.ips' },
      forcedCrashProvenance: { ok: true, detail: 'crash was forced by this run' },
      binaryIdentity: { ok: true, detail: 'digest captured before the crash and unchanged after' },
      packagedBinaryIdentity: { ok: true, detail: 'accepted binary is the packaged member' },
    },
  };
}

test('phase 0 evidence with a forced crash and a shipped binary passes', () => {
  const evaluated = evaluatePhase0Evidence({ exitCode: 0, report: passingPhase0() });
  assert.equal(evaluated.status, 'pass');
  assert.deepEqual(evaluated.problems, []);
});

test('phase 0 refuses a supplied crash report that was never forced', () => {
  const report = { ...passingPhase0(), forcedCrash: null };
  const evaluated = evaluatePhase0Evidence({ exitCode: 0, report });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_PHASE0_NOT_FORCED'));
});

test('phase 0 refuses a negative run that was not the UUID-mismatch refusal', () => {
  const report = { ...passingPhase0(), negative: { gateCode: 'DSYM_MISSING', namedFrames: 0 } };
  const evaluated = evaluatePhase0Evidence({ exitCode: 0, report });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_PHASE0_NEGATIVE_NOT_REFUSED'));
});

test('phase 0 refuses a dSYM that named no frame', () => {
  const report = { ...passingPhase0(), symbolicated: { namedFrames: 0 } };
  const evaluated = evaluatePhase0Evidence({ exitCode: 0, report });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_PHASE0_NO_NAMED_FRAME'));
});

test('phase 0 refuses evidence whose binary is not the packaged binary', () => {
  const report = { ...passingPhase0(), packagedBinary: { sha256: 'b'.repeat(64) } };
  const evaluated = evaluatePhase0Evidence({ exitCode: 0, report });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_PHASE0_BINARY_NOT_SHIPPED'));
});

// ---------------------------------------------------------------------------------------------
// packaging: shipped keeper payload and rollback floor
// ---------------------------------------------------------------------------------------------

test('a packaged keeper whose manifest digest matches its bytes passes', () => {
  const keeperBytes = Buffer.from('#!/bin/sh\nexit 0\n');
  const manifest = { schema: 'ferryx-keeper-bundle/v1', version: '2026.1113.2', sha256: sha256Hex(keeperBytes), bytes: keeperBytes.length };
  const evaluated = evaluateKeeperPayload({ manifest, keeperBytes });
  assert.equal(evaluated.status, 'pass');
  assert.deepEqual(evaluated.problems, []);
});

test('a keeper re-signed after refreshKeeperDigest is refused on the digest', () => {
  // This is the signing-order hazard: `keeper.json` was refreshed, then finalize re-signed the keeper,
  // so the shipped manifest describes bytes that no longer exist.
  const shipped = Buffer.from('refreshed-then-resigned');
  const manifest = { schema: 'ferryx-keeper-bundle/v1', version: '2026.1113.2', sha256: sha256Hex(Buffer.from('pre-sign')), bytes: shipped.length };
  const evaluated = evaluateKeeperPayload({ manifest, keeperBytes: shipped });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_KEEPER_DIGEST_MISMATCH'));
});

test('a keeper that is absent from the packaged bundle is refused', () => {
  const evaluated = evaluateKeeperPayload({
    manifest: { schema: 'ferryx-keeper-bundle/v1', version: '1.0.0', sha256: sha256Hex(Buffer.alloc(0)), bytes: 0 },
    keeperBytes: null,
  });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_KEEPER_BINARY_MISSING'));
});

test('a rollback floor that differs between bundle and release page is refused', () => {
  const bundle = { keeper: { oldestSafeRollbackVersion: '2026.1113.1' } };
  const published = { keeper: { oldestSafeRollbackVersion: '2026.1113.2' } };
  const evaluated = evaluateRollbackFloor({ bundleManifest: bundle, publishedManifest: published });
  assert.equal(evaluated.status, 'fail');
  assert.ok(codesOf(evaluated).includes('GATE_FLOOR_MISMATCH'));
});

test('one shipped floor on both documents passes', () => {
  const doc = { keeper: { oldestSafeRollbackVersion: '2026.1113.1' } };
  const evaluated = evaluateRollbackFloor({ bundleManifest: doc, publishedManifest: doc });
  assert.equal(evaluated.status, 'pass');
});

// ---------------------------------------------------------------------------------------------
// bundle seal
//
// The seal is read from the shipped archive's own `_CodeSignature/CodeResources`, and only the keeper
// is checked against it. Apple records the main executable's hash in the CodeDirectory, not in this
// resource seal: every real bundle on the authoring Mac (2026.1025.4, 2026.1026.1, 2026.1027.6) lists
// `MacOS/ferryx-account`, `-cli` and `-relay` - nested code - and never `MacOS/ferryx`. A check that
// demanded it could never pass on an honest build, so the daemon is bound to the candidate by
// `buildCandidate`'s digest comparison instead. These pin both halves.
// ---------------------------------------------------------------------------------------------

/**
 * A CodeResources document shaped exactly like the ones `codesign` writes.
 *
 * Two details are load-bearing and a tidier fixture would hide the bug this pins:
 *
 *  - every digest sits on its own indented line inside `<data>`. A pattern capturing
 *    `([A-Za-z0-9+/=]+)` between the tags matches ZERO entries of a real file for that reason alone.
 *  - `files` holds a bare `<data>` (SHA-1), while `files2` holds `<dict><key>hash2</key><data>`
 *    (SHA-256), or a `cdhash`/`requirement` pair for nested code. The `rules` section carries regex
 *    patterns such as `^Resources/` that are not sealed paths at all.
 */
function codeResourcesFor(entries, { rules = ['^Resources/'] } = {}) {
  const tab = (depth) => '\t'.repeat(depth);
  const digest = (depth, value) => `${tab(depth)}<data>\n${tab(depth + 1)}${value}\n${tab(depth)}</data>`;
  const files = entries
    .filter((entry) => entry.sha1)
    .map((entry) => `${tab(2)}<key>${entry.path}</key>\n${digest(2, entry.sha1)}`)
    .join('\n');
  // Each entry is a <dict> holding either `hash2` (SHA-256), or `cdhash` for nested code. An entry
  // with neither is not a sealed file and gets no files2 record, exactly as codesign writes it.
  const files2 = entries
    .filter((entry) => entry.sha256 || entry.cdhash)
    .map((entry) => {
      const body = entry.sha256
        ? `${tab(3)}<key>hash2</key>\n${digest(3, entry.sha256)}`
        : `${tab(3)}<key>cdhash</key>\n${digest(3, entry.cdhash)}`;
      return `${tab(2)}<key>${entry.path}</key>\n${tab(2)}<dict>\n${body}\n${tab(2)}</dict>`;
    })
    .join('\n');
  const ruleBody = rules
    .map((pattern) => `${tab(2)}<key>${pattern}</key>\n${tab(2)}<dict>\n${tab(3)}<key>weight</key>\n${tab(3)}<real>20</real>\n${tab(2)}</dict>`)
    .join('\n');
  return [
    '<?xml version="1.0" encoding="UTF-8"?>',
    '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">',
    '<plist version="1.0">',
    '<dict>',
    `${tab(1)}<key>files</key>`,
    `${tab(1)}<dict>`,
    files,
    `${tab(1)}</dict>`,
    `${tab(1)}<key>files2</key>`,
    `${tab(1)}<dict>`,
    files2,
    `${tab(1)}</dict>`,
    `${tab(1)}<key>rules</key>`,
    `${tab(1)}<dict>`,
    ruleBody,
    `${tab(1)}</dict>`,
    `${tab(1)}<key>rules2</key>`,
    `${tab(1)}<dict>`,
    ruleBody,
    `${tab(1)}</dict>`,
    '</dict>',
    '</plist>',
    '',
  ].join('\n');
}

/** The two digests codesign records for one loose file: `files` SHA-1 and `files2` `hash2` SHA-256. */
function sealedEntry(path, bytes, { sha1 = true, sha256 = true } = {}) {
  return {
    path,
    sha1: sha1 ? createHash('sha1').update(bytes).digest('base64') : null,
    sha256: sha256 ? createHash('sha256').update(bytes).digest('base64') : null,
  };
}

// The regression this exists for: measured against a real sealed bundle the old single-line pattern
// found 0 entries, so every release failed GATE_SEAL_NOT_LISTED no matter what the build did.
test('a multi-line <data> digest is found: the shape codesign actually writes', () => {
  const keeperBytes = Buffer.from('#!/bin/sh\nexec ferryx-ptykeeper\n');
  const seal = codeResourcesFor([
    { path: 'Resources/helpers/manifest.json', sha1: 'Jm16oHfXfQv4UX1miszm4xK2oIc=' },
    sealedEntry('Resources/keeper/ferryx-ptykeeper', keeperBytes),
  ]);
  // Guard the fixture itself: if it ever collapses back to one line this test stops proving anything.
  assert.match(seal, /<data>\n\t+[A-Za-z0-9+/=]+\n\t+<\/data>/);
  const evaluated = evaluateBundleSeal({ codeResourcesText: seal, keeperBytes });
  assert.equal(evaluated.status, 'pass', JSON.stringify(evaluated.problems));
  assert.equal(evaluated.observed.sealedEntries, 2);
});

test('the keeper is checked against files2 hash2, the SHA-256 the bundle seals', () => {
  const keeperBytes = Buffer.from('keeper bytes');
  const seal = codeResourcesFor([sealedEntry('Resources/keeper/ferryx-ptykeeper', keeperBytes)]);
  const evaluated = evaluateBundleSeal({ codeResourcesText: seal, keeperBytes });
  assert.equal(evaluated.status, 'pass', JSON.stringify(evaluated.problems));
  assert.equal(
    evaluated.observed.keeperDigest.sha256,
    createHash('sha256').update(keeperBytes).digest('base64'),
  );
});

test('a keeper sealed under Resources/keeper passes without a MacOS/ferryx entry', () => {
  const keeperBytes = Buffer.from('#!/bin/sh\nexec ferryx-ptykeeper\n');
  const seal = codeResourcesFor([
    { path: 'Resources/helpers/manifest.json', sha1: 'Jm16oHfXfQv4UX1miszm4xK2oIc=' },
    sealedEntry('Resources/keeper/ferryx-ptykeeper', keeperBytes),
    { path: 'MacOS/ferryx-cli', cdhash: 'iDFRX4AHVIA1BGE0JT926HZx4sU=' },
  ]);
  const evaluated = evaluateBundleSeal({ codeResourcesText: seal, keeperBytes });
  assert.equal(evaluated.status, 'pass', JSON.stringify(evaluated.problems));
});

test('a keeper re-signed after the seal was written is refused on the seal digest', () => {
  const sealedBytes = Buffer.from('the bytes codesign actually sealed');
  const shippedBytes = Buffer.from('re-signed afterwards, same path');
  const seal = codeResourcesFor([sealedEntry('Resources/keeper/ferryx-ptykeeper', sealedBytes)]);
  const evaluated = evaluateBundleSeal({ codeResourcesText: seal, keeperBytes: shippedBytes });
  assert.equal(evaluated.status, 'fail');
  assert.deepEqual(codesOf(evaluated), ['GATE_SEAL_DIGEST_UNMATCHED']);
});

test('the SHA-256 decides when files2 disagrees with a stale files SHA-1', () => {
  // A digest recorded in only one of the two sections is the half-written seal a partial re-sign
  // leaves. The strong digest is authoritative, so a matching SHA-1 cannot rescue a stale SHA-256.
  const keeperBytes = Buffer.from('current keeper bytes');
  const stale = sealedEntry('Resources/keeper/ferryx-ptykeeper', Buffer.from('older bytes'));
  const seal = codeResourcesFor([{ ...stale, sha1: createHash('sha1').update(keeperBytes).digest('base64') }]);
  const evaluated = evaluateBundleSeal({ codeResourcesText: seal, keeperBytes });
  assert.equal(evaluated.status, 'fail');
  assert.deepEqual(codesOf(evaluated), ['GATE_SEAL_DIGEST_UNMATCHED']);
  assert.match(evaluated.problems[0].detail, /sha256:/);
});

test('a files-only seal still verifies on its SHA-1', () => {
  const keeperBytes = Buffer.from('older bundle without files2');
  const seal = codeResourcesFor([sealedEntry('Resources/keeper/ferryx-ptykeeper', keeperBytes, { sha256: false })]);
  const evaluated = evaluateBundleSeal({ codeResourcesText: seal, keeperBytes });
  assert.equal(evaluated.status, 'pass', JSON.stringify(evaluated.problems));
});

test('a cdhash entry is not mistaken for a sealed file digest', () => {
  // codesign records nested code as cdhash/requirement instead of a content hash. Treating that as a
  // match would report a keeper as sealed on the strength of a signature, not on its bytes.
  const keeperBytes = Buffer.from('keeper bytes');
  const seal = codeResourcesFor([{ path: 'Resources/keeper/ferryx-ptykeeper', cdhash: 'iDFRX4AHVIA1BGE0JT926HZx4sU=' }]);
  const evaluated = evaluateBundleSeal({ codeResourcesText: seal, keeperBytes });
  assert.equal(evaluated.status, 'fail');
  assert.deepEqual(codesOf(evaluated), ['GATE_SEAL_NOT_LISTED']);
});

test('a keeper the seal does not cover is refused even when its manifest digest matches', () => {
  // The manifest and the bytes can agree with each other while the bundle never sealed the binary,
  // which would let the daemon install an unsealed keeper. That is a separate finding from a mismatch.
  const keeperBytes = Buffer.from('unsealed');
  const seal = codeResourcesFor([{ path: 'Resources/helpers/manifest.json', sha1: 'Jm16oHfXfQv4UX1miszm4xK2oIc=' }]);
  const evaluated = evaluateBundleSeal({ codeResourcesText: seal, keeperBytes });
  assert.equal(evaluated.status, 'fail');
  assert.deepEqual(codesOf(evaluated), ['GATE_SEAL_NOT_LISTED']);
});

test('a rules pattern is never read as a sealed path', () => {
  // `rules` holds regexes like `^Resources/`. A document-wide scan would record them as entries and
  // could then "find" a path that the bundle never sealed.
  const seal = codeResourcesFor([{ path: 'Resources/helpers/manifest.json', sha1: 'Jm16oHfXfQv4UX1miszm4xK2oIc=' }], {
    rules: ['^Resources/', '^Resources/keeper/ferryx-ptykeeper', '^MacOS/'],
  });
  const evaluated = evaluateBundleSeal({ codeResourcesText: seal, keeperBytes: Buffer.from('x') });
  assert.equal(evaluated.status, 'fail');
  assert.deepEqual(codesOf(evaluated), ['GATE_SEAL_NOT_LISTED']);
  assert.equal(evaluated.observed.sealedEntries, 1);
});

// ---------------------------------------------------------------------------------------------
// candidate identity and aggregation
// ---------------------------------------------------------------------------------------------

test('the candidate digest is immune to key order and changes with the bytes', () => {
  const a = { commitSha: 'c'.repeat(40), binaries: { daemon: { sha256: 'd'.repeat(64) } } };
  const b = { binaries: { daemon: { sha256: 'd'.repeat(64) } }, commitSha: 'c'.repeat(40) };
  assert.equal(canonicalJson(a), canonicalJson(b));
  assert.equal(candidateDigestOf(a), candidateDigestOf(b));
  const changed = { ...a, binaries: { daemon: { sha256: 'e'.repeat(64) } } };
  assert.notEqual(candidateDigestOf(a), candidateDigestOf(changed));
});

test('every required step must be present and passing', () => {
  const requiredIds = requiredStepIds();
  const allPassing = requiredIds.map((id) => ({ id, status: 'pass', problems: [] }));
  assert.equal(evaluateGate({ steps: allPassing, requiredIds }).verdict, 'accepted');

  const missing = allPassing.filter((step) => step.id !== 'phase3-crash-survival');
  const verdict = evaluateGate({ steps: missing, requiredIds });
  assert.equal(verdict.verdict, 'rejected');
  assert.ok(verdict.problems.some((problem) => problem.code === 'GATE_STEP_ABSENT'));
});

test('a blocked step rejects the gate even when every other step passed', () => {
  const requiredIds = requiredStepIds();
  const steps = requiredIds.map((id) => ({ id, status: 'pass', problems: [] }));
  steps[steps.length - 1] = { id: steps[steps.length - 1].id, status: 'blocked', problems: [{ code: 'GATE_GENERATIONS_MISSING', detail: 'no second build' }] };
  const verdict = evaluateGate({ steps, requiredIds });
  assert.equal(verdict.verdict, 'rejected');
  assert.ok(verdict.problems.some((problem) => problem.code === 'GATE_STEP_NOT_PASSED'));
});

test('a step whose evidence file is missing is never a skip', () => {
  const requiredIds = requiredStepIds();
  const steps = requiredIds.map((id) => ({ id, status: 'pass', problems: [] }));
  steps[0] = { id: steps[0].id, status: 'missing', problems: [] };
  const verdict = evaluateGate({ steps, requiredIds });
  assert.equal(verdict.verdict, 'rejected');
  assert.ok(verdict.problems.some((problem) => problem.code === 'GATE_STEP_EVIDENCE_MISSING'));
});

test('the gate schema is versioned so an older report is not read as this one', () => {
  assert.equal(GATE_SCHEMA, 'ferryx-combined-release-gate/1');
});

// ---------------------------------------------------------------------------------------------
// Phase 1 Test B is ADVISORY (rev 5.1, 2026-11-20)
//
// The soak stopped gating a release: 60 min cannot reach the retained-history ceiling (~14.5 h at
// default settings), Test A proves that ceiling deterministically, and the single t=40 min reference
// fails a FALLING footprint. These pin the consequence: the soak is not a required step, its failure
// cannot reject a green run, and its absence cannot either.
// ---------------------------------------------------------------------------------------------

test('the soak is planned and judged but is not a required step', () => {
  assert.equal(requiredStepIds().includes('phase1-footprint-soak'), false);
  assert.deepEqual([...ADVISORY_STEP_IDS], ['phase1-footprint-soak']);
  // Everything that was required stays required: removing the soak must not weaken another check.
  for (const id of [
    'phase0-symbolication',
    'phase1-budget-pressure',
    'phase2-supervisor',
    'phase2-launchd-chain',
    'phase3-crash-survival',
    'phase4-handover-failure',
    'risk-p99-echo',
    'aux-host-agnostic-tests',
    'packaging-shipped-payload',
  ]) {
    assert.ok(requiredStepIds().includes(id), `${id} is still a gating step`);
  }
});

test('failing soak evidence leaves an all-green gate accepted, with the problems as advisory', () => {
  // Everything else is green: every required step passes. The soak then fails on the two predicates
  // that used to reject the release - a breach over the 2.5 GiB target and a trend outside +/-5%.
  const requiredIds = requiredStepIds();
  const steps = requiredIds.map((id) => ({ id, status: 'pass', problems: [] }));
  const over = evaluateSoakEvidence({ exitCode: 0, summary: passingSoak({ peak: { totalBytes: PLAN_SOAK.targetBytes + 1 } }) });
  assert.equal(over.status, 'fail');
  const drifted = evaluateSoakSamples({
    samples: [
      { elapsedMs: 40 * 60_000, totalBytes: 300 * 1024 * 1024 },
      // 2 min later, up 10%: outside the plan's +/-5% band, and this is exactly what used to reject.
      { elapsedMs: 42 * 60_000, totalBytes: Math.round(300 * 1024 * 1024 * 1.1) },
    ],
  });
  assert.equal(drifted.status, 'fail');

  const evaluated = evaluateGate({
    steps,
    requiredIds,
    advisories: [{ id: 'phase1-footprint-soak', phase: 1, status: 'fail', problems: [...over.problems, ...drifted.problems] }],
  });

  assert.equal(evaluated.verdict, 'accepted');
  assert.deepEqual(evaluated.problems, []);
  assert.equal(evaluated.advisory.length, 1);
  const codes = evaluated.advisory[0].problems.map((problem) => problem.code);
  assert.ok(codes.includes('GATE_SOAK_TARGET_EXCEEDED'), JSON.stringify(codes));
  assert.ok(codes.includes('GATE_SOAK_TREND'), JSON.stringify(codes));
});

test('evaluateSoakAdvisory turns failing evidence into a report entry the verdict ignores', () => {
  // The gate's only soak entry point. It must apply both predicates (a summary can agree with the plan
  // while the samples breach, and vice versa) and must return an ADVISORY entry, not a step.
  const failing = evaluateSoakAdvisory({
    summary: passingSoak({ verdict: 'fail', fullSpec: false }),
    samples: [{ elapsedMs: 40 * 60_000, totalBytes: PLAN_SOAK.targetBytes + 1 }],
  });
  assert.equal(failing.status, 'fail');
  assert.equal(failing.id, 'phase1-footprint-soak');
  assert.equal(failing.phase, 1);
  const codes = failing.problems.map((problem) => problem.code);
  assert.ok(codes.includes('GATE_SOAK_VERDICT'), JSON.stringify(codes));
  assert.ok(codes.includes('GATE_SOAK_REDUCED_SPEC'), JSON.stringify(codes));
  assert.ok(codes.includes('GATE_SOAK_TARGET_EXCEEDED'), JSON.stringify(codes));
  // And the all-green gate that receives it stays accepted.
  const requiredIds = requiredStepIds();
  const steps = requiredIds.map((id) => ({ id, status: 'pass', problems: [] }));
  assert.equal(evaluateGate({ steps, requiredIds, advisories: [failing] }).verdict, 'accepted');
});

test('evaluateSoakAdvisory with absent evidence reports missing evidence, not a pass', () => {
  // A caller that points --soak-evidence at a directory must be told what was (not) found; a silently
  // passing advisory would be worse than no advisory at all.
  const evaluated = evaluateSoakAdvisory({ summary: null, samples: null });
  assert.equal(evaluated.status, 'fail');
  const codes = evaluated.problems.map((problem) => problem.code);
  assert.ok(codes.includes('GATE_SOAK_SUMMARY_MISSING'), JSON.stringify(codes));
  assert.ok(codes.includes('GATE_SOAK_SAMPLES_MISSING'), JSON.stringify(codes));
});

test('no soak evidence at all is not a failure and records no advisory', () => {
  const requiredIds = requiredStepIds();
  const steps = requiredIds.map((id) => ({ id, status: 'pass', problems: [] }));
  const evaluated = evaluateGate({ steps, requiredIds });
  assert.equal(evaluated.verdict, 'accepted');
  assert.deepEqual(evaluated.advisory, []);
});

test('a passing soak is recorded as advisory too, and never as a required step', () => {
  const requiredIds = requiredStepIds();
  const steps = requiredIds.map((id) => ({ id, status: 'pass', problems: [] }));
  const evaluated = evaluateGate({ steps, requiredIds, advisories: [{ id: 'phase1-footprint-soak', phase: 1, status: 'pass', problems: [] }] });
  assert.equal(evaluated.verdict, 'accepted');
  assert.equal(evaluated.advisory[0].status, 'pass');
  assert.deepEqual(
    steps.map((step) => step.id).filter((id) => id === 'phase1-footprint-soak'),
    [],
    'the soak is not in the gated step list',
  );
});

test('the report prints advisory problems under a heading that says they do not gate', () => {
  const digest = 'a'.repeat(64);
  const report = {
    schema: GATE_SCHEMA,
    verdict: 'accepted',
    candidate: { commitSha: 'c'.repeat(40) },
    candidateDigest: digest,
    steps: requiredStepIds().map((id) => ({ id, phase: 1, title: id, status: 'pass', exitCode: 0, problems: [] })),
    gaps: [],
    advisory: [
      {
        id: 'phase1-footprint-soak',
        phase: 1,
        status: 'fail',
        problems: [{ code: 'GATE_SOAK_TREND', detail: 'the sample at t=3600 s is -7.00% from the t=40 min reference' }],
        counts: null,
        detail: 'evidence /tmp/soak + /tmp/soak/footprint-samples.jsonl',
      },
    ],
  };
  const rendered = renderGateReport(report);
  assert.match(rendered, /^# Combined release verification gate - ACCEPTED$/m);
  assert.match(rendered, /^## Advisory \(recorded, never gating\)$/m);
  assert.match(rendered, /phase1-footprint-soak: FAIL/);
  assert.match(rendered, /GATE_SOAK_TREND/);
  assert.match(rendered, /^ACCEPTED: every phase passed on this candidate\.$/m);
  assert.doesNotMatch(rendered, /NOT ACCEPTED/);
});

// ---------------------------------------------------------------------------------------------
// harness JSON extraction
// ---------------------------------------------------------------------------------------------

test('a JSON document preceded by progress lines is still read', () => {
  const text = 'PASS   identity-60-session-sigkill\n\n{"run":"phase3","summary":{"accepted":true}}\n';
  assert.deepEqual(lastJsonDocument(text), { run: 'phase3', summary: { accepted: true } });
});

test('garbage yields null rather than a partial object', () => {
  assert.equal(lastJsonDocument('no json here\n'), null);
  assert.equal(lastJsonDocument(''), null);
  assert.equal(lastJsonDocument('{"truncated": '), null);
});

// ---------------------------------------------------------------------------------------------
// recorded receipts: missing, tampered, and stale-candidate evidence
//
// These are the `--verify-only` paths. A receipt that is absent, unparsable, edited to claim a pass,
// or recorded for a different candidate must all fail: the whole point of retaining raw evidence is
// that a later check can refuse it.
// ---------------------------------------------------------------------------------------------

const DIGEST_A = 'a'.repeat(64);
const DIGEST_B = 'b'.repeat(64);

/** Writes a step receipt at `<out>/steps/<id>/result.json` and returns the out dir. */
function withReceipt(record, fn) {
  const out = mkdtempSync(join(tmpdir(), 'ferryx-gate-test-'));
  try {
    if (record !== undefined) {
      const dir = join(out, 'steps', 'phase3-crash-survival');
      mkdirSync(dir, { recursive: true });
      writeFileSync(join(dir, 'result.json'), typeof record === 'string' ? record : `${JSON.stringify(record, null, 2)}\n`);
    }
    return fn(out);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}

function passingReceipt(overrides = {}) {
  return {
    id: 'phase3-crash-survival',
    phase: 3,
    title: 'phase 3: PTY crash survival',
    status: 'pass',
    exitCode: 0,
    problems: [],
    candidateDigest: DIGEST_A,
    ...overrides,
  };
}

test('a missing receipt is evidence-missing, never an implicit pass', () => {
  withReceipt(undefined, (out) => {
    const verified = verifyRecordedStep({ outDir: out, id: 'phase3-crash-survival', requiredDigest: DIGEST_A });
    assert.equal(verified.status, 'missing');
    assert.deepEqual(verified.problems.map((problem) => problem.code), ['GATE_STEP_EVIDENCE_MISSING']);
  });
});

test('an unparsable receipt is evidence-missing rather than an exception', () => {
  withReceipt('{not json', (out) => {
    const verified = verifyRecordedStep({ outDir: out, id: 'phase3-crash-survival', requiredDigest: DIGEST_A });
    assert.equal(verified.status, 'missing');
  });
});

test('a receipt recorded for another candidate demotes the pass to a failure', () => {
  withReceipt(passingReceipt({ candidateDigest: DIGEST_B }), (out) => {
    const verified = verifyRecordedStep({ outDir: out, id: 'phase3-crash-survival', requiredDigest: DIGEST_A });
    assert.equal(verified.status, 'fail');
    assert.deepEqual(verified.problems.map((problem) => problem.code), ['GATE_CANDIDATE_DRIFT']);
  });
});

test('a receipt for the same candidate passes', () => {
  withReceipt(passingReceipt(), (out) => {
    const verified = verifyRecordedStep({ outDir: out, id: 'phase3-crash-survival', requiredDigest: DIGEST_A });
    assert.equal(verified.status, 'pass');
    assert.deepEqual(verified.problems, []);
  });
});

test('a tampered receipt that claims pass while carrying problems is not accepted', () => {
  // Someone edits the receipt to `status: "pass"` but leaves the failure it recorded behind. The
  // recorded problems are replayed, so the claim does not survive.
  const tampered = passingReceipt({ problems: [{ code: 'GATE_CASE_BLOCKED', detail: 'ownership-disable blocked' }] });
  withReceipt(tampered, (out) => {
    const verified = verifyRecordedStep({ outDir: out, id: 'phase3-crash-survival', requiredDigest: DIGEST_A });
    assert.equal(verified.status, 'fail');
    assert.ok(verified.problems.some((problem) => problem.code === 'GATE_CASE_BLOCKED'));
  });
});

test('a receipt that was never given a candidate digest cannot satisfy a bound check', () => {
  // A receipt with no digest is evidence from an unknown candidate; the gate records it as a drift
  // finding rather than trusting a bare `pass`.
  const unbound = passingReceipt({ candidateDigest: null });
  withReceipt(unbound, (out) => {
    const verified = verifyRecordedStep({ outDir: out, id: 'phase3-crash-survival', requiredDigest: DIGEST_A });
    // The gate always writes a digest, so a digest-less receipt is either hand-written or truncated.
    // Neither is evidence about THIS candidate, and "no digest to compare" must not read as "nothing
    // to object to": the pass is demoted and the reason is typed.
    assert.equal(verified.status, 'fail');
    assert.deepEqual(verified.problems.map((problem) => problem.code), ['GATE_RECEIPT_UNBOUND']);
    assert.equal(verified.candidateDigest, null);
  });
});

test('a recorded blocked step stays blocked through verification', () => {
  withReceipt(passingReceipt({ status: 'blocked', problems: [{ code: 'GATE_GENERATIONS_MISSING', detail: 'one build' }] }), (out) => {
    const verified = verifyRecordedStep({ outDir: out, id: 'phase3-crash-survival', requiredDigest: DIGEST_A });
    assert.equal(verified.status, 'blocked');
  });
});

// ---------------------------------------------------------------------------------------------
// Frozen candidate snapshot
//
// The gate's cross-step digest proves every step agreed with every OTHER step. It says nothing about
// agreeing with the tree that was FROZEN for the run, and two runs of different byte-identical-looking
// trees can both be internally consistent. These pin the binding to `candidate.meta`.
// ---------------------------------------------------------------------------------------------

/** Writes a snapshot dir whose meta describes the files beside it, the way the shell script does. */
function withSnapshot({ metaPatches = {}, patch = 'patch-bytes', manifest = 'manifest-bytes' } = {}, fn) {
  const dir = mkdtempSync(join(tmpdir(), 'ferryx-gate-snapshot-'));
  try {
    writeFileSync(join(dir, 'candidate.patch'), patch);
    writeFileSync(join(dir, 'candidate.manifest'), manifest);
    const fields = {
      base: 'c'.repeat(40),
      patch_sha256: sha256Hex(Buffer.from(patch, 'utf8')),
      manifest_sha256: sha256Hex(Buffer.from(manifest, 'utf8')),
      path_set_sha256: 'd'.repeat(64),
      entries: '3',
      patch_paths: '3',
      gitlinks: '1',
      stable: 'yes',
      ...metaPatches,
    };
    writeFileSync(
      join(dir, 'candidate.meta'),
      `${Object.entries(fields).map(([key, value]) => `${key} ${value}`).join('\n')}\n`,
    );
    return fn(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const snapshotPaths = (dir) => ({
  metaPath: join(dir, 'candidate.meta'),
  patchPath: join(dir, 'candidate.patch'),
  manifestPath: join(dir, 'candidate.manifest'),
});

test('a snapshot whose meta matches the files beside it passes', () => {
  withSnapshot({}, (dir) => {
    const evaluated = evaluateCandidateSnapshot(snapshotPaths(dir));
    assert.equal(evaluated.status, 'pass', JSON.stringify(evaluated.problems));
    assert.equal(evaluated.snapshot.entries, 3);
    assert.equal(evaluated.snapshot.gitlinks, 1);
  });
});

test('an edited patch is refused even when the meta was not updated', () => {
  // This is the substitution the binding exists to catch: swap the staged patch, leave the claim.
  withSnapshot({}, (dir) => {
    writeFileSync(join(dir, 'candidate.patch'), 'different-bytes');
    const evaluated = evaluateCandidateSnapshot(snapshotPaths(dir));
    assert.equal(evaluated.status, 'fail');
    assert.ok(evaluated.problems.some((problem) => problem.code === 'GATE_SNAPSHOT_DIGEST_MISMATCH'));
  });
});

test('an unstable snapshot is not a candidate', () => {
  withSnapshot({ metaPatches: { stable: 'no' } }, (dir) => {
    const evaluated = evaluateCandidateSnapshot(snapshotPaths(dir));
    assert.equal(evaluated.status, 'fail');
    assert.ok(evaluated.problems.some((problem) => problem.code === 'GATE_SNAPSHOT_UNSTABLE'));
  });
});

test('a snapshot staged at another commit than --commit-sha is refused', () => {
  withSnapshot({}, (dir) => {
    const evaluated = evaluateCandidateSnapshot({ ...snapshotPaths(dir), commitSha: 'e'.repeat(40) });
    assert.equal(evaluated.status, 'fail');
    assert.ok(evaluated.problems.some((problem) => problem.code === 'GATE_SNAPSHOT_COMMIT_MISMATCH'));
  });
});

test('a snapshot whose patch and manifest name different path sets is refused', () => {
  withSnapshot({ metaPatches: { patch_paths: '4' } }, (dir) => {
    const evaluated = evaluateCandidateSnapshot(snapshotPaths(dir));
    assert.equal(evaluated.status, 'fail');
    assert.ok(evaluated.problems.some((problem) => problem.code === 'GATE_SNAPSHOT_PATH_SET_SPLIT'));
  });
});

test('a missing candidate.meta is a typed failure rather than a skipped binding', () => {
  const evaluated = evaluateCandidateSnapshot({ metaPath: '/nonexistent/candidate.meta' });
  assert.equal(evaluated.status, 'fail');
  assert.deepEqual(evaluated.problems.map((problem) => problem.code), ['GATE_SNAPSHOT_META_MISSING']);
});

// ---------------------------------------------------------------------------------------------
// Cross-step host consistency
//
// `evaluateHostConsistency` had no caller, so a run whose phase 3 measured one keeper build and
// phase 4 measured another still reported `accepted`. These pin the comparison it performs.
// ---------------------------------------------------------------------------------------------

const receiptedStep = (id, overrides = {}) => ({
  id,
  status: 'pass',
  receipt: {
    host: 'build-1',
    platform: 'darwin',
    head: 'a'.repeat(40),
    binaries: { daemon: 'f'.repeat(64), keeper: '0'.repeat(64) },
    ...overrides,
  },
});

test('two steps that measured the same bytes on the same platform are consistent', () => {
  const evaluated = evaluateHostConsistency({ steps: [receiptedStep('phase3'), receiptedStep('phase4')] });
  assert.equal(evaluated.status, 'pass', JSON.stringify(evaluated.problems));
  assert.deepEqual(evaluated.hosts, ['build-1']);
});

test('a keeper digest that differs between steps is a candidate split', () => {
  const split = receiptedStep('phase4', { binaries: { daemon: 'f'.repeat(64), keeper: '1'.repeat(64) } });
  const evaluated = evaluateHostConsistency({ steps: [receiptedStep('phase3'), split] });
  assert.equal(evaluated.status, 'fail');
  assert.ok(evaluated.problems.some((problem) => problem.code === 'GATE_CANDIDATE_SPLIT'));
});

test('two steps at different commits are a candidate split even when every binary matches', () => {
  const other = receiptedStep('phase4', { head: 'b'.repeat(40) });
  const evaluated = evaluateHostConsistency({ steps: [receiptedStep('phase3'), other] });
  assert.equal(evaluated.status, 'fail');
  assert.ok(evaluated.problems.some((problem) => problem.code === 'GATE_CANDIDATE_SPLIT'));
});

test('symbolication that ran on a non-Darwin host fails regardless of what it reported', () => {
  const linux = receiptedStep('phase0-symbolication', { platform: 'linux' });
  const evaluated = evaluateHostConsistency({ steps: [linux] });
  assert.equal(evaluated.status, 'fail');
  assert.ok(evaluated.problems.some((problem) => problem.code === 'GATE_DARWIN_REQUIRED'));
});

test('a run with no receipts has no identity to compare and does not pass by accident', () => {
  // No receipt means no claim: the note must say so, and the status must not pretend a comparison
  // happened. The steps themselves are still judged by evaluateGate.
  const evaluated = evaluateHostConsistency({ steps: [{ id: 'packaging', status: 'pass', receipt: null }] });
  assert.equal(evaluated.status, 'pass');
  assert.deepEqual(evaluated.hosts, []);
  assert.ok(evaluated.note);
});

// ---------------------------------------------------------------------------------------------
// Procedure documents
//
// Three documents were promised and are still being written. Absence is a recorded GAP, never a
// waiver, and it must never be able to change a verdict - the properties are gated by required steps.
// ---------------------------------------------------------------------------------------------

test('an absent procedure document is a gap and never a verdict change', () => {
  const evaluated = evaluateProcedureDocs({
    repoRoot: '/nonexistent-root-for-test',
    docs: [{ path: '.omo/evidence/supervisor/GATE-COMMAND.md', step: 'phase2-launchd-chain' }],
    stepIds: ['phase2-launchd-chain'],
  });
  assert.equal(evaluated.status, 'pass');
  assert.equal(evaluated.present.length, 0);
  assert.deepEqual(evaluated.gaps.map((gap) => gap.code), ['GATE_PROCEDURE_DOC_ABSENT']);
});

test('a present procedure document is bound by digest and names a step the gate plans', () => {
  const root = mkdtempSync(join(tmpdir(), 'ferryx-gate-docs-'));
  try {
    mkdirSync(join(root, '.omo', 'evidence', 'keeper-shipping'), { recursive: true });
    const body = '# keeper shipping\n\nrun the gate\n';
    writeFileSync(join(root, '.omo', 'evidence', 'keeper-shipping', 'GATE-PROCEDURE.md'), body);
    const evaluated = evaluateProcedureDocs({
      repoRoot: root,
      docs: [{ path: '.omo/evidence/keeper-shipping/GATE-PROCEDURE.md', step: 'packaging-shipped-payload' }],
      stepIds: ['packaging-shipped-payload'],
    });
    assert.equal(evaluated.status, 'pass', JSON.stringify(evaluated.problems));
    assert.equal(evaluated.present[0].sha256, sha256Hex(Buffer.from(body, 'utf8')));
    assert.equal(evaluated.gaps.length, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('a procedure document naming a step this gate does not run is a finding', () => {
  const root = mkdtempSync(join(tmpdir(), 'ferryx-gate-docs-'));
  try {
    mkdirSync(join(root, 'docs'), { recursive: true });
    writeFileSync(join(root, 'docs', 'X.md'), 'body\n');
    const evaluated = evaluateProcedureDocs({
      repoRoot: root,
      docs: [{ path: 'docs/X.md', step: 'a-step-that-does-not-exist' }],
      stepIds: ['phase3-crash-survival'],
    });
    assert.equal(evaluated.status, 'fail');
    assert.deepEqual(evaluated.problems.map((problem) => problem.code), ['GATE_PROCEDURE_DOC_UNKNOWN_STEP']);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
