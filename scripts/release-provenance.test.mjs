import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { assembleReleaseRun, verifyReleaseRun } from "./release-local.mjs";
import {
  attachReleaseProvenance,
  CHECKSUMS_NAME,
  CHECKSUMS_SIGNATURE_NAME,
  CYCLONEDX_NPM_PACKAGE,
  generateSboms,
  minisignPublicKeyLine,
  PROVENANCE_ASSET_NAMES,
  PROVENANCE_NAME,
  RUST_SBOM_NAME,
  UI_SBOM_NAME,
  verifyChecksums,
} from "./release-provenance.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SCRIPT = join(REPO_ROOT, "scripts", "release-provenance.mjs");
const MINISIGN = process.env.MINISIGN_BIN || "minisign";
const COMMIT = "5d5499806a1b207849778f488b4e3e7b821a751b";

function sha256(buf) {
  return createHash("sha256").update(buf).digest("hex");
}

/** Unencrypted minisign test keypair; env mimics the coordinator's Tauri signing variables. */
function createTestKey(dir) {
  const pubPath = join(dir, "test.pub");
  const keyPath = join(dir, "test.key");
  execFileSync(MINISIGN, ["-G", "-W", "-p", pubPath, "-s", keyPath], { stdio: "pipe" });
  const publicKey = readFileSync(pubPath, "utf8");
  const secretKey = readFileSync(keyPath, "utf8");
  return {
    publicKey,
    pubLine: minisignPublicKeyLine(publicKey),
    keyPath,
    // Tauri stores the key base64-wrapped, exactly like TAURI_SIGNING_PRIVATE_KEY.
    env: { TAURI_SIGNING_PRIVATE_KEY: Buffer.from(secretKey).toString("base64") },
  };
}

function fakeSbomGenerator({ outDir }) {
  for (const name of [RUST_SBOM_NAME, UI_SBOM_NAME]) {
    writeFileSync(join(outDir, name), JSON.stringify({ bomFormat: "CycloneDX", specVersion: "1.5", components: [] }));
  }
  return { cargoCyclonedx: "fixture", cyclonedxNpm: "fixture", specVersion: "1.5" };
}

function plan() {
  return {
    schemaVersion: 1,
    runId: "rel-2026.09.08.1-5d549980",
    repo: "Indosaram/ferryx",
    commitSha: COMMIT,
    tag: "v2026.09.08.1",
    appVersion: "2026.908.1",
    msixVersion: "2026.908.1.0",
    channels: { store: true, nsisMigration: false },
    requiredTargets: ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"],
    toolchains: { node: ">=22.0.0", bun: ">=1.4.0", zig: "0.16.0" },
    sourceDateEpoch: 1788868800,
    createdAt: "2026-09-08T12:00:00.000Z",
  };
}

function writeAssetDir(dir) {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "Ferryx_universal.dmg"), "dmg bytes");
  writeFileSync(join(dir, "Ferryx_amd64.AppImage"), "appimage bytes");
  writeFileSync(join(dir, "ferryx-cli-linux-amd64"), "cli bytes");
  writeFileSync(join(dir, "latest.json"), "{}\n");
}

function receiptsFor(p) {
  return [
    {
      host: "omaki",
      completedAt: "2026-09-08T12:35:00.000Z",
      toolchains: { node: "22.22.0", bun: "1.4.0", zig: "0.16.0", rust: "1.88.0" },
      artifacts: [{ kind: "appimage", name: "Ferryx_amd64.AppImage", bytes: 14, sha256: "a".repeat(64) }],
    },
    {
      host: "macbook",
      completedAt: "2026-09-08T12:30:00.000Z",
      artifacts: [{ kind: "dmg", name: "Ferryx_universal.dmg", bytes: 9, sha256: "b".repeat(64) }],
    },
  ].map((r) => ({ ...r, runId: p.runId, commitSha: p.commitSha }));
}

