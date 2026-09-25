import { describe, it, expect } from "vitest";
import {
  attachPrologue,
  encodeFrame,
  decodeFrames,
  createNoiseInitiator,
  type NoisePrimitives,
  MAX_HANDSHAKE_MESSAGE,
} from "./attachFraming";
import { createNoisePrimitives } from "./noisePrimitives";

const NOISE_PROTOCOL_NAME = new TextEncoder().encode("Noise_IK_25519_ChaChaPoly_BLAKE2s");

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

class TestNoiseResponder {
  private primitives: NoisePrimitives;
  private responderPriv: Uint8Array;
  private responderPub: Uint8Array;
  private prologue: Uint8Array;
  private txKey: Uint8Array | null = null;
  private rxKey: Uint8Array | null = null;
  private txNonce = 0;
  private rxNonce = 0;

  constructor(
    primitives: NoisePrimitives,
    responderPriv: Uint8Array,
    responderPub: Uint8Array,
    prologue: Uint8Array,
  ) {
    this.primitives = primitives;
    this.responderPriv = responderPriv;
    this.responderPub = responderPub;
    this.prologue = prologue;
  }

  async accept(
    msg1: Uint8Array,
    authorize: (initiatorPub: Uint8Array) => boolean
  ): Promise<{ msg2: Uint8Array; initiatorStaticPub: Uint8Array }> {
    if (msg1.length !== 96) {
      throw new Error(`RESPONDER_BAD_MSG1_LEN: expected 96, got ${msg1.length}`);
    }

    const hkdf = this.primitives.hkdf!.bind(this.primitives);

    let h: Uint8Array = await this.primitives.hash(NOISE_PROTOCOL_NAME);
    let ck: Uint8Array = new Uint8Array(h);

    h = await this.primitives.hash(concatBytes(h, this.prologue));
    h = await this.primitives.hash(concatBytes(h, this.responderPub));

    const e_init = msg1.subarray(0, 32);
    h = await this.primitives.hash(concatBytes(h, e_init));

    const dh_es = await this.primitives.x25519(this.responderPriv, e_init);
    const [ck1, k1] = await hkdf(ck, dh_es);
    ck = ck1;
    let n1 = 0;

    const ct_s = msg1.subarray(32, 80);
    const initiatorStaticPub = await this.primitives.aeadDecrypt(k1, n1++, h, ct_s);
    h = await this.primitives.hash(concatBytes(h, ct_s));

    if (!authorize(initiatorStaticPub)) {
      throw new Error("UNAUTHORIZED_INITIATOR_KEY");
    }

    const dh_ss = await this.primitives.x25519(this.responderPriv, initiatorStaticPub);
    const [ck2, k2] = await hkdf(ck, dh_ss);
    ck = ck2;
    let n2 = 0;

    const ct_p = msg1.subarray(80, 96);
    await this.primitives.aeadDecrypt(k2, n2++, h, ct_p);
    h = await this.primitives.hash(concatBytes(h, ct_p));

    const re_resp = await this.primitives.generateEphemeralKey();
    h = await this.primitives.hash(concatBytes(h, re_resp.publicKey));

    const dh_ee = await this.primitives.x25519(re_resp.privateKey, e_init);
    const [ck3] = await hkdf(ck, dh_ee);
    ck = ck3;

    const dh_se = await this.primitives.x25519(re_resp.privateKey, initiatorStaticPub);
    const [ck4, k4] = await hkdf(ck, dh_se);
    ck = ck4;
    let n4 = 0;

    const ct_p2 = await this.primitives.aeadEncrypt(k4, n4++, h, new Uint8Array(0));
    h = await this.primitives.hash(concatBytes(h, ct_p2));

    const msg2 = concatBytes(re_resp.publicKey, ct_p2);

    const [rx, tx] = await hkdf(ck, new Uint8Array(0));
    this.rxKey = rx;
    this.txKey = tx;
    this.txNonce = 0;
    this.rxNonce = 0;

    return { msg2, initiatorStaticPub };
  }

  async encrypt(plaintext: Uint8Array): Promise<Uint8Array> {
    if (!this.txKey) throw new Error("NOT_IN_TRANSPORT_MODE");
    return this.primitives.aeadEncrypt(this.txKey, this.txNonce++, new Uint8Array(0), plaintext);
  }

  async decrypt(ciphertext: Uint8Array): Promise<Uint8Array> {
    if (!this.rxKey) throw new Error("NOT_IN_TRANSPORT_MODE");
    return this.primitives.aeadDecrypt(this.rxKey, this.rxNonce++, new Uint8Array(0), ciphertext);
  }
}

