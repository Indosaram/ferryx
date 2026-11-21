#!/usr/bin/env bun
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  closeSync,
  existsSync,
  fsyncSync,
  linkSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { inflateRawSync } from "node:zlib";

import { assembleRelease } from "./build-latest-json.mjs";
import { appendSurvivalReport, generateChangelog } from "./lib/release-changelog.mjs";
import { parsePlan, parseReceipt } from "./lib/release-contract.mjs";
import { loadHostConfig, redactProcessOutput } from "./lib/release-hosts.mjs";
import { buildHost, preflightAll } from "./lib/release-platforms.mjs";
import { assertSurvivalGate } from "./lib/release-survival.mjs";
import { assertSymbolicationGate } from "./lib/release-symbolication.mjs";
import {
  compareKeeperVersions,
  OLDEST_SAFE_ROLLBACK_KEY,
  keeperManifest,
  parseKeeperLedger,
  readKeeperLedger,
  recordKeeperRelease,
} from "./lib/release-keeper.mjs";

/** The checked-in keeper ledger that owns the oldest-safe-rollback floor. */
const KEEPER_LEDGER_PATH = resolve(dirname(fileURLToPath(import.meta.url)), "..", "packaging", "keeper", "releases.json");
import { writeHomebrewPackages } from "./release-packaging.mjs";
import {
  attachReleaseProvenance,
  generateSboms,
  PROVENANCE_ASSET_NAMES,
  PROVENANCE_NAME,
  verifyChecksums,
} from "./release-provenance.mjs";
import { assertReleaseTag } from "./release-tag-policy.mjs";
import { parseReleaseTag, toAppVersion, toMsixVersion } from "./sync-version.mjs";

function assertNotInCI(stageName) {
  if (process.env.GITHUB_ACTIONS === "true" || process.env.CI === "true") {
    throw new Error(
      `Mutating release stage '${stageName}' is forbidden under CI / GitHub Actions environment.`,
    );
  }
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

export const STORE_MSIX_NAME = "Ferryx_x64.msix";
export const STORE_PRODUCT_ID = "9NLHQL5JNLM4";


/** Typed refusal raised when a Windows build lacks the Store-ingestion MSIX at the plan version. */
export class StoreMsixError extends Error {
  constructor(code, message) {
    super(`${code}: ${message}`);
    this.name = "StoreMsixError";
    this.code = code;
  }
}

const ZIP_U16_MAX = 0xffff;
const ZIP_U32_MAX = 0xffffffff;

/**
 * Reads one entry from a zip (MSIX) archive; supports stored and deflated entries and ZIP64
 * archives (MakeAppx always writes MSIX packages as ZIP64: saturated EOCD fields, a ZIP64 EOCD
 * locator/record, and 0x0001 extended-information extra fields on central directory entries).
 */
function readZipEntry(buffer, entryName) {
  const eocdMin = buffer.length - 22;
  let eocd = -1;
  for (let i = eocdMin; i >= Math.max(0, eocdMin - 0xffff); i -= 1) {
    if (buffer.readUInt32LE(i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) return null;
  let entries = buffer.readUInt16LE(eocd + 10);
  let offset = buffer.readUInt32LE(eocd + 16);
  if (entries === ZIP_U16_MAX || offset === ZIP_U32_MAX) {
    const locator = eocd - 20;
    if (locator < 0 || buffer.readUInt32LE(locator) !== 0x07064b50) return null;
    const record = Number(buffer.readBigUInt64LE(locator + 8));
    if (buffer.readUInt32LE(record) !== 0x06064b50) return null;
    entries = Number(buffer.readBigUInt64LE(record + 32));
    offset = Number(buffer.readBigUInt64LE(record + 48));
  }
  for (let n = 0; n < entries; n += 1) {
    if (buffer.readUInt32LE(offset) !== 0x02014b50) return null;
    const method = buffer.readUInt16LE(offset + 10);
    let compressedSize = buffer.readUInt32LE(offset + 20);
    const uncompressedSize = buffer.readUInt32LE(offset + 24);
    const nameLength = buffer.readUInt16LE(offset + 28);
    const extraLength = buffer.readUInt16LE(offset + 30);
    const commentLength = buffer.readUInt16LE(offset + 32);
    let localOffset = buffer.readUInt32LE(offset + 42);
    const name = buffer.toString("utf8", offset + 46, offset + 46 + nameLength);
    // ZIP64 extended information: 8-byte values present only for saturated fields, in the
    // order uncompressed size, compressed size, local header offset.
    let extra = offset + 46 + nameLength;
    const extraEnd = extra + extraLength;
    while (extra + 4 <= extraEnd) {
      const id = buffer.readUInt16LE(extra);
      const size = buffer.readUInt16LE(extra + 2);
      if (id === 0x0001) {
        let field = extra + 4;
        if (uncompressedSize === ZIP_U32_MAX) field += 8;
        if (compressedSize === ZIP_U32_MAX) {
          compressedSize = Number(buffer.readBigUInt64LE(field));
          field += 8;
        }
        if (localOffset === ZIP_U32_MAX) localOffset = Number(buffer.readBigUInt64LE(field));
        break;
      }
      extra += 4 + size;
    }
    if (name === entryName) {
      const localNameLength = buffer.readUInt16LE(localOffset + 26);
      const localExtraLength = buffer.readUInt16LE(localOffset + 28);
      const start = localOffset + 30 + localNameLength + localExtraLength;
      const data = buffer.subarray(start, start + compressedSize);
      if (method === 0) return data;
      if (method === 8) return inflateRawSync(data);
      return null;
    }
    offset += 46 + nameLength + extraLength + commentLength;
  }
  return null;
}

export function storeSubmissionLine(plan) {
  return `Store submission pending: ${plan.msixVersion}`;
}

/**
 * Store gate: a run with a Windows build must carry the Store-ingestion MSIX (published as
 * Ferryx_x64.msix) whose package Identity version is the plan's canonical quad version. The
 * MSIX is located through the maho-win receipt, else at artifacts/windows/Ferryx_x64.msix.
 * Returns the release-summary line.
 */
export function assertStoreMsix({ runDir, plan }) {
  let msixPath = join(runDir, "artifacts", "windows", STORE_MSIX_NAME);
  const receiptPath = join(runDir, "receipts", "build-receipt-maho-win.json");
  if (existsSync(receiptPath)) {
    let artifact = null;
    try {
      artifact = JSON.parse(readFileSync(receiptPath, "utf8")).artifacts?.find((entry) => entry?.kind === "msix") ?? null;
    } catch {
      artifact = null;
    }
    msixPath = artifact && typeof artifact.relPath === "string" ? join(runDir, "artifacts", artifact.relPath) : null;
  }
  if (!msixPath || !existsSync(msixPath) || statSync(msixPath).size === 0) {
    throw new StoreMsixError(
      "STORE_MSIX_MISSING",
      `Windows build has no ${STORE_MSIX_NAME}${msixPath ? ` at ${msixPath}` : " in the maho-win receipt"}; every Windows release must produce the Store-ingestion MSIX (scripts/build-msix.ps1)`,
    );
  }
  let manifest = null;
  try {
    manifest = readZipEntry(readFileSync(msixPath), "AppxManifest.xml");
  } catch {
    manifest = null;
  }
  if (!manifest) {
    throw new StoreMsixError("STORE_MSIX_INVALID", `${STORE_MSIX_NAME} has no readable AppxManifest.xml`);
  }
  const identity = manifest.toString("utf8").match(/<Identity\b[^>]*>/);
  const version = identity?.[0].match(/\bVersion="([^"]*)"/)?.[1] ?? null;
  if (version !== plan.msixVersion) {
    throw new StoreMsixError(
      "STORE_MSIX_VERSION_MISMATCH",
      `${STORE_MSIX_NAME} Identity version is ${JSON.stringify(version)}, expected canonical quad ${plan.msixVersion}`,
    );
  }
  return storeSubmissionLine(plan);
}

function hasWindowsBuild(runDir) {
  const windowsDir = join(runDir, "artifacts", "windows");
  return (
    existsSync(join(runDir, "receipts", "build-receipt-maho-win.json")) ||
    (existsSync(windowsDir) && readdirSync(windowsDir).length > 0)
  );
}

function directoryDigest(directory) {
  const hash = createHash("sha256");
  const visit = (current) => {
    for (const name of readdirSync(current).sort()) {
      const path = join(current, name);
      if (statSync(path).isDirectory()) {
        visit(path);
      } else {
        hash.update(path.slice(directory.length + 1)).update("\0").update(readFileSync(path));
      }
    }
  };
  visit(directory);
  return hash.digest("hex");
}

function validatePreparedRun(runDir, configPath = null) {
  const planPath = join(runDir, "plan.json");
  const statePath = join(runDir, "prepare-state.json");
  if (!existsSync(planPath) || !existsSync(statePath)) {
    throw new Error(`Run directory missing plan.json or prepare-state.json: ${runDir}`);
  }
  const planBytes = readFileSync(planPath);
  const state = JSON.parse(readFileSync(statePath, "utf8"));
  if (sha256(planBytes) !== state.planDigest) {
    throw new Error("plan.json digest does not match prepare-state.json: plan was tampered with");
  }
  const effectiveConfigPath = resolve(configPath ?? state.configPath);
  if (effectiveConfigPath !== resolve(state.configPath)) {
    throw new Error("Host config path does not match the prepared release config");
  }
  if (sha256(readFileSync(effectiveConfigPath)) !== state.configDigest) {
    throw new Error("Host config digest does not match prepare-state.json: config was tampered with");
  }
  const plan = parsePlan(planBytes.toString("utf8"));
  if (plan.commitSha !== state.commitSha || plan.runId !== state.runId) {
    throw new Error("Prepared state identity does not match plan.json");
  }
  return { plan, state, configPath: effectiveConfigPath };
}

function validateAssemblyState(runDir) {
  const { plan, state } = validatePreparedRun(runDir);
  const assemblyPath = join(runDir, "assembly-state.json");
  if (!existsSync(assemblyPath)) throw new Error("assembly-state.json is missing; assemble the release first");
  const assembly = JSON.parse(readFileSync(assemblyPath, "utf8"));
  if (assembly.planDigest !== state.planDigest) throw new Error("Assembly plan digest mismatch");
  if (directoryDigest(join(runDir, "receipts")) !== assembly.receiptsDigest) {
    throw new Error("Release receipts changed after assembly");
  }
  if (directoryDigest(join(runDir, "artifacts")) !== assembly.artifactsDigest) {
    throw new Error("Release artifacts changed after assembly");
  }
  if (directoryDigest(join(runDir, "publish")) !== assembly.publishDigest) {
    throw new Error("Publish inventory changed after assembly");
  }
  return plan;
}

/**
 * Parses CLI flags and arguments.
 */
function parseCliArgs(argv) {
  const flags = new Set();
  const options = new Map();
  const positional = [];

  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token === "--help" || token === "-h") {
      flags.add("help");
    } else if (token.startsWith("--")) {
      const key = token.slice(2);
      const next = argv[i + 1];
      if (next !== undefined && !next.startsWith("--")) {
        options.set(key, next);
        i += 1;
      } else {
        flags.add(key);
      }
    } else {
      positional.push(token);
    }
  }

  return { positional, options, flags };
}

