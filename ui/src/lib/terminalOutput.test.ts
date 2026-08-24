import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import {
  TerminalOutputDecoderRegistry,
  decodeBase64,
  decodeTerminalOutputFrame,
} from "./terminalOutput";

function encodeTerminalOutputFrame(
  sessionId: string,
  data: Uint8Array,
  sequence?: bigint,
  daemonEpoch?: bigint,
): Uint8Array {
  const sessionBytes = new TextEncoder().encode(sessionId);
  const frame = new Uint8Array(20 + sessionBytes.byteLength + data.byteLength);
  const view = new DataView(frame.buffer);
  let flags = 0;
  if (sequence !== undefined) flags |= 1;
  if (daemonEpoch !== undefined) flags |= 2;
  view.setUint8(0, 1);
  view.setUint8(1, flags);
  view.setUint16(2, sessionBytes.byteLength, true);
  view.setBigUint64(4, sequence ?? 0n, true);
  view.setBigUint64(12, daemonEpoch ?? 0n, true);
  frame.set(sessionBytes, 20);
  frame.set(data, 20 + sessionBytes.byteLength);
  return frame;
}

describe("TerminalOutputDecoderRegistry", () => {
  it("preserves Korean and emoji UTF-8 sequences split across PTY chunk boundaries", () => {
    const registry = new TerminalOutputDecoderRegistry();
    const bytes = new TextEncoder().encode("A한🙂B");
    const chunks = [bytes.slice(0, 2), bytes.slice(2, 5), bytes.slice(5, 8), bytes.slice(8)];

    const output = chunks
      .map((chunk) => registry.decode("session-1", chunk))
      .join("") + registry.finish("session-1");

    expect(output).toBe("A한🙂B");
    expect(output).not.toContain("\uFFFD");
  });

  it("keeps decoder state isolated per backend session", () => {
    const registry = new TerminalOutputDecoderRegistry();
    const korean = new TextEncoder().encode("한");
    const emoji = new TextEncoder().encode("🙂");

    const firstA = registry.decode("a", korean.slice(0, 1));
    const firstB = registry.decode("b", emoji.slice(0, 2));
    const secondA = registry.decode("a", korean.slice(1));
    const secondB = registry.decode("b", emoji.slice(2));

    expect(firstA + secondA + registry.finish("a")).toBe("한");
    expect(firstB + secondB + registry.finish("b")).toBe("🙂");
  });

  it("does not use a per-character charCodeAt decode loop in terminalOutput.ts", () => {
    const sourcePath = resolve(__dirname, "terminalOutput.ts");
    const source = readFileSync(sourcePath, "utf-8");
    expect(source).not.toContain("charCodeAt");
  });

  it("correctly decodes large multi-chunk byte payloads without corruption", () => {
    const registry = new TerminalOutputDecoderRegistry();
    const baseText = "\x1b[32m[INFO]\x1b[0m 빌드 성공! 🚀 Process completed in 1.42s.\n";
    const fullText = baseText.repeat(100);
    const bytes = new TextEncoder().encode(fullText);

    const chunks: Uint8Array[] = [];
    for (let offset = 0; offset < bytes.length; offset += 37) {
      chunks.push(bytes.slice(offset, offset + 37));
    }

    const output = chunks
      .map((chunk) => registry.decode("session-large", chunk))
      .join("") + registry.finish("session-large");

    expect(output).toBe(fullText);
  });
});

describe("decodeTerminalOutputFrame", () => {
  it("parses session metadata while preserving the PTY payload as a Uint8Array view", () => {
    const payload = new Uint8Array([0, 1, 2, 0x1b, 0xff, 65]);
    const frame = encodeTerminalOutputFrame("session-raw-1", payload, 1234567890123n, 987654321n);

    const decoded = decodeTerminalOutputFrame(frame);

    expect(decoded.sessionId).toBe("session-raw-1");
    expect(decoded.sequence).toBe("1234567890123");
    expect(decoded.daemonEpoch).toBe("987654321");
    expect(decoded.data).toBeInstanceOf(Uint8Array);
    expect(Array.from(decoded.data)).toEqual(Array.from(payload));
    expect(decoded.data.buffer).toBe(frame.buffer);
  });

  it("supports frames without sequence/epoch metadata", () => {
    const frame = encodeTerminalOutputFrame("session-unsequenced", new TextEncoder().encode("hello"));
    const decoded = decodeTerminalOutputFrame(frame);

    expect(decoded.sequence).toBeNull();
    expect(decoded.daemonEpoch).toBeNull();
    expect(new TextDecoder().decode(decoded.data)).toBe("hello");
  });

  it("rejects malformed version and truncated session headers", () => {
    const wrongVersion = encodeTerminalOutputFrame("s", new Uint8Array([1]));
    wrongVersion[0] = 9;
    expect(() => decodeTerminalOutputFrame(wrongVersion)).toThrow("unsupported terminal output frame version");

    const truncated = new Uint8Array(20);
    const view = new DataView(truncated.buffer);
    view.setUint8(0, 1);
    view.setUint16(2, 5, true);
    expect(() => decodeTerminalOutputFrame(truncated)).toThrow("session id overruns payload");
  });
});

describe("decodeBase64", () => {
  it("decodes empty base64 string to empty Uint8Array", () => {
    const decoded = decodeBase64("");
    expect(decoded.length).toBe(0);
  });

  it("decodes binary and UTF-8 payloads identically to Buffer.from", () => {
    const originalBytes = new Uint8Array([0, 1, 2, 127, 128, 254, 255, 65, 66, 67]);
    const b64 = Buffer.from(originalBytes).toString("base64");
    const decoded = decodeBase64(b64);
    expect(Array.from(decoded)).toEqual(Array.from(originalBytes));
  });
});
