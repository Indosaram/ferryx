import { describe, it, expect, vi } from "vitest";
import { readFileSync } from "fs";
import { resolve } from "path";
import {
  createTunnelTransport,
  openAccountTunnel,
  WS_OPEN,
  WS_CLOSED,
  type TunnelByteStream,
} from "./attachTunnel";
import {
  attachPrologue,
  encodeFrame,
  decodeFrames,
  type NoisePrimitives,
} from "./attachFraming";
import { createNoisePrimitives } from "./noisePrimitives";
import { getOrCreateAttachKey } from "./accountAttach";

class ScriptedByteStream implements TunnelByteStream {
  public written: Uint8Array[] = [];
  private incomingQueue: Uint8Array[] = [];
  private pendingRead: ((chunk: Uint8Array) => void) | null = null;
  private writeWaiters: Array<{ count: number; resolve: () => void }> = [];
  public closed = false;

  pushIncoming(data: Uint8Array | string): void {
    const chunk = typeof data === "string" ? new TextEncoder().encode(data) : data;
    if (this.pendingRead) {
      const resolveRead = this.pendingRead;
      this.pendingRead = null;
      resolveRead(chunk);
    } else {
      this.incomingQueue.push(chunk);
    }
  }

  async write(bytes: Uint8Array): Promise<void> {
    this.written.push(bytes);
    this.checkWaiters();
  }

  async read(): Promise<Uint8Array> {
    if (this.incomingQueue.length > 0) {
      return this.incomingQueue.shift()!;
    }
    if (this.closed) {
      return new Uint8Array(0);
    }
    return new Promise<Uint8Array>((resolve) => {
      this.pendingRead = resolve;
    });
  }

  close(): void {
    this.closed = true;
    if (this.pendingRead) {
      const resolveRead = this.pendingRead;
      this.pendingRead = null;
      resolveRead(new Uint8Array(0));
    }
  }

  waitForWrites(count: number): Promise<void> {
    if (this.written.length >= count) {
      return Promise.resolve();
    }
    return new Promise<void>((resolve) => {
      this.writeWaiters.push({ count, resolve });
    });
  }

  async waitForNextWrite(): Promise<Uint8Array> {
    const targetCount = this.written.length + 1;
    await this.waitForWrites(targetCount);
    return this.written[targetCount - 1];
  }

  private checkWaiters(): void {
    const remaining: Array<{ count: number; resolve: () => void }> = [];
    for (const waiter of this.writeWaiters) {
      if (this.written.length >= waiter.count) {
        waiter.resolve();
      } else {
        remaining.push(waiter);
      }
    }
    this.writeWaiters = remaining;
  }
}

function concatAll(arrays: Uint8Array[]): Uint8Array {
  const total = arrays.reduce((acc, a) => acc + a.length, 0);
  const out = new Uint8Array(total);
  let off = 0;
  for (const a of arrays) {
    out.set(a, off);
    off += a.length;
  }
  return out;
}