/**
 * Prepares an isolated release run directory and immutable release plan.
 */
export function prepareRelease({
  configPath,
  tag,
  commit = "HEAD",
  outDir,
  nsisMigration = false,
  changelog = generateChangelog,
}) {
  assertNotInCI("prepare");

  if (!configPath) throw new Error("Missing required argument: --config");
  if (!tag) throw new Error("Missing required argument: --tag");
  if (!commit) throw new Error("Missing required argument: --commit");
  if (!outDir) throw new Error("Missing required argument: --out");

  // New releases must use vYYYY.MMDD.N (scripts/release-tag-policy.mjs); checked before any
  // config, git, or build work so a malformed tag stops the run immediately.
  assertReleaseTag(tag);

  const resolvedOutDir = resolve(outDir);
  if (existsSync(resolvedOutDir)) {
    throw new Error(`Output run directory already exists: ${outDir}`);
  }

  const config = loadHostConfig(configPath);
  const repoDir = config.repository;
  const ghosttyDir = config.ghosttyRepository;

  // Validate release tag
  parseReleaseTag(tag);
  const appVersion = toAppVersion(tag);
  const msixVersion = toMsixVersion(tag);

  // Resolve commit SHA
  const commitSha = execFileSync(
    "git",
    ["rev-parse", "--verify", `${commit}^{commit}`],
    { cwd: repoDir, encoding: "utf8" },
  ).trim();

  if (!/^[0-9a-f]{40}$/.test(commitSha)) {
    throw new Error(`Resolved commit SHA is not 40-character hex: '${commitSha}'`);
  }

  // Resolve commit timestamp
  const tsRaw = execFileSync("git", ["log", "-1", "--format=%ct", commitSha], {
    cwd: repoDir,
    encoding: "utf8",
  }).trim();
  const sourceDateEpoch = parseInt(tsRaw, 10);
  if (Number.isNaN(sourceDateEpoch) || sourceDateEpoch <= 0) {
    throw new Error(`Failed to resolve commit timestamp for ${commitSha}`);
  }
  const createdAt = new Date(sourceDateEpoch * 1000).toISOString();

  // Extract expected Ghostty pin from source tree
  const rsContent = execFileSync(
    "git",
    ["show", `${commitSha}:src-tauri/native_terminal/build_ghostty.rs`],
    { cwd: repoDir, encoding: "utf8" },
  );
  const pinMatch = rsContent.match(/EXPECTED_GHOSTTY_SHA:\s*&str\s*=\s*"([0-9a-f]{40})"/);
  if (!pinMatch) {
    throw new Error("Could not extract EXPECTED_GHOSTTY_SHA from build_ghostty.rs");
  }
  const ghosttyPin = pinMatch[1];

  // Verify local Ghostty repository HEAD matches
  const ghosttyHead = execFileSync("git", ["rev-parse", "HEAD"], {
    cwd: ghosttyDir,
    encoding: "utf8",
  }).trim();
  if (ghosttyHead !== ghosttyPin) {
    throw new Error(
      `Ghostty HEAD mismatch: local Ghostty repository is at ${ghosttyHead}, but source requires ${ghosttyPin}`,
    );
  }

  // Extract updater public key from committed tauri.conf.json
  const tauriConfRaw = execFileSync(
    "git",
    ["show", `${commitSha}:src-tauri/tauri.conf.json`],
    { cwd: repoDir, encoding: "utf8" },
  );
  const tauriConf = JSON.parse(tauriConfRaw);
  const updaterPubkey = tauriConf.plugins?.updater?.pubkey;
  if (!updaterPubkey) {
    throw new Error("plugins.updater.pubkey missing from committed src-tauri/tauri.conf.json");
  }

  const runId = `rel-${tag.replace(/^v/, "")}-${commitSha.slice(0, 8)}`;
  const channels = {
    store: true,
    nsisMigration: Boolean(nsisMigration),
  };

  const requiredTargets = Boolean(nsisMigration)
    ? ["darwin-aarch64", "darwin-x86_64", "linux-x86_64", "windows-x86_64"]
    : ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"];

  const plan = {
    schemaVersion: 1,
    runId,
    repo: config.repo,
    commitSha,
    tag,
    appVersion,
    msixVersion,
    channels,
    requiredTargets,
    toolchains: { node: ">=22.0.0", bun: ">=1.4.0", zig: "0.17.0" },
    sourceDateEpoch,
    createdAt,
  };

  // Strict contract validation
  parsePlan(plan);

  // Materialize run directory structure
  mkdirSync(resolvedOutDir, { recursive: true });
  mkdirSync(join(resolvedOutDir, "artifacts", "darwin"), { recursive: true });
  mkdirSync(join(resolvedOutDir, "artifacts", "linux"), { recursive: true });
  mkdirSync(join(resolvedOutDir, "artifacts", "windows"), { recursive: true });
  mkdirSync(join(resolvedOutDir, "receipts"), { recursive: true });
  mkdirSync(join(resolvedOutDir, "logs"), { recursive: true });
  mkdirSync(join(resolvedOutDir, "bundles"), { recursive: true });

  const planContent = JSON.stringify(plan, null, 2) + "\n";
  const planDigest = sha256(planContent);
  const configContent = readFileSync(configPath, "utf8");
  const configDigest = sha256(configContent);

  const prepareState = {
    schemaVersion: 1,
    runId,
    planDigest,
    configDigest,
    configPath: resolve(configPath),
    commitSha,
    sourceDateEpoch,
    ghosttyPin,
    updaterPublicKey: updaterPubkey,
  };

  const sourceInputs = {
    commitSha,
    sourceDateEpoch,
    ghosttyPin,
    ghosttyHead,
  };

  writeFileSync(join(resolvedOutDir, "plan.json"), planContent);
  writeFileSync(join(resolvedOutDir, "prepare-state.json"), JSON.stringify(prepareState, null, 2) + "\n");
  writeFileSync(join(resolvedOutDir, "source-inputs.json"), JSON.stringify(sourceInputs, null, 2) + "\n");

  try {
    const notes = changelog({
      repo: config.repo,
      tag,
      commitSha,
    });
    writeFileSync(join(resolvedOutDir, "RELEASE_NOTES.md"), notes);
  } catch {
    // Fail-open: changelog generation failure never aborts prepare
  }

  return { plan, outDir: resolvedOutDir, planDigest, configDigest };
}

/**
 * Runs preflight environmental and toolchain probes across all configured hosts.
 */
export async function preflightRelease({ configPath, planPath = null, options = {} }) {
  if (!configPath) throw new Error("Missing required argument: --config");
  if (planPath) {
    const resolvedPlanPath = resolve(planPath);
    if (basename(resolvedPlanPath) !== "plan.json") {
      throw new Error("Prepared release plan must be named plan.json");
    }
    validatePreparedRun(dirname(resolvedPlanPath), configPath);
  }
  const config = loadHostConfig(configPath);
  const res = await preflightAll(config, options);
  return res;
}

/**
 * Orchestrates platform build on a specific host.
 */
export async function buildRelease({
  configPath,
  runDir,
  host,
  approveNotarization = false,
  runner = null,
}) {
  assertNotInCI("build");

  if (!configPath) throw new Error("Missing required argument: --config");
  if (!runDir) throw new Error("Missing required argument: --run");
  if (!host) throw new Error("Missing required argument: --host");

  const resolvedRunDir = resolve(runDir);
  validatePreparedRun(resolvedRunDir, configPath);
  const config = loadHostConfig(configPath);

  return buildHost({
    hostName: host,
    config,
    runDir: resolvedRunDir,
    approveNotarization,
    runner,
  });
}

/**
 * Keeper facts recorded by the macOS build in `dsyms/manifest.json`, shaped for provenance.
 *
 * Returns null when the run has no such manifest (Linux/Windows-only runs, older run directories),
 * which keeps `provenance.json` additive instead of making every existing run fail to assemble.
 *
 * The rollback floor is NOT derived here: `scripts/lib/release-keeper.mjs` owns it, sourced from
 * the checked-in ledger so the published `latest.json` and `provenance.json` cannot disagree.
 */
function readKeeperProvenance(runDir, plan, ledgerPath = KEEPER_LEDGER_PATH) {
  const manifestPath = join(runDir, "dsyms", "manifest.json");
  if (!existsSync(manifestPath)) return null;
  let keeper;
  try {
    keeper = JSON.parse(readFileSync(manifestPath, "utf8")).keeper;
  } catch {
    return null;
  }
  if (!keeper || typeof keeper.version !== "string") return null;
  const manifest = keeperManifest({ appVersion: plan.appVersion, ledger: readKeeperLedger(ledgerPath) });
  return {
    ...keeper,
    oldestSafeRollbackVersion: manifest.oldestSafeRollbackVersion,
    retainedGenerations: manifest.retainedGenerations,
  };
}

