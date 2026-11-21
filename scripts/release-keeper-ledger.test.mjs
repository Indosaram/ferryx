// The keeper ledger append, exercised through `publishRelease` rather than by calling the helper
// directly: the append is only correct in its place in the publish sequence (after both evidence
// gates, BEFORE the release goes public), and a unit test of the helper would not prove that.
//
// Everything here is deterministic: a fixture run directory, a fake `gh` on the configured command,
// and a temporary ledger. Nothing reads the clock, sleeps, or touches the network.
//
// The checked-in ledger at `packaging/keeper/releases.json` is the floor every real release reads.
// Each test records its bytes and asserts they are unchanged at the end, so a regression that
// rewrites the repository's ledger instead of the injected path fails loudly here.
import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { once } from "node:events";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";
import { gzipSync } from "node:zlib";

import { writeSurvivalFixture } from "./fixtures/survival-fixture.mjs";
import { keeperManifest, readKeeperLedger, recordKeeperRelease } from "./lib/release-keeper.mjs";

/**
 * The floor a run with this ledger state would publish for `appVersion`.
 *
 * Derived through `keeperManifest` rather than hard-coded so the fixture states what assemble would
 * have written, which is exactly what publish now binds its append to.
 */
function appVersionFloor(appVersion, ledger = EMPTY_LEDGER) {
  return keeperManifest({ appVersion, ledger }).oldestSafeRollbackVersion;
}
import {
  assembleReleaseRun,
  assertKeeperFloorDurable,
  mutateKeeperLedgerLocked,
  publishRelease,
  reconcileKeeperPublication,
  writeFileDurably,
} from "./release-local.mjs";

const COMMIT = "5d5499806a1b207849778f488b4e3e7b821a751b";
const APP_VERSION = "2026.1024.1";
const TAG = `v${APP_VERSION}`;
const NEXT_VERSION = "2026.1030.1";
/** A keeper release older than APP_VERSION, for fixtures that need a ledger with history below it. */
const PRIOR_VERSION = "2026.1010.1";

/**
 * The ledger a real release reads; a test that writes it has broken the next release. Derived from
 * this file's location, not `process.cwd()`, so the guard holds wherever the suite is launched from.
 */
const REPO_LEDGER_PATH = resolve(dirname(fileURLToPath(import.meta.url)), "..", "packaging", "keeper", "releases.json");

const EMPTY_LEDGER = { schemaVersion: 1, keeperReleases: [] };

function digestDirectory(directory) {
  const hash = createHash("sha256");
  const visit = (current) => {
    for (const name of readdirSync(current).sort()) {
      const path = join(current, name);
      if (statSync(path).isDirectory()) visit(path);
      else hash.update(path.slice(directory.length + 1)).update("\0").update(readFileSync(path));
    }
  };
  visit(directory);
  return hash.digest("hex");
}

/** publishRelease refuses to run with CI markers set, so a test must clear them. */
async function withoutCIMarkers(fn) {
  const saved = { CI: process.env.CI, GITHUB_ACTIONS: process.env.GITHUB_ACTIONS };
  delete process.env.CI;
  delete process.env.GITHUB_ACTIONS;
  try {
    return await fn();
  } finally {
    if (saved.CI !== undefined) process.env.CI = saved.CI;
    if (saved.GITHUB_ACTIONS !== undefined) process.env.GITHUB_ACTIONS = saved.GITHUB_ACTIONS;
  }
}

/**
 * The symbolication evidence the publish gate requires: a PASS verdict whose UUIDs are the ones the
 * dSYM archive records for the shipped binary. Without it publish refuses before the ledger step,
 * so every fixture that is meant to reach the append must carry one.
 */
const BINARY_UUID = "11111111-2222-3333-4444-555555555555";

/**
 * One ustar member, built here so the packaged artifact is byte-identical on every machine.
 *
 * The symbolication gate binds its evidence to the bytes inside
 * `artifacts/darwin/Ferryx.app.tar.gz`, so the fixture has to ship a real archive with the member
 * the gate hashes rather than only a report claiming it does.
 */
function tarMember(name, bytes) {
  const header = Buffer.alloc(512);
  const octal = (value, length) => `${value.toString(8).padStart(length - 1, "0")}\0`;
  header.write(name, 0, 100, "utf8");
  header.write(octal(0o644, 8), 100, 8, "utf8");
  header.write(octal(0, 8), 108, 8, "utf8");
  header.write(octal(0, 8), 116, 8, "utf8");
  header.write(octal(bytes.length, 12), 124, 12, "utf8");
  header.write(octal(0, 12), 136, 12, "utf8");
  header.write("0", 156, 1, "utf8");
  header.write("ustar\0", 257, 6, "utf8");
  header.write("00", 263, 2, "utf8");
  header.write(" ".repeat(8), 148, 8, "utf8");
  let sum = 0;
  for (const byte of header) sum += byte;
  header.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148, 8, "utf8");
  const padded = Buffer.alloc(Math.ceil(bytes.length / 512) * 512);
  bytes.copy(padded, 0);
  return Buffer.concat([header, padded]);
}

function writeSymbolicationFixture(runDir) {
  const dsymDir = join(runDir, "dsyms");
  mkdirSync(dsymDir, { recursive: true });
  // The shipped binary the gate hashes, and the bytes it must find inside the packaged artifact.
  const binaryBytes = Buffer.from("fixture release binary\n");
  const binarySha256 = createHash("sha256").update(binaryBytes).digest("hex");
  const artifactDir = join(runDir, "artifacts", "darwin");
  mkdirSync(artifactDir, { recursive: true });
  writeFileSync(
    join(artifactDir, "Ferryx.app.tar.gz"),
    gzipSync(
      Buffer.concat([tarMember("Ferryx.app/Contents/MacOS/ferryx", binaryBytes), Buffer.alloc(1024)]),
    ),
  );
  writeFileSync(
    join(dsymDir, "manifest.json"),
    `${JSON.stringify({ schema: "ferryx-dsym-archive/v1", gate: [
      {
        binary: join(runDir, "artifacts/darwin/Ferryx.app/Contents/MacOS/ferryx"),
        uuids: [BINARY_UUID],
        dsyms: ["ferryx.dSYM"],
      },
    ] })}\n`,
  );
  const evidenceDir = join(runDir, "symbolication");
  mkdirSync(evidenceDir, { recursive: true });
  writeFileSync(
    join(evidenceDir, "report.json"),
    `${JSON.stringify({
      schema: "ferryx-dsym-symbolication/1",
      verdict: "PASS",
      binary: { uuids: [BINARY_UUID], sha256: binarySha256 },
      // The gate refuses a report that cannot prove the SIGSEGV was forced by this run.
      forcedCrash: { signal: "SIGSEGV", pid: 4321 },
      symbolicated: { namedFrames: 3 },
      negative: { gateCode: "DSYM_UUID_MISMATCH", namedFrames: 0 },
      checks: { positiveNamesFrame: { ok: true, detail: "named 3 frames" }, negativeRefused: { ok: true, detail: null } },
    })}\n`,
  );
}

/** Records the keeper generation the macOS build would have shipped in `dsyms/manifest.json`. */
function recordKeeperInDsymManifest(runDir, version = APP_VERSION) {
  const manifestPath = join(runDir, "dsyms", "manifest.json");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  manifest.keeper = {
    version,
    protocolMajor: 1,
    protocolMinor: 0,
    binary: "Ferryx.app/Contents/Resources/keeper/ferryx-ptykeeper",
    sha256: "a".repeat(64),
  };
  writeFileSync(manifestPath, `${JSON.stringify(manifest)}\n`);
}

function removeKeeperFromDsymManifest(runDir) {
  const manifestPath = join(runDir, "dsyms", "manifest.json");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  delete manifest.keeper;
  writeFileSync(manifestPath, `${JSON.stringify(manifest)}\n`);
}

/**
 * A fake `gh` that records its calls and round-trips assets, so draft verification sees the same
 * bytes it uploaded. `failEdit` makes the undraft step fail, which is the last network step and so
 * the closest thing to "the release never went public" a test can produce.
 *
 * `draftState` models what already exists on the remote at the start of the run:
 *  - "absent" (default): `release view` reports not found, so publish creates the draft.
 *  - "draft": a partial draft from an interrupted run; publish must resume instead of failing.
 *  - "public": an already-published release; publish must refuse and point at `reconcile`.
 */
