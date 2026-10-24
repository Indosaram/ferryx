#!/usr/bin/env node
/**
 * Release provenance: SHA256SUMS over every release asset, a minisign signature over it
 * (same key as the Tauri updater), CycloneDX SBOMs for src-tauri and ui, and a provenance
 * manifest recording which host built which asset from which commit and toolchains.
 *
 * Runs only on the local release coordinator (see docs/releases/LOCAL_RELEASE_RUNBOOK.md).
 */
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
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

import { parsePublicKey, verifyMinisign } from "./lib/minisign-verify.mjs";

export const CHECKSUMS_NAME = "SHA256SUMS";
export const CHECKSUMS_SIGNATURE_NAME = "SHA256SUMS.minisig";
export const RUST_SBOM_NAME = "ferryx-src-tauri.cdx.json";
export const UI_SBOM_NAME = "ferryx-ui.cdx.json";
export const PROVENANCE_NAME = "provenance.json";
/** Assets added on top of the build-latest-json inventory. Contract consumed by todos 19 and 22. */
export const PROVENANCE_ASSET_NAMES = Object.freeze([
  RUST_SBOM_NAME,
  UI_SBOM_NAME,
  PROVENANCE_NAME,
  CHECKSUMS_NAME,
  CHECKSUMS_SIGNATURE_NAME,
]);
export const CYCLONEDX_NPM_PACKAGE = "@cyclonedx/cyclonedx-npm@6.0.1";
export const CYCLONEDX_SPEC_VERSION = "1.5";

const CHECKSUM_LINE = /^([0-9a-f]{64}) {2}(.+)$/;

function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function listAssets(dir) {
  const names = [];
  for (const name of readdirSync(dir).sort()) {
    if (!statSync(join(dir, name)).isFile()) {
      throw new Error(`Release asset directory must be flat; '${name}' is not a regular file`);
    }
    if (name === CHECKSUMS_NAME || name === CHECKSUMS_SIGNATURE_NAME) continue;
    names.push(name);
  }
  return names;
}

/**
 * Writes `SHA256SUMS` (shasum -a 256 -c / sha256sum -c format) over every file in `dir`
 * except the checksum file and its signature.
 */
export function writeChecksums(dir) {
  const names = listAssets(dir);
  if (names.length === 0) throw new Error(`No release assets to checksum in ${dir}`);
  const lines = names.map((name) => `${sha256File(join(dir, name))}  ${name}`);
  writeFileSync(join(dir, CHECKSUMS_NAME), lines.join("\n") + "\n");
  return names;
}

/**
 * Decodes a Tauri signing key (base64-wrapped minisign secret key, the same value as
 * TAURI_SIGNING_PRIVATE_KEY) or a plain minisign secret key file body, or a path to either.
 */
export function decodeSecretKey(value) {
  if (typeof value !== "string" || value.trim() === "") throw new Error("Signing key is empty");
  let text = value.trim();
  if (!text.includes("\n") && existsSync(text) && statSync(text).isFile()) {
    text = readFileSync(text, "utf8").trim();
  }
  if (!text.startsWith("untrusted comment:")) {
    text = Buffer.from(text.replace(/\s+/g, ""), "base64").toString("utf8").trim();
  }
  if (!text.startsWith("untrusted comment:")) {
    throw new Error("Signing key is neither a minisign secret key nor a Tauri base64-wrapped key");
  }
  return `${text}\n`;
}

/**
 * Resolves the updater signing key from the coordinator environment (same variables the
 * updater signer uses). Throws when absent: a release is never published unsigned.
 */
export function signingKeyFromEnv(env = process.env) {
  const raw = env.TAURI_SIGNING_PRIVATE_KEY_PATH || env.TAURI_SIGNING_PRIVATE_KEY;
  if (!raw) {
    throw new Error(
      "SHA256SUMS signing requires TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH (the updater key)",
    );
  }
  return { secretKey: decodeSecretKey(raw), password: env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? "" };
}

