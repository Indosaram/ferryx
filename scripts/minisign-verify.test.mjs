import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash, generateKeyPairSync, randomBytes, sign } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { verifyMinisign } from "./lib/minisign-verify.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURE_ARCHIVE_PATH = join(REPO_ROOT, "scripts", "fixtures", "updater", "Ferryx.app.tar.gz");
const FIXTURE_SIG_PATH = join(REPO_ROOT, "scripts", "fixtures", "updater", "Ferryx.app.tar.gz.sig");
const TAURI_CONF_PATH = join(REPO_ROOT, "src-tauri", "tauri.conf.json");

/**
 * Invokes the minisign CLI oracle directly.
 * Fails with a clear prerequisite error if minisign is not installed on the coordinator host.
 */
function invokeMinisign(args, options = {}) {
  try {
    return execFileSync("minisign", args, options);
  } catch (error) {
    if (error.code === "ENOENT") {
      throw new Error(
        "minisign CLI oracle is required on coordinator workstation but minisign executable was not found in PATH",
      );
    }
    throw error;
  }
}

/**
 * Generates an independent Minisign keypair and signature using pure Node crypto.
 * Used to test verifyMinisign without relying on verifier internals.
 */
function generateIndependentMinisignFixture({
  algorithm = "ED", // "ED" (blake2b512) or "Ed" (raw)
  comment = "trusted test comment 12345",
  data = Buffer.from("independent test payload for minisign verification"),
  keyId = null,
} = {}) {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const rawPub = publicKey.export({ format: "der", type: "spki" }).subarray(12);

  const actualKeyId = keyId ? Buffer.from(keyId) : randomBytes(8);
  const keyIdHex = Buffer.from(actualKeyId).reverse().toString("hex").toUpperCase();

  // Minisign public key format: 2 bytes "Ed" + 8 bytes key ID + 32 bytes pubkey = 42 bytes
  const pubBin = Buffer.concat([Buffer.from("Ed", "utf8"), actualKeyId, rawPub]);
  const pubB64 = pubBin.toString("base64");
  const pubText = `untrusted comment: minisign public key ${keyIdHex}\n${pubB64}\n`;
  const pubWrapped = Buffer.from(pubText, "utf8").toString("base64");

  // Sign data
  let verifyMessage;
  if (algorithm === "ED") {
    verifyMessage = createHash("blake2b512").update(data).digest();
  } else if (algorithm === "Ed") {
    verifyMessage = data;
  } else {
    throw new Error(`Unsupported test algorithm: ${algorithm}`);
  }

  const payloadSig = sign(null, verifyMessage, privateKey);
  // Signature line: 2 bytes algorithm + 8 bytes key ID + 64 bytes sig = 74 bytes
  const sigBin = Buffer.concat([Buffer.from(algorithm, "utf8"), actualKeyId, payloadSig]);
  const sigB64 = sigBin.toString("base64");

  // Trusted comment
  const trustedCommentLine = `trusted comment: ${comment}`;
  const globalMsg = Buffer.concat([payloadSig, Buffer.from(comment, "utf8")]);
  const globalSig = sign(null, globalMsg, privateKey);
  const globalSigB64 = globalSig.toString("base64");

  const sigText = [
    "untrusted comment: signature from independent test key",
    sigB64,
    trustedCommentLine,
    globalSigB64,
  ].join("\n") + "\n";
  const sigWrapped = Buffer.from(sigText, "utf8").toString("base64");

  return {
    algorithm,
    comment,
    data,
    keyId: actualKeyId,
    privateKey,
    pubB64,
    pubBin,
    pubText,
    pubWrapped,
    rawPub,
    sigB64,
    sigBin,
    sigText,
    sigWrapped,
    payloadSig,
    globalSig,
  };
}

// ---------------------------------------------------------------------------
// 1. Authentic Repository Fixtures & Configured Updater Public Key
// ---------------------------------------------------------------------------

test("verifies authentic repo archive fixture with configured updater public key (Tauri base64 wrapped)", () => {
  const tauriConf = JSON.parse(readFileSync(TAURI_CONF_PATH, "utf8"));
  const configuredPubKey = tauriConf.plugins.updater.pubkey;
  const signatureText = readFileSync(FIXTURE_SIG_PATH, "utf8").trim();
  const archiveData = readFileSync(FIXTURE_ARCHIVE_PATH);

  const result = verifyMinisign({
    data: archiveData,
    signature: signatureText,
    publicKey: configuredPubKey,
  });

  assert.equal(result, true);
});