async function computeAcceptKey(secKey: string): Promise<string> {
  const acceptInput = new TextEncoder().encode(secKey + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
  const digest = await crypto.subtle.digest("SHA-1", acceptInput);
  let binary = "";
  const digestBytes = new Uint8Array(digest);
  for (let i = 0; i < digestBytes.length; i++) {
    binary += String.fromCharCode(digestBytes[i]);
  }
  return btoa(binary);
}

async function establishTestWebSocket(
  transport: ReturnType<typeof createTunnelTransport>,
  stream: ScriptedByteStream,
  path = "/api/v1/ws",
) {
  const wsPromise = transport.openWebSocket(path);
  await stream.waitForWrites(1);
  const reqText = new TextDecoder().decode(concatAll(stream.written));
  const secKey = reqText.match(/Sec-WebSocket-Key:\s*([^\r\n]+)/i)![1].trim();
  const acceptKey = await computeAcceptKey(secKey);

  stream.pushIncoming(
    `HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${acceptKey}\r\n\r\n`,
  );
  return wsPromise;
}

describe("attachTunnel HTTP/1.1 transport", () => {
  it("parses Content-Length response, exact request line, and matches Content-Length to body", async () => {
    // Given a scripted byte stream and tunnel transport
    const stream = new ScriptedByteStream();
    const transport = createTunnelTransport(stream);

    // When a POST request with body is initiated
    const requestPromise = transport.fetchLike("/api/v1/pair/exchange", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: '{"token":"123456"}',
    });

    // Then wait for the request to be written to the stream
    await stream.waitForWrites(1);
    const writtenBytes = concatAll(stream.written);
    const reqText = new TextDecoder().decode(writtenBytes);
    expect(reqText.startsWith("POST /api/v1/pair/exchange HTTP/1.1\r\n")).toBe(true);
    expect(reqText.includes("Host: localhost\r\n")).toBe(true);
    expect(reqText.includes("Connection: keep-alive\r\n")).toBe(true);
    const expectedBody = '{"token":"123456"}';
    expect(reqText.includes(`Content-Length: ${expectedBody.length}\r\n`)).toBe(true);
    expect(reqText.endsWith(`\r\n\r\n${expectedBody}`)).toBe(true);

    // When the server replies with Content-Length response
    const resPayload = '{"status":"paired"}';
    stream.pushIncoming(
      `HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ${resPayload.length}\r\n\r\n${resPayload}`,
    );

    // Then response is parsed correctly
    const res = await requestPromise;
    expect(res.status).toBe(200);
    expect(res.headers["content-type"]).toBe("application/json");
    expect(new TextDecoder().decode(res.body)).toBe(resPayload);
  });

  it("parses chunked transfer-encoding response", async () => {
    // Given a transport
    const stream = new ScriptedByteStream();
    const transport = createTunnelTransport(stream);

    // When a GET request is dispatched
    const requestPromise = transport.fetchLike("/api/v1/stream");
    await stream.waitForWrites(1);

    // When the server responds with chunked body
    stream.pushIncoming(
      "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n" +
        "5\r\nhello\r\n" +
        "7\r\n chunks\r\n" +
        "0\r\n\r\n",
    );

    // Then the assembled body matches the chunks concatenated
    const res = await requestPromise;
    expect(res.status).toBe(200);
    expect(new TextDecoder().decode(res.body)).toBe("hello chunks");
  });

  it("handles two sequential requests with glued responses in a single incoming chunk", async () => {
    // Given a transport and two sequential requests
    const stream = new ScriptedByteStream();
    const transport = createTunnelTransport(stream);

    const req1 = transport.fetchLike("/first");
    const req2 = transport.fetchLike("/second");
    await stream.waitForWrites(1);

    // When server returns both responses glued together in a single chunk
    const gluedResponses =
      "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nFIRST" +
      "HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nSECOND";

    stream.pushIncoming(gluedResponses);

    // Then buffer splitting allows both requests to resolve correctly
    const res1 = await req1;
    const res2 = await req2;

    expect(res1.status).toBe(200);
    expect(new TextDecoder().decode(res1.body)).toBe("FIRST");

    expect(res2.status).toBe(200);
    expect(new TextDecoder().decode(res2.body)).toBe("SECOND");
  });
});

