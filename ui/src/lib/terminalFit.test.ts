import type { FitAddon } from "@xterm/addon-fit";
import { describe, expect, it, vi } from "vitest";

import { fitTerminal } from "./terminalInstanceFactory";
import { terminalEventBus, type TerminalOutputChunk } from "./terminalEvents";
import { attachScheduledOutputSubscription } from "./terminalOutputScheduler";

type BufferMockOptions = {
  readonly viewportY: number;
  readonly baseY: number;
};

type MockTerminal = {
  readonly buffer: {
    readonly active: {
      baseY: number;
      viewportY: number;
    };
  };
  scrollToBottom(): void;
  reset(): void;
  write(data: string | Uint8Array, callback?: () => void): void;
};

type TestOutputListener = (
  data: TerminalOutputChunk,
  sequence?: string | null,
  daemonEpoch?: string | null,
) => void;

type TestReplayGapListener = (gap: {
  requestedAfterSequence: string;
  availableFromSequence: string;
  startSequence?: string | null;
  endSequence?: string | null;
  history: string;
  daemonEpoch?: string | null;
}) => void;

function createMockTerminal(bufferOptions: BufferMockOptions): {
  readonly terminal: MockTerminal;
  readonly scrollToBottom: ReturnType<typeof vi.fn>;
  readonly reset: ReturnType<typeof vi.fn>;
  readonly write: ReturnType<typeof vi.fn>;
} {
  const scrollToBottom = vi.fn();
  const reset = vi.fn();
  const write = vi.fn();
  const terminal: MockTerminal = {
    buffer: {
      active: {
        viewportY: bufferOptions.viewportY,
        baseY: bufferOptions.baseY,
      },
    },
    scrollToBottom,
    reset,
    write(data, callback) {
      write(typeof data === "string" ? data : new TextDecoder().decode(data));
      callback?.();
    },
  };

  return { terminal, scrollToBottom, reset, write };
}

function createMockFitAddon(): {
  readonly fitAddon: Pick<FitAddon, "fit">;
  readonly fit: ReturnType<typeof vi.fn>;
} {
  const fit = vi.fn();
  const fitAddon: Pick<FitAddon, "fit"> = { fit };
  return { fitAddon, fit };
}

describe("fitTerminal viewport preservation & output scheduling", () => {
  it("calls scrollToBottom() after fit when terminal is at the bottom", () => {
    const { terminal, scrollToBottom } = createMockTerminal({ viewportY: 100, baseY: 100 });
    const { fitAddon, fit } = createMockFitAddon();

    fitTerminal(terminal, fitAddon);

    expect(fit).toHaveBeenCalledOnce();
    expect(scrollToBottom).toHaveBeenCalledOnce();
  });

  it("does not call scrollToBottom() after fit when terminal is scrolled up in scrollback", () => {
    const { terminal, scrollToBottom } = createMockTerminal({ viewportY: 40, baseY: 100 });
    const { fitAddon, fit } = createMockFitAddon();

    fitTerminal(terminal, fitAddon);

    expect(fit).toHaveBeenCalledOnce();
    expect(scrollToBottom).not.toHaveBeenCalled();
  });

  it("does not call scrollToBottom() during normal output scheduling", () => {
    const { terminal, scrollToBottom, write } = createMockTerminal({ viewportY: 100, baseY: 100 });

    let capturedRaf: FrameRequestCallback | null = null;
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
      capturedRaf = cb;
      return 123;
    });
    vi.stubGlobal("cancelAnimationFrame", () => undefined);
    vi.spyOn(terminalEventBus, "subscribeOutput").mockImplementation((_sessionId, listener) => {
      listener("live output");
      return vi.fn();
    });

    const unsubscribe = attachScheduledOutputSubscription("test-sched-sess", terminal);

    expect(scrollToBottom).not.toHaveBeenCalled();
    expect(write).not.toHaveBeenCalled();

    if (capturedRaf) {
      (capturedRaf as FrameRequestCallback)(0);
    }
    expect(write).toHaveBeenCalledWith("live output");
    expect(scrollToBottom).not.toHaveBeenCalled();

    unsubscribe();
    vi.unstubAllGlobals();
  });

  it("resets and replays history across a forced runtime replay gap before resuming sequenced live output", async () => {
    const { terminal, reset, write } = createMockTerminal({ viewportY: 100, baseY: 100 });
    let capturedRaf: FrameRequestCallback | null = null;
    const outputListeners: TestOutputListener[] = [];
    const replayGapListeners: TestReplayGapListener[] = [];

    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
      capturedRaf = cb;
      return 456;
    });
    vi.stubGlobal("cancelAnimationFrame", () => undefined);
    vi.spyOn(terminalEventBus, "subscribeOutput").mockImplementation((_sessionId, listener) => {
      outputListeners.push(listener);
      return vi.fn();
    });
    const replayGapSpy = vi.spyOn(terminalEventBus, "subscribeReplayGap").mockImplementation((_sessionId, listener) => {
      replayGapListeners.push(listener);
      return vi.fn();
    });

    const onSequenceUpdate = vi.fn();
    const onGap = vi.fn();
    const unsubscribe = attachScheduledOutputSubscription("test-runtime-gap", terminal, {
      initialSequence: "10",
      daemonEpoch: "epoch-A",
      onSequenceUpdate,
      onGap,
      attachFn: vi.fn(async () => ({
        sessionId: "test-runtime-gap",
        daemonEpoch: "epoch-A",
        historyStartSequence: null,
        historyEndSequence: null,
        history: "",
        gap: null,
      })),
    });
    await Promise.resolve();

    const emitReplayGap = replayGapListeners[0];
    expect(emitReplayGap).toBeDefined();
    if (!emitReplayGap) throw new Error("replay gap listener was not registered");
    emitReplayGap({
      requestedAfterSequence: "10",
      availableFromSequence: "20",
      startSequence: "20",
      endSequence: "30",
      history: btoa("RESET_HISTORY_20_30;"),
      daemonEpoch: "epoch-A",
    });

    const emitOutput = outputListeners[0];
    expect(emitOutput).toBeDefined();
    if (!emitOutput) throw new Error("output listener was not registered");
    emitOutput("LIVE_31;", "31", "epoch-A");

    expect(reset).toHaveBeenCalledOnce();
    expect(onGap).toHaveBeenCalledOnce();
    expect(write).not.toHaveBeenCalled();
    if (capturedRaf) {
      (capturedRaf as FrameRequestCallback)(0);
    }

    expect(write).toHaveBeenCalledWith("RESET_HISTORY_20_30;LIVE_31;");
    expect(onSequenceUpdate).toHaveBeenLastCalledWith("31", "epoch-A");

    unsubscribe();
    replayGapSpy.mockRestore();
    vi.unstubAllGlobals();
  });
});