describe("attachFraming", () => {
  it("attachPrologue produces exact UTF-8 bytes for known triple", () => {
    const machineId = "mach-123";
    const sessionId = "sess-456";
    const epoch = 789;
    const prologue = attachPrologue(machineId, sessionId, epoch);

    const decoded = new TextDecoder().decode(prologue);
    expect(decoded).toBe("ferryx-attach-v1:mach-123:sess-456:789");
  });

  it("encodeFrame produces 4-byte BE prefix and decodeFrames round-trips complete messages", () => {
    const msg1 = new Uint8Array([1, 2, 3, 4, 5]);
    const frame1 = encodeFrame(msg1);

    expect(frame1.length).toBe(4 + 5);
    const view = new DataView(frame1.buffer, frame1.byteOffset, 4);
    expect(view.getUint32(0, false)).toBe(5);
    expect(Array.from(frame1.subarray(4))).toEqual(Array.from(msg1));

    const { messages, rest } = decodeFrames(frame1);
    expect(messages).toHaveLength(1);
    expect(Array.from(messages[0])).toEqual(Array.from(msg1));
    expect(rest.length).toBe(0);
  });

  it("decodeFrames round-trips a stream split across arbitrary chunk boundaries including mid-length-prefix", () => {
    const msg1 = new TextEncoder().encode("first message");
    const msg2 = new TextEncoder().encode("second message longer than first");
    const frame1 = encodeFrame(msg1);
    const frame2 = encodeFrame(msg2);

    const combined = new Uint8Array(frame1.length + frame2.length);
    combined.set(frame1, 0);
    combined.set(frame2, frame1.length);

    const chunk1 = combined.subarray(0, 19);
    const chunk2 = combined.subarray(19);

    const res1 = decodeFrames(chunk1);
    expect(res1.messages).toHaveLength(1);
    expect(Array.from(res1.messages[0])).toEqual(Array.from(msg1));
    expect(res1.rest.length).toBe(2);

    const recombined2 = new Uint8Array(res1.rest.length + chunk2.length);
    recombined2.set(res1.rest, 0);
    recombined2.set(chunk2, res1.rest.length);

    const res2 = decodeFrames(recombined2);
    expect(res2.messages).toHaveLength(1);
    expect(Array.from(res2.messages[0])).toEqual(Array.from(msg2));
    expect(res2.rest.length).toBe(0);
  });

  it("rejects declared lengths above MAX_HANDSHAKE_MESSAGE in encodeFrame and decodeFrames", () => {
    const oversizedMsg = new Uint8Array(MAX_HANDSHAKE_MESSAGE + 1);
    expect(() => encodeFrame(oversizedMsg)).toThrow(/FRAME_TOO_LARGE/);

    const oversizedHeader = new Uint8Array(8);
    const view = new DataView(oversizedHeader.buffer, oversizedHeader.byteOffset, 8);
    view.setUint32(0, MAX_HANDSHAKE_MESSAGE + 1, false);
    expect(() => decodeFrames(oversizedHeader)).toThrow(/FRAME_TOO_LARGE/);
  });

  it("executes full msg1 -> msg2 -> transport round trip against minimal responder", async () => {
    const primitives = createNoisePrimitives();

    const initiatorKeys = await primitives.generateEphemeralKey();
    const responderKeys = await primitives.generateEphemeralKey();

    const prologue = attachPrologue("mach-1", "sess-1", 42);

    const initiator = createNoiseInitiator(primitives, {
      localPrivateKey: initiatorKeys.privateKey,
      localPublicKey: initiatorKeys.publicKey,
      remotePublicKey: responderKeys.publicKey,
      prologue,
    });

    const responder = new TestNoiseResponder(
      primitives,
      responderKeys.privateKey,
      responderKeys.publicKey,
      prologue
    );

    const msg1 = await initiator.createHandshakeMessage1();
    expect(msg1.length).toBe(96);
    expect(initiator.isTransport()).toBe(false);

    let observedInitiatorKey: Uint8Array | null = null;
    const { msg2, initiatorStaticPub } = await responder.accept(msg1, (key) => {
      observedInitiatorKey = key;
      return true;
    });

    expect(msg2.length).toBe(48);
    expect(observedInitiatorKey).not.toBeNull();
    expect(Array.from(observedInitiatorKey!)).toEqual(Array.from(initiatorKeys.publicKey));
    expect(Array.from(observedInitiatorKey!)).not.toEqual(Array.from(initiatorKeys.privateKey));
    expect(Array.from(initiatorStaticPub)).toEqual(Array.from(initiatorKeys.publicKey));

    await initiator.processHandshakeMessage2(msg2);
    expect(initiator.isTransport()).toBe(true);

    const msgA1 = new TextEncoder().encode("hello from initiator");
    const ctA1 = await initiator.encrypt(msgA1);
    const ptA1 = await responder.decrypt(ctA1);
    expect(new TextDecoder().decode(ptA1)).toBe("hello from initiator");

    const msgB1 = new TextEncoder().encode("hello from responder");
    const ctB1 = await responder.encrypt(msgB1);
    const ptB1 = await initiator.decrypt(ctB1);
    expect(new TextDecoder().decode(ptB1)).toBe("hello from responder");

    const msgA2 = new TextEncoder().encode("second message from initiator");
    const ctA2 = await initiator.encrypt(msgA2);
    const ptA2 = await responder.decrypt(ctA2);
    expect(new TextDecoder().decode(ptA2)).toBe("second message from initiator");
  });

  it("createNoiseInitiator refuses to run when required primitives are missing", () => {
    const incompletePrimitives = {
      generateEphemeralKey: async () => ({ publicKey: new Uint8Array(32), privateKey: new Uint8Array(32) }),
    } as unknown as NoisePrimitives;

    expect(() =>
      createNoiseInitiator(incompletePrimitives, {
        localPrivateKey: new Uint8Array(32),
        remotePublicKey: new Uint8Array(32),
        prologue: new Uint8Array(10),
      }),
    ).toThrow(/NOISE_PRIMITIVES_MISSING/);
  });
});