function writeFakeGh(tmp, { failEdit = false, draftState = "absent" } = {}) {
  const callLog = join(tmp, "gh-calls.log");
  const notesCopy = join(tmp, "draft-notes.md");
  const assets = join(tmp, "assets");
  const gh = join(tmp, "fake-gh.mjs");
  writeFileSync(
    gh,
    `#!/usr/bin/env bun
import fs from "node:fs";
import path from "node:path";
const args = process.argv.slice(2);
fs.appendFileSync(${JSON.stringify(callLog)}, args.join(" ") + "\\n");
const [cmd, sub] = args;
if (cmd !== "release") process.exit(1);
if (sub === "view") {
  const wantsJson = args.includes("--json");
  const state = ${JSON.stringify(draftState)};
  if (state === "absent") { process.stderr.write("release not found\\n"); process.exit(1); }
  if (wantsJson) {
    process.stdout.write(JSON.stringify({ isDraft: state === "draft", tagName: args[2] }) + "\\n");
    process.exit(0);
  }
  process.exit(0);
}
if (sub === "create") { fs.copyFileSync(args[args.indexOf("--notes-file") + 1], ${JSON.stringify(notesCopy)}); process.exit(0); }
if (sub === "upload") {
  fs.mkdirSync(${JSON.stringify(assets)}, { recursive: true });
  for (const f of args.slice(3).filter((a) => a.includes(path.sep) && fs.existsSync(a))) fs.copyFileSync(f, path.join(${JSON.stringify(assets)}, path.basename(f)));
  process.exit(0);
}
if (sub === "download") {
  const out = args[args.indexOf("--dir") + 1];
  fs.mkdirSync(out, { recursive: true });
  for (const f of fs.readdirSync(${JSON.stringify(assets)})) fs.copyFileSync(path.join(${JSON.stringify(assets)}, f), path.join(out, f));
  process.exit(0);
}
${
  failEdit
    ? `if (sub === "edit") { process.stderr.write("undraft failed: 502 from api.github.com\\n"); process.exit(1); }`
    : `if (sub === "edit") process.exit(0);`
}
process.exit(1);
`,
  );
  chmodSync(gh, 0o755);
  return { gh, callLog };
}

/**
 * An assembled run directory that passes both evidence gates, plus a temp ledger to append to.
 * The assembly digests are computed from the directories as written, the same way `assemble` seals
 * them and `publish` re-checks them.
 */
function assembledRun(tmp, { ledger = EMPTY_LEDGER, failEdit = false, draftState = "absent" } = {}) {
  const runDir = join(tmp, "run");
  const publishDir = join(runDir, "publish");
  mkdirSync(publishDir, { recursive: true });
  const plan = {
    schemaVersion: 1,
    runId: "rel-run-keeper-ledger-1",
    repo: "Indosaram/ferryx",
    commitSha: COMMIT,
    tag: TAG,
    appVersion: APP_VERSION,
    msixVersion: `${APP_VERSION}.0`,
    channels: { store: true, nsisMigration: false },
    requiredTargets: ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"],
    toolchains: { node: ">=22.0.0", bun: ">=1.4.0", zig: "0.17.0" },
    sourceDateEpoch: 1788868800,
    createdAt: "2026-09-08T12:00:00.000Z",
  };
  writeFileSync(join(runDir, "plan.json"), JSON.stringify(plan));
  // The published manifest carries the floor it ships with: `publish` now binds the ledger append to
  // this value, so a fixture without it would be refused for having published no floor at all.
  writeFileSync(
    join(publishDir, "latest.json"),
    JSON.stringify({ version: plan.appVersion, keeper: { oldestSafeRollbackVersion: appVersionFloor(plan.appVersion) } }),
  );
  writeFileSync(join(runDir, "RELEASE_NOTES.md"), `# Ferryx release ${TAG}\n`);
  const configPath = join(runDir, "hosts.json");
  writeFileSync(configPath, "{}\n");
  const planDigest = createHash("sha256").update(readFileSync(join(runDir, "plan.json"))).digest("hex");
  writeFileSync(
    join(runDir, "prepare-state.json"),
    JSON.stringify({
      schemaVersion: 1,
      runId: plan.runId,
      planDigest,
      configDigest: createHash("sha256").update(readFileSync(configPath)).digest("hex"),
      configPath,
      commitSha: COMMIT,
      sourceDateEpoch: plan.sourceDateEpoch,
      ghosttyPin: "a".repeat(40),
      updaterPublicKey: "fixture",
    }),
  );
  for (const dir of ["receipts", "artifacts"]) mkdirSync(join(runDir, dir), { recursive: true });

  writeSurvivalFixture(runDir, COMMIT);
  writeSymbolicationFixture(runDir);

  writeFileSync(
    join(runDir, "assembly-state.json"),
    JSON.stringify({
      schemaVersion: 1,
      planDigest,
      receiptsDigest: digestDirectory(join(runDir, "receipts")),
      artifactsDigest: digestDirectory(join(runDir, "artifacts")),
      publishDigest: digestDirectory(publishDir),
    }),
  );

  const ledgerPath = join(tmp, "releases.json");
  writeFileSync(ledgerPath, `${JSON.stringify(ledger, null, 2)}\n`);

  const { gh, callLog } = writeFakeGh(tmp, { failEdit, draftState });
  return { runDir, gh, callLog, ledgerPath };
}

const publishArgs = (runDir, gh, ledgerPath) => ({
  runDir,
  approvePublish: true,
  ghCommand: gh,
  env: { FERRYX_APPROVE_PUBLISH: "1" },
  lsRemoteFn: () => `${COMMIT}\trefs/tags/${TAG}\n`,
  keeperLedgerPath: ledgerPath,
});

/**
 * Re-seals a fixture run after a test edits `plan.json` or `publish/`.
 *
 * `assembly-state.json` records the digest of the plan, the receipts, the artifacts and the publish
 * inventory, and `publishRelease` refuses a run whose bytes moved after it was sealed. A test that
 * changes any of those has to seal the new state, exactly as a re-run of `assemble` would.
 */
function resealRun(runDir) {
  const preparePath = join(runDir, "prepare-state.json");
  const state = JSON.parse(readFileSync(preparePath, "utf8"));
  state.planDigest = createHash("sha256").update(readFileSync(join(runDir, "plan.json"))).digest("hex");
  writeFileSync(preparePath, JSON.stringify(state));
  const assemblyPath = join(runDir, "assembly-state.json");
  const assembly = JSON.parse(readFileSync(assemblyPath, "utf8"));
  assembly.planDigest = state.planDigest;
  assembly.receiptsDigest = digestDirectory(join(runDir, "receipts"));
  assembly.artifactsDigest = digestDirectory(join(runDir, "artifacts"));
  assembly.publishDigest = digestDirectory(join(runDir, "publish"));
  writeFileSync(assemblyPath, JSON.stringify(assembly));
}

/** Runs `fn` with a temp directory and proves the repository ledger was left alone. */
async function withFixture(fn, options = {}) {
  const repoLedgerBefore = existsSync(REPO_LEDGER_PATH) ? readFileSync(REPO_LEDGER_PATH) : null;
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-keeper-ledger-"));
  try {
    return await fn(assembledRun(tmp, options), tmp);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
    const repoLedgerAfter = existsSync(REPO_LEDGER_PATH) ? readFileSync(REPO_LEDGER_PATH) : null;
    if (repoLedgerBefore === null) {
      assert.equal(repoLedgerAfter, null, "the test created the repository keeper ledger; it must only write its injected path");
    } else {
      assert.ok(
        Buffer.compare(repoLedgerBefore, repoLedgerAfter) === 0,
        "the test rewrote packaging/keeper/releases.json; every append must go to the injected ledger path",
      );
    }
  }
}

