import {
  encodeFrame,
  decodeFrames,
  attachPrologue,
  createNoiseInitiator,
  MAX_ATTACH_FRAME,
  type NoiseInitiatorConfig,
} from "./attachFraming";
import { createNoisePrimitives } from "./noisePrimitives";

export const WS_CONNECTING = 0;
export const WS_OPEN = 1;
export const WS_CLOSING = 2;
export const WS_CLOSED = 3;

const CRLF = new Uint8Array([0x0d, 0x0a]);
const CRLF_CRLF = new Uint8Array([0x0d, 0x0a, 0x0d, 0x0a]);
const WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/**
 * Hard cap on one inbound tunnel WebSocket frame and on a reassembled fragmented message.
 * The attach layer bounds each encrypted chunk to MAX_ATTACH_FRAME, but a gateway WebSocket
 * message spans many chunks: the largest gateway routes accept 4 MiB messages and terminal
 * replays carry a 512 KiB ring, so 16 MiB leaves headroom while refusing the 4 GiB the
 * 64-bit length field allows.
 */
export const MAX_TUNNEL_WS_MESSAGE_BYTES = 16 * 1024 * 1024;
/** RFC 6455 5.5: control frames carry at most 125 payload bytes and are never fragmented. */
export const MAX_WS_CONTROL_PAYLOAD = 125;
/** Per-request deadline for the tunnel HTTP shim (headers and body). */
export const DEFAULT_TUNNEL_REQUEST_TIMEOUT_MS = 30_000;
/** Cap on the response status line + headers (and on a chunk-size line). */
export const MAX_TUNNEL_RESPONSE_HEADER_BYTES = 64 * 1024;
/** Cap on a response body, whatever its framing. */
export const MAX_TUNNEL_RESPONSE_BODY_BYTES = 16 * 1024 * 1024;

const WS_CLOSE_PROTOCOL_ERROR = 1002;
const WS_CLOSE_MESSAGE_TOO_BIG = 1009;

export interface TunnelByteStream {
  write(bytes: Uint8Array): Promise<void>;
  read(): Promise<Uint8Array>;
  close(): void;
  /** True once the underlying socket is gone; a closed stream never reopens. */
  readonly isClosed?: boolean;
}

export interface TunnelResponse {
  status: number;
  headers: Record<string, string>;
  body: Uint8Array;
}

export interface FetchLikeInit {
  method?: string;
  headers?: Record<string, string>;
  body?: Uint8Array | string;
  /**
   * Aborts the request. HTTP/1.1 cannot cancel one response on a shared stream, so an abort
   * after the request was written rejects the caller while the response is drained and dropped.
   */
  signal?: AbortSignal;
  /** Deadline in ms; defaults to DEFAULT_TUNNEL_REQUEST_TIMEOUT_MS. */
  timeoutMs?: number;
}

export interface TunnelMessageEvent {
  data: Uint8Array | string;
}

export interface TunnelCloseEvent {
  code: number;
  reason: string;
  wasClean: boolean;
}

/** WebSocket close code (1008 Policy Violation) the relay uses to refuse an attach. */
export const ATTACH_REFUSAL_CLOSE_CODE = 1008;

/** Exact close reasons the relay sends with 1008; mirrors the account-service codes. */
export type AttachTunnelRefusalCode = "REMOTE_SUSPENDED" | "PLAN_LIMIT_REACHED";

/**
 * Maps a pre-handshake close to a structured plan refusal. Only close code 1008 with a
 * reason exactly equal to a contract code matches; any other close stays a premature close.
 */
export function attachTunnelRefusalCode(
  event: Partial<TunnelCloseEvent> | null | undefined,
): AttachTunnelRefusalCode | null {
  if (!event || event.code !== ATTACH_REFUSAL_CLOSE_CODE) return null;
  if (event.reason === "REMOTE_SUSPENDED" || event.reason === "PLAN_LIMIT_REACHED") {
    return event.reason;
  }
  return null;
}

/** Rejection for a relay refusal; `code` is the structured contract code callers branch on. */
export class AttachTunnelRefusedError extends Error {
  readonly code: AttachTunnelRefusalCode;

  constructor(code: AttachTunnelRefusalCode) {
    super(`ATTACH_TUNNEL_REFUSED: ${code}`);
    this.name = "AttachTunnelRefusedError";
    this.code = code;
  }
}

export interface TunnelErrorEvent {
  error?: unknown;
  message?: string;
}

/** Structured tunnel failure: callers branch on `code` (and `status` for an HTTP upgrade refusal). */
export class TunnelError extends Error {
  readonly code: string;
  readonly status?: number;
  constructor(code: string, message: string, status?: number) {
    super(`${code}: ${message}`);
    this.name = "TunnelError";
    this.code = code;
    this.status = status;
  }
}

export interface TunnelWebSocket {
  send(data: Uint8Array | string): void;
  close(code?: number, reason?: string): void;
  readyState: number;
  binaryType: string;
  onopen: ((event: Event) => void) | null;
  onmessage: ((event: TunnelMessageEvent) => void) | null;
  onerror: ((event: TunnelErrorEvent) => void) | null;
  onclose: ((event: TunnelCloseEvent) => void) | null;
}

export interface TunnelTransport {
  fetchLike(
    pathAndQuery: string,
    init?: FetchLikeInit,
  ): Promise<TunnelResponse>;
  openWebSocket(
    pathAndQuery: string,
    headers?: Record<string, string>,
  ): Promise<TunnelWebSocket>;
  close(): void;
  /** True once the transport or its stream has closed; callers must reacquire, not reuse. */
  readonly isClosed?: boolean;
}

