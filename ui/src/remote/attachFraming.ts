export const MAX_ATTACH_FRAME = 65519;
export const MAX_HANDSHAKE_MESSAGE = 65535;

export function attachPrologue(
  machineId: string,
  sessionId: string,
  enrollmentEpoch: number | string,
): Uint8Array {
  const s = `ferryx-attach-v1:${machineId}:${sessionId}:${enrollmentEpoch}`;
  return new TextEncoder().encode(s);
}

export function encodeFrame(message: Uint8Array): Uint8Array {
  const len = message.length;
  if (len > MAX_HANDSHAKE_MESSAGE) {
    throw new Error(`FRAME_TOO_LARGE: message length ${len} exceeds MAX_HANDSHAKE_MESSAGE (${MAX_HANDSHAKE_MESSAGE})`);
  }
  const frame = new Uint8Array(4 + len);
  const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
  view.setUint32(0, len, false);
  frame.set(message, 4);
  return frame;
}

export function decodeFrames(buffer: Uint8Array): { messages: Uint8Array[]; rest: Uint8Array } {
  const messages: Uint8Array[] = [];
  let offset = 0;

  while (offset + 4 <= buffer.length) {
    const view = new DataView(buffer.buffer, buffer.byteOffset + offset, 4);
    const frameLen = view.getUint32(0, false);

    if (frameLen > MAX_HANDSHAKE_MESSAGE) {
      throw new Error(`FRAME_TOO_LARGE: declared frame length ${frameLen} exceeds MAX_HANDSHAKE_MESSAGE (${MAX_HANDSHAKE_MESSAGE})`);
    }

    if (offset + 4 + frameLen > buffer.length) {
      break;
    }

    const msg = buffer.subarray(offset + 4, offset + 4 + frameLen);
    messages.push(new Uint8Array(msg));
    offset += 4 + frameLen;
  }

  const rest = buffer.subarray(offset);
  return {
    messages,
    rest: new Uint8Array(rest),
  };
}

export interface NoisePrimitives {
  generateEphemeralKey(): Promise<{ publicKey: Uint8Array; privateKey: Uint8Array }>;
  x25519(privateKey: Uint8Array, publicKey: Uint8Array): Promise<Uint8Array>;
  aeadEncrypt(key: Uint8Array, nonce: number, associatedData: Uint8Array, plaintext: Uint8Array): Promise<Uint8Array>;
  aeadDecrypt(key: Uint8Array, nonce: number, associatedData: Uint8Array, ciphertext: Uint8Array): Promise<Uint8Array>;
  hash(data: Uint8Array): Promise<Uint8Array>;
  hkdf?(ck: Uint8Array, ikm: Uint8Array): Promise<[Uint8Array, Uint8Array]>;
  hkdf3?(ck: Uint8Array, ikm: Uint8Array): Promise<[Uint8Array, Uint8Array, Uint8Array]>;
}

export interface NoiseInitiatorConfig {
  localPrivateKey: Uint8Array;
  localPublicKey?: Uint8Array;
  remotePublicKey: Uint8Array;
  prologue: Uint8Array;
  ephemeralOverride?: { publicKey: Uint8Array; privateKey: Uint8Array };
}

export interface NoiseInitiator {
  createHandshakeMessage1(): Promise<Uint8Array>;
  processHandshakeMessage2(message: Uint8Array): Promise<void>;
  encrypt(plaintext: Uint8Array): Promise<Uint8Array>;
  decrypt(ciphertext: Uint8Array): Promise<Uint8Array>;
  isTransport(): boolean;
}

const NOISE_PROTOCOL_NAME = new TextEncoder().encode("Noise_IK_25519_ChaChaPoly_BLAKE2s");
const BASE_POINT_U9 = new Uint8Array(32);
BASE_POINT_U9[0] = 9;