test("a successful publish appends the shipped keeper generation to the ledger", async () => {
  await withFixture(async ({ runDir, gh, ledgerPath }) => {
    recordKeeperInDsymManifest(runDir);

    const result = await withoutCIMarkers(() => publishRelease(publishArgs(runDir, gh, ledgerPath)));

    assert.equal(result.published, true);
    assert.equal(result.keeperLedger.changed, true);
    assert.equal(result.keeperLedger.path, ledgerPath);
    assert.equal(result.keeperLedger.oldestSafeRollbackVersion, APP_VERSION);

    const ledger = readKeeperLedger(ledgerPath);
    assert.deepEqual(ledger.keeperReleases, [
      { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
    ]);
  });
});

test("publishing a second release keeps the first as the floor instead of advancing it", async () => {
  // This is the degradation the append prevents: with an empty ledger every release publishes
  // itself as the floor, so the floor moves forward on every cut and no user can roll back.
  const firstRelease = {
    schemaVersion: 1,
    keeperReleases: [{ appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION }],
  };
  await withFixture(async ({ runDir, gh, ledgerPath }) => {
    recordKeeperInDsymManifest(runDir, NEXT_VERSION);
    const plan = JSON.parse(readFileSync(join(runDir, "plan.json"), "utf8"));
    plan.appVersion = NEXT_VERSION;
    plan.msixVersion = `${NEXT_VERSION}.0`;
    plan.tag = `v${NEXT_VERSION}`;
    writeFileSync(join(runDir, "plan.json"), JSON.stringify(plan));
    writeFileSync(
      join(runDir, "publish", "latest.json"),
      JSON.stringify({ version: NEXT_VERSION, keeper: { oldestSafeRollbackVersion: APP_VERSION } }),
    );

    // Re-seal the digests the fixture recorded before the version changed: the plan, and the
    // publish inventory that now carries the new latest.json.
    const state = JSON.parse(readFileSync(join(runDir, "prepare-state.json"), "utf8"));
    state.planDigest = createHash("sha256").update(readFileSync(join(runDir, "plan.json"))).digest("hex");
    writeFileSync(join(runDir, "prepare-state.json"), JSON.stringify(state));
    const assembly = JSON.parse(readFileSync(join(runDir, "assembly-state.json"), "utf8"));
    assembly.planDigest = state.planDigest;
    assembly.publishDigest = digestDirectory(join(runDir, "publish"));
    writeFileSync(join(runDir, "assembly-state.json"), JSON.stringify(assembly));

    const result = await withoutCIMarkers(() =>
      publishRelease({
        ...publishArgs(runDir, gh, ledgerPath),
        lsRemoteFn: () => `${COMMIT}\trefs/tags/v${NEXT_VERSION}\n`,
      }),
    );

    assert.equal(result.keeperLedger.oldestSafeRollbackVersion, APP_VERSION);
    const ledger = readKeeperLedger(ledgerPath);
    assert.equal(ledger.keeperReleases.length, 2);
    assert.deepEqual(ledger.keeperReleases[1], {
      appVersion: NEXT_VERSION,
      protocolMajor: 1,
      oldestSafeRollbackVersion: APP_VERSION,
    });
  }, { ledger: firstRelease });
});

test("re-publishing an already recorded release is idempotent and leaves the ledger bytes untouched", async () => {
  const alreadyRecorded = {
    schemaVersion: 1,
    keeperReleases: [{ appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION }],
  };
  await withFixture(
    async ({ runDir, gh, ledgerPath }) => {
      recordKeeperInDsymManifest(runDir);
      const before = readFileSync(ledgerPath);

      const result = await withoutCIMarkers(() => publishRelease(publishArgs(runDir, gh, ledgerPath)));

      assert.equal(result.published, true);
      assert.equal(result.keeperLedger.changed, false);
      assert.ok(Buffer.compare(before, readFileSync(ledgerPath)) === 0, "an idempotent append must not rewrite the ledger");
      assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 1);
    },
    { ledger: alreadyRecorded },
  );
});

test("a publication that never goes public appends first and leaves an intent behind", async () => {
  await withFixture(
    async ({ runDir, gh, ledgerPath }) => {
      recordKeeperInDsymManifest(runDir);

      await assert.rejects(
        () => withoutCIMarkers(() => publishRelease(publishArgs(runDir, gh, ledgerPath))),
        // The failure is reported with the recovery step, not as a bare gh error.
        /Failed to undraft release[\s\S]*reconcile/,
      );

      // The append happens BEFORE the undraft, so a failed undraft leaves the release recorded.
      // That is the safe direction for the floor: the alternative (public but unrecorded) is what
      // silently un-pins it. The intent file is what marks the publish as unfinished.
      const ledger = readKeeperLedger(ledgerPath);
      assert.equal(ledger.keeperReleases.length, 1);
      assert.equal(ledger.keeperReleases[0].appVersion, APP_VERSION);
      assert.ok(existsSync(`${ledgerPath}.pending`), "the unfinished publish must leave an intent file");

      // A retry must not overwrite the live intent, and must not double-append. The intent guard
      // runs before the create/upload steps precisely so a stuck publish cannot be re-driven over
      // state `reconcile` still has to settle.
      await assert.rejects(
        () => withoutCIMarkers(() => publishRelease(publishArgs(runDir, gh, ledgerPath))),
        /KEEPER_PUBLISH_IN_FLIGHT/,
      );
      assert.ok(existsSync(`${ledgerPath}.pending`), "the refused retry must leave the original intent intact");
      assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 1);
    },
    { failEdit: true },
  );
});

test("a run that shipped no keeper generation does not touch the ledger", async () => {
  await withFixture(async ({ runDir, gh, ledgerPath }) => {
    removeKeeperFromDsymManifest(runDir);
    const before = readFileSync(ledgerPath);

    const result = await withoutCIMarkers(() => publishRelease(publishArgs(runDir, gh, ledgerPath)));

    assert.equal(result.published, true);
    assert.equal(result.keeperLedger, null);
    assert.ok(Buffer.compare(before, readFileSync(ledgerPath)) === 0);
  });
});

test("a refused release never reaches the ledger step", async () => {
  await withFixture(async ({ runDir, gh, ledgerPath, callLog }) => {
    recordKeeperInDsymManifest(runDir);
    // A failed symbolication check is refused before any network call.
    const reportPath = join(runDir, "symbolication", "report.json");
    const report = JSON.parse(readFileSync(reportPath, "utf8"));
    report.checks.negativeRefused = { ok: false, detail: "negative run named a frame" };
    writeFileSync(reportPath, `${JSON.stringify(report)}\n`);
    const before = readFileSync(ledgerPath);

    await assert.rejects(
      () => withoutCIMarkers(() => publishRelease(publishArgs(runDir, gh, ledgerPath))),
      (error) => error.code === "SYMBOLICATION_CHECK_FAILED",
    );

    assert.equal(existsSync(callLog), false, "no gh call may run before the gate passes");
    assert.ok(Buffer.compare(before, readFileSync(ledgerPath)) === 0);
  });
});

// The floor in a published `latest.json` comes from the ledger, and publish writes that ledger to
// the working tree only. If the append is never committed, the next clone reads an empty ledger and
// publishes ITSELF as the floor -- the floor then advances on every cut and no user can roll back.
// That failure is silent, so assemble refuses to publish a floor it cannot reproduce.
const LEDGER_WITH = { schemaVersion: 1, keeperReleases: [{ appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION }] };

