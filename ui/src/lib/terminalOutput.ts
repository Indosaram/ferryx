declare global {
  interface Uint8ArrayConstructor {
    fromBase64?(data: string, options?: { readonly alphabet?: "base64" | "base64url" }): Uint8Array;
  }
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

export class TerminalOutputDecoderRegistry {
  private readonly decoders = new Map<string, TextDecoder>();

  decode(sessionId: string, base64Data: string): string {
    const decoder = this.getDecoder(sessionId);
    return decoder.decode(decodeBase64(base64Data), { stream: true });
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