test("fixture asset dir: SHA256SUMS passes shasum -c, minisig verifies with the test key, SBOMs and provenance present", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-provenance-"));
  try {
    const key = createTestKey(tmp);
    const dir = join(tmp, "assets");
    writeAssetDir(dir);
    const p = plan();
    attachReleaseProvenance({
      publishDir: dir,
      plan: p,
      state: { ghosttyPin: "c".repeat(40) },
      receipts: receiptsFor(p),
      sourceRepo: REPO_ROOT,
      publicKey: key.publicKey,
      env: key.env,
      sbomGenerator: fakeSbomGenerator,
    });

    for (const name of PROVENANCE_ASSET_NAMES) assert.ok(existsSync(join(dir, name)), `${name} missing`);

    // Every asset except the checksum file and its signature is listed exactly once.
    const listed = readFileSync(join(dir, CHECKSUMS_NAME), "utf8").trim().split("\n").map((l) => l.split("  ")[1]);
    const expected = readdirSync(dir).filter((n) => n !== CHECKSUMS_NAME && n !== CHECKSUMS_SIGNATURE_NAME).sort();
    assert.deepEqual(listed, expected);
    assert.ok(listed.includes(RUST_SBOM_NAME) && listed.includes(UI_SBOM_NAME) && listed.includes(PROVENANCE_NAME));

    const shasum = spawnSync("shasum", ["-a", "256", "-c", CHECKSUMS_NAME], { cwd: dir, encoding: "utf8" });
    assert.equal(shasum.status, 0, shasum.stdout + shasum.stderr);

    const verify = spawnSync(MINISIGN, ["-Vm", CHECKSUMS_NAME, "-P", key.pubLine], { cwd: dir, encoding: "utf8" });
    assert.equal(verify.status, 0, verify.stdout + verify.stderr);
    assert.match(verify.stdout, new RegExp(`ferryx ${p.tag} commit ${COMMIT}`));

    const provenance = JSON.parse(readFileSync(join(dir, PROVENANCE_NAME), "utf8"));
    assert.equal(provenance.commitSha, COMMIT);
    assert.deepEqual(provenance.builds.map((b) => b.host), ["macbook", "omaki"]);
    assert.equal(provenance.builds[1].toolchains.rust, "1.88.0");
    assert.deepEqual(provenance.builds[1].assets.map((a) => a.name), ["Ferryx_amd64.AppImage"]);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("tampering one byte of an asset makes shasum -c and the verify CLI exit non-zero", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-provenance-tamper-"));
  try {
    const key = createTestKey(tmp);
    const dir = join(tmp, "assets");
    writeAssetDir(dir);
    const seal = spawnSync(process.execPath, [SCRIPT, "seal", "--dir", dir, "--pubkey", key.publicKey], {
      encoding: "utf8",
      env: { ...process.env, ...key.env, TAURI_SIGNING_PRIVATE_KEY_PATH: "" },
    });
    assert.equal(seal.status, 0, seal.stderr);
    const ok = spawnSync(process.execPath, [SCRIPT, "verify", "--dir", dir, "--pubkey", key.publicKey], { encoding: "utf8" });
    assert.equal(ok.status, 0, ok.stderr);

    const target = join(dir, "Ferryx_universal.dmg");
    const bytes = readFileSync(target);
    bytes[0] ^= 0x01;
    writeFileSync(target, bytes);

    assert.notEqual(spawnSync("shasum", ["-a", "256", "-c", CHECKSUMS_NAME], { cwd: dir }).status, 0);
    const bad = spawnSync(process.execPath, [SCRIPT, "verify", "--dir", dir, "--pubkey", key.publicKey], { encoding: "utf8" });
    assert.equal(bad.status, 1);
    assert.match(bad.stderr, /Checksum mismatch for Ferryx_universal\.dmg/);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("verifyChecksums rejects a forged SHA256SUMS, a foreign key and an unlisted asset", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-provenance-forge-"));
  try {
    const key = createTestKey(tmp);
    const other = createTestKey(mkdtempSync(join(tmp, "other-")));
    const dir = join(tmp, "assets");
    writeAssetDir(dir);
    execFileSync(process.execPath, [SCRIPT, "seal", "--dir", dir, "--pubkey", key.publicKey], {
      env: { ...process.env, ...key.env, TAURI_SIGNING_PRIVATE_KEY_PATH: "" },
    });

    assert.throws(() => verifyChecksums({ dir, publicKey: other.publicKey }), /minisig verification failed/);

    writeFileSync(join(dir, "extra.bin"), "unlisted");
    assert.throws(() => verifyChecksums({ dir, publicKey: key.publicKey }), /not covered by SHA256SUMS: extra\.bin/);
    rmSync(join(dir, "extra.bin"));

    const sums = join(dir, CHECKSUMS_NAME);
    writeFileSync(sums, readFileSync(sums, "utf8").replace(/^[0-9a-f]/, (c) => (c === "0" ? "1" : "0")));
    assert.throws(() => verifyChecksums({ dir, publicKey: key.publicKey }), /minisig verification failed/);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("signing fails closed without the updater key or with a key that does not match the public key", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-provenance-nokey-"));
  try {
    const key = createTestKey(tmp);
    const other = createTestKey(mkdtempSync(join(tmp, "other-")));
    const dir = join(tmp, "assets");
    writeAssetDir(dir);
    const base = { publishDir: dir, plan: plan(), state: {}, receipts: [], sourceRepo: REPO_ROOT, sbomGenerator: fakeSbomGenerator };
    assert.throws(() => attachReleaseProvenance({ ...base, publicKey: key.publicKey, env: {} }), /TAURI_SIGNING_PRIVATE_KEY/);
    assert.throws(
      () => attachReleaseProvenance({ ...base, publicKey: other.publicKey, env: key.env }),
      /does not verify against the updater public key/,
    );
    assert.equal(existsSync(join(dir, CHECKSUMS_SIGNATURE_NAME)), false);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("generateSboms runs cargo cyclonedx for src-tauri and cyclonedx-npm for ui from a clean archive of the commit", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-provenance-sbom-"));
  try {
    const calls = [];
    const run = (command, args, opts = {}) => {
      calls.push({ command, args, opts });
      if (command === "git") {
        mkdirSync(dirname(args[args.indexOf("-o") + 1]), { recursive: true });
        return "";
      }
      if (command === "npm") return "/fake/npm/root\n";
      if (command === "bunx") {
        writeFileSync(args[args.indexOf("--output-file") + 1], JSON.stringify({ bomFormat: "CycloneDX" }));
        return "";
      }
      if (command === "cargo" && args[1] === "--version") return "cargo-cyclonedx 0.5.9\n";
      if (command === "cargo") {
        const manifest = args[args.indexOf("--manifest-path") + 1];
        const stem = args[args.indexOf("--override-filename") + 1];
        mkdirSync(dirname(manifest), { recursive: true });
        writeFileSync(join(dirname(manifest), `${stem}.json`), JSON.stringify({ bomFormat: "CycloneDX" }));
        return "";
      }
      return "";
    };
    const tools = generateSboms({ sourceRepo: "/repo", commitSha: COMMIT, outDir: tmp, env: {}, run });

    assert.ok(existsSync(join(tmp, RUST_SBOM_NAME)));
    assert.ok(existsSync(join(tmp, UI_SBOM_NAME)));
    assert.equal(tools.cargoCyclonedx, "cargo-cyclonedx 0.5.9");
    const git = calls.find((c) => c.command === "git");
    assert.deepEqual(git.args.slice(-4), [COMMIT, "src-tauri", "crates", "ui"]);
    const npmSbom = calls.find((c) => c.command === "bunx");
    assert.equal(npmSbom.args[0], CYCLONEDX_NPM_PACKAGE);
    assert.match(npmSbom.opts.cwd, /[/\\]ui$/);
    assert.equal(npmSbom.opts.env.npm_execpath, join("/fake/npm/root", "npm", "bin", "npm-cli.js"));
    const cargoSbom = calls.find((c) => c.command === "cargo" && c.args.includes("--manifest-path"));
    assert.match(cargoSbom.args[cargoSbom.args.indexOf("--manifest-path") + 1], /src-tauri[/\\]Cargo\.toml$/);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

// ---- release-local integration: assemble attaches provenance; verify enforces it ----

function writeRun(tmp, key) {
  const runDir = join(tmp, "run");
  const artifacts = join(runDir, "artifacts");
  const receipts = join(runDir, "receipts");
  for (const d of ["darwin", "linux", "windows"]) mkdirSync(join(artifacts, d), { recursive: true });
  mkdirSync(receipts);
  const p = plan();
  const planBytes = JSON.stringify(p, null, 2) + "\n";
  writeFileSync(join(runDir, "plan.json"), planBytes);
  const configPath = join(tmp, "hosts.json");
  writeFileSync(configPath, "{}\n");
  writeFileSync(join(runDir, "prepare-state.json"), JSON.stringify({
    schemaVersion: 1, runId: p.runId, planDigest: sha256(planBytes), configDigest: sha256(readFileSync(configPath)),
    configPath, commitSha: p.commitSha, sourceDateEpoch: p.sourceDateEpoch, ghosttyPin: "c".repeat(40),
    updaterPublicKey: key.publicKey,
  }));

  const app = join(tmp, "app");
  mkdirSync(join(app, "Ferryx.app", "Contents"), { recursive: true });
  writeFileSync(join(app, "Ferryx.app", "Contents", "Info.plist"), "fixture");
  execFileSync("tar", ["-czf", join(artifacts, "darwin", "Ferryx.app.tar.gz"), "Ferryx.app"], {
    cwd: app, env: { ...process.env, COPYFILE_DISABLE: "1" },
  });
  const files = {
    "darwin/Ferryx_2026.908.1_universal.dmg": "dmg",
    "darwin/ferryx-cli-darwin-universal": "mac cli",
    "linux/Ferryx_2026.908.1_amd64.AppImage": "appimage",
    "linux/Ferryx_2026.908.1_amd64.deb": "deb",
    "linux/ferryx-cli-linux-amd64": "linux cli",
    "windows/Ferryx_2026.908.1.0_x64.msix": "msix",
  };
  for (const [rel, body] of Object.entries(files)) writeFileSync(join(artifacts, rel), body);
  for (const rel of ["darwin/Ferryx.app.tar.gz", "linux/Ferryx_2026.908.1_amd64.AppImage"]) {
    execFileSync(MINISIGN, ["-S", "-s", key.keyPath, "-m", join(artifacts, rel), "-x", join(artifacts, `${rel}.sig`)], { input: "\n" });
  }
  const art = (kind, rel, signed, targets = []) => {
    const buf = readFileSync(join(artifacts, rel));
    return { kind, name: rel.split("/")[1], relPath: rel, bytes: buf.length, sha256: sha256(buf), signatureRelPath: signed ? `${rel}.sig` : null, targets };
  };
  const receipt = (host, artifactsList) => ({
    schemaVersion: 1, runId: p.runId, host, commitSha: p.commitSha, appVersion: p.appVersion,
    completedAt: "2026-09-08T12:30:00.000Z", exitCode: 0,
    toolchains: { node: "22.22.0", bun: "1.4.0", zig: "0.16.0", rust: "1.88.0" }, artifacts: artifactsList,
  });
  const write = (host, list) => writeFileSync(join(receipts, `build-receipt-${host}.json`), JSON.stringify(receipt(host, list)));
  write("macbook", [
    art("macos-updater", "darwin/Ferryx.app.tar.gz", true, ["darwin-aarch64", "darwin-x86_64"]),
    art("dmg", "darwin/Ferryx_2026.908.1_universal.dmg", false),
    art("cli-darwin-universal", "darwin/ferryx-cli-darwin-universal", false),
  ]);
  write("omaki", [
    art("appimage", "linux/Ferryx_2026.908.1_amd64.AppImage", true, ["linux-x86_64"]),
    art("deb", "linux/Ferryx_2026.908.1_amd64.deb", false),
    art("cli-linux-amd64", "linux/ferryx-cli-linux-amd64", false),
  ]);
  write("maho-win", [art("msix", "windows/Ferryx_2026.908.1.0_x64.msix", false)]);
  return runDir;
}

test("release-local assemble publishes signed SHA256SUMS + SBOMs + provenance, and verify rejects a tampered publish dir", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-provenance-run-"));
  const savedCi = { CI: process.env.CI, GITHUB_ACTIONS: process.env.GITHUB_ACTIONS };
  delete process.env.CI;
  delete process.env.GITHUB_ACTIONS;
  try {
    const key = createTestKey(tmp);
    const runDir = writeRun(tmp, key);
    const result = assembleReleaseRun({
      runDir, pubkey: key.publicKey, env: key.env, sbomGenerator: fakeSbomGenerator, sourceRepo: REPO_ROOT,
    });
    const publishDir = join(runDir, "publish");
    for (const name of PROVENANCE_ASSET_NAMES) assert.ok(result.files.includes(name), `${name} not published`);
    assert.ok(result.files.includes("SHA256SUMS.txt"), "legacy SHA256SUMS.txt stays for existing installers");
    assert.equal(spawnSync("shasum", ["-a", "256", "-c", CHECKSUMS_NAME], { cwd: publishDir }).status, 0);
    assert.equal(spawnSync(MINISIGN, ["-Vm", CHECKSUMS_NAME, "-P", key.pubLine], { cwd: publishDir }).status, 0);

    // On darwin, verify ends with a Gatekeeper check the unsigned fixture DMG cannot pass; it runs
    // after the provenance checks, so reaching it proves those passed.
    if (process.platform === "darwin") {
      await assert.rejects(() => verifyReleaseRun({ runDir, pubkey: key.publicKey }), /Gatekeeper spctl/);
    } else {
      assert.equal((await verifyReleaseRun({ runDir, pubkey: key.publicKey })).ok, true);
    }

    // Tamper the provenance asset (outside the byte-for-byte re-derivation) and re-seal the
    // assembly digest, so only the signed-checksum check can catch it.
    const provPath = join(publishDir, PROVENANCE_NAME);
    writeFileSync(provPath, readFileSync(provPath, "utf8").replace(COMMIT, "f".repeat(40)));
    const statePath = join(runDir, "assembly-state.json");
    const state = JSON.parse(readFileSync(statePath, "utf8"));
    const digest = createHash("sha256");
    for (const name of readdirSync(publishDir).sort()) digest.update(name).update("\0").update(readFileSync(join(publishDir, name)));
    state.publishDigest = digest.digest("hex");
    writeFileSync(statePath, JSON.stringify(state));
    await assert.rejects(() => verifyReleaseRun({ runDir, pubkey: key.publicKey }), /Checksum mismatch for provenance\.json/);
  } finally {
    for (const [k, v] of Object.entries(savedCi)) if (v !== undefined) process.env[k] = v;
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("release-local assemble leaves no publish directory when signing fails", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-provenance-run-fail-"));
  const savedCi = { CI: process.env.CI, GITHUB_ACTIONS: process.env.GITHUB_ACTIONS };
  delete process.env.CI;
  delete process.env.GITHUB_ACTIONS;
  try {
    const key = createTestKey(tmp);
    const runDir = writeRun(tmp, key);
    assert.throws(
      () => assembleReleaseRun({ runDir, pubkey: key.publicKey, env: {}, sbomGenerator: fakeSbomGenerator, sourceRepo: REPO_ROOT }),
      /TAURI_SIGNING_PRIVATE_KEY/,
    );
    assert.equal(existsSync(join(runDir, "publish")), false);
    assert.equal(existsSync(join(runDir, "assembly-state.json")), false);
  } finally {
    for (const [k, v] of Object.entries(savedCi)) if (v !== undefined) process.env[k] = v;
    rmSync(tmp, { recursive: true, force: true });
  }
});
