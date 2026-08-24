import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { terminalEventBus, type TerminalOutputChunk } from "./terminalEvents";
import {
  attachScheduledOutputSubscription,
  getTerminalOutputSchedulerMetricsForTest,
  resetTerminalOutputSchedulerMetricsForTest,
  type TerminalOutputWriter,
} from "./terminalOutputScheduler";

function emptyAttachment(sessionId: string) {
  return Promise.resolve({
    sessionId,
    daemonEpoch: null,
    historyStartSequence: null,
    historyEndSequence: null,
    history: "",
    gap: null,
  });
}

describe("terminal output scheduler burst instrumentation", () => {
  let outputListener: ((data: TerminalOutputChunk, sequence?: string | null, daemonEpoch?: string | null) => void) | null;
  let frameCallback: FrameRequestCallback | null;

  beforeEach(() => {
    resetTerminalOutputSchedulerMetricsForTest();
    outputListener = null;
    frameCallback = null;

    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frameCallback = callback;
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", () => undefined);
    vi.spyOn(terminalEventBus, "subscribeOutput").mockImplementation((_sessionId, listener) => {
      outputListener = listener;
      return vi.fn();
    });
    vi.spyOn(terminalEventBus, "subscribeReplayGap").mockImplementation(() => vi.fn());
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("coalesces a same-frame byte burst into one xterm write and records the behavior", () => {
    const writes: Uint8Array[] = [];
    const writer = {
      write(data: string | Uint8Array) {
        writes.push(typeof data === "string" ? new TextEncoder().encode(data) : data);
      },
    } as TerminalOutputWriter;

    const unsubscribe = attachScheduledOutputSubscription("metrics-session", writer, {
      attachFn: () => emptyAttachment("metrics-session"),
    });

    outputListener?.(new TextEncoder().encode("first-"));
    outputListener?.(new TextEncoder().encode("second-"));
    outputListener?.(new TextEncoder().encode("third"));

    expect(writes).toHaveLength(0);
    const frameTime = performance.now() + 8;
    frameCallback?.(frameTime);

    expect(writes).toHaveLength(1);
    expect(new TextDecoder().decode(writes[0])).toBe("first-second-third");

    const metrics = getTerminalOutputSchedulerMetricsForTest();
    expect(metrics.receivedChunks).toBe(3);
    expect(metrics.receivedBytes).toBe("first-second-third".length);
    expect(metrics.writes).toBe(1);
    expect(metrics.frameFlushes).toBe(1);
    expect(metrics.thresholdFlushes).toBe(0);
    expect(metrics.coalescedChunks).toBe(2);
    expect(metrics.maxFrameWaitMs).toBeGreaterThanOrEqual(0);

    unsubscribe();
  });
});
