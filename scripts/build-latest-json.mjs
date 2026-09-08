#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import fs, {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { verifyMinisign } from "./lib/minisign-verify.mjs";
import {
  isSafeRelativePath,
  KIND_DEFINITIONS,
  parsePlan,
  parseReceipt,
  requiredKinds,
  VALID_HOSTS,
} from "./lib/release-contract.mjs";

const EXPECTED_RECEIPT_FILENAMES = Object.freeze(
  new Set(["build-receipt-macbook.json", "build-receipt-omaki.json", "build-receipt-maho-win.json"]),
);

/**
 * Parses CLI arguments.
 */
function parseArgs(argv) {
  const args = new Map();
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (!token.startsWith("--")) continue;
    const next = argv[index + 1];
    if (next === undefined || next.startsWith("--")) {
      throw new Error(`option ${token} requires a value`);
    }
    args.set(token.slice(2), next);
    index += 1;
  }
  return args;
}

/**
 * Attempts to load the updater public key from checked-in Tauri config.
 */
function loadDefaultPublicKey() {
  const candidates = [
    join(process.cwd(), "src-tauri", "tauri.conf.json"),
    resolve(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "tauri.conf.json"),
  ];

  for (const p of candidates) {
    if (existsSync(p)) {
      try {
        const conf = JSON.parse(readFileSync(p, "utf8"));
        if (conf.plugins?.updater?.pubkey) {
          return conf.plugins.updater.pubkey;
        }
      } catch {
        // continue
      }
    }
  }
  throw new Error("Public key was not provided and could not be loaded from src-tauri/tauri.conf.json");
}

/**
 * Assembles and verifies a complete release inventory based on release plan and host receipts.
 *
 * @param {object} options
 * @param {string} options.planPath
 * @param {string} options.receiptsDir
 * @param {string} options.artifactsDir
 * @param {string} options.outDir
 * @param {string} [options.publicKey]
 * @returns {{ outDir: string, plan: object, receipts: object[], manifest: object, files: string[] }}
 */
