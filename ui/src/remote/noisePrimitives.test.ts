import { describe, it, expect } from "vitest";
import { createNoisePrimitives } from "./noisePrimitives";

function hexToBytes(hex: string): Uint8Array {
  const cleanHex = hex.replace(/[^0-9a-fA-F]/g, "");
  const bytes = new Uint8Array(cleanHex.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(cleanHex.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

function bytesToHex(bytes: Uint8Array): string {
  let hex = "";
  for (let i = 0; i < bytes.length; i++) {
    hex += bytes[i].toString(16).padStart(2, "0");
  }
  return hex;
}

describe("noisePrimitives", () => {
  it("computes RFC 7693 Appendix B BLAKE2s-256 for 'abc'", async () => {
    const primitives = createNoisePrimitives();
    const input = new TextEncoder().encode("abc");
    const digest = await primitives.hash(input);
    const expectedHex = "508c5e8c327c14e2e1a72ba34eeb452f37458b209ed63a294d999b4c86675982";
    expect(bytesToHex(digest)).toBe(expectedHex);
  });

  it("computes RFC 8439 Section 2.8.2 ChaCha20-Poly1305 known-answer", async () => {
    const primitives = createNoisePrimitives();

    const key = hexToBytes("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
    const nonce = hexToBytes("070000004041424344454647");
    const aad = hexToBytes("50515253c0c1c2c3c4c5c6c7");
    const plaintext = new TextEncoder().encode(
      "Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it."
    );

    // RFC 8439 Section 2.8.2, independently reproduced with the OpenSSL-backed Python cryptography library on 2026-09-25
    const expectedFullHex =
      "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d6" +
      "3dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b36" +
      "92ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc" +
      "3ff4def08e4b7a9de576d26586cec64b61161ae10b594f09e26a7e902ecbd0600691";

    const encrypted = await (primitives.aeadEncrypt as unknown as (
      k: Uint8Array,
      n: Uint8Array,
      ad: Uint8Array,
      pt: Uint8Array
    ) => Promise<Uint8Array>)(key, nonce, aad, plaintext);

    expect(bytesToHex(encrypted)).toBe(expectedFullHex);

    const decrypted = await (primitives.aeadDecrypt as unknown as (
      k: Uint8Array,
      n: Uint8Array,
      ad: Uint8Array,
      ct: Uint8Array
    ) => Promise<Uint8Array>)(key, nonce, aad, encrypted);

    expect(new TextDecoder().decode(decrypted)).toBe(
      "Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it."
    );
  });

  it("computes HKDF with HMAC-BLAKE2s matching independent RFC 2104 expansion", async () => {
    const primitives = createNoisePrimitives();

    const ck = new Uint8Array(32).fill(0x11);
    const ikm = new Uint8Array(32).fill(0x22);

    const [o1, o2] = await primitives.hkdf!(ck, ikm);
    const [triple1, triple2, triple3] = await primitives.hkdf3!(ck, ikm);

    expect(o1.length).toBe(32);
    expect(o2.length).toBe(32);
    expect(bytesToHex(o1)).toBe(bytesToHex(triple1));
    expect(bytesToHex(o2)).toBe(bytesToHex(triple2));
    expect(triple3.length).toBe(32);

    const blockLen = 64;
    const kPad = new Uint8Array(blockLen);
    kPad.set(ck, 0);

    const ipad = new Uint8Array(blockLen);
    const opad = new Uint8Array(blockLen);
    for (let i = 0; i < blockLen; i++) {
      ipad[i] = kPad[i] ^ 0x36;
      opad[i] = kPad[i] ^ 0x5c;
    }

    const innerMsg = new Uint8Array(blockLen + ikm.length);
    innerMsg.set(ipad, 0);
    innerMsg.set(ikm, blockLen);
    const innerHash = await primitives.hash(innerMsg);

    const outerMsg = new Uint8Array(blockLen + innerHash.length);
    outerMsg.set(opad, 0);
    outerMsg.set(innerHash, blockLen);
    const manualTempKey = await primitives.hash(outerMsg);

    const kPad2 = new Uint8Array(blockLen);
    kPad2.set(manualTempKey, 0);
    const ipad2 = new Uint8Array(blockLen);
    const opad2 = new Uint8Array(blockLen);
    for (let i = 0; i < blockLen; i++) {
      ipad2[i] = kPad2[i] ^ 0x36;
      opad2[i] = kPad2[i] ^ 0x5c;
    }

    const info1 = new Uint8Array([0x01]);
    const inner1 = new Uint8Array(blockLen + info1.length);
    inner1.set(ipad2, 0);
    inner1.set(info1, blockLen);
    const hashInner1 = await primitives.hash(inner1);

    const outer1 = new Uint8Array(blockLen + hashInner1.length);
    outer1.set(opad2, 0);
    outer1.set(hashInner1, blockLen);
    const manualO1 = await primitives.hash(outer1);

    expect(bytesToHex(o1)).toBe(bytesToHex(manualO1));
  });

  it("generateEphemeralKey and x25519 perform valid Diffie-Hellman key exchange", async () => {
    const primitives = createNoisePrimitives();

    const alice = await primitives.generateEphemeralKey();
    const bob = await primitives.generateEphemeralKey();

    expect(alice.publicKey.length).toBe(32);
    expect(alice.privateKey.length).toBe(32);
    expect(bob.publicKey.length).toBe(32);
    expect(bob.privateKey.length).toBe(32);

    const sharedA = await primitives.x25519(alice.privateKey, bob.publicKey);
    const sharedB = await primitives.x25519(bob.privateKey, alice.publicKey);

    expect(sharedA.length).toBe(32);
    expect(bytesToHex(sharedA)).toBe(bytesToHex(sharedB));
  });
});