/** Runs the guard against `onDisk` while `committedText` is what HEAD holds (null = file absent). */
function checkDurability(onDisk, committedText) {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-keeper-floor-"));
  try {
    const ledgerPath = join(tmp, "releases.json");
    if (onDisk !== null) writeFileSync(ledgerPath, `${JSON.stringify(onDisk, null, 2)}\n`);
    return assertKeeperFloorDurable({ ledgerPath, readCommitted: () => committedText });
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

test("a ledger whose entries are all committed passes the durability guard", () => {
  const result = checkDurability(LEDGER_WITH, `${JSON.stringify(LEDGER_WITH)}\n`);
  assert.deepEqual(result, { entries: 1, uncommitted: [] });
});

test("an uncommitted ledger append is refused, naming the version that would be lost", () => {
  // The exact state finding 3 describes: publish appended, operator never committed, next cut runs.
  assert.throws(
    () => checkDurability(LEDGER_WITH, null),
    (error) => {
      assert.equal(error.code, "KEEPER_LEDGER_UNCOMMITTED");
      assert.deepEqual(error.detail.uncommitted, [APP_VERSION]);
      assert.match(error.message, /Committing the ledger append is what keeps oldestSafeRollbackVersion pinned/);
      return true;
    },
  );
});

test("an entry added on disk but not committed is refused even when older entries are committed", () => {
  const committed = { schemaVersion: 1, keeperReleases: [{ appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION }] };
  const onDisk = {
    schemaVersion: 1,
    keeperReleases: [...committed.keeperReleases, { appVersion: NEXT_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION }],
  };
  assert.throws(
    () => checkDurability(onDisk, `${JSON.stringify(committed)}\n`),
    (error) => {
      assert.equal(error.code, "KEEPER_LEDGER_UNCOMMITTED");
      // Only the new entry is named: the committed floor is not the problem.
      assert.deepEqual(error.detail.uncommitted, [NEXT_VERSION]);
      return true;
    },
  );
});

test("an empty ledger passes, because the first keeper release legitimately has no floor yet", () => {
  // Publish appends the first release; refusing assemble here would make the first keeper release
  // impossible to cut at all.
  assert.deepEqual(checkDurability(EMPTY_LEDGER, null), { entries: 0, uncommitted: [] });
});

test("a missing ledger is refused rather than treated as an empty floor", () => {
  assert.throws(
    () => checkDurability(null, null),
    (error) => error.code === "KEEPER_LEDGER_MISSING",
  );
});

test("assemble refuses before writing anything when the ledger is uncommitted", async () => {
  await withFixture(async ({ runDir }) => {
    // Assembly seals the run by writing assembly-state.json (`flag: "wx"`). The fixture pre-creates
    // it as publish does, so removing it leaves the seal as the observable step: it must be absent
    // afterwards, proving assembly never ran past the guard.
    rmSync(join(runDir, "assembly-state.json"));
    let called = false;
    // `assembleReleaseRun` is synchronous: `assert.rejects` would treat the synchronous throw as a
    // failed assertion instead of the expected refusal, so this must be `assert.throws`.
    await withoutCIMarkers(async () => {
      assert.throws(
        () =>
          assembleReleaseRun({
            runDir,
            pubkey: "fixture",
            minisignBin: "minisign",
            keeperLedgerPath: join(runDir, "missing-ledger.json"),
            assertFloorDurable: () => {
              called = true;
              throw Object.assign(new Error("KEEPER_LEDGER_UNCOMMITTED: fixture"), { code: "KEEPER_LEDGER_UNCOMMITTED" });
            },
          }),
        (error) => error.code === "KEEPER_LEDGER_UNCOMMITTED",
      );
    });
    assert.equal(called, true, "the durability guard must run");
    assert.equal(existsSync(join(runDir, "assembly-state.json")), false, "a refused assemble must not seal the run");
  });
});

// ---------------------------------------------------------------------------------------------
// Crash window: the process dies between recording the keeper and confirming the release is
// public. These are deterministic, not timing-based. A "crash" is modelled by writing exactly the
// on-disk state a crash leaves and then invoking the next step, or by injecting the failure at the
// exact syscall boundary. No sleeps, no polling, no retries.
// ---------------------------------------------------------------------------------------------

/** The intent file a crash after `prepareKeeperPublication` leaves behind. */
function writeIntent(
  ledgerPath,
  { appVersion = APP_VERSION, tag = TAG, repo = "Indosaram/ferryx", oldestSafeRollbackVersion = APP_VERSION } = {},
) {
  writeFileSync(
    `${ledgerPath}.pending`,
    `${JSON.stringify(
      {
        schema: "ferryx-keeper-publish-intent/1",
        appVersion,
        tag,
        repo,
        commitSha: COMMIT,
        oldestSafeRollbackVersion,
      },
      null,
      2,
    )}\n`,
  );
}

test("a crash after the append leaves an intent that fails the next assemble closed", async () => {
  await withFixture(async ({ ledgerPath }) => {
    const ledger = { schemaVersion: 1, keeperReleases: [{ appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION }] };
    writeFileSync(ledgerPath, `${JSON.stringify(ledger, null, 2)}\n`);
    writeIntent(ledgerPath);

    assert.throws(
      () => assertKeeperFloorDurable({ ledgerPath, readCommitted: () => `${JSON.stringify(ledger)}\n` }),
      (error) => {
        // The publish code, not the uncommitted one: the ledger content is fine, the doubt is
        // whether the release it describes ever shipped.
        assert.equal(error.code, "KEEPER_PUBLISH_UNFINISHED");
        assert.equal(error.detail.intent.appVersion, APP_VERSION);
        assert.match(error.message, /Resolve it first/);
        return true;
      },
    );
  });
});

test("an unreadable intent is still treated as an unfinished publish", async () => {
  // Fail closed: a truncated intent is evidence of a crash we cannot explain, so it must not be
  // ignored the way a missing file legitimately is.
  await withFixture(async ({ ledgerPath }) => {
    writeFileSync(`${ledgerPath}.pending`, "{ truncated");
    assert.throws(
      () => assertKeeperFloorDurable({ ledgerPath, readCommitted: () => `${JSON.stringify(EMPTY_LEDGER)}\n` }),
      (error) => error.code === "KEEPER_PUBLISH_UNFINISHED",
    );
  });
});

test("reconcile keeps the entry when the release is public and clears the intent", async () => {
  await withFixture(async ({ ledgerPath }) => {
    const ledger = { schemaVersion: 1, keeperReleases: [{ appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION }] };
    writeFileSync(ledgerPath, `${JSON.stringify(ledger, null, 2)}\n`);
    writeIntent(ledgerPath);

    const result = await reconcileKeeperPublication({
      ledgerPath,
      spawn: () => ({ status: 0, stdout: `{"isDraft":false,"tagName":"${TAG}"}`, stderr: "" }),
    });

    assert.deepEqual({ public: result.public, withdrew: result.withdrew }, { public: true, withdrew: false });
    assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 1, "a shipped entry must not be withdrawn");
    assert.equal(existsSync(`${ledgerPath}.pending`), false);
    assert.equal(readKeeperLedger(ledgerPath).keeperReleases[0].oldestSafeRollbackVersion, APP_VERSION);
  });
});

test("reconcile withdraws the entry when the release is still a draft", async () => {
  await withFixture(async ({ ledgerPath }) => {
    const ledger = { schemaVersion: 1, keeperReleases: [{ appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION }] };
    writeFileSync(ledgerPath, `${JSON.stringify(ledger, null, 2)}\n`);
    writeIntent(ledgerPath);

    const result = await reconcileKeeperPublication({
      ledgerPath,
      spawn: () => ({ status: 0, stdout: `{"isDraft":true,"tagName":"${TAG}"}`, stderr: "" }),
    });

    assert.deepEqual({ public: result.public, withdrew: result.withdrew }, { public: false, withdrew: true });
    // An unshipped release must not stay the recorded floor: that would publish a floor derived
    // from a release no user can install.
    assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 0);
    assert.equal(existsSync(`${ledgerPath}.pending`), false);
  });
});

test("reconcile withdraws the entry when the release does not exist", async () => {
  await withFixture(async ({ ledgerPath }) => {
    const ledger = { schemaVersion: 1, keeperReleases: [{ appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION }] };
    writeFileSync(ledgerPath, `${JSON.stringify(ledger, null, 2)}\n`);
    writeIntent(ledgerPath);

    const result = await reconcileKeeperPublication({
      ledgerPath,
      spawn: () => ({ status: 1, stdout: "", stderr: "release not found" }),
    });

    assert.deepEqual({ public: result.public, withdrew: result.withdrew }, { public: false, withdrew: true });
    assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 0);
  });
});

test("reconcile refuses to guess when GitHub cannot be reached", async () => {
  // The dangerous outcome is clearing the intent without knowing the release state, so an
  // inconclusive answer must leave both the intent and the ledger untouched.
  await withFixture(async ({ ledgerPath }) => {
    const ledger = { schemaVersion: 1, keeperReleases: [{ appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION }] };
    writeFileSync(ledgerPath, `${JSON.stringify(ledger, null, 2)}\n`);
    writeIntent(ledgerPath);

    await assert.rejects(
      () => reconcileKeeperPublication({ ledgerPath, spawn: () => ({ status: 1, stdout: "", stderr: "HTTP 502 Bad Gateway" }) }),
      (error) => error.code === "KEEPER_RECONCILE_UNKNOWN",
    );

    assert.ok(existsSync(`${ledgerPath}.pending`), "an inconclusive reconcile must not clear the intent");
    assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 1);
  });
});