describe("attachTunnel RFC 6455 WebSocket transport", () => {
  it("carries Upgrade, Sec-WebSocket-Key, and Sec-WebSocket-Version in handshake", async () => {
    // Given a transport
    const stream = new ScriptedByteStream();
    const transport = createTunnelTransport(stream);

    // When openWebSocket is initiated
    const wsPromise = transport.openWebSocket("/api/v1/terminal/sess-1");

    // Then wait for handshake request to be written
    await stream.waitForWrites(1);
    const writtenBytes = concatAll(stream.written);
    const reqText = new TextDecoder().decode(writtenBytes);

    expect(reqText.startsWith("GET /api/v1/terminal/sess-1 HTTP/1.1\r\n")).toBe(true);
    expect(reqText.includes("Upgrade: websocket\r\n")).toBe(true);
    expect(reqText.includes("Connection: Upgrade\r\n")).toBe(true);
    expect(reqText.includes("Sec-WebSocket-Version: 13\r\n")).toBe(true);

    const keyMatch = reqText.match(/Sec-WebSocket-Key:\s*([^\r\n]+)/i);
    expect(keyMatch).not.toBeNull();
    const secKey = keyMatch![1].trim();

    // When server computes accept and responds with 101
    const acceptKey = await computeAcceptKey(secKey);
    stream.pushIncoming(
      `HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${acceptKey}\r\n\r\n`,
    );

    const ws = await wsPromise;
    expect(ws.readyState).toBe(WS_OPEN);
  });

  it("reassembles a server frame split across two incoming chunks", async () => {
    // Given an established WebSocket
    const stream = new ScriptedByteStream();
    const transport = createTunnelTransport(stream);
    const ws = await establishTestWebSocket(transport, stream);

    // When server frame for 'hello world' is split across two chunks
    const messagePromise = new Promise<string>((resolve) => {
      ws.onmessage = (event) => resolve(event.data as string);
    });

    const fullPayload = new TextEncoder().encode("hello world");
    const frameHeader = new Uint8Array([0x81, fullPayload.length]);
    const fullFrame = new Uint8Array(frameHeader.length + fullPayload.length);
    fullFrame.set(frameHeader, 0);
    fullFrame.set(fullPayload, frameHeader.length);

    const chunk1 = fullFrame.slice(0, 6);
    const chunk2 = fullFrame.slice(6);

    stream.pushIncoming(chunk1);
    stream.pushIncoming(chunk2);

    // Then the full message is delivered
    const message = await messagePromise;
    expect(message).toBe("hello world");
  });

  it("masks client frames and uses extended lengths for 126+ and 65536+ payloads", async () => {
    // Given an established WebSocket
    const stream = new ScriptedByteStream();
    const transport = createTunnelTransport(stream);
    const ws = await establishTestWebSocket(transport, stream);

    // When sending small text frame (len 11)
    const writeCountBeforeSmall = stream.written.length;
    ws.send("hello world");
    await stream.waitForWrites(writeCountBeforeSmall + 1);
    const smallFrame = stream.written[writeCountBeforeSmall];
    expect(smallFrame[0]).toBe(0x81);
    expect(smallFrame[1] & 0x80).toBe(0x80);
    expect(smallFrame[1] & 0x7f).toBe(11);
    const smallMask = smallFrame.slice(2, 6);
    const unmaskedSmall = new Uint8Array(11);
    for (let i = 0; i < 11; i++) {
      unmaskedSmall[i] = smallFrame[6 + i] ^ smallMask[i % 4];
    }
    expect(new TextDecoder().decode(unmaskedSmall)).toBe("hello world");

    // When sending medium binary frame (len 126)
    const writeCountBeforeMed = stream.written.length;
    const payload126 = new Uint8Array(126).fill(0x77);
    ws.send(payload126);
    await stream.waitForWrites(writeCountBeforeMed + 1);
    const medFrame = stream.written[writeCountBeforeMed];
    expect(medFrame[0]).toBe(0x82);
    expect(medFrame[1] & 0x80).toBe(0x80);
    expect(medFrame[1] & 0x7f).toBe(126);
    const view16 = new DataView(medFrame.buffer, medFrame.byteOffset, medFrame.byteLength);
    expect(view16.getUint16(2, false)).toBe(126);
    const mask16 = medFrame.slice(4, 8);
    const unmasked126 = new Uint8Array(126);
    for (let i = 0; i < 126; i++) {
      unmasked126[i] = medFrame[8 + i] ^ mask16[i % 4];
    }
    expect(unmasked126).toEqual(payload126);

    // When sending large binary frame (len 65536)
    const writeCountBeforeLarge = stream.written.length;
    const payload65536 = new Uint8Array(65536).fill(0x55);
    ws.send(payload65536);
    await stream.waitForWrites(writeCountBeforeLarge + 1);
    const largeFrame = stream.written[writeCountBeforeLarge];
    expect(largeFrame[0]).toBe(0x82);
    expect(largeFrame[1] & 0x80).toBe(0x80);
    expect(largeFrame[1] & 0x7f).toBe(127);
    const view64 = new DataView(largeFrame.buffer, largeFrame.byteOffset, largeFrame.byteLength);
    expect(view64.getBigUint64(2, false)).toBe(65536n);
    const mask64 = largeFrame.slice(10, 14);
    const unmasked65536 = new Uint8Array(65536);
    for (let i = 0; i < 65536; i++) {
      unmasked65536[i] = largeFrame[14 + i] ^ mask64[i % 4];
    }
    expect(unmasked65536).toEqual(payload65536);
  });

  it("responds to server Ping with client Pong", async () => {
    // Given an established WebSocket
    const stream = new ScriptedByteStream();
    const transport = createTunnelTransport(stream);
    const ws = await establishTestWebSocket(transport, stream);
    expect(ws.readyState).toBe(WS_OPEN);

    const writeCountBeforePing = stream.written.length;

    // When server sends Ping frame (opcode 0x9)
    const pingFrame = new Uint8Array([0x89, 4, 1, 2, 3, 4]);
    stream.pushIncoming(pingFrame);

    // Then wait for Pong frame to be written
    await stream.waitForWrites(writeCountBeforePing + 1);
    const pongFrame = stream.written[writeCountBeforePing];
    expect(pongFrame[0]).toBe(0x8a);
    expect(pongFrame[1] & 0x80).toBe(0x80);
    expect(pongFrame[1] & 0x7f).toBe(4);
    const mask = pongFrame.slice(2, 6);
    const unmaskedPong = new Uint8Array(4);
    for (let i = 0; i < 4; i++) {
      unmaskedPong[i] = pongFrame[6 + i] ^ mask[i % 4];
    }
    expect(Array.from(unmaskedPong)).toEqual([1, 2, 3, 4]);
  });

  it("fires onclose when server sends close frame", async () => {
    // Given an established WebSocket
    const stream = new ScriptedByteStream();
    const transport = createTunnelTransport(stream);
    const ws = await establishTestWebSocket(transport, stream);

    // When server sends Close frame (opcode 0x8) with code 1000 and reason 'normal'
    const closePromise = new Promise<{ code: number; reason: string; wasClean: boolean }>((resolve) => {
      ws.onclose = (event) => resolve(event);
    });

    const reason = new TextEncoder().encode("normal");
    const payload = new Uint8Array(2 + reason.length);
    new DataView(payload.buffer).setUint16(0, 1000, false);
    payload.set(reason, 2);

    const closeFrame = new Uint8Array(2 + payload.length);
    closeFrame[0] = 0x88;
    closeFrame[1] = payload.length;
    closeFrame.set(payload, 2);

    stream.pushIncoming(closeFrame);

    // Then onclose fires with clean close
    const closeEvent = await closePromise;
    expect(closeEvent.code).toBe(1000);
    expect(closeEvent.reason).toBe("normal");
    expect(closeEvent.wasClean).toBe(true);
    expect(ws.readyState).toBe(WS_CLOSED);
  });

  it("handles unscripted invalid bytes by firing onerror and onclose without hanging", async () => {
    // Given an established WebSocket
    const stream = new ScriptedByteStream();
    const transport = createTunnelTransport(stream);
    const ws = await establishTestWebSocket(transport, stream);

    const errorFired = new Promise<boolean>((resolve) => {
      ws.onerror = () => resolve(true);
    });
    const closeFired = new Promise<boolean>((resolve) => {
      ws.onclose = () => resolve(true);
    });

    // When unexpected invalid opcode 0xF arrives
    stream.pushIncoming(new Uint8Array([0x8f, 0x00]));

    // Then both onerror and onclose fire and state transitions to closed
    const hadError = await errorFired;
    const hadClose = await closeFired;
    expect(hadError).toBe(true);
    expect(hadClose).toBe(true);
    expect(ws.readyState).toBe(WS_CLOSED);
  });

  it("enforces stream exclusivity: fetchLike fails after 101 upgrade", async () => {
    // Given an upgraded transport
    const stream = new ScriptedByteStream();
    const transport = createTunnelTransport(stream);
    await establishTestWebSocket(transport, stream);

    // When attempting fetchLike after upgrade
    // Then it rejects with STREAM_UPGRADED
    await expect(transport.fetchLike("/api/v1/sessions")).rejects.toThrow(/STREAM_UPGRADED/);
  });
});

