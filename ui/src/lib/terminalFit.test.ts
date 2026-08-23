import type { FitAddon } from "@xterm/addon-fit";
import { describe, expect, it, vi } from "vitest";

import { fitTerminal } from "./terminalInstanceFactory";
import { terminalEventBus } from "./terminalEvents";
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
  write(data: string, callback?: () => void): void;
};

function createMockTerminal(bufferOptions: BufferMockOptions): {
  readonly terminal: MockTerminal;
  readonly scrollToBottom: ReturnType<typeof vi.fn>;
  readonly write: ReturnType<typeof vi.fn>;
} {
  const scrollToBottom = vi.fn();
  const write = vi.fn();
  const terminal: MockTerminal = {
    buffer: {
      active: {
        viewportY: bufferOptions.viewportY,
        baseY: bufferOptions.baseY,
      },
    },
    scrollToBottom,
    write,
  };

  return { terminal, scrollToBottom, write };
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
});
