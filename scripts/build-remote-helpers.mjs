import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, mkdtempSync, renameSync, rmSync } from "node:fs";
import { resolve, join, dirname, relative, isAbsolute } from "node:path";

const REPO_ROOT = resolve(import.meta.dirname, "..");
const TARGETS = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "x86_64-pc-windows-msvc"];
export function computeSha256(buffer) {
  return createHash("sha256").update(buffer).digest("hex");
}

// Receipts must come from the build owner, not discovery of old target files.
// This stages, never compiles, installs, or infers an ABI from a filename.
export function stageHelpers({ repoRoot = REPO_ROOT, resourcesDir = join(repoRoot, "src-tauri/resources/helpers"), requiredTargets = TARGETS, receipts = [], profile = "release" } = {}) {
  assert(requiredTargets.length > 0 && new Set(requiredTargets).size === requiredTargets.length);
  assert.equal(receipts.length, requiredTargets.length, "complete build receipt matrix required");
  assert.equal(new Set(receipts.map(r => r.target)).size, receipts.length, "duplicate build receipt");
  const version = readFileSync(join(repoRoot, "remote-helper/Cargo.toml"), "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  assert(version, "helper package version missing");
  const artifacts = requiredTargets.map(target => {
    assert(TARGETS.includes(target), "unsupported target");
    const receipt = receipts.find(r => r.target === target);
    assert(receipt, `missing receipt: ${target}`);
    assert.equal(receipt.profile, profile, "profile mismatch");
    assert(["release", "debug"].includes(profile));
    assert(receipt.sources?.some(s => s.path === "remote-helper/Cargo.lock"), "dependency lock receipt required");
    assert(receipt.sources.some(s => s.path === "remote-helper/Cargo.toml"), "manifest receipt required");
    for (const source of receipt.sources) {
      const path = resolve(repoRoot, source.path);
      const rel = relative(repoRoot, path);
      assert(!isAbsolute(rel) && rel !== ".." && !rel.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`), "source outside repository");
      assert.equal(computeSha256(readFileSync(path)), source.sha256, `source changed: ${source.path}`);
    }
    const bytes = readFileSync(receipt.executable);
    assert(bytes.length > 0);
    assert.equal(computeSha256(bytes), receipt.sha256, "artifact hash mismatch");
    const windows = target.endsWith("windows-msvc");
    if (windows) {
      assert(bytes.length >= 64 && bytes.toString("ascii", 0, 2) === "MZ", "missing PE header");
      const pe = bytes.readUInt32LE(60);
      assert(pe + 6 <= bytes.length && bytes.toString("ascii", pe, pe + 4) === "PE\0\0", "invalid PE header");
      assert.equal(bytes.readUInt16LE(pe + 4), 0x8664, "PE machine mismatch");
    } else {
      assert(bytes.length >= 20 && bytes.subarray(0, 4).equals(Buffer.from([127, 69, 76, 70])), "missing ELF header");
      assert.equal(bytes[4], 2, "ELF must be 64 bit");
      assert.equal(bytes[5], 1, "ELF must be little endian");
      assert.equal(bytes.readUInt16LE(18), target.startsWith("aarch64") ? 183 : 62, "ELF machine mismatch");
    }
    return { target, filename: windows ? "ferryx-remote-helper.exe" : "ferryx-remote-helper", sha256: receipt.sha256, byteLength: bytes.length, executable: receipt.executable };
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
    const manifest = { schemaVersion: 1, helperVersion: version, protocolVersion: 1, artifacts: artifacts.map(({ executable, ...artifact }) => artifact) };
    writeFileSync(join(temporary, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
    renameSync(temporary, resourcesDir);
    return manifest;
  } catch (error) {
    rmSync(temporary, { recursive: true, force: true });
    throw error;
  }
}

if (import.meta.main) {
  const receiptPath = process.argv[2];
  assert(receiptPath, "explicit build-receipt JSON required; no automatic build or fallback staging");
  stageHelpers(JSON.parse(readFileSync(receiptPath, "utf8")));
}
