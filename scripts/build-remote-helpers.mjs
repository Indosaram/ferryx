import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, mkdtempSync, renameSync, rmSync, readdirSync } from "node:fs";
import { resolve, join, dirname, relative, isAbsolute } from "node:path";

const REPO_ROOT = resolve(import.meta.dirname, "..");
const TARGETS = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc"];
export const REQUIRED_HELPER_CAPABILITIES = Object.freeze(["sshHelperV1", "dagSubscribeV1", "agentStateV1", "ptyRecoveryV1"]);

export function computeSha256(buffer) {
  return createHash("sha256").update(buffer).digest("hex");
}

function findRsFiles(dir) {
  assert(existsSync(dir), `Helper source directory missing: ${dir}`);
  const entries = readdirSync(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    // macOS bsdtar writes AppleDouble sidecars (`._name.rs`) when a snapshot is
    // unpacked on a foreign host. Their bytes carry host metadata, so counting
    // them would make the closure — and every fingerprint derived from it —
    // depend on which host happened to extract the tree. They are never source.
    if (entry.name.startsWith("._")) continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...findRsFiles(full));
    } else if (entry.name.endsWith(".rs")) {
      files.push(full);
    }
  }
  return files;
}

export function computeHelperSourceClosure(repoRoot = REPO_ROOT) {
  const fixed = [
    "remote-helper/Cargo.lock",
    "remote-helper/Cargo.toml",
    "src-tauri/src/dag/journal.rs",
    "src-tauri/src/dag/paths.rs",
    "src-tauri/src/scoped_contracts.rs",
  ];
  const sshDir = join(repoRoot, "src-tauri/src/ferryx_scope/ssh");
  assert(existsSync(sshDir), `Helper ssh source directory missing: ${sshDir}`);
  const sshFiles = findRsFiles(sshDir).map(p => relative(repoRoot, p).replace(/\\/g, "/"));
  const all = [...fixed, ...sshFiles];
  all.sort();
  return all;
}

export function computeHelperSourceFingerprint(repoRoot = REPO_ROOT) {
  const closure = computeHelperSourceClosure(repoRoot);
  const hash = createHash("sha256");
  const sources = [];
  for (const relPath of closure) {
    const full = resolve(repoRoot, relPath);
    assert(existsSync(full), `Helper source closure file missing: ${relPath}`);
    const fileBytes = readFileSync(full);
    const fileSha = computeSha256(fileBytes);
    hash.update(`${relPath}:${fileSha}\n`);
    sources.push({ path: relPath, sha256: fileSha });
  }
  return { fingerprint: hash.digest("hex"), sources };
}

