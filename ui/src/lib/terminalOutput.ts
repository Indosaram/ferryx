declare global {
  interface Uint8ArrayConstructor {
    fromBase64?(data: string, options?: { readonly alphabet?: "base64" | "base64url" }): Uint8Array;
  }
}

const TERMINAL_OUTPUT_FRAME_VERSION = 1;
const TERMINAL_OUTPUT_FRAME_FIXED_BYTES = 20;
const TERMINAL_OUTPUT_FRAME_HAS_SEQUENCE = 1 << 0;
const TERMINAL_OUTPUT_FRAME_HAS_DAEMON_EPOCH = 1 << 1;
const utf8Decoder = new TextDecoder("utf-8", { fatal: false });

export type DecodedTerminalOutputFrame = {
  sessionId: string;
  data: Uint8Array;
  sequence?: string | null;
  daemonEpoch?: string | null;
};

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

  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const version = view.getUint8(0);
  if (version !== TERMINAL_OUTPUT_FRAME_VERSION) {
    throw new Error(`unsupported terminal output frame version: ${version}`);
  }

  const flags = view.getUint8(1);
  const sessionIdLength = view.getUint16(2, true);
  const payloadOffset = TERMINAL_OUTPUT_FRAME_FIXED_BYTES + sessionIdLength;
  if (payloadOffset > bytes.byteLength) {
    throw new Error(
      `terminal output frame session id overruns payload: ${sessionIdLength} > ${bytes.byteLength - TERMINAL_OUTPUT_FRAME_FIXED_BYTES}`,
    );
  }

  const sessionId = utf8Decoder.decode(bytes.subarray(TERMINAL_OUTPUT_FRAME_FIXED_BYTES, payloadOffset));
  const sequence = (flags & TERMINAL_OUTPUT_FRAME_HAS_SEQUENCE) !== 0
    ? view.getBigUint64(4, true).toString()
    : null;
  const daemonEpoch = (flags & TERMINAL_OUTPUT_FRAME_HAS_DAEMON_EPOCH) !== 0
    ? view.getBigUint64(12, true).toString()
    : null;

  return {
    sessionId,
    data: bytes.subarray(payloadOffset),
    sequence,
    daemonEpoch,
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
