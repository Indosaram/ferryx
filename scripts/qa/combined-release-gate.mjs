#!/usr/bin/env node
/**
 * Combined release verification gate (plan rev 5, sections "Release: ONE release, all phases",
 * "Verification gates" and "One combined verification gate").
 *
 * The plan ships phases 0-4 in ONE release and states that a single gate covers them all, with no
 * partial-release exemption. Before this file there was no such gate: each acceptance harness could
 * be run by hand, and nothing refused a release whose Phase 3 evidence was blocked, whose packaged
 * keeper payload described bytes the archive does not seal, or whose symbolication report described a
 * different binary. This file is that refusal.
 *
 * What it is not: it does not implement any acceptance property. Every property is owned by the
 * harness that measures it, and this gate runs those harnesses and then judges their evidence:
 *
 *   Phase 0  scripts/qa/dsym-symbolication-acceptance.mjs  forced SIGSEGV of the RELEASE binary,
 *            UUID-matched archived dSYM, named frame, negative run refused with DSYM_UUID_MISMATCH.
 *            Judged a second time by `evaluateSymbolicationEvidence`, the same authority
 *            `release-local publish` uses, so the gate cannot pass evidence publish would refuse.
 *   Phase 1  scripts/soak-pty-footprint.mjs              ADVISORY since rev 5.1 (2026-11-20): 60 min,
 *            71 panes, 30 s samples, `footprint-summary.json` (schemaVersion 3) - explicitly NOT the
 *            `survival/footprint.json` schema of the older daemon-lifetime gate. It is NOT a gating
 *            step: when `--soak-evidence` is supplied its problems go to the report's `advisory`
 *            section and never reach the verdict, and its absence is not a gap. The retained-history
 *            ceiling needs ~14.5 h at default settings to fill, so 60 min cannot exercise it; Test A
 *            proves the ceiling deterministically and a single t=40 min reference made a falling
 *            footprint fail (run14h: 30 of 36 late samples below -5 %, peak 345 MiB vs 2.5 GiB).
 *   Phase 1  `cargo test --lib terminal::scrollback_budget`  the three deterministic pressure cases
 *            (mixed / all-active / floor exhaustion) plus the focused-pane and counter cases.
 *   Phase 2  `cargo test --test daemon_crash_supervisor_acceptance`  four plan cases plus the
 *            "no keeper -> supervised restart stays off" assertion, AND a real launchd ->
 *            supervisor -> daemon chain on an isolated host. The cargo harness alone CANNOT satisfy
 *            Phase 2: it starts children directly, so it never proves launchd owns the supervisor
 *            (the plan's "supervisor ownership belongs to the launchd job, so app replacement cannot
 *            start a second one"). A source audit (st_01a1e627) found no launchd chain exercised
 *            anywhere in the tree, so that half is supplied as external evidence and fails closed
 *            when absent. See `evaluateLaunchdChainEvidence`.
 *   Phase 3  scripts/qa/phase3-pty-crash-survival.acceptance.mjs --keep --json
 *   Phase 4  scripts/qa/phase4-handover-failure-injection.acceptance.mjs --keep --json
 *   Risk row scripts/qa/pty-echo-p99-comparison.mjs      direct-daemon vs keeper-daemon p99.
 *   Aux      `node --test` on the four host-agnostic test files, `scripts/build-host-claims.test.mjs`
 *            among them: that file is the only executable check on `fx_claim_release`'s refusal
 *            semantics, and the whole remote dispatch rests on those semantics.
 *
 * `phase2-launchd-chain` and `packaging-shipped-payload` are required steps, not optional extras:
 * the first is the only evidence launchd owns the supervisor, the second is the only place the
 * shipped keeper's digest and the rollback floor can be read. Both are `blocked` when their evidence
 * is absent, and a blocked step rejects - which is the difference between "missing evidence fails"
 * and "missing evidence is quietly skipped".
 *
 * Three rules make the judgement fail closed, and they are the reason this file exists:
 *
 *  1. **exit 0 is not acceptance.** A harness exits 0 only when every selected case passed, but the
 *     gate does not trust that: it parses the harness's own JSON summary and requires
 *     `summary.accepted === true`, `failed === 0`, `blocked === 0`, `total === the required count`,
 *     and a `pass` outcome for every required case id by name. A run that exits 0 while reporting a
 *     blocked case is rejected, and so is a run whose summary is absent or unparsable.
 *  2. **one candidate.** Every step is bound to one candidate identity: the commit, the plan digest,
 *     and the sha256 of the daemon, keeper and CLI bytes actually used. Each step records the
 *     candidate digest it ran under; any step whose record disagrees is rejected. When a release run
 *     directory is supplied, those bytes must also equal the bytes inside the shipped
 *     `artifacts/darwin/Ferryx.app.tar.gz`, so a `target/` binary cannot be substituted for the
 *     shipped one.
 *  3. **raw output is retained.** Each step's exact argv, exit status and full stdout/stderr are
 *     written under `<out>/steps/<id>/`, and the verdict names those paths. A step whose logs are
 *     missing is a failure, not a skip.
 *
 * A contract self-check is never counted as acceptance. `isolation-guard` (Phase 3/4) and the
 * fixture-level `scripts/dsym-symbolication.test.mjs` are recorded as auxiliary checks: they must
 * pass, but a run in which they are the only thing that ran is rejected (`GATE_SELF_CHECK_ONLY`).
 * `phase4-handover-contract-audit.mjs --self-check` is deliberately not run as a gate step: it starts
 * no daemon and proves nothing about serving, and its own header says so.
 *
 * **Nothing executes locally.** Every executable step - both cargo suites, the three JS acceptance
 * harnesses, the p99 comparison and the aux test files - is dispatched through
 * `scripts/build-remote.sh` to the host `scripts/pick-build-host.sh` claims (AGENTS.md: never a bare
 * ssh, because Tailscale SSH sends no exit-status). Only file reads, JSON judging and report writing
 * happen here. A step that could not run remotely is `blocked`, never skipped. The advisory soak is the
 * one thing this gate never dispatches at all: it reads evidence an operator already produced.
 *
 * `--on-host` is the mode this gate is normally INVOKED in. The orchestrator makes exactly one
 * `scripts/build-remote.sh` call, and that call runs this file inside the staged worktree on the
 * claimed host; every executable step then runs in place. Without the flag the gate would shell out
 * to `build-remote.sh` again for each step, taking a SECOND claim while the first is live - the
 * nested-claim hazard: `pick-build-host.sh` skips a claimed host, so the nested call exits 3, or with
 * a longer roster silently lands on a different machine than the one holding the candidate bytes. The
 * mode also cannot be confused with a local run: it is opt-in, and the receipts it produces still
 * name the host and platform that ran each step.
 *
 * Because the harnesses run on the claimed host, the binaries they use must exist THERE, and the
 * gate never assumes they are the right ones. Every executable step is a small shell program that
 * (1) `cd`s into the staged remote worktree, (2) prints `__FERRYX_RECEIPT__` lines naming the host,
 * its platform and the sha256 of each binary it is about to use, and only then (3) runs the harness.
 * `evaluateRemoteReceipt` compares those digests with the local candidate: a host holding different
 * bytes fails the step instead of producing a pass. `evaluateHostConsistency` then requires the
 * candidate *identity* - revision and every binary digest - to be the same across every step,
 * whatever host each one landed on. It never pins a host and never narrows the roster: which host
 * runs a step is the picker's decision, and a pre-claim would make the picker skip that host
 * (the nested-claim hazard the staging runbook records). Steps that can only be proven on Darwin
 * declare that in `DARWIN_REQUIRED_STEPS` and are judged against the platform the host itself
 * reported; no host name is ever guessed, and a Linux pass of such a step is a failure.
 *
 * The remote paths are named explicitly (`--remote-daemon-binary`, `--remote-keeper-binary`,
 * `--remote-cli-binary`) together with `--remote-worktree` and, for the Darwin-only half of phase 2,
 * `--launchd-chain-evidence`. None of them has a default: a missing one makes its step `blocked`,
 * which is a rejection, never a silent local run.
 *
 * Usage (Bun, like the other acceptance harness in this tree; `node --test` runs the tests only):
 *   bun scripts/qa/combined-release-gate.mjs --list
 *   bun scripts/qa/combined-release-gate.mjs \
 *     --run ~/ferryx-releases/<run> --commit-sha <40-hex> \
 *     --daemon-binary <shipped ferryx> --keeper-binary <shipped ferryx-ptykeeper> \
 *     --cli-binary <shipped ferryx-cli> \
 *     --remote-daemon-binary <path on build host> --remote-keeper-binary <path on build host> \
 *     --remote-cli-binary <path on build host> \
 *     --remote-generations <older>,<newer> --remote-daemon-generations <older>,<newer> \
 *     [--launchd-chain-evidence <dir>] [--out <dir>] [--soak-evidence <dir>] \
 *     [--candidate-snapshot <dir>] [--on-host] \
 *     [--verify-only] [--only <step>]... [--json]
 *
 * In practice the whole run is one remote invocation, which is the only form that covers all hosts:
 *   scripts/build-remote.sh gate-<slug> -- sh -c 'cd <remote worktree> && \
 *     node scripts/qa/combined-release-gate.mjs --on-host ...'
 *
 * Exit codes:
 *   0 accepted - every required step passed and every result is bound to this candidate
 *   1 rejected - a required step failed, was blocked, or its evidence was missing/inconsistent
 *   2 usage or candidate-identity error
 *   3 incomplete - only a subset was requested (`--only`); never a release verdict
 */

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import {
  PACKAGED_APP_ARTIFACT,
  PACKAGED_APP_MEMBER,
  SYMBOLICATION_SCHEMA,
  evaluateSymbolicationEvidence,
} from '../lib/release-symbolication.mjs';
import { readTarGzMember, sha256Hex } from '../lib/dsym-symbolication.mjs';
import { KEEPER_BINARY_NAME, KEEPER_MANIFEST_NAME, KEEPER_MANIFEST_SCHEMA, KEEPER_RESOURCE_DIR } from '../lib/keeper-bundle.mjs';
import { KEEPER_MANIFEST_KEY, OLDEST_SAFE_ROLLBACK_KEY } from '../lib/release-keeper.mjs';

/** This checkout, so every default path is this tree's own. */
const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

/** Marks the gate's own evidence format, so an old report is never read as this one. */
export const GATE_SCHEMA = 'ferryx-combined-release-gate/1';
/** `scripts/soak-pty-footprint.mjs`: SAMPLE_SCHEMA_VERSION. A different schema is a different run. */
export const SOAK_SCHEMA_VERSION = 3;
/**
 * The two bases a v3 `totalBytes` can come from (`soak-pty-footprint.mjs:2280`). v3 measures
 * physically charged footprint on Darwin and resident size elsewhere, and the comment at
 * `soak-pty-footprint.mjs:65-67` states a v2 RSS sum must never be compared with a v3 number, so the
 * basis is read rather than assumed before `peak.totalBytes` is judged against the target.
 */
export const SOAK_MEASUREMENTS = Object.freeze(['phys_footprint', 'rss']);
/** `scripts/qa/pty-echo-p99-comparison.mjs`: REPORT_SCHEMA. */
export const P99_SCHEMA = 'ferryx-pty-echo-p99/v1';

/**
 * Marker each remote step prints once, before it runs anything, so the gate knows which host it
 * reached and which bytes were there. Without a receipt a step is unproven, not merely undocumented.
 */
export const RECEIPT_PREFIX = '__FERRYX_RECEIPT__';

/**
 * The marker the remote program prints with its status. Tailscale SSH sends no exit-status request,
 * so the wrapper's exit code is the ssh client's, not the command's; a missing marker therefore means
 * the chain broke, and it must never be read as success.
 */
export const EXIT_MARKER_PREFIX = '__FERRYX_EXIT__';

/**
 * The evidence format `--launchd-chain-evidence` must carry. It is written by the Darwin host that
 * actually ran the job chain, because nothing on this Mac can produce it (see the Phase 2 note in
 * the header): `launchd.rs`/`keeper_launchd.rs` exist only on Darwin.
 */
export const LAUNCHD_CHAIN_SCHEMA = 'ferryx-launchd-supervisor-chain/1';

/**
 * Steps that cannot be proven on anything but Darwin, keyed by the reason. A Linux host can run the
 * harness and report a pass, but that pass says nothing about launchd ownership, dSYM symbolication
 * or a sealed `.app`, so accepting it would be the exact overclaim the plan forbids.
 */
export const DARWIN_REQUIRED_STEPS = Object.freeze({
  'phase0-symbolication': 'needs xcrun/dwarfdump/symbolicatecrash and a real SIGSEGV of the release binary',
  'phase2-launchd-chain': 'launchd owns the supervisor job; launchd.rs exists only on Darwin',
});
/**
 * The plan's Phase 1 Test B numbers, kept as the reference spec for the ADVISORY soak evidence: they
 * are restated here so a reduced-spec run can never read as the plan's own numbers. Since rev 5.1
 * (2026-11-20) the soak is judged against these but never gates a release.
 */
export const PLAN_SOAK = Object.freeze({
  sessions: 71,
  durationMin: 60,
  intervalMs: 30_000,
  trendFromMs: 40 * 60_000,
  tolerance: 0.05,
  targetBytes: 2560 * 1024 * 1024,
  minimumSamples: 120,
});

/** Phase 11 of the plan: `Session 193` at 1.5 GiB / 8 MiB. Pinned for the budget step's report. */
export const PLAN_ADMISSION_LIMIT = 192;

/** Cases a harness may run that prove only its own contract, never the product. */
const SELF_CHECK_CASE_IDS = Object.freeze(['isolation-guard', 'isolation_guard', 'self-check']);

/**
 * The Phase 3 acceptance set from the plan's "Verification gates" and Phase 3 acceptance text.
 * `mixed-ownership-drain`, `rollback-refusal-names-keeper-count` and
 * `ownership-disable-attaches-keeper-sessions` are the plan's enable/disable/rollback harness with
 * both ownership types present at once. Note what is NOT here: there is no FD-migration predicate.
 * Plan rev 5 lines 149-152 make coexistence an ordering property (new spawns go to the keeper, direct
 * sessions drain where they are), and the Phase 3 author retracted the migration blocker in
 * `.omo/evidence/phase3-gate/CORRECTION-to-st_01a1e623.md`. A gate that demanded a migration seam
 * would be citing a withdrawn defect.
 */
export const PHASE3_REQUIRED_CASES = Object.freeze([
  'identity-60-session-sigkill',
  'no-gap-history-altscreen-downtime-input',
  'overflow-replay-gap-reset',
  'stale-epoch-fenced',
  'multi-generation-compat',
  'mixed-generation-replacement-and-crash',
  'graceful-app-replacement',
  'mixed-ownership-drain',
  'rollback-refusal-names-keeper-count',
  'ownership-disable-attaches-keeper-sessions',
]);

/** Phase 4 (a), (b), (c-direct) and the keeper-owned half of (c). Plan lines 248-263. */
export const PHASE4_REQUIRED_CASES = Object.freeze([
  'a-successor-dies-before-decision',
  'b-successor-dies-after-decision',
  'c-predecessor-dies-before-decision-direct',
  'c-keeper-owned-session-survives-predecessor-death',
  'c-keeper-session-adopted-by-daemon',
]);

/** Phase 1 Test A: the three pressure cases, plus the two that keep the counters honest. */
export const PHASE1_BUDGET_TESTS = Object.freeze([
  'inactive_evicted_before_active',
  'all_sessions_active_stays_within_budget',
  'floor_exhaustion_refuses_typed',
  'a_sole_focused_session_is_trimmed_to_its_own_ceiling',
  'eviction_counters_only_grow_and_start_at_zero',
]);

/** Phase 2: the plan's four supervisor cases and its "no keeper -> stays off" assertion. */
export const PHASE2_SUPERVISOR_TESTS = Object.freeze([
  'abnormal_exit_restarts_exactly_once_then_exit_zero_stops_supervision',
  'exit_zero_never_restarts',
  'handover_predecessor_exits_zero_and_is_never_restarted',
  'six_rapid_failures_stop_at_the_restart_limit',
  'no_keeper_refuses_supervised_restart_and_starts_the_daemon_once',
]);

/**
 * The host-agnostic test files the staging runbook designates as required, as `bun test` targets.
 *
 * `build-host-claims.test.mjs` is listed FIRST on purpose: it is the only executable check on
 * `fx_claim_release`'s refusal semantics, and every remote step in this gate depends on that code
 * handing the host back correctly. A candidate that passes phase 3 and fails this is still not a
 * candidate, because its own evidence dispatch is unproven.
 *
 * All four are host-agnostic by construction (injected spawn/watch, in-process tar fixtures, pure
 * helpers), so any host the picker claims can run them - and they must run THERE, never here.
 */
export const AUX_TEST_FILES = Object.freeze([
  'scripts/build-host-claims.test.mjs',
  'scripts/dsym-symbolication.test.mjs',
  'scripts/release-symbolication.test.mjs',
  'scripts/qa/combined-release-gate.test.mjs',
]);

/**
 * Procedure documents other owners are still writing, per the staging runbook's open items.
 *
 * Each one names the commands its step must be executed with. While a document is absent this gate
 * records a `GATE_PROCEDURE_DOC_ABSENT` gap naming its owner's evidence directory - a gap is a
 * recorded fact, never a waiver: every property those documents describe is ALREADY gated by a
 * required step that fails closed when its evidence is missing (`phase2-launchd-chain`,
 * `phase4-handover-failure`). The document adds procedural provenance; it cannot make a missing
 * property pass. When it appears, its digest is bound into the report and the step ids it names are
 * required to exist, so a document that describes a step this gate does not run is a finding.
 */
export const PROCEDURE_DOCS = Object.freeze([
  { path: '.omo/evidence/supervisor/GATE-COMMAND.md', step: 'phase2-launchd-chain' },
  { path: '.omo/evidence/phase4-handover-failure-injection/GATE-COMMAND.md', step: 'phase4-handover-failure' },
  { path: '.omo/evidence/keeper-shipping/GATE-PROCEDURE.md', step: 'packaging-shipped-payload' },
]);

// ---------------------------------------------------------------------------------------------
// pure helpers (exported so scripts/qa/combined-release-gate.test.mjs can pin them)
// ---------------------------------------------------------------------------------------------

/** Typed refusal: `code` leads the message and every problem is carried in `problems`. */
export class GateError extends Error {
  constructor(code, message, problems = []) {
    super(`${code}: ${message}`);
    this.name = 'GateError';
    this.code = code;
    this.problems = problems;
  }
}

/** Stable JSON: sorted keys, so a digest over a candidate does not depend on key order. */
export function canonicalJson(value) {
  const walk = (node) => {
    if (Array.isArray(node)) return node.map(walk);
    if (node && typeof node === 'object') {
      return Object.fromEntries(
        Object.keys(node)
          .sort()
          .map((key) => [key, walk(node[key])]),
      );
    }
    return node;
  };
  return JSON.stringify(walk(value));
}

/** The one identity every step must agree on. */
export function candidateDigestOf(candidate) {
  return createHash('sha256').update(canonicalJson(candidate)).digest('hex');
}