test("verifies authentic repo archive fixture with raw un-wrapped Minisign text format", () => {
  const tauriConf = JSON.parse(readFileSync(TAURI_CONF_PATH, "utf8"));
  const configuredPubKey = tauriConf.plugins.updater.pubkey;
  const signatureWrapped = readFileSync(FIXTURE_SIG_PATH, "utf8").trim();
  const archiveData = readFileSync(FIXTURE_ARCHIVE_PATH);

  const rawPubKeyText = Buffer.from(configuredPubKey, "base64").toString("utf8");
  const rawSigText = Buffer.from(signatureWrapped, "base64").toString("utf8");

  const result = verifyMinisign({
    data: archiveData,
    signature: rawSigText,
    publicKey: rawPubKeyText,
  });

  assert.equal(result, true);
});

test("verifies authentic repo archive fixture with single-line base64 public key (56-char minisign -P format)", () => {
  const tauriConf = JSON.parse(readFileSync(TAURI_CONF_PATH, "utf8"));
  const configuredPubKey = tauriConf.plugins.updater.pubkey;
  const signatureWrapped = readFileSync(FIXTURE_SIG_PATH, "utf8").trim();
  const archiveData = readFileSync(FIXTURE_ARCHIVE_PATH);

  const rawPubKeyText = Buffer.from(configuredPubKey, "base64").toString("utf8");
  const singleLinePubKey = rawPubKeyText.split(/\r?\n/)[1].trim();

  const result = verifyMinisign({
    data: archiveData,
    signature: signatureWrapped,
    publicKey: singleLinePubKey,
  });

  assert.equal(result, true);
});

test("supports data passed as Uint8Array or utf8 string", () => {
  const fixture = generateIndependentMinisignFixture({
    data: Buffer.from("Hello string and uint8array verification"),
  });

  // Test as Uint8Array
  const u8Array = new Uint8Array(fixture.data);
  assert.equal(
    verifyMinisign({
      data: u8Array,
      signature: fixture.sigWrapped,
      publicKey: fixture.pubWrapped,
    }),
    true,
  );

  // Test as string
  assert.equal(
    verifyMinisign({
      data: "Hello string and uint8array verification",
      signature: fixture.sigWrapped,
      publicKey: fixture.pubWrapped,
    }),
    true,
  );
});

// ---------------------------------------------------------------------------
// 2. Independent Signing Logic & Algorithm Matrix (ED / Blake2b-512 vs Ed / raw)
// ---------------------------------------------------------------------------

test("verifies independent ED (Blake2b-512 prehash) signatures across wrapped, text, and single-line formats", () => {
  const fixture = generateIndependentMinisignFixture({
    algorithm: "ED",
    comment: "release notes v2026.09.08.1",
    data: Buffer.from("prehashed blake2b content payload"),
  });

  // Wrapped format
  assert.equal(
    verifyMinisign({
      data: fixture.data,
      signature: fixture.sigWrapped,
      publicKey: fixture.pubWrapped,
    }),
    true,
  );

  // Text format
  assert.equal(
    verifyMinisign({
      data: fixture.data,
      signature: fixture.sigText,
      publicKey: fixture.pubText,
    }),
    true,
  );

  // Single-line pubkey format
  assert.equal(
    verifyMinisign({
      data: fixture.data,
      signature: fixture.sigText,
      publicKey: fixture.pubB64,
    }),
    true,
  );
});

test("verifies independent Ed (raw legacy Ed25519) signatures across wrapped, text, and single-line formats", () => {
  const fixture = generateIndependentMinisignFixture({
    algorithm: "Ed",
    comment: "legacy raw ed25519 signature format",
    data: Buffer.from("raw ed25519 message payload"),
  });

  // Wrapped format
  assert.equal(
    verifyMinisign({
      data: fixture.data,
      signature: fixture.sigWrapped,
      publicKey: fixture.pubWrapped,
    }),
    true,
  );

  // Text format
  assert.equal(
    verifyMinisign({
      data: fixture.data,
      signature: fixture.sigText,
      publicKey: fixture.pubText,
    }),
    true,
  );

  // Single-line pubkey format
  assert.equal(
    verifyMinisign({
      data: fixture.data,
      signature: fixture.sigText,
      publicKey: fixture.pubB64,
    }),
    true,
  );
});

// ---------------------------------------------------------------------------
// 3. Minisign CLI Oracle Two-Way Cross Validation (Direct Required Oracle)
// ---------------------------------------------------------------------------

