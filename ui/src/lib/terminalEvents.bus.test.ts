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

import {
  getBacklogMetricsForTest,
  terminalEventBus,
  type TerminalOutputChunk,
} from "./terminalEvents";

function encodeOutput(text: string) {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

function outputChunkToText(chunk: TerminalOutputChunk): string {
  return typeof chunk === "string" ? chunk : new TextDecoder().decode(chunk);
}

describe("terminalEventBus title tracking", () => {
  it("publishes OSC title changes even when no terminal output subscriber is mounted", async () => {
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

describe("terminalEventBus backlog buffer", () => {
  it("stores incoming output in chunk array without quadratic concatenation", async () => {
    await terminalEventBus.ensureStarted();
    const sessionId = "backend-chunks";
    const chunkCount = 25;
    const chunkText = "x".repeat(100);

    // An output listener must be attached for the session to retain a replay backlog.
    const unsubscribe = terminalEventBus.subscribeOutput(sessionId, () => undefined, false);
    for (let index = 0; index < chunkCount; index += 1) {
      callbacks.output?.({
        sessionId,
        data: encodeOutput(chunkText),
      });
    }

    const metrics = getBacklogMetricsForTest(sessionId);
    expect(metrics.sessions).toBe(1);
    expect(metrics.chars).toBe(chunkCount * 100);
    expect(metrics.chunks).toBeGreaterThan(1);
    expect(metrics.chunks).toBe(chunkCount);

    unsubscribe();
    terminalEventBus.clearSession(sessionId);
  });

  it("replays retained chunks in order without one contiguous totalBytes delivery", async () => {
    await terminalEventBus.ensureStarted();
    const sessionId = "backend-segmented-replay";
    const retainedChunks = ["first", "second-longer", "third"];

    // A live output listener retains the backlog; a later replay subscriber then receives it.
    const unsubscribeSink = terminalEventBus.subscribeOutput(sessionId, () => undefined, false);
    for (const text of retainedChunks) {
      callbacks.output?.({
        sessionId,
        data: encodeOutput(text),
      });
    }

    const replayedChunks: Uint8Array[] = [];
    const unsubscribe = terminalEventBus.subscribeOutput(
      sessionId,
      (chunk) => {
        expect(chunk).toBeInstanceOf(Uint8Array);
        replayedChunks.push(chunk as Uint8Array);
      },
      true,
    );

    expect(replayedChunks.map(outputChunkToText)).toEqual(retainedChunks);
    expect(replayedChunks.map((chunk) => chunk.byteLength)).toEqual(
      retainedChunks.map((text) => new TextEncoder().encode(text).byteLength),
    );
    expect(replayedChunks).toHaveLength(retainedChunks.length);
    expect(replayedChunks.some((chunk) => chunk.byteLength === retainedChunks.join("").length)).toBe(false);

    unsubscribe();
    unsubscribeSink();
    terminalEventBus.clearSession(sessionId);
  });

  it("bounds total backlog size to MAX_BACKLOG_CHARS and replays latest tail to new subscriber", async () => {
    await terminalEventBus.ensureStarted();
    const sessionId = "backend-overflow";
    const chunkSize = 10000;
    const chunkCount = 60;
    const tailMarker = "TAIL_SENTINEL_2026";

    // A live output listener retains the bounded backlog for the later replay subscriber.
    const unsubscribeSink = terminalEventBus.subscribeOutput(sessionId, () => undefined, false);
    for (let index = 0; index < chunkCount; index += 1) {
      const isLast = index === chunkCount - 1;
      const text = isLast
        ? "y".repeat(chunkSize - tailMarker.length) + tailMarker
        : "y".repeat(chunkSize);
      callbacks.output?.({
        sessionId,
        data: encodeOutput(text),
      });
    }

    let replayed = "";
    const unsubscribe = terminalEventBus.subscribeOutput(sessionId, (chunk) => {
      replayed += outputChunkToText(chunk);
    }, true);

    const maxBacklog = 512 * 1024;
    expect(replayed.length).toBe(maxBacklog);
    expect(replayed.endsWith(tailMarker)).toBe(true);

    const metrics = getBacklogMetricsForTest(sessionId);
    expect(metrics.chars).toBe(maxBacklog);
    expect(metrics.chunks).toBeGreaterThan(1);

    unsubscribe();
    unsubscribeSink();
    terminalEventBus.clearSession(sessionId);
  });

  it("clears session backlog and listeners on clearSession", async () => {
    await terminalEventBus.ensureStarted();
    const sessionId = "backend-clear";
    const unsubscribe = terminalEventBus.subscribeOutput(sessionId, () => undefined, false);
    callbacks.output?.({
      sessionId,
      data: encodeOutput("some initial output"),
    });

    expect(getBacklogMetricsForTest(sessionId).chunks).toBe(1);
    unsubscribe();
    terminalEventBus.clearSession(sessionId);

    expect(getBacklogMetricsForTest(sessionId)).toEqual({
      sessions: 0,
      chunks: 0,
      chars: 0,
    });
  });

  it("replays bounded retained suffix in order and preserves title updates while unsubscribed", async () => {
    await terminalEventBus.ensureStarted();
    const sessionId = "unsubscribed-bounded-replay";
    const titles: Array<[string, string]> = [];
    const unsubscribeTitle = terminalEventBus.subscribeTitle((id, title) => {
      titles.push([id, title]);
    });

    // Title updates fire from the title listener alone, but retaining an output backlog for a
    // later replay subscriber requires a live output listener to be attached.
    const unsubscribeSink = terminalEventBus.subscribeOutput(sessionId, () => undefined, false);

    const maxBacklog = 512 * 1024;
    const initialPrefix = "START_OF_STREAM_THAT_SHOULD_BE_DROPPED\n";
    callbacks.output?.({
      sessionId,
      data: encodeOutput(initialPrefix),
    });

    callbacks.output?.({
      sessionId,
      data: encodeOutput("\x1b]0;background task running\x07"),
    });

    const chunkSize = 10000;
    const chunkCount = 60;
    for (let index = 0; index < chunkCount; index += 1) {
      callbacks.output?.({
        sessionId,
        data: encodeOutput(`chunk-${index.toString().padStart(4, "0")}: ${"a".repeat(chunkSize - 16)}\n`),
      });
    }

    callbacks.output?.({
      sessionId,
      data: encodeOutput("\x1b]0;background task complete\x07"),
    });

    expect(titles).toEqual([
      [sessionId, "background task running"],
      [sessionId, "background task complete"],
    ]);

    const receivedChunks: string[] = [];
    const unsubscribeOutput = terminalEventBus.subscribeOutput(
      sessionId,
      (chunk) => {
        receivedChunks.push(outputChunkToText(chunk));
      },
      true,
    );

    expect(receivedChunks.length).toBeGreaterThan(1);
    const replayedBacklog = receivedChunks.join("");
    expect(replayedBacklog.length).toBe(maxBacklog);
    expect(replayedBacklog.includes(initialPrefix)).toBe(false);
    expect(replayedBacklog.includes("chunk-0059:")).toBe(true);

    const liveChunk1 = "LIVE_CHUNK_ALPHA\n";
    const liveChunk2 = "LIVE_CHUNK_BETA\n";
    callbacks.output?.({
      sessionId,
      data: encodeOutput(liveChunk1),
    });
    callbacks.output?.({
      sessionId,
      data: encodeOutput(liveChunk2),
    });

    expect(receivedChunks.slice(0, -2).join("")).toBe(replayedBacklog);
    expect(receivedChunks.slice(-2)).toEqual([liveChunk1, liveChunk2]);

    unsubscribeTitle();
    unsubscribeOutput();
    unsubscribeSink();
    terminalEventBus.clearSession(sessionId);
  });

  it("passes sequence and daemonEpoch metadata to output subscribers", async () => {
    await terminalEventBus.ensureStarted();
    const sessionId = "backend-metadata";
    const delivered: Array<{ text: string; sequence?: string | null; daemonEpoch?: string | null }> = [];

    const unsubscribe = terminalEventBus.subscribeOutput(
      sessionId,
      (chunk, sequence, daemonEpoch) => {
        delivered.push({ text: outputChunkToText(chunk), sequence, daemonEpoch });
      },
      false,
    );

    callbacks.output?.({
      sessionId,
      data: encodeOutput("chunk-1"),
      sequence: "101",
      daemonEpoch: "epoch-A",
    });

    callbacks.output?.({
      sessionId,
      data: encodeOutput("chunk-2"),
      sequence: "102",
      daemonEpoch: "epoch-A",
    });

    expect(delivered).toEqual([
      { text: "chunk-1", sequence: "101", daemonEpoch: "epoch-A" },
      { text: "chunk-2", sequence: "102", daemonEpoch: "epoch-A" },
    ]);

    unsubscribe();
    terminalEventBus.clearSession(sessionId);
  });

  it("releases the replay backlog when the last output listener unsubscribes", async () => {
    await terminalEventBus.ensureStarted();
    const sessionId = "backend-backlog-release";

    const unsubscribe = terminalEventBus.subscribeOutput(sessionId, () => undefined, false);
    callbacks.output?.({ sessionId, data: encodeOutput("y".repeat(4096)) });
    expect(getBacklogMetricsForTest(sessionId).chars).toBe(4096);

    // The backlog exists only to replay into an attached listener. Once the last
    // one detaches, holding those bytes is a pure leak: nothing can consume them,
    // and up to MAX_BACKLOG_BYTES per session stays resident for the app's life.
    unsubscribe();

    expect(getBacklogMetricsForTest(sessionId)).toEqual({ sessions: 0, chunks: 0, chars: 0 });
  });

  it("treats an undecodable frame as a replay gap for the session named in its header", async () => {
    const sessionId = "backend-bad-frame";
    const gaps: Array<{ reason?: string }> = [];
    const unsubscribeGap = terminalEventBus.subscribeReplayGap(sessionId, (gap) => gaps.push(gap));
    const unsubscribeOutput = terminalEventBus.subscribeOutput(sessionId, () => undefined, false);
    callbacks.output?.({ sessionId, data: encodeOutput("before") });
    expect(getBacklogMetricsForTest(sessionId).chars).toBe(6);

    // Valid v2 header naming the session, but an unsupported flag layout is irrelevant: force the
    // body check to fail by declaring a gap block the payload does not contain.
    const name = new TextEncoder().encode(sessionId);
    const frame = new Uint8Array(20 + name.byteLength);
    frame[0] = 2;
    frame[1] = 1 << 2;
    frame[2] = name.byteLength;
    frame.set(name, 20);
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    (terminalEventBus as unknown as { handleChannelFrame(f: Uint8Array): void }).handleChannelFrame(frame);
    consoleError.mockRestore();

    expect(gaps).toHaveLength(1);
    expect(gaps[0]?.reason).toBe("decodeError");
    expect(getBacklogMetricsForTest(sessionId).chars).toBe(0);

    unsubscribeGap();
    unsubscribeOutput();
    terminalEventBus.clearSession(sessionId);
  });

  it("trims the backlog head on a UTF-8 boundary and copies the retained tail", async () => {
    await terminalEventBus.ensureStarted();
    const sessionId = "backend-utf8-trim";
    const maxBacklog = 512 * 1024;
    const unsubscribeSink = terminalEventBus.subscribeOutput(sessionId, () => undefined, false);
    // "€" is e2 82 ac. Overflowing by 2 bytes cuts it after its lead byte, leaving a lone 0xac.
    callbacks.output?.({ sessionId, data: encodeOutput("€" + "a".repeat(maxBacklog - 3)) });
    callbacks.output?.({ sessionId, data: encodeOutput("bb") });
    const replayedChunks: Uint8Array[] = [];
    const unsubscribe = terminalEventBus.subscribeOutput(sessionId, (chunk) => {
      replayedChunks.push(chunk as Uint8Array);
    }, true);
    const first = replayedChunks[0]!;
    expect(first[0]).toBe("a".charCodeAt(0));
    expect(getBacklogMetricsForTest(sessionId).chars).toBe(maxBacklog - 1);
    // slice() detaches the tail from the original frame buffer instead of pinning all of it.
    expect(first.buffer.byteLength).toBe(first.byteLength);
    unsubscribe();
    unsubscribeSink();
    terminalEventBus.clearSession(sessionId);
  });
});