export interface OpenAccountTunnelOpts {
  socketUrl: string;
  machineId: string;
  enrollmentEpoch: string;
  machineAttachPublicKey: string;
  localKeyPair: {
    publicKey: string;
    privateKey: string;
  };
  /**
   * Session ID for the attach channel prologue.
   *
   * REQUIRED: For phone / browser connections through the relay opaque tunnel
   * (wss://<relay>/tunnel/opaque/{sessionId}), the session ID is allocated by the relay
   * and must match the session ID used by the daemon's paired responder half in
   * its prologue: `ferryx-attach-v1:{machineId}:{sessionId}:{enrollmentEpoch}`.
   * (The literal string "direct" was only used for direct /api/v1/attach machine connections).
   */
  sessionId: string;
}

function toCleanBytes(arr: Uint8Array): Uint8Array {
  const out = new Uint8Array(arr.length);
  out.set(arr);
  return out;
}

function sliceBytes(arr: Uint8Array, start: number, end?: number): Uint8Array {
  const actualEnd = end !== undefined ? end : arr.length;
  const len = Math.max(0, actualEnd - start);
  const out = new Uint8Array(len);
  out.set(arr.subarray(start, actualEnd));
  return out;
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

function findSubarray(haystack: Uint8Array, needle: Uint8Array, fromIndex = 0): number {
  if (needle.length === 0) return 0;
  if (haystack.length < needle.length) return -1;
  const max = haystack.length - needle.length;
  for (let i = fromIndex; i <= max; i++) {
    let match = true;
    for (let j = 0; j < needle.length; j++) {
      if (haystack[i + j] !== needle[j]) {
        match = false;
        break;
      }
    }
    if (match) return i;
  }
  return -1;
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

function base64ToBytes(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

function getRandomBytes(count: number): Uint8Array {
  const buf = new Uint8Array(count);
  crypto.getRandomValues(buf);
  return buf;
}

/** A receive-side violation that fails the WebSocket with a specific close code. */
class WsFailure extends Error {
  readonly closeCode: number;
  readonly closeReason: string;
  constructor(closeCode: number, closeReason: string, message: string) {
    super(message);
    this.closeCode = closeCode;
    this.closeReason = closeReason;
  }
}

function encodeClosePayload(code: number, reason?: string): Uint8Array {
  const reasonBytes = reason ? new TextEncoder().encode(reason) : new Uint8Array(0);
  const payload = new Uint8Array(2 + reasonBytes.length);
  const view = new DataView(payload.buffer, payload.byteOffset, 2);
  view.setUint16(0, code, false);
  payload.set(reasonBytes, 2);
  return payload;
}

function encodeClientFrame(opcode: number, payload: Uint8Array): Uint8Array {
  const len = payload.length;
  let headerSize = 2;
  let lenType = 0;

  if (len < 126) {
    headerSize += 4;
    lenType = 0;
  } else if (len <= 65535) {
    headerSize += 2 + 4;
    lenType = 1;
  } else {
    headerSize += 8 + 4;
    lenType = 2;
  }

  const frame = new Uint8Array(headerSize + len);
  frame[0] = 0x80 | (opcode & 0x0f);

  let offset = 2;
  if (lenType === 0) {
    frame[1] = 0x80 | len;
  } else if (lenType === 1) {
    frame[1] = 0x80 | 126;
    const view = new DataView(frame.buffer, frame.byteOffset, headerSize);
    view.setUint16(2, len, false);
    offset = 4;
  } else {
    frame[1] = 0x80 | 127;
    const view = new DataView(frame.buffer, frame.byteOffset, headerSize);
    view.setBigUint64(2, BigInt(len), false);
    offset = 10;
  }

  const mask = getRandomBytes(4);
  frame.set(mask, offset);
  offset += 4;

  for (let i = 0; i < len; i++) {
    frame[offset + i] = payload[i] ^ mask[i % 4];
  }

  return frame;
}

/**
 * BYTE STREAM DISPATCH & OWNERSHIP ARCHITECTURE:
 *
 * Both HTTP/1.1 requests (`fetchLike`) and WebSocket connections (`openWebSocket`) share
 * a single `TunnelByteStream` connected to the remote gateway.
 *
 * 1. Request Serialization:
 *    HTTP/1.1 requests are strictly serialized through an internal promise queue so that
 *    never more than one request or handshake is in flight over the byte stream.
 *
 * 2. Response Buffer Splitting:
 *    When reading responses, incoming chunks from `stream.read()` are buffered into an internal
 *    `readBuffer`. The parser consumes only the bytes required for the current response
 *    (headers + body via Content-Length or Chunked transfer-encoding), leaving any trailing
 *    bytes in `readBuffer` for the next sequential request.
 *
 * 3. 101 Switching Protocols Stream Handover:
 *    When `openWebSocket` initiates an upgrade request, it verifies the HTTP 101 response
 *    and Sec-WebSocket-Accept header. Once validated, the byte stream belongs PERMANENTLY
 *    and EXCLUSIVELY to the `TunnelWebSocket` until it closes. Any residual bytes remaining
 *    in `readBuffer` after the 101 headers are passed to the WebSocket receiver loop as the
 *    initial incoming frame bytes. Any subsequent calls to `fetchLike` or `openWebSocket`
 *    fail immediately with `STREAM_UPGRADED`.
 */
export function createTunnelTransport(stream: TunnelByteStream): TunnelTransport {
  let readBuffer: Uint8Array = new Uint8Array(0);
  let isUpgraded = false;
  let isClosed = false;

  let requestQueue: Promise<unknown> = Promise.resolve();
  let markClosed!: () => void;
  /** Resolves once the transport is closed, so the queue never waits on a dead stream. */
  const closedSignal = new Promise<void>((resolve) => {
    markClosed = resolve;
  });

  /** Closes the shared stream: after a timeout, abort, parse failure or cap violation its framing is unknown. */
  function failTransport() {
    if (isClosed) return;
    isClosed = true;
    markClosed();
    stream.close();
  }

  function tooLarge(message: string): Error {
    failTransport();
    return new Error(message);
  }

  /**
   * Queues one request on the shared stream. Its deadline and abort listener start at enqueue,
   * so time spent behind other requests counts: a request still waiting when either fires is
   * rejected and skipped (never sent). An abort of a request already on the wire rejects the
   * caller only: its response is still read and discarded so the shared stream stays in sync.
   * Only the deadline (or a parse failure) closes the stream, since its framing is then unknown.
   */
  function schedule<T>(
    work: () => Promise<T>,
    timeoutMs: number,
    signal?: AbortSignal,
  ): Promise<T> {
    if (signal?.aborted) {
      return Promise.reject(new TunnelError("HTTP_ABORTED", "request aborted before it was sent"));
    }
    return new Promise<T>((resolve, reject) => {
      let settled = false;
      let started = false;
      let workDone = false;
      let timer: ReturnType<typeof setTimeout> | undefined;
      const finish = (complete: () => void) => {
        if (settled) return;
        settled = true;
        signal?.removeEventListener("abort", onAbort);
        // An in-flight request keeps its deadline until the response has drained.
        if (!started || workDone) clearTimeout(timer);
        complete();
      };
      const onAbort = () =>
        finish(() =>
          reject(
            new TunnelError(
              "HTTP_ABORTED",
              started ? "request aborted; its response is discarded" : "request aborted before it was sent",
            ),
          ),
        );
      timer = setTimeout(() => {
        if (started && !workDone) failTransport();
        finish(() =>
          reject(
            new TunnelError(
              "HTTP_TIMEOUT",
              started
                ? `no complete response within ${timeoutMs}ms; tunnel stream closed`
                : `request waited ${timeoutMs}ms behind earlier requests`,
            ),
          ),
        );
      }, timeoutMs);
      signal?.addEventListener("abort", onAbort, { once: true });
      const run = async (): Promise<void> => {
        if (settled) return;
        if (isUpgraded) {
          finish(() => reject(new TunnelError("STREAM_UPGRADED", "stream has been handed over to WebSocket")));
          return;
        }
        if (isClosed) {
          finish(() => reject(new TunnelError("STREAM_CLOSED", "transport has been closed")));
          return;
        }
        started = true;
        const outcome = work();
        outcome.then(
          (value) => {
            workDone = true;
            clearTimeout(timer);
            finish(() => resolve(value));
          },
          (err) => {
            workDone = true;
            clearTimeout(timer);
            // Any failure mid-response leaves unknown bytes in the buffer; never reuse the stream.
            failTransport();
            finish(() => reject(err instanceof Error ? err : new Error(String(err))));
          },
        );
        await Promise.race([outcome.then(() => {}, () => {}), closedSignal]);
      };
      requestQueue = requestQueue.then(run, run);
    });
  }

  async function readChunk(): Promise<Uint8Array> {
    const chunk = await stream.read();
    return chunk;
  }

  /** Buffers until readBuffer holds at least minLength bytes; joins chunks once. */
  async function fillTo(minLength: number, onEof: (have: number) => Error): Promise<void> {
    if (readBuffer.length >= minLength) return;
    const parts: Uint8Array[] = [readBuffer];
    let total = readBuffer.length;
    try {
      while (total < minLength) {
        const chunk = await readChunk();
        if (chunk.length === 0) throw onEof(total);
        parts.push(chunk);
        total += chunk.length;
      }
    } finally {
      readBuffer = concatBytes(...parts);
    }
  }

  /** Returns the index of needle in readBuffer, reading more (at most limit bytes) as needed. */
  async function readUntil(
    needle: Uint8Array,
    limit: number,
    onEof: () => Error,
    onLimit: () => Error,
  ): Promise<number> {
    let index = findSubarray(readBuffer, needle);
    while (index === -1) {
      if (readBuffer.length > limit) throw onLimit();
      const from = Math.max(0, readBuffer.length - (needle.length - 1));
      const chunk = await readChunk();
      if (chunk.length === 0) throw onEof();
      readBuffer = concatBytes(readBuffer, chunk);
      index = findSubarray(readBuffer, needle, from);
    }
    if (index > limit) throw onLimit();
    return index;
  }

  async function parseHeaders(): Promise<{ status: number; headers: Record<string, string> }> {
    const headerEndIndex = await readUntil(
      CRLF_CRLF,
      MAX_TUNNEL_RESPONSE_HEADER_BYTES,
      () => new Error("HTTP_PREMATURE_CLOSE: stream closed while reading headers"),
      () =>
        tooLarge(`HTTP_HEADERS_TOO_LARGE: response headers exceed ${MAX_TUNNEL_RESPONSE_HEADER_BYTES} bytes`),
    );

    const headerBytes = sliceBytes(readBuffer, 0, headerEndIndex);
    readBuffer = sliceBytes(readBuffer, headerEndIndex + 4);

    const headerText = new TextDecoder("utf-8").decode(headerBytes);
    const lines = headerText.split("\r\n");
    if (lines.length === 0) {
      throw new Error("HTTP_MALFORMED_RESPONSE: empty status line");
    }

    const statusLine = lines[0];
    const statusMatch = statusLine.match(/^HTTP\/1\.[01]\s+(\d{3})(?:\s+.*)?$/i);
    if (!statusMatch) {
      throw new Error(`HTTP_MALFORMED_STATUS_LINE: ${statusLine}`);
    }
    const status = parseInt(statusMatch[1], 10);

    const headers: Record<string, string> = {};
    for (let i = 1; i < lines.length; i++) {
      const line = lines[i];
      if (!line) continue;
      const colon = line.indexOf(":");
      if (colon === -1) continue;
      const name = line.substring(0, colon).trim().toLowerCase();
      const value = line.substring(colon + 1).trim();
      headers[name] = headers[name] !== undefined ? `${headers[name]}, ${value}` : value;
    }

    return { status, headers };
  }

  async function parseBody(status: number, headers: Record<string, string>): Promise<Uint8Array> {
    if (status < 200 || status === 204 || status === 304) {
      return new Uint8Array(0);
    }

    const transferEncoding = headers["transfer-encoding"]?.toLowerCase();
    if (transferEncoding && transferEncoding.includes("chunked")) {
      const chunks: Uint8Array[] = [];
      let bodyBytes = 0;
      const textDecoder = new TextDecoder("utf-8");

      while (true) {
        const crlfIndex = await readUntil(
          CRLF,
          MAX_TUNNEL_RESPONSE_HEADER_BYTES,
          () => new Error("HTTP_PREMATURE_CLOSE: stream closed while reading chunk size"),
          () => tooLarge("HTTP_CHUNK_SIZE_LINE_TOO_LARGE: chunk-size line exceeds header cap"),
        );

        const sizeLineBytes = sliceBytes(readBuffer, 0, crlfIndex);
        readBuffer = sliceBytes(readBuffer, crlfIndex + 2);
        const sizeStr = textDecoder.decode(sizeLineBytes).split(";")[0].trim();
        const chunkSize = parseInt(sizeStr, 16);
        if (isNaN(chunkSize) || chunkSize < 0) {
          throw new Error(`HTTP_INVALID_CHUNK_SIZE: '${sizeStr}'`);
        }

        if (chunkSize === 0) {
          let trailerEnd = findSubarray(readBuffer, CRLF);
          while (trailerEnd === -1) {
            const next = await readChunk();
            if (next.length === 0) break;
            readBuffer = concatBytes(readBuffer, next);
            trailerEnd = findSubarray(readBuffer, CRLF);
          }
          if (trailerEnd !== -1) {
            if (trailerEnd === 0) {
              readBuffer = sliceBytes(readBuffer, 2);
            } else {
              let doubleCrlf = findSubarray(readBuffer, CRLF_CRLF);
              while (doubleCrlf === -1) {
                const next = await readChunk();
                if (next.length === 0) break;
                readBuffer = concatBytes(readBuffer, next);
                doubleCrlf = findSubarray(readBuffer, CRLF_CRLF);
              }
              if (doubleCrlf !== -1) {
                readBuffer = sliceBytes(readBuffer, doubleCrlf + 4);
              }
            }
          }
          break;
        }

        bodyBytes += chunkSize;
        if (bodyBytes > MAX_TUNNEL_RESPONSE_BODY_BYTES) {
          throw tooLarge(`HTTP_BODY_TOO_LARGE: chunked body exceeds ${MAX_TUNNEL_RESPONSE_BODY_BYTES} bytes`);
        }
        await fillTo(
          chunkSize + 2,
          () => new Error("HTTP_PREMATURE_CLOSE: stream closed while reading chunk data"),
        );

        const chunkData = sliceBytes(readBuffer, 0, chunkSize);
        if (readBuffer[chunkSize] !== 0x0d || readBuffer[chunkSize + 1] !== 0x0a) {
          throw new Error("HTTP_MALFORMED_CHUNK: missing CRLF after chunk data");
        }
        readBuffer = sliceBytes(readBuffer, chunkSize + 2);
        chunks.push(chunkData);
      }

      return concatBytes(...chunks);
    }

    if (headers["content-length"] !== undefined) {
      const len = parseInt(headers["content-length"], 10);
      if (isNaN(len) || len < 0) {
        throw new Error(`HTTP_INVALID_CONTENT_LENGTH: '${headers["content-length"]}'`);
      }
      if (len > MAX_TUNNEL_RESPONSE_BODY_BYTES) {
        throw tooLarge(
          `HTTP_BODY_TOO_LARGE: content-length ${len} exceeds ${MAX_TUNNEL_RESPONSE_BODY_BYTES} bytes`,
        );
      }
      await fillTo(len, (have) => new Error(`HTTP_PREMATURE_CLOSE: expected ${len} bytes, got ${have}`));
      const body = sliceBytes(readBuffer, 0, len);
      readBuffer = sliceBytes(readBuffer, len);
      return body;
    }

    if (headers["connection"]?.toLowerCase() === "close") {
      const parts: Uint8Array[] = [readBuffer];
      let total = readBuffer.length;
      while (true) {
        if (total > MAX_TUNNEL_RESPONSE_BODY_BYTES) {
          throw tooLarge(`HTTP_BODY_TOO_LARGE: body exceeds ${MAX_TUNNEL_RESPONSE_BODY_BYTES} bytes`);
        }
        const next = await readChunk();
        if (next.length === 0) break;
        parts.push(next);
        total += next.length;
      }
      const body = concatBytes(...parts);
      if (body.length > MAX_TUNNEL_RESPONSE_BODY_BYTES) {
        throw tooLarge(`HTTP_BODY_TOO_LARGE: body exceeds ${MAX_TUNNEL_RESPONSE_BODY_BYTES} bytes`);
      }
      readBuffer = new Uint8Array(0);
      return body;
    }

    // Neither length nor chunked framing: the body bytes would be parsed as the next response.
    throw new Error("HTTP_DESYNC: response has neither content-length nor chunked framing");
  }

  async function doFetch(
    pathAndQuery: string,
    init?: FetchLikeInit,
  ): Promise<TunnelResponse> {
    const method = (init?.method || "GET").toUpperCase();
    const callerHeaders = init?.headers || {};

    let hasHost = false;
    let hasConnection = false;
    let hasContentLength = false;
    for (const k of Object.keys(callerHeaders)) {
      const lk = k.toLowerCase();
      if (lk === "host") hasHost = true;
      if (lk === "connection") hasConnection = true;
      if (lk === "content-length") hasContentLength = true;
    }

    let bodyBytes: Uint8Array | null = null;
    if (init?.body !== undefined) {
      if (typeof init.body === "string") {
        bodyBytes = new TextEncoder().encode(init.body);
      } else {
        bodyBytes = init.body;
      }
    }

    let reqStr = `${method} ${pathAndQuery} HTTP/1.1\r\n`;
    if (!hasHost) {
      reqStr += "Host: localhost\r\n";
    }
    if (!hasConnection) {
      reqStr += "Connection: keep-alive\r\n";
    }

    for (const [k, v] of Object.entries(callerHeaders)) {
      if (bodyBytes !== null && k.toLowerCase() === "content-length") continue;
      reqStr += `${k}: ${v}\r\n`;
    }

    if (bodyBytes !== null && !hasContentLength) {
      reqStr += `Content-Length: ${bodyBytes.length}\r\n`;
    } else if (bodyBytes !== null && hasContentLength) {
      reqStr += `Content-Length: ${bodyBytes.length}\r\n`;
    }

    reqStr += "\r\n";
    const headerBytes = new TextEncoder().encode(reqStr);
    const fullRequest = bodyBytes ? concatBytes(headerBytes, bodyBytes) : headerBytes;

    await stream.write(fullRequest);

    const { status, headers } = await parseHeaders();
    const body = await parseBody(status, headers);
    return { status, headers, body };
  }

  async function doOpenWebSocket(
    pathAndQuery: string,
    headers?: Record<string, string>,
  ): Promise<TunnelWebSocket> {
    const keyBytes = getRandomBytes(16);
    const secWebSocketKey = bytesToBase64(keyBytes);

    let hasHost = false;
    const callerHeaders = headers || {};
    for (const k of Object.keys(callerHeaders)) {
      if (k.toLowerCase() === "host") hasHost = true;
    }

    let reqStr = `GET ${pathAndQuery} HTTP/1.1\r\n`;
    if (!hasHost) {
      reqStr += "Host: localhost\r\n";
    }
    reqStr += "Upgrade: websocket\r\n";
    reqStr += "Connection: Upgrade\r\n";
    reqStr += `Sec-WebSocket-Key: ${secWebSocketKey}\r\n`;
    reqStr += "Sec-WebSocket-Version: 13\r\n";

    for (const [k, v] of Object.entries(callerHeaders)) {
      const lk = k.toLowerCase();
      if (
        lk === "host" ||
        lk === "upgrade" ||
        lk === "connection" ||
        lk === "sec-websocket-key" ||
        lk === "sec-websocket-version"
      ) {
        continue;
      }
      reqStr += `${k}: ${v}\r\n`;
    }
    reqStr += "\r\n";

    await stream.write(new TextEncoder().encode(reqStr));

    const { status, headers: resHeaders } = await parseHeaders();
    if (status !== 101) {
      throw new TunnelError("WEBSOCKET_UPGRADE_FAILED", `server returned status ${status}`, status);
    }

    const acceptExpectedRaw = new TextEncoder().encode(secWebSocketKey + WS_GUID);
    const digest = await crypto.subtle.digest("SHA-1", acceptExpectedRaw);
    const expectedAccept = bytesToBase64(new Uint8Array(digest));

    const receivedAccept = resHeaders["sec-websocket-accept"];
    if (receivedAccept !== expectedAccept) {
      throw new Error(
        `WEBSOCKET_ACCEPT_MISMATCH: expected accept header '${expectedAccept}', got '${receivedAccept}'`,
      );
    }

    isUpgraded = true;
    const initialBuffer = readBuffer;
    readBuffer = new Uint8Array(0);

    return createTunnelWebSocketInstance(stream, initialBuffer);
  }

  return {
    fetchLike(pathAndQuery: string, init?: FetchLikeInit): Promise<TunnelResponse> {
      return schedule(
        () => doFetch(pathAndQuery, init),
        init?.timeoutMs ?? DEFAULT_TUNNEL_REQUEST_TIMEOUT_MS,
        init?.signal,
      );
    },
    openWebSocket(pathAndQuery: string, headers?: Record<string, string>): Promise<TunnelWebSocket> {
      return schedule(() => doOpenWebSocket(pathAndQuery, headers), DEFAULT_TUNNEL_REQUEST_TIMEOUT_MS);
    },
    close(): void {
      isClosed = true;
      markClosed();
      stream.close();
    },
    get isClosed(): boolean {
      return isClosed || stream.isClosed === true;
    },
  };
}

function createTunnelWebSocketInstance(
  stream: TunnelByteStream,
  initialBuffer: Uint8Array,
): TunnelWebSocket {
  let readyState = WS_OPEN;
  let binaryType = "arraybuffer";
  let buffer: Uint8Array = initialBuffer;
  let isClosing = false;

  let onopenCallback: ((event: Event) => void) | null = null;
  let onmessageCallback: ((event: TunnelMessageEvent) => void) | null = null;
  let onerrorCallback: ((event: TunnelErrorEvent) => void) | null = null;
  let oncloseCallback: ((event: TunnelCloseEvent) => void) | null = null;
  // Messages that arrive before the caller assigns onmessage (frames glued to the 101, or read
  // while the socket is still being handed over); flushed in order on assignment.
  let undelivered: TunnelMessageEvent[] = [];

  let writeQueue: Promise<void> = Promise.resolve();

  function enqueueWrite(frame: Uint8Array): Promise<void> {
    const next = writeQueue.then(
      () => stream.write(frame),
      () => stream.write(frame),
    ).catch((err) => {
      handleError(err);
    });
    writeQueue = next;
    return next;
  }

  function handleError(err: unknown) {
    if (onerrorCallback) {
      onerrorCallback({ error: err, message: err instanceof Error ? err.message : String(err) });
    }
  }

  function handleClose(code: number, reason: string, wasClean: boolean) {
    if (readyState === WS_CLOSED) return;
    readyState = WS_CLOSED;
    stream.close();
    if (oncloseCallback) {
      oncloseCallback({ code, reason, wasClean });
    }
  }

  async function sendFrame(opcode: number, payload: Uint8Array): Promise<void> {
    const frame = encodeClientFrame(opcode, payload);
    return enqueueWrite(frame);
  }

  function deliverMessage(opcode: number, payload: Uint8Array) {
    const event: TunnelMessageEvent = { data: opcode === 0x1 ? new TextDecoder("utf-8").decode(payload) : payload };
    if (!onmessageCallback || undelivered.length > 0) {
      undelivered.push(event);
      return;
    }
    onmessageCallback(event);
  }

  function flushUndelivered() {
    while (onmessageCallback && undelivered.length > 0) {
      const event = undelivered.shift()!;
      onmessageCallback(event);
    }
  }

  async function runReceiveLoop() {
    let fragmentedOpcode = 0;
    let fragmentedPayloads: Uint8Array[] = [];
    let fragmentedBytes = 0;

    try {
      while (readyState !== WS_CLOSED) {
        while (buffer.length < 2) {
          const chunk = await stream.read();
          if (chunk.length === 0) {
            handleClose(1006, "Abnormal closure", false);
            return;
          }
          buffer = concatBytes(buffer, chunk);
        }

        const byte0 = buffer[0];
        const byte1 = buffer[1];
        const fin = (byte0 & 0x80) !== 0;
        const opcode = byte0 & 0x0f;
        const hasMask = (byte1 & 0x80) !== 0;
        let payloadLen = byte1 & 0x7f;
        let headerSize = 2;

        if ((byte0 & 0x70) !== 0) {
          throw new WsFailure(
            WS_CLOSE_PROTOCOL_ERROR,
            "protocol error",
            "PROTOCOL_ERROR: nonzero RSV bits without a negotiated extension",
          );
        }
        if ((opcode & 0x08) !== 0 && (!fin || payloadLen > MAX_WS_CONTROL_PAYLOAD)) {
          throw new WsFailure(
            WS_CLOSE_PROTOCOL_ERROR,
            "protocol error",
            "PROTOCOL_ERROR: control frame is fragmented or longer than 125 bytes",
          );
        }

        if (payloadLen === 126) {
          headerSize += 2;
        } else if (payloadLen === 127) {
          headerSize += 8;
        }
        if (hasMask) {
          headerSize += 4;
        }

        while (buffer.length < headerSize) {
          const chunk = await stream.read();
          if (chunk.length === 0) {
            handleClose(1006, "Abnormal closure", false);
            return;
          }
          buffer = concatBytes(buffer, chunk);
        }

        let offset = 2;
        if (payloadLen === 126) {
          const view = new DataView(buffer.buffer, buffer.byteOffset + offset, 2);
          payloadLen = view.getUint16(0, false);
          offset += 2;
        } else if (payloadLen === 127) {
          const view = new DataView(buffer.buffer, buffer.byteOffset + offset, 8);
          const high = view.getUint32(0, false);
          const low = view.getUint32(4, false);
          if (high !== 0) {
            throw new WsFailure(
              WS_CLOSE_MESSAGE_TOO_BIG,
              "message too big",
              `FRAME_TOO_LARGE: frame exceeds ${MAX_TUNNEL_WS_MESSAGE_BYTES} bytes`,
            );
          }
          payloadLen = low;
          offset += 8;
        }

        const messageBytes = opcode === 0x0 ? fragmentedBytes + payloadLen : payloadLen;
        if (messageBytes > MAX_TUNNEL_WS_MESSAGE_BYTES) {
          throw new WsFailure(
            WS_CLOSE_MESSAGE_TOO_BIG,
            "message too big",
            `FRAME_TOO_LARGE: message exceeds ${MAX_TUNNEL_WS_MESSAGE_BYTES} bytes`,
          );
        }

        let maskKey: Uint8Array | null = null;
        if (hasMask) {
          maskKey = sliceBytes(buffer, offset, offset + 4);
          offset += 4;
        }

        const frameTotalSize = headerSize + payloadLen;
        while (buffer.length < frameTotalSize) {
          const chunk = await stream.read();
          if (chunk.length === 0) {
            handleClose(1006, "Abnormal closure", false);
            return;
          }
          buffer = concatBytes(buffer, chunk);
        }

        const rawPayload = sliceBytes(buffer, offset, offset + payloadLen);
        buffer = sliceBytes(buffer, frameTotalSize);

        let payload: Uint8Array;
        if (hasMask && maskKey) {
          payload = new Uint8Array(rawPayload.length);
          for (let i = 0; i < rawPayload.length; i++) {
            payload[i] = rawPayload[i] ^ maskKey[i % 4];
          }
        } else {
          payload = rawPayload;
        }

        if (opcode === 0x8) {
          let code = 1005;
          let reason = "";
          if (payload.length >= 2) {
            const view = new DataView(payload.buffer, payload.byteOffset, 2);
            code = view.getUint16(0, false);
            if (payload.length > 2) {
              reason = new TextDecoder("utf-8").decode(sliceBytes(payload, 2));
            }
          }
          if (readyState === WS_OPEN && !isClosing) {
            try {
              await sendFrame(0x8, payload);
            } catch {}
          }
          handleClose(code, reason, true);
          return;
        } else if (opcode === 0x9) {
          try {
            await sendFrame(0xA, payload);
          } catch (err) {
            handleError(err);
          }
        } else if (opcode === 0xA) {
        } else if (opcode === 0x0) {
          if (fragmentedOpcode === 0) {
            throw new Error("PROTOCOL_ERROR: unexpected continuation frame");
          }
          fragmentedPayloads.push(payload);
          fragmentedBytes += payload.length;
          if (fin) {
            const assembled = concatBytes(...fragmentedPayloads);
            const deliverOp = fragmentedOpcode;
            fragmentedOpcode = 0;
            fragmentedPayloads = [];
            fragmentedBytes = 0;
            deliverMessage(deliverOp, assembled);
          }
        } else if (opcode === 0x1 || opcode === 0x2) {
          if (!fin) {
            fragmentedOpcode = opcode;
            fragmentedPayloads = [payload];
            fragmentedBytes = payload.length;
          } else {
            deliverMessage(opcode, payload);
          }
        } else {
          throw new Error(`PROTOCOL_ERROR: unsupported opcode 0x${opcode.toString(16)}`);
        }
      }
    } catch (err) {
      handleError(err);
      if (err instanceof WsFailure) {
        if (readyState === WS_OPEN) {
          readyState = WS_CLOSING;
          isClosing = true;
          await sendFrame(0x8, encodeClosePayload(err.closeCode, err.closeReason));
        }
        handleClose(err.closeCode, err.closeReason, false);
      } else {
        handleClose(1006, "Stream read error", false);
      }
    }
  }

  runReceiveLoop();

  queueMicrotask(() => {
    if (readyState === WS_OPEN && onopenCallback) {
      const event = typeof Event !== "undefined" ? new Event("open") : ({ type: "open" } as Event);
      onopenCallback(event);
    }
  });

  const ws: TunnelWebSocket = {
    send(data: Uint8Array | string): void {
      if (readyState !== WS_OPEN) {
        throw new Error("INVALID_STATE: WebSocket is not open");
      }
      if (typeof data === "string") {
        const payload = new TextEncoder().encode(data);
        sendFrame(0x1, payload).catch((err) => handleError(err));
      } else {
        sendFrame(0x2, data).catch((err) => handleError(err));
      }
    },
    close(code?: number, reason?: string): void {
      if (readyState === WS_CLOSING || readyState === WS_CLOSED) return;
      readyState = WS_CLOSING;
      isClosing = true;

      const payload = code !== undefined ? encodeClosePayload(code, reason) : new Uint8Array(0);

      sendFrame(0x8, payload).finally(() => {
        handleClose(code ?? 1000, reason ?? "", true);
      });
    },
    get readyState() {
      return readyState;
    },
    get binaryType() {
      return binaryType;
    },
    set binaryType(val: string) {
      binaryType = val;
    },
    get onopen() {
      return onopenCallback;
    },
    set onopen(cb: ((event: Event) => void) | null) {
      onopenCallback = cb;
    },
    get onmessage() {
      return onmessageCallback;
    },
    set onmessage(cb: ((event: TunnelMessageEvent) => void) | null) {
      onmessageCallback = cb;
      flushUndelivered();
    },
    get onerror() {
      return onerrorCallback;
    },
    set onerror(cb: ((event: TunnelErrorEvent) => void) | null) {
      onerrorCallback = cb;
    },
    get onclose() {
      return oncloseCallback;
    },
    set onclose(cb: ((event: TunnelCloseEvent) => void) | null) {
      oncloseCallback = cb;
    },
  };

  return ws;
}

export async function openAccountTunnel(
  opts: OpenAccountTunnelOpts,
): Promise<{ transport: TunnelTransport; stream?: TunnelByteStream; close(): void }> {
  if (!opts.socketUrl) {
    throw new Error("MISSING_SOCKET_URL: socketUrl is required");
  }
  if (!opts.machineId) {
    throw new Error("MISSING_MACHINE_ID: machineId is required");
  }
  if (!opts.sessionId) {
    throw new Error("MISSING_SESSION_ID: sessionId is required");
  }
  if (!opts.enrollmentEpoch) {
    throw new Error("MISSING_ENROLLMENT_EPOCH: enrollmentEpoch is required");
  }

  let localPriv: Uint8Array;
  let localPub: Uint8Array;
  let remotePub: Uint8Array;

  try {
    if (!opts.localKeyPair?.privateKey || !opts.localKeyPair?.publicKey || !opts.machineAttachPublicKey) {
      throw new Error("MISSING_KEYS: localKeyPair (publicKey, privateKey) and machineAttachPublicKey are required");
    }
    localPriv = base64ToBytes(opts.localKeyPair.privateKey);
    localPub = base64ToBytes(opts.localKeyPair.publicKey);
    remotePub = base64ToBytes(opts.machineAttachPublicKey);

    if (localPriv.length !== 32 || localPub.length !== 32 || remotePub.length !== 32) {
      throw new Error("INVALID_KEY_LENGTH: X25519 keys must be exactly 32 bytes");
    }
  } catch (err) {
    throw new Error(`ATTACH_KEY_INVALID: ${err instanceof Error ? err.message : String(err)}`);
  }

  const prologue = attachPrologue(opts.machineId, opts.sessionId, opts.enrollmentEpoch);
  const primitives = createNoisePrimitives();
  const config: NoiseInitiatorConfig = {
    localPrivateKey: localPriv,
    remotePublicKey: remotePub,
    prologue,
  };

  const initiator = createNoiseInitiator(primitives, config);

  const ws = new WebSocket(opts.socketUrl);
  ws.binaryType = "arraybuffer";

  const HANDSHAKE_TIMEOUT_MS = 10000;

  let handshakeComplete = false;
  let timeoutId: ReturnType<typeof setTimeout> | null = null;

  const incomingQueue: Uint8Array[] = [];
  let pendingReadResolve: ((bytes: Uint8Array) => void) | null = null;
  let pendingReadReject: ((err: Error) => void) | null = null;
  let isClosed = false;
  let socketError: Error | null = null;
  let rxBuffer: Uint8Array = new Uint8Array(0);
  // Every inbound event (message, error, close) runs on this chain so plaintext reaches
  // readers in arrival order even if AEAD decrypts settle out of order, and a close or
  // error never overtakes a message that arrived before it.
  let incomingChain: Promise<void> = Promise.resolve();

  function enqueueIncoming(task: () => Promise<void> | void) {
    incomingChain = incomingChain.then(task).catch((err) => {
      failTerminal(err instanceof Error ? err : new Error(String(err)));
    });
  }

  /**
   * The one terminal path after the handshake: records the first error, marks the stream
   * closed, closes the socket, and wakes a pending read (rejecting it when an error caused
   * the termination, otherwise resolving it with EOF).
   */
  function failTerminal(err: Error | null) {
    if (err && !socketError) socketError = err;
    if (isClosed) return;
    isClosed = true;
    try {
      ws.close();
    } catch {
      // The socket is already closing; nothing else to release.
    }
    const resolve = pendingReadResolve;
    const reject = pendingReadReject;
    pendingReadResolve = null;
    pendingReadReject = null;
    if (socketError) {
      reject?.(socketError);
    } else {
      resolve?.(new Uint8Array(0));
    }
  }

  function pushPlaintext(bytes: Uint8Array) {
    if (bytes.length === 0) return;
    if (pendingReadResolve) {
      const resolve = pendingReadResolve;
      pendingReadResolve = null;
      pendingReadReject = null;
      resolve(bytes);
    } else {
      incomingQueue.push(bytes);
    }
  }

  async function handleIncomingEncryptedBytes(raw: Uint8Array) {
    if (isClosed) return;
    rxBuffer = concatBytes(rxBuffer, raw);
    const { messages, rest: remaining } = decodeFrames(rxBuffer);
    rxBuffer = toCleanBytes(remaining);
    for (const ciphertext of messages) {
      const plaintext = await initiator.decrypt(ciphertext);
      pushPlaintext(plaintext);
    }
  }

  await new Promise<void>((resolve, reject) => {
    timeoutId = setTimeout(() => {
      if (!handshakeComplete) {
        ws.close();
        reject(new TunnelError("HANDSHAKE_TIMEOUT", "handshake timed out after 10000ms"));
      }
    }, HANDSHAKE_TIMEOUT_MS);

    ws.onopen = async () => {
      try {
        const msg1 = await initiator.createHandshakeMessage1();
        const frame1 = encodeFrame(msg1);
        ws.send(frame1);
      } catch (err) {
        if (timeoutId) clearTimeout(timeoutId);
        ws.close();
        reject(err instanceof Error ? err : new Error(String(err)));
      }
    };

    ws.onmessage = (event) => {
      if (handshakeComplete) {
        if (typeof event.data === "string") {
          enqueueIncoming(() => {
            throw new Error("NON_BINARY_MESSAGE: received non-binary WebSocket message");
          });
          return;
        }
        const bytes = toCleanBytes(new Uint8Array(event.data as ArrayBuffer));
        enqueueIncoming(() => handleIncomingEncryptedBytes(bytes));
        return;
      }
      enqueueIncoming(() => handleHandshakeMessage(event));
    };

    const handleHandshakeMessage = async (event: MessageEvent) => {
      if (handshakeComplete) {
        // A message queued behind message 2 is ordinary transport data.
        if (typeof event.data === "string") {
          throw new Error("NON_BINARY_MESSAGE: received non-binary WebSocket message");
        }
        await handleIncomingEncryptedBytes(toCleanBytes(new Uint8Array(event.data as ArrayBuffer)));
        return;
      }
      if (typeof event.data === "string") {
        if (timeoutId) clearTimeout(timeoutId);
        ws.close();
        reject(new Error("NON_BINARY_MESSAGE: received non-binary WebSocket message during handshake"));
        return;
      }

      const raw = toCleanBytes(new Uint8Array(event.data as ArrayBuffer));

      rxBuffer = concatBytes(rxBuffer, raw);
      try {
        const { messages, rest: remaining } = decodeFrames(rxBuffer);
        if (messages.length > 0) {
          const msg2 = messages[0];
          rxBuffer = toCleanBytes(remaining);
          await initiator.processHandshakeMessage2(msg2);
          handshakeComplete = true;
          if (timeoutId) clearTimeout(timeoutId);

          for (let i = 1; i < messages.length; i++) {
            const pt = await initiator.decrypt(messages[i]);
            pushPlaintext(pt);
          }
          if (rxBuffer.length > 0) {
            const decoded = decodeFrames(rxBuffer);
            rxBuffer = toCleanBytes(decoded.rest);
            for (const ct of decoded.messages) {
              const pt = await initiator.decrypt(ct);
              pushPlaintext(pt);
            }
          }

          resolve();
        }
      } catch (err) {
        if (timeoutId) clearTimeout(timeoutId);
        ws.close();
        reject(err instanceof Error ? err : new Error(String(err)));
      }
    };

    ws.onerror = (event) => {
      const err = new Error(`WEBSOCKET_ERROR: ${String(event)}`);
      if (!handshakeComplete) {
        if (timeoutId) clearTimeout(timeoutId);
        socketError = err;
        reject(err);
        return;
      }
      enqueueIncoming(() => failTerminal(err));
    };

    ws.onclose = (event: TunnelCloseEvent) => {
      if (!handshakeComplete) {
        if (timeoutId) clearTimeout(timeoutId);
        isClosed = true;
        const refusal = attachTunnelRefusalCode(event);
        reject(
          refusal
            ? new AttachTunnelRefusedError(refusal)
            : new TunnelError("PREMATURE_CLOSE", "socket closed before handshake completed"),
        );
        return;
      }
      enqueueIncoming(() => failTerminal(null));
    };
  });

  const tunnelByteStream: TunnelByteStream = {
    get isClosed(): boolean {
      return isClosed;
    },
    async write(plaintext: Uint8Array): Promise<void> {
      if (isClosed) {
        throw new TunnelError("STREAM_CLOSED", "cannot write to closed stream");
      }
      if (plaintext.length === 0) return;

      let offset = 0;
      while (offset < plaintext.length) {
        const chunkSize = Math.min(plaintext.length - offset, MAX_ATTACH_FRAME);
        const chunk = sliceBytes(plaintext, offset, offset + chunkSize);
        offset += chunkSize;

        const ciphertext = await initiator.encrypt(chunk);
        const frame = encodeFrame(ciphertext);
        ws.send(frame);
      }
    },

    async read(): Promise<Uint8Array> {
      if (incomingQueue.length > 0) {
        return incomingQueue.shift()!;
      }
      if (socketError) {
        throw socketError;
      }
      if (isClosed) {
        return new Uint8Array(0);
      }
      return new Promise<Uint8Array>((resolve, reject) => {
        pendingReadResolve = resolve;
        pendingReadReject = reject;
      });
    },

    close(): void {
      failTerminal(null);
    },
  };

  const transport = createTunnelTransport(tunnelByteStream);
  return {
    transport,
    stream: tunnelByteStream,
    close: () => tunnelByteStream.close(),
  };
}