/**
 * Binds the gate to ONE frozen candidate snapshot - `stage-dirty-candidate.sh`'s `candidate.meta`.
 *
 * The gate already requires every step to agree with every other on a candidate digest, but that
 * digest is computed from whatever binaries are on this Mac at the moment it runs. Two runs a minute
 * apart can therefore both be "internally consistent" while having gated different bytes. The frozen
 * snapshot closes that: `candidate.meta` records the base commit, the patch sha256, the manifest
 * sha256 and the path-set sha256 of the exact tree that is supposed to run on the host, and this
 * function re-derives them from the files beside it.
 *
 * Re-derivation, not trust: `patch_sha256` and `manifest_sha256` are recomputed from
 * `candidate.patch` and `candidate.manifest` on disk. A meta file that claims one digest while the
 * file beside it hashes to another is refused, because the point of freezing is that the claim and
 * the bytes cannot diverge. `stable yes` is required: the script writes `stable no`/fails when the
 * tree moved between its two passes, and a moving tree has no candidate.
 */
export function evaluateCandidateSnapshot({ metaPath, patchPath, manifestPath, commitSha = null }) {
  const problems = [];
  const readField = (text, name) => {
    const match = new RegExp(`^${name} (.*)$`, 'm').exec(text);
    return match ? match[1].trim() : null;
  };
  if (!metaPath || !existsSync(metaPath)) {
    return {
      status: 'fail',
      problems: [{ code: 'GATE_SNAPSHOT_META_MISSING', detail: `${metaPath ?? 'candidate.meta'} was not readable` }],
      snapshot: null,
    };
  }
  const metaText = readFileSync(metaPath, 'utf8');
  const base = readField(metaText, 'base');
  const patchSha256 = readField(metaText, 'patch_sha256');
  const manifestSha256 = readField(metaText, 'manifest_sha256');
  const pathSetSha256 = readField(metaText, 'path_set_sha256');
  const stable = readField(metaText, 'stable');
  const entries = readField(metaText, 'entries');
  const patchPaths = readField(metaText, 'patch_paths');
  const gitlinks = readField(metaText, 'gitlinks');

  if (!/^[0-9a-f]{40}$/.test(base ?? '')) {
    problems.push({ code: 'GATE_SNAPSHOT_BASE_INVALID', detail: `candidate.meta base is ${JSON.stringify(base)}` });
  }
  if (commitSha && base && commitSha !== base) {
    problems.push({
      code: 'GATE_SNAPSHOT_COMMIT_MISMATCH',
      detail: `--commit-sha ${commitSha} but the frozen candidate was staged at ${base}`,
    });
  }
  if (stable !== 'yes') {
    problems.push({
      code: 'GATE_SNAPSHOT_UNSTABLE',
      detail: `candidate.meta records ${JSON.stringify(stable)}; the tree moved while it was snapshotted, so it is not a candidate`,
    });
  }

  // Recompute rather than trust. A snapshot whose meta disagrees with the bytes beside it is exactly
  // the substitution this binding exists to catch.
  const verifyFile = (label, path, claimed) => {
    if (!path || !existsSync(path)) {
      problems.push({ code: 'GATE_SNAPSHOT_FILE_MISSING', detail: `${label} at ${path ?? '(unset)'} does not exist` });
      return null;
    }
    const actual = sha256Hex(readFileSync(path));
    if (claimed && actual !== claimed) {
      problems.push({
        code: 'GATE_SNAPSHOT_DIGEST_MISMATCH',
        detail: `${label} hashes to ${actual} but candidate.meta records ${claimed}`,
      });
    }
    return actual;
  };
  const actualPatch = verifyFile('candidate.patch', patchPath, patchSha256);
  const actualManifest = verifyFile('candidate.manifest', manifestPath, manifestSha256);

  // A patch and a manifest that name different path sets describe different trees. The staging script
  // refuses this itself, so seeing it here means the directory was edited by hand afterwards.
  if (patchPaths !== null && entries !== null && patchPaths !== entries) {
    problems.push({
      code: 'GATE_SNAPSHOT_PATH_SET_SPLIT',
      detail: `candidate.meta records ${entries} manifest entries but ${patchPaths} patch paths`,
    });
  }

  return {
    status: problems.length === 0 ? 'pass' : 'fail',
    problems,
    snapshot: {
      metaPath,
      base,
      patchSha256: actualPatch ?? patchSha256,
      manifestSha256: actualManifest ?? manifestSha256,
      pathSetSha256,
      entries: entries === null ? null : Number(entries),
      patchPaths: patchPaths === null ? null : Number(patchPaths),
      gitlinks: gitlinks === null ? null : Number(gitlinks),
    },
  };
}

/**
 * Parses `cargo test` output into the per-test results and the `test result:` totals.
 *
 * Only real observed lines are recorded: a missing test name is `absent`, never assumed to have
 * passed because the suite as a whole printed `ok`.
 */
export function parseCargoTestLog(text) {
  const source = String(text ?? '');
  const tests = [];
  const suites = [];
  for (const line of source.split('\n')) {
    const running = /^\s*running \d+ tests?/.exec(line);
    if (running) continue;
    const result = /^\s*test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;/.exec(line);
    if (result) {
      suites.push({
        status: result[1],
        passed: Number(result[2]),
        failed: Number(result[3]),
        ignored: Number(result[4]),
      });
      continue;
    }
    const row = /^\s*test ([^\s]+) \.\.\. (ok|FAILED|ignored)\b/.exec(line);
    if (row) tests.push({ name: row[1], status: row[2] });
  }
  return {
    tests,
    suites,
    totals: {
      passed: suites.reduce((sum, suite) => sum + suite.passed, 0),
      failed: suites.reduce((sum, suite) => sum + suite.failed, 0),
    },
  };
}

/** True when a cargo test path `terminal::scrollback_budget_tests::foo` names `want`. */
export function cargoTestNamed(fullName, want) {
  return fullName === want || String(fullName).endsWith(`::${want}`);
}

/**
 * Judges one cargo step's log: every required test must appear by name with `ok`, and no suite may
 * report a failure. A filtered run that skipped a required name therefore fails, which is the point.
 */
export function evaluateCargoStep({ id, exitCode, logText, requiredTests }) {
  const problems = [];
  const parsed = parseCargoTestLog(logText);
  if (exitCode !== 0) {
    problems.push({ code: 'GATE_CARGO_EXIT', detail: `${id} exited ${exitCode}` });
  }
  if (parsed.suites.length === 0) {
    problems.push({
      code: 'GATE_CARGO_NO_RESULT_LINE',
      detail: `${id} printed no "test result:" line, so no test outcome was observed`,
    });
  }
  if (parsed.totals.failed !== 0) {
    problems.push({ code: 'GATE_CARGO_FAILURES', detail: `${id} reported ${parsed.totals.failed} failed test(s)` });
  }
  for (const want of requiredTests) {
    const row = parsed.tests.find((test) => cargoTestNamed(test.name, want));
    if (!row) {
      problems.push({ code: 'GATE_CARGO_TEST_ABSENT', detail: `${id} never ran ${want}` });
    } else if (row.status !== 'ok') {
      problems.push({ code: 'GATE_CARGO_TEST_NOT_OK', detail: `${id}: ${want} was ${row.status}` });
    }
  }
  return {
    status: problems.length === 0 ? 'pass' : 'fail',
    problems,
    counts: { required: requiredTests.length, passed: parsed.totals.passed, failed: parsed.totals.failed },
  };
}

/**
 * Judges one harness run from its own JSON summary.
 *
 * `requiredCases` is the exact set the gate asked the harness to run (`--case` per id), so the
 * summary must account for exactly that many cases. Anything else - a partial run, a self-check-only
 * run, a blocked case under an exit code of 0 - is rejected.
 */
export function evaluateHarnessStep({ id, exitCode, summary, requiredCases, selfCheckIds = SELF_CHECK_CASE_IDS }) {
  const problems = [];
  if (exitCode !== 0) problems.push({ code: 'GATE_HARNESS_EXIT', detail: `${id} exited ${exitCode}` });
  if (!summary || typeof summary !== 'object') {
    problems.push({
      code: 'GATE_HARNESS_SUMMARY_MISSING',
      detail: `${id} produced no parseable JSON summary, so nothing about its cases is known`,
    });
    return { status: 'fail', problems, counts: null };
  }
  const counts = summary.summary && typeof summary.summary === 'object' ? summary.summary : null;
  const outcomes = Array.isArray(summary.outcomes) ? summary.outcomes : [];
  if (!counts) {
    problems.push({ code: 'GATE_HARNESS_COUNTS_MISSING', detail: `${id} summary carries no summary counts` });
  }
  if (outcomes.length === 0) {
    problems.push({ code: 'GATE_HARNESS_OUTCOMES_MISSING', detail: `${id} summary carries no per-case outcomes` });
  }

  const byId = new Map(outcomes.map((row) => [row?.id, row]));
  for (const requiredId of requiredCases) {
    const row = byId.get(requiredId);
    if (!row) {
      problems.push({ code: 'GATE_CASE_ABSENT', detail: `${id} never ran ${requiredId}` });
      continue;
    }
    if (row.outcome === 'pass') continue;
    const why = row.failure ?? row.gap ?? row.detail ?? row.outcome ?? 'unknown';
    problems.push({
      code: row.outcome === 'blocked' ? 'GATE_CASE_BLOCKED' : 'GATE_CASE_FAILED',
      detail: `${id}: ${requiredId} was ${row.outcome}: ${why}`,
    });
  }

  if (counts) {
    if (counts.failed !== 0) {
      problems.push({ code: 'GATE_CASE_FAILED', detail: `${id} reported ${counts.failed} failed case(s)` });
    }
    // A blocked case is not a pass even when the process exited 0. This is the assertion that
    // makes "raw exit zero" insufficient.
    if (counts.blocked !== 0) {
      problems.push({ code: 'GATE_CASE_BLOCKED', detail: `${id} reported ${counts.blocked} blocked case(s)` });
    }
    // `accepted` is the harness's own claim, so it is checked against the counts rather than
    // believed: a self-reported `accepted: true` beside a blocked or failed case is exactly the
    // contradiction that makes a raw exit-0 check a false pass. Acceptance is derived here.
    const derivedAccepted = counts.accepted === true && counts.blocked === 0 && counts.failed === 0;
    if (!derivedAccepted) {
      problems.push({
        code: 'GATE_NOT_ACCEPTED',
        detail:
          counts.accepted === true
            ? `${id} reported accepted: true while reporting ${counts.failed} failed and ${counts.blocked} blocked case(s)`
            : `${id} did not report accepted: true`,
      });
    }
    if (counts.total !== requiredCases.length) {
      problems.push({
        code: 'GATE_CASE_COUNT_MISMATCH',
        detail: `${id} ran ${counts.total} case(s), the gate selected ${requiredCases.length}`,
      });
    }
    if (counts.passed !== requiredCases.length) {
      problems.push({
        code: 'GATE_CASE_COUNT_MISMATCH',
        detail: `${id} passed ${counts.passed} case(s), the gate selected ${requiredCases.length}`,
      });
    }
  }

  const exercised = outcomes.some((row) => row?.outcome === 'pass' && !selfCheckIds.includes(row?.id));
  if (!exercised) {
    problems.push({
      code: 'GATE_SELF_CHECK_ONLY',
      detail: `${id} passed only contract self-checks, so no acceptance property was exercised`,
    });
  }

  return {
    status: problems.length === 0 ? 'pass' : 'fail',
    problems,
    counts: counts ?? null,
  };
}

/**
 * Judges `footprint-summary.json` (soak schemaVersion 3).
 *
 * The older daemon-lifetime gate reads `survival/footprint.json`, which is a different document; a
 * file without `schemaVersion`, `verdict`, `sampleCount` and `peak.totalBytes` is refused here rather
 * than reinterpreted, so the two schemas can never be confused for one another.
 *
 * v3 added `measurement`, which names the basis `totalBytes` was measured on. That field is not
 * decoration here: the whole point of the 2.5 GiB target is a `footprint -p` reading, and v3's
 * header comment states a v2 RSS sum must never be compared with a v3 number. So the basis is
 * required and named in the counts instead of being inferred from `peak.totalBytes` alone.
 *
 * ADVISORY (rev 5.1, 2026-11-20): this judge is unchanged and still exported, but its `fail` no
 * longer rejects a release. Callers route the result into the gate report's `advisory` section. The
 * predicates are kept exact so the advisory says something real: a soak that breached the target or
 * drifted off its trend is still reported as failing, it just cannot hold the candidate back.
 */
export function evaluateSoakEvidence({ exitCode, summary, plan = PLAN_SOAK }) {
  const problems = [];
  // `null` means the caller verified existing evidence rather than observing a live exit; the
  // recorded `summary.exitCode` below is what decides in that case.
  if (exitCode !== null && exitCode !== 0) {
    problems.push({ code: 'GATE_SOAK_EXIT', detail: `the soak exited ${exitCode}` });
  }
  if (!summary || typeof summary !== 'object') {
    problems.push({ code: 'GATE_SOAK_SUMMARY_MISSING', detail: 'no footprint-summary.json was read' });
    return { status: 'fail', problems, counts: null };
  }
  if (summary.schemaVersion !== SOAK_SCHEMA_VERSION) {
    problems.push({
      code: 'GATE_SOAK_SCHEMA',
      detail:
        `footprint summary schemaVersion is ${JSON.stringify(summary.schemaVersion ?? null)}, expected ` +
        `${SOAK_SCHEMA_VERSION}; a survival-style footprint.json is a different document`,
    });
  }
  if (summary.verdict !== 'pass') {
    problems.push({ code: 'GATE_SOAK_VERDICT', detail: `the soak verdict is ${JSON.stringify(summary.verdict ?? null)}` });
  }
  if (summary.exitCode !== 0) {
    problems.push({ code: 'GATE_SOAK_RECORDED_EXIT', detail: `the soak recorded exit ${summary.exitCode}` });
  }
  if (summary.fullSpec !== true) {
    problems.push({
      code: 'GATE_SOAK_REDUCED_SPEC',
      detail: 'the soak ran a reduced spec, so its pass does not describe the plan run',
    });
  }
  const recordedPlan = summary.plan && typeof summary.plan === 'object' ? summary.plan : {};
  for (const key of ['sessions', 'durationMin', 'intervalMs', 'trendFromMs', 'tolerance']) {
    if (recordedPlan[key] !== plan[key]) {
      problems.push({
        code: 'GATE_SOAK_PLAN_MISMATCH',
        detail: `soak plan.${key} is ${JSON.stringify(recordedPlan[key] ?? null)}, the plan states ${plan[key]}`,
      });
    }
  }
  if (!Number.isInteger(summary.sampleCount) || summary.sampleCount < plan.minimumSamples) {
    problems.push({
      code: 'GATE_SOAK_SAMPLES',
      detail: `the soak took ${JSON.stringify(summary.sampleCount ?? null)} samples, needs at least ${plan.minimumSamples}`,
    });
  }
  if (!SOAK_MEASUREMENTS.includes(summary.measurement)) {
    problems.push({
      code: 'GATE_SOAK_MEASUREMENT',
      detail:
        `the soak's measurement basis is ${JSON.stringify(summary.measurement ?? null)}, expected one of ` +
        `${SOAK_MEASUREMENTS.join('|')}; its peak cannot be compared with the target without it`,
    });
  }
  const peak = summary.peak && typeof summary.peak === 'object' ? summary.peak : null;
  const target = typeof summary.targetBytes === 'number' ? summary.targetBytes : plan.targetBytes;
  if (!peak || typeof peak.totalBytes !== 'number') {
    problems.push({ code: 'GATE_SOAK_PEAK_MISSING', detail: 'the summary records no peak.totalBytes' });
  } else if (peak.totalBytes > target) {
    problems.push({
      code: 'GATE_SOAK_TARGET_EXCEEDED',
      detail: `peak combined footprint ${peak.totalBytes} exceeds the ${target} byte target`,
    });
  }
  return {
    status: problems.length === 0 ? 'pass' : 'fail',
    problems,
    counts: peak
      ? {
          peakTotalBytes: peak.totalBytes,
          samples: summary.sampleCount ?? null,
          measurement: summary.measurement ?? null,
        }
      : null,
  };
}

/**
 * Judges the p99 comparison.
 *
 * The plan's risk row asks for a p99 echo benchmark against today's baseline and defines **no**
 * numeric threshold, so this gate requires the measurement (both comparable arms obtained through the
 * same daemon endpoint shape) and records the absence of a threshold as a stated gap. It does not
 * invent a number and it does not accept the incomparable `keeper-uds` arm as a substitute.
 */
export function evaluateP99Evidence({ exitCode, report }) {
  const problems = [];
  if (exitCode !== 0) problems.push({ code: 'GATE_P99_EXIT', detail: `the p99 harness exited ${exitCode}` });
  if (!report || typeof report !== 'object') {
    problems.push({ code: 'GATE_P99_REPORT_MISSING', detail: 'no p99 report JSON was read' });
    return { status: 'fail', problems, counts: null, gaps: [] };
  }
  if (report.schema !== P99_SCHEMA) {
    problems.push({
      code: 'GATE_P99_SCHEMA',
      detail: `p99 report schema is ${JSON.stringify(report.schema ?? null)}, expected ${P99_SCHEMA}`,
    });
  }
  if (report.failed === true) {
    problems.push({ code: 'GATE_P99_FAILED', detail: `the p99 harness reported ${report.code ?? 'a gap'}` });
  }
  const comparison = report.comparison && typeof report.comparison === 'object' ? report.comparison : null;
  if (!comparison || comparison.obtained !== true) {
    problems.push({
      code: 'GATE_P99_NO_COMPARISON',
      detail: comparison?.whyNotDirectlyComparable ?? 'no direct-daemon vs keeper-daemon comparison was obtained',
    });
  }
  for (const arm of ['direct-daemon', 'keeper-daemon']) {
    const measured = report.arms?.[arm];
    if (!measured || !Number.isInteger(measured.count) || measured.count <= 0) {
      problems.push({ code: 'GATE_P99_ARM_ABSENT', detail: `arm ${arm} was not measured` });
    }
  }
  return {
    status: problems.length === 0 ? 'pass' : 'fail',
    problems,
    counts: comparison ? { p50DeltaMs: comparison.p50DeltaMs ?? null, p99DeltaMs: comparison.p99DeltaMs ?? null } : null,
    gaps: [
      {
        code: 'GATE_P99_NO_THRESHOLD',
        detail:
          'plan rev 5 defines no numeric p99 target, so this comparison is a recorded measurement, not a ' +
          'threshold gate; a pass here says the benchmark ran, not that latency is acceptable',
      },
    ],
  };
}

/**
 * Reads the `__FERRYX_RECEIPT__` lines a remote step printed before it ran anything.
 *
 * The format is one `key=value` per line, repeated per key (`binary` up to three times). It is
 * parsed rather than grepped so a missing key is a specific finding, and any line that is not a
 * well-formed pair is reported instead of being skipped: a truncated receipt must not read as a
 * receipt that simply omitted a field.
 */
