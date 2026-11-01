#!/usr/bin/env bun
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
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
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { inflateRawSync } from "node:zlib";

import { assembleRelease } from "./build-latest-json.mjs";
import { appendSurvivalReport, generateChangelog } from "./lib/release-changelog.mjs";
import { parsePlan, parseReceipt } from "./lib/release-contract.mjs";
import { loadHostConfig, redactProcessOutput } from "./lib/release-hosts.mjs";
import { buildHost, preflightAll } from "./lib/release-platforms.mjs";
import { assertSurvivalGate } from "./lib/release-survival.mjs";
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
}) {
  assertNotInCI("assemble");

  if (!runDir) throw new Error("Missing required argument: --run");
  const resolvedRunDir = resolve(runDir);

  const { plan, state, configPath } = validatePreparedRun(resolvedRunDir);
  // Store gate runs before assembly so a Windows build without the MSIX fails with a typed error.
  const storeSubmission = hasWindowsBuild(resolvedRunDir)
    ? assertStoreMsix({ runDir: resolvedRunDir, plan })
    : null;
  const planPath = join(resolvedRunDir, "plan.json");
  const receiptsDir = join(resolvedRunDir, "receipts");
  const artifactsDir = join(resolvedRunDir, "artifacts");
  const outDir = join(resolvedRunDir, "publish");

  const result = assembleRelease({ planPath, receiptsDir, artifactsDir, outDir, publicKey: pubkey });
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
export async function verifyReleaseRun({ runDir, pubkey = null }) {
  if (!runDir) throw new Error("Missing required argument: --run");
  const resolvedRunDir = resolve(runDir);
  const planPath = join(resolvedRunDir, "plan.json");
  const plan = validateAssemblyState(resolvedRunDir);
  const { state } = validatePreparedRun(resolvedRunDir);

  const publishDir = join(resolvedRunDir, "publish");
  if (!existsSync(publishDir)) {
    throw new Error(`publish directory not found in run directory: ${runDir}`);
  }

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
    assembleRelease({
      planPath,
      receiptsDir: join(resolvedRunDir, "receipts"),
      artifactsDir: join(resolvedRunDir, "artifacts"),
      outDir: tempPublish,
      publicKey: pubkey,
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

  // 2. Check if release already exists on GitHub
  const viewRes = spawnSync(ghCommand, ["release", "view", plan.tag, "--repo", plan.repo], {
    encoding: "utf8",
  });
  if (viewRes.status === 0) {
    throw new Error(`Release '${plan.tag}' already exists on remote repo ${plan.repo}. Refusing to overwrite.`);
  }
  const viewOutput = `${viewRes.stdout ?? ""}\n${viewRes.stderr ?? ""}`;
  if (viewRes.status === null || !/release not found|not found|HTTP 404/i.test(viewOutput)) {
    throw new Error(`Unable to establish that release '${plan.tag}' is absent: ${redactProcessOutput(viewOutput, env)}`);
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

  // 3. Create Draft Release
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

  const createRes = spawnSync(
    ghCommand,
    createArgs,
    { encoding: "utf8" },
  );
  if (createRes.status !== 0) {
    throw new Error(`Failed to create draft release: ${createRes.stderr || createRes.stdout}`);
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

  // 6. Undraft the release to make it public
  const editRes = spawnSync(
    ghCommand,
    ["release", "edit", plan.tag, "--repo", plan.repo, "--draft=false"],
    { encoding: "utf8" },
  );
  if (editRes.status !== 0) {
    throw new Error(`Failed to undraft release: ${editRes.stderr || editRes.stdout}`);
  }

  return {
    published: true,
    tag: plan.tag,
    runId: plan.runId,
    storeSubmission: storeSubmissionLine(plan),
    survivalWaiver: survival.result.waiver,
  };
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
  publish --run <dir> --approve-publish [--survival-waiver "<reason>"]
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
      case "publish": {
        const runDir = options.get("run");
        const approvePublish = flags.has("approve-publish");
        const survivalWaiver = options.get("survival-waiver") || null;
        const result = await publishRelease({ runDir, approvePublish, survivalWaiver });
        console.log(`Release published successfully: ${result.tag}`);
        if (result.survivalWaiver) console.log(`Survival waiver: ${result.survivalWaiver}`);
        console.log(result.storeSubmission);
        console.log(`Submit via docs/releases/MS_STORE_RESUBMISSION_RECOVERY.md (product ${STORE_PRODUCT_ID})`);
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
    console.error(`Error in ${subcommand}: ${err.message}`);
    process.exit(1);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  main();
}