test("minisign CLI oracle validates independent Node-generated fixtures, and verifyMinisign validates CLI fixtures", () => {
  const tmpDir = mkdtempSync(join(tmpdir(), "minisign-oracle-"));
  try {
    // A. Verify Node-generated fixture with minisign CLI oracle
    const nodeFixture = generateIndependentMinisignFixture({
      algorithm: "ED",
      comment: "oracle check comment",
      data: Buffer.from("oracle check data payload"),
    });

    const nodePubPath = join(tmpDir, "node.pub");
    const nodeDataPath = join(tmpDir, "node.data");
    const nodeSigPath = join(tmpDir, "node.data.minisig");

    writeFileSync(nodePubPath, nodeFixture.pubText);
    writeFileSync(nodeDataPath, nodeFixture.data);
    writeFileSync(nodeSigPath, nodeFixture.sigText);

    const cliOut = invokeMinisign([
      "-V",
      "-p",
      nodePubPath,
      "-m",
      nodeDataPath,
      "-x",
      nodeSigPath,
    ], { encoding: "utf8" });

    assert.match(cliOut, /Signature and comment signature verified/);

    // B. Generate fixture with minisign CLI and verify with verifyMinisign
    const cliPubPath = join(tmpDir, "cli.pub");
    const cliKeyPath = join(tmpDir, "cli.key");
    const cliDataPath = join(tmpDir, "cli.data");
    const cliSigPath = join(tmpDir, "cli.data.minisig");

    invokeMinisign(["-f", "-G", "-W", "-p", cliPubPath, "-s", cliKeyPath]);
    writeFileSync(cliDataPath, "CLI generated payload content");
    invokeMinisign([
      "-S",
      "-s",
      cliKeyPath,
      "-m",
      cliDataPath,
      "-x",
      cliSigPath,
      "-t",
      "cli trusted comment",
    ]);

    const cliPubKey = readFileSync(cliPubPath, "utf8");
    const cliSig = readFileSync(cliSigPath, "utf8");
    const cliData = readFileSync(cliDataPath);

    assert.equal(
      verifyMinisign({
        data: cliData,
        signature: cliSig,
        publicKey: cliPubKey,
      }),
      true,
    );

    // C. Minisign CLI legacy (-l, Ed raw) format verified with verifyMinisign
    const cliLegacySigPath = join(tmpDir, "cli.legacy.minisig");
    invokeMinisign([
      "-S",
      "-l",
      "-s",
      cliKeyPath,
      "-m",
      cliDataPath,
      "-x",
      cliLegacySigPath,
      "-t",
      "cli legacy trusted comment",
    ]);

    const cliLegacySig = readFileSync(cliLegacySigPath, "utf8");
    assert.equal(
      verifyMinisign({
        data: cliData,
        signature: cliLegacySig,
        publicKey: cliPubKey,
      }),
      true,
    );
  } finally {
    rmSync(tmpDir, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// 4. Adversarial Cases: Tampered Data, Signature, Key-ID, and Comment
// ---------------------------------------------------------------------------

test("rejects tampered data (single bit flip, truncation, appending)", () => {
  const fixture = generateIndependentMinisignFixture();

  // Single bit flip
  const tamperedBit = Buffer.from(fixture.data);
  tamperedBit[0] ^= 0x01;
  assert.throws(
    () =>
      verifyMinisign({
        data: tamperedBit,
        signature: fixture.sigWrapped,
        publicKey: fixture.pubWrapped,
      }),
    /payload signature verification failed/i,
  );

  // Truncated data
  const tamperedTruncated = fixture.data.subarray(0, fixture.data.length - 1);
  assert.throws(
    () =>
      verifyMinisign({
        data: tamperedTruncated,
        signature: fixture.sigWrapped,
        publicKey: fixture.pubWrapped,
      }),
    /payload signature verification failed/i,
  );

  // Appended data
  const tamperedAppended = Buffer.concat([fixture.data, Buffer.from("!")]);
  assert.throws(
    () =>
      verifyMinisign({
        data: tamperedAppended,
        signature: fixture.sigWrapped,
        publicKey: fixture.pubWrapped,
      }),
    /payload signature verification failed/i,
  );
});

test("rejects tampered authentic repo archive fixture data", () => {
  const tauriConf = JSON.parse(readFileSync(TAURI_CONF_PATH, "utf8"));
  const configuredPubKey = tauriConf.plugins.updater.pubkey;
  const signatureText = readFileSync(FIXTURE_SIG_PATH, "utf8").trim();
  const archiveData = Buffer.from(readFileSync(FIXTURE_ARCHIVE_PATH));

  // Flip one byte in archive
  archiveData[4] ^= 0xaa;

  assert.throws(
    () =>
      verifyMinisign({
        data: archiveData,
        signature: signatureText,
        publicKey: configuredPubKey,
      }),
    /payload signature verification failed/i,
  );
});

test("rejects tampered payload signature bytes in line 2", () => {
  const fixture = generateIndependentMinisignFixture();

  // Tamper payload signature in line 2
  const lines = fixture.sigText.trim().split("\n");
  const sigBin = Buffer.from(lines[1], "base64");
  sigBin[25] ^= 0x88; // within the 64-byte payload signature region
  lines[1] = sigBin.toString("base64");
  const tamperedSigText = lines.join("\n") + "\n";

  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: tamperedSigText,
        publicKey: fixture.pubText,
      }),
    /payload signature verification failed/i,
  );
});

test("rejects tampered global/trusted comment signature bytes in line 4", () => {
  const fixture = generateIndependentMinisignFixture();

  const lines = fixture.sigText.trim().split("\n");
  const globalBin = Buffer.from(lines[3], "base64");
  globalBin[15] ^= 0x55;
  lines[3] = globalBin.toString("base64");
  const tamperedSigText = lines.join("\n") + "\n";

  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: tamperedSigText,
        publicKey: fixture.pubText,
      }),
    /trusted comment signature verification failed/i,
  );
});

