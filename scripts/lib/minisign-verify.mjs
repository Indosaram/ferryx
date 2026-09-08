import { createHash, createPublicKey, verify } from "node:crypto";

/**
 * ASN.1 DER SubjectPublicKeyInfo prefix for Ed25519 (OID 1.3.101.112 id-Ed25519).
 * 30 2a 30 05 06 03 2b 65 70 03 21 00 (12 bytes)
 */
const ED25519_SPKI_PREFIX = Buffer.from("302a300506032b6570032100", "hex");

const BASE64_REGEX = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;

/**
 * Decodes and strictly validates a base64 string.
 * Rejects non-canonical encodings, invalid padding, and invalid characters.
 *
 * @param {string} str
 * @param {number | null} expectedLen
 * @param {string} description
 * @returns {Buffer}
 */
function decodeStrictBase64(str, expectedLen = null, description = "base64 data") {
  if (typeof str !== "string") {
    throw new Error(`Invalid ${description}: expected string`);
  }
  const clean = str.trim();
  if (clean.length === 0) {
    throw new Error(`Invalid ${description}: empty string`);
  }
  if (clean.length % 4 !== 0 || !BASE64_REGEX.test(clean)) {
    throw new Error(`Invalid ${description}: malformed base64 encoding`);
  }
  const buf = Buffer.from(clean, "base64");
  if (buf.toString("base64") !== clean) {
    throw new Error(`Invalid ${description}: non-canonical base64 encoding`);
  }
  if (expectedLen !== null && buf.length !== expectedLen) {
    throw new Error(`Invalid ${description}: expected ${expectedLen} bytes, got ${buf.length}`);
  }
  return buf;
}

/**
 * Parses a Minisign public key from text or Tauri base64-wrapped format.
 *
 * @param {string | Buffer | Uint8Array} publicKey
 * @returns {{ keyId: Buffer, rawPublicKey: Buffer, keyObject: import("node:crypto").KeyObject }}
 */
export function parsePublicKey(publicKey) {
  if (publicKey === null || publicKey === undefined) {
    throw new Error("Invalid publicKey: missing or empty");
  }
  let text = typeof publicKey === "string"
    ? publicKey.trim()
    : Buffer.isBuffer(publicKey) || publicKey instanceof Uint8Array
    ? Buffer.from(publicKey).toString("utf8").trim()
    : null;
  if (!text) {
    throw new Error("Invalid publicKey: expected non-empty string or Buffer");
  }

  // If wrapped in Tauri base64 (which decodes to text starting with "untrusted comment:"), unwrap it
  if (!text.startsWith("untrusted comment:")) {
    const clean = text.replace(/\s+/g, "");
    if (clean.length % 4 === 0 && BASE64_REGEX.test(clean)) {
      const decodedBuf = Buffer.from(clean, "base64");
      if (decodedBuf.toString("base64") === clean) {
        const decodedText = decodedBuf.toString("utf8").trim();
        if (decodedText.startsWith("untrusted comment:")) {
          text = decodedText;
        }
      }
    }
  }

  let keyBase64;
  if (text.startsWith("untrusted comment:")) {
    const rawLines = text.split(/\r?\n/);
    while (rawLines.length > 0 && rawLines[rawLines.length - 1].trim() === "") {
      rawLines.pop();
    }
    if (rawLines.length < 2) {
      throw new Error("Invalid publicKey: expected at least 2 lines");
    }
    if (rawLines.length > 2) {
      throw new Error("Invalid publicKey: unexpected extra lines in public key");
    }
    if (!rawLines[0].startsWith("untrusted comment:")) {
      throw new Error("Invalid publicKey: line 1 must start with 'untrusted comment:'");
    }
    keyBase64 = rawLines[1].trim();
  } else {
    // Check for raw 56-character base64 public key (minisign -P format)
    const clean = text.replace(/\s+/g, "");
    if (clean.length === 56 && clean.length % 4 === 0 && BASE64_REGEX.test(clean)) {
      keyBase64 = clean;
    } else {
      throw new Error("Invalid publicKey: not a valid Minisign public key format");
    }
  }

  // Must decode to 42 bytes: 2 bytes alg ("Ed") + 8 bytes key ID + 32 bytes Ed25519 pubkey
  const keyBytes = decodeStrictBase64(keyBase64, 42, "publicKey");
  const algorithm = keyBytes.subarray(0, 2).toString("utf8");
  if (algorithm !== "Ed") {
    throw new Error(`Invalid publicKey algorithm: expected 'Ed', got '${algorithm}'`);
  }
  const keyId = keyBytes.subarray(2, 10);
  const rawPublicKey = keyBytes.subarray(10, 42);

  const spkiDer = Buffer.concat([ED25519_SPKI_PREFIX, rawPublicKey]);
  const keyObject = createPublicKey({ key: spkiDer, format: "der", type: "spki" });

  return {
    keyId,
    rawPublicKey,
    keyObject,
  };
}

/**
 * Parses a Minisign signature from 4-line text or Tauri base64-wrapped format.
 *
 * @param {string | Buffer | Uint8Array} signature
 * @returns {{ algorithm: string, keyId: Buffer, payloadSig: Buffer, trustedComment: string, trustedCommentLine: string, globalSig: Buffer }}
 */