test("reconcile does not contact GitHub when no publish is in flight", async () => {
  await withFixture(async ({ ledgerPath }) => {
    const result = await reconcileKeeperPublication({
      ledgerPath,
      spawn: () => {
        throw new Error("reconcile must not contact GitHub without an intent");
      },
    });
    assert.equal(result.reconciled, false);
  });
});

test("a failure inside the durable write leaves the previous ledger bytes whole", async () => {
  // `writeFileSync` truncates in place, so a mid-write failure would leave a prefix that
  // `readKeeperLedger` cannot parse -- which reads as an empty ledger and silently re-floors the
  // release. The injected fsync failure happens after the temp file is written and before the
  // rename, so the destination must still hold its original bytes.
  await withFixture(async ({ ledgerPath }) => {
    const before = `${JSON.stringify(EMPTY_LEDGER, null, 2)}\n`;
    writeFileSync(ledgerPath, before);

    assert.throws(() =>
      writeFileDurably(ledgerPath, `${JSON.stringify(LEDGER_WITH, null, 2)}\n`, {
        fsyncSync: () => {
          throw Object.assign(new Error("injected EIO"), { code: "EIO" });
        },
      }),
    );

    assert.equal(readFileSync(ledgerPath, "utf8"), before, "a failed write must not touch the destination");
    assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 0);
  });
});

// ---------------------------------------------------------------------------------------------
// Audit round 2: defects found against the first crash-safe implementation.
// Each test drives a real production entry point (publishRelease / reconcileKeeperPublication /
// assertKeeperFloorDurable), not an internal helper, so a wiring regression is caught too.
// ---------------------------------------------------------------------------------------------

test("a resuming publish continues from an existing draft instead of failing", async () => {
  // The crash this covers: `release create --draft` succeeded, then the process died before the
  // intent was written. Nothing local records the attempt, so the only evidence is the remote draft.
  await withFixture(
    async ({ runDir, gh, ledgerPath, callLog }) => {
      recordKeeperInDsymManifest(runDir);

      const result = await withoutCIMarkers(() => publishRelease(publishArgs(runDir, gh, ledgerPath)));
      assert.equal(result.published, true);

      // The draft must not be re-created (that fails with "already exists" on real gh), but it must
      // still be uploaded to and finally undrafted. The fake `gh` logs its whole argv, so each line
      // starts with `release <subcommand>`.
      const calls = readFileSync(callLog, "utf8").trim().split("\n");
      assert.ok(
        !calls.some((line) => line.startsWith("release create")),
        "an existing draft must not be re-created",
      );
      assert.ok(calls.some((line) => line.startsWith("release upload")), "the existing draft must be uploaded to");
      assert.ok(calls.some((line) => line.startsWith("release edit")), "the release must still be undrafted");
      assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 1);
    },
    { draftState: "draft" },
  );
});

test("publishing over an already public release is refused and points at reconcile", async () => {
  await withFixture(
    async ({ runDir, gh, ledgerPath }) => {
      recordKeeperInDsymManifest(runDir);

      await assert.rejects(
        () => withoutCIMarkers(() => publishRelease(publishArgs(runDir, gh, ledgerPath))),
        /already exists[\s\S]*reconcile/,
      );
      // A refusal must not touch the ledger: nothing shipped in this run.
      assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 0);
    },
    { draftState: "public" },
  );
});

test("reconcile appends a public release the ledger does not record", async () => {
  // The pre-intent crash: the release went public, but the process died before recording it.
  // Clearing the intent without appending would leave a shipped keeper unrecorded and let the
  // floor walk forward.
  await withFixture(async ({ ledgerPath }) => {
    writeFileSync(ledgerPath, `${JSON.stringify(EMPTY_LEDGER, null, 2)}\n`);
    writeIntent(ledgerPath);

    const result = await reconcileKeeperPublication({
      ledgerPath,
      spawn: () => ({ status: 0, stdout: `{"isDraft":false,"tagName":"${TAG}"}`, stderr: "" }),
    });

    assert.equal(result.public, true);
    assert.equal(result.appended, true);
    assert.deepEqual(readKeeperLedger(ledgerPath).keeperReleases, [
      { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
    ]);
    assert.equal(existsSync(`${ledgerPath}.pending`), false);
  });
});

test("reconcile does not double-append a public release the ledger already records", async () => {
  await withFixture(async ({ ledgerPath }) => {
    const ledger = { schemaVersion: 1, keeperReleases: [{ appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION }] };
    writeFileSync(ledgerPath, `${JSON.stringify(ledger, null, 2)}\n`);
    writeIntent(ledgerPath);

    const result = await reconcileKeeperPublication({
      ledgerPath,
      spawn: () => ({ status: 0, stdout: `{"isDraft":false,"tagName":"${TAG}"}`, stderr: "" }),
    });

    assert.equal(result.appended, false);
    assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 1);
  });
});