test("rejects tampered key ID in signature (mismatched key ID)", () => {
  const fixture = generateIndependentMinisignFixture();

  const lines = fixture.sigText.trim().split("\n");
  const sigBin = Buffer.from(lines[1], "base64");
  sigBin[5] ^= 0xff; // within the 8-byte key ID region (bytes 2..10)
  lines[1] = sigBin.toString("base64");
  const tamperedSigText = lines.join("\n") + "\n";

  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: tamperedSigText,
        publicKey: fixture.pubText,
      }),
    /does not match public key ID/i,
  );
});

test("rejects tampered key ID in public key (mismatched key ID)", () => {
  const fixture = generateIndependentMinisignFixture();

  const lines = fixture.pubText.trim().split("\n");
  const pubBin = Buffer.from(lines[1], "base64");
  pubBin[4] ^= 0xee; // within 8-byte key ID region
  lines[1] = pubBin.toString("base64");
  const tamperedPubText = lines.join("\n") + "\n";

  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: fixture.sigText,
        publicKey: tamperedPubText,
      }),
    /does not match public key ID/i,
  );
});

test("rejects tampered trusted comment text in signature", () => {
  const fixture = generateIndependentMinisignFixture({
    comment: "legitimate trusted comment",
  });

  const lines = fixture.sigText.trim().split("\n");
  lines[2] = "trusted comment: forged attacker comment";
  const tamperedSigText = lines.join("\n") + "\n";

  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: tamperedSigText,
        publicKey: fixture.pubText,
      }),
    /trusted comment signature verification failed/i,
  );
});

// ---------------------------------------------------------------------------
// 5. Adversarial Cases: Malformed Data, Strict Lengths, Algorithm, & Base64
// ---------------------------------------------------------------------------

test("strictly rejects arbitrary base64 strings (no arbitrary base64 acceptance)", () => {
  const fixture = generateIndependentMinisignFixture();

  // Arbitrary text encoded as base64
  const arbitraryB64_1 = Buffer.from("this is definitely not a minisign key or signature").toString("base64");
  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: fixture.sigWrapped,
        publicKey: arbitraryB64_1,
      }),
    /invalid publickey/i,
  );

  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: arbitraryB64_1,
        publicKey: fixture.pubWrapped,
      }),
    /invalid signature/i,
  );

  // Arbitrary random bytes of 74 bytes base64 (matching signature length but not minisign wrapper format)
  const arbitraryB64_74 = randomBytes(74).toString("base64");
  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: arbitraryB64_74,
        publicKey: fixture.pubWrapped,
      }),
    /invalid signature/i,
  );

  // Arbitrary random bytes of 42 bytes base64 (matching public key length but not valid algorithm or key)
  const arbitraryB64_42 = randomBytes(42).toString("base64");
  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: fixture.sigWrapped,
        publicKey: arbitraryB64_42,
      }),
    /invalid publickey/i,
  );
});

