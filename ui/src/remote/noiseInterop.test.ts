import { describe, it, expect } from "vitest";
import { createNoiseInitiator } from "./attachFraming";
import { createNoisePrimitives } from "./noisePrimitives";

// Source: .omo/evidence/account-issued-remote-grants/noise-golden-responder-transcript.json
// Produced by emit_golden_responder_transcript in src-tauri/src/remote/attach_crypto.rs on 2026-09-25
const PROLOGUE_HEX =
  "6665727279782d6174746163682d76313a4d414348494e453a53455353494f4e3a45504f4348";
const INITIATOR_STATIC_PUB_HEX =
  "8c60c09873704981788c8647798311432d71809a7b0ec1aab237d79e0a37562a";
const INITIATOR_STATIC_PRIV_HEX =
  "e8476a6a89e7587a1be7587442bf58971be7587442bf58971be7587442bf5897";
const INITIATOR_EPHEMERAL_PUB_HEX =
  "ebdb97a5ee6ebe715911f28e0bbf76028b7c499465a280180fef060aeaf39e3c";
const INITIATOR_EPHEMERAL_PRIV_HEX =
  "89abcdef0123456789abcdef0123456789abcdef0123456789abcdef01234567";
const RESPONDER_STATIC_PUB_HEX =
  "509b6c7d0177a5fac052815bb5eabd4bcfc3f9f32fe8e4a4d976c00fb983851d";
const MSG1_HEX =
  "ebdb97a5ee6ebe715911f28e0bbf76028b7c499465a280180fef060aeaf39e3c189346429b7b139c8216507c4c5bb2f69634720de2982171dd39db651f7d4d5d7dde498ab9ccc101d0bad61d4e2703d1231d17560bd359dde592809f463b7873";
const MSG2_HEX =
  "31f73abc92771f332f1e54608b4f6418d9e278e16020629da90bc2f966db4832a348315ff51e0e2393e8119992e45827";
const TRANSPORT_PLAINTEXT_HEX =
  "6665727279782d6e6f6973652d7472616e73706f72742d706c61696e74657874";
const TRANSPORT_CIPHERTEXT_HEX =
  "4d12c45b577ae399f1580ae9773269b613d9d8e8c8bd23871eda88aed37660dc2323525e6d08a952fab01b2e2b18f924";

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

describe("noiseInterop with Rust snow responder", () => {
  it("interoperates with snow responder golden transcript across handshake and transport frame decryption", async () => {
    const primitives = createNoisePrimitives();

    const initiator = createNoiseInitiator(primitives, {
      localPrivateKey: hexToBytes(INITIATOR_STATIC_PRIV_HEX),
      localPublicKey: hexToBytes(INITIATOR_STATIC_PUB_HEX),
      remotePublicKey: hexToBytes(RESPONDER_STATIC_PUB_HEX),
      prologue: hexToBytes(PROLOGUE_HEX),
      ephemeralOverride: {
        publicKey: hexToBytes(INITIATOR_EPHEMERAL_PUB_HEX),
        privateKey: hexToBytes(INITIATOR_EPHEMERAL_PRIV_HEX),
      },
    });

    const msg1 = await initiator.createHandshakeMessage1();
    expect(bytesToHex(msg1)).toBe(MSG1_HEX);
    expect(initiator.isTransport()).toBe(false);

    await initiator.processHandshakeMessage2(hexToBytes(MSG2_HEX));
    expect(initiator.isTransport()).toBe(true);

    const decrypted = await initiator.decrypt(hexToBytes(TRANSPORT_CIPHERTEXT_HEX));
    expect(bytesToHex(decrypted)).toBe(TRANSPORT_PLAINTEXT_HEX);
  });
});