test("a floor value that diverges from the committed ledger is refused", async () => {
  // Versions match but the floor does not: the working tree moved the floor forward. Checking only
  // appVersions would accept this and publish the advanced floor.
  await withFixture(async ({ ledgerPath }) => {
    const onDisk = {
      schemaVersion: 1,
      keeperReleases: [
        { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
        { appVersion: NEXT_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: NEXT_VERSION },
      ],
    };
    const committed = {
      schemaVersion: 1,
      keeperReleases: [
        { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
        { appVersion: NEXT_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
      ],
    };
    writeFileSync(ledgerPath, `${JSON.stringify(onDisk, null, 2)}\n`);

    assert.throws(
      () => assertKeeperFloorDurable({ ledgerPath, readCommitted: () => `${JSON.stringify(committed)}\n` }),
      (error) => {
        assert.equal(error.code, "KEEPER_LEDGER_UNCOMMITTED");
        assert.deepEqual(error.detail.uncommitted, [NEXT_VERSION]);
        return true;
      },
    );
  });
});

test("identical floors across every entry are accepted", async () => {
  // The positive control for the check above: same versions AND same floors must pass.
  await withFixture(async ({ ledgerPath }) => {
    const ledger = {
      schemaVersion: 1,
      keeperReleases: [
        { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
        { appVersion: NEXT_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
      ],
    };
    writeFileSync(ledgerPath, `${JSON.stringify(ledger, null, 2)}\n`);

    assert.doesNotThrow(() =>
      assertKeeperFloorDurable({ ledgerPath, readCommitted: () => `${JSON.stringify(ledger)}\n` }),
    );
  });
});

test("a concurrent ledger writer blocks the append instead of being silently overwritten", async () => {
  // Modelled by creating the lock directory a concurrent writer would hold while this publish tries
  // to append. Without the lock both would read the same ledger and the later rename would drop the
  // other release entirely.
  await withFixture(async ({ runDir, gh, ledgerPath }) => {
    recordKeeperInDsymManifest(runDir);
    mkdirSync(`${ledgerPath}.lock`);

    await assert.rejects(
      () => withoutCIMarkers(() => publishRelease(publishArgs(runDir, gh, ledgerPath))),
      (error) => {
        assert.equal(error.code, "KEEPER_LEDGER_LOCKED");
        assert.match(error.message, /stale/);
        return true;
      },
    );

    assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 0, "a blocked append must write nothing");
    assert.equal(existsSync(`${ledgerPath}.pending`), false, "a blocked append must not leak an intent");
  });
});

test("the ledger lock is released after a successful publish", async () => {
  await withFixture(async ({ runDir, gh, ledgerPath }) => {
    recordKeeperInDsymManifest(runDir);
    await withoutCIMarkers(() => publishRelease(publishArgs(runDir, gh, ledgerPath)));
    assert.equal(existsSync(`${ledgerPath}.lock`), false, "a successful publish must not leave the lock behind");
  });
});

// ---------------------------------------------------------------------------------------------
// Lock interop and stale-lock behaviour.
//
// These drive the real mutator across real process boundaries, which is the only place `mkdir`
// EEXIST semantics and pid liveness mean anything. Ordering is by explicit handshake (a flag file
// the child writes from inside the critical section) or by an awaited child exit -- never by
// sleeping -- so no test here can pass or fail on timing luck.
// ---------------------------------------------------------------------------------------------

/** The pid of a process that has already exited and been reaped, so `kill(pid, 0)` gives ESRCH. */
function reapedPid() {
  const result = spawnSync(process.execPath, ["-e", ""], { encoding: "utf8", timeout: 60_000 });
  assert.equal(result.status, 0, "the fixture process must exit cleanly so its pid is provably dead");
  return result.pid;
}

/** Runs a child and returns its last stdout line parsed as JSON. */
function runChild(scriptPath, options = {}) {
  const result = spawnSync(process.execPath, [scriptPath], { encoding: "utf8", timeout: 60_000, ...options });
  if (result.status !== 0) {
    throw new Error(
      `child exited ${result.status} (signal ${result.signal})\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
    );
  }
  const lastLine = result.stdout.trim().split("\n").pop();
  return lastLine ? JSON.parse(lastLine) : null;
}

const RELEASE_LOCAL_URL = new URL("./release-local.mjs", import.meta.url).href;

test("a second process cannot take a held lock, and can once the holder releases it", async () => {
  await withFixture(async ({ ledgerPath }, tmp) => {
    writeFileSync(ledgerPath, `${JSON.stringify(EMPTY_LEDGER, null, 2)}\n`);
    const held = join(tmp, "held.flag");
    const go = join(tmp, "go.flag");
    const holderScript = join(tmp, "holder.mjs");
    const contenderScript = join(tmp, "contender.mjs");

    // The children import the shipped mutator, so this proves the real lock, not a re-implementation.
    writeFileSync(
      holderScript,
      `#!/usr/bin/env bun
import fs from "node:fs";
import { mutateKeeperLedgerLocked } from ${JSON.stringify(RELEASE_LOCAL_URL)};
mutateKeeperLedgerLocked({
  ledgerPath: ${JSON.stringify(ledgerPath)},
  mutate: () => {
    fs.writeFileSync(${JSON.stringify(held)}, "1");
    while (!fs.existsSync(${JSON.stringify(go)})) {}
    return null;
  },
});
process.stdout.write(JSON.stringify({ released: true }) + "\\n");
`,
    );
    writeFileSync(
      contenderScript,
      `#!/usr/bin/env bun
import { mutateKeeperLedgerLocked } from ${JSON.stringify(RELEASE_LOCAL_URL)};
try {
  mutateKeeperLedgerLocked({ ledgerPath: ${JSON.stringify(ledgerPath)}, mutate: () => null });
  process.stdout.write(JSON.stringify({ acquired: true }) + "\\n");
} catch (error) {
  process.stdout.write(
    JSON.stringify({ acquired: false, code: error.code, state: error.detail?.lock?.state }) + "\\n",
  );
}
`,
    );

    const holder = spawn(process.execPath, [holderScript], { stdio: ["ignore", "pipe", "pipe"] });
    // Subscribed before the release is signalled, so a holder that exits promptly cannot be missed.
    const exited = once(holder, "exit");
    try {
      // Handshake, not a delay: wait until the holder signals it is inside the critical section.
      const deadline = Date.now() + 30_000;
      while (!existsSync(held)) {
        if (Date.now() > deadline) throw new Error("the holder never entered its critical section");
      }
      assert.ok(existsSync(`${ledgerPath}.lock`), "the lock directory must exist while it is held");

      const contender = runChild(contenderScript);
      assert.equal(contender.acquired, false, "a second process must not acquire a held lock");
      assert.equal(contender.code, "KEEPER_LEDGER_LOCKED");
      assert.equal(contender.state, "live", "a live holder must be classified live, never stale");
    } finally {
      // Release the holder the way a publish releases it -- an orderly exit through the mutator's
      // `finally`. A SIGKILL would instead leave a lock the product deliberately reports as stale
      // and never reclaims, which is a different case (covered by the stale-lock tests above).
      writeFileSync(go, "1");
    }
    // Awaited exit, not a delay: the lock is gone only once the holder's `release()` has run.
    await exited;

    const after = runChild(contenderScript);
    assert.equal(after.acquired, true, "the lock must be acquirable once the holder is gone");
    assert.equal(existsSync(`${ledgerPath}.lock`), false, "acquire then release must leave no lock behind");
    // The key interop property: two processes serialized, and neither lost the other's work.
    assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 0, "a no-op mutation must not alter the ledger");
  });
});

test("reconcile refuses to withdraw while another process holds the lock, and writes nothing", async () => {
  // This is the regression test for the unlocked withdraw. The old code took no lock at all on the
  // withdraw path, so it would have rewritten the ledger here even though a writer held the lock --
  // silently reverting whatever that writer was in the middle of. The assertion that the ledger is
  // byte-identical afterwards is what makes the difference visible.
  await withFixture(async ({ ledgerPath }) => {
    const before = `${JSON.stringify(LEDGER_WITH, null, 2)}\n`;
    writeFileSync(ledgerPath, before);
    writeIntent(ledgerPath);
    mkdirSync(`${ledgerPath}.lock`);
    writeFileSync(join(`${ledgerPath}.lock`, "owner.json"), `${JSON.stringify({ pid: process.pid })}\n`);

    await assert.rejects(
      () =>
        reconcileKeeperPublication({
          ledgerPath,
          spawn: () => ({ status: 0, stdout: `{"isDraft":true,"tagName":"${TAG}"}`, stderr: "" }),
        }),
      (error) => {
        assert.equal(error.code, "KEEPER_LEDGER_LOCKED");
        return true;
      },
    );

    assert.equal(readFileSync(ledgerPath, "utf8"), before, "a locked withdraw must not touch the ledger");
    assert.ok(existsSync(`${ledgerPath}.pending`), "a locked withdraw must not clear the intent");
  });
});

test("withdraw removes only the unshipped release and keeps every other entry", async () => {
  // The withdraw decision must be a filter over the ledger read under the lock, not a rewrite of a
  // stale snapshot: entries recorded by other releases have to survive untouched.
  await withFixture(async ({ ledgerPath }) => {
    const ledger = {
      schemaVersion: 1,
      keeperReleases: [
        { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
        { appVersion: NEXT_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
      ],
    };
    writeFileSync(ledgerPath, `${JSON.stringify(ledger, null, 2)}\n`);
    // The intent names the NEWEST entry, because a crash can only leave the last append premature;
    // `reconcile` refuses to withdraw any other one (KEEPER_RECONCILE_NOT_NEWEST).
    writeIntent(ledgerPath, { appVersion: NEXT_VERSION, tag: `v${NEXT_VERSION}`, oldestSafeRollbackVersion: APP_VERSION });

    const result = await reconcileKeeperPublication({
      ledgerPath,
      spawn: () => ({ status: 0, stdout: `{"isDraft":true,"tagName":"v${NEXT_VERSION}"}`, stderr: "" }),
    });

    assert.equal(result.withdrew, true);
    assert.equal(result.appended, false);
    assert.deepEqual(readKeeperLedger(ledgerPath).keeperReleases, [
      { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
    ]);
    assert.equal(existsSync(`${ledgerPath}.pending`), false);
  });
});

test("a lock held by a provably dead process is reported stale and left in place", async () => {
  // Stale is REPORTED, never reclaimed: deleting another run's lock directory is destructive, and a
  // holder that is merely slow is indistinguishable from a dead one to an automated cleanup.
  await withFixture(async ({ ledgerPath }) => {
    writeFileSync(ledgerPath, `${JSON.stringify(EMPTY_LEDGER, null, 2)}\n`);
    mkdirSync(`${ledgerPath}.lock`);
    const deadPid = reapedPid();
    const ownerText = `${JSON.stringify({ pid: deadPid, at: "2026-11-16T00:00:00.000Z" })}\n`;
    writeFileSync(join(`${ledgerPath}.lock`, "owner.json"), ownerText);

    assert.throws(
      () => mutateKeeperLedgerLocked({ ledgerPath, mutate: () => null }),
      (error) => {
        assert.equal(error.code, "KEEPER_LEDGER_LOCKED");
        assert.equal(error.detail.lock.state, "stale");
        assert.equal(error.detail.lock.pid, deadPid);
        // The remedy must name the exact directory, warn about a running publish, and route to reconcile.
        assert.match(error.message, /rm -rf /);
        assert.match(error.message, /Never remove it while a publish is running/);
        assert.match(error.message, /reconcile/);
        return true;
      },
    );

    assert.ok(existsSync(`${ledgerPath}.lock`), "a stale lock must never be deleted automatically");
    assert.equal(readFileSync(join(`${ledgerPath}.lock`, "owner.json"), "utf8"), ownerText);
    assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 0, "a refused write must write nothing");
  });
});

test("a lock with no owner record is reported unknown, not guessed dead", async () => {
  // No owner.json means the holder died between `mkdir` and writing its record. The pid is unknown,
  // not proven gone, so this must not be reported as stale.
  await withFixture(async ({ ledgerPath }) => {
    writeFileSync(ledgerPath, `${JSON.stringify(EMPTY_LEDGER, null, 2)}\n`);
    mkdirSync(`${ledgerPath}.lock`);

    assert.throws(
      () => mutateKeeperLedgerLocked({ ledgerPath, mutate: () => null }),
      (error) => {
        assert.equal(error.code, "KEEPER_LEDGER_LOCKED");
        assert.equal(error.detail.lock.state, "unknown");
        assert.match(error.message, /no readable owner record/);
        return true;
      },
    );
    assert.ok(existsSync(`${ledgerPath}.lock`), "an unowned lock must not be deleted automatically");
  });
});

test("a lock whose owner record is unparseable is reported unknown and preserved byte for byte", async () => {
  await withFixture(async ({ ledgerPath }) => {
    writeFileSync(ledgerPath, `${JSON.stringify(EMPTY_LEDGER, null, 2)}\n`);
    mkdirSync(`${ledgerPath}.lock`);
    writeFileSync(join(`${ledgerPath}.lock`, "owner.json"), "{ truncated");

    assert.throws(
      () => mutateKeeperLedgerLocked({ ledgerPath, mutate: () => null }),
      (error) => {
        assert.equal(error.detail.lock.state, "unknown");
        return true;
      },
    );
    assert.equal(readFileSync(join(`${ledgerPath}.lock`, "owner.json"), "utf8"), "{ truncated");
  });
});

test("a lock recorded by this same process is classified live, not stale", async () => {
  // A re-entrant call is not evidence that the outer holder died, and killing it would corrupt a
  // ledger the outer frame is still editing.
  await withFixture(async ({ ledgerPath }) => {
    writeFileSync(ledgerPath, `${JSON.stringify(EMPTY_LEDGER, null, 2)}\n`);
    mkdirSync(`${ledgerPath}.lock`);
    writeFileSync(join(`${ledgerPath}.lock`, "owner.json"), `${JSON.stringify({ pid: process.pid })}\n`);

    assert.throws(
      () => mutateKeeperLedgerLocked({ ledgerPath, mutate: () => null }),
      (error) => {
        assert.equal(error.detail.lock.state, "live");
        return true;
      },
    );
  });
});

// ---------------------------------------------------------------------------------------------
// Same-version replacement guards.
//
// A release that is present in the ledger but carries a different floor than it published is the
// dangerous case: every "is it recorded?" check passes while users of that version are told they
// may roll back to a build the ledger does not guarantee. Nothing repairs this automatically,
// because rewriting a shipped floor is the corruption itself.
// ---------------------------------------------------------------------------------------------

test("reconcile refuses to append a public release whose ledger floor disagrees with the intent", async () => {
  await withFixture(async ({ ledgerPath }) => {
    // The ledger records the release, but with a floor that is not what it published. The recorded
    // floor has to be a version this release may actually roll back to (`parseKeeperLedger` refuses a
    // floor newer than its own appVersion), so the divergence is PRIOR_VERSION against APP_VERSION.
    writeFileSync(
      ledgerPath,
      `${JSON.stringify(
        {
          schemaVersion: 1,
          keeperReleases: [
            { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: PRIOR_VERSION },
          ],
        },
        null,
        2,
      )}\n`,
    );
    writeIntent(ledgerPath, { oldestSafeRollbackVersion: APP_VERSION });
    const before = readFileSync(ledgerPath);

    await assert.rejects(
      () =>
        reconcileKeeperPublication({
          ledgerPath,
          spawn: () => ({ status: 0, stdout: `{"isDraft":false,"tagName":"${TAG}"}`, stderr: "" }),
        }),
      (error) => {
        assert.equal(error.code, "KEEPER_LEDGER_FLOOR_DIVERGED");
        assert.equal(error.detail.published, APP_VERSION);
        assert.equal(error.detail.recorded, PRIOR_VERSION);
        assert.match(error.message, /Resolve by hand/);
        return true;
      },
    );

    // A divergence is reported, never repaired: the ledger bytes and the intent both survive.
    assert.ok(Buffer.compare(before, readFileSync(ledgerPath)) === 0, "a divergent floor must not be rewritten");
    assert.ok(existsSync(`${ledgerPath}.pending`), "the intent must survive so the operator can inspect it");
  });
});

test("reconcile refuses to withdraw a draft that is not the newest ledger entry", async () => {
  // A crash can only leave the LAST append premature. An intent naming an older entry means the
  // intent is misfiled or the ledger was hand-edited, and withdrawing would erase a shipped release.
  await withFixture(async ({ ledgerPath }) => {
    writeFileSync(
      ledgerPath,
      `${JSON.stringify(
        {
          schemaVersion: 1,
          keeperReleases: [
            { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
            { appVersion: NEXT_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
          ],
        },
        null,
        2,
      )}\n`,
    );
    // The intent names the OLDER of the two entries.
    writeIntent(ledgerPath, { appVersion: APP_VERSION, tag: TAG, oldestSafeRollbackVersion: APP_VERSION });
    const before = readFileSync(ledgerPath);

    await assert.rejects(
      () =>
        reconcileKeeperPublication({
          ledgerPath,
          spawn: () => ({ status: 0, stdout: `{"isDraft":true,"tagName":"${TAG}"}`, stderr: "" }),
        }),
      (error) => {
        assert.equal(error.code, "KEEPER_RECONCILE_NOT_NEWEST");
        assert.equal(error.detail.newest, NEXT_VERSION);
        assert.match(error.message, /nothing was changed/);
        return true;
      },
    );

    assert.ok(Buffer.compare(before, readFileSync(ledgerPath)) === 0, "a refused withdraw must not rewrite the ledger");
    assert.ok(existsSync(`${ledgerPath}.pending`), "the intent must survive a refused withdraw");
  });
});

test("reconcile withdraws the newest entry when it is the one the intent names", async () => {
  // The positive control for the guard above: the real crash shape must still be repairable.
  await withFixture(async ({ ledgerPath }) => {
    writeFileSync(
      ledgerPath,
      `${JSON.stringify(
        {
          schemaVersion: 1,
          keeperReleases: [
            { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
            { appVersion: NEXT_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
          ],
        },
        null,
        2,
      )}\n`,
    );
    writeIntent(ledgerPath, { appVersion: NEXT_VERSION, tag: `v${NEXT_VERSION}`, oldestSafeRollbackVersion: APP_VERSION });

    const result = await reconcileKeeperPublication({
      ledgerPath,
      spawn: () => ({ status: 0, stdout: `{"isDraft":true,"tagName":"v${NEXT_VERSION}"}`, stderr: "" }),
    });

    assert.equal(result.withdrew, true);
    assert.deepEqual(readKeeperLedger(ledgerPath).keeperReleases, [
      { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
    ]);
  });
});

test("reconcile appends a shipped release with the floor the intent recorded, not a re-derived one", async () => {
  // The appended entry must carry what was published, even though the ledger it is appended to has
  // an older floor available to derive from. Re-deriving would make the ledger disagree with the
  // `latest.json` that already shipped for this version.
  await withFixture(async ({ ledgerPath }) => {
    writeFileSync(
      ledgerPath,
      `${JSON.stringify(
        {
          schemaVersion: 1,
          keeperReleases: [
            { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
          ],
        },
        null,
        2,
      )}\n`,
    );
    writeIntent(ledgerPath, { appVersion: NEXT_VERSION, tag: `v${NEXT_VERSION}`, oldestSafeRollbackVersion: NEXT_VERSION });

    const result = await reconcileKeeperPublication({
      ledgerPath,
      spawn: () => ({ status: 0, stdout: `{"isDraft":false,"tagName":"v${NEXT_VERSION}"}`, stderr: "" }),
    });

    assert.equal(result.appended, true);
    assert.deepEqual(readKeeperLedger(ledgerPath).keeperReleases[1], {
      appVersion: NEXT_VERSION,
      protocolMajor: 1,
      oldestSafeRollbackVersion: NEXT_VERSION,
    });
  });
});

// ---------------------------------------------------------------------------------------------
// Ordering contract: the ledger append is bound to the floor this run already PUBLISHED.
//
// `keeperManifest` derives the floor from the ledger, so a derivation repeated at publish time is
// only equal to what shipped while the ledger state is unchanged. Between `assemble` (which writes
// `latest.json`) and `publish` (which appends), the ledger can gain entries -- a concurrent cut, a
// hand edit, a rebase. Recording the re-derived value would then leave the shipped manifest and the
// ledger stating different floors for the same version. The published manifest wins, and a
// disagreement is refused rather than silently resolved.
// ---------------------------------------------------------------------------------------------

test("publish records the floor its run already published", async () => {
  await withFixture(
    async ({ runDir, gh, ledgerPath }) => {
      recordKeeperInDsymManifest(runDir);
      // First keeper release: the ledger has an older entry, so the published floor is that entry's,
      // not this release's own version.
      writeFileSync(
        join(runDir, "publish", "latest.json"),
        JSON.stringify({ version: APP_VERSION, keeper: { oldestSafeRollbackVersion: APP_VERSION } }),
      );

      const result = await withoutCIMarkers(() => publishRelease(publishArgs(runDir, gh, ledgerPath)));

      assert.equal(result.keeperLedger.oldestSafeRollbackVersion, APP_VERSION);
      assert.deepEqual(readKeeperLedger(ledgerPath).keeperReleases, [
        { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
      ]);
    },
    { ledger: EMPTY_LEDGER },
  );
});

test("publish refuses when the ledger floor no longer matches the published manifest", async () => {
  // The divergence: this run published floor APP_VERSION, but by publish time the ledger already
  // records NEXT_VERSION with its own floor, so a fresh derivation yields NEXT_VERSION. Appending
  // that would contradict the manifest users install, so nothing may be published.
  const ledgerAtPublish = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: APP_VERSION },
      { appVersion: NEXT_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: NEXT_VERSION },
    ],
  };
  await withFixture(
    async ({ runDir, gh, ledgerPath }) => {
      recordKeeperInDsymManifest(runDir, NEXT_VERSION);
      // The run was assembled against the older ledger state, so it published APP_VERSION.
      const plan = JSON.parse(readFileSync(join(runDir, "plan.json"), "utf8"));
      plan.appVersion = NEXT_VERSION;
      plan.msixVersion = `${NEXT_VERSION}.0`;
      plan.tag = `v${NEXT_VERSION}`;
      writeFileSync(join(runDir, "plan.json"), JSON.stringify(plan));
      const state = JSON.parse(readFileSync(join(runDir, "prepare-state.json"), "utf8"));
      state.planDigest = createHash("sha256").update(readFileSync(join(runDir, "plan.json"))).digest("hex");
      writeFileSync(join(runDir, "prepare-state.json"), JSON.stringify(state));
      const assembly = JSON.parse(readFileSync(join(runDir, "assembly-state.json"), "utf8"));
      assembly.planDigest = state.planDigest;
      writeFileSync(join(runDir, "assembly-state.json"), JSON.stringify(assembly));

      writeFileSync(
        join(runDir, "publish", "latest.json"),
        JSON.stringify({ version: NEXT_VERSION, keeper: { oldestSafeRollbackVersion: APP_VERSION } }),
      );
      // The rewritten latest.json moves the publish inventory, so the run has to be re-sealed or
      // validateAssemblyState refuses it before the floor guard is ever reached.
      resealRun(runDir);
      const before = readFileSync(ledgerPath);

      await assert.rejects(
        () =>
          withoutCIMarkers(() =>
            publishRelease({
              ...publishArgs(runDir, gh, ledgerPath),
              lsRemoteFn: () => `${COMMIT}\trefs/tags/v${NEXT_VERSION}\n`,
            }),
          ),
        (error) => {
          assert.equal(error.code, "KEEPER_FLOOR_DIVERGED_FROM_PUBLISHED");
          assert.equal(error.detail.publishedFloor, APP_VERSION);
          assert.equal(error.detail.derivedFloor, NEXT_VERSION);
          return true;
        },
      );

      // Nothing public is left behind, and nothing was recorded.
      assert.ok(Buffer.compare(before, readFileSync(ledgerPath)) === 0, "a divergent floor must not be written");
      assert.equal(existsSync(`${ledgerPath}.pending`), false, "the divergence must be caught before the intent");
    },
    { ledger: ledgerAtPublish },
  );
});

test("publish refuses a run whose published manifest states no floor", async () => {
  // A manifest without the floor predates the keeper contract or was hand-written. Publishing it
  // would record a floor in the ledger that no shipped manifest states.
  await withFixture(async ({ runDir, gh, ledgerPath }) => {
    recordKeeperInDsymManifest(runDir);
    writeFileSync(join(runDir, "publish", "latest.json"), JSON.stringify({ version: APP_VERSION }));
    resealRun(runDir);

    await assert.rejects(
      () => withoutCIMarkers(() => publishRelease(publishArgs(runDir, gh, ledgerPath))),
      (error) => {
        assert.equal(error.code, "KEEPER_PUBLISHED_FLOOR_MISSING");
        return true;
      },
    );
    assert.equal(readKeeperLedger(ledgerPath).keeperReleases.length, 0);
  });
});

test("the ordering is acyclic: assemble needs the ledger committed, and publish only appends", async () => {
  // e5f3's build gate wants a clean ledger committed at HEAD; this tool appends only at publish.
  // Those compose only if the append is never a precondition of the build that produces it. Pin the
  // direction here: assemble on the pre-release ledger must pass, and the append must come after.
  const committedBeforeRelease = {
    schemaVersion: 1,
    keeperReleases: [{ appVersion: PRIOR_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: PRIOR_VERSION }],
  };
  await withFixture(
    async ({ ledgerPath }) => {
      // 1. Build-time state: ledger committed, containing only older releases. Assemble passes,
      //    because assembling does not require this release to be recorded yet.
      writeFileSync(ledgerPath, `${JSON.stringify(committedBeforeRelease, null, 2)}\n`);
      assert.doesNotThrow(() =>
        assertKeeperFloorDurable({ ledgerPath, readCommitted: () => `${JSON.stringify(committedBeforeRelease)}\n` }),
      );

      // 2. Publish-time state: the append is what records this release. Doing it before the build
      //    would be the circular requirement, which is why it lives here and nowhere else.
      mutateKeeperLedgerLocked({
        ledgerPath,
        mutate: (ledger) => recordKeeperRelease(ledger, { appVersion: APP_VERSION }),
      });
      assert.deepEqual(readKeeperLedger(ledgerPath).keeperReleases, [
        { appVersion: PRIOR_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: PRIOR_VERSION },
        { appVersion: APP_VERSION, protocolMajor: 1, oldestSafeRollbackVersion: PRIOR_VERSION },
      ]);

      // 3. That append is now uncommitted, so the NEXT assemble is refused until it is committed.
      //    The chain is acyclic: commit, build, publish, commit, build, ...
      assert.throws(
        () => assertKeeperFloorDurable({ ledgerPath, readCommitted: () => `${JSON.stringify(committedBeforeRelease)}\n` }),
        (error) => {
          assert.equal(error.code, "KEEPER_LEDGER_UNCOMMITTED");
          assert.deepEqual(error.detail.uncommitted, [APP_VERSION]);
          return true;
        },
      );
    },
  );
});