export function parseSignature(signature) {
  if (signature === null || signature === undefined) {
    throw new Error("Invalid signature: missing or empty");
  }
  let text = typeof signature === "string"
    ? signature.trim()
    : Buffer.isBuffer(signature) || signature instanceof Uint8Array
    ? Buffer.from(signature).toString("utf8").trim()
    : null;
  if (!text) {
    throw new Error("Invalid signature: expected non-empty string or Buffer");
  }

  // If wrapped in Tauri base64 (which decodes to text starting with "untrusted comment:"), unwrap it
  if (!text.startsWith("untrusted comment:")) {
    const clean = text.replace(/\s+/g, "");
    if (clean.length % 4 === 0 && BASE64_REGEX.test(clean)) {
      const decodedBuf = Buffer.from(clean, "base64");
      if (decodedBuf.toString("base64") === clean) {
        const decodedText = decodedBuf.toString("utf8").trim();
        if (decodedText.startsWith("untrusted comment:")) {
          text = decodedText;
        }
      }
    }
  }

  if (!text.startsWith("untrusted comment:")) {
    throw new Error("Invalid signature: missing 'untrusted comment:' header");
  }

  const rawLines = text.split(/\r?\n/);
  while (rawLines.length > 0 && rawLines[rawLines.length - 1].trim() === "") {
    rawLines.pop();
  }
  if (rawLines.length !== 4) {
    throw new Error(`Invalid signature: expected 4 lines, got ${rawLines.length}`);
  }

  const [line1, line2, line3, line4] = rawLines;

  if (!line1.startsWith("untrusted comment:")) {
    throw new Error("Invalid signature: line 1 must start with 'untrusted comment:'");
  }

  // Line 2: 74 bytes base64 (2 bytes alg + 8 bytes key ID + 64 bytes sig)
  const sigBytes = decodeStrictBase64(line2, 74, "signature line 2");
  const algorithm = sigBytes.subarray(0, 2).toString("utf8");
  if (algorithm !== "Ed" && algorithm !== "ED") {
    throw new Error(`Invalid signature algorithm: expected 'Ed' or 'ED', got '${algorithm}'`);
  }
  const keyId = sigBytes.subarray(2, 10);
  const payloadSig = sigBytes.subarray(10, 74);

  // Line 3: trusted comment
  if (!line3.startsWith("trusted comment:")) {
    throw new Error("Invalid signature: line 3 must start with 'trusted comment:'");
  }
  let trustedComment = "";
  if (line3.startsWith("trusted comment: ")) {
    trustedComment = line3.slice("trusted comment: ".length);
  } else if (line3 === "trusted comment:") {
    trustedComment = "";
  } else {
    throw new Error("Invalid signature: line 3 must start with 'trusted comment: '");
  }

  // Line 4: 64 bytes base64 (global signature)
  const globalSig = decodeStrictBase64(line4, 64, "signature line 4");

  return {
    algorithm,
    keyId,
    payloadSig,
    trustedComment,
    trustedCommentLine: line3,
    globalSig,
  };
}

/**
 * Verifies a Minisign signature against data using pure Node crypto.
 *
 * @param {object} options
 * @param {string | Buffer | Uint8Array} options.data
 * @param {string | Buffer | Uint8Array} options.signature
 * @param {string | Buffer | Uint8Array} options.publicKey
 * @returns {boolean} returns true on success, throws Error on invalid
 */
export function verifyMinisign(options) {
  if (!options || typeof options !== "object") {
    throw new Error("verifyMinisign requires an options object { data, signature, publicKey }");
  }

  const { data, signature, publicKey } = options;

  if (data === null || data === undefined) {
    throw new Error("Invalid data: missing or undefined");
  }
  let dataBuffer;
  if (typeof data === "string") {
    dataBuffer = Buffer.from(data, "utf8");
  } else if (Buffer.isBuffer(data)) {
    dataBuffer = data;
  } else if (data instanceof Uint8Array) {
    dataBuffer = Buffer.from(data.buffer, data.byteOffset, data.byteLength);
  } else {
    throw new Error("Invalid data: expected string, Buffer, or Uint8Array");
  }

  const parsedPub = parsePublicKey(publicKey);
  const parsedSig = parseSignature(signature);

  // 1. Verify key ID match
  if (!parsedPub.keyId.equals(parsedSig.keyId)) {
    throw new Error(
      `Signature key ID (${parsedSig.keyId.toString("hex").toUpperCase()}) does not match public key ID (${parsedPub.keyId.toString("hex").toUpperCase()})`
    );
  }

  // 2. Verify payload signature
  let verifyMessage;
  if (parsedSig.algorithm === "ED") {
    verifyMessage = createHash("blake2b512").update(dataBuffer).digest();
  } else if (parsedSig.algorithm === "Ed") {
    verifyMessage = dataBuffer;
  } else {
    throw new Error(`Unsupported signature algorithm: ${parsedSig.algorithm}`);
  }

  const payloadOk = verify(null, verifyMessage, parsedPub.keyObject, parsedSig.payloadSig);
  if (!payloadOk) {
    throw new Error("Minisign payload signature verification failed");
  }

  // 3. Verify global / trusted comment signature
  // Signed message is: payloadSig (64 bytes) || trustedComment (utf8 bytes)
  const globalMessage = Buffer.concat([
    parsedSig.payloadSig,
    Buffer.from(parsedSig.trustedComment, "utf8"),
  ]);

  const globalOk = verify(null, globalMessage, parsedPub.keyObject, parsedSig.globalSig);
  if (!globalOk) {
    throw new Error("Minisign trusted comment signature verification failed");
  }

  return true;
}
