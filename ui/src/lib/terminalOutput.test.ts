import { describe, expect, it } from "vitest";

import { TerminalOutputDecoderRegistry } from "./terminalOutput";

function encodeBase64(bytes: Uint8Array) {
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
    expect(output).not.toContain("�");
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
});