export function parseRemoteReceipt(text) {
  const receipt = { host: null, platform: null, arch: null, head: null, binaries: {}, malformed: [] };
  for (const line of String(text ?? '').split('\n')) {
    if (!line.startsWith(RECEIPT_PREFIX)) continue;
    const body = line.slice(RECEIPT_PREFIX.length).trim();
    if (body === '') {
      receipt.malformed.push(line);
      continue;
    }
    const equals = body.indexOf('=');
    if (equals <= 0) {
      receipt.malformed.push(line);
      continue;
    }
    const key = body.slice(0, equals).trim();
    const value = body.slice(equals + 1).trim();
    if (key === 'binary') {
      const separator = value.indexOf(':');
      if (separator <= 0) {
        receipt.malformed.push(line);
        continue;
      }
      receipt.binaries[value.slice(0, separator)] = value.slice(separator + 1);
      continue;
    }
    if (key === 'host' || key === 'platform' || key === 'arch' || key === 'head') receipt[key] = value;
    else receipt.malformed.push(line);
  }
  return receipt;
}

/**
 * Judges a remote step's receipt against the candidate the gate is gating.
 *
 * This is what makes "the harness ran remotely" mean something: the host re-hashes the binaries it
 * is about to use, and those digests must equal the local candidate's. The alternative - trusting
 * that whatever the host had lying around is the release - is how a gate passes a build nobody
 * shipped. `requiredBinaries` names the labels the step actually uses, so a receipt that omits one
 * is a failure rather than a pass on the ones it did mention.
 */
export function evaluateRemoteReceipt({ id, receipt, candidate, requiredBinaries }) {
  const problems = [];
  if (!receipt || typeof receipt !== 'object') {
    problems.push({
      code: 'GATE_RECEIPT_ABSENT',
      detail: `${id} printed no ${RECEIPT_PREFIX} line, so its host and bytes are unknown`,
    });
    return { status: 'fail', problems, observed: null };
  }
  if (receipt.malformed?.length) {
    problems.push({
      code: 'GATE_RECEIPT_MALFORMED',
      detail: `${id} printed ${receipt.malformed.length} unparsable receipt line(s): ${JSON.stringify(receipt.malformed.slice(0, 3))}`,
    });
  }
  for (const key of ['host', 'platform', 'arch']) {
    if (!receipt[key]) problems.push({ code: 'GATE_RECEIPT_INCOMPLETE', detail: `${id} receipt names no ${key}` });
  }
  for (const label of requiredBinaries) {
    const observed = receipt.binaries?.[label] ?? null;
    const expected = candidate?.binaries?.[label]?.sha256 ?? null;
    if (!expected) {
      problems.push({ code: 'GATE_RECEIPT_NO_CANDIDATE', detail: `${id} needs the ${label} digest, the candidate records none` });
      continue;
    }
    if (!observed) {
      problems.push({ code: 'GATE_RECEIPT_BINARY_ABSENT', detail: `${id} receipt does not hash the ${label} binary it used` });
      continue;
    }
    if (observed !== expected) {
      problems.push({
        code: 'GATE_REMOTE_BINARY_DRIFT',
        detail: `${id} used a ${label} hashing to ${observed}, the candidate ships ${expected}`,
      });
    }
  }
  if (candidate?.commitSha && receipt.head && receipt.head !== candidate.commitSha) {
    problems.push({
      code: 'GATE_REMOTE_REVISION_DRIFT',
      detail: `${id} ran in a worktree at ${receipt.head}, the candidate is ${candidate.commitSha}`,
    });
  }
  return { status: problems.length === 0 ? 'pass' : 'fail', problems, observed: receipt };
}

/**
 * Requires every step to have measured the SAME candidate, and the Darwin-only steps to have done so
 * on Darwin.
 *
 * Two things this deliberately does not do. It does not require one host: which host a step lands on
 * is the picker's decision and pinning it would either narrow the roster or collide with the claim
 * `build-remote.sh` takes for itself. And it does not infer a platform from a host's name - the
 * platform is whatever the receipt says, because a name is a guess and a guess is how "phase 3
 * passed" quietly becomes a Linux claim about launchd.
 *
 * Only steps that produced a receipt take part. A step that ran no remote command (the packaging
 * check reads the tarball locally) has nothing to compare and is skipped.
 */
export function evaluateHostConsistency({ steps, darwinRequired = DARWIN_REQUIRED_STEPS }) {
  const problems = [];
  const observed = [];
  for (const step of steps) {
    const receipt = step?.receipt ?? null;
    if (!receipt || !receipt.host) continue;
    observed.push({
      id: step.id,
      host: receipt.host,
      platform: receipt.platform ?? null,
      arch: receipt.arch ?? null,
      head: receipt.head ?? null,
      binaries: receipt.binaries ?? {},
    });
  }
  if (observed.length === 0) {
    return {
      status: 'pass',
      problems,
      hosts: [],
      note: 'no step produced a remote receipt, so there was no cross-host identity to compare',
    };
  }

  const reference = observed[0];
  for (const entry of observed.slice(1)) {
    if (entry.head && reference.head && entry.head !== reference.head) {
      problems.push({
        code: 'GATE_CANDIDATE_SPLIT',
        detail: `${entry.id} ran at ${entry.head} on ${entry.host} while ${reference.id} ran at ${reference.head} on ${reference.host}`,
      });
    }
    for (const [label, sha256] of Object.entries(entry.binaries)) {
      const first = reference.binaries[label];
      if (!first || first === sha256) continue;
      problems.push({
        code: 'GATE_CANDIDATE_SPLIT',
        detail: `${entry.id} measured ${label} ${sha256} on ${entry.host}, ${reference.id} measured ${first} on ${reference.host}`,
      });
    }
  }

  for (const [id, why] of Object.entries(darwinRequired)) {
    const entry = observed.find((row) => row.id === id);
    if (!entry) continue;
    if (entry.platform !== 'darwin') {
      problems.push({
        code: 'GATE_DARWIN_REQUIRED',
        detail: `${id} ran on ${entry.host} reporting platform ${JSON.stringify(entry.platform)}; ${why}, so a non-Darwin pass proves nothing`,
      });
    }
  }

  const hosts = [...new Set(observed.map((entry) => entry.host))];
  return { status: problems.length === 0 ? 'pass' : 'fail', problems, hosts, note: null };
}

/**
 * Judges the soak from its SAMPLES, not from its summary's verdict.
 *
 * The plan (Phase 1 Test B) states the rule arithmetically: every sample in the last 20 minutes must
 * be within +/-5 % of the sample at t=40 min, and no sample may exceed 2.5 GiB combined. A summary's
 * `verdict` is the harness's own conclusion about that; accepting it would let a summary disagree with
 * the samples it was computed from and still pass. So `footprint-samples.jsonl` is read here and the
 * arithmetic is redone, and a summary whose verdict says `pass` while a sample breaches is a failure
 * with the breaching sample named.
 *
 * `samples` is the parsed JSONL array. Anything unparsable, out of range or off the plan's cadence is
 * a problem rather than something to skip: a sample stream with holes cannot support "no upward trend".
 *
 * ADVISORY (rev 5.1, 2026-11-20), for the same reason as `evaluateSoakEvidence`: the arithmetic is
 * still applied and still reported, but a breach here is recorded and cannot reject the candidate.
 */
export function evaluateSoakSamples({ samples, plan = PLAN_SOAK }) {
  const problems = [];
  if (!Array.isArray(samples) || samples.length === 0) {
    problems.push({ code: 'GATE_SOAK_SAMPLES_MISSING', detail: 'no footprint samples were read, so the trend rule was never applied' });
    return { status: 'fail', problems, counts: null };
  }
  const usable = samples.filter((sample) => Number.isFinite(sample?.elapsedMs) && Number.isFinite(sample?.totalBytes));
  if (usable.length !== samples.length) {
    problems.push({
      code: 'GATE_SOAK_SAMPLE_MALFORMED',
      detail: `${samples.length - usable.length} of ${samples.length} sample(s) carry no numeric elapsedMs/totalBytes`,
    });
  }
  if (usable.length === 0) return { status: 'fail', problems, counts: null };

  const ordered = [...usable].sort((left, right) => left.elapsedMs - right.elapsedMs);
  let peak = ordered[0];
  for (const sample of ordered) if (sample.totalBytes > peak.totalBytes) peak = sample;
  const target = typeof plan.targetBytes === 'number' ? plan.targetBytes : PLAN_SOAK.targetBytes;
  if (peak.totalBytes > target) {
    problems.push({
      code: 'GATE_SOAK_TARGET_EXCEEDED',
      detail: `the sample at t=${Math.round(peak.elapsedMs / 1000)} s reached ${peak.totalBytes} bytes, over the ${target} byte target`,
    });
  }

  // The reference sample is the one nearest t=40 min, within one sample interval: the plan compares
  // every later sample against it, so a run that never sampled near t=40 min cannot answer.
  const instant = plan.trendFromMs;
  let reference = null;
  for (const sample of ordered) {
    const distance = Math.abs(sample.elapsedMs - instant);
    if (distance > plan.intervalMs) continue;
    if (!reference || distance < Math.abs(reference.elapsedMs - instant)) reference = sample;
  }
  if (!reference) {
    problems.push({
      code: 'GATE_SOAK_NO_REFERENCE_SAMPLE',
      detail: `no sample within ${plan.intervalMs} ms of t=${Math.round(instant / 60000)} min, which the trend compares against`,
    });
    return { status: 'fail', problems, counts: { samples: ordered.length, peakTotalBytes: peak.totalBytes } };
  }
  const trend = ordered.filter((sample) => sample.elapsedMs >= plan.trendFromMs);
  const tolerance = plan.tolerance;
  const breaching = trend.filter(
    (sample) => Math.abs(sample.totalBytes - reference.totalBytes) > reference.totalBytes * tolerance,
  );
  for (const sample of breaching) {
    const deltaPct = ((sample.totalBytes - reference.totalBytes) / reference.totalBytes) * 100;
    problems.push({
      code: 'GATE_SOAK_TREND',
      detail:
        `the sample at t=${Math.round(sample.elapsedMs / 1000)} s is ${deltaPct.toFixed(2)}% from the t=40 min ` +
        `reference (${reference.totalBytes} bytes), outside +/-${(tolerance * 100).toFixed(0)}%`,
    });
  }
  return {
    status: problems.length === 0 ? 'pass' : 'fail',
    problems,
    counts: {
      samples: ordered.length,
      trendSamples: trend.length,
      referenceBytes: reference.totalBytes,
      peakTotalBytes: peak.totalBytes,
      breaches: breaching.length,
    },
  };
}

/**
 * Records Phase 1 Test B as an ADVISORY finding (rev 5.1, 2026-11-20).
 *
 * This is the only entry point the gate uses for the soak. It applies both existing predicates - the
 * summary judgement and the trend arithmetic over the raw samples, which must agree - and returns a
 * report entry, never a step and never a problem list the verdict is built from. Callers pass the
 * result to `evaluateGate({ advisories })`, which copies it into the report without merging it into
 * `problems`, so a failing soak cannot reject a candidate and a passing one cannot rescue one.
 *
 * `summary` and `samples` are already-parsed evidence. Absent evidence is a FAILED advisory, not a
 * silent pass: if an operator points `--soak-evidence` at a directory, the gate says what it found.
 */
export function evaluateSoakAdvisory({ summary, samples, plan = PLAN_SOAK }) {
  const summaryVerdict = evaluateSoakEvidence({ exitCode: summary?.exitCode ?? null, summary, plan });
  const sampleVerdict = evaluateSoakSamples({ samples, plan });
  return {
    id: 'phase1-footprint-soak',
    phase: 1,
    status: summaryVerdict.status === 'pass' && sampleVerdict.status === 'pass' ? 'pass' : 'fail',
    problems: [...summaryVerdict.problems, ...sampleVerdict.problems],
    counts: { ...summaryVerdict.counts, ...sampleVerdict.counts },
    detail: null,
  };
}

/**
 * Phase 0 as observed by `dsym-symbolication-acceptance.mjs`: a forced SIGSEGV of the release binary,
 * a named frame through the archived dSYM, a negative run refused as DSYM_UUID_MISMATCH naming
 * nothing, and the accepted binary equal to the packaged one by bytes.
 *
 * This duplicates nothing: `evaluateSymbolicationEvidence` (the publish authority) is also run by the
 * CLI, and its result must agree. Agreement is checked there; this function is the same predicates
 * applied to an already-read report so the rule is testable without a crash.
 */
export function evaluatePhase0Evidence({ exitCode, report }) {
  const problems = [];
  if (exitCode !== 0) problems.push({ code: 'GATE_PHASE0_EXIT', detail: `the symbolication harness exited ${exitCode}` });
  if (!report || typeof report !== 'object') {
    problems.push({ code: 'GATE_PHASE0_REPORT_MISSING', detail: 'no symbolication report.json was read' });
    return { status: 'fail', problems, counts: null };
  }
  if (report.schema !== SYMBOLICATION_SCHEMA) {
    problems.push({
      code: 'GATE_PHASE0_SCHEMA',
      detail: `symbolication schema is ${JSON.stringify(report.schema ?? null)}, expected ${SYMBOLICATION_SCHEMA}`,
    });
  }
  if (report.verdict !== 'PASS') {
    problems.push({ code: 'GATE_PHASE0_VERDICT', detail: `verdict is ${JSON.stringify(report.verdict ?? null)}` });
  }
  const forced = report.forcedCrash && typeof report.forcedCrash === 'object' ? report.forcedCrash : null;
  if (!forced) {
    problems.push({
      code: 'GATE_PHASE0_NOT_FORCED',
      detail: 'the report records no forced crash, so its crash report was not produced by this run',
    });
  } else {
    if (forced.signal !== 'SIGSEGV') {
      problems.push({ code: 'GATE_PHASE0_SIGNAL', detail: `forced signal is ${JSON.stringify(forced.signal ?? null)}` });
    }
    if (!Number.isInteger(forced.pid) || forced.pid <= 0) {
      problems.push({ code: 'GATE_PHASE0_PID', detail: 'the forced crash names no pid' });
    }
  }
  const namedFrames = report.symbolicated?.namedFrames;
  if (!Number.isInteger(namedFrames) || namedFrames <= 0) {
    problems.push({
      code: 'GATE_PHASE0_NO_NAMED_FRAME',
      detail: `namedFrames is ${JSON.stringify(namedFrames ?? null)}; the archived dSYM named no frame`,
    });
  }
  if (report.negative?.gateCode !== 'DSYM_UUID_MISMATCH') {
    problems.push({
      code: 'GATE_PHASE0_NEGATIVE_NOT_REFUSED',
      detail: `the negative run reported ${JSON.stringify(report.negative?.gateCode ?? null)}, expected DSYM_UUID_MISMATCH`,
    });
  }
  if (report.negative?.namedFrames !== 0) {
    problems.push({
      code: 'GATE_PHASE0_NEGATIVE_NAMED',
      detail: `the negative run named ${JSON.stringify(report.negative?.namedFrames ?? null)} frame(s)`,
    });
  }
  // The plan asks for a *named function*, not merely a nonzero count: a symbolicated frame with no
  // name is an offset, and an offset is what a missing dSYM produces. `binaryFrames` is the count of
  // frames attributed to this binary, so requiring both makes a foreign or partial symbolication fail.
  if (Number.isInteger(report.symbolicated?.binaryFrames) && report.symbolicated.binaryFrames <= 0) {
    problems.push({
      code: 'GATE_PHASE0_NO_BINARY_FRAME',
      detail: 'no symbolicated frame was attributed to the crashed binary',
    });
  }
  // The harness's own named checks are the record of WHAT it proved (forced-crash provenance, binary
  // identity before/after the crash, packaged-binary identity). A report that omits them, or records
  // one as failed, has not established the property however good its verdict string looks.
  const checks = report.checks && typeof report.checks === 'object' ? report.checks : {};
  if (Object.keys(checks).length === 0) {
    problems.push({ code: 'GATE_PHASE0_CHECKS_MISSING', detail: 'the report records no named checks, so no property was proven' });
  }
  for (const [name, check] of Object.entries(checks)) {
    if (check?.ok !== true) {
      problems.push({ code: 'GATE_PHASE0_CHECK_FAILED', detail: `${name}: ${check?.detail ?? 'not ok'}` });
    }
  }
  // Provenance: the digest must have been captured BEFORE the crash and still match after it.
  // Otherwise the "release binary crashed" claim rests on a file read at some unknown time.
  if (report.binary?.capturedBeforeCrash !== true) {
    problems.push({ code: 'GATE_PHASE0_DIGEST_NOT_PRECRASH', detail: 'the binary digest was not captured before the forced crash' });
  }
  if (report.binary?.verifiedUnchangedAfterCrash !== true) {
    problems.push({ code: 'GATE_PHASE0_BINARY_CHANGED', detail: 'the binary was not verified unchanged after the crash' });
  }
  const accepted = typeof report.binary?.sha256 === 'string' ? report.binary.sha256 : null;
  const packaged = typeof report.packagedBinary?.sha256 === 'string' ? report.packagedBinary.sha256 : null;
  if (!accepted) {
    problems.push({ code: 'GATE_PHASE0_BINARY_UNHASHED', detail: 'the accepted binary records no sha256' });
  } else if (packaged !== accepted) {
    problems.push({
      code: 'GATE_PHASE0_BINARY_NOT_SHIPPED',
      detail: `packaged binary hashes to ${JSON.stringify(packaged)}, the accepted binary to ${accepted}`,
    });
  }
  // A crash report that came from anywhere but this run's isolated root is not this crash's evidence.
  if (report.collector && report.collector.daemonLogFromCrashedRoot !== true) {
    problems.push({
      code: 'GATE_PHASE0_FOREIGN_DIAGNOSTICS',
      detail: 'the collected daemon log did not come from the crashed process root',
    });
  }
  return {
    status: problems.length === 0 ? 'pass' : 'fail',
    problems,
    counts: { namedFrames: namedFrames ?? null, uuidCount: report.binary?.uuids?.length ?? 0 },
  };
}

/**
 * The Phase 2 half the cargo harness cannot supply: launchd owning the supervisor job.
 *
 * `daemon_crash_supervisor_acceptance` starts children directly, so it proves restart policy and
 * nothing about who owns the supervisor. A source audit (st_01a1e627 F3) found no launchd chain
 * exercised anywhere in the tree. That evidence therefore comes from a Darwin host that ran the
 * job, and this function is deliberately hostile to a hand-written summary of one:
 *
 *  - every raw artifact the chain claims is recomputed here. `chain.raw[]` records `{name, sha256}`
 *    and the matching text must hash to exactly that, so editing `launchctl-print.txt` after the
 *    fact is detected rather than believed.
 *  - the job must be the QA label, never `com.ferryx.daemon` / `com.rorca.daemon`: a gate run that
 *    installed over the production job would be evidence of the wrong thing, produced destructively.
 *  - the supervisor's parent must be launchd (pid 1), because that single number is the whole claim
 *    the plan makes ("supervisor ownership belongs to the launchd job").
 *  - the binaries and revision must be the frozen candidate's, so evidence from another build cannot
 *    be attached to this one.
 *
 * Nothing here runs `launchctl`; the evaluator only judges bytes it is given, which is why the raw
 * capture requirement is not a nicety.
 */
