import { describe, expect, it, vi } from "vitest";

import type { TerminalLifecyclePayload, TerminalOutputPayload } from "./types";

const callbacks = vi.hoisted(() => ({
  output: null as ((payload: TerminalOutputPayload) => void) | null,
  lifecycle: null as ((payload: TerminalLifecyclePayload) => void) | null,
}));

vi.mock("./tauri", () => ({
  onTerminalOutput: vi.fn(async (listener: (payload: TerminalOutputPayload) => void) => {
    callbacks.output = listener;
    return () => undefined;
  }),
  onTerminalLifecycle: vi.fn(async (listener: (payload: TerminalLifecyclePayload) => void) => {
    callbacks.lifecycle = listener;
    return () => undefined;
  }),
}));

import { terminalEventBus } from "./terminalEvents";

function encodeOutput(text: string) {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

describe("terminalEventBus title tracking", () => {
  it("publishes OSC title changes even when no xterm output subscriber is mounted", async () => {
    await terminalEventBus.ensureStarted();
    const titles: Array<[string, string]> = [];
    const unsubscribe = terminalEventBus.subscribeTitle((sessionId, title) => titles.push([sessionId, title]));

    callbacks.output?.({
      sessionId: "backend-background",
      data: encodeOutput("progress\x1b]0;⠋ omo: working\x07more"),
    });
    callbacks.output?.({
      sessionId: "backend-background",
      data: encodeOutput("\x1b]0;✳ omo: done\x07"),
    });

    expect(titles).toEqual([
      ["backend-background", "⠋ omo: working"],
      ["backend-background", "✳ omo: done"],
    ]);

    unsubscribe();
    terminalEventBus.clearSession("backend-background");
  });

  it("deduplicates repeated identical OSC titles", async () => {
    await terminalEventBus.ensureStarted();
    const titles: string[] = [];
    const unsubscribe = terminalEventBus.subscribeTitle((_sessionId, title) => titles.push(title));
    const repeated = encodeOutput("\x1b]2;✋ codex: needs input\x07");

    callbacks.output?.({ sessionId: "backend-dedupe", data: repeated });
    callbacks.output?.({ sessionId: "backend-dedupe", data: repeated });

    expect(titles).toEqual(["✋ codex: needs input"]);

    unsubscribe();
    terminalEventBus.clearSession("backend-dedupe");
  });
});