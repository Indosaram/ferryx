import assert from "node:assert/strict";
import childProcess, { execFileSync, spawnSync } from "node:child_process";
import { createHash, generateKeyPairSync, randomBytes, sign } from "node:crypto";
import fs, {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { syncBuiltinESMExports } from "node:module";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { assembleRelease } from "./build-latest-json.mjs";
import { verifyMinisign } from "./lib/minisign-verify.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SCRIPT = join(REPO_ROOT, "scripts", "build-latest-json.mjs");
const FIXTURE_ARCHIVE_PATH = join(REPO_ROOT, "scripts", "fixtures", "updater", "Ferryx.app.tar.gz");
const FIXTURE_SIG_PATH = join(REPO_ROOT, "scripts", "fixtures", "updater", "Ferryx.app.tar.gz.sig");
const TAURI_CONF_PATH = join(REPO_ROOT, "src-tauri", "tauri.conf.json");

/**
 * Generates an independent test keypair and signs data using pure Node crypto.
 */
function createMinisignTestKeys() {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const rawPub = publicKey.export({ format: "der", type: "spki" }).subarray(12);
  const keyId = randomBytes(8);
  const keyIdHex = Buffer.from(keyId).reverse().toString("hex").toUpperCase();

  const pubBin = Buffer.concat([Buffer.from("Ed", "utf8"), keyId, rawPub]);
  const pubB64 = pubBin.toString("base64");
  const pubText = `untrusted comment: minisign public key ${keyIdHex}\n${pubB64}\n`;
  const pubWrapped = Buffer.from(pubText, "utf8").toString("base64");

  function signPayload(data, comment = "test trusted comment") {
    const dataBuf = Buffer.isBuffer(data) ? data : Buffer.from(data);
    const blakeHash = createHash("blake2b512").update(dataBuf).digest();
    const payloadSig = sign(null, blakeHash, privateKey);
    const sigBin = Buffer.concat([Buffer.from("ED", "utf8"), keyId, payloadSig]);
    const sigB64 = sigBin.toString("base64");

    const globalMsg = Buffer.concat([payloadSig, Buffer.from(comment, "utf8")]);
    const globalSig = sign(null, globalMsg, privateKey);
    const globalSigB64 = globalSig.toString("base64");

    return [
      "untrusted comment: signature from test key",
      sigB64,
      `trusted comment: ${comment}`,
      globalSigB64,
      "",
    ].join("\n");
  }

  return { publicKeyWrapped: pubWrapped, publicKeyText: pubText, signPayload };
}

function sha256(buf) {
  return createHash("sha256").update(buf).digest("hex");
}

function createValidUpdaterArchive() {
  const tmp = mkdtempSync(join(tmpdir(), "mac-updater-fixture-"));
  const app = join(tmp, "Ferryx.app");
  mkdirSync(join(app, "Contents"), { recursive: true });
  writeFileSync(join(app, "Contents", "Info.plist"), "fixture");
  const archivePath = join(tmp, "Ferryx.app.tar.gz");
  execFileSync("tar", ["-czf", archivePath, "Ferryx.app"], {
    cwd: tmp,
    env: { ...process.env, COPYFILE_DISABLE: "1" },
  });
  const buf = readFileSync(archivePath);
  rmSync(tmp, { recursive: true, force: true });
  return buf;
}

/**
 * Helper to build a complete synthetic release fixture directory structure.
 */
function createSyntheticReleaseFixture({
  tag = "v2026.09.08.1",
  appVersion = "2026.908.1",
  msixVersion = "2026.908.1.0",
  runId = "rel-20260908-1",
  commitSha = "5d5499806a1b207849778f488b4e3e7b821a751b",
  nsisMigration = false,
  keys = createMinisignTestKeys(),
} = {}) {
  const rootDir = mkdtempSync(join(tmpdir(), "ferryx-release-fixture-"));
  const receiptsDir = join(rootDir, "receipts");
  const artifactsDir = join(rootDir, "artifacts");
  mkdirSync(receiptsDir, { recursive: true });
  mkdirSync(artifactsDir, { recursive: true });

  const requiredTargets = ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"];
  if (nsisMigration) {
    requiredTargets.push("windows-x86_64");
  }

  const plan = {
    schemaVersion: 1,
    runId,
    repo: "Indosaram/ferryx",
    commitSha,
    tag,
    appVersion,
    msixVersion,
    channels: {
      store: true,
      nsisMigration,
    },
    requiredTargets,
    toolchains: {
      node: ">=22.0.0",
      bun: ">=1.4.0",
      zig: "0.16.0",
      rust: "1.85.0",
      tauri: "2.3.0",
    },
    sourceDateEpoch: 1788868800,
    createdAt: "2026-09-08T12:00:00.000Z",
  };
  const planPath = join(rootDir, "release-plan.json");
  writeFileSync(planPath, JSON.stringify(plan, null, 2) + "\n");

  // macOS artifacts
  const macDir = join(artifactsDir, "darwin");
  mkdirSync(macDir, { recursive: true });
  const macUpdaterBuf = createValidUpdaterArchive();
  const macUpdaterSig = keys.signPayload(macUpdaterBuf, "macos updater sig");
  writeFileSync(join(macDir, "Ferryx.app.tar.gz"), macUpdaterBuf);
  writeFileSync(join(macDir, "Ferryx.app.tar.gz.sig"), macUpdaterSig);

  const macDmgBuf = Buffer.from("mac-dmg-installer-content-67890");
  writeFileSync(join(macDir, "Ferryx_2026.908.1_universal.dmg"), macDmgBuf);

  const macReceipt = {
    schemaVersion: 1,
    runId,
    host: "macbook",
    commitSha,
    appVersion,
    completedAt: "2026-09-08T12:30:00.000Z",
    exitCode: 0,
    toolchains: {
      node: "22.22.0",
      bun: "1.4.0",
      zig: "0.14.0",
      rust: "1.82.0",
      tauri: "2.3.0",
    },
    artifacts: [
      {
        kind: "macos-updater",
        name: "Ferryx.app.tar.gz",
        relPath: "darwin/Ferryx.app.tar.gz",
        bytes: macUpdaterBuf.length,
        sha256: sha256(macUpdaterBuf),
        signatureRelPath: "darwin/Ferryx.app.tar.gz.sig",
        targets: ["darwin-aarch64", "darwin-x86_64"],
      },
      {
        kind: "dmg",
        name: "Ferryx_2026.908.1_universal.dmg",
        relPath: "darwin/Ferryx_2026.908.1_universal.dmg",
        bytes: macDmgBuf.length,
        sha256: sha256(macDmgBuf),
        signatureRelPath: null,
        targets: [],
      },
    ],
  };
  writeFileSync(join(receiptsDir, "build-receipt-macbook.json"), JSON.stringify(macReceipt, null, 2) + "\n");

  // Linux artifacts
  const linuxDir = join(artifactsDir, "linux");
  mkdirSync(linuxDir, { recursive: true });
  const appimageBuf = Buffer.from("linux-appimage-payload-content-abcdef");
  const appimageSig = keys.signPayload(appimageBuf, "linux appimage sig");
  writeFileSync(join(linuxDir, "Ferryx_2026.908.1_amd64.AppImage"), appimageBuf);
  writeFileSync(join(linuxDir, "Ferryx_2026.908.1_amd64.AppImage.sig"), appimageSig);

  const debBuf = Buffer.from("linux-deb-installer-content-fedcba");
  writeFileSync(join(linuxDir, "Ferryx_2026.908.1_amd64.deb"), debBuf);

  const linuxReceipt = {
    schemaVersion: 1,
    runId,
    host: "omaki",
    commitSha,
    appVersion,
    completedAt: "2026-09-08T12:35:00.000Z",
    exitCode: 0,
    toolchains: {
      node: "22.22.0",
      bun: "1.4.0",
      zig: "0.14.0",
      rust: "1.82.0",
      tauri: "2.3.0",
    },
    artifacts: [
      {
        kind: "appimage",
        name: "Ferryx_2026.908.1_amd64.AppImage",
        relPath: "linux/Ferryx_2026.908.1_amd64.AppImage",
        bytes: appimageBuf.length,
        sha256: sha256(appimageBuf),
        signatureRelPath: "linux/Ferryx_2026.908.1_amd64.AppImage.sig",
        targets: ["linux-x86_64"],
      },
      {
        kind: "deb",
        name: "Ferryx_2026.908.1_amd64.deb",
        relPath: "linux/Ferryx_2026.908.1_amd64.deb",
        bytes: debBuf.length,
        sha256: sha256(debBuf),
        signatureRelPath: null,
        targets: [],
      },
    ],
  };
  writeFileSync(join(receiptsDir, "build-receipt-omaki.json"), JSON.stringify(linuxReceipt, null, 2) + "\n");

  // Windows artifacts
  const winDir = join(artifactsDir, "windows");
  mkdirSync(winDir, { recursive: true });
  const msixBuf = Buffer.from("windows-msix-installer-content-store");
  writeFileSync(join(winDir, "Ferryx_2026.908.1.0_x64.msix"), msixBuf);

  const winArtifacts = [
    {
      kind: "msix",
      name: "Ferryx_2026.908.1.0_x64.msix",
      relPath: "windows/Ferryx_2026.908.1.0_x64.msix",
      bytes: msixBuf.length,
      sha256: sha256(msixBuf),
      signatureRelPath: null,
      targets: [],
    },
  ];

  if (nsisMigration) {
    const nsisBuf = Buffer.from("windows-nsis-setup-payload-exe");
    const nsisSig = keys.signPayload(nsisBuf, "windows nsis sig");
    writeFileSync(join(winDir, "Ferryx_2026.908.1_x64-setup.exe"), nsisBuf);
    writeFileSync(join(winDir, "Ferryx_2026.908.1_x64-setup.exe.sig"), nsisSig);

    winArtifacts.push({
      kind: "nsis",
      name: "Ferryx_2026.908.1_x64-setup.exe",
      relPath: "windows/Ferryx_2026.908.1_x64-setup.exe",
      bytes: nsisBuf.length,
      sha256: sha256(nsisBuf),
      signatureRelPath: "windows/Ferryx_2026.908.1_x64-setup.exe.sig",
      targets: ["windows-x86_64"],
    });
  }

  const winReceipt = {
    schemaVersion: 1,
    runId,
    host: "maho-win",
    commitSha,
    appVersion,
    completedAt: "2026-09-08T12:40:00.000Z",
    exitCode: 0,
    toolchains: {
      node: "22.22.0",
      bun: "1.4.0",
      zig: "0.14.0",
      rust: "1.82.0",
      tauri: "2.3.0",
    },
    artifacts: winArtifacts,
  };
  writeFileSync(join(receiptsDir, "build-receipt-maho-win.json"), JSON.stringify(winReceipt, null, 2) + "\n");

  return {
    rootDir,
    planPath,
    receiptsDir,
    artifactsDir,
    keys,
    cleanup() {
      rmSync(rootDir, { recursive: true, force: true });
    },
  };
}

// ----------------- Legacy CLI Tests -----------------

test("legacy CLI invocation fails closed with clear migration instructions", () => {
  const result = spawnSync(
    process.execPath,
    [
      SCRIPT,
      "--version",
      "2026.09.08.1",
      "--dir",
      "/tmp",
      "--out",
      "/tmp/latest.json",
      "--repo",
      "Indosaram/ferryx",
      "--tag",
      "v2026.09.08.1",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /legacy cli arguments are no longer supported/i);
  assert.match(result.stderr, /--plan/i);
});

// ----------------- assembleRelease API Tests -----------------

test("assembleRelease creates complete release inventory, latest.json first, stable aliases and SHA256SUMS closure", () => {
  const fixture = createSyntheticReleaseFixture();
  const outDir = join(fixture.rootDir, "publish-out");
  try {
    const res = assembleRelease({
      planPath: fixture.planPath,
      receiptsDir: fixture.receiptsDir,
      artifactsDir: fixture.artifactsDir,
      outDir,
      publicKey: fixture.keys.publicKeyWrapped,
    });

    assert.equal(existsSync(outDir), true);
    assert.equal(existsSync(join(outDir, "latest.json")), true);
    assert.equal(existsSync(join(outDir, "SHA256SUMS.txt")), true);

    // Check latest.json
    const manifest = JSON.parse(readFileSync(join(outDir, "latest.json"), "utf8"));
    assert.equal(manifest.version, "2026.908.1");
    assert.equal(manifest.pub_date, new Date(1788868800 * 1000).toISOString());
    assert.deepEqual(Object.keys(manifest.platforms).sort(), [
      "darwin-aarch64",
      "darwin-x86_64",
      "linux-x86_64",
    ]);

    // Check deterministic stable aliases
    assert.equal(existsSync(join(outDir, "Ferryx_universal.dmg")), true);
    assert.equal(existsSync(join(outDir, "Ferryx_amd64.AppImage")), true);
    assert.equal(existsSync(join(outDir, "Ferryx_amd64.AppImage.sig")), true);
    assert.equal(existsSync(join(outDir, "Ferryx_amd64.deb")), true);
    assert.equal(existsSync(join(outDir, "Ferryx_x64.msix")), true);

    // Original artifacts present
    assert.equal(existsSync(join(outDir, "Ferryx_2026.908.1_universal.dmg")), true);
    assert.equal(existsSync(join(outDir, "Ferryx_2026.908.1_amd64.AppImage")), true);

    // Checksum closure: all files in outDir except SHA256SUMS.txt must be listed
    const checksums = readFileSync(join(outDir, "SHA256SUMS.txt"), "utf8").trim().split("\n");
    const listedFiles = checksums.map((l) => l.split(/\s+/)[1]).sort();
    const actualFiles = res.files.filter((f) => f !== "SHA256SUMS.txt").sort();

    assert.deepEqual(listedFiles, actualFiles);
    assert.ok(listedFiles.includes("latest.json"));

    // Verify all checksums match actual files
    for (const line of checksums) {
      const [expectedHash, filename] = line.split(/\s+/);
      const actualHash = sha256(readFileSync(join(outDir, filename)));
      assert.equal(actualHash, expectedHash, `Checksum mismatch for ${filename}`);
    }
  } finally {
    fixture.cleanup();
  }
});

test("assembleRelease supports NSIS migration channel with windows updater target", () => {
  const fixture = createSyntheticReleaseFixture({ nsisMigration: true });
  const outDir = join(fixture.rootDir, "publish-out-nsis");
  try {
    const res = assembleRelease({
      planPath: fixture.planPath,
      receiptsDir: fixture.receiptsDir,
      artifactsDir: fixture.artifactsDir,
      outDir,
      publicKey: fixture.keys.publicKeyWrapped,
    });

    const manifest = JSON.parse(readFileSync(join(outDir, "latest.json"), "utf8"));
    assert.deepEqual(Object.keys(manifest.platforms).sort(), [
      "darwin-aarch64",
      "darwin-x86_64",
      "linux-x86_64",
      "windows-x86_64",
    ]);
    assert.equal(existsSync(join(outDir, "Ferryx_x64-setup.exe")), true);
    assert.equal(existsSync(join(outDir, "Ferryx_x64-setup.exe.sig")), true);
  } finally {
    fixture.cleanup();
  }
});

test("assembleRelease fails closed if outDir already exists", () => {
  const fixture = createSyntheticReleaseFixture();
  const outDir = join(fixture.rootDir, "existing-dir");
  mkdirSync(outDir, { recursive: true });
  try {
    assert.throws(
      () =>
        assembleRelease({
          planPath: fixture.planPath,
          receiptsDir: fixture.receiptsDir,
          artifactsDir: fixture.artifactsDir,
          outDir,
          publicKey: fixture.keys.publicKeyWrapped,
        }),
      /output directory already exists/i,
    );
  } finally {
    fixture.cleanup();
  }
});

test("assembleRelease fails closed and leaves no partial output on signature verification failure", () => {
  const fixture = createSyntheticReleaseFixture();
  const outDir = join(fixture.rootDir, "publish-tampered-sig");

  // Tamper signature
  const macSigPath = join(fixture.artifactsDir, "darwin", "Ferryx.app.tar.gz.sig");
  writeFileSync(macSigPath, "tampered-bad-signature\n");

  try {
    assert.throws(
      () =>
        assembleRelease({
          planPath: fixture.planPath,
          receiptsDir: fixture.receiptsDir,
          artifactsDir: fixture.artifactsDir,
          outDir,
          publicKey: fixture.keys.publicKeyWrapped,
        }),
      /minisign/i,
    );

    // Verify outDir was not left behind
    assert.equal(existsSync(outDir), false);
  } finally {
    fixture.cleanup();
  }
});

test("assembleRelease fails closed and leaves no partial output on artifact content / hash tampering", () => {
  const fixture = createSyntheticReleaseFixture();
  const outDir = join(fixture.rootDir, "publish-tampered-hash");

  // Tamper mac updater payload
  const macPayloadPath = join(fixture.artifactsDir, "darwin", "Ferryx.app.tar.gz");
  writeFileSync(macPayloadPath, "corrupted-payload-content");

  try {
    assert.throws(
      () =>
        assembleRelease({
          planPath: fixture.planPath,
          receiptsDir: fixture.receiptsDir,
          artifactsDir: fixture.artifactsDir,
          outDir,
          publicKey: fixture.keys.publicKeyWrapped,
        }),
      /mismatch/i,
    );

    assert.equal(existsSync(outDir), false);
  } finally {
    fixture.cleanup();
  }
});

test("assembleRelease fails closed if a receipt has mismatched commitSha or appVersion", () => {
  const fixture = createSyntheticReleaseFixture();
  const outDir = join(fixture.rootDir, "publish-bad-receipt");

  // Mismatch omaki receipt SHA
  const linuxReceiptPath = join(fixture.receiptsDir, "build-receipt-omaki.json");
  const linuxReceipt = JSON.parse(readFileSync(linuxReceiptPath, "utf8"));
  linuxReceipt.commitSha = "0000000000000000000000000000000000000000";
  writeFileSync(linuxReceiptPath, JSON.stringify(linuxReceipt, null, 2) + "\n");

  try {
    assert.throws(
      () =>
        assembleRelease({
          planPath: fixture.planPath,
          receiptsDir: fixture.receiptsDir,
          artifactsDir: fixture.artifactsDir,
          outDir,
          publicKey: fixture.keys.publicKeyWrapped,
        }),
      /commitSha mismatch/i,
    );
    assert.equal(existsSync(outDir), false);
  } finally {
    fixture.cleanup();
  }
});

test("assembleRelease fails closed if receipts directory contains unexpected files", () => {
  const fixture = createSyntheticReleaseFixture();
  const outDir = join(fixture.rootDir, "publish-extra-file");

  // Add rogue file to receipts
  writeFileSync(join(fixture.receiptsDir, "rogue-receipt.json"), "{}");

  try {
    assert.throws(
      () =>
        assembleRelease({
          planPath: fixture.planPath,
          receiptsDir: fixture.receiptsDir,
          artifactsDir: fixture.artifactsDir,
          outDir,
          publicKey: fixture.keys.publicKeyWrapped,
        }),
      /unexpected file in receipts directory/i,
    );
    assert.equal(existsSync(outDir), false);
  } finally {
    fixture.cleanup();
  }
});

test("assembleRelease rejects path traversal and symlink escape outside artifacts root", () => {
  const fixture = createSyntheticReleaseFixture();
  const outDir = join(fixture.rootDir, "publish-escape");

  // Create an outside secret file
  const secretFile = join(fixture.rootDir, "secret.txt");
  writeFileSync(secretFile, "sensitive secret data");

  // Attempt symlink escape inside darwin
  const escapedLink = join(fixture.artifactsDir, "darwin", "escaped.dmg");
  try {
    symlinkSync(secretFile, escapedLink);
  } catch (err) {
    // skip if symlink creation unsupported
  }

  if (existsSync(escapedLink)) {
    const macReceiptPath = join(fixture.receiptsDir, "build-receipt-macbook.json");
    const macReceipt = JSON.parse(readFileSync(macReceiptPath, "utf8"));
    macReceipt.artifacts[1].relPath = "darwin/escaped.dmg";
    macReceipt.artifacts[1].bytes = readFileSync(secretFile).length;
    macReceipt.artifacts[1].sha256 = sha256(readFileSync(secretFile));
    writeFileSync(macReceiptPath, JSON.stringify(macReceipt, null, 2) + "\n");

    try {
      assert.throws(
        () =>
          assembleRelease({
            planPath: fixture.planPath,
            receiptsDir: fixture.receiptsDir,
            artifactsDir: fixture.artifactsDir,
            outDir,
            publicKey: fixture.keys.publicKeyWrapped,
          }),
        /escapes artifacts directory/i,
      );
      assert.equal(existsSync(outDir), false);
    } finally {
      fixture.cleanup();
    }
  } else {
    fixture.cleanup();
  }
});

test("assembleRelease fails closed if macos-updater archive layout is invalid", () => {
  const fixture = createSyntheticReleaseFixture();
  const outDir = join(fixture.rootDir, "publish-bad-archive");
  try {
    // Overwrite Ferryx.app.tar.gz with invalid non-tar content and valid signature for that content
    const badBuf = Buffer.from("invalid-tar-content-not-rooted");
    const badSig = fixture.keys.signPayload(badBuf, "bad archive sig");
    writeFileSync(join(fixture.artifactsDir, "darwin", "Ferryx.app.tar.gz"), badBuf);
    writeFileSync(join(fixture.artifactsDir, "darwin", "Ferryx.app.tar.gz.sig"), badSig);

    // Update mac receipt with new hash/bytes so it passes receipt parsing
    const macReceiptPath = join(fixture.receiptsDir, "build-receipt-macbook.json");
    const macReceipt = JSON.parse(readFileSync(macReceiptPath, "utf8"));
    macReceipt.artifacts[0].bytes = badBuf.length;
    macReceipt.artifacts[0].sha256 = sha256(badBuf);
    writeFileSync(macReceiptPath, JSON.stringify(macReceipt, null, 2) + "\n");

    assert.throws(
      () =>
        assembleRelease({
          planPath: fixture.planPath,
          receiptsDir: fixture.receiptsDir,
          artifactsDir: fixture.artifactsDir,
          outDir,
          publicKey: fixture.keys.publicKeyWrapped,
        }),
      /macOS updater archive layout validation failed/i,
    );
    assert.equal(existsSync(outDir), false, "outDir must not exist on failure");
  } finally {
    fixture.cleanup();
  }
});

test("assembleRelease fails closed and leaves no publishable output if data changes at copy boundary", () => {
  const fixture = createSyntheticReleaseFixture();
  const outDir = join(fixture.rootDir, "publish-boundary-tamper");
  const originalCopyFileSync = fs.copyFileSync;
  try {
    // Inject mutation at copy boundary: append bytes to source right before filesystem copy
    fs.copyFileSync = (src, dest, flags) => {
      if (typeof src === "string" && src.endsWith(".dmg")) {
        writeFileSync(src, Buffer.concat([readFileSync(src), Buffer.from("-tampered-boundary-bytes")]));
      }
      originalCopyFileSync(src, dest, flags);
    };

    assert.throws(
      () =>
        assembleRelease({
          planPath: fixture.planPath,
          receiptsDir: fixture.receiptsDir,
          artifactsDir: fixture.artifactsDir,
          outDir,
          publicKey: fixture.keys.publicKeyWrapped,
        }),
      /Post-copy checksum verification failed/i,
    );
    assert.equal(existsSync(outDir), false, "outDir must not exist on failure");
  } finally {
    fs.copyFileSync = originalCopyFileSync;
    fixture.cleanup();
  }
});

test("assembleRelease CLI execution works end-to-end via subprocess", () => {
  const fixture = createSyntheticReleaseFixture();
  const outDir = join(fixture.rootDir, "cli-publish-out");
  try {
    const result = spawnSync(
      process.execPath,
      [
        SCRIPT,
        "--plan",
        fixture.planPath,
        "--receipts-dir",
        fixture.receiptsDir,
        "--artifacts-dir",
        fixture.artifactsDir,
        "--out-dir",
        outDir,
        "--pubkey",
        fixture.keys.publicKeyWrapped,
      ],
      { encoding: "utf8" },
    );

    assert.equal(result.status, 0, `stderr: ${result.stderr}`);
    assert.equal(existsSync(outDir), true);
    assert.equal(existsSync(join(outDir, "latest.json")), true);
    assert.equal(existsSync(join(outDir, "SHA256SUMS.txt")), true);
  } finally {
    fixture.cleanup();
  }
});

test("real repo signed archive verifies against checked-in Tauri public key", () => {
  assert.equal(existsSync(FIXTURE_ARCHIVE_PATH), true);
  assert.equal(existsSync(FIXTURE_SIG_PATH), true);
  assert.equal(existsSync(TAURI_CONF_PATH), true);

  const tauriConf = JSON.parse(readFileSync(TAURI_CONF_PATH, "utf8"));
  const pubkey = tauriConf.plugins?.updater?.pubkey;
  assert.ok(pubkey);

  const archiveBuf = readFileSync(FIXTURE_ARCHIVE_PATH);
  const sigText = readFileSync(FIXTURE_SIG_PATH, "utf8");

  // Verify import of minisign-verify directly
  const verified = verifyMinisign({
    data: archiveBuf,
    signature: sigText,
    publicKey: pubkey,
  });
  assert.equal(verified, true);
});

test("signature mutation during staging cannot publish a mismatched signature file", () => {
  const fixture = createSyntheticReleaseFixture();
  const realCopy = fs.copyFileSync;
  const outDir = join(fixture.rootDir, "out");
  let signatureCopySeen = false;
  try {
    fs.copyFileSync = (source, destination, ...args) => {
      if (source.endsWith(".sig") && !signatureCopySeen) {
        signatureCopySeen = true;
        writeFileSync(source, "changed after signature verification\n");
      }
      return realCopy(source, destination, ...args);
    };
    assert.throws(
      () => assembleRelease({
        planPath: fixture.planPath,
        receiptsDir: fixture.receiptsDir,
        artifactsDir: fixture.artifactsDir,
        outDir,
        publicKey: fixture.keys.publicKeyWrapped,
      }),
      /Post-copy checksum verification failed for staged file 'Ferryx\.app\.tar\.gz\.sig'/,
    );
    assert.equal(signatureCopySeen, true);
    assert.equal(existsSync(outDir), false);
  } finally {
    fs.copyFileSync = realCopy;
    fixture.cleanup();
  }
});

test("archive layout is checked against staged bytes rather than a mutable source", () => {
  const fixture = createSyntheticReleaseFixture();
  const source = fs.realpathSync(join(fixture.artifactsDir, "darwin", "Ferryx.app.tar.gz"));
  const invalidArchive = Buffer.from("authenticated but invalid archive");
  const validArchive = readFileSync(source);
  const realSpawn = childProcess.spawnSync;
  const outDir = join(fixture.rootDir, "out");
  let layoutChecked = false;
  try {
    writeFileSync(source, invalidArchive);
    writeFileSync(`${source}.sig`, fixture.keys.signPayload(invalidArchive));
    const receiptPath = join(fixture.receiptsDir, "build-receipt-macbook.json");
    const receipt = JSON.parse(readFileSync(receiptPath, "utf8"));
    receipt.artifacts[0].bytes = invalidArchive.length;
    receipt.artifacts[0].sha256 = sha256(invalidArchive);
    writeFileSync(receiptPath, JSON.stringify(receipt));
    childProcess.spawnSync = (command, args, options) => {
      if (args?.[0]?.endsWith("assert-updater-archive-layout.mjs")) {
        layoutChecked = true;
        // A concurrent source replacement can fool a source-path-only check.
        if (fs.realpathSync(args[1]) === source) {
          writeFileSync(source, validArchive);
          try {
            return realSpawn(command, args, options);
          } finally {
            writeFileSync(source, invalidArchive);
          }
        }
      }
      return realSpawn(command, args, options);
    };
    syncBuiltinESMExports();
    assert.throws(
      () => assembleRelease({
        planPath: fixture.planPath,
        receiptsDir: fixture.receiptsDir,
        artifactsDir: fixture.artifactsDir,
        outDir,
        publicKey: fixture.keys.publicKeyWrapped,
      }),
      /archive layout/i,
    );
    assert.equal(layoutChecked, true);
    assert.equal(existsSync(outDir), false);
  } finally {
    childProcess.spawnSync = realSpawn;
    syncBuiltinESMExports();
    fixture.cleanup();
  }
});
