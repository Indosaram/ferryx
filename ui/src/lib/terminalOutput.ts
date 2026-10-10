declare global {
  interface Uint8ArrayConstructor {
    fromBase64?(data: string, options?: { readonly alphabet?: "base64" | "base64url" }): Uint8Array;
  }
}

const TERMINAL_OUTPUT_FRAME_VERSION_V1 = 1;
const TERMINAL_OUTPUT_FRAME_VERSION_V2 = 2;
const TERMINAL_OUTPUT_FRAME_FIXED_BYTES = 20;
const TERMINAL_OUTPUT_FRAME_HAS_SEQUENCE = 1 << 0;
const TERMINAL_OUTPUT_FRAME_HAS_DAEMON_EPOCH = 1 << 1;
export const TERMINAL_OUTPUT_FRAME_HAS_GAP = 1 << 2;
const TERMINAL_OUTPUT_FRAME_GAP_BYTES = 32;
const utf8Decoder = new TextDecoder("utf-8", { fatal: false });
const bigUint64Buffer = new ArrayBuffer(8);
const bigUint64View = new DataView(bigUint64Buffer);

export type DecodedTerminalOutputGap = {
  requestedAfterSequence: string;
  availableFromSequence: string;
  startSequence: string;
  endSequence: string;
};

export type DecodedTerminalOutputFrame = {
  sessionId: string;
  data: Uint8Array;
  sequence?: string | null;
  daemonEpoch?: string | null;
  gap?: DecodedTerminalOutputGap;
};

function readUint64LittleEndian(bytes: Uint8Array, offset: number): bigint {
  for (let i = 0; i < 8; i++) {
    bigUint64View.setUint8(i, bytes[offset + i]!);
  }
  return bigUint64View.getBigUint64(0, true);
}

export function decodeBase64(data: string): Uint8Array {
  if (typeof Uint8Array.fromBase64 === "function") {
    return Uint8Array.fromBase64(data);
  }
  if (typeof Buffer !== "undefined") {
    const buffer = Buffer.from(data, "base64");
    return new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
  }
  const binary = globalThis.atob(data);
  return Uint8Array.from(binary, (char) => char.codePointAt(0) ?? 0);
}

export function decodeTerminalOutputFrame(frame: ArrayBuffer | Uint8Array): DecodedTerminalOutputFrame {
  const bytes = ArrayBuffer.isView(frame)
    ? new Uint8Array(frame.buffer, frame.byteOffset, frame.byteLength)
    : new Uint8Array(frame);
  if (bytes.byteLength < TERMINAL_OUTPUT_FRAME_FIXED_BYTES) {
    throw new Error(`terminal output frame is too short: ${bytes.byteLength}`);
  }

  const version = bytes[0]!;
  if (version !== TERMINAL_OUTPUT_FRAME_VERSION_V1 && version !== TERMINAL_OUTPUT_FRAME_VERSION_V2) {
    throw new Error(`unsupported terminal output frame version: ${version}`);
  }

  const flags = bytes[1]!;
  const sessionIdLength = bytes[2]! | (bytes[3]! << 8);
  const sessionEnd = TERMINAL_OUTPUT_FRAME_FIXED_BYTES + sessionIdLength;
  if (sessionEnd > bytes.byteLength) {
    throw new Error(
      `terminal output frame session id overruns payload: ${sessionIdLength} > ${bytes.byteLength - TERMINAL_OUTPUT_FRAME_FIXED_BYTES}`,
    );
  }

  const hasGap = (flags & TERMINAL_OUTPUT_FRAME_HAS_GAP) !== 0;
  const gapBytes = hasGap ? TERMINAL_OUTPUT_FRAME_GAP_BYTES : 0;
  const payloadOffset = sessionEnd + gapBytes;
  if (payloadOffset > bytes.byteLength) {
    throw new Error(
      `terminal output frame gap fields overrun payload: ${gapBytes} > ${bytes.byteLength - sessionEnd}`,
    );
  }

  const sessionId = utf8Decoder.decode(bytes.subarray(TERMINAL_OUTPUT_FRAME_FIXED_BYTES, sessionEnd));
  const sequence = (flags & TERMINAL_OUTPUT_FRAME_HAS_SEQUENCE) !== 0
    ? readUint64LittleEndian(bytes, 4).toString()
    : null;
  const daemonEpoch = (flags & TERMINAL_OUTPUT_FRAME_HAS_DAEMON_EPOCH) !== 0
    ? readUint64LittleEndian(bytes, 12).toString()
    : null;
  const gap = hasGap
    ? {
        requestedAfterSequence: readUint64LittleEndian(bytes, sessionEnd).toString(),
        availableFromSequence: readUint64LittleEndian(bytes, sessionEnd + 8).toString(),
        startSequence: readUint64LittleEndian(bytes, sessionEnd + 16).toString(),
        endSequence: readUint64LittleEndian(bytes, sessionEnd + 24).toString(),
      }
    : undefined;

  return {
    sessionId,
    data: bytes.subarray(payloadOffset),
    sequence,
    daemonEpoch,
    ...(gap ? { gap } : {}),
  };
}

export class TerminalOutputDecoderRegistry {
  private readonly decoders = new Map<string, TextDecoder>();

  decode(sessionId: string, data: Uint8Array): string {
    const decoder = this.getDecoder(sessionId);
    return decoder.decode(data, { stream: true });
  }

  finish(sessionId: string): string {
    const decoder = this.decoders.get(sessionId);
    if (!decoder) return "";
    this.decoders.delete(sessionId);
    return decoder.decode();
  }

  reset(sessionId: string): void {
    this.decoders.delete(sessionId);
  }

  clear(): void {
    this.decoders.clear();
  }

  private getDecoder(sessionId: string): TextDecoder {
    const existing = this.decoders.get(sessionId);
    if (existing) return existing;
    const decoder = new TextDecoder("utf-8", { fatal: false });
    this.decoders.set(sessionId, decoder);
    return decoder;
  }
}