describe("attachTunnel security & validation constraints", () => {
  it("asserts attachTunnel source contains no Math.random", () => {
    // Given the source code of attachTunnel.ts
    const filePath = resolve(__dirname, "attachTunnel.ts");
    const source = readFileSync(filePath, "utf-8");

    // Then Math.random is not used anywhere
    expect(source.includes("Math.random")).toBe(false);
  });

  it("fails closed on missing or invalid keys, missing machineId or sessionId", async () => {
    // When given missing keys
    await expect(
      openAccountTunnel({
        socketUrl: "wss://relay/tunnel/opaque/sess-123",
        machineId: "m1",
        enrollmentEpoch: "1",
        machineAttachPublicKey: "",
        localKeyPair: { publicKey: "", privateKey: "" },
        sessionId: "sess-123",
      }),
    ).rejects.toThrow(/ATTACH_KEY_INVALID/);

    // When given invalid length key
    await expect(
      openAccountTunnel({
        socketUrl: "wss://relay/tunnel/opaque/sess-123",
        machineId: "m1",
        enrollmentEpoch: "1",
        machineAttachPublicKey: btoa("short-key"),
        localKeyPair: { publicKey: btoa("short-key"), privateKey: btoa("short-key") },
        sessionId: "sess-123",
      }),
    ).rejects.toThrow(/INVALID_KEY_LENGTH/);

    // When given missing machineId
    const dummyKey = btoa(String.fromCharCode(...new Array(32).fill(1)));
    await expect(
      openAccountTunnel({
        socketUrl: "wss://relay/tunnel/opaque/sess-123",
        machineId: "",
        enrollmentEpoch: "1",
        machineAttachPublicKey: dummyKey,
        localKeyPair: { publicKey: dummyKey, privateKey: dummyKey },
        sessionId: "sess-123",
      }),
    ).rejects.toThrow(/MISSING_MACHINE_ID/);

    // When given missing sessionId
    await expect(
      openAccountTunnel({
        socketUrl: "wss://relay/tunnel/opaque/sess-123",
        machineId: "m1",
        enrollmentEpoch: "1",
        machineAttachPublicKey: dummyKey,
        localKeyPair: { publicKey: dummyKey, privateKey: dummyKey },
        sessionId: "",
      }),
    ).rejects.toThrow(/MISSING_SESSION_ID/);
  });
});