export function assembleRelease(options) {
  if (!options || typeof options !== "object") {
    throw new Error("assembleRelease options must be an object");
  }

  const { planPath, receiptsDir, artifactsDir, outDir, publicKey } = options;

  if (!planPath || typeof planPath !== "string") {
    throw new Error("Missing required option: planPath");
  }
  if (!receiptsDir || typeof receiptsDir !== "string") {
    throw new Error("Missing required option: receiptsDir");
  }
  if (!artifactsDir || typeof artifactsDir !== "string") {
    throw new Error("Missing required option: artifactsDir");
  }
  if (!outDir || typeof outDir !== "string") {
    throw new Error("Missing required option: outDir");
  }

  const resolvedOutDir = resolve(outDir);
  if (existsSync(resolvedOutDir)) {
    throw new Error(`Output directory already exists: ${outDir}`);
  }

  if (!existsSync(planPath)) {
    throw new Error(`Plan file not found: ${planPath}`);
  }
  if (!existsSync(receiptsDir) || !statSync(receiptsDir).isDirectory()) {
    throw new Error(`Receipts directory not found or not a directory: ${receiptsDir}`);
  }
  if (!existsSync(artifactsDir) || !statSync(artifactsDir).isDirectory()) {
    throw new Error(`Artifacts directory not found or not a directory: ${artifactsDir}`);
  }

  const effectivePubKey = publicKey || loadDefaultPublicKey();

  // 1. Parse and validate release plan
  const rawPlan = readFileSync(planPath, "utf8");
  const plan = parsePlan(rawPlan);

  // 2. Scan and validate receipts directory
  const receiptEntries = readdirSync(receiptsDir);
  for (const entry of receiptEntries) {
    if (!EXPECTED_RECEIPT_FILENAMES.has(entry)) {
      throw new Error(`Unexpected file in receipts directory: '${entry}'`);
    }
  }

  for (const expectedName of EXPECTED_RECEIPT_FILENAMES) {
    if (!receiptEntries.includes(expectedName)) {
      throw new Error(`Missing expected build receipt file: '${expectedName}'`);
    }
  }

  const receipts = [];
  const hostsSeen = new Set();

  for (const filename of Array.from(EXPECTED_RECEIPT_FILENAMES).sort()) {
    const receiptPath = join(receiptsDir, filename);
    const receipt = parseReceipt(readFileSync(receiptPath, "utf8"), plan);

    if (hostsSeen.has(receipt.host)) {
      throw new Error(`Duplicate receipt host: '${receipt.host}'`);
    }
    hostsSeen.add(receipt.host);
    receipts.push(receipt);
  }

  for (const validHost of VALID_HOSTS) {
    if (!hostsSeen.has(validHost)) {
      throw new Error(`Missing receipt for required host: '${validHost}'`);
    }
  }

  // 3. Artifact kinds completeness check
  const expectedKinds = requiredKinds(plan);
  const seenKinds = new Set();
  const allArtifactEntries = [];

  for (const receipt of receipts) {
    for (const art of receipt.artifacts) {
      if (seenKinds.has(art.kind)) {
        throw new Error(`Duplicate artifact kind '${art.kind}' across receipts`);
      }
      seenKinds.add(art.kind);
      allArtifactEntries.push({ artifact: art, host: receipt.host });
    }
  }

  for (const reqKind of expectedKinds) {
    if (!seenKinds.has(reqKind)) {
      throw new Error(`Missing required artifact kind: '${reqKind}'`);
    }
  }

  for (const foundKind of seenKinds) {
    if (!expectedKinds.includes(foundKind)) {
      throw new Error(`Unexpected artifact kind '${foundKind}' not permitted by release plan`);
    }
  }

  // 4. Verify artifact contents and signatures in filesystem
  const realArtifactsDir = realpathSync(artifactsDir);
  const validatedArtifacts = [];

  for (const { artifact } of allArtifactEntries) {
    if (!isSafeRelativePath(artifact.relPath)) {
      throw new Error(`Artifact relPath is not a safe relative path: ${artifact.relPath}`);
    }

    const fullPath = resolve(artifactsDir, artifact.relPath);
    if (!existsSync(fullPath)) {
      throw new Error(`Artifact file not found: ${artifact.relPath}`);
    }

    const realFullPath = realpathSync(fullPath);
    if (!realFullPath.startsWith(realArtifactsDir + "/") && realFullPath !== realArtifactsDir) {
      throw new Error(`Artifact path escapes artifacts directory: ${artifact.relPath}`);
    }

    const fileStat = statSync(fullPath);
    if (!fileStat.isFile()) {
      throw new Error(`Artifact path is not a regular file: ${artifact.relPath}`);
    }

    if (fileStat.size !== artifact.bytes) {
      throw new Error(
        `Artifact size mismatch for '${artifact.name}': expected ${artifact.bytes} bytes, found ${fileStat.size} bytes`,
      );
    }

    const fileBuffer = readFileSync(fullPath);
    const actualSha = createHash("sha256").update(fileBuffer).digest("hex");
    if (actualSha !== artifact.sha256) {
      throw new Error(
        `Artifact SHA256 mismatch for '${artifact.name}': expected ${artifact.sha256}, got ${actualSha}`,
      );
    }

    let fullSigPath = null;
    let sigContent = null;

    if (artifact.signatureRelPath) {
      if (!isSafeRelativePath(artifact.signatureRelPath)) {
        throw new Error(`Signature relPath is not a safe relative path: ${artifact.signatureRelPath}`);
      }

      fullSigPath = resolve(artifactsDir, artifact.signatureRelPath);
      if (!existsSync(fullSigPath)) {
        throw new Error(`Signature file not found: ${artifact.signatureRelPath}`);
      }

      const realSigPath = realpathSync(fullSigPath);
      if (!realSigPath.startsWith(realArtifactsDir + "/")) {
        throw new Error(`Signature path escapes artifacts directory: ${artifact.signatureRelPath}`);
      }

      const sigStat = statSync(fullSigPath);
      if (!sigStat.isFile() || sigStat.size === 0) {
        throw new Error(`Signature file is empty or not a regular file: ${artifact.signatureRelPath}`);
      }

      sigContent = readFileSync(fullSigPath, "utf8");
    }

    const kindDef = KIND_DEFINITIONS[artifact.kind];
    if (kindDef.isUpdater) {
      if (!sigContent) {
        throw new Error(`Updater artifact '${artifact.name}' requires a valid signature file`);
      }

      try {
        verifyMinisign({
          data: fileBuffer,
          signature: sigContent,
          publicKey: effectivePubKey,
        });
      } catch (err) {
        throw new Error(
          `Minisign verification failed for updater artifact '${artifact.name}': ${err.message}`,
        );
      }
    }

    validatedArtifacts.push({
      artifact,
      kindDef,
      fullPath,
      fullSigPath,
      sigContent,
    });
  }

  // 5. Construct latest.json platforms mapping
  const platforms = {};
  for (const { artifact, kindDef, sigContent } of validatedArtifacts) {
    if (!kindDef.isUpdater) continue;

    for (const target of artifact.targets) {
      if (platforms[target]) {
        throw new Error(`Platform target collision: '${target}' claimed multiple times`);
      }
      platforms[target] = {
        signature: sigContent.trim(),
        url: `https://github.com/${plan.repo}/releases/download/${plan.tag}/${artifact.name}`,
      };
    }
  }

  for (const reqTarget of plan.requiredTargets) {
    if (!platforms[reqTarget]) {
      throw new Error(`Missing required platform target in updater platforms: '${reqTarget}'`);
    }
  }

  for (const platKey of Object.keys(platforms)) {
    if (!plan.requiredTargets.includes(platKey)) {
      throw new Error(`Unexpected platform target '${platKey}' in updater platforms`);
    }
  }

  const pubDateEpoch = plan.sourceDateEpoch > 1e11 ? plan.sourceDateEpoch : plan.sourceDateEpoch * 1000;
  const manifest = {
    version: plan.appVersion,
    notes: `Ferryx ${plan.appVersion}`,
    pub_date: new Date(pubDateEpoch).toISOString(),
    platforms,
  };

  // 6. Staging and atomic output publication
  const parentDir = dirname(resolvedOutDir);
  if (!existsSync(parentDir)) {
    mkdirSync(parentDir, { recursive: true });
  }

  const stagingDir = join(
    parentDir,
    `.staging-${basename(resolvedOutDir)}-${randomBytes(8).toString("hex")}`,
  );
  mkdirSync(stagingDir, { recursive: true });

  try {
    const stagedFiles = new Map();

    function stageFile(targetName, sourcePath, expectedSha256 = null) {
      if (stagedFiles.has(targetName)) {
        const existingSource = stagedFiles.get(targetName);
        if (existingSource !== sourcePath) {
          throw new Error(
            `File collision in output staging: '${targetName}' already staged from another file`,
          );
        }
        return;
      }
      const destPath = join(stagingDir, targetName);
      fs.copyFileSync(sourcePath, destPath);

      if (expectedSha256) {
        const stagedBuf = readFileSync(destPath);
        const stagedSha = createHash("sha256").update(stagedBuf).digest("hex");
        if (stagedSha !== expectedSha256) {
          throw new Error(
            `Post-copy checksum verification failed for staged file '${targetName}': expected ${expectedSha256}, got ${stagedSha}`,
          );
        }
      }

      stagedFiles.set(targetName, sourcePath);
    }

    // Write latest.json FIRST
    const latestPath = join(stagingDir, "latest.json");
    writeFileSync(latestPath, JSON.stringify(manifest, null, 2) + "\n");
    stagedFiles.set("latest.json", "manifest");

    // Stage artifacts, signatures, and stable aliases
    for (const { artifact, kindDef, fullPath, fullSigPath, sigContent } of validatedArtifacts) {
      stageFile(artifact.name, fullPath, artifact.sha256);
      const signatureHash = fullSigPath
        ? createHash("sha256").update(sigContent).digest("hex")
        : null;

      if (artifact.kind === "macos-updater") {
        const checkerPath = resolve(dirname(fileURLToPath(import.meta.url)), "assert-updater-archive-layout.mjs");
        const checkRes = spawnSync(process.execPath, [checkerPath, join(stagingDir, artifact.name)], { encoding: "utf8" });
        if (checkRes.status !== 0) {
          throw new Error(
            `macOS updater archive layout validation failed for '${artifact.name}': ${checkRes.stderr.trim() || checkRes.stdout.trim() || "exit code " + checkRes.status}`,
          );
        }
      }

      if (artifact.signatureRelPath && fullSigPath) {
        stageFile(basename(artifact.signatureRelPath), fullSigPath, signatureHash);
      }

      const alias = kindDef.stableAlias;
      if (alias) {
        stageFile(alias, fullPath, artifact.sha256);
        if (kindDef.isUpdater && fullSigPath) {
          stageFile(`${alias}.sig`, fullSigPath, signatureHash);
        }
      }
    }

    // Compute checksum closure across ALL publish files (excluding SHA256SUMS.txt itself)
    const filesToHash = readdirSync(stagingDir).filter((f) => f !== "SHA256SUMS.txt").sort();
    const sumLines = filesToHash.map((filename) => {
      const buf = readFileSync(join(stagingDir, filename));
      const hash = createHash("sha256").update(buf).digest("hex");
      return `${hash}  ${filename}`;
    });

    writeFileSync(join(stagingDir, "SHA256SUMS.txt"), sumLines.join("\n") + "\n");

    // Atomic rename only after complete verification
    renameSync(stagingDir, resolvedOutDir);
  } catch (err) {
    rmSync(stagingDir, { recursive: true, force: true });
    throw err;
  }

  const publishedFiles = readdirSync(resolvedOutDir).sort();
  return {
    outDir: resolvedOutDir,
    plan,
    receipts,
    manifest,
    files: publishedFiles,
  };
}