export function createNoiseInitiator(
  primitives: NoisePrimitives,
  config: NoiseInitiatorConfig,
): NoiseInitiator {
  if (
    !primitives ||
    typeof primitives.generateEphemeralKey !== "function" ||
    typeof primitives.x25519 !== "function" ||
    typeof primitives.aeadEncrypt !== "function" ||
    typeof primitives.aeadDecrypt !== "function" ||
    typeof primitives.hash !== "function"
  ) {
    throw new Error("NOISE_PRIMITIVES_MISSING: required cryptographic primitives are unavailable");
  }

  if (typeof primitives.hkdf !== "function") {
    throw new Error("NOISE_PRIMITIVES_MISSING: hkdf primitive is required");
  }

  if (!config.localPrivateKey || config.localPrivateKey.length === 0) {
    throw new Error("NOISE_CONFIG_INVALID: localPrivateKey is required");
  }
  if (!config.remotePublicKey || config.remotePublicKey.length === 0) {
    throw new Error("NOISE_CONFIG_INVALID: remotePublicKey is required");
  }

  const hkdf = primitives.hkdf.bind(primitives);

  let phase = 0;
  let txNonce = 0;
  let rxNonce = 0;

  let ephemeral: { publicKey: Uint8Array; privateKey: Uint8Array } | null = null;
  let handshakeH: Uint8Array | null = null;
  let handshakeCk: Uint8Array | null = null;
  let txKey: Uint8Array | null = null;
  let rxKey: Uint8Array | null = null;

  return {
    async createHandshakeMessage1(): Promise<Uint8Array> {
      if (phase !== 0) {
        throw new Error(`INVALID_HANDSHAKE_STATE: cannot emit message 1 in phase ${phase}`);
      }

      let h: Uint8Array = await primitives.hash(NOISE_PROTOCOL_NAME);
      let ck: Uint8Array = new Uint8Array(h);

      h = await primitives.hash(concatBytes(h, config.prologue));
      h = await primitives.hash(concatBytes(h, config.remotePublicKey));

      ephemeral = config.ephemeralOverride ?? (await primitives.generateEphemeralKey());
      h = await primitives.hash(concatBytes(h, ephemeral.publicKey));

      const dh_es = await primitives.x25519(ephemeral.privateKey, config.remotePublicKey);
      const [ck1, k1] = await hkdf(ck, dh_es);
      ck = ck1;
      let n1 = 0;

      let localPub = config.localPublicKey;
      if (!localPub) {
        localPub = await primitives.x25519(config.localPrivateKey, BASE_POINT_U9);
      }
      const ct_s = await primitives.aeadEncrypt(k1, n1++, h, localPub);
      h = await primitives.hash(concatBytes(h, ct_s));

      const dh_ss = await primitives.x25519(config.localPrivateKey, config.remotePublicKey);
      const [ck2, k2] = await hkdf(ck, dh_ss);
      ck = ck2;
      let n2 = 0;

      const ct_p = await primitives.aeadEncrypt(k2, n2++, h, new Uint8Array(0));
      h = await primitives.hash(concatBytes(h, ct_p));

      handshakeH = h;
      handshakeCk = ck;
      phase = 1;

      return concatBytes(ephemeral.publicKey, ct_s, ct_p);
    },

    async processHandshakeMessage2(message: Uint8Array): Promise<void> {
      if (phase !== 1 || !ephemeral || !handshakeH || !handshakeCk) {
        throw new Error(`INVALID_HANDSHAKE_STATE: cannot process message 2 in phase ${phase}`);
      }
      if (message.length < 48) {
        throw new Error("HANDSHAKE_MESSAGE_TOO_SHORT: message 2 must be at least 48 bytes");
      }

      let h: Uint8Array = handshakeH;
      let ck: Uint8Array = handshakeCk;

      const re = message.subarray(0, 32);
      h = await primitives.hash(concatBytes(h, re));

      const dh_ee = await primitives.x25519(ephemeral.privateKey, re);
      const [ck3] = await hkdf(ck, dh_ee);
      ck = ck3;

      const dh_se = await primitives.x25519(config.localPrivateKey, re);
      const [ck4, k4] = await hkdf(ck, dh_se);
      ck = ck4;
      let n4 = 0;

      const payloadCt = message.subarray(32);
      await primitives.aeadDecrypt(k4, n4++, h, payloadCt);
      h = await primitives.hash(concatBytes(h, payloadCt));

      const [k1, k2] = await hkdf(ck, new Uint8Array(0));
      txKey = k1;
      rxKey = k2;

      phase = 2;
      txNonce = 0;
      rxNonce = 0;
    },

    async encrypt(plaintext: Uint8Array): Promise<Uint8Array> {
      if (phase !== 2 || !txKey) {
        throw new Error("NOT_IN_TRANSPORT_MODE: cannot encrypt before completing handshake");
      }
      const nonce = txNonce++;
      return primitives.aeadEncrypt(txKey, nonce, new Uint8Array(0), plaintext);
    },

    async decrypt(ciphertext: Uint8Array): Promise<Uint8Array> {
      if (phase !== 2 || !rxKey) {
        throw new Error("NOT_IN_TRANSPORT_MODE: cannot decrypt before completing handshake");
      }
      const nonce = rxNonce++;
      return primitives.aeadDecrypt(rxKey, nonce, new Uint8Array(0), ciphertext);
    },

    isTransport(): boolean {
      return phase === 2;
    },
  };
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
