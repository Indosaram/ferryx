import { createNoisePrimitives } from "../src/remote/noisePrimitives";
import { createNoiseInitiator } from "../src/remote/attachFraming";

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

function concatBytes(...arrays: Uint8Array[]): Uint8Array {
  const totalLength = arrays.reduce((sum, a) => sum + a.length, 0);
  const result = new Uint8Array(totalLength);
  let offset = 0;
  for (const arr of arrays) {
    result.set(arr, offset);
    offset += arr.length;
  }
  return result;
}

async function main() {
  const primitives = createNoisePrimitives();
  const basePoint = new Uint8Array(32);
  basePoint[0] = 9;

  const initiatorStaticPriv = hexToBytes(
    "e8476a6a89e7587a1be7587442bf58971be7587442bf58971be7587442bf5897"
  );
  const initiatorStaticPub = await primitives.x25519(initiatorStaticPriv, basePoint);

  const responderStaticPriv = hexToBytes(
    "546f6b696f527573744461656d6f6e5374617469634b65793031323334353637"
  );
  const responderStaticPub = await primitives.x25519(responderStaticPriv, basePoint);

  const initiatorEphemeralPriv = hexToBytes(
    "89abcdef0123456789abcdef0123456789abcdef0123456789abcdef01234567"
  );
  const initiatorEphemeralPub = await primitives.x25519(initiatorEphemeralPriv, basePoint);

  const responderEphemeralPriv = hexToBytes(
    "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
  );
  const responderEphemeralPub = await primitives.x25519(responderEphemeralPriv, basePoint);

  const prologue = new TextEncoder().encode("ferryx-attach-v1:MACHINE:SESSION:EPOCH");
  const protocolName = new TextEncoder().encode("Noise_IK_25519_ChaChaPoly_BLAKE2s");

  const initiator = createNoiseInitiator(primitives, {
    localPrivateKey: initiatorStaticPriv,
    localPublicKey: initiatorStaticPub,
    remotePublicKey: responderStaticPub,
    prologue,
    ephemeralOverride: {
      publicKey: initiatorEphemeralPub,
      privateKey: initiatorEphemeralPriv,
    },
  });

  const msg1 = await initiator.createHandshakeMessage1();

  const hkdf = primitives.hkdf!.bind(primitives);
  let respH: Uint8Array = await primitives.hash(protocolName);
  let respCk: Uint8Array = new Uint8Array(respH);

  respH = await primitives.hash(concatBytes(respH, prologue));
  respH = await primitives.hash(concatBytes(respH, responderStaticPub));

  const eInit = msg1.subarray(0, 32);
  respH = await primitives.hash(concatBytes(respH, eInit));

  const dhEs = await primitives.x25519(responderStaticPriv, eInit);
  const [ck1, k1] = await hkdf(respCk, dhEs);
  respCk = ck1;
  let n1 = 0;

  const ctS = msg1.subarray(32, 80);
  const recoveredInitPub = await primitives.aeadDecrypt(k1, n1++, respH, ctS);
  respH = await primitives.hash(concatBytes(respH, ctS));

  const dhSs = await primitives.x25519(responderStaticPriv, recoveredInitPub);
  const [ck2, k2] = await hkdf(respCk, dhSs);
  respCk = ck2;
  let n2 = 0;

  const ctP = msg1.subarray(80, 96);
  await primitives.aeadDecrypt(k2, n2++, respH, ctP);
  respH = await primitives.hash(concatBytes(respH, ctP));

  respH = await primitives.hash(concatBytes(respH, responderEphemeralPub));

  const dhEe = await primitives.x25519(responderEphemeralPriv, eInit);
  const [ck3] = await hkdf(respCk, dhEe);
  respCk = ck3;

  const dhSe = await primitives.x25519(responderEphemeralPriv, recoveredInitPub);
  const [ck4, k4] = await hkdf(respCk, dhSe);
  respCk = ck4;
  let n4 = 0;

  const ctP2 = await primitives.aeadEncrypt(k4, n4++, respH, new Uint8Array(0));
  respH = await primitives.hash(concatBytes(respH, ctP2));

  const msg2 = concatBytes(responderEphemeralPub, ctP2);

  const [respRxKey] = await hkdf(respCk, new Uint8Array(0));

  await initiator.processHandshakeMessage2(msg2);

  const transportPlaintext = new TextEncoder().encode("ferryx-noise-transport-plaintext");
  const transportCiphertext = await initiator.encrypt(transportPlaintext);

  const decryptedPlaintext = await primitives.aeadDecrypt(
    respRxKey,
    0,
    new Uint8Array(0),
    transportCiphertext
  );
  if (bytesToHex(decryptedPlaintext) !== bytesToHex(transportPlaintext)) {
    throw new Error("TRANSPORT_DECRYPT_SELF_CHECK_FAILED");
  }

  const golden = {
    protocol: "Noise_IK_25519_ChaChaPoly_BLAKE2s",
    prologue_hex: bytesToHex(prologue),
    initiator_static_pub_hex: bytesToHex(initiatorStaticPub),
    initiator_static_priv_hex: bytesToHex(initiatorStaticPriv),
    responder_static_pub_hex: bytesToHex(responderStaticPub),
    responder_static_priv_hex: bytesToHex(responderStaticPriv),
    initiator_ephemeral_pub_hex: bytesToHex(initiatorEphemeralPub),
    initiator_ephemeral_priv_hex: bytesToHex(initiatorEphemeralPriv),
    msg1_hex: bytesToHex(msg1),
    transport_plaintext_hex: bytesToHex(transportPlaintext),
    transport_plaintext_ciphertext_hex: bytesToHex(transportCiphertext),
    expected_transport_key_hex: bytesToHex(respRxKey),
  };

  console.log(JSON.stringify(golden));
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