/**
 * Signs `SHA256SUMS` with minisign into `SHA256SUMS.minisig`, then verifies the signature
 * against `publicKey` so a wrong key fails closed.
 */
export function signChecksums({
  dir,
  secretKey,
  password = "",
  publicKey,
  trustedComment,
  minisignBin = process.env.MINISIGN_BIN || "minisign",
}) {
  if (!publicKey) throw new Error("signChecksums requires the updater public key");
  const sumsPath = join(dir, CHECKSUMS_NAME);
  const sigPath = join(dir, CHECKSUMS_SIGNATURE_NAME);
  const keyDir = mkdtempSync(join(tmpdir(), "ferryx-sums-key-"));
  try {
    const keyPath = join(keyDir, "minisign.key");
    writeFileSync(keyPath, secretKey, { mode: 0o600 });
    const args = ["-S", "-s", keyPath, "-m", sumsPath, "-x", sigPath];
    if (trustedComment) args.push("-t", trustedComment);
    const res = spawnSync(minisignBin, args, { input: `${password}\n`, encoding: "utf8" });
    if (res.error) throw new Error(`Failed to run minisign (${minisignBin}): ${res.error.message}`);
    if (res.status !== 0) {
      throw new Error(`minisign signing failed (exit ${res.status}): ${(res.stderr || res.stdout).trim()}`);
    }
  } finally {
    rmSync(keyDir, { recursive: true, force: true });
  }
  try {
    verifyMinisign({
      data: readFileSync(sumsPath),
      signature: readFileSync(sigPath, "utf8"),
      publicKey,
    });
  } catch (err) {
    rmSync(sigPath, { force: true });
    throw new Error(`SHA256SUMS signature does not verify against the updater public key: ${err.message}`);
  }
}

/**
 * Verifies `SHA256SUMS.minisig` against `publicKey`, then that `SHA256SUMS` lists exactly the
 * files present in `dir` with matching hashes.
 */
export function verifyChecksums({ dir, publicKey }) {
  const sumsPath = join(dir, CHECKSUMS_NAME);
  const sigPath = join(dir, CHECKSUMS_SIGNATURE_NAME);
  if (!existsSync(sumsPath)) throw new Error(`${CHECKSUMS_NAME} not found in ${dir}`);
  if (!existsSync(sigPath)) throw new Error(`${CHECKSUMS_SIGNATURE_NAME} not found in ${dir}`);
  const sumsBytes = readFileSync(sumsPath);
  try {
    verifyMinisign({ data: sumsBytes, signature: readFileSync(sigPath, "utf8"), publicKey });
  } catch (err) {
    throw new Error(`${CHECKSUMS_SIGNATURE_NAME} verification failed: ${err.message}`);
  }

  const listed = new Map();
  for (const line of sumsBytes.toString("utf8").split("\n")) {
    if (line === "") continue;
    const match = CHECKSUM_LINE.exec(line);
    if (!match) throw new Error(`Malformed ${CHECKSUMS_NAME} line: ${JSON.stringify(line)}`);
    if (listed.has(match[2])) throw new Error(`Duplicate ${CHECKSUMS_NAME} entry: ${match[2]}`);
    listed.set(match[2], match[1]);
  }
  const present = listAssets(dir);
  const missing = present.filter((name) => !listed.has(name));
  if (missing.length > 0) throw new Error(`Assets not covered by ${CHECKSUMS_NAME}: ${missing.join(", ")}`);
  for (const [name, expected] of listed) {
    const path = join(dir, name);
    if (!existsSync(path)) throw new Error(`File listed in ${CHECKSUMS_NAME} not found: ${name}`);
    const actual = sha256File(path);
    if (actual !== expected) {
      throw new Error(`Checksum mismatch for ${name}: expected ${expected}, got ${actual}`);
    }
  }
  return { files: [...listed.keys()] };
}