const NOISE_PROTOCOL_NAME = new TextEncoder().encode("Noise_IK_25519_ChaChaPoly_BLAKE2s");

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

function containsSubarray(haystack: Uint8Array, needle: Uint8Array): boolean {
  if (needle.length === 0 || haystack.length < needle.length) return false;
  for (let i = 0; i <= haystack.length - needle.length; i++) {
    let match = true;
    for (let j = 0; j < needle.length; j++) {
      if (haystack[i + j] !== needle[j]) {
        match = false;
        break;
      }
    }
    if (match) return true;
  }
  return false;
}

function unmaskClientFrame(frame: Uint8Array): Uint8Array {
  const hasMask = (frame[1] & 0x80) !== 0;
  let payloadLen = frame[1] & 0x7f;
  let offset = 2;
  if (payloadLen === 126) {
    offset = 4;
    const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
    payloadLen = view.getUint16(2, false);
  } else if (payloadLen === 127) {
    offset = 10;
    const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
    payloadLen = Number(view.getBigUint64(2, false));
  }
  if (!hasMask) {
    return frame.subarray(offset, offset + payloadLen);
  }
  const mask = frame.subarray(offset, offset + 4);
  offset += 4;
  const rawPayload = frame.subarray(offset, offset + payloadLen);
  const unmasked = new Uint8Array(rawPayload.length);
  for (let i = 0; i < rawPayload.length; i++) {
    unmasked[i] = rawPayload[i] ^ mask[i % 4];
  }
  return unmasked;
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
    authorize: (initiatorPub: Uint8Array) => boolean,
  ): Promise<{ msg2: Uint8Array; initiatorStaticPub: Uint8Array }> {
    if (msg1.length !== 96) {
      throw new Error(`RESPONDER_BAD_MSG1_LEN: expected 96, got ${msg1.length}`);
    }

    const hkdf = this.primitives.hkdf!.bind(this.primitives);

    let h: Uint8Array = await this.primitives.hash(NOISE_PROTOCOL_NAME);
    let ck: Uint8Array = new Uint8Array(h);

    h = await this.primitives.hash(concatAll([h, this.prologue]));
    h = await this.primitives.hash(concatAll([h, this.responderPub]));

    const e_init = msg1.subarray(0, 32);
    h = await this.primitives.hash(concatAll([h, e_init]));

    const dh_es = await this.primitives.x25519(this.responderPriv, e_init);
    const [ck1, k1] = await hkdf(ck, dh_es);
    ck = ck1;
    let n1 = 0;

    const ct_s = msg1.subarray(32, 80);
    const initiatorStaticPub = await this.primitives.aeadDecrypt(k1, n1++, h, ct_s);
    h = await this.primitives.hash(concatAll([h, ct_s]));

    if (!authorize(initiatorStaticPub)) {
      throw new Error("UNAUTHORIZED_INITIATOR_KEY");
    }

    const dh_ss = await this.primitives.x25519(this.responderPriv, initiatorStaticPub);
    const [ck2, k2] = await hkdf(ck, dh_ss);
    ck = ck2;
    let n2 = 0;

    const ct_p = msg1.subarray(80, 96);
    await this.primitives.aeadDecrypt(k2, n2++, h, ct_p);
    h = await this.primitives.hash(concatAll([h, ct_p]));

    const re_resp = await this.primitives.generateEphemeralKey();
    h = await this.primitives.hash(concatAll([h, re_resp.publicKey]));

    const dh_ee = await this.primitives.x25519(re_resp.privateKey, e_init);
    const [ck3] = await hkdf(ck, dh_ee);
    ck = ck3;

    const dh_se = await this.primitives.x25519(re_resp.privateKey, initiatorStaticPub);
    const [ck4, k4] = await hkdf(ck, dh_se);
    ck = ck4;
    let n4 = 0;

    const ct_p2 = await this.primitives.aeadEncrypt(k4, n4++, h, new Uint8Array(0));
    h = await this.primitives.hash(concatAll([h, ct_p2]));

    const msg2 = concatAll([re_resp.publicKey, ct_p2]);

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

class MockEncryptedAttachWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;

  public url: string;
  public readyState: number = MockEncryptedAttachWebSocket.CONNECTING;
  public binaryType: string = "arraybuffer";
  public onopen: (() => void) | null = null;
  public onclose: ((event?: unknown) => void) | null = null;
  public onmessage: ((event: { data: ArrayBuffer | string }) => void) | null = null;
  public onerror: ((event?: unknown) => void) | null = null;

  public sentRawPayloads: Uint8Array[] = [];
  public onSend: ((data: Uint8Array) => Promise<void> | void) | null = null;

  constructor(url: string) {
    this.url = url;
    queueMicrotask(() => {
      if (this.readyState === MockEncryptedAttachWebSocket.CONNECTING) {
        this.readyState = MockEncryptedAttachWebSocket.OPEN;
        this.onopen?.();
      }
    });
  }

  send(data: string | Uint8Array | ArrayBuffer) {
    const raw =
      typeof data === "string"
        ? new TextEncoder().encode(data)
        : data instanceof Uint8Array
        ? new Uint8Array(data)
        : new Uint8Array(data);
    this.sentRawPayloads.push(raw);
    if (this.onSend) {
      void this.onSend(raw);
    }
  }

  pushServerMessage(bytes: Uint8Array) {
    const copy = new Uint8Array(bytes.length);
    copy.set(bytes);
    this.onmessage?.({ data: copy.buffer });
  }

  close() {
    this.readyState = MockEncryptedAttachWebSocket.CLOSED;
    this.onclose?.({ code: 1000, reason: "normal", wasClean: true });
  }
}