export function evaluateLaunchdChainEvidence({ evidence, candidate }) {
  const problems = [];
  const chain = evidence?.chain ?? null;
  const raw = evidence?.raw ?? {};
  if (!chain || typeof chain !== 'object') {
    problems.push({ code: 'GATE_LAUNCHD_EVIDENCE_MISSING', detail: 'no launchd chain evidence was supplied' });
    return { status: 'fail', problems, counts: null };
  }
  if (chain.schema !== LAUNCHD_CHAIN_SCHEMA) {
    problems.push({
      code: 'GATE_LAUNCHD_SCHEMA',
      detail: `chain schema is ${JSON.stringify(chain.schema ?? null)}, expected ${LAUNCHD_CHAIN_SCHEMA}`,
    });
  }
  if (chain.platform !== 'darwin') {
    problems.push({
      code: 'GATE_LAUNCHD_NOT_DARWIN',
      detail: `the chain was captured on platform ${JSON.stringify(chain.platform ?? null)}; launchd exists only on Darwin`,
    });
  }

  // Raw artifacts: presence, then digest. A summary that names a capture it did not ship is refused.
  const REQUIRED_RAW = ['launchctl-print.txt', 'job.plist', 'restart-log.txt'];
  const recorded = new Map((Array.isArray(chain.raw) ? chain.raw : []).map((entry) => [entry?.name, entry]));
  for (const name of REQUIRED_RAW) {
    if (!recorded.has(name)) {
      problems.push({ code: 'GATE_LAUNCHD_RAW_UNRECORDED', detail: `the chain records no ${name}` });
      continue;
    }
    const text = raw[name];
    if (typeof text !== 'string' || text === '') {
      problems.push({ code: 'GATE_LAUNCHD_RAW_MISSING', detail: `${name} is recorded but was not supplied` });
      continue;
    }
    const actual = sha256Hex(Buffer.from(text, 'utf8'));
    if (recorded.get(name).sha256 !== actual) {
      problems.push({
        code: 'GATE_LAUNCHD_RAW_TAMPERED',
        detail: `${name} hashes to ${actual}, the chain records ${JSON.stringify(recorded.get(name).sha256 ?? null)}`,
      });
    }
  }

  const label = typeof chain.label === 'string' ? chain.label : '';
  if (!label) {
    problems.push({ code: 'GATE_LAUNCHD_LABEL_MISSING', detail: 'the chain names no job label' });
  } else if (label === 'com.ferryx.daemon' || label === 'com.rorca.daemon' || label.startsWith('com.ferryx.daemon.')) {
    problems.push({
      code: 'GATE_LAUNCHD_PRODUCTION_LABEL',
      detail: `the chain was captured from the production job ${label}; QA evidence must come from an isolated label`,
    });
  } else if (!label.startsWith('com.ferryx.qa')) {
    problems.push({
      code: 'GATE_LAUNCHD_LABEL_NOT_ISOLATED',
      detail: `the job label ${label} is neither the production job nor a com.ferryx.qa label`,
    });
  }

  // launchd owns the supervisor: this is the assertion the cargo harness cannot make.
  if (chain.supervisorPpid !== 1) {
    problems.push({
      code: 'GATE_LAUNCHD_NOT_OWNED',
      detail: `the supervisor's parent pid is ${JSON.stringify(chain.supervisorPpid ?? null)}; launchd is pid 1, so launchd does not own it`,
    });
  }
  const plist = typeof raw['job.plist'] === 'string' ? raw['job.plist'] : '';
  if (plist && !plist.includes('--daemon-crash-supervisor')) {
    problems.push({
      code: 'GATE_LAUNCHD_PLIST_NOT_SUPERVISOR',
      detail: 'the captured plist does not run --daemon-crash-supervisor, so the job starts a daemon directly',
    });
  }
  const printed = typeof raw['launchctl-print.txt'] === 'string' ? raw['launchctl-print.txt'] : '';
  if (printed && !/state\s*=\s*running/.test(printed)) {
    problems.push({ code: 'GATE_LAUNCHD_NOT_RUNNING', detail: 'the captured launchctl print does not show state = running' });
  }

  // Restart accounting is what the plan's Phase 2 acceptance names, so it is checked by count.
  if (chain.killSignal !== 'SIGKILL') {
    problems.push({
      code: 'GATE_LAUNCHD_KILL_NOT_ABNORMAL',
      detail: `the injected death was ${JSON.stringify(chain.killSignal ?? null)}, not SIGKILL`,
    });
  }
  if (chain.restartsAfterAbnormal !== 1) {
    problems.push({
      code: 'GATE_LAUNCHD_RESTART_COUNT',
      detail: `an abnormal exit restarted ${JSON.stringify(chain.restartsAfterAbnormal ?? null)} time(s), the plan requires exactly once`,
    });
  }
  if (chain.restartsAfterExitZero !== 0) {
    problems.push({
      code: 'GATE_LAUNCHD_EXIT_ZERO_RESTARTED',
      detail: `exit 0 restarted ${JSON.stringify(chain.restartsAfterExitZero ?? null)} time(s); an intentional exit must not restart`,
    });
  }
  if (chain.daemonPidBefore === chain.daemonPidAfter) {
    problems.push({
      code: 'GATE_LAUNCHD_DAEMON_UNCHANGED',
      detail: 'the daemon pid did not change across the crash, so no restart was observed',
    });
  }

  // The evidence must describe the frozen candidate, not whatever binary that host happened to have.
  const daemonSha = candidate?.binaries?.daemon?.sha256 ?? null;
  if (!daemonSha || chain.daemonSha256 !== daemonSha) {
    problems.push({
      code: 'GATE_LAUNCHD_BINARY_DRIFT',
      detail: `the chain ran a daemon hashing to ${JSON.stringify(chain.daemonSha256 ?? null)}, the candidate is ${JSON.stringify(daemonSha)}`,
    });
  }
  if (candidate?.commitSha && chain.head !== candidate.commitSha) {
    problems.push({
      code: 'GATE_LAUNCHD_REVISION_DRIFT',
      detail: `the chain was captured at ${JSON.stringify(chain.head ?? null)}, the candidate is ${candidate.commitSha}`,
    });
  }

  return {
    status: problems.length === 0 ? 'pass' : 'fail',
    problems,
    counts: {
      restartsAfterAbnormal: chain.restartsAfterAbnormal ?? null,
      restartsAfterExitZero: chain.restartsAfterExitZero ?? null,
    },
  };
}

/** Where the keeper sits inside `Ferryx.app`, as the seal names it. */
const KEEPER_SEAL_PATH = 'Resources/keeper/ferryx-ptykeeper';

/**
 * The `<dict>` body of one top-level CodeResources section, or null when the section is absent.
 *
 * A single pattern over the whole document is wrong in both directions. `codesign` writes each digest
 * on its own indented line inside `<data>`, so a `([A-Za-z0-9+/=]+)` capture between `<data>` and
 * `</data>` matches nothing at all: measured against a real sealed bundle that pattern yields 0
 * entries, against 1467 once whitespace inside the element is allowed. And the sections are not
 * interchangeable - `files2` nests `<key>hash2</key><data>` inside each entry's `<dict>`, while
 * `rules`/`rules2` hold regex patterns such as `^Resources/` that a document-wide scan would record
 * as though they were sealed paths. Each section is bounded by the next top-level key, and `codesign`
 * writes `files`, `files2`, `rules` and `rules2` exactly once each.
 */
function codeResourcesSection(text, name) {
  const marker = `<key>${name}</key>`;
  const start = text.indexOf(marker);
  if (start === -1) return null;
  const from = start + marker.length;
  let end = text.length;
  for (const other of ['files', 'files2', 'rules', 'rules2']) {
    if (other === name) continue;
    const at = text.indexOf(`<key>${other}</key>`, from);
    if (at !== -1 && at < end) end = at;
  }
  const body = text.slice(from, end);
  const open = body.indexOf('<dict>');
  return open === -1 ? null : body.slice(open);
}

/**
 * Every sealed path the document names, keyed by path with the digests recorded for it.
 *
 * `files` holds a bare `<data>` (SHA-1). `files2` holds `<dict><key>hash2</key><data>` (SHA-256), and
 * for nested code a `cdhash`/`requirement` pair instead of a file hash. Both are read so the keeper
 * can be checked against the SHA-256 `files2` records, which is the strong digest modern bundles
 * carry; a `files`-only SHA-1 is used when no `hash2` exists. Whitespace is stripped from every
 * captured digest because codesign wraps base64 across lines.
 */
function readSealedEntries(text) {
  const sealed = new Map();
  const remember = (path, kind, raw) => {
    const digest = raw.replace(/\s+/g, '');
    if (!digest) return;
    const entry = sealed.get(path) ?? {};
    entry[kind] = digest;
    sealed.set(path, entry);
  };
  const files = codeResourcesSection(text, 'files');
  if (files) {
    const pair = /<key>([^<]+)<\/key>\s*<data>([\s\S]*?)<\/data>/g;
    for (let match = pair.exec(files); match; match = pair.exec(files)) remember(match[1], 'sha1', match[2]);
  }
  const files2 = codeResourcesSection(text, 'files2');
  if (files2) {
    const entry = /<key>([^<]+)<\/key>\s*<dict>([\s\S]*?)<\/dict>/g;
    for (let match = entry.exec(files2); match; match = entry.exec(files2)) {
      const hash2 = /<key>hash2<\/key>\s*<data>([\s\S]*?)<\/data>/.exec(match[2]);
      if (hash2) {
        remember(match[1], 'sha256', hash2[1]);
        continue;
      }
      const hash = /<key>hash<\/key>\s*<data>([\s\S]*?)<\/data>/.exec(match[2]);
      if (hash) remember(match[1], 'sha1', hash[1]);
    }
  }
  return sealed;
}

/**
 * The code signature as the shipped archive records it.
 *
 * The hazard this closes is a signing-order one, reported by the packaging audit: `refreshKeeperDigest`
 * runs before `finalizeMacosBundle` re-signs the keeper, and re-signing appends a signature blob, so
 * `keeper.json` can describe bytes that no longer exist while the bundle still seals. `evaluateKeeperPayload`
 * catches that via the digest; this catches the other half - a keeper that is not in the seal at all,
 * which a digest match alone would not notice.
 *
 * The input is the archive's own `_CodeSignature/CodeResources`, never a summary of it: a JSON claim
 * that "the bundle is sealed" is exactly the synthetic evidence this gate refuses to accept. When the
 * resource is absent the step fails rather than being skipped, because an unsealed bundle cannot be
 * notarized and therefore cannot ship.
 */
export function evaluateBundleSeal({ codeResourcesText, keeperBytes }) {
  const problems = [];
  if (typeof codeResourcesText !== 'string' || codeResourcesText.trim() === '') {
    problems.push({
      code: 'GATE_SEAL_MISSING',
      detail: 'the packaged app carries no Contents/_CodeSignature/CodeResources, so the bundle is not sealed',
    });
    return { status: 'fail', problems, observed: null };
  }
  const sealed = readSealedEntries(codeResourcesText);

  const digestOf = (bytes) => ({
    sha256: createHash('sha256').update(bytes).digest('base64'),
    sha1: createHash('sha1').update(bytes).digest('base64'),
  });
  const check = (label, resourcePath, bytes) => {
    const recorded = sealed.get(resourcePath) ?? null;
    if (!recorded || (!recorded.sha256 && !recorded.sha1)) {
      problems.push({
        code: 'GATE_SEAL_NOT_LISTED',
        detail:
          `${label} (${resourcePath}) has no file digest in CodeResources; the sealed bundle records neither ` +
          'a SHA-1 nor a SHA-256 for it, so the shipped bytes are not covered',
      });
      return;
    }
    const digests = digestOf(bytes);
    // `files2`' SHA-256 is the strong digest, so it decides whenever it is present.
    const kind = recorded.sha256 ? 'sha256' : 'sha1';
    if (recorded[kind] === digests[kind]) return;
    problems.push({
      code: 'GATE_SEAL_DIGEST_UNMATCHED',
      detail:
        `${label} hashes to ${kind}:${digests[kind]} but CodeResources records ${recorded[kind]} for it; ` +
        'the shipped bytes are not the bytes that were sealed',
    });
  };

  // Only the KEEPER is checked here. Apple records the main executable's hash in the CodeDirectory,
  // not in `_CodeSignature/CodeResources`: in every real bundle this Mac holds (2026.1025.4,
  // 2026.1026.1, 2026.1027.6) `MacOS/ferryx` is absent from the resource seal, which lists only
  // `MacOS/ferryx-account`, `-cli` and `-relay` because those are nested code, not the bundle's own
  // executable. Requiring it made this check impossible to satisfy on an honest build. The daemon
  // bytes are not left unverified by dropping it: `buildCandidate` requires `--daemon-binary` to
  // hash exactly to the packaged `Ferryx.app/Contents/MacOS/ferryx` member, so the shipped daemon is
  // already bound to the candidate identity by digest.
  if (keeperBytes && keeperBytes.length > 0) check('the keeper', KEEPER_SEAL_PATH, keeperBytes);
  else problems.push({ code: 'GATE_SEAL_KEEPER_ABSENT', detail: 'no keeper bytes were available to check against the seal' });

  return {
    status: problems.length === 0 ? 'pass' : 'fail',
    problems,
    observed: { sealedEntries: sealed.size, keeperDigest: sealed.get(KEEPER_SEAL_PATH) ?? null },
  };
}

/**
 * The keeper payload as it ships: `keeper.json` beside `ferryx-ptykeeper`, and its digest over the
 * exact bytes in the archive.
 *
 * This is checked on the packaged artifact because that is the only place a stale digest is visible.
 * `refreshKeeperDigest` runs before `finalizeMacosBundle`, so a keeper binary that is re-signed inside
 * the bundle would leave `keeper.json` describing bytes that no longer exist and the daemon's
 * `payload_digest` check would refuse the shipped keeper. Reported to the packaging owner
 * (st_01a1e60b); this gate only refuses to pass such a build.
 */
export function evaluateKeeperPayload({ manifest, keeperBytes }) {
  const problems = [];
  if (!manifest || typeof manifest !== 'object') {
    problems.push({ code: 'GATE_KEEPER_MANIFEST_MISSING', detail: `no ${KEEPER_MANIFEST_NAME} in the packaged bundle` });
    return { status: 'fail', problems };
  }
  if (manifest.schema !== KEEPER_MANIFEST_SCHEMA) {
    problems.push({
      code: 'GATE_KEEPER_SCHEMA',
      detail: `keeper manifest schema is ${JSON.stringify(manifest.schema ?? null)}, expected ${KEEPER_MANIFEST_SCHEMA}`,
    });
  }
  if (typeof manifest.version !== 'string' || manifest.version.length === 0) {
    problems.push({ code: 'GATE_KEEPER_VERSION', detail: 'the keeper manifest names no generation version' });
  }
  if (!keeperBytes || keeperBytes.length === 0) {
    problems.push({ code: 'GATE_KEEPER_BINARY_MISSING', detail: `no ${KEEPER_BINARY_NAME} in the packaged bundle` });
    return { status: 'fail', problems };
  }
  const actual = sha256Hex(keeperBytes);
  if (manifest.sha256 !== actual) {
    problems.push({
      code: 'GATE_KEEPER_DIGEST_MISMATCH',
      detail:
        `${KEEPER_MANIFEST_NAME} records sha256 ${JSON.stringify(manifest.sha256 ?? null)} but the packaged ` +
        `${KEEPER_BINARY_NAME} hashes to ${actual}; the shipped keeper would fail payload verification`,
    });
  }
  if (manifest.bytes !== keeperBytes.length) {
    problems.push({
      code: 'GATE_KEEPER_BYTES_MISMATCH',
      detail: `keeper.json records ${JSON.stringify(manifest.bytes ?? null)} bytes, the packaged binary is ${keeperBytes.length}`,
    });
  }
  return { status: problems.length === 0 ? 'pass' : 'fail', problems, observed: { sha256: actual, bytes: keeperBytes.length } };
}

/**
 * The rollback floor as two shipped documents state it: the bundle's `keeper/latest.json` and the
 * published updater manifest. Plan Phase 3 requires the oldest safe rollback version in the release
 * manifest, and a bundle that states one floor while the release page promises another would make
 * `daemon::rollback` compare against a promise nobody published.
 *
 * `release-local.mjs` and `scripts/lib/keeper-rollback-manifest.mjs` each export a function of this
 * name with different inputs; neither is the authority this check needs (one is test-only, the other
 * is publish-time and reads the ledger). So this gate reads the two shipped documents itself and asks
 * the packaging owner to converge the two helpers.
 */
export function evaluateRollbackFloor({ bundleManifest, publishedManifest }) {
  const problems = [];
  const bundleFloor = bundleManifest?.[KEEPER_MANIFEST_KEY]?.[OLDEST_SAFE_ROLLBACK_KEY];
  const publishedFloor = publishedManifest?.[KEEPER_MANIFEST_KEY]?.[OLDEST_SAFE_ROLLBACK_KEY];
  if (typeof bundleFloor !== 'string' || bundleFloor.length === 0) {
    problems.push({
      code: 'GATE_FLOOR_BUNDLE_MISSING',
      detail: `the packaged keeper manifest states no ${OLDEST_SAFE_ROLLBACK_KEY}`,
    });
  }
  if (typeof publishedFloor !== 'string' || publishedFloor.length === 0) {
    problems.push({
      code: 'GATE_FLOOR_PUBLISHED_MISSING',
      detail: `the published latest.json states no keeper.${OLDEST_SAFE_ROLLBACK_KEY}`,
    });
  }
  if (!problems.length && bundleFloor !== publishedFloor) {
    problems.push({
      code: 'GATE_FLOOR_MISMATCH',
      detail: `the bundle states ${JSON.stringify(bundleFloor)} but the release publishes ${JSON.stringify(publishedFloor)}`,
    });
  }
  return { status: problems.length === 0 ? 'pass' : 'fail', problems, observed: { bundleFloor, publishedFloor } };
}

/**
 * The one aggregation rule. `steps` are results in execution order; `requiredIds` is what the release
 * needs. Anything short of every required step in state `pass` is a rejection, and a step whose
 * evidence is absent is `missing`, which is never a skip.
 *
 * `advisories` are findings the report must SHOW but must never judge by. They are copied into the
 * result under `advisory`, deliberately not merged into `problems`, so an advisory finding - the
 * Phase 1 Test B soak since rev 5.1 - cannot move the verdict in either direction. Keeping the copy
 * here rather than at the report writer is what makes "advisory cannot gate" testable.
 */