/**
 * Refuses to record a floor that is not the floor this run already published.
 *
 * This is the call site the append needed. `keeperManifest` derives the floor from the ledger, so a
 * re-derivation is only equal to what shipped when the ledger state is identical -- and by publish
 * time it need not be, because the ledger may have gained entries since `assemble` wrote
 * `latest.json`. Recording a freshly derived floor would then put a value in the ledger that
 * disagrees with the `latest.json` already sitting in the publish directory, and every later release
 * would derive its floor from the ledger's side of that disagreement.
 *
 * The published manifest is the authority: it is what users install and what the bundle ships, so
 * the append must match it exactly instead of recomputing it.
 */
export function assertKeeperFloorMatchesPublished({ runDir, plan, ledgerPath = KEEPER_LEDGER_PATH }) {
  const latestPath = join(runDir, "publish", "latest.json");
  if (!existsSync(latestPath)) {
    throw keeperLedgerError(
      "KEEPER_PUBLISHED_MANIFEST_MISSING",
      `${latestPath} does not exist, so there is no published floor to record. Run assemble before publish.`,
      { path: latestPath },
    );
  }
  let published;
  try {
    published = JSON.parse(readFileSync(latestPath, "utf8"))?.keeper;
  } catch {
    published = null;
  }
  const publishedFloor = published?.[OLDEST_SAFE_ROLLBACK_KEY];
  if (!publishedFloor) {
    throw keeperLedgerError(
      "KEEPER_PUBLISHED_FLOOR_MISSING",
      `${latestPath} carries no keeper.${OLDEST_SAFE_ROLLBACK_KEY}, so this release published no rollback floor. ` +
        "Publishing it would record a floor in the ledger that no shipped manifest states.",
      { path: latestPath },
    );
  }
  const keeper = readKeeperProvenance(runDir, plan, ledgerPath);
  if (!keeper) return { publishedFloor, derivedFloor: null, checked: false };
  if (compareKeeperVersions(keeper.oldestSafeRollbackVersion, publishedFloor) !== 0) {
    throw keeperLedgerError(
      "KEEPER_FLOOR_DIVERGED_FROM_PUBLISHED",
      `${latestPath} publishes oldestSafeRollbackVersion ${publishedFloor}, but the ledger now derives ` +
        `${keeper.oldestSafeRollbackVersion} for ${plan.appVersion}. The ledger changed after assemble. ` +
        "Recording the derived value would contradict the manifest users install. Re-run assemble so both agree.",
      {
        path: latestPath,
        appVersion: plan.appVersion,
        publishedFloor,
        derivedFloor: keeper.oldestSafeRollbackVersion,
      },
    );
  }
  return { publishedFloor, derivedFloor: keeper.oldestSafeRollbackVersion, checked: true };
}

/**
 * Appends this release to the checked-in keeper ledger. Called BEFORE the release goes public.
 *
 * The ledger is the ONLY source of the rollback floor, and `keeperManifest` derives that floor from
 * its OLDEST entry. Without an append the ledger stays empty, so every release publishes itself as
 * the floor: the floor then advances on every cut and a user can never roll back even one version.
 * Recording the release is what pins the floor at the first keeper-capable build.
 *
 * The append happens while the release is still a draft (see `prepareKeeperPublication`), so a
 * failure throws while it is still recoverable and the release never goes public with an
 * unrecorded ledger. It writes the working tree, not a commit: the coordinator never commits on the
 * operator's behalf, so the returned path is reported for review and commit.
 *
 * `ledgerPath` is a seam, not a feature: tests must never rewrite the checked-in ledger, so they
 * point this at a temporary file. Production always takes the default.
 */
function recordKeeperReleaseInLedger({ runDir, plan, ledgerPath = KEEPER_LEDGER_PATH }) {
  const keeper = readKeeperProvenance(runDir, plan, ledgerPath);
  if (!keeper) return null; // no macOS build in this run, so no keeper generation was shipped
  const { changed } = mutateKeeperLedgerLocked({
    ledgerPath,
    retryHint: "Re-run publish to re-read the ledger and append on top of it",
    mutate: (ledger) => {
      // Pin the floor that this release's `latest.json` already published, rather than letting
      // `recordKeeperRelease` re-derive it from the ledger it is reading. Re-deriving under the lock
      // reads a ledger that may have grown since the manifest was assembled, which would record a
      // floor that disagrees with the shipped `latest.json` for the same version -- a same-version
      // divergence in which the ledger and the published floor say different things.
      const recorded = recordKeeperRelease(ledger, {
        appVersion: plan.appVersion,
        oldestSafeRollbackVersion: keeper.oldestSafeRollbackVersion,
      });
      // Returning null means "already correct": an idempotent append must not rewrite the bytes.
      return JSON.stringify(recorded) === JSON.stringify(parseKeeperLedger(ledger)) ? null : recorded;
    },
  });
  return { path: ledgerPath, changed, oldestSafeRollbackVersion: keeper.oldestSafeRollbackVersion };
}

/**
 * The single read-modify-write path for the keeper ledger: every writer goes through here.
 *
 * Two writers racing without it would each read the same ledger and each write their own version,
 * so the later rename wins and silently drops the other release -- un-recording a shipped keeper and
 * letting the floor walk forward. The lock is a directory because `mkdir` is atomic and fails with
 * EEXIST, so exactly one holder can exist without depending on advisory locking.
 *
 * `mutate` receives the ledger as read UNDER the lock and returns the replacement, or null for "no
 * change". Reading under the lock is the whole point: a caller that inspected the ledger before
 * taking the lock would act on a snapshot that another writer may already have superseded, and its
 * rewrite would then silently revert that writer (this is exactly the withdraw-after-append bug).
 *
 * The post-mutation re-read guards writers that bypass the lock entirely -- a hand edit, a
 * `git checkout`, or an older build of this tool. It cannot catch those via the lock, so it compares
 * the authority of the write against the state actually on disk immediately before the rename.
 */
// Exported for tests: the lock interop and stale-lock behaviour can only be proven against the real
// mutator, and re-implementing it in a test would prove nothing about the shipped path.
export function mutateKeeperLedgerLocked({ ledgerPath, mutate, retryHint = "Re-run the command" }) {
  const lockPath = `${ledgerPath}.lock`;
  const release = acquireKeeperLedgerLock(lockPath);
  try {
    const ledger = readKeeperLedger(ledgerPath);
    const before = JSON.stringify(parseKeeperLedger(ledger));
    const next = mutate(ledger);
    if (next === null || next === undefined) return { changed: false };
    // Parse before writing: a mutator that produced a ledger violating the floor/ordering rules
    // must fail here, while the destination is still untouched, not after the bytes have landed.
    const nextText = `${JSON.stringify(parseKeeperLedger(next), null, 2)}\n`;
    const current = JSON.stringify(parseKeeperLedger(readKeeperLedger(ledgerPath)));
    if (current !== before) {
      throw keeperLedgerError(
        "KEEPER_LEDGER_CONCURRENT_MODIFICATION",
        `${ledgerPath} changed while this process held its lock, so a writer that ignores the lock ` +
          `modified it; refusing to overwrite that write. ${retryHint}.`,
        { path: ledgerPath },
      );
    }
    writeFileDurably(ledgerPath, nextText);
    return { changed: true };
  } finally {
    release();
  }
}

/**
 * Classifies an existing lock without modifying it.
 *
 * Three answers, and the difference matters because the remediation is destructive:
 *  - "live": the recorded pid is alive, or the record is unreadable. Refuse; the holder may be
 *    mid-write, and a lock that is merely slow looks identical to a stolen one from here.
 *  - "stale": the recorded pid is provably gone, so nobody can still be writing through this lock.
 *  - "unknown": no readable pid record (killed before it was written, or hand-made).
 *
 * A pid is only ever called dead when the kernel says ESRCH. `EPERM` means the process exists but is
 * not ours to signal, which is still alive. Pid reuse can only make a dead holder look alive (a
 * refusal, safe); it cannot make a live holder look dead, because the holder recorded its own live
 * pid -- so "stale" is never the direction that clobbers an active writer.
 */
export function inspectKeeperLedgerLock(lockPath) {
  let owner = null;
  try {
    owner = JSON.parse(readFileSync(join(lockPath, "owner.json"), "utf8"));
  } catch {
    // Missing or unparseable: still a lock, just one that cannot name its holder.
  }
  const pid = owner && Number.isInteger(owner.pid) ? owner.pid : null;
  const at = owner && typeof owner.at === "string" ? owner.at : null;
  if (pid === null) return { state: "unknown", pid: null, at, owner: null };
  if (pid === process.pid) return { state: "live", pid, at, owner, self: true };
  let alive;
  try {
    process.kill(pid, 0);
    alive = true;
  } catch (error) {
    alive = error.code === "EPERM";
  }
  return { state: alive ? "live" : "stale", pid, at, owner };
}

/**
 * Serializes ledger writers with an atomic `mkdir`.
 *
 * Returns a release function. This never steals and never deletes: reclaiming a lock whose holder is
 * merely slow would let two writers interleave, which is precisely the corruption the lock exists to
 * prevent. Instead the refusal is classified (see `inspectKeeperLedgerLock`) so the operator gets the
 * one command that applies. Deleting the directory is left to them because only they can confirm no
 * publish is running -- but the message never leaves a blocked release unexplained.
 */