/**
 * CLI Entry point.
 */
function main() {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (err) {
    process.stderr.write(`${err.message}\n`);
    return 2;
  }

  // Detect legacy CLI arguments
  if (args.has("version") || args.has("dir") || args.has("repo") || args.has("tag")) {
    process.stderr.write(
      "Legacy CLI arguments are no longer supported. Receipt-driven assembly requires: " +
        "--plan <path> --receipts-dir <path> --artifacts-dir <path> --out-dir <path> [--pubkey <key>]\n",
    );
    return 2;
  }

  const planPath = args.get("plan");
  const receiptsDir = args.get("receipts-dir");
  const artifactsDir = args.get("artifacts-dir");
  const outDir = args.get("out-dir") || args.get("out");
  const publicKey = args.get("pubkey");

  if (!planPath || !receiptsDir || !artifactsDir || !outDir) {
    process.stderr.write(
      "usage: build-latest-json.mjs --plan <path> --receipts-dir <path> --artifacts-dir <path> --out-dir <path> [--pubkey <key>]\n",
    );
    return 2;
  }

  try {
    const res = assembleRelease({
      planPath,
      receiptsDir,
      artifactsDir,
      outDir,
      publicKey,
    });
    process.stdout.write(
      `Successfully assembled release ${res.plan.tag} (${res.files.length} publish files staged in ${res.outDir})\n`,
    );
    return 0;
  } catch (err) {
    process.stderr.write(`assembleRelease error: ${err.message}\n`);
    return 1;
  }
}

const isDirectExecution =
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (isDirectExecution) {
  process.exit(main());
}
