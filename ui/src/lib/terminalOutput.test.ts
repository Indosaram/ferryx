import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { TerminalOutputDecoderRegistry, decodeBase64 } from "./terminalOutput";

function encodeBase64(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString("base64");
}

describe("TerminalOutputDecoderRegistry", () => {
  it("preserves Korean and emoji UTF-8 sequences split across PTY chunk boundaries", () => {
    const registry = new TerminalOutputDecoderRegistry();
    const bytes = new TextEncoder().encode("A한🙂B");
    const chunks = [bytes.slice(0, 2), bytes.slice(2, 5), bytes.slice(5, 8), bytes.slice(8)];

    const output = chunks
      .map((chunk) => registry.decode("session-1", encodeBase64(chunk)))
      .join("") + registry.finish("session-1");

    expect(output).toBe("A한🙂B");
    expect(output).not.toContain("\uFFFD");
  });

  it("keeps decoder state isolated per backend session", () => {
    const registry = new TerminalOutputDecoderRegistry();
    const korean = new TextEncoder().encode("한");
    const emoji = new TextEncoder().encode("🙂");

    const firstA = registry.decode("a", encodeBase64(korean.slice(0, 1)));
    const firstB = registry.decode("b", encodeBase64(emoji.slice(0, 2)));
    const secondA = registry.decode("a", encodeBase64(korean.slice(1)));
    const secondB = registry.decode("b", encodeBase64(emoji.slice(2)));

    expect(firstA + secondA + registry.finish("a")).toBe("한");
    expect(firstB + secondB + registry.finish("b")).toBe("🙂");
  });

  it("does not use a per-character charCodeAt decode loop in terminalOutput.ts", () => {
    // Given: the source code of terminalOutput.ts
    const sourcePath = resolve(__dirname, "terminalOutput.ts");
    const source = readFileSync(sourcePath, "utf-8");

    // When: checking the implementation of base64 decoding
    // Then: it must not rely on an interpreted per-index charCodeAt loop
    expect(source).not.toContain("charCodeAt");
  });

  it("correctly decodes large multi-chunk base64 payloads without corruption", () => {
    // Given: a large multi-chunk payload containing mixed ASCII, Korean, emoji, and ANSI escapes
    const registry = new TerminalOutputDecoderRegistry();
    const baseText = "\x1b[32m[INFO]\x1b[0m 빌드 성공! 🚀 Process completed in 1.42s.\n";
    const fullText = baseText.repeat(100);
    const bytes = new TextEncoder().encode(fullText);

    // When: streamed across arbitrary chunk boundaries
    const chunkSize = 37;
    const chunks: Uint8Array[] = [];
    for (let offset = 0; offset < bytes.length; offset += chunkSize) {
      chunks.push(bytes.slice(offset, offset + chunkSize));
    }

    const output = chunks
      .map((chunk) => registry.decode("session-large", encodeBase64(chunk)))
      .join("") + registry.finish("session-large");

    // Then: decoded output matches the original input string exactly
    expect(output).toBe(fullText);
  });
});

describe("decodeBase64", () => {
  it("decodes empty base64 string to empty Uint8Array", () => {
    // Given: an empty base64 string
    // When: decoding
    const decoded = decodeBase64("");

    // Then: returns 0-length Uint8Array
    expect(decoded.length).toBe(0);
  });

  it("decodes binary and UTF-8 payloads identically to Buffer.from", () => {
    // Given: arbitrary byte array with non-ASCII and binary bytes
    const originalBytes = new Uint8Array([0, 1, 2, 127, 128, 254, 255, 65, 66, 67]);
    const b64 = Buffer.from(originalBytes).toString("base64");

    // When: decoding with decodeBase64
    const decoded = decodeBase64(b64);

    // Then: matches original bytes
    expect(Array.from(decoded)).toEqual(Array.from(originalBytes));
  });
});