function acquireKeeperLedgerLock(lockPath) {
  try {
    mkdirSync(lockPath);
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
    const held = inspectKeeperLedgerLock(lockPath);
    const remedy = {
      live: "Wait for that publish to finish. Do not remove the directory: a live holder may be mid-write.",
      stale:
        `Its process is gone, so the lock is stale. Confirm nothing is running, then remove it with ` +
        `\`rm -rf ${lockPath}\`. Never remove it while a publish is running. ` +
        "After removing a stale lock, run `reconcile` before the next publish: the killed run may have " +
        "left an intent or a ledger entry that also needs settling.",
      unknown:
        "The lock has no readable owner record, so it is either stale or a publish killed before it " +
        `recorded itself. Confirm with \`ps\`, then remove it with \`rm -rf ${lockPath}\` and run \`reconcile\`.`,
    }[held.state];
    throw keeperLedgerError(
      "KEEPER_LEDGER_LOCKED",
      `${lockPath} already exists, so another publish is recording a keeper release right now ` +
        `(${held.pid === null ? "holder unknown" : `pid ${held.pid}${held.at ? ` since ${held.at}` : ""}`}, ` +
        `state ${held.state}). ${remedy}`,
      { path: lockPath, lock: held },
    );
  }
  try {
    writeFileSync(join(lockPath, "owner.json"), `${JSON.stringify({ pid: process.pid, at: new Date().toISOString() })}\n`);
  } catch {
    // The owner record is diagnostics only; the lock itself is the mkdir, which already succeeded.
  }
  return () => rmSync(lockPath, { recursive: true, force: true });
}

/**
 * Records the intent to publish, then the ledger append, and only then lets the release go public.
 *
 * Order is the whole point. The ledger and GitHub cannot be updated atomically, so one of the two
 * orderings must be chosen for the crash in between:
 *
 *  - Undraft first, append after (the previous behaviour): a crash leaves a PUBLIC release whose
 *    append was never written. `keeperManifest` then derives the floor with its
 *    `?? appVersion` fallback on an empty ledger, so the floor re-floors onto every release and no
 *    user can roll back even one version -- silently, and unfixable for that tag afterwards because
 *    `latest.json` is already covered by its own SHA256SUMS.
 *  - Intent + append first, undraft after (this function): a crash leaves a ledger entry for a
 *    release that may still be a draft. The floor stays pinned (the safe direction for the floor
 *    rule), and the intent file makes the gap visible so a later cut cannot proceed on a guess.
 *
 * The intent is written before the ledger so that any state change is already explained by a file
 * on disk. Errors propagate: a failure here happens before the irreversible step, so throwing is
 * recoverable and is the fail-closed choice.
 */
function prepareKeeperPublication({ runDir, plan, ledgerPath = KEEPER_LEDGER_PATH }) {
  const keeper = readKeeperProvenance(runDir, plan, ledgerPath);
  if (!keeper) return null; // no macOS build in this run, so no keeper generation was shipped
  // Bind the append to the floor this run already published, BEFORE anything becomes public and
  // before the intent is written. If the ledger moved since assemble, the two disagree and the only
  // safe outcome is to stop: recording the newly derived floor here would leave the shipped
  // `latest.json` and the ledger stating different floors for the same version, and every later
  // release derives its floor from the ledger. Re-running assemble is the fix, and it is cheap.
  assertKeeperFloorMatchesPublished({ runDir, plan, ledgerPath });
  // Non-clobbering: an intent already on disk belongs to an earlier unfinished publish, and
  // erasing it is exactly how a stuck publish would be laundered into a clean-looking one.
  writeKeeperIntentOnce(ledgerPath, {
    schema: "ferryx-keeper-publish-intent/1",
    appVersion: plan.appVersion,
    tag: plan.tag,
    repo: plan.repo,
    commitSha: plan.commitSha,
    // The floor this release actually published. Recorded here so a later reconcile can tell
    // "this ledger entry is the one that shipped" from "this ledger entry disagrees with what
    // shipped for the same version" without re-deriving a floor that may have moved since.
    oldestSafeRollbackVersion: keeper.oldestSafeRollbackVersion,
  });
  try {
    return recordKeeperReleaseInLedger({ runDir, plan, ledgerPath });
  } catch (error) {
    // The append is what the intent explains, so a refusal that happened BEFORE the write -- another
    // publish holding the lock, a writer that ignored it, a mutation that failed to parse, a write
    // that failed before its rename -- leaves the ledger exactly as it was. The intent we just wrote
    // would then describe a publication that provably never started, and every later publish would
    // be refused (and told to run `reconcile`) over state that does not exist.
    //
    // Only removed when the ledger PROVES nothing was recorded: when the entry did land, the intent
    // is the crash evidence `reconcile` needs to withdraw a premature append, so it must survive.
    let recorded = true;
    try {
      recorded = readKeeperLedger(ledgerPath).keeperReleases.some((release) => release.appVersion === plan.appVersion);
    } catch {
      // An unreadable ledger is not proof that nothing was appended; leave the intent in place.
    }
    if (!recorded) clearKeeperIntent(ledgerPath);
    throw error;
  }
}

/**
 * Makes a directory's entries durable.
 *
 * A rename or unlink is metadata: without an fsync on the directory itself the change can be lost
 * on power failure while the file's data blocks survive, which shows up as "the file is there but
 * is the old version" (or, for an unlink, "the intent is back").
 */
function fsyncDirectory(dir, fsync = fsyncSync) {
  let fd;
  try {
    fd = openSync(dir, "r");
    fsync(fd);
  } catch {
    // Directory fsync is unsupported on some platforms (notably Windows); the rename/unlink is
    // still atomic there, so the loss is durability under power failure, not correctness.
  } finally {
    if (fd !== undefined) closeSync(fd);
  }
}

/**
 * `writeFileSync` truncates in place, so a crash mid-write leaves a half-written ledger. Here the
 * bytes go to a sibling temp file that is fsynced before the rename: until the rename lands the
 * destination is untouched, and after it the destination is complete. The directory fsync is what
 * makes the rename itself durable on APFS, because a rename is metadata.
 */
export function writeFileDurably(path, text, { fsyncSync: fsync = fsyncSync } = {}) {
  const dir = dirname(path);
  const temp = join(dir, `.${basename(path)}.${process.pid}.tmp`);
  const fd = openSync(temp, "w", 0o644);
  try {
    writeFileSync(fd, text);
    fsync(fd);
  } catch (error) {
    // A failure before the rename leaves the destination untouched; drop the partial temp file so
    // the next attempt does not inherit debris.
    try {
      closeSync(fd);
    } finally {
      rmSync(temp, { force: true });
    }
    throw error;
  }
  closeSync(fd);
  renameSync(temp, path);
  // Persist the directory entry, not just the file.
  fsyncDirectory(dir, fsync);
}

/**
 * Where an in-flight publish records what it is about to make public.
 *
 * The ledger append cannot be atomic with `gh release edit --draft=false`: they are different
 * systems. So the append is written BEFORE the release goes public and the intent file records
 * that a publication of `appVersion` is in flight. If the process dies between the two, the intent
 * survives and the next `assemble` sees a release that is recorded as shipped but that no public
 * release confirms -- or, if the undraft succeeded, an intent that was never cleared. Both are
 * reported rather than guessed at.
 */
function keeperIntentPath(ledgerPath) {
  return `${ledgerPath}.pending`;
}

function readKeeperIntent(ledgerPath) {
  const path = keeperIntentPath(ledgerPath);
  if (!existsSync(path)) return null;
  try {
    const parsed = JSON.parse(readFileSync(path, "utf8"));
    return parsed && typeof parsed === "object" ? parsed : null;
  } catch {
    // An unparseable intent is still evidence that a publish died mid-flight; treat it as present
    // but nameless so callers fail closed instead of silently ignoring it.
    return { appVersion: null, tag: null, unreadable: true };
  }
}

function clearKeeperIntent(ledgerPath) {
  const path = keeperIntentPath(ledgerPath);
  rmSync(path, { force: true });
  // An unlink is metadata: without the directory fsync the entry can reappear after a power
  // failure, which would make a completed publish look unfinished forever.
  fsyncDirectory(dirname(path));
}

/**
 * Records the publish intent, refusing to overwrite one that is already there.
 *
 * `writeFileDurably` goes through a rename, which silently replaces the destination. Overwriting a
 * live intent would erase the only on-disk evidence of an *earlier* unfinished publish -- including
 * the appVersion it recorded -- so a second run could launder a stuck publish into a clean-looking
 * one. The check is `link`-based rather than `existsSync`-then-write so two concurrent publishes
 * cannot both pass: `linkSync` fails with EEXIST for exactly one winner.
 */
function writeKeeperIntentOnce(ledgerPath, intent) {
  const path = keeperIntentPath(ledgerPath);
  const text = `${JSON.stringify(intent, null, 2)}\n`;
  const staged = join(dirname(path), `.${basename(path)}.${process.pid}.staged`);
  writeFileSync(staged, text, { flag: "wx", mode: 0o644 });
  try {
    linkSync(staged, path);
    fsyncDirectory(dirname(path));
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
    const existing = readKeeperIntent(ledgerPath);
    throw keeperLedgerError(
      "KEEPER_PUBLISH_IN_FLIGHT",
      `${path} already records an unfinished publish${existing?.appVersion ? ` of ${existing.appVersion}` : ""}. ` +
        "Refusing to overwrite it: it is the only evidence of the earlier attempt. " +
        "Settle it first with `reconcile`, then re-run publish.",
      { path, intent: existing },
    );
  } finally {
    rmSync(staged, { force: true });
  }
}

function keeperLedgerError(code, message, detail = {}) {
  const error = new Error(`${code}: ${message}`);
  error.code = code;
  error.detail = detail;
  return error;
}