test("strictly rejects invalid or unsupported algorithms in public key and signature", () => {
  const fixture = generateIndependentMinisignFixture();

  // Public key algorithm changed from "Ed" to "ED"
  const pubLines = fixture.pubText.trim().split("\n");
  const pubBin = Buffer.from(pubLines[1], "base64");
  pubBin[0] = 0x45; // 'E'
  pubBin[1] = 0x44; // 'D' (invalid for pubkey)
  pubLines[1] = pubBin.toString("base64");
  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: fixture.sigText,
        publicKey: pubLines.join("\n"),
      }),
    /invalid publickey algorithm/i,
  );

  // Signature algorithm changed from "ED" to "XX"
  const sigLines = fixture.sigText.trim().split("\n");
  const sigBin = Buffer.from(sigLines[1], "base64");
  sigBin[0] = 0x58; // 'X'
  sigBin[1] = 0x58; // 'X'
  sigLines[1] = sigBin.toString("base64");
  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: sigLines.join("\n"),
        publicKey: fixture.pubText,
      }),
    /invalid signature algorithm/i,
  );
});

test("strictly rejects invalid byte lengths for public key, payload signature, and global signature", () => {
  const fixture = generateIndependentMinisignFixture();

  // Public key line with 41 bytes instead of 42
  const pubLines = fixture.pubText.trim().split("\n");
  pubLines[1] = Buffer.from(pubLines[1], "base64").subarray(0, 41).toString("base64");
  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: fixture.sigText,
        publicKey: pubLines.join("\n"),
      }),
    /expected 42 bytes/i,
  );

  // Signature line 2 with 73 bytes instead of 74
  const sigLines1 = fixture.sigText.trim().split("\n");
  sigLines1[1] = Buffer.from(sigLines1[1], "base64").subarray(0, 73).toString("base64");
  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: sigLines1.join("\n"),
        publicKey: fixture.pubText,
      }),
    /expected 74 bytes/i,
  );

  // Global signature line 4 with 63 bytes instead of 64
  const sigLines2 = fixture.sigText.trim().split("\n");
  sigLines2[3] = Buffer.from(sigLines2[3], "base64").subarray(0, 63).toString("base64");
  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: sigLines2.join("\n"),
        publicKey: fixture.pubText,
      }),
    /expected 64 bytes/i,
  );
});

test("rejects malformed base64 encoding (invalid characters, invalid padding)", () => {
  const fixture = generateIndependentMinisignFixture();

  // Signature with invalid base64 characters
  const sigLines = fixture.sigText.trim().split("\n");
  sigLines[1] = sigLines[1].slice(0, -4) + "@@==";
  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: sigLines.join("\n"),
        publicKey: fixture.pubText,
      }),
    /base64/i,
  );

  // Public key with non-canonical padding / invalid length
  const pubLines = fixture.pubText.trim().split("\n");
  pubLines[1] = pubLines[1] + "===";
  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: fixture.sigText,
        publicKey: pubLines.join("\n"),
      }),
    /base64/i,
  );
});

test("rejects missing headers, truncated lines, or malformed input types", () => {
  const fixture = generateIndependentMinisignFixture();

  // Missing untrusted comment header in signature
  const noHeaderSig = fixture.sigText.replace("untrusted comment: ", "corrupted comment: ");
  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: noHeaderSig,
        publicKey: fixture.pubText,
      }),
    /untrusted comment/i,
  );

  // Missing trusted comment header in signature
  const noTrustedHeaderSig = fixture.sigText.replace("trusted comment: ", "invalid comment: ");
  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: noTrustedHeaderSig,
        publicKey: fixture.pubText,
      }),
    /trusted comment/i,
  );

  // Truncated signature lines (only 2 lines instead of 4)
  const truncatedSig = fixture.sigText.split("\n").slice(0, 2).join("\n");
  assert.throws(
    () =>
      verifyMinisign({
        data: fixture.data,
        signature: truncatedSig,
        publicKey: fixture.pubText,
      }),
    /expected 4 lines/i,
  );

  // Missing arguments / null / undefined / empty
  assert.throws(() => verifyMinisign(), /options/i);
  assert.throws(() => verifyMinisign({}), /invalid data/i);
  assert.throws(
    () => verifyMinisign({ data: fixture.data, signature: null, publicKey: fixture.pubText }),
    /invalid signature/i,
  );
  assert.throws(
    () => verifyMinisign({ data: fixture.data, signature: fixture.sigText, publicKey: null }),
    /invalid publickey/i,
  );
});