// Capability evidence must be measured by executing the built artifact on its
// build host (`--capabilities`, plus a real handshake where one is available). A
// byte scan of the image cannot stand in for that: the compiler legitimately
// splits or merges string constants, so a missing token is not evidence of a
// missing capability (2026-09-30: release artifacts that advertise every
// capability over a live handshake contain no contiguous capability token).
// Receipts must come from the build owner, not discovery of old target files.
// This stages, never compiles, installs, or infers an ABI from a filename.
export function stageHelpers({ repoRoot = REPO_ROOT, resourcesDir = join(repoRoot, "src-tauri/resources/helpers"), requiredTargets = TARGETS, receipts = [], profile = "release" } = {}) {
  assert(requiredTargets.length > 0 && new Set(requiredTargets).size === requiredTargets.length);
  assert.equal(receipts.length, requiredTargets.length, "complete build receipt matrix required");
  assert.equal(new Set(receipts.map(r => r.target)).size, receipts.length, "duplicate build receipt");
  const version = readFileSync(join(repoRoot, "remote-helper/Cargo.toml"), "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  assert(version, "helper package version missing");

  const expectedClosure = computeHelperSourceClosure(repoRoot);
  const { fingerprint: sourceFingerprint } = computeHelperSourceFingerprint(repoRoot);

  const artifacts = requiredTargets.map(target => {
    assert(TARGETS.includes(target), "unsupported target");
    const receipt = receipts.find(r => r.target === target);
    assert(receipt, `missing receipt: ${target}`);
    assert.equal(receipt.profile, profile, "profile mismatch");
    assert(["release", "debug"].includes(profile));

    // Verify source closure completeness and freshness against actual repository source tree
    assert(receipt.sources && Array.isArray(receipt.sources), "source receipt list required");
    const receiptSourcePaths = receipt.sources.map(s => s.path).sort();
    assert.deepEqual(receiptSourcePaths, expectedClosure, `source closure mismatch for ${target}`);

    for (const source of receipt.sources) {
      const path = resolve(repoRoot, source.path);
      const rel = relative(repoRoot, path);
      assert(!isAbsolute(rel) && rel !== ".." && !rel.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`), "source outside repository");
      assert.equal(computeSha256(readFileSync(path)), source.sha256, `source changed: ${source.path}`);
    }

    if (receipt.sourceFingerprint) {
      assert.equal(receipt.sourceFingerprint, sourceFingerprint, `receipt source fingerprint mismatch for ${target}`);
    }

    const bytes = readFileSync(receipt.executable);
    assert(bytes.length > 0);
    assert.equal(computeSha256(bytes), receipt.sha256, "artifact hash mismatch");

    const windows = target.endsWith("windows-msvc");
    const darwin = target.endsWith("apple-darwin");
    if (windows) {
      assert(bytes.length >= 64 && bytes.toString("ascii", 0, 2) === "MZ", "missing PE header");
      const pe = bytes.readUInt32LE(60);
      assert(pe + 6 <= bytes.length && bytes.toString("ascii", pe, pe + 4) === "PE\0\0", "invalid PE header");
      assert.equal(bytes.readUInt16LE(pe + 4), 0x8664, "PE machine mismatch");
    } else if (darwin) {
      assert(bytes.length >= 16, "truncated Mach-O header");
      assert.equal(bytes.readUInt32LE(0), 0xfeedfacf, "missing 64-bit Mach-O magic");
      assert.equal(bytes.readUInt32LE(4), target.startsWith("aarch64") ? 0x0100000c : 0x01000007, "Mach-O cputype mismatch");
    } else {
      assert(bytes.length >= 20 && bytes.subarray(0, 4).equals(Buffer.from([127, 69, 76, 70])), "missing ELF header");
      assert.equal(bytes[4], 2, "ELF must be 64 bit");
      assert.equal(bytes[5], 1, "ELF must be little endian");
      assert.equal(bytes.readUInt16LE(18), target.startsWith("aarch64") ? 183 : 62, "ELF machine mismatch");
    }

    assert.ok(Array.isArray(receipt.capabilities), `capability evidence required for ${target}: run the built artifact's --capabilities on its build host`);
    assert.ok(receipt.capabilities.every(cap => typeof cap === "string" && cap.length > 0), `capability evidence for ${target} must be non-empty strings`);
    for (const cap of REQUIRED_HELPER_CAPABILITIES) {
      assert.ok(receipt.capabilities.includes(cap), `helper artifact for ${target} does not advertise required capability '${cap}': ${JSON.stringify(receipt.capabilities)}`);
    }
    assert.equal(receipt.helperVersion, version, `artifact version mismatch for ${target}: ${receipt.helperVersion} != ${version}`);
    if (receipt.runtimeCapabilities !== undefined) {
      assert.ok(Array.isArray(receipt.runtimeCapabilities), `runtimeCapabilities must be an array for ${target}`);
      assert.ok(receipt.runtimeCapabilities.every(cap => receipt.capabilities.includes(cap)), `runtime capabilities exceed the advertised compile surface for ${target}`);
    }

    return {
      target,
      filename: windows ? "ferryx-remote-helper.exe" : "ferryx-remote-helper",
      sha256: receipt.sha256,
      byteLength: bytes.length,
      capabilities: [...new Set(receipt.capabilities)],
      executable: receipt.executable,
    };
  });
  // Existing published resources are never evidence of a build. Refuse replacement
  // rather than leaving a mixed manifest/artifact generation after a failed copy.
  assert(!existsSync(resourcesDir), "destination already exists; use a new owned staging directory");
  mkdirSync(dirname(resourcesDir), { recursive: true });
  const temporary = mkdtempSync(join(dirname(resourcesDir), ".helpers-stage-"));
  try {
    for (const artifact of artifacts) {
      const dir = join(temporary, artifact.target); mkdirSync(dir);
      const path = join(dir, artifact.filename); copyFileSync(artifact.executable, path);
      assert.equal(computeSha256(readFileSync(path)), artifact.sha256, "artifact changed during copy");
    }
    const manifest = {
      schemaVersion: 1,
      helperVersion: version,
      protocolVersion: 1,
      sourceFingerprint,
      requiredCapabilities: [...REQUIRED_HELPER_CAPABILITIES],
      artifacts: artifacts.map(({ executable, ...artifact }) => artifact),
    };
    writeFileSync(join(temporary, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
    renameSync(temporary, resourcesDir);
    return manifest;
  } catch (error) {
    rmSync(temporary, { recursive: true, force: true });
    throw error;
  }
}

export function validateBundledHelpers({ repoRoot = REPO_ROOT, resourcesDir = join(repoRoot, "src-tauri/resources/helpers"), requiredTargets = TARGETS } = {}) {
  const manifestPath = join(resourcesDir, "manifest.json");
  if (!existsSync(manifestPath)) {
    // Standalone development/test without staged helper resources is permitted.
    return { ok: true, reason: "no_manifest" };
  }
  const manifestRaw = readFileSync(manifestPath, "utf8");
  let manifest;
  try {
    manifest = JSON.parse(manifestRaw);
  } catch (error) {
    throw new Error(`Invalid JSON in helper manifest at ${manifestPath}: ${error.message}`);
  }

  assert(manifest.sourceFingerprint, "bundled helper manifest missing sourceFingerprint; stale pre-fingerprint receipt");
  const { fingerprint: expectedFingerprint } = computeHelperSourceFingerprint(repoRoot);
  assert.equal(
    manifest.sourceFingerprint,
    expectedFingerprint,
    `stale bundled helper assets detected: manifest sourceFingerprint '${manifest.sourceFingerprint}' does not match current repository helper source fingerprint '${expectedFingerprint}'. Rebuild and stage remote helpers before release packaging.`,
  );

  const expectedHelperVersion = readFileSync(join(repoRoot, "remote-helper/Cargo.toml"), "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  assert.equal(
    manifest.helperVersion,
    expectedHelperVersion,
    `bundled helper version '${manifest.helperVersion}' does not match remote-helper/Cargo.toml '${expectedHelperVersion}'; the release runtime rejects this bundle, so restage helpers from the release commit`,
  );

  assert(Array.isArray(manifest.artifacts) && manifest.artifacts.length > 0, "empty helper artifacts in manifest");
  for (const artifact of manifest.artifacts) {
    const binaryPath = join(resourcesDir, artifact.target, artifact.filename);
    assert(existsSync(binaryPath), `bundled helper executable missing for target ${artifact.target} at ${binaryPath}`);
    const bytes = readFileSync(binaryPath);
    assert.equal(computeSha256(bytes), artifact.sha256, `checksum mismatch for bundled helper ${artifact.target}`);
    assert.equal(bytes.length, artifact.byteLength, `byteLength mismatch for bundled helper ${artifact.target}`);
    // The coordinator cannot execute a foreign-arch artifact, so the release gate
    // verifies the capability evidence recorded from the build owner's execution
    // against the bytes it just hashed, rather than scanning the image.
    // The canonical contract is enforced unconditionally: a manifest that declares
    // an empty or narrowed requiredCapabilities must not relax this check.
    const recorded = new Set(Array.isArray(artifact.capabilities) ? artifact.capabilities : []);
    for (const cap of REQUIRED_HELPER_CAPABILITIES) {
      assert.ok(recorded.has(cap), `bundled helper ${artifact.target} lacks recorded capability '${cap}'`);
    }
  }

  // The shipped directory is the trust artifact, so the target matrix is verified
  // here too: a narrowed or duplicated artifact set must not pass the release gate.
  const shippedTargets = manifest.artifacts.map(artifact => artifact.target).sort();
  assert.deepEqual(shippedTargets, [...requiredTargets].sort(), `bundled helper target matrix mismatch: shipped ${JSON.stringify(shippedTargets)}`);

  return { ok: true, sourceFingerprint: expectedFingerprint };
}

if (import.meta.main) {
  const arg = process.argv[2];
  if (arg === "--check-bundled") {
    const repoRoot = process.argv[3] ? resolve(process.argv[3]) : REPO_ROOT;
    try {
      const res = validateBundledHelpers({ repoRoot });
      if (res.reason === "no_manifest") {
        console.log("[helper-freshness-gate] No helper manifest found; skipped development build gate.");
      } else {
        console.log(`[helper-freshness-gate] Bundled helper assets verified match source fingerprint ${res.sourceFingerprint}.`);
      }
      process.exit(0);
    } catch (error) {
      console.error(`\n[helper-freshness-gate ERROR] ${error.message}\n`);
      process.exit(1);
    }
  }

  assert(arg, "explicit build-receipt JSON required (or --check-bundled [repoRoot]); no automatic build or fallback staging");
  stageHelpers(JSON.parse(readFileSync(arg, "utf8")));
}