/** The ledger as committed at HEAD, or null when git has no such file (first keeper release). */
function readCommittedKeeperLedger(ledgerPath) {
  const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const relativePath = relative(repoRoot, ledgerPath);
  if (relativePath.startsWith("..")) {
    throw keeperLedgerError(
      "KEEPER_LEDGER_OUTSIDE_REPO",
      `${ledgerPath} is outside ${repoRoot}, so its committed state cannot be checked`,
    );
  }
  // `git show HEAD:<path>` is the only read that distinguishes "recorded and committed" from
  // "written to this checkout". A dirty working tree is exactly what must not seed a published
  // floor, because the next clone would not see it.
  const result = spawnSync("git", ["show", `HEAD:${relativePath.split(sep).join("/")}`], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  if (result.status === 0) return result.stdout;
  // Absent from HEAD is the normal first-release state, not an error; any other failure is.
  if (result.error) {
    throw keeperLedgerError(
      "KEEPER_LEDGER_UNREADABLE",
      `cannot run git to read ${relativePath} at HEAD: ${result.error.message}`,
    );
  }
  const stderr = result.stderr ?? "";
  if (/does not exist in|exists on disk, but not in|unknown revision|path '.*' does not exist/.test(stderr)) return null;
  if (/not a git repository/i.test(stderr)) {
    throw keeperLedgerError("KEEPER_LEDGER_NOT_A_REPO", `${repoRoot} is not a git repository, so the floor cannot be verified`);
  }
  throw keeperLedgerError("KEEPER_LEDGER_UNREADABLE", `cannot read ${relativePath} at HEAD: ${stderr.trim()}`);
}

/**
 * Refuses to derive a published rollback floor from a ledger that exists only in this checkout.
 *
 * `recordKeeperReleaseInLedger` writes the working tree by design -- the coordinator never commits
 * on the operator's behalf -- so unless that append is committed, the floor it records is lost at
 * the next clone. A lost append is not a visible failure: the next release reads an empty ledger
 * and publishes ITSELF as the floor, so the floor walks forward on every cut and no user can roll
 * back even one version. That degradation is silent, which is why it is refused here instead of
 * being left to a runbook step.
 *
 * Only entries already on disk are checked: the first keeper release legitimately starts with an
 * empty ledger, and appending to it is publish's job, not a precondition of assembling.
 */
export function assertKeeperFloorDurable({
  ledgerPath = KEEPER_LEDGER_PATH,
  readCommitted = readCommittedKeeperLedger,
} = {}) {
  if (!existsSync(ledgerPath)) {
    throw keeperLedgerError(
      "KEEPER_LEDGER_MISSING",
      `no keeper ledger at ${ledgerPath}; a release that cannot state its rollback floor cannot be assembled`,
    );
  }
  // A leftover intent means a publish died between recording the ledger and confirming the release
  // was public. The ledger entry may describe a release that is still a draft, so the floor it
  // implies is not yet real. Refusing here is what stops the next cut from building on that doubt:
  // either finish the publish, or delete the intent deliberately once resolved.
  const intent = readKeeperIntent(ledgerPath);
  if (intent) {
    throw keeperLedgerError(
      "KEEPER_PUBLISH_UNFINISHED",
      `${keeperIntentPath(ledgerPath)} exists, so a previous publish of ${intent.appVersion ?? "(unreadable intent)"} never confirmed its release went public. ` +
        "Resolve it first: re-run `publish` for that tag (the ledger append is idempotent), or, if the release was already undrafted, delete the intent file. " +
        "Assembling now would derive the next floor from a release that may not exist.",
      { path: keeperIntentPath(ledgerPath), intent },
    );
  }
  const onDisk = readKeeperLedger(ledgerPath);
  if (onDisk.keeperReleases.length === 0) {
    return { entries: 0, uncommitted: [] };
  }
  const committedText = readCommitted(ledgerPath);
  const committed = committedText === null ? { keeperReleases: [] } : parseKeeperLedger(committedText);
  // Compare FLOOR VALUES, not just versions. The committed ledger is what actually publishes the
  // floor, so a committed entry for the same appVersion carrying a different
  // oldestSafeRollbackVersion is a divergence, not a match: checking versions alone would accept a
  // working-tree edit that moves the floor forward while the release is assembled, which is exactly
  // the silent-advance failure this gate exists to prevent.
  const committedByVersion = new Map(committed.keeperReleases.map((release) => [release.appVersion, release]));
  const uncommitted = onDisk.keeperReleases
    .filter((release) => {
      const atHead = committedByVersion.get(release.appVersion);
      if (!atHead) return true;
      return (
        atHead.oldestSafeRollbackVersion !== release.oldestSafeRollbackVersion ||
        atHead.protocolMajor !== release.protocolMajor
      );
    })
    .map((release) => release.appVersion);
  if (uncommitted.length > 0) {
    throw keeperLedgerError(
      "KEEPER_LEDGER_UNCOMMITTED",
      `${ledgerPath} records ${uncommitted.join(", ")} but ${committedByVersion.size === 0 ? "no keeper release is" : "not all of them are"} committed at HEAD with the same floor. ` +
        "Committing the ledger append is what keeps oldestSafeRollbackVersion pinned; assembling now would publish a floor the next clone cannot reproduce, " +
        "and the following release would advance the floor instead of preserving it.",
      { path: ledgerPath, uncommitted, committedVersions: [...committedByVersion.keys()] },
    );
  }
  return { entries: onDisk.keeperReleases.length, uncommitted: [] };
}

/**
 * Assembles and verifies release manifest, deterministic aliases, and SHA256SUMS.txt, then
 * renders the Homebrew cask and formula from SHA256SUMS.txt, then attaches provenance:
 * CycloneDX SBOMs, provenance.json, SHA256SUMS and SHA256SUMS.minisig (signed with the
 * updater key from TAURI_SIGNING_PRIVATE_KEY[_PATH]), so the Homebrew files are signed too.
 */
export function assembleReleaseRun({
  runDir,
  pubkey = null,
  env = process.env,
  minisignBin,
  sbomGenerator = generateSboms,
  sourceRepo = null,
  keeperLedgerPath = KEEPER_LEDGER_PATH,
  assertFloorDurable = assertKeeperFloorDurable,
}) {
  assertNotInCI("assemble");

  if (!runDir) throw new Error("Missing required argument: --run");
  const resolvedRunDir = resolve(runDir);

  const { plan, state, configPath } = validatePreparedRun(resolvedRunDir);
  // The rollback floor in `latest.json` is read from the ledger, so the ledger must already be
  // committed: an append left in the working tree would publish a floor the next clone cannot
  // reproduce, and the release after it would advance the floor instead of pinning it. Checked
  // before anything is written so a refused assemble leaves no sealed publish directory behind.
  assertFloorDurable({ ledgerPath: keeperLedgerPath });
  // Store gate runs before assembly so a Windows build without the MSIX fails with a typed error.
  const storeSubmission = hasWindowsBuild(resolvedRunDir)
    ? assertStoreMsix({ runDir: resolvedRunDir, plan })
    : null;
  const planPath = join(resolvedRunDir, "plan.json");
  const receiptsDir = join(resolvedRunDir, "receipts");
  const artifactsDir = join(resolvedRunDir, "artifacts");
  const outDir = join(resolvedRunDir, "publish");

  // Same ledger for the durability check above and for the floor written into `latest.json`; two
  // different paths here would let a release publish a floor it never validated.
  const result = assembleRelease({
    planPath,
    receiptsDir,
    artifactsDir,
    outDir,
    publicKey: pubkey,
    keeperLedgerPath,
  });
  try {
    writeHomebrewPackages({ sumsPath: join(outDir, "SHA256SUMS.txt"), outDir, tag: plan.tag, repo: plan.repo });
    attachReleaseProvenance({
      publishDir: outDir,
      plan,
      state,
      receipts: result.receipts,
      sourceRepo: sourceRepo ?? loadHostConfig(configPath).repository,
      publicKey: pubkey || state.updaterPublicKey,
      env,
      minisignBin,
      sbomGenerator,
      keeper: readKeeperProvenance(resolvedRunDir, plan, keeperLedgerPath),
    });
  } catch (err) {
    // assembleRelease created outDir in this call; never leave a half-sealed publish directory.
    rmSync(outDir, { recursive: true, force: true });
    throw err;
  }
  result.files = readdirSync(outDir).sort();
  result.storeSubmission = storeSubmission;
  writeFileSync(join(resolvedRunDir, "assembly-state.json"), JSON.stringify({
    schemaVersion: 1,
    planDigest: state.planDigest,
    receiptsDigest: directoryDigest(receiptsDir),
    artifactsDigest: directoryDigest(artifactsDir),
    publishDigest: directoryDigest(outDir),
  }, null, 2) + "\n", { flag: "wx" });
  return result;
}

/**
 * Verifies assembled release inventory byte-for-byte against independent temp derivation.
 */
export async function verifyReleaseRun({ runDir, pubkey = null, keeperLedgerPath = KEEPER_LEDGER_PATH }) {
  if (!runDir) throw new Error("Missing required argument: --run");
  const resolvedRunDir = resolve(runDir);
  const planPath = join(resolvedRunDir, "plan.json");
  const plan = validateAssemblyState(resolvedRunDir);
  const { state } = validatePreparedRun(resolvedRunDir);

  const publishDir = join(resolvedRunDir, "publish");
  if (!existsSync(publishDir)) {
    throw new Error(`publish directory not found in run directory: ${runDir}`);
  }

  // Set when an existing draft release is being resumed; `create` is then skipped and the upload
  // clobbers whatever the interrupted attempt left behind.
  let resumeFrom = null;

  const latestJsonPath = join(publishDir, "latest.json");
  const sumsPath = join(publishDir, "SHA256SUMS.txt");
  if (!existsSync(latestJsonPath)) {
    throw new Error("latest.json not found in publish directory");
  }
  if (!existsSync(sumsPath)) {
    throw new Error("SHA256SUMS.txt not found in publish directory");
  }

  // Parse SHA256SUMS.txt and verify every listed file
  const sumsContent = readFileSync(sumsPath, "utf8");
  const sumsLines = sumsContent.trim().split("\n");
  const sumsMap = new Map();
  for (const line of sumsLines) {
    const parts = line.trim().split(/\s+/);
    if (parts.length >= 2) {
      sumsMap.set(parts[1], parts[0]);
    }
  }

  for (const [file, expectedSha] of sumsMap) {
    const fullPath = join(publishDir, file);
    if (!existsSync(fullPath)) {
      throw new Error(`File listed in SHA256SUMS.txt not found on disk: ${file}`);
    }
    const actualSha = createHash("sha256").update(readFileSync(fullPath)).digest("hex");
    if (actualSha !== expectedSha) {
      throw new Error(`Checksum mismatch for published file ${file}: expected ${expectedSha}, got ${actualSha}`);
    }
  }

  // Signed SHA256SUMS must verify against the updater key and cover every published file.
  verifyChecksums({ dir: publishDir, publicKey: pubkey || state.updaterPublicKey });
  const provenance = JSON.parse(readFileSync(join(publishDir, PROVENANCE_NAME), "utf8"));
  if (provenance.commitSha !== plan.commitSha || provenance.tag !== plan.tag) {
    throw new Error(`${PROVENANCE_NAME} does not match the release plan commit/tag`);
  }

  // Re-derive output in private temp directory to verify repeatability
  const tempDir = mkdtempSync(join(tmpdir(), "ferryx-verify-repeatability-"));
  try {
    const tempPublish = join(tempDir, "publish");
    // The ledger must be passed here too. `latest.json` carries `keeper.oldestSafeRollbackVersion`,
    // which `assembleRelease` derives from the ledger; re-deriving without the same ledger writes a
    // floor of the release itself and fails the byte comparison below on a release that is actually
    // correct. Both paths must name the same file, exactly as assemble's own comment warns.
    assembleRelease({
      planPath,
      receiptsDir: join(resolvedRunDir, "receipts"),
      artifactsDir: join(resolvedRunDir, "artifacts"),
      outDir: tempPublish,
      publicKey: pubkey,
      keeperLedgerPath,
    });
    writeHomebrewPackages({
      sumsPath: join(tempPublish, "SHA256SUMS.txt"),
      outDir: tempPublish,
      tag: plan.tag,
      repo: plan.repo,
    });

    // Provenance assets (SBOMs, provenance.json, SHA256SUMS[.minisig]) are covered by the
    // signature check above; everything else must re-derive byte-for-byte.
    const allPublished = readdirSync(publishDir).sort();
    for (const name of PROVENANCE_ASSET_NAMES) {
      if (!allPublished.includes(name)) throw new Error(`Provenance asset missing from publish directory: ${name}`);
    }
    const origFiles = allPublished.filter((name) => !PROVENANCE_ASSET_NAMES.includes(name));
    const tempFiles = readdirSync(tempPublish).sort();
    if (origFiles.length !== tempFiles.length) {
      throw new Error(
        `Published inventory length mismatch with fresh re-derivation: ${origFiles.length} vs ${tempFiles.length}`,
      );
    }

    for (let i = 0; i < origFiles.length; i += 1) {
      const origName = origFiles[i];
      const tempName = tempFiles[i];
      if (origName !== tempName) {
        throw new Error(`Inventory file mismatch: '${origName}' vs '${tempName}'`);
      }
      const origBuf = readFileSync(join(publishDir, origName));
      const tempBuf = readFileSync(join(tempPublish, tempName));
      if (!origBuf.equals(tempBuf)) {
        throw new Error(`Byte-for-byte mismatch between published file and fresh derivation: ${origName}`);
      }
    }
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }

  // On macOS, verify Gatekeeper spctl assessment on published DMG
  if (process.platform === "darwin") {
    const dmgFile = join(publishDir, "Ferryx_universal.dmg");
    if (existsSync(dmgFile)) {
      const spctlRes = spawnSync("spctl", ["-a", "-vvv", "-t", "install", dmgFile], { encoding: "utf8" });
      const spctlCombined = `${spctlRes.stdout || ""}\n${spctlRes.stderr || ""}`;
      if (spctlRes.status !== 0 || !spctlCombined.includes("Notarized Developer ID")) {
        throw new Error(
          `Gatekeeper spctl verification failed for published DMG: ${spctlCombined.trim()}`,
        );
      }
    }
  }

  return { ok: true, plan, files: Array.from(sumsMap.keys()) };
}

/**
 * Session survival gate (runbook section 4b): survival/<platform>/summary.json for mac, linux and
 * windows at the plan commit with zero FAIL, plus survival/footprint.json. Writes
 * survival/survival-report.md and throws SurvivalGateError when the release must not be drafted.
 */
export function survivalGateRelease({ runDir, survivalWaiver = null }) {
  if (!runDir) throw new Error("Missing required argument: --run");
  const resolvedRunDir = resolve(runDir);
  const plan = validateAssemblyState(resolvedRunDir);
  return assertSurvivalGate({ releaseDir: resolvedRunDir, commitSha: plan.commitSha, waiver: survivalWaiver });
}

/**
 * Crash symbolication gate (runbook section 4c): symbolication/report.json from
 * scripts/qa/dsym-symbolication-acceptance.mjs must show a named frame for the release binary's
 * forced SIGSEGV and a refused mismatched dSYM, with UUIDs matching this run's dsyms/manifest.json.
 * Writes symbolication/symbolication-report.md and throws SymbolicationGateError on refusal.
 */
export function symbolicationGateRelease({ runDir }) {
  if (!runDir) throw new Error("Missing required argument: --run");
  const resolvedRunDir = resolve(runDir);
  const plan = validateAssemblyState(resolvedRunDir);
  const { result, reportPath } = assertSymbolicationGate({ releaseDir: resolvedRunDir });
  return { plan, result, reportPath };
}

/**
 * Publishes release assets to GitHub Release as draft, verifies bytes, and undrafts. Refuses to
 * draft unless the session survival gate passes (or the owner waived unreachable evidence).
 */
export async function publishRelease({
  runDir,
  approvePublish = false,
  survivalWaiver = null,
  ghCommand = "gh",
  lsRemoteFn = null,
  env = process.env,
  keeperLedgerPath = KEEPER_LEDGER_PATH,
}) {
  assertNotInCI("publish");

  if (!approvePublish) {
    throw new Error("Publication requires explicit --approve-publish flag");
  }
  if (env.FERRYX_APPROVE_PUBLISH !== "1") {
    throw new Error("Publication requires FERRYX_APPROVE_PUBLISH=1 environment variable");
  }

  const resolvedRunDir = resolve(runDir);
  const plan = validateAssemblyState(resolvedRunDir);
  const publishDir = join(resolvedRunDir, "publish");
  if (!existsSync(publishDir)) {
    throw new Error(`publish directory not found in run directory: ${runDir}`);
  }

  // 0. Session survival gate, before any network call.
  const survival = assertSurvivalGate({ releaseDir: resolvedRunDir, commitSha: plan.commitSha, waiver: survivalWaiver });

  // 0b. Phase 0 crash symbolication, also before any network call. The dSYM archive is only worth
  // shipping if a forced SIGSEGV of the release binary symbolicated to a named function through it
  // and a mismatched dSYM was refused. Unlike the survival gate there is no waiver: an operator
  // cannot assert a dSYM works, it either names a frame or it does not.
  assertSymbolicationGate({ releaseDir: resolvedRunDir });

  // 1. Verify remote tag matches plan commitSha
  let lsRemoteOut = "";
  if (lsRemoteFn) {
    lsRemoteOut = await lsRemoteFn();
  } else {
    try {
      lsRemoteOut = execFileSync("git", ["ls-remote", "--tags", "origin", `refs/tags/${plan.tag}`, `refs/tags/${plan.tag}^{}`], {
        encoding: "utf8",
      });
    } catch (err) {
      throw new Error(`Failed to query remote tags from origin: ${redactProcessOutput(err instanceof Error ? err.message : err, env)}`);
    }
  }

  if (!lsRemoteOut || !lsRemoteOut.includes(plan.tag)) {
    throw new Error(`Remote tag '${plan.tag}' does not exist on origin. Local coordinator never auto-pushes tags.`);
  }

  // Extract peeled tag SHA if present, else tag SHA
  const tagLines = lsRemoteOut.trim().split("\n");
  let remoteSha = null;
  for (const line of tagLines) {
    const [sha, ref] = line.trim().split(/\s+/);
    if (ref === `refs/tags/${plan.tag}^{}`) {
      remoteSha = sha;
      break;
    }
    if (ref === `refs/tags/${plan.tag}`) {
      remoteSha = sha;
    }
  }

  if (remoteSha !== plan.commitSha) {
    throw new Error(
      `Remote tag '${plan.tag}' resolves to ${remoteSha}, but plan requires commit ${plan.commitSha}`,
    );
  }

  // Set when an existing DRAFT release is being resumed; `create` is then skipped and the upload
  // clobbers whatever the interrupted attempt left behind.
  let resumeFrom = null;

  // 2. Check if release already exists on GitHub
  const viewRes = spawnSync(ghCommand, ["release", "view", plan.tag, "--repo", plan.repo], {
    encoding: "utf8",
  });
  if (viewRes.status === 0) {
    // The release exists. Two very different situations look identical from here -- a completed
    // publish versus one that died between `release create --draft` and the intent write -- so the
    // draft state is the discriminator, and it must be read rather than assumed.
    const existing = spawnSync(ghCommand, ["release", "view", plan.tag, "--repo", plan.repo, "--json", "isDraft"], {
      encoding: "utf8",
    });
    if (existing.status !== 0) {
      throw new Error(
        `Release '${plan.tag}' exists but its draft state could not be read: ${redactProcessOutput(
          `${existing.stdout ?? ""}\n${existing.stderr ?? ""}`,
          env,
        )}`,
      );
    }
    const isDraft = /"isDraft"\s*:\s*true/.test(existing.stdout ?? "");
    if (!isDraft) {
      const pending = readKeeperIntent(keeperLedgerPath);
      const hint =
        pending && pending.tag === plan.tag
          ? ` ${keeperIntentPath(keeperLedgerPath)} marks a publish of ${plan.tag} as unfinished: confirm with ` +
            "`gh release view <tag> --json isDraft` that it is public, then run `reconcile`."
          : " It is already public, so the ledger append should have happened; run `reconcile` to record it.";
      throw new Error(`Release '${plan.tag}' already exists on remote repo ${plan.repo}. Refusing to overwrite.${hint}`);
    }
    // Still a draft: the previous attempt never published, so resuming means re-uploading over it
    // and continuing. Falling through (instead of throwing) is what makes that crash resumable;
    // `--clobber` below replaces any partial upload, and the byte verification re-checks the result.
    resumeFrom = { skippedCreate: true };
  }
  // The absence check only applies when there is no draft to resume. A resuming publish saw
  // `release view` SUCCEED, so its output can never contain "not found" and running the check here
  // would refuse every resume -- the exact crash window this branch exists to recover from.
  if (!resumeFrom?.skippedCreate) {
    const viewOutput = `${viewRes.stdout ?? ""}\n${viewRes.stderr ?? ""}`;
    if (viewRes.status === null || !/release not found|not found|HTTP 404/i.test(viewOutput)) {
      throw new Error(`Unable to establish that release '${plan.tag}' is absent: ${redactProcessOutput(viewOutput, env)}`);
    }
  }

  // 2.5 Resolve or generate release notes
  const notesPath = join(resolvedRunDir, "RELEASE_NOTES.md");
  if (!existsSync(notesPath)) {
    try {
      const generated = generateChangelog({
        repo: plan.repo,
        tag: plan.tag,
        commitSha: plan.commitSha,
        ghCommand,
      });
      writeFileSync(notesPath, generated, "utf8");
    } catch {
      // Fail-open: write basic fallback notes
      writeFileSync(notesPath, `Ferryx release ${plan.tag}\n`, "utf8");
    }
  }
  writeFileSync(notesPath, appendSurvivalReport(readFileSync(notesPath, "utf8"), survival.report), "utf8");

  // 3. Create Draft Release (skipped when resuming an existing draft).
  const createArgs = [
    "release",
    "create",
    plan.tag,
    "--repo",
    plan.repo,
    "--draft",
    "--title",
    plan.tag,
  ];
  createArgs.push("--notes-file", notesPath);

  if (!resumeFrom?.skippedCreate) {
    const createRes = spawnSync(
      ghCommand,
      createArgs,
      { encoding: "utf8" },
    );
    if (createRes.status !== 0) {
      throw new Error(`Failed to create draft release: ${createRes.stderr || createRes.stdout}`);
    }
  }

  // 4. Upload exact inventory
  const publishFiles = readdirSync(publishDir).map((f) => join(publishDir, f));
  const uploadRes = spawnSync(
    ghCommand,
    ["release", "upload", plan.tag, "--repo", plan.repo, "--clobber", ...publishFiles],
    { encoding: "utf8" },
  );
  if (uploadRes.status !== 0) {
    throw new Error(`Failed to upload assets to draft release: ${uploadRes.stderr || uploadRes.stdout}`);
  }

  // 5. Download and verify draft asset bytes against local publish directory
  const tempVerifyDir = mkdtempSync(join(tmpdir(), "ferryx-draft-verify-"));
  try {
    const dlRes = spawnSync(
      ghCommand,
      ["release", "download", plan.tag, "--repo", plan.repo, "--dir", tempVerifyDir],
      { encoding: "utf8" },
    );
    if (dlRes.status !== 0) {
      throw new Error(`Failed to download draft assets for verification: ${dlRes.stderr || dlRes.stdout}`);
    }

    const expectedNames = publishFiles.map((file) => basename(file)).sort();
    const downloadedNames = readdirSync(tempVerifyDir).sort();
    if (expectedNames.join("\n") !== downloadedNames.join("\n")) {
      throw new Error("Draft asset inventory does not exactly match local publish inventory");
    }
    for (const file of publishFiles) {
      const name = basename(file);
      const dlFile = join(tempVerifyDir, name);
      if (!existsSync(dlFile)) {
        throw new Error(`Draft asset '${name}' missing after upload`);
      }
      const localBuf = readFileSync(file);
      const dlBuf = readFileSync(dlFile);
      if (!localBuf.equals(dlBuf)) {
        throw new Error(`Draft asset '${name}' byte mismatch after upload`);
      }
    }
  } finally {
    rmSync(tempVerifyDir, { recursive: true, force: true });
  }

  // 6. Record the keeper generation BEFORE the release becomes public.
  //
  // This ordering is what makes the publish crash-safe. See `prepareKeeperPublication`: undrafting
  // first would leave a public release with an unappended ledger, which silently un-pins the
  // rollback floor for that tag forever. Doing it here means a failure throws while the release is
  // still a draft -- recoverable -- and the intent file left behind explains any crash.
  // `recordKeeperRelease` is idempotent, so re-running publish after a failed undraft is a no-op
  // on the ledger and reuses the pinned floor.
  const keeperLedger = prepareKeeperPublication({ runDir: resolvedRunDir, plan, ledgerPath: keeperLedgerPath });

  // 7. Undraft the release to make it public.
  //
  // After this point nothing may throw, because the release is public and cannot be un-published.
  // The intent file is cleared only once the undraft is confirmed, so an operator who finds one
  // left behind knows the publish did not finish.
  const editRes = spawnSync(
    ghCommand,
    ["release", "edit", plan.tag, "--repo", plan.repo, "--draft=false"],
    { encoding: "utf8" },
  );
  if (editRes.status !== 0) {
    throw new Error(
      `Failed to undraft release: ${editRes.stderr || editRes.stdout}. ` +
        `The keeper ledger at ${keeperLedgerPath} already records ${plan.appVersion} and ${keeperIntentPath(keeperLedgerPath)} marks the publish as unfinished. ` +
        "The release is still a draft. Settle it with `bun scripts/release-local.mjs reconcile` (it will withdraw the " +
        "premature ledger entry and clear the intent), then re-run publish; publishing again without reconciling is " +
        "refused, because the live intent must not be overwritten.",
    );
  }

  // Publication is complete: the intent has been satisfied.
  clearKeeperIntent(keeperLedgerPath);

  return {
    published: true,
    tag: plan.tag,
    runId: plan.runId,
    storeSubmission: storeSubmissionLine(plan),
    survivalWaiver: survival.result.waiver,
    keeperLedger,
  };
}

/**
 * Settles a publish that died between recording the keeper and confirming the release is public.
 *
 * The outcomes are distinguishable only from GitHub, so this asks and then acts:
 *  - release is public, ledger already records it -> clear the intent.
 *  - release is public, ledger does NOT record it -> append it, then clear the intent. This is the
 *    pre-intent crash: `gh release create --draft` plus an undraft that succeeded before any local
 *    state was written. Clearing the intent without appending would leave a shipped release
 *    unrecorded, so the floor would advance on the next cut and the tag would be unfixable
 *    afterwards -- the exact failure the ledger exists to prevent.
 *  - release is a draft -> the ledger entry describes something not yet shipped. The floor must not
 *                          be derived from it, so the entry is removed and the intent is cleared;
 *                          re-running `publish` will append it again before undrafting.
 *  - release is absent  -> same as a draft: nothing shipped, so the entry is not a shipped fact.
 *
 * This never publishes and never pushes a tag. It only reconciles local state with a remote fact,
 * which is why it is safe to run at any time.
 */
export async function reconcileKeeperPublication({
  ledgerPath = KEEPER_LEDGER_PATH,
  ghCommand = "gh",
  repo,
  spawn = spawnSync,
} = {}) {
  const intent = readKeeperIntent(ledgerPath);
  if (!intent) return { reconciled: false, reason: "no intent recorded" };
  if (!intent.tag || !intent.appVersion) {
    throw keeperLedgerError(
      "KEEPER_INTENT_UNREADABLE",
      `${keeperIntentPath(ledgerPath)} is unreadable, so the in-flight publish cannot be identified. ` +
        "Inspect it, confirm the release state with `gh release view`, then delete it.",
      { path: keeperIntentPath(ledgerPath) },
    );
  }

  const targetRepo = repo ?? intent.repo;
  const view = spawn(ghCommand, ["release", "view", intent.tag, "--repo", targetRepo, "--json", "isDraft,tagName"], {
    encoding: "utf8",
  });
  if (view.status !== 0) {
    const output = `${view.stdout ?? ""}\n${view.stderr ?? ""}`;
    // "not found" is a real answer: nothing shipped for this tag, so the ledger entry is premature.
    if (!/release not found|not found|HTTP 404/i.test(output)) {
      throw keeperLedgerError(
        "KEEPER_RECONCILE_UNKNOWN",
        `cannot determine whether ${intent.tag} is public: ${output.trim()}. The intent is left in place.`,
        { path: keeperIntentPath(ledgerPath), tag: intent.tag },
      );
    }
  }

  const isPublic = view.status === 0 && /"isDraft"\s*:\s*false/.test(view.stdout ?? "");

  // Both branches are decided from the ledger as read UNDER the lock, never from a snapshot taken
  // before it. The withdraw path used to write a pre-lock snapshot, so a publish that appended a
  // different release between the read and the write had its entry silently reverted -- the ledger
  // then claimed a shipped release was never recorded, and the floor walked forward.
  let action = "none";
  mutateKeeperLedgerLocked({
    ledgerPath,
    retryHint: "Nothing was changed; re-run reconcile to re-read the ledger",
    mutate: (ledger) => {
      const records = ledger.keeperReleases.some((release) => release.appVersion === intent.appVersion);
      if (isPublic) {
        // A public release with no ledger entry must be appended, not merely forgotten: clearing the
        // intent alone would leave a shipped keeper unrecorded. If an entry is already there, its
        // floor must match what that release published before this is treated as settled.
        if (records) {
          assertLedgerMatchesPublishedFloor(ledger, intent);
          return null;
        }
        action = "appended";
        return recordKeeperRelease(ledger, {
          appVersion: intent.appVersion,
          // Record the floor the intent says actually shipped, so the append cannot drift from the
          // `latest.json` that went out. Without a recorded floor the append would re-derive one
          // from a ledger that may have grown since, which is the same-version divergence again.
          oldestSafeRollbackVersion: intent.oldestSafeRollbackVersion,
        });
      }
      // Withdraw a ledger entry that did not ship. `parseKeeperLedger` enforces strictly increasing
      // appVersions and a non-decreasing floor, so removing the newest entry cannot violate either;
      // if it somehow did, the write fails before touching the destination.
      if (!records) return null;
      // Only the newest entry may be withdrawn. A crash can only leave the *last* append premature,
      // so removing a middle entry could only be a misfiled intent erasing a release that really
      // shipped. That is also the case that would break the floor: `keeperManifest` derives the
      // floor from the oldest entry, but a same-version replacement in the middle would silently
      // re-point every later release at a different floor.
      const newest = ledger.keeperReleases[ledger.keeperReleases.length - 1];
      if (newest.appVersion !== intent.appVersion) {
        throw keeperLedgerError(
          "KEEPER_RECONCILE_NOT_NEWEST",
          `${intent.tag} is still a draft, but the ledger records it before ${newest.appVersion}. ` +
            "A crash can only leave the newest append premature, so this entry is not this publish's and " +
            "withdrawing it would erase a release that may already have shipped. " +
            `Inspect ${ledgerPath} by hand; nothing was changed.`,
          { path: ledgerPath, appVersion: intent.appVersion, newest: newest.appVersion },
        );
      }
      action = "withdrew";
      return {
        schemaVersion: ledger.schemaVersion,
        keeperReleases: ledger.keeperReleases.filter((release) => release.appVersion !== intent.appVersion),
      };
    },
  });
  const withdrew = action === "withdrew";
  const appended = action === "appended";

  clearKeeperIntent(ledgerPath);
  return { reconciled: true, tag: intent.tag, appVersion: intent.appVersion, public: isPublic, withdrew, appended };
}

/**
 * Refuses a ledger whose recorded floor for a shipped release disagrees with what that release
 * published.
 *
 * This is the same-version hazard in its most dangerous form: the version is present, so every
 * "is it recorded?" check passes, but the floor it carries is not the floor in the shipped
 * `latest.json`. Users of that version are then told they can roll back to a build the ledger does
 * not actually guarantee. A recorded-but-different floor is never corrected automatically, because
 * silently rewriting a shipped floor is exactly the corruption this guard exists to catch.
 */
function assertLedgerMatchesPublishedFloor(ledger, intent) {
  if (!intent.oldestSafeRollbackVersion) return; // intent predates floor recording; nothing to compare
  const entry = ledger.keeperReleases.find((release) => release.appVersion === intent.appVersion);
  if (!entry) return; // the caller's own append path handles the missing-entry case
  if (compareKeeperVersions(entry.oldestSafeRollbackVersion, intent.oldestSafeRollbackVersion) === 0) return;
  throw keeperLedgerError(
    "KEEPER_LEDGER_FLOOR_DIVERGED",
    `${intent.appVersion} shipped with oldestSafeRollbackVersion ${intent.oldestSafeRollbackVersion}, but the ledger ` +
      `records ${entry.oldestSafeRollbackVersion} for it. Those must agree: the ledger is what every later release ` +
      "derives its floor from, so a different value silently changes what users of this version may roll back to. " +
      "Resolve by hand: restore the shipped value, or re-publish if the published value was wrong.",
    {
      appVersion: intent.appVersion,
      published: intent.oldestSafeRollbackVersion,
      recorded: entry.oldestSafeRollbackVersion,
    },
  );
}

/**
 * Verifies public availability of release metadata and checksums on GitHub.
 */
export async function verifyRemoteRelease({ runDir, baseUrl = null }) {
  if (!runDir) throw new Error("Missing required argument: --run");
  const resolvedRunDir = resolve(runDir);
  const plan = validateAssemblyState(resolvedRunDir);

  const rootUrl = baseUrl || `https://github.com/${plan.repo}/releases/download`;
  const latestUrl = `${rootUrl}/${plan.tag}/latest.json`;
  const sumsUrl = `${rootUrl}/${plan.tag}/SHA256SUMS.txt`;

  const fetchRes = await fetch(latestUrl);
  if (!fetchRes.ok) {
    throw new Error(`Failed to fetch remote latest.json: HTTP ${fetchRes.status} from ${latestUrl}`);
  }
  const manifest = await fetchRes.json();
  if (manifest.version !== plan.appVersion) {
    throw new Error(`Remote manifest version mismatch: expected ${plan.appVersion}, got ${manifest.version}`);
  }

  const sumsRes = await fetch(sumsUrl);
  if (!sumsRes.ok) {
    throw new Error(`Failed to fetch remote SHA256SUMS.txt: HTTP ${sumsRes.status} from ${sumsUrl}`);
  }

  return { ok: true, tag: plan.tag };
}

/**
 * CLI Main entry point.
 */
async function main() {
  const args = process.argv.slice(2);
  const { positional, options, flags } = parseCliArgs(args);

  if (flags.has("help") || positional.length === 0) {
    console.log(`
Usage: release-local <subcommand> [options]

Subcommands:
  prepare --config <json> --tag <date> --commit <sha-or-ref> --out <new-run-dir> [--nsis-migration]
  preflight --config <json> [--plan <path>]
  build --config <json> --run <dir> --host macbook|omaki|maho-win [--approve-notarization]
  assemble --run <dir> [--pubkey <fixture-key>]
  verify --run <dir> [--pubkey <fixture-key>]
  survival-gate --run <dir> [--survival-waiver "<reason>"]
  symbolication-gate --run <dir>
  publish --run <dir> --approve-publish [--survival-waiver "<reason>"]
  reconcile [--repo <owner/name>]
  verify-remote --run <dir>
`);
    process.exit(0);
  }

  const subcommand = positional[0];

  try {
    switch (subcommand) {
      case "prepare": {
        const configPath = options.get("config");
        const tag = options.get("tag");
        const commit = options.get("commit") || "HEAD";
        const outDir = options.get("out");
        const nsisMigration = flags.has("nsis-migration");
        const result = prepareRelease({ configPath, tag, commit, outDir, nsisMigration });
        console.log(`Release run prepared successfully at ${result.outDir}`);
        console.log(JSON.stringify(result.plan, null, 2));
        break;
      }
      case "preflight": {
        const configPath = options.get("config");
        const planPath = options.get("plan") || null;
        const result = await preflightRelease({ configPath, planPath });
        console.log(JSON.stringify(result, null, 2));
        if (!result.ok) {
          process.exit(1);
        }
        break;
      }
      case "build": {
        const configPath = options.get("config");
        const runDir = options.get("run");
        const host = options.get("host");
        const approveNotarization = flags.has("approve-notarization");
        const receipt = await buildRelease({
          configPath,
          runDir,
          host,
          approveNotarization,
        });
        console.log(JSON.stringify(receipt, null, 2));
        break;
      }
      case "assemble": {
        const runDir = options.get("run");
        const pubkey = options.get("pubkey") || null;
        const result = assembleReleaseRun({ runDir, pubkey });
        console.log(`Release assembled successfully with ${result.files.length} files`);
        if (result.storeSubmission) console.log(result.storeSubmission);
        break;
      }
      case "verify": {
        const runDir = options.get("run");
        const pubkey = options.get("pubkey") || null;
        const result = await verifyReleaseRun({ runDir, pubkey });
        console.log(`Release verified successfully against ${result.files.length} files`);
        break;
      }
      case "survival-gate": {
        const runDir = options.get("run");
        const survivalWaiver = options.get("survival-waiver") || null;
        const { result, reportPath } = survivalGateRelease({ runDir, survivalWaiver });
        console.log(`Session survival gate passed; report at ${reportPath}`);
        if (result.waiver) console.log(`Survival waiver: ${result.waiver}`);
        break;
      }
      case "symbolication-gate": {
        const runDir = options.get("run");
        const { result, reportPath } = symbolicationGateRelease({ runDir });
        console.log(`Crash symbolication gate passed; report at ${reportPath}`);
        console.log(`Binary UUIDs: ${(result.report?.binary?.uuids ?? []).join(", ")}`);
        break;
      }
      case "publish": {
        const runDir = options.get("run");
        const approvePublish = flags.has("approve-publish");
        const survivalWaiver = options.get("survival-waiver") || null;
        const result = await publishRelease({ runDir, approvePublish, survivalWaiver });
        console.log(`Release published successfully: ${result.tag}`);
        if (result.survivalWaiver) console.log(`Survival waiver: ${result.survivalWaiver}`);
        if (result.keeperLedger) {
          // The append is now a precondition of publication, not a best-effort follow-up, so there
          // is no failure branch to report here: a ledger that could not be written throws before
          // the release goes public.
          const state = result.keeperLedger.changed ? "recorded" : "already present";
          console.log(
            `Keeper ledger ${state} at ${result.keeperLedger.path} ` +
              `(oldestSafeRollbackVersion ${result.keeperLedger.oldestSafeRollbackVersion}); review and commit it`,
          );
        }
        console.log(result.storeSubmission);
        console.log(`Submit via docs/releases/MS_STORE_RESUBMISSION_RECOVERY.md (product ${STORE_PRODUCT_ID})`);
        break;
      }
      case "reconcile": {
        const result = await reconcileKeeperPublication({ repo: options.get("repo") || undefined });
        if (!result.reconciled) {
          console.log("No unfinished keeper publish to reconcile.");
        } else if (result.public) {
          console.log(`Reconciled ${result.tag}: the release is public, so the ledger entry stands. Intent cleared.`);
          console.log("Review and commit packaging/keeper/releases.json before the next assemble.");
        } else {
          console.log(
            `Reconciled ${result.tag}: the release is NOT public, so ${result.withdrew ? "its premature ledger entry was withdrawn and " : ""}` +
              "the intent was cleared. Re-run `publish` when ready.",
          );
        }
        break;
      }
      case "verify-remote": {
        const runDir = options.get("run");
        const result = await verifyRemoteRelease({ runDir });
        console.log(`Remote release verified successfully: ${result.tag}`);
        break;
      }
      default:
        console.error(`Unknown subcommand: ${subcommand}`);
        process.exit(1);
    }
  } catch (err) {
    // A remote build failure is only diagnosable through the child's own output:
    // err.message is the bare wrapper ("Command failed with exit code 1: ssh")
    // while the compiler or host script error lives in stdout/stderr.
    console.error(`Error in ${subcommand}: ${err.message}`);
    for (const field of ["stderr", "stdout"]) {
      const text = typeof err?.[field] === "string" ? err[field].trim() : "";
      if (text !== "") console.error(`--- ${field} ---\n${text}`);
    }
    process.exit(1);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  main();
}