function defaultRun(command, args, { cwd, env } = {}) {
  return execFileSync(command, args, { cwd, env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
}

function assertCycloneDx(path) {
  if (!existsSync(path)) throw new Error(`SBOM was not produced: ${path}`);
  const bom = JSON.parse(readFileSync(path, "utf8"));
  if (bom.bomFormat !== "CycloneDX") throw new Error(`Not a CycloneDX SBOM: ${path}`);
}

/**
 * Generates CycloneDX SBOMs for src-tauri (cargo cyclonedx) and ui (cyclonedx-npm) from a
 * clean `git archive` of `commitSha`, so a dirty coordinator checkout cannot leak in.
 * Requires `cargo install cargo-cyclonedx --locked` on the coordinator.
 */
export function generateSboms({ sourceRepo, commitSha, outDir, env = process.env, run = defaultRun }) {
  const work = mkdtempSync(join(tmpdir(), "ferryx-sbom-"));
  try {
    const tarPath = join(work, "source.tar");
    const srcDir = join(work, "src");
    run("git", ["-C", sourceRepo, "archive", "--format=tar", "-o", tarPath, commitSha, "src-tauri", "crates", "ui"]);
    run("mkdir", ["-p", srcDir]);
    run("tar", ["-xf", tarPath, "-C", srcDir]);

    const uiDir = join(srcDir, "ui");
    run("bun", ["install", "--frozen-lockfile", "--ignore-scripts"], { cwd: uiDir, env });
    // cyclonedx-npm shells out to npm-ls through npm_execpath, which bunx points at bun.
    const npmCli = join(run("npm", ["root", "-g"], { env }).trim(), "npm", "bin", "npm-cli.js");
    run(
      "bunx",
      [
        CYCLONEDX_NPM_PACKAGE,
        "--omit", "dev",
        "--output-reproducible",
        "--spec-version", CYCLONEDX_SPEC_VERSION,
        "--output-format", "JSON",
        "--output-file", join(outDir, UI_SBOM_NAME),
        "package.json",
      ],
      { cwd: uiDir, env: { ...env, npm_execpath: npmCli } },
    );

    const manifestPath = join(srcDir, "src-tauri", "Cargo.toml");
    const cargoCyclonedx = run("cargo", ["cyclonedx", "--version"], { env }).trim();
    run(
      "cargo",
      [
        "cyclonedx",
        "--manifest-path", manifestPath,
        "--format", "json",
        "--spec-version", CYCLONEDX_SPEC_VERSION,
        "--target", "all",
        "--override-filename", RUST_SBOM_NAME.replace(/\.json$/, ""),
      ],
      { cwd: srcDir, env },
    );
    copyFileSync(join(srcDir, "src-tauri", RUST_SBOM_NAME), join(outDir, RUST_SBOM_NAME));

    assertCycloneDx(join(outDir, UI_SBOM_NAME));
    assertCycloneDx(join(outDir, RUST_SBOM_NAME));
    return { cargoCyclonedx, cyclonedxNpm: CYCLONEDX_NPM_PACKAGE, specVersion: CYCLONEDX_SPEC_VERSION };
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

/**
 * Builds the provenance manifest: commit, plan toolchains, and per host the recorded
 * toolchain versions and the assets it built.
 */
export function buildProvenance({ plan, state = {}, receipts, sbomTools }) {
  return {
    schemaVersion: 1,
    repo: plan.repo,
    tag: plan.tag,
    appVersion: plan.appVersion,
    runId: plan.runId,
    commitSha: plan.commitSha,
    sourceDateEpoch: plan.sourceDateEpoch,
    ghosttyPin: state.ghosttyPin ?? null,
    plannedToolchains: plan.toolchains,
    builds: [...receipts]
      .sort((a, b) => a.host.localeCompare(b.host))
      .map((receipt) => ({
        host: receipt.host,
        completedAt: receipt.completedAt,
        toolchains: receipt.toolchains ?? null,
        assets: receipt.artifacts.map((artifact) => ({
          name: artifact.name,
          kind: artifact.kind,
          bytes: artifact.bytes,
          sha256: artifact.sha256,
        })),
      })),
    sbom: { files: [RUST_SBOM_NAME, UI_SBOM_NAME], tools: sbomTools ?? null },
    verification: {
      checksums: CHECKSUMS_NAME,
      signature: CHECKSUMS_SIGNATURE_NAME,
      signingKey: "plugins.updater.pubkey in src-tauri/tauri.conf.json at commitSha",
    },
  };
}

/**
 * Adds SBOMs, provenance.json, SHA256SUMS and SHA256SUMS.minisig to an assembled publish dir.
 */
export function attachReleaseProvenance({
  publishDir,
  plan,
  state,
  receipts,
  sourceRepo,
  publicKey,
  env = process.env,
  minisignBin,
  sbomGenerator = generateSboms,
}) {
  const { secretKey, password } = signingKeyFromEnv(env);
  const sbomTools = sbomGenerator({ sourceRepo, commitSha: plan.commitSha, outDir: publishDir, env });
  for (const name of [RUST_SBOM_NAME, UI_SBOM_NAME]) {
    if (!existsSync(join(publishDir, name))) throw new Error(`SBOM missing after generation: ${name}`);
  }
  writeFileSync(
    join(publishDir, PROVENANCE_NAME),
    JSON.stringify(buildProvenance({ plan, state, receipts, sbomTools }), null, 2) + "\n",
  );
  writeChecksums(publishDir);
  signChecksums({
    dir: publishDir,
    secretKey,
    password,
    publicKey,
    trustedComment: `ferryx ${plan.tag} commit ${plan.commitSha}`,
    minisignBin,
  });
  return verifyChecksums({ dir: publishDir, publicKey });
}

/** Committed updater public key (Tauri base64-wrapped minisign public key). */
export function loadCommittedPublicKey() {
  const confPath = resolve(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "tauri.conf.json");
  const pubkey = JSON.parse(readFileSync(confPath, "utf8")).plugins?.updater?.pubkey;
  if (!pubkey) throw new Error("plugins.updater.pubkey missing from src-tauri/tauri.conf.json");
  return pubkey;
}

/** The single-line key `minisign -P` accepts, derived from the Tauri-wrapped public key. */
export function minisignPublicKeyLine(publicKey) {
  const parsed = parsePublicKey(publicKey);
  return Buffer.concat([Buffer.from("Ed"), parsed.keyId, parsed.rawPublicKey]).toString("base64");
}

function main(argv) {
  const [subcommand, ...rest] = argv;
  const options = new Map();
  for (let i = 0; i < rest.length; i += 2) options.set(rest[i].replace(/^--/, ""), rest[i + 1]);
  const dir = options.get("dir");
  const publicKey = options.get("pubkey") || loadCommittedPublicKey();
  switch (subcommand) {
    case "seal": {
      if (!dir) throw new Error("seal requires --dir");
      const { secretKey, password } = signingKeyFromEnv();
      const files = writeChecksums(dir);
      signChecksums({ dir, secretKey, password, publicKey, trustedComment: options.get("comment") });
      console.log(`Wrote and signed ${CHECKSUMS_NAME} over ${files.length} files`);
      return;
    }
    case "verify": {
      if (!dir) throw new Error("verify requires --dir");
      const { files } = verifyChecksums({ dir, publicKey });
      console.log(`${CHECKSUMS_SIGNATURE_NAME} and ${files.length} checksums verified`);
      return;
    }
    case "pubkey":
      console.log(minisignPublicKeyLine(publicKey));
      return;
    default:
      console.log(`Usage: release-provenance <subcommand> [options]

  seal --dir <assets> [--pubkey <key>] [--comment <text>]
      write SHA256SUMS and sign it (key from TAURI_SIGNING_PRIVATE_KEY[_PATH])
  verify --dir <assets> [--pubkey <key>]
      verify SHA256SUMS.minisig and every listed checksum; exits 1 on any mismatch
  pubkey [--pubkey <key>]
      print the committed updater key in 'minisign -P' form`);
      process.exitCode = subcommand ? 1 : 0;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  try {
    main(process.argv.slice(2));
  } catch (err) {
    console.error(`release-provenance: ${err.message}`);
    process.exit(1);
  }
}