describe("attachTunnel encrypted channel (P19 / F3)", () => {
  it("drives full Noise handshake, transmits PHONE_INPUT_MARKER exclusively as ciphertext over mock WebSocket, and recovers marker on peer side", async () => {
    const PHONE_INPUT_MARKER = "PHONE_INPUT_MARKER_9981";
    const markerBytes = new TextEncoder().encode(PHONE_INPUT_MARKER);

    // 1. Obtain real local attach keypair (initiator) and machine attach public key (responder)
    const primitives = createNoisePrimitives();
    const localKeyPair = await getOrCreateAttachKey();
    const responderKeys = await primitives.generateEphemeralKey();
    const machineAttachPublicKey = bytesToBase64(responderKeys.publicKey);

    const machineId = "mach-phone-encrypted-1";
    const sessionId = "sess-phone-encrypted-1";
    const enrollmentEpoch = "1";
    const prologue = attachPrologue(machineId, sessionId, enrollmentEpoch);

    const responder = new TestNoiseResponder(
      primitives,
      responderKeys.privateKey,
      responderKeys.publicKey,
      prologue,
    );

    let activeSocket: MockEncryptedAttachWebSocket | null = null;
    let handshakeCompleted = false;
    let upgradeCompleted = false;
    const decryptedPayloadsFromInitiator: Uint8Array[] = [];

    vi.stubGlobal(
      "WebSocket",
      class extends MockEncryptedAttachWebSocket {
        constructor(url: string) {
          super(url);
          activeSocket = this;
          this.onSend = async (raw: Uint8Array) => {
            const { messages } = decodeFrames(raw);
            for (const msg of messages) {
              if (!handshakeCompleted) {
                // Handshake message 1 from initiator (96 bytes)
                const { msg2, initiatorStaticPub } = await responder.accept(msg, () => true);
                expect(bytesToBase64(initiatorStaticPub)).toBe(localKeyPair.publicKey);
                handshakeCompleted = true;
                const frame2 = encodeFrame(msg2);
                this.pushServerMessage(frame2);
              } else if (!upgradeCompleted) {
                // Encrypted HTTP Upgrade request
                const decryptedHttpReq = await responder.decrypt(msg);
                decryptedPayloadsFromInitiator.push(decryptedHttpReq);
                const reqText = new TextDecoder().decode(decryptedHttpReq);
                expect(reqText).toContain("Upgrade: websocket");
                const secKeyMatch = reqText.match(/Sec-WebSocket-Key:\s*([^\r\n]+)/i);
                expect(secKeyMatch).not.toBeNull();
                const secKey = secKeyMatch![1].trim();
                const acceptKey = await computeAcceptKey(secKey);
                upgradeCompleted = true;
                const resText = `HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${acceptKey}\r\n\r\n`;
                const ct101 = await responder.encrypt(new TextEncoder().encode(resText));
                this.pushServerMessage(encodeFrame(ct101));
              } else {
                // Transport messages (encrypted WebSocket frame or direct stream write)
                const decryptedMsg = await responder.decrypt(msg);
                decryptedPayloadsFromInitiator.push(decryptedMsg);
              }
            }
          };
        }
      },
    );

    try {
      // 2. Open account tunnel over mock WebSocket
      const tunnel = await openAccountTunnel({
        socketUrl: `wss://relay.example.com/tunnel/opaque/${sessionId}`,
        machineId,
        enrollmentEpoch,
        machineAttachPublicKey,
        localKeyPair,
        sessionId,
      });

      expect(handshakeCompleted).toBe(true);
      expect(activeSocket).not.toBeNull();

      // 3. Open WebSocket on transport with marker in path, and send marker via socket and stream
      const tunnelWs = await tunnel.transport.openWebSocket(
        `/api/v1/terminal/${sessionId}?marker=${encodeURIComponent(PHONE_INPUT_MARKER)}`,
      );
      expect(upgradeCompleted).toBe(true);

      // Write marker through returned WebSocket
      tunnelWs.send(PHONE_INPUT_MARKER);

      // Write marker through returned stream
      await tunnel.stream!.write(markerBytes);

      // Wait until all decrypted frames are processed by responder
      await vi.waitFor(() => {
        expect(decryptedPayloadsFromInitiator.length).toBeGreaterThanOrEqual(3);
      });

      // 4. Assert: NONE of the raw frames sent over the mock WebSocket contain PHONE_INPUT_MARKER in plaintext
      expect(activeSocket!.sentRawPayloads.length).toBeGreaterThan(0);
      for (const rawPayload of activeSocket!.sentRawPayloads) {
        expect(containsSubarray(rawPayload, markerBytes)).toBe(false);
        const decoded = new TextDecoder("utf-8", { fatal: false }).decode(rawPayload);
        expect(decoded.includes(PHONE_INPUT_MARKER)).toBe(false);
      }

      // 5. Assert: PHONE_INPUT_MARKER IS recoverable after decrypting on the peer (responder) side
      // Identify payloads by CONTENT, never by index, ensuring robustness to arrival order:
      const isHttpUpgrade = (payload: Uint8Array): boolean => {
        const text = new TextDecoder("utf-8", { fatal: false }).decode(payload);
        return text.startsWith("GET ") && text.includes("Upgrade: websocket");
      };

      const isMaskedWsFrame = (payload: Uint8Array): boolean => {
        if (payload.length < 6) return false;
        const b0 = payload[0];
        const opcode = b0 & 0x0f;
        const isTextOrBinary = opcode === 0x1 || opcode === 0x2;
        const isMasked = (payload[1] & 0x80) !== 0;
        return isTextOrBinary && isMasked;
      };

      const isRawStreamPayload = (payload: Uint8Array): boolean => {
        if (payload.length !== markerBytes.length) return false;
        for (let i = 0; i < payload.length; i++) {
          if (payload[i] !== markerBytes[i]) return false;
        }
        return true;
      };

      const httpUpgradePayloads = decryptedPayloadsFromInitiator.filter(isHttpUpgrade);
      const wsFramePayloads = decryptedPayloadsFromInitiator.filter(isMaskedWsFrame);
      const rawStreamPayloads = decryptedPayloadsFromInitiator.filter(isRawStreamPayload);

      expect(httpUpgradePayloads).toHaveLength(1);
      expect(wsFramePayloads).toHaveLength(1);
      expect(rawStreamPayloads).toHaveLength(1);

      // 5a. Recover from HTTP Upgrade request
      const decryptedHttpText = new TextDecoder().decode(httpUpgradePayloads[0]);
      expect(decryptedHttpText).toContain(PHONE_INPUT_MARKER);

      // 5b. Recover from WebSocket frame (unmask RFC6455 frame)
      const unmaskedWsPayload = unmaskClientFrame(wsFramePayloads[0]);
      expect(new TextDecoder().decode(unmaskedWsPayload)).toBe(PHONE_INPUT_MARKER);

      // 5c. Recover from direct stream write
      expect(new TextDecoder().decode(rawStreamPayloads[0])).toBe(PHONE_INPUT_MARKER);

      tunnel.close();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