export function evaluateGate({ steps, requiredIds, advisories = [] }) {
  const problems = [];
  const byId = new Map(steps.map((step) => [step.id, step]));
  for (const id of requiredIds) {
    const step = byId.get(id);
    if (!step) {
      problems.push({ code: 'GATE_STEP_ABSENT', detail: `${id} was not run and has no recorded evidence` });
      continue;
    }
    if (step.status === 'pass') continue;
    problems.push({
      code: step.status === 'missing' ? 'GATE_STEP_EVIDENCE_MISSING' : 'GATE_STEP_NOT_PASSED',
      detail: `${id} is ${step.status}${step.problems?.length ? `: ${step.problems.map((p) => p.code).join(', ')}` : ''}`,
    });
  }
  const verdict = problems.length === 0 ? 'accepted' : 'rejected';
  const advisory = advisories.map((entry) => ({
    id: entry.id,
    phase: entry.phase ?? null,
    status: entry.status,
    problems: entry.problems ?? [],
    counts: entry.counts ?? null,
    detail: entry.detail ?? null,
  }));
  return { verdict, problems, advisory };
}

/**
 * Steps whose evidence is a file this gate reads, keyed by the step that produced it.
 *
 * `phase1-footprint-soak` is deliberately absent: since rev 5.1 (2026-11-20) Phase 1 Test B is
 * ADVISORY. Requiring it refused releases for a property a 60 minute run cannot measure anyway (the
 * retained-history ceiling needs ~14.5 h at default settings to fill, and Test A proves the ceiling
 * and its eviction deterministically). It is still planned, still judged when `--soak-evidence` names
 * it, and still recorded in the report - just never able to reject the candidate.
 */
export function requiredStepIds() {
  return [
    'phase0-symbolication',
    'phase1-budget-pressure',
    'phase2-supervisor',
    'phase2-launchd-chain',
    'phase3-crash-survival',
    'phase4-handover-failure',
    'risk-p99-echo',
    'aux-host-agnostic-tests',
    'packaging-shipped-payload',
  ];
}

/** Step ids that are planned and reported but never gated. `--only` on one cannot reject a run. */
export const ADVISORY_STEP_IDS = Object.freeze(['phase1-footprint-soak']);

/** Human-readable report; every line a reader needs to see why the verdict is what it is.
 *
 * The advisory section exists so a finding the plan chose not to gate is still READ by whoever audits
 * the run. It is printed under a heading that says it cannot change the verdict, because an
 * unreported advisory is indistinguishable from no measurement at all.
 */
export function renderGateReport(report) {
  const lines = [];
  lines.push(`# Combined release verification gate - ${report.verdict.toUpperCase()}`);
  lines.push('');
  lines.push(`Candidate commit: ${report.candidate.commitSha}`);
  lines.push(`Candidate digest: ${report.candidateDigest}`);
  if (report.candidate.planDigest) lines.push(`Plan digest: ${report.candidate.planDigest}`);
  if (report.candidate.artifact?.sha256) lines.push(`Packaged artifact: ${report.candidate.artifact.sha256}`);
  lines.push('');
  lines.push('| phase | step | status | exit | detail |');
  lines.push('| --- | --- | --- | --- | --- |');
  for (const step of report.steps) {
    lines.push(
      `| ${step.phase} | ${step.id} | ${step.status.toUpperCase()} | ${step.exitCode ?? 'n/a'} | ${(step.problems ?? [])
        .map((problem) => problem.code)
        .join(', ') || '-'} |`,
    );
  }
  lines.push('');
  for (const step of report.steps) {
    if (step.status === 'pass') continue;
    lines.push(`## ${step.id} (${step.status})`);
    for (const problem of step.problems ?? []) lines.push(`- ${problem.code}: ${problem.detail}`);
    if (step.logs?.stdout) lines.push(`- stdout: ${step.logs.stdout}`);
    if (step.logs?.stderr) lines.push(`- stderr: ${step.logs.stderr}`);
    lines.push('');
  }
  if (report.gaps.length > 0) {
    lines.push('## Recorded gaps (not waivers)');
    for (const gapId of report.gaps) lines.push(`- ${gapId.code}: ${gapId.detail}`);
    lines.push('');
  }
  if (report.advisory?.length > 0) {
    lines.push('## Advisory (recorded, never gating)');
    for (const entry of report.advisory) {
      lines.push(`- ${entry.id}: ${String(entry.status).toUpperCase()}${entry.detail ? ` - ${entry.detail}` : ''}`);
      for (const problem of entry.problems ?? []) lines.push(`  - ${problem.code}: ${problem.detail}`);
    }
    lines.push('');
  }
  lines.push(report.verdict === 'accepted' ? 'ACCEPTED: every phase passed on this candidate.' : 'NOT ACCEPTED: this candidate must not ship.');
  return `${lines.join('\n')}\n`;
}

// ---------------------------------------------------------------------------------------------
// candidate identity
// ---------------------------------------------------------------------------------------------

function hashFile(path) {
  const bytes = readFileSync(path);
  return { path, sha256: sha256Hex(bytes), bytes: bytes.length };
}

function artifactMember(artifactPath, memberPath) {
  if (!artifactPath || !existsSync(artifactPath)) return null;
  const member = readTarGzMember({ bytes: readFileSync(artifactPath), memberPath });
  return member ? { sha256: member.sha256, bytes: member.bytes.length, memberPath } : null;
}

/**
 * Builds the candidate identity and refuses contradictions before anything runs.
 *
 * The shipped-bytes rule: when the packaged artifact is present, the daemon/keeper/CLI binaries the
 * harnesses will use must hash to the members inside it. A `target/release` binary is the same source
 * but not the same bytes as the stripped, signed artifact, so accepting it would let a release be
 * gated on something that does not ship.
 */
export function buildCandidate({ runDir, commitSha, daemonBinary, keeperBinary, cliBinary, snapshot = null }) {
  const problems = [];
  let plan = null;
  let planDigest = null;
  if (runDir) {
    const planPath = join(runDir, 'plan.json');
    if (!existsSync(planPath)) {
      problems.push({ code: 'GATE_PLAN_MISSING', detail: `no plan.json under ${runDir}` });
    } else {
      const bytes = readFileSync(planPath);
      planDigest = sha256Hex(bytes);
      try {
        plan = JSON.parse(bytes.toString('utf8'));
      } catch (error) {
        problems.push({ code: 'GATE_PLAN_UNREADABLE', detail: `${planPath} is not JSON: ${error.message}` });
      }
    }
    const statePath = join(runDir, 'prepare-state.json');
    if (existsSync(statePath)) {
      try {
        const state = JSON.parse(readFileSync(statePath, 'utf8'));
        if (state.planDigest && planDigest && state.planDigest !== planDigest) {
          problems.push({
            code: 'GATE_PLAN_TAMPERED',
            detail: `plan.json hashes to ${planDigest} but prepare-state records ${state.planDigest}`,
          });
        }
      } catch {
        problems.push({ code: 'GATE_PLAN_STATE_UNREADABLE', detail: `${statePath} is not JSON` });
      }
    }
  }
  const resolvedCommit = commitSha ?? plan?.commitSha ?? null;
  if (!resolvedCommit || !/^[0-9a-f]{40}$/.test(resolvedCommit)) {
    problems.push({ code: 'GATE_COMMIT_INVALID', detail: `commit is ${JSON.stringify(resolvedCommit)}` });
  }
  if (commitSha && plan?.commitSha && commitSha !== plan.commitSha) {
    problems.push({
      code: 'GATE_COMMIT_MISMATCH',
      detail: `--commit-sha ${commitSha} but plan.json names ${plan.commitSha}`,
    });
  }
  for (const [label, path] of [['daemon', daemonBinary], ['keeper', keeperBinary], ['cli', cliBinary]]) {
    if (!path) {
      problems.push({ code: 'GATE_BINARY_MISSING', detail: `--${label}-binary was not supplied` });
    } else if (!existsSync(path)) {
      problems.push({ code: 'GATE_BINARY_MISSING', detail: `${label} binary ${path} does not exist` });
    } else if (!statSync(path).isFile()) {
      problems.push({ code: 'GATE_BINARY_NOT_FILE', detail: `${label} binary ${path} is not a regular file` });
    }
  }
  if (problems.length > 0) throw new GateError(problems[0].code, problems[0].detail, problems);

  const binaries = {
    daemon: hashFile(daemonBinary),
    keeper: hashFile(keeperBinary),
    cli: hashFile(cliBinary),
  };

  let artifact = null;
  let packaged = null;
  if (runDir) {
    const artifactPath = join(runDir, PACKAGED_APP_ARTIFACT);
    const ferryx = artifactMember(artifactPath, PACKAGED_APP_MEMBER);
    const keeper = artifactMember(artifactPath, `Ferryx.app/Contents/Resources/${KEEPER_RESOURCE_DIR}/${KEEPER_BINARY_NAME}`);
    const keeperManifest = artifactMember(
      artifactPath,
      `Ferryx.app/Contents/Resources/${KEEPER_RESOURCE_DIR}/${KEEPER_MANIFEST_NAME}`,
    );
    const bundleFloor = artifactMember(
      artifactPath,
      `Ferryx.app/Contents/Resources/${KEEPER_RESOURCE_DIR}/latest.json`,
    );
    if (!ferryx) {
      problems.push({ code: 'GATE_ARTIFACT_MEMBER_MISSING', detail: `${artifactPath} has no ${PACKAGED_APP_MEMBER}` });
    }
    artifact = ferryx ? { sha256: ferryx.sha256, bytes: ferryx.bytes, path: artifactPath } : null;
    packaged = { ferryx, keeper, keeperManifest, bundleFloor };
    if (ferryx && ferryx.sha256 !== binaries.daemon.sha256) {
      problems.push({
        code: 'GATE_DAEMON_NOT_SHIPPED',
        detail:
          `--daemon-binary hashes to ${binaries.daemon.sha256} but the packaged ${PACKAGED_APP_MEMBER} hashes to ` +
          `${ferryx.sha256}; gate the shipped binary, not a build-tree one`,
      });
    }
    if (keeper?.sha256 && keeper.sha256 !== binaries.keeper.sha256) {
      problems.push({
        code: 'GATE_KEEPER_NOT_SHIPPED',
        detail: `--keeper-binary hashes to ${binaries.keeper.sha256} but the packaged keeper hashes to ${keeper.sha256}`,
      });
    }
  }
  if (problems.length > 0) throw new GateError(problems[0].code, problems[0].detail, problems);

  const candidate = {
    commitSha: resolvedCommit,
    // Folded into the identity so every step's recorded digest is a digest *of the frozen tree*, not
    // merely of the binaries present at run time. Two runs of an unchanged snapshot agree; a run
    // against re-staged bytes does not, which is the drift `--verify-only` must be able to see.
    snapshot: snapshot
      ? {
          base: snapshot.base,
          patchSha256: snapshot.patchSha256,
          manifestSha256: snapshot.manifestSha256,
          pathSetSha256: snapshot.pathSetSha256,
          entries: snapshot.entries,
        }
      : null,
    planDigest,
    runId: plan?.runId ?? null,
    binaries,
    artifact,
    packaged: packaged
      ? {
          ferryx: packaged.ferryx?.sha256 ?? null,
          keeper: packaged.keeper?.sha256 ?? null,
          keeperManifest: packaged.keeperManifest?.sha256 ?? null,
          bundleFloor: packaged.bundleFloor?.sha256 ?? null,
        }
      : null,
  };
  return { candidate, candidateDigest: candidateDigestOf(candidate), plan, packaged };
}

// ---------------------------------------------------------------------------------------------
// execution
// ---------------------------------------------------------------------------------------------

/**
 * Runs one step on the picker-claimed build host through `scripts/build-remote.sh`.
 *
 * EVERY executable step goes through here - cargo, the JS acceptance harnesses and the p99 comparison
 * alike. None of them run on this Mac: the task that produced this file forbids builds,
 * tests and acceptance harnesses locally, and `build-remote.sh` is the only sanctioned executor
 * (AGENTS.md: never a bare ssh, because Tailscale SSH sends no exit-status). The wrapper reports the
 * remote status on a marker line, so a remote failure cannot arrive as a local exit 0.
 *
 * The remote command is a small shell program, not a bare harness invocation, and its first act is to
 * print the receipt: the host, the platform it reports about itself, its `HEAD`, and the sha256 of
 * every binary it is about to use. Only then does it `cd` into the worktree and run the real command.
 * That ordering is the point - a receipt printed after the harness could describe anything.
 *
 * `build-remote.sh` prints the picked host on stderr, which is retained with the rest of the output.
 */
function runRemote({ id, outDir, slug, worktree, binaryPaths = {}, command, timeoutMs, onHost = false }) {
  // A remote step's status is only trustworthy when the wrapper echoes the remote marker. The wrapper
  // is being repaired (st_01a1e653) for a missing-marker release race, so the marker is checked here
  // rather than assumed: `exec` would replace the shell and skip `printf`, so the command is run as a
  // plain child and the marker is printed by the same shell that observed its status.
  const MARKER = '__FERRYX_EXIT__';
  // The receipt prints digests of bytes that exist on the host, using a hasher every roster host is
  // known to have (`shasum` on Darwin, `sha256sum` on Linux). A host missing both prints an empty
  // digest, and `evaluateRemoteReceipt` then fails the step rather than trusting it.
  const hasher = 'if command -v shasum >/dev/null 2>&1; then __fx_h() { shasum -a 256 "$1" | cut -d" " -f1; };'
    + ' elif command -v sha256sum >/dev/null 2>&1; then __fx_h() { sha256sum "$1" | cut -d" " -f1; };'
    + ' else __fx_h() { printf ""; }; fi';
  const lines = [
    'set -u',
    hasher,
    `printf '%s host=%s\\n' ${shq(RECEIPT_PREFIX)} "$(hostname 2>/dev/null || uname -n)" >&2`,
    `printf '%s platform=%s\\n' ${shq(RECEIPT_PREFIX)} "$(uname -s | tr '[:upper:]' '[:lower:]')" >&2`,
    `printf '%s arch=%s\\n' ${shq(RECEIPT_PREFIX)} "$(uname -m)" >&2`,
    `printf '%s head=%s\\n' ${shq(RECEIPT_PREFIX)} "$(git -C ${shq(worktree)} rev-parse HEAD 2>/dev/null || printf '')" >&2`,
  ];
  for (const [label, path] of Object.entries(binaryPaths)) {
    lines.push(`printf '%s binary=${label}:%s\\n' ${shq(RECEIPT_PREFIX)} "$(__fx_h ${shq(path)})" >&2`);
  }
  lines.push(`cd ${shq(worktree)}`);
  lines.push(command);
  lines.push('__fx_status=$?');
  lines.push(`printf '${MARKER}%s\\n' "$__fx_status"`);
  lines.push('exit "$__fx_status"');

  // `--on-host` means this process is ALREADY running on the claimed host, inside the one
  // `build-remote.sh` call the orchestrator made. In that mode the program runs directly: calling
  // `build-remote.sh` again would take a SECOND claim while the first is live, which the staging
  // runbook records as the nested-claim hazard - the picker skips the held host and the run either
  // exits 3 or silently lands on a different machine than the one holding the candidate bytes.
  const program = lines.join('\n');
  const run = onHost
    ? runChild({ id, outDir, command: 'sh', args: ['-c', program], timeoutMs })
    : runChild({
        id,
        outDir,
        command: 'bash',
        args: ['scripts/build-remote.sh', slug, '--', 'sh', '-c', program],
        timeoutMs,
      });
  // `build-remote: <host> (<transport>) <- ...` is the wrapper's own line for the host it claimed.
  const claimed = onHost ? null : /^build-remote: (\S+) \((\S+)\) <-/m.exec(run.stderr ?? '');
  // `build-remote.sh` re-emits the remote command's combined output on ITS stdout, so the receipt is
  // read from there; stderr is included too because a transport that splits the streams must not turn
  // a real receipt into a missing one. The roster name the wrapper prints and the box's own `hostname`
  // are deliberately NOT compared: they are different namespaces (`omarchy2` vs `omarchy`), and
  // asserting they match would fail every honest run.
  const receipt = parseRemoteReceipt(`${run.stdout ?? ''}\n${run.stderr ?? ''}`);
  // The remote exit marker is the only trustworthy status on this transport: Tailscale SSH sends no
  // exit-status request, so the wrapper's own exit code is not the command's. A missing marker means
  // the chain broke (the wrapper's release race, a dropped channel, a killed shell), and "no marker"
  // must never be read as success - it is reported as its own finding.
  const markerText = `${run.stdout ?? ''}\n${run.stderr ?? ''}`;
  const markerMatch = new RegExp(`^${MARKER}(\\d+)$`, 'm').exec(markerText);
  const remoteExit = markerMatch ? Number(markerMatch[1]) : null;
  const claim = {
    // In `--on-host` mode there is no wrapper line to read and no claim this process took. That is
    // recorded as `null` rather than invented: the orchestrator's own single invocation owns the
    // claim, and this gate must not claim to have released something it never took.
    host: claimed?.[1] ?? null,
    transport: claimed?.[2] ?? (onHost ? 'in-place' : null),
    // `build-remote.sh` releases its own claim on success and converts it to a wall-clock lease on
    // INT/TERM/HUP. Exit 5 is its "the claim is not ours any more" refusal. Recording the outcome
    // here is what lets the report show whether the host was handed back or is still leased.
    refused: onHost ? false : run.exitCode === 5,
    wrapperExit: run.exitCode,
    released: onHost ? null : run.exitCode === 0 || run.exitCode === 1,
  };
  return {
    ...run,
    remote: {
      host: claim.host ?? receipt.host ?? null,
      transport: claim.transport,
      selfReported: receipt.host,
      exitMarker: remoteExit,
      markerPresent: markerMatch !== null,
      claim,
    },
    receipt,
  };
}

