function decodeBase64(data: string): Uint8Array {
  const binary = globalThis.atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
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

  reset(sessionId: string) {
    this.decoders.delete(sessionId);
  }

  clear() {
    this.decoders.clear();
  }

  private getDecoder(sessionId: string) {
    const existing = this.decoders.get(sessionId);
    if (existing) return existing;
    const decoder = new TextDecoder("utf-8", { fatal: false });
    this.decoders.set(sessionId, decoder);
    return decoder;
  }
}