/** POSIX single-quote for embedding one argv word in a remote shell program. */
function shq(value) {
  return `'${String(value).split("'").join(`'\\''`)}'`;
}

function stepDir(outDir, id) {
  const dir = join(outDir, 'steps', id);
  mkdirSync(dir, { recursive: true });
  return dir;
}

/**
 * The environment every child gets.
 *
 * The gate runs acceptance harnesses that create isolated runtimes, and a leaked `FERRYX_*` variable
 * is how one of them points at the production daemon's socket or data directory. So every inherited
 * `FERRYX_*` is dropped here, and the harnesses are expected to set exactly the ones they documented.
 * The build-host claim variables are the only exception: `build-remote.sh` and `pick-build-host.sh`
 * are driven by them, and dropping them would silently change which host a run claims or how long a
 * claim lives.
 */
const PRESERVED_ENV = Object.freeze([
  'PATH',
  'HOME',
  'TMPDIR',
  'SHELL',
  'USER',
  'LOGNAME',
  'LANG',
  'LC_ALL',
  // `build-remote.sh` reaches every host with `ssh -o BatchMode=yes`, which refuses to prompt - so a
  // host that authenticates by key needs the agent socket, and dropping it here would fail the
  // transport with an error that has nothing to do with the candidate. Tailscale-authenticated hosts
  // ignore both; preserving them cannot widen what a harness can reach.
  'SSH_AUTH_SOCK',
  'SSH_AGENT_PID',
  'FERRYX_BUILD_HOSTS_FILE',
  'FERRYX_BUILD_LOCKDIR',
  'FERRYX_BUILD_OWNER_PID',
  'FERRYX_BUILD_CLAIM_TTL',
  'FERRYX_BUILD_CLAIM_GRACE',
]);

export function scrubbedEnv(env = process.env) {
  const kept = {};
  for (const key of PRESERVED_ENV) {
    if (env[key] !== undefined) kept[key] = env[key];
  }
  return kept;
}

/**
 * Runs one child and retains everything: argv, exit status, both streams, timings.
 *
 * `spawnSync` is used so an interrupted gate cannot leave a half-reported step: the verdict is written
 * only after every selected step has returned a status.
 */
function runChild({ id, outDir, command, args, timeoutMs }) {
  const dir = stepDir(outDir, id);
  const startedAt = new Date().toISOString();
  const started = Date.now();
  const result = spawnSync(command, args, {
    cwd: REPO_ROOT,
    env: scrubbedEnv(),
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
    timeout: timeoutMs > 0 ? timeoutMs : undefined,
  });
  const endedAt = new Date().toISOString();
  const stdoutPath = join(dir, 'stdout.log');
  const stderrPath = join(dir, 'stderr.log');
  const stdout = result.stdout ?? '';
  const stderr = result.stderr ?? (result.error ? `${result.error.message}\n` : '');
  writeFileSync(stdoutPath, stdout);
  writeFileSync(stderrPath, stderr);
  writeFileSync(join(dir, 'command.txt'), `${[command, ...args].join(' ')}\n`);
  const exitCode = typeof result.status === 'number' ? result.status : null;
  const record = {
    id,
    command: [command, ...args],
    startedAt,
    endedAt,
    durationMs: Date.now() - started,
    exitCode,
    signal: result.signal ?? null,
    timedOut: result.signal === 'SIGTERM' && result.status === null,
    logs: { stdout: stdoutPath, stderr: stderrPath },
  };
  writeFileSync(join(dir, 'result.json'), `${JSON.stringify(record, null, 2)}\n`);
  return { ...record, stdout, stderr };
}

/** Reads a JSON file, returning `null` rather than throwing so a missing summary is a finding. */
function readJsonIfPresent(path) {
  if (!path || !existsSync(path)) return null;
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    return null;
  }
}

/** Reads a text file verbatim, `null` when absent: raw evidence must be hashed as bytes on disk. */
function readTextIfPresent(path) {
  if (!path || !existsSync(path)) return null;
  try {
    return readFileSync(path, 'utf8');
  } catch {
    return null;
  }
}

/**
 * Reads a JSONL evidence file into an array of objects, keeping unparsable lines as `null`.
 *
 * The `null` entries are deliberate: `evaluateSoakSamples` counts them as malformed samples, so a
 * truncated final line in `footprint-samples.jsonl` becomes a finding instead of a shorter run.
 */
function readJsonLinesIfPresent(path) {
  const text = readTextIfPresent(path);
  if (text === null) return null;
  const rows = [];
  for (const line of text.split('\n')) {
    if (line.trim() === '') continue;
    try {
      rows.push(JSON.parse(line));
    } catch {
      rows.push(null);
    }
  }
  return rows;
}

/** Extracts the text between two marker lines, `null` when either marker is absent or misordered. */
function parseBandedText(text, begin, end) {
  const source = String(text ?? '');
  const start = source.indexOf(begin);
  if (start === -1) return null;
  const from = start + begin.length;
  const stop = source.indexOf(end, from);
  if (stop === -1) return null;
  return source.slice(from, stop).replace(/^\n/, '');
}

/** Extracts and parses the JSON between two marker lines; `null` when absent or unparsable. */
function parseBandedJson(text, begin, end) {
  const body = parseBandedText(text, begin, end);
  if (body === null) return null;
  try {
    return JSON.parse(body);
  } catch {
    return null;
  }
}

/** Extracts the last JSON object in a stream, for harnesses that print a single JSON document. */
export function lastJsonDocument(text) {
  const source = String(text ?? '').trim();
  if (source === '') return null;
  try {
    return JSON.parse(source);
  } catch {
    // Fall through: a harness may precede the document with progress lines.
  }
  const start = source.indexOf('{');
  const end = source.lastIndexOf('}');
  if (start === -1 || end <= start) return null;
  try {
    return JSON.parse(source.slice(start, end + 1));
  } catch {
    return null;
  }
}

function finalizeStep(step) {
  const dir = join(step.outDir, 'steps', step.id);
  mkdirSync(dir, { recursive: true });
  const record = {
    id: step.id,
    phase: step.phase,
    title: step.title,
    status: step.status,
    exitCode: step.exitCode ?? null,
    command: step.command ?? null,
    logs: step.logs ?? null,
    counts: step.counts ?? null,
    problems: step.problems ?? [],
    gaps: step.gaps ?? [],
    candidateDigest: step.candidateDigest ?? null,
    // The receipt and the transport record are persisted, not just used: `--verify-only` re-derives
    // the verdict from these files alone, and `evaluateHostConsistency` compares every step's receipt
    // against every other's. Dropping them here made a re-verification blind to the host identity and
    // to a candidate split between steps - the two things the receipt exists to prove.
    receipt: step.receipt ?? null,
    remote: step.remote ?? null,
    detail: step.detail ?? null,
  };
  writeFileSync(join(dir, 'result.json'), `${JSON.stringify(record, null, 2)}\n`);
  return record;
}

function blockedStep({ id, phase, title, code, detail, command = null }) {
  return {
    id,
    phase,
    title,
    status: 'blocked',
    exitCode: null,
    command,
    logs: null,
    counts: null,
    problems: [{ code, detail }],
  };
}

function readStepRecord(outDir, id) {
  return readJsonIfPresent(join(outDir, 'steps', id, 'result.json'));
}

/** Verifies a previously recorded step, including that it ran on this candidate. */
function verifyRecordedStep({ outDir, id, requiredDigest }) {
  const record = readStepRecord(outDir, id);
  if (!record) {
    return { id, status: 'missing', problems: [{ code: 'GATE_STEP_EVIDENCE_MISSING', detail: `no recorded evidence at ${join(outDir, 'steps', id)}` }] };
  }
  const problems = [...(record.problems ?? [])];
  // A receipt with no candidate digest is unbound evidence: it could belong to any build. The gate
  // always writes a digest, so its absence means the receipt was hand-written or truncated, and
  // neither can be accepted for this candidate.
  if (!record.candidateDigest) {
    problems.push({
      code: 'GATE_RECEIPT_UNBOUND',
      detail: `${id} recorded no candidate digest, so its evidence is not tied to any build`,
    });
  } else if (requiredDigest && record.candidateDigest !== requiredDigest) {
    problems.push({
      code: 'GATE_CANDIDATE_DRIFT',
      detail: `${id} ran under candidate ${record.candidateDigest}, this run is ${requiredDigest}`,
    });
  }
  // A recorded pass is only a pass for THIS candidate: a digest mismatch demotes it to a failure
  // rather than being reported as a pass with a note.
  const recordedStatus = record.status === 'pass' && problems.length > 0 ? 'fail' : record.status;
  return { ...record, status: recordedStatus, problems };
}

// ---------------------------------------------------------------------------------------------
// steps
// ---------------------------------------------------------------------------------------------

function parseArgs(argv, env = process.env) {
  const options = {
    runDir: null,
    commitSha: null,
    daemonBinary: env.FERRYX_DAEMON_BIN ?? null,
    keeperBinary: env.FERRYX_PTY_KEEPER_BIN ?? null,
    cliBinary: env.FERRYX_CLI_BIN ?? null,
    // The remote side. These have no default on purpose: a path guessed here would be a path this
    // gate never verified, and every remote step refuses to run without its own.
    remoteWorktree: env.FERRYX_REMOTE_WORKTREE ?? null,
    remoteRunDir: env.FERRYX_REMOTE_RUN_DIR ?? null,
    remoteDaemonBinary: env.FERRYX_REMOTE_DAEMON_BIN ?? null,
    remoteKeeperBinary: env.FERRYX_REMOTE_KEEPER_BIN ?? null,
    remoteCliBinary: env.FERRYX_REMOTE_CLI_BIN ?? null,
    remoteGenerations: (env.FERRYX_REMOTE_PTY_KEEPER_GENERATIONS ?? '').split(',').map((v) => v.trim()).filter(Boolean),
    remoteDaemonGenerations: (env.FERRYX_REMOTE_DAEMON_GENERATIONS ?? '').split(',').map((v) => v.trim()).filter(Boolean),
    launchdChainEvidence: null,
    // The frozen snapshot: a `stage-dirty-candidate.sh` output directory. Its meta/patch/manifest are
    // what the host is supposed to be running, so the gate binds to them instead of to whatever
    // binaries happen to be on this Mac.
    candidateSnapshot: env.FERRYX_CANDIDATE_SNAPSHOT ?? null,
    // This process is already ON the claimed host, inside the orchestrator's single
    // `build-remote.sh` invocation. Every executable step then runs in place instead of taking a
    // second, nested claim.
    onHost: env.FERRYX_GATE_ON_HOST === '1',
    out: null,
    soakEvidence: null,
    only: [],
    verifyOnly: false,
    dryRun: false,
    json: false,
    list: false,
    timeoutMin: 240,
  };
  const take = (index, flag) => {
    const value = argv[index];
    if (value === undefined || value.startsWith('--')) throw new GateError('GATE_USAGE', `${flag} needs a value`);
    return value;
  };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    switch (flag) {
      case '--run': options.runDir = resolve(take(++index, flag)); break;
      case '--commit-sha': options.commitSha = take(++index, flag); break;
      case '--daemon-binary': options.daemonBinary = take(++index, flag); break;
      case '--keeper-binary': options.keeperBinary = take(++index, flag); break;
      case '--cli-binary': options.cliBinary = take(++index, flag); break;
      case '--remote-worktree': options.remoteWorktree = take(++index, flag); break;
      case '--remote-run-dir': options.remoteRunDir = take(++index, flag); break;
      case '--remote-daemon-binary': options.remoteDaemonBinary = take(++index, flag); break;
      case '--remote-keeper-binary': options.remoteKeeperBinary = take(++index, flag); break;
      case '--remote-cli-binary': options.remoteCliBinary = take(++index, flag); break;
      case '--remote-generations':
        options.remoteGenerations = take(++index, flag).split(',').map((v) => v.trim()).filter(Boolean);
        break;
      case '--remote-daemon-generations':
        options.remoteDaemonGenerations = take(++index, flag).split(',').map((v) => v.trim()).filter(Boolean);
        break;
      case '--launchd-chain-evidence': options.launchdChainEvidence = resolve(take(++index, flag)); break;
      case '--candidate-snapshot': options.candidateSnapshot = resolve(take(++index, flag)); break;
      case '--on-host': options.onHost = true; break;
      case '--out': options.out = resolve(take(++index, flag)); break;
      case '--soak-evidence': options.soakEvidence = resolve(take(++index, flag)); break;
      case '--only': options.only.push(take(++index, flag)); break;
      case '--timeout-min': {
        const value = Number(take(++index, flag));
        if (!Number.isFinite(value) || value <= 0) throw new GateError('GATE_USAGE', '--timeout-min must be positive');
        options.timeoutMin = value;
        break;
      }
      case '--verify-only': options.verifyOnly = true; break;
      case '--dry-run': options.dryRun = true; break;
      case '--json': options.json = true; break;
      case '--list': options.list = true; break;
      case '-h':
      case '--help': options.help = true; break;
      default: throw new GateError('GATE_USAGE', `unknown argument ${JSON.stringify(flag)}`);
    }
  }
  return options;
}

const USAGE = `Usage: node scripts/qa/combined-release-gate.mjs [options]

Runs every phase 0-4 acceptance harness for one release candidate, retains its raw output, and
refuses the candidate unless every required result passed and every result is bound to the same
candidate. Exit 0 is not acceptance: the harness's own JSON summary must report accepted: true with
zero failed and zero blocked cases, and every required case id must appear as pass.

Options:
  --run <dir>                 prepared release run directory (plan.json, artifacts, dsyms)
  --commit-sha <40-hex>       release commit (defaults to plan.json commitSha)
  --daemon-binary <path>      the shipped ferryx (must hash to the packaged artifact member)
  --keeper-binary <path>      the shipped ferryx-ptykeeper
  --cli-binary <path>         the shipped ferryx-cli (drives 'service rollback')
  --remote-worktree <dir>     staged candidate worktree ON the build host (every remote step cd's here)
  --remote-run-dir <dir>      release run directory ON the build host (phase 0 reads its bundle/dsyms)
  --remote-daemon-binary <p>  ferryx path on the build host (re-hashed there, compared with --daemon-binary)
  --remote-keeper-binary <p>  ferryx-ptykeeper path on the build host
  --remote-cli-binary <p>     ferryx-cli path on the build host
  --remote-generations a,b    two keeper builds on the build host
  --remote-daemon-generations a,b  two daemon builds on the build host
  --launchd-chain-evidence <dir>  isolated launchd -> supervisor -> daemon capture from a Darwin host
  --candidate-snapshot <dir>  stage-dirty-candidate.sh output dir; binds the run to one frozen tree
  --on-host                   run already on the claimed host: execute in place, never claim again
  --out <dir>                 evidence root (default: <run>/combined-gate)
  --soak-evidence <dir>       ADVISORY: judge an existing footprint-summary.json and record it; never gates
  --only <step>               run one step; repeatable. Report is INCOMPLETE, never accepted
  --verify-only               read recorded step evidence, run nothing
  --dry-run                   print the plan and exit
  --list                      list step ids and required case ids
  --timeout-min <n>           per-step timeout (default 240)
  --json                      print the report as JSON
  -h, --help                  print this text

Exit codes: 0 accepted; 1 rejected; 2 usage/identity error; 3 incomplete (--only).`;

function plannedSteps(options) {
  return [
    { id: 'phase0-symbolication', phase: 0, title: 'forced SIGSEGV of the release binary symbolicated through the archived dSYM, negative run refused' },
    { id: 'phase1-budget-pressure', phase: 1, title: 'deterministic scrollback budget pressure (mixed / all-active / floor exhaustion)' },
    { id: 'phase1-footprint-soak', phase: 1, title: 'ADVISORY (rev 5.1): 60 min combined daemon+keeper footprint soak at 71 panes - recorded, never gating' },
    { id: 'phase2-supervisor', phase: 2, title: 'supervisor four-case harness plus no-keeper refusal' },
    { id: 'phase2-launchd-chain', phase: 2, title: 'isolated launchd job owns the supervisor and restarts it exactly once' },
    { id: 'phase3-crash-survival', phase: 3, title: '60-session SIGKILL identity, replay gap, fencing, generations, graceful transfer, ownership' },
    { id: 'phase4-handover-failure', phase: 4, title: 'pre/post decision failure injection with observable serving assertions' },
    { id: 'risk-p99-echo', phase: 'risk', title: 'direct-daemon vs keeper-daemon p99 echo comparison' },
    { id: 'aux-host-agnostic-tests', phase: 'aux', title: 'host-agnostic test files, build-host-claims among them' },
    { id: 'packaging-shipped-payload', phase: 'packaging', title: 'packaged keeper payload digest, bundle seal and rollback floor agreement' },
  ];
}

async function runSteps(options, context) {
  const steps = [];
  const selected = options.only.length > 0 ? new Set(options.only) : null;
  const want = (id) => !selected || selected.has(id);
  const timeoutMs = options.timeoutMin * 60_000;
  const { candidate, candidateDigest, outDir, packaged, runDir } = context;

  const recordStep = (step) => {
    const finalized = finalizeStep({ ...step, outDir, candidateDigest });
    steps.push(finalized);
    return finalized;
  };

  // -- remote plumbing ------------------------------------------------------------------------
  //
  // One place decides whether a step can run remotely, and it never has a local fallback. A missing
  // path is a `blocked` step, which rejects the release; running the harness here instead would be
  // the one outcome the task forbids outright.
  const REMOTE_FLAGS = {
    daemon: ['--remote-daemon-binary', () => options.remoteDaemonBinary],
    keeper: ['--remote-keeper-binary', () => options.remoteKeeperBinary],
    cli: ['--remote-cli-binary', () => options.remoteCliBinary],
  };

  const remoteContext = (labels) => {
    const missing = [];
    // On the host, the worktree IS the working directory, and every receipt records `git rev-parse
    // HEAD` from it. Defaulting here rather than requiring the operator to restate the path removes a
    // way to bind the run to a tree other than the one under test.
    const worktree = options.remoteWorktree ?? (options.onHost ? process.cwd() : null);
    if (!worktree) missing.push('--remote-worktree');
    const binaryPaths = {};
    for (const label of labels) {
      const entry = REMOTE_FLAGS[label];
      if (!entry) {
        missing.push(`--remote-${label}-binary`);
        continue;
      }
      const value = entry[1]();
      if (!value) missing.push(entry[0]);
      else binaryPaths[label] = value;
    }
    if (missing.length > 0) {
      return {
        ok: false,
        code: 'GATE_REMOTE_PATH_MISSING',
        detail: `cannot run remotely without ${missing.join(', ')}; the step is blocked rather than executed on this Mac`,
      };
    }
    return { ok: true, worktree, binaryPaths };
  };

  /**
   * Runs one step remotely and judges its receipt. Returns either `{ blocked }` or
   * `{ run, receipt, evidencePath }`, where the caller adds its own harness judgement.
   */
  const runRemoteStep = ({ id, phase, slug, labels = [], command, detailTimeoutMs = timeoutMs }) => {
    const ctx = remoteContext(labels);
    if (!ctx.ok) return { blocked: { id, phase, code: ctx.code, detail: ctx.detail } };
    const run = runRemote({
      id,
      outDir,
      slug,
      worktree: ctx.worktree,
      binaryPaths: ctx.binaryPaths,
      command,
      timeoutMs: detailTimeoutMs,
      onHost: options.onHost,
    });
    const receipt = evaluateRemoteReceipt({ id, receipt: run.receipt, candidate, requiredBinaries: labels });
    // The transport half: a claim the wrapper refused, or an absent remote exit marker, is a failed
    // step even when the harness inside it printed a pass. Neither can be recovered from the local
    // exit code, which on this transport reports the ssh client, not the command.
    if (run.remote.claim?.refused) {
      receipt.problems.push({
        code: 'GATE_CLAIM_REFUSED',
        detail: `${id}: build-remote.sh exited 5 - the claim on ${run.remote.claim.host ?? 'the host'} was not ours, so nothing was run`,
      });
      receipt.status = 'fail';
    }
    if (!run.remote.markerPresent) {
      receipt.problems.push({
        code: 'GATE_REMOTE_MARKER_ABSENT',
        detail:
          `${id} produced no ${EXIT_MARKER_PREFIX} marker; the status is unknown, ` +
          `so a harness pass cannot be attributed to a completed run (child exit ${run.exitCode})`,
      });
      receipt.status = 'fail';
    }
    // A harness that reports its own evidence root is telling the operator where the raw files are.
    // They are NOT fetched: a second `build-remote.sh` call would take a second claim and could land
    // on another host, which is the nested-claim hazard. The path is recorded instead.
    const evidencePath = /__FERRYX_EVIDENCE__(\S+)/.exec(`${run.stdout ?? ''}`)?.[1] ?? null;
    return { run, receipt, evidencePath };
  };

  /** Merges the receipt verdict into whatever the harness itself said. */
  const withReceipt = (evaluated, receipt) => ({
    ...evaluated,
    status: evaluated.status === 'pass' && receipt.status === 'pass' ? 'pass' : 'fail',
    problems: [...(evaluated.problems ?? []), ...(receipt.problems ?? [])],
  });

  // -- Phase 0 -------------------------------------------------------------------------------
  if (want('phase0-symbolication')) {
    const id = 'phase0-symbolication';
    if (!runDir) {
      recordStep(blockedStep({ id, phase: 0, title: 'phase 0', code: 'GATE_RUN_DIR_REQUIRED', detail: 'phase 0 is judged from a release run directory' }));
    } else {
      // Symbolication is a Darwin-only act: it needs the Mach-O binary, `symbolicatecrash`,
      // `atos`/`dwarfdump` through the archived dSYM bundle, and a real SIGSEGV of a Mach-O process.
      // A Linux host cannot produce any of it, so the step runs on the claimed host (the picker may
      // well choose this Mac's roster entry) and `DARWIN_REQUIRED_STEPS` fails it if that host is
      // not Darwin. The paths handed to the harness are the HOST's paths.
      const remoteRun = options.remoteRunDir;
      if (!remoteRun) {
        recordStep(blockedStep({
          id,
          phase: 0,
          title: 'phase 0',
          code: 'GATE_REMOTE_RUN_DIR_MISSING',
          detail:
            'phase 0 must run on a Darwin build host, which needs --remote-run-dir naming the release run ' +
            'directory there (plan.json, bundle, dsyms, artifact); it is not executed on this Mac',
        }));
      } else {
        const remoteBinary = `${remoteRun}/bundle/macos/Ferryx.app/Contents/MacOS/ferryx`;
        const remoteDsym = `${remoteRun}/dsyms`;
        const remoteArtifact = `${remoteRun}/${PACKAGED_APP_ARTIFACT}`;
        const remote = runRemoteStep({
          id,
          phase: 0,
          slug: 'pty-dsym-symbolication',
          // The daemon binary is re-hashed on the host and compared with --daemon-binary, so the
          // crash that gets symbolicated is the candidate's own executable, not a neighbour's.
          labels: ['daemon'],
          command:
            `__fx_out="$PWD/.omo/evidence/combined-gate-phase0"; ` +
            `bun scripts/qa/dsym-symbolication-acceptance.mjs ` +
            `--binary ${shq(remoteBinary)} --dsym ${shq(remoteDsym)} --artifact ${shq(remoteArtifact)} ` +
            `--out "$__fx_out" --json; __fx_s=$?; ` +
            `printf '__FERRYX_EVIDENCE__%s\\n' "$__fx_out"; ` +
            `printf '__FERRYX_PHASE0_REPORT_BEGIN__\\n'; cat "$__fx_out/report.json" 2>/dev/null; ` +
            `printf '__FERRYX_PHASE0_REPORT_END__\\n'; exit $__fx_s`,
        });
        if (remote.blocked) {
          recordStep(blockedStep({ ...remote.blocked, title: 'phase 0: dSYM symbolication' }));
        } else {
          const report = parseBandedJson(remote.run.stdout, '__FERRYX_PHASE0_REPORT_BEGIN__', '__FERRYX_PHASE0_REPORT_END__');
          const harness = evaluatePhase0Evidence({ exitCode: remote.run.exitCode, report });
          const problems = [...harness.problems];
          // Second opinion from the authority publish itself uses - only when this Mac holds the run
          // directory, because that function reads release-local evidence. When it does not, the
          // omission is stated rather than silently treated as agreement.
          let authorityNote = null;
          if (existsSync(runDir)) {
            const authority = evaluateSymbolicationEvidence({ releaseDir: runDir });
            if (!authority.ok) {
              problems.push({
                code: 'GATE_PHASE0_AUTHORITY_REFUSED',
                detail: authority.blocking.map((problem) => `${problem.code}: ${problem.detail}`).join('; '),
              });
            }
          } else {
            authorityNote = `local release directory ${runDir} is absent, so the publish-authority cross-check was not applied`;
          }
          const evaluated = withReceipt({ status: problems.length === 0 ? 'pass' : 'fail', problems }, remote.receipt);
          if (report) writeFileSync(join(stepDir(outDir, id), 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
          const where = remote.evidencePath ? `remote symbolication evidence ${remote.evidencePath}` : null;
          recordStep({
            id,
            phase: 0,
            title: 'phase 0: dSYM symbolication',
            status: evaluated.status,
            exitCode: remote.run.exitCode,
            command: remote.run.command,
            logs: remote.run.logs,
            counts: harness.counts,
            problems: evaluated.problems,
            receipt: remote.run.receipt,
            remote: remote.run.remote,
            detail: [where, authorityNote].filter(Boolean).join('; ') || null,
          });
        }
      }
    }
  }

  // -- Phase 1: budget pressure --------------------------------------------------------------
  if (want('phase1-budget-pressure')) {
    const id = 'phase1-budget-pressure';
    const remote = runRemoteStep({
      id,
      phase: 1,
      slug: 'pty-budget-pressure',
      labels: [],
      command: 'cargo test --manifest-path src-tauri/Cargo.toml --lib -- terminal::scrollback_budget --test-threads=1',
    });
    if (remote.blocked) {
      recordStep(blockedStep({ ...remote.blocked, title: 'phase 1 Test A: deterministic budget pressure' }));
    } else {
      const evaluated = withReceipt(
        evaluateCargoStep({ id, exitCode: remote.run.exitCode, logText: `${remote.run.stdout}${remote.run.stderr}`, requiredTests: PHASE1_BUDGET_TESTS }),
        remote.receipt,
      );
      recordStep({
        id,
        phase: 1,
        title: 'phase 1 Test A: deterministic budget pressure',
        status: evaluated.status,
        exitCode: remote.run.exitCode,
        command: remote.run.command,
        logs: remote.run.logs,
        counts: evaluated.counts,
        problems: evaluated.problems,
        receipt: remote.run.receipt,
        remote: remote.run.remote,
        detail: `admission ceiling at 1.5 GiB / 8 MiB is ${PLAN_ADMISSION_LIMIT} sessions; the floor case asserts the typed refusal`,
      });
    }
  }

  // -- Phase 1: footprint soak: ADVISORY since rev 5.1 (2026-11-20) ---------------------------
  //
  // Test B is no longer a gating condition, so it is not a step: it is not in `steps`, it is not in
  // `requiredStepIds()`, and no SOAK problem can reach the verdict. `main` records it through
  // `evaluateSoakAdvisory` instead, which runs in every mode (`--verify-only` included) and never
  // blocks. Nothing is dispatched for it: the 60 minute run is evidence an operator already produced.

  // -- Phase 2: supervisor -------------------------------------------------------------------
  if (want('phase2-supervisor')) {
    const id = 'phase2-supervisor';
    const remote = runRemoteStep({
      id,
      phase: 2,
      slug: 'pty-supervisor-acceptance',
      labels: [],
      command: 'cargo test --manifest-path src-tauri/Cargo.toml --test daemon_crash_supervisor_acceptance -- --test-threads=1',
    });
    if (remote.blocked) {
      recordStep(blockedStep({ ...remote.blocked, title: 'phase 2: supervisor four-case harness' }));
    } else {
      const evaluated = withReceipt(
        evaluateCargoStep({ id, exitCode: remote.run.exitCode, logText: `${remote.run.stdout}${remote.run.stderr}`, requiredTests: PHASE2_SUPERVISOR_TESTS }),
        remote.receipt,
      );
      recordStep({
        id,
        phase: 2,
        title: 'phase 2: supervisor four-case harness',
        status: evaluated.status,
        exitCode: remote.run.exitCode,
        command: remote.run.command,
        logs: remote.run.logs,
        counts: evaluated.counts,
        problems: evaluated.problems,
        receipt: remote.run.receipt,
        remote: remote.run.remote,
      });
    }
  }

  // -- Phase 2: the launchd chain the cargo harness cannot prove ------------------------------
  //
  // This is the half st_01a1e627 F3 found missing from the whole tree, so it fails closed rather than
  // being folded into the cargo step: without it, "launchd owns the supervisor" is an assumption.
  if (want('phase2-launchd-chain')) {
    const id = 'phase2-launchd-chain';
    const dir = options.launchdChainEvidence;
    if (!dir) {
      recordStep(blockedStep({
        id,
        phase: 2,
        title: 'phase 2: launchd -> supervisor -> daemon chain',
        code: 'GATE_LAUNCHD_EVIDENCE_REQUIRED',
        detail:
          'the cargo harness starts children directly and cannot prove launchd owns the supervisor; ' +
          'supply a capture from a Darwin host with --launchd-chain-evidence',
      }));
    } else {
      const evidence = readJsonIfPresent(join(dir, 'chain.json'));
      const raw = {};
      for (const name of ['launchctl-print.txt', 'job.plist', 'restart-log.txt']) raw[name] = readTextIfPresent(join(dir, name));
      const evaluated = evaluateLaunchdChainEvidence({ evidence: { chain: evidence, raw }, candidate });
      recordStep({
        id,
        phase: 2,
        title: 'phase 2: launchd -> supervisor -> daemon chain',
        status: evidence ? evaluated.status : 'blocked',
        exitCode: null,
        command: null,
        logs: { stdout: join(dir, 'chain.json'), stderr: join(dir, 'restart-log.txt') },
        counts: evaluated.counts,
        problems: evidence ? evaluated.problems : [...evaluated.problems, { code: 'GATE_LAUNCHD_EVIDENCE_MISSING', detail: `${join(dir, 'chain.json')} was not readable` }],
        detail: `launchd chain evidence at ${dir}`,
      });
    }
  }

  // -- Phase 3 -------------------------------------------------------------------------------
  if (want('phase3-crash-survival')) {
    const id = 'phase3-crash-survival';
    if (options.remoteGenerations.length < 2 || options.remoteDaemonGenerations.length < 2) {
      recordStep(blockedStep({
        id,
        phase: 3,
        title: 'phase 3',
        code: 'GATE_GENERATIONS_MISSING',
        detail:
          'the plan requires multi-release keeper compatibility across two app replacements, which needs ' +
          '--remote-generations a,b and --remote-daemon-generations a,b naming builds on the build host ' +
          '(different protocol versions)',
      }));
    } else {
      const parts = [
        'bun scripts/qa/phase3-pty-crash-survival.acceptance.mjs',
        `--keeper-binary ${shq(options.remoteKeeperBinary ?? '')}`,
        `--daemon-binary ${shq(options.remoteDaemonBinary ?? '')}`,
        `--cli-binary ${shq(options.remoteCliBinary ?? '')}`,
        `--generations ${shq(options.remoteGenerations.join(','))}`,
        `--daemon-generations ${shq(options.remoteDaemonGenerations.join(','))}`,
        '--sessions 60',
        // --keep is mandatory: without it an all-pass run deletes the evidence the gate must retain.
        '--keep',
        '--json',
      ];
      for (const caseId of PHASE3_REQUIRED_CASES) parts.push(`--case ${shq(caseId)}`);
      const remote = runRemoteStep({
        id,
        phase: 3,
        slug: 'pty-phase3-crash-survival',
        labels: ['daemon', 'keeper', 'cli'],
        command: parts.join(' '),
      });
      if (remote.blocked) {
        recordStep(blockedStep({ ...remote.blocked, title: 'phase 3: PTY crash survival' }));
      } else {
        const summary = lastJsonDocument(remote.run.stdout);
        const evaluated = withReceipt(
          evaluateHarnessStep({ id, exitCode: remote.run.exitCode, summary, requiredCases: PHASE3_REQUIRED_CASES }),
          remote.receipt,
        );
        if (summary) writeFileSync(join(outDir, 'steps', id, 'summary.json'), `${JSON.stringify(summary, null, 2)}\n`);
        recordStep({
          id,
          phase: 3,
          title: 'phase 3: PTY crash survival',
          status: evaluated.status,
          exitCode: remote.run.exitCode,
          command: remote.run.command,
          logs: remote.run.logs,
          counts: evaluated.counts,
          problems: evaluated.problems,
          receipt: remote.run.receipt,
          remote: remote.run.remote,
          detail: summary?.root ? `harness evidence retained on ${remote.run.remote.host ?? 'the build host'} at ${summary.root}` : null,
        });
      }
    }
  }

  // -- Phase 4 -------------------------------------------------------------------------------
  if (want('phase4-handover-failure')) {
    const id = 'phase4-handover-failure';
    const parts = [
      'bun scripts/qa/phase4-handover-failure-injection.acceptance.mjs',
      `--daemon-binary ${shq(options.remoteDaemonBinary ?? '')}`,
      `--keeper-binary ${shq(options.remoteKeeperBinary ?? '')}`,
      '--keep',
      '--json',
    ];
    for (const caseId of PHASE4_REQUIRED_CASES) parts.push(`--case ${shq(caseId)}`);
    const remote = runRemoteStep({
      id,
      phase: 4,
      slug: 'pty-phase4-handover-failure',
      labels: ['daemon', 'keeper'],
      command: parts.join(' '),
    });
    if (remote.blocked) {
      recordStep(blockedStep({ ...remote.blocked, title: 'phase 4: handover failure injection' }));
    } else {
      const summary = lastJsonDocument(remote.run.stdout);
      const evaluated = withReceipt(
        evaluateHarnessStep({ id, exitCode: remote.run.exitCode, summary, requiredCases: PHASE4_REQUIRED_CASES }),
        remote.receipt,
      );
      if (summary) writeFileSync(join(outDir, 'steps', id, 'summary.json'), `${JSON.stringify(summary, null, 2)}\n`);
      recordStep({
        id,
        phase: 4,
        title: 'phase 4: handover failure injection',
        status: evaluated.status,
        exitCode: remote.run.exitCode,
        command: remote.run.command,
        logs: remote.run.logs,
        counts: evaluated.counts,
        problems: evaluated.problems,
        receipt: remote.run.receipt,
        remote: remote.run.remote,
        detail: summary?.root ? `harness evidence retained on ${remote.run.remote.host ?? 'the build host'} at ${summary.root}` : null,
      });
    }
  }

  // -- Risk row: p99 echo comparison ----------------------------------------------------------
  if (want('risk-p99-echo')) {
    const id = 'risk-p99-echo';
    // This step used to call `node` here, on whatever machine ran the gate. That is the one thing
    // the plan forbids (`all cargo/clippy/test runs go through scripts/build-remote.sh - never a
    // bare ssh and never on this Mac`), and it also handed the harness THIS Mac's binary paths,
    // which do not exist on the host that would be measured. It is now a remote step like the rest.
    //
    // The report is a single JSON document on stdout and the harness has no `--out`, so it is framed
    // on banded markers inside the one call - a second `build-remote.sh` call would take a second
    // claim and could land on another host (the nested-claim hazard the staging runbook records).
    const remote = runRemoteStep({
      id,
      phase: 'risk',
      slug: 'pty-p99-echo',
      labels: ['daemon', 'keeper'],
      command:
        // `--quiet` is not cosmetic: `build-remote.sh` merges the remote stdout and stderr into one
        // stream, so a harness writing progress lines to stderr DURING the report would interleave
        // them into the banded region and make a perfectly good report unparsable. `--quiet` silences
        // that log; the report itself is a single atomic write on stdout.
        // `--keep` is mandatory for the same reason it is in phase 3: the fixture is the raw evidence.
        `printf '__FERRYX_P99_REPORT_BEGIN__\\n'; ` +
        `bun scripts/qa/pty-echo-p99-comparison.mjs ` +
        `--daemon-binary ${shq(options.remoteDaemonBinary ?? '')} ` +
        `--keeper-binary ${shq(options.remoteKeeperBinary ?? '')} ` +
        `--arms direct-daemon,keeper-daemon --quiet --keep --json; __fx_s=$?; ` +
        `printf '__FERRYX_P99_REPORT_END__\\n'; ` +
        `exit $__fx_s`,
    });
    if (remote.blocked) {
      recordStep(blockedStep({ ...remote.blocked, title: 'risk row: p99 echo comparison' }));
    } else {
      const report = parseBandedJson(
        remote.run.stdout,
        '__FERRYX_P99_REPORT_BEGIN__',
        '__FERRYX_P99_REPORT_END__',
      );
      const evaluated = withReceipt(
        evaluateP99Evidence({ exitCode: remote.run.exitCode, report }),
        remote.receipt,
      );
      if (report) writeFileSync(join(stepDir(outDir, id), 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
      // The kept fixture lives in the HOST's $TMPDIR under a `mkdtemp` name the harness chose, and it
      // is recorded in the report rather than guessed at. This step used to print a
      // `__FERRYX_EVIDENCE__` path that the harness never creates, which reads as evidence on the
      // host that does not exist.
      const keptRoot = report?.cleanup?.keptRoot ?? null;
      recordStep({
        id,
        phase: 'risk',
        title: 'risk row: p99 echo comparison',
        status: evaluated.status,
        exitCode: remote.run.exitCode,
        command: remote.run.command,
        logs: remote.run.logs,
        counts: evaluated.counts,
        problems: evaluated.problems,
        gaps: evaluated.gaps,
        receipt: remote.run.receipt,
        remote: remote.run.remote,
        detail: keptRoot
          ? `kept fixture on ${remote.run.remote.host ?? 'the build host'} at ${keptRoot}`
          : 'the harness kept no fixture root, so only its report was retained',
      });
    }
  }

  // -- Aux: host-agnostic test files ---------------------------------------------------------
  //
  // These four are `node:test` files (`import test from "node:test"`), NOT Bun tests, so they run
  // under `node --test`, which is the command the repo's own evidence documents use for them. The
  // runner is chosen for what its output can PROVE: `node --test --test-reporter=tap` prints a
  // `1..N` plan and `# pass`/`# fail` totals per file, so a file can be shown to have executed a
  // non-zero number of assertions. A runner that only prints a human summary would leave this gate
  // parsing prose, which is the thing it refuses to do everywhere else.
  //
  // They run on the host, never here, and all four share ONE claim - they are one step, and a second
  // `build-remote.sh` call would take a second claim and could land on a different host.
  //
  // Judged per file, not per footer. Each file is run with `--test-reporter=tap` and wrapped in its
  // own banded region: the region's presence proves the file was executed, and `# fail 0` with a
  // non-zero `# pass` proves it actually asserted something. A file that printed no region, or a
  // region with no `# pass` line, fails - "no output" must never read as "nothing to complain about".
  if (want('aux-host-agnostic-tests')) {
    const id = 'aux-host-agnostic-tests';
    const parts = AUX_TEST_FILES.map((file, index) => {
      const band = `__FERRYX_AUX_${index}_`;
      return (
        `printf '${band}BEGIN__\\n'; ` +
        `node --test --test-reporter=tap ${shq(file)}; __fx_a=$?; ` +
        `printf '${band}END__\\n'; ` +
        `printf '__FERRYX_AUX_STATUS__%s:%s\\n' ${shq(file)} "$__fx_a"; ` +
        `[ "$__fx_a" -eq 0 ] || __fx_all=1`
      );
    });
    const remote = runRemoteStep({
      id,
      phase: 'aux',
      slug: 'pty-aux-tests',
      labels: [],
      // The runner is `node --test` because these are `node:test` files. A host without node would
      // otherwise fail with an opaque "no totals", so the absence is reported as itself: it is an
      // environment finding to fix, not a candidate finding to hide.
      command:
        `command -v node >/dev/null 2>&1 || { printf '__FERRYX_AUX_NO_NODE__\\n'; exit 3; }; ` +
        `__fx_all=0; ${parts.join('; ')}; exit "$__fx_all"`,
    });
    if (remote.blocked) {
      recordStep(blockedStep({ ...remote.blocked, title: 'aux: host-agnostic test files' }));
    } else {
      const text = `${remote.run.stdout ?? ''}\n${remote.run.stderr ?? ''}`;
      const problems = [];
      const perFile = [];
      if (text.includes('__FERRYX_AUX_NO_NODE__')) {
        problems.push({
          code: 'GATE_AUX_NODE_MISSING',
          detail:
            'no `node` on the build host, but these are node:test files; install a Node >= 22 or run ' +
            'this step on a host that has one - a Bun runner would not execute node:test assertions',
        });
      }
      for (const [index, file] of AUX_TEST_FILES.entries()) {
        if (text.includes('__FERRYX_AUX_NO_NODE__')) break;
        const band = `__FERRYX_AUX_${index}_BEGIN__`;
        const body = parseBandedText(text, band, `__FERRYX_AUX_${index}_END__`);
        if (body === null) {
          problems.push({
            code: 'GATE_AUX_NOT_EXECUTED',
            detail: `${file} produced no banded output, so it cannot be shown to have run at all`,
          });
          continue;
        }
        const pass = /^# pass (\d+)$/m.exec(body);
        const fail = /^# fail (\d+)$/m.exec(body);
        const passes = pass ? Number(pass[1]) : 0;
        const failures = fail ? Number(fail[1]) : null;
        perFile.push({ file, passes, failures });
        if (failures === null) {
          problems.push({
            code: 'GATE_AUX_NO_TOTALS',
            detail: `${file} printed no TAP "# fail" total, so its result cannot be read`,
          });
        } else if (failures > 0) {
          problems.push({ code: 'GATE_AUX_FAILURES', detail: `${file} reported ${failures} failing test(s)` });
        }
        if (passes === 0) {
          problems.push({
            code: 'GATE_AUX_NO_PASSES',
            detail: `${file} reported no passing test, so running it proved nothing`,
          });
        }
      }
      // `build-host-claims` is checked FIRST and separately: it is the only executable check on
      // `fx_claim_release`'s refusal semantics, and every remote step here depends on that code
      // handing a host back. Reporting it by name makes that dependency visible in the verdict.
      const claims = perFile.find((row) => row.file === AUX_TEST_FILES[0]);
      if (claims && claims.passes === 0) {
        problems.push({
          code: 'GATE_AUX_CLAIM_UNPROVEN',
          detail: `${AUX_TEST_FILES[0]} proved nothing, so the claim/release path every remote step uses is unverified`,
        });
      }
      if (remote.run.exitCode !== 0 && problems.length === 0) {
        problems.push({ code: 'GATE_AUX_EXIT', detail: `${id} exited ${remote.run.exitCode} with no per-file failure recorded` });
      }
      const evaluated = withReceipt(
        { status: problems.length === 0 ? 'pass' : 'fail', problems, counts: { files: perFile } },
        remote.receipt,
      );
      recordStep({
        id,
        phase: 'aux',
        title: 'aux: host-agnostic test files (build-host-claims first)',
        status: evaluated.status,
        exitCode: remote.run.exitCode,
        command: remote.run.command,
        logs: remote.run.logs,
        counts: evaluated.counts,
        problems: evaluated.problems,
        receipt: remote.run.receipt,
        remote: remote.run.remote,
        detail: `files: ${AUX_TEST_FILES.join(', ')}`,
      });
    }
  }

  // -- Packaging: shipped keeper payload and rollback floor -----------------------------------
  if (want('packaging-shipped-payload')) {
    const id = 'packaging-shipped-payload';
    if (!packaged?.keeperManifest) {
      recordStep(blockedStep({
        id,
        phase: 'packaging',
        title: 'packaged keeper payload',
        code: 'GATE_PACKAGED_KEEPER_MISSING',
        detail: 'the packaged artifact carries no Contents/Resources/keeper/keeper.json',
      }));
    } else {
      // Everything is read out of the packaged artifact, never out of a build tree: the digest a
      // shipped keeper is verified against only exists in the tarball the user installs.
      const artifactPath = join(runDir, PACKAGED_APP_ARTIFACT);
      const artifactBytes = readFileSync(artifactPath);
      const memberText = (name) => {
        const read = readTarGzMember({ bytes: artifactBytes, memberPath: `Ferryx.app/Contents/Resources/${KEEPER_RESOURCE_DIR}/${name}` });
        if (!read) return null;
        writeFileSync(join(stepDir(outDir, id), name), read.bytes);
        return read.bytes;
      };
      const manifestBytes = memberText(KEEPER_MANIFEST_NAME);
      const keeperBytes = memberText(KEEPER_BINARY_NAME);
      const bundleFloorBytes = memberText('latest.json');
      const parseOrNull = (bytes) => {
        if (!bytes) return null;
        try {
          return JSON.parse(bytes.toString('utf8'));
        } catch {
          return null;
        }
      };
      const payload = evaluateKeeperPayload({ manifest: parseOrNull(manifestBytes), keeperBytes });
      const bundleFloor = parseOrNull(bundleFloorBytes);
      const publishedManifest = readJsonIfPresent(join(runDir, 'publish', 'latest.json'));
      const floor = evaluateRollbackFloor({ bundleManifest: bundleFloor, publishedManifest });
      // The seal, from the archive's own `_CodeSignature/CodeResources`. `evaluateBundleSeal` existed
      // but nothing called it, so a keeper that was re-signed after `refreshKeeperDigest` would leave
      // `keeper.json` describing bytes that no longer exist while the digest check still passed on
      // the tarball member. Reading the seal from the shipped archive closes that gap: the member the
      // digest names must also be the member the bundle seals.
      const codeResources = readTarGzMember({
        bytes: artifactBytes,
        memberPath: 'Ferryx.app/Contents/_CodeSignature/CodeResources',
      });
      const seal = evaluateBundleSeal({
        codeResourcesText: codeResources ? codeResources.bytes.toString('utf8') : null,
        keeperBytes,
      });
      const problems = [...payload.problems, ...floor.problems, ...seal.problems];
      recordStep({
        id,
        phase: 'packaging',
        title: 'packaged keeper payload digest, bundle seal and rollback floor',
        status: problems.length === 0 ? 'pass' : 'fail',
        exitCode: null,
        command: null,
        logs: null,
        counts: { keeperBytes: payload.observed?.bytes ?? null, sealedEntries: seal.observed?.sealedEntries ?? null },
        problems,
      });
    }
  }

  return steps;
}

/**
 * Binds the procedure documents other owners are writing into the verdict as recorded facts.
 *
 * Three named documents are absent while this file is written (see `PROCEDURE_DOCS`). The tempting
 * shortcut is to skip their steps until they appear; that would let a release ship with no launchd
 * evidence and no Phase 4 evidence, because the only thing missing would be prose. So the split is:
 *
 *  - the PROPERTY is gated by a required step that is `blocked` when its evidence is absent, and a
 *    blocked step rejects. Absence of prose cannot make it pass.
 *  - the DOCUMENT is recorded here: present means its digest is bound and every step id it names is
 *    required to exist in this plan; absent means a `GATE_PROCEDURE_DOC_ABSENT` gap that names the
 *    owning step, so the omission is visible in the report instead of silently forgotten.
 *
 * A gap is a recorded fact, never a waiver: `renderGateReport` prints gaps under a heading that says
 * so, and no gap can change a verdict.
 */
export function evaluateProcedureDocs({ repoRoot = REPO_ROOT, docs = PROCEDURE_DOCS, stepIds = null } = {}) {
  const known = new Set(stepIds ?? plannedSteps({}).map((step) => step.id));
  const present = [];
  const gaps = [];
  const problems = [];
  for (const doc of docs) {
    const path = join(repoRoot, doc.path);
    if (!existsSync(path)) {
      gaps.push({
        code: 'GATE_PROCEDURE_DOC_ABSENT',
        detail:
          `${doc.path} does not exist yet, so the commands its owner must run ${doc.step} with are not ` +
          'recorded here; the step itself is still required and fails closed without its evidence',
      });
      continue;
    }
    const bytes = readFileSync(path);
    if (bytes.length === 0) {
      problems.push({ code: 'GATE_PROCEDURE_DOC_EMPTY', detail: `${doc.path} exists but is empty` });
      continue;
    }
    if (!known.has(doc.step)) {
      problems.push({
        code: 'GATE_PROCEDURE_DOC_UNKNOWN_STEP',
        detail: `${doc.path} names step ${doc.step}, which this gate does not plan`,
      });
      continue;
    }
    present.push({ path: doc.path, step: doc.step, sha256: sha256Hex(bytes), bytes: bytes.length });
  }
  return { status: problems.length === 0 ? 'pass' : 'fail', problems, present, gaps };
}

async function main(argv = process.argv.slice(2), env = process.env) {
  let options;
  try {
    options = parseArgs(argv, env);
  } catch (error) {
    process.stderr.write(`${error.message}\n\n${USAGE}\n`);
    return 2;
  }
  if (options.help) {
    process.stdout.write(`${USAGE}\n`);
    return 0;
  }
  if (options.list) {
    for (const step of plannedSteps(options)) process.stdout.write(`${step.id}  (phase ${step.phase})  ${step.title}\n`);
    process.stdout.write('\nphase 3 required cases:\n');
    for (const id of PHASE3_REQUIRED_CASES) process.stdout.write(`  ${id}\n`);
    process.stdout.write('\nphase 4 required cases:\n');
    for (const id of PHASE4_REQUIRED_CASES) process.stdout.write(`  ${id}\n`);
    return 0;
  }
  if (options.dryRun) {
    // A dry run must show what WOULD happen, including which inputs are missing, because the usual
    // reason to dry-run this gate is to find out why it would block. Printing USAGE here made
    // `--dry-run` indistinguishable from `--help` and hid every one of those gaps.
    const lines = ['# combined release gate - plan only, nothing executed', ''];
    const need = [
      ['--run', options.runDir],
      ['--daemon-binary', options.daemonBinary],
      ['--keeper-binary', options.keeperBinary],
      ['--cli-binary', options.cliBinary],
      ['--remote-worktree', options.remoteWorktree],
      ['--remote-run-dir', options.remoteRunDir],
      ['--remote-daemon-binary', options.remoteDaemonBinary],
      ['--remote-keeper-binary', options.remoteKeeperBinary],
      ['--remote-cli-binary', options.remoteCliBinary],
      ['--launchd-chain-evidence', options.launchdChainEvidence],
      ['--remote-generations', options.remoteGenerations.length >= 2 ? options.remoteGenerations.join(',') : null],
      ['--remote-daemon-generations', options.remoteDaemonGenerations.length >= 2 ? options.remoteDaemonGenerations.join(',') : null],
    ];
    for (const [flag, value] of need) {
      lines.push(`${value ? 'given  ' : 'MISSING'} ${flag}${value ? ` = ${value}` : ' (the step that needs it will be BLOCKED)'}`);
    }
    lines.push('', 'steps:');
    for (const step of plannedSteps(options)) {
      const advisory = ADVISORY_STEP_IDS.includes(step.id) ? ' [ADVISORY - judged from --soak-evidence when given, never gates]' : '';
      lines.push(`  ${step.id} (phase ${step.phase})${options.only.includes(step.id) || options.only.length === 0 ? '' : ' [not selected]'}${advisory}`);
    }
    lines.push('', `required: ${requiredStepIds().join(', ')}`);
    process.stdout.write(`${lines.join('\n')}\n`);
    return 0;
  }

  let built;
  try {
    // The frozen snapshot is verified BEFORE any candidate identity is built, so a stale or edited
    // staging directory is a refusal rather than something every step later inherits.
    let snapshot = null;
    if (options.candidateSnapshot) {
      const evaluated = evaluateCandidateSnapshot({
        metaPath: join(options.candidateSnapshot, 'candidate.meta'),
        patchPath: join(options.candidateSnapshot, 'candidate.patch'),
        manifestPath: join(options.candidateSnapshot, 'candidate.manifest'),
        commitSha: options.commitSha,
      });
      if (evaluated.status !== 'pass') {
        const error = new GateError(evaluated.problems[0].code, evaluated.problems[0].detail, evaluated.problems);
        throw error;
      }
      snapshot = evaluated.snapshot;
    }
    built = buildCandidate({
      runDir: options.runDir,
      commitSha: options.commitSha,
      daemonBinary: options.daemonBinary,
      keeperBinary: options.keeperBinary,
      cliBinary: options.cliBinary,
      snapshot,
    });
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    for (const problem of error.problems ?? []) process.stderr.write(`  - ${problem.code}: ${problem.detail}\n`);
    return 2;
  }

  const outDir = options.out ?? (options.runDir ? join(options.runDir, 'combined-gate') : join(REPO_ROOT, '.omo', 'evidence', 'combined-release-gate'));
  mkdirSync(outDir, { recursive: true });
  const candidatePath = join(outDir, 'candidate.json');
  writeFileSync(candidatePath, `${JSON.stringify({ candidate: built.candidate, candidateDigest: built.candidateDigest }, null, 2)}\n`);

  const steps = [];
  const gaps = [];
  if (options.verifyOnly) {
    for (const id of plannedSteps(options).map((step) => step.id)) {
      if (options.only.length > 0 && !options.only.includes(id)) continue;
      // An advisory id has no `result.json` and no verdict to re-derive; its evidence is read below
      // from `--soak-evidence`, so it is never replayed here as a MISSING step.
      if (ADVISORY_STEP_IDS.includes(id)) continue;
      steps.push(verifyRecordedStep({ outDir, id, requiredDigest: built.candidateDigest }));
    }
  } else {
    const produced = await runSteps(options, {
      candidate: built.candidate,
      candidateDigest: built.candidateDigest,
      outDir,
      packaged: built.packaged,
      runDir: options.runDir,
    });
    steps.push(...produced);
    for (const step of produced) gaps.push(...(step.gaps ?? []));
  }

  // Phase 1 Test B (rev 5.1, 2026-11-20): ADVISORY. Judged in every mode, recorded, and never gating.
  // It is computed here rather than inside `runSteps` because it dispatches nothing and must also be
  // recorded under `--verify-only`, where no step runs at all. `--only` still reaches it, since its
  // absence from `requiredStepIds()` is what keeps it out of the verdict either way.
  const advisories = [];
  if (options.soakEvidence) {
    const summaryPath = join(options.soakEvidence, 'footprint-summary.json');
    const samplesPath = join(options.soakEvidence, 'footprint-samples.jsonl');
    const summary = readJsonIfPresent(summaryPath);
    const samples = readJsonLinesIfPresent(samplesPath);
    const advisory = evaluateSoakAdvisory({ summary, samples });
    // The evidence is retained under the step directory so an auditor reads what the advisory was
    // derived from: the sample stream byte for byte, the summary re-serialised (it was parsed to be
    // judged, and a pretty-printed copy is what a reader can actually diff against the plan).
    const dir = stepDir(outDir, 'phase1-footprint-soak');
    if (summary) writeFileSync(join(dir, 'footprint-summary.json'), `${JSON.stringify(summary, null, 2)}\n`);
    const sampleText = readTextIfPresent(samplesPath);
    if (sampleText !== null) writeFileSync(join(dir, 'footprint-samples.jsonl'), sampleText);
    advisories.push({ ...advisory, detail: `evidence ${summaryPath} + ${samplesPath}` });
  }

  // Procedure documents other owners are still writing. Recorded (with digests when present) and
  // never allowed to change the verdict: the properties they describe are gated by required steps.
  const procedures = evaluateProcedureDocs({ stepIds: plannedSteps(options).map((step) => step.id) });
  gaps.push(...procedures.gaps);

  // `--only phase1-footprint-soak` selects an ADVISORY step, so it must not become a required id: that
  // would turn a step the plan deliberately stopped gating back into a rejection through the back door.
  const requiredIds = options.only.length > 0
    ? options.only.filter((id) => !ADVISORY_STEP_IDS.includes(id))
    : requiredStepIds();
  const evaluated = evaluateGate({ steps, requiredIds, advisories });

  // Cross-step identity: every receipted step must have run the same candidate on a host that reports
  // the right platform. `evaluateHostConsistency` was written for exactly this and had no caller, so a
  // run in which phase 3 measured one keeper build and phase 4 measured another could still report
  // `accepted`. Its problems fold into the same list the verdict is derived from.
  const consistency = evaluateHostConsistency({ steps });
  if (consistency.status !== 'pass') evaluated.problems.push(...consistency.problems);

  const incomplete = options.only.length > 0;
  const verdict = incomplete ? 'incomplete' : evaluated.problems.length === 0 ? 'accepted' : 'rejected';

  const report = {
    schema: GATE_SCHEMA,
    generatedAt: new Date().toISOString(),
    plan: { file: '.omo/plans/local-pty-crash-survival-20261116.md', revision: 'rev 5' },
    candidate: built.candidate,
    candidateDigest: built.candidateDigest,
    outDir,
    verdict,
    problems: evaluated.problems,
    // Recorded, never judged: `evaluateGate` copies these without merging them into `problems`, so the
    // verdict above cannot depend on them. The section is absent when no soak evidence was supplied.
    advisory: evaluated.advisory,
    gaps,
    hosts: consistency.hosts ?? [],
    procedureDocs: procedures.present,
    steps: steps.map((step) => ({
      id: step.id,
      phase: step.phase,
      title: step.title,
      status: step.status,
      exitCode: step.exitCode ?? null,
      command: step.command ?? null,
      logs: step.logs ?? null,
      counts: step.counts ?? null,
      problems: step.problems ?? [],
      gaps: step.gaps ?? [],
      detail: step.detail ?? null,
      // Kept in the report because the host and the claim outcome are part of the evidence: a pass
      // from a step whose claim was refused, or whose host is never named, cannot be audited later.
      receipt: step.receipt ?? null,
      remote: step.remote ?? null,
    })),
  };
  writeFileSync(join(outDir, 'gate-report.json'), `${JSON.stringify(report, null, 2)}\n`);
  writeFileSync(join(outDir, 'gate-report.md'), renderGateReport(report));

  if (options.json) {
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  } else {
    process.stdout.write(`${renderGateReport(report)}\n`);
    process.stdout.write(`evidence: ${outDir}\n`);
  }

  if (verdict === 'accepted') return 0;
  if (verdict === 'incomplete') return 3;
  return 1;
}

export { main, parseArgs, plannedSteps, verifyRecordedStep };

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  main()
    .then((code) => {
      process.exitCode = code;
    })
    .catch((error) => {
      process.stderr.write(`${error?.stack ?? error}\n`);
      process.exitCode = 1;
    });
}
