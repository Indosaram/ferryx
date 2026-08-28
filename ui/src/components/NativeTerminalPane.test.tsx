import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TerminalSession } from "../lib/types";
import { useShortcuts } from "../lib/shortcuts";
import { NATIVE_TERMINAL_HANDLE_INSET_PX, NativeTerminalPane } from "./NativeTerminalPane";

const tauriCoreMocks = vi.hoisted(() => ({
  invoke: vi.fn<(cmd: string, args?: any) => Promise<any>>(async () => undefined),
  isTauri: vi.fn(() => true),
}));

const nativeTerminalEventMocks = vi.hoisted(() => ({
  scrollbarListener: null as ((payload: { sessionId: string; total: number; offset: number; len: number }) => void) | null,
  onNativeTerminalScrollbar: vi.fn(async (handler: (payload: {
    sessionId: string;
    total: number;
    offset: number;
    len: number;
  }) => void) => {
    nativeTerminalEventMocks.scrollbarListener = handler;
    return () => undefined;
  }),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCoreMocks.invoke,
  isTauri: tauriCoreMocks.isTauri,
}));

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  onNativeTerminalScrollbar: nativeTerminalEventMocks.onNativeTerminalScrollbar,
}));

type ResizeRecord = {
  observer: MockResizeObserver;
  callback: ResizeObserverCallback;
  observedElements: Set<Element>;
};

const resizeRecords: ResizeRecord[] = [];

class MockResizeObserver implements ResizeObserver {
  readonly observe = vi.fn((target: Element) => {
    this.observedElements.add(target);
  });
  readonly unobserve = vi.fn((target: Element) => {
    this.observedElements.delete(target);
  });
  readonly disconnect = vi.fn(() => {
    this.observedElements.clear();
  });
  readonly observedElements = new Set<Element>();

  constructor(public callback: ResizeObserverCallback) {
    resizeRecords.push({
      observer: this,
      callback: this.callback,
      observedElements: this.observedElements,
    });
  }
}

// jsdom reports an all-zero rect, which the compositor rejects as invalid
// dimensions, so panes under test must measure a real area to reach set_bounds.
const PANE_RECT = {
  x: 10,
  y: 20,
  width: 800,
  height: 600,
  top: 20,
  bottom: 620,
  left: 10,
  right: 810,
  toJSON: () => ({}),
} as DOMRect;

function stubPaneRect(): () => void {
  const original = HTMLElement.prototype.getBoundingClientRect;
  HTMLElement.prototype.getBoundingClientRect = function () {
    return PANE_RECT;
  };
  return () => {
    HTMLElement.prototype.getBoundingClientRect = original;
  };
}

function createSession(
  sessionId = "term-session-1",
  backendSessionId: string | null = sessionId,
): TerminalSession {
  return {
    id: sessionId,
    cwd: "/workspace/orca-lite",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId,
    lifecycle: "working",
  };
}

describe("NativeTerminalPane IPC failure reporting and visible error state", () => {
  let restorePaneRect: () => void;
  beforeEach(() => {
    restorePaneRect = stubPaneRect();
    tauriCoreMocks.invoke.mockReset();
    tauriCoreMocks.invoke.mockResolvedValue(undefined);
    tauriCoreMocks.isTauri.mockReset();
    tauriCoreMocks.isTauri.mockReturnValue(true);
    nativeTerminalEventMocks.scrollbarListener = null;
    nativeTerminalEventMocks.onNativeTerminalScrollbar.mockClear();
    resizeRecords.length = 0;
    vi.stubGlobal("ResizeObserver", MockResizeObserver);
  });

  afterEach(() => {
    restorePaneRect();
    cleanup();
    vi.unstubAllGlobals();
  });

  it("suppresses the attach error banner across fast retries, surfaces it once the failure persists, and clears it on retry", async () => {
    vi.useFakeTimers();
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      const session = createSession("term-session-1");

      tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
        if (cmd === "cmd_native_terminal_attach") {
          throw new Error("failed to attach native terminal surface at /Users/secret/path/project");
        }
        return undefined;
      });

      const { queryByRole, rerender } = render(
        <NativeTerminalPane sessionId="term-session-1" session={session} />,
      );

      // Attempt 1 fails. The failure is reported to the IPC failure channel immediately, but the
      // banner stays hidden because a fast retry is already pending.
      await act(async () => {
        await Promise.resolve();
      });
      expect(consoleSpy).toHaveBeenCalledWith("Native terminal IPC command failed", {
        command: "cmd_native_terminal_attach",
        error: expect.any(Error),
      });
      expect(queryByRole("alert")).not.toBeInTheDocument();

      // Backoff 1 (250ms) -> attempt 2 fails: still inside the fast-retry window, still silent.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(250);
      });
      expect(queryByRole("alert")).not.toBeInTheDocument();

      // Backoff 2 (500ms) -> attempt 3 fails: the failure has outlived the fast retries, so the
      // accessible banner surfaces, with the raw filesystem path redacted.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(500);
      });
      const alert = queryByRole("alert");
      expect(alert).toBeInTheDocument();
      expect(alert).toHaveTextContent("Failed to attach native terminal");
      expect(alert).not.toHaveTextContent("/Users/secret/path/project");

      // When attach succeeds on next mount / session change
      tauriCoreMocks.invoke.mockImplementation(async () => undefined);
      rerender(
        <NativeTerminalPane sessionId="term-session-2" session={createSession("term-session-2")} />,
      );

      await act(async () => {
        await vi.advanceTimersByTimeAsync(1);
        await vi.advanceTimersByTimeAsync(1);
      });
      expect(queryByRole("alert")).not.toBeInTheDocument();
    } finally {
      consoleSpy.mockRestore();
      vi.useRealTimers();
    }
  });

  it("displays an accessible error banner when bounds IPC fails and does not show detach errors on unmount", async () => {
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const session = createSession("term-session-1");

    tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_set_bounds") {
        throw new Error("bounds calculation failure at /Users/secret/path/bounds");
      }
      if (cmd === "cmd_native_terminal_detach") {
        throw new Error("detach cleanup failure");
      }
      return undefined;
    });

    const { findByRole, unmount } = render(
      <NativeTerminalPane sessionId="term-session-1" session={session} />,
    );

    const alert = await findByRole("alert");
    expect(alert).toBeInTheDocument();
    expect(alert).toHaveTextContent("Failed to update native terminal bounds");
    expect(alert).not.toHaveTextContent("/Users/secret/path/bounds");

    // Unmounting invokes detach failure which must still be logged but not crash / render UI
    unmount();
    await waitFor(() => {
      expect(consoleSpy).toHaveBeenCalledWith("Native terminal IPC command failed", {
        command: "cmd_native_terminal_detach",
        error: expect.any(Error),
      });
    });

    consoleSpy.mockRestore();
  });

  it("stays silent when a bounds update loses the race with its own detach", async () => {
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const session = createSession("term-session-1");

    // The compositor released this surface before the in-flight geometry update landed, which is
    // exactly what rapid tab switching produces. It is a benign no-op, not a terminal failure.
    tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_set_bounds") {
        throw {
          code: "SESSION_NOT_FOUND",
          message: "Native terminal session term-session-1 has no attached surface",
        };
      }
      return undefined;
    });

    const { queryByRole } = render(
      <NativeTerminalPane sessionId="term-session-1" session={session} />,
    );

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
        "cmd_native_terminal_set_bounds",
        expect.anything(),
      );
    });

    expect(queryByRole("alert")).not.toBeInTheDocument();
    expect(consoleSpy).not.toHaveBeenCalledWith(
      "Native terminal IPC command failed",
      expect.objectContaining({ command: "cmd_native_terminal_set_bounds" }),
    );

    consoleSpy.mockRestore();
  });
});

describe("NativeTerminalPane geometry reporting contract", () => {
  let originalPixelRatio: number;
  let originalGetBoundingClientRect: typeof HTMLElement.prototype.getBoundingClientRect;

  beforeEach(() => {
    tauriCoreMocks.invoke.mockReset();
    tauriCoreMocks.invoke.mockResolvedValue(undefined);
    tauriCoreMocks.isTauri.mockReset();
    tauriCoreMocks.isTauri.mockReturnValue(true);
    nativeTerminalEventMocks.scrollbarListener = null;
    nativeTerminalEventMocks.onNativeTerminalScrollbar.mockClear();

    resizeRecords.length = 0;
    vi.stubGlobal("ResizeObserver", MockResizeObserver);

    originalPixelRatio = window.devicePixelRatio;
    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: 2,
    });

    originalGetBoundingClientRect = HTMLElement.prototype.getBoundingClientRect;
    HTMLElement.prototype.getBoundingClientRect = function () {
      return {
        x: 10,
        y: 20,
        width: 800,
        height: 600,
        top: 20,
        bottom: 620,
        left: 10,
        right: 810,
        toJSON: () => ({}),
      } as DOMRect;
    };
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: originalPixelRatio,
    });
    HTMLElement.prototype.getBoundingClientRect = originalGetBoundingClientRect;
  });

  it("measures viewport DOM geometry and passes bounds and scaleFactor in cmd_native_terminal_attach on mount", async () => {
    const session = createSession("term-session-presize");

    render(<NativeTerminalPane sessionId="term-session-presize" session={session} />);

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_attach", {
      sessionId: "term-session-presize",
      bounds: {
        x: 10,
        y: 20,
        width: 800,
        height: 600,
      },
      scaleFactor: 2,
    });
  });

  it("omits geometry in cmd_native_terminal_attach when viewport has zero-area dimensions on mount", async () => {
    HTMLElement.prototype.getBoundingClientRect = function () {
      return {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
        top: 0,
        bottom: 0,
        left: 0,
        right: 0,
        toJSON: () => ({}),
      } as DOMRect;
    };

    const session = createSession("term-session-zero");
    render(<NativeTerminalPane sessionId="term-session-zero" session={session} />);

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_attach", {
      sessionId: "term-session-zero",
    });
  });

  it("observes DOM rectangle, explicitly attaches session, and reports initial bounds on mount in Tauri mode", async () => {
    const session = createSession("term-session-1");

    render(<NativeTerminalPane sessionId="term-session-1" session={session} />);

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_attach", {
      sessionId: "term-session-1",
      bounds: {
        x: 10,
        y: 20,
        width: 800,
        height: 600,
      },
      scaleFactor: 2,
    });

    await waitFor(() => {
      expect(resizeRecords.length).toBeGreaterThan(0);
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
        "cmd_native_terminal_set_bounds",
        {
          sessionId: "term-session-1",
          bounds: {
            x: 10,
            y: 20,
            width: 800,
            height: 600,
          },
          scaleFactor: 2,
        },
      );
    });

    const primaryRecord = resizeRecords[0];
    expect(primaryRecord.observer.observe).toHaveBeenCalled();
  });

  it("reserves the pane-handle strip at the top of the reported native bounds", async () => {
    // The native compositor view is parented above the WKWebView, so anything
    // inside the reported bounds is painted over. The pane-drag handle only
    // stays visible if that strip is excluded from the surface geometry.
    const session = createSession("term-session-1");
    const paneRect = { x: 10, y: 20, width: 800, height: 600 };
    // The viewport is offset from the pane box by the reserved handle strip, so
    // the browser measures it shorter and lower than its parent.
    // The browser applies the reservation, so both the pane box and the viewport
    // inside it measure below the strip and shorter than the pane slot.
    HTMLElement.prototype.getBoundingClientRect = function () {
      return {
        ...paneRect,
        y: paneRect.y + NATIVE_TERMINAL_HANDLE_INSET_PX,
        height: paneRect.height - NATIVE_TERMINAL_HANDLE_INSET_PX,
        top: paneRect.y + NATIVE_TERMINAL_HANDLE_INSET_PX,
        bottom: paneRect.y + paneRect.height,
        left: paneRect.x,
        right: paneRect.x + paneRect.width,
        toJSON: () => ({}),
      } as DOMRect;
    };

    const { getByTestId } = render(
      <NativeTerminalPane sessionId="term-session-1" session={session} />,
    );

    // The strip is reserved on the pane's own box, so it is not terminal area
    // in the DOM and cannot swallow the press that starts a handle drag ...
    const pane = getByTestId("native-terminal-pane");
    expect(pane.style.marginTop).toBe(`${NATIVE_TERMINAL_HANDLE_INSET_PX}px`);
    expect(pane.style.height).toBe(`calc(100% - ${NATIVE_TERMINAL_HANDLE_INSET_PX}px)`);

    // ... and excluded from the geometry handed to the compositor, so the
    // native surface cannot paint over the handle.
    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_set_bounds", {
        sessionId: "term-session-1",
        bounds: {
          x: paneRect.x,
          y: paneRect.y + NATIVE_TERMINAL_HANDLE_INSET_PX,
          width: paneRect.width,
          height: paneRect.height - NATIVE_TERMINAL_HANDLE_INSET_PX,
        },
        scaleFactor: 2,
      });
    });
  });

  it("reports changed bounds payload once when geometry changes", async () => {
    const session = createSession("term-session-1");

    const { container } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);

    await waitFor(() => {
      expect(resizeRecords.length).toBeGreaterThan(0);
    });

    const observedElement = container.firstElementChild as HTMLElement;
    expect(observedElement).toBeTruthy();

    HTMLElement.prototype.getBoundingClientRect = function () {
      return {
        x: 15,
        y: 25,
        width: 1024,
        height: 768,
        top: 25,
        bottom: 793,
        left: 15,
        right: 1039,
        toJSON: () => ({}),
      } as DOMRect;
    };

    const primaryRecord = resizeRecords[0];
    act(() => {
      primaryRecord.callback(
        [
          {
            target: observedElement,
            contentRect: new DOMRectReadOnly(15, 25, 1024, 768),
            borderBoxSize: [],
            contentBoxSize: [],
            devicePixelContentBoxSize: [],
          } as unknown as ResizeObserverEntry,
        ],
        primaryRecord.observer,
      );
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
        "cmd_native_terminal_set_bounds",
        {
          sessionId: "term-session-1",
          bounds: {
            x: 15,
            y: 25,
            width: 1024,
            height: 768,
          },
          scaleFactor: 2,
        },
      );
    });
  });

  it("coalesces rapid height increases until the prior native resize completes", async () => {
    const session = createSession("term-session-resize-coalesce");
    let resolveInitialBounds!: () => void;
    const initialBounds = new Promise<void>((resolve) => {
      resolveInitialBounds = resolve;
    });
    tauriCoreMocks.invoke.mockImplementation(async (command, args) => {
      if (
        command === "cmd_native_terminal_set_bounds" &&
        args?.bounds.height === 600
      ) {
        await initialBounds;
      }
      return undefined;
    });

    const { container } = render(
      <NativeTerminalPane sessionId={session.id} session={session} />,
    );
    const boundsCalls = () =>
      tauriCoreMocks.invoke.mock.calls.filter(
        ([command]) => command === "cmd_native_terminal_set_bounds",
      );

    await waitFor(() => {
      expect(boundsCalls()).toHaveLength(1);
    });

    const observedElement = container.firstElementChild as HTMLElement;
    const primaryRecord = resizeRecords[0];
    let height = 700;
    HTMLElement.prototype.getBoundingClientRect = function () {
      return {
        x: 10,
        y: 20,
        width: 800,
        height,
        top: 20,
        bottom: 20 + height,
        left: 10,
        right: 810,
        toJSON: () => ({}),
      } as DOMRect;
    };

    act(() => {
      primaryRecord.callback(
        [
          {
            target: observedElement,
            contentRect: new DOMRectReadOnly(10, 20, 800, height),
            borderBoxSize: [],
            contentBoxSize: [],
            devicePixelContentBoxSize: [],
          } as unknown as ResizeObserverEntry,
        ],
        primaryRecord.observer,
      );
      height = 800;
      primaryRecord.callback(
        [
          {
            target: observedElement,
            contentRect: new DOMRectReadOnly(10, 20, 800, height),
            borderBoxSize: [],
            contentBoxSize: [],
            devicePixelContentBoxSize: [],
          } as unknown as ResizeObserverEntry,
        ],
        primaryRecord.observer,
      );
    });

    expect(boundsCalls()).toHaveLength(1);

    resolveInitialBounds();
    await waitFor(() => {
      expect(boundsCalls()).toHaveLength(2);
      expect(boundsCalls()[1]).toEqual([
        "cmd_native_terminal_set_bounds",
        {
          sessionId: "term-session-resize-coalesce",
          bounds: { x: 10, y: 20, width: 800, height: 800 },
          scaleFactor: 2,
        },
      ]);
    });
  });

  it("refreshes the thumb metrics after a height resize changes visible rows", async () => {
    const session = createSession("term-session-resize-scrollbar");
    let metrics = { total: 200, offset: 0, len: 20 };
    tauriCoreMocks.invoke.mockImplementation(async (command) => {
      if (command === "cmd_native_terminal_scrollbar") return metrics;
      return undefined;
    });

    const { container, getByTestId } = render(
      <NativeTerminalPane sessionId={session.id} session={session} />,
    );
    const thumb = await waitFor(() => getByTestId("native-terminal-scrollbar-thumb"));
    expect(thumb).toHaveStyle({ height: "10%" });

    metrics = { total: 200, offset: 0, len: 40 };
    HTMLElement.prototype.getBoundingClientRect = function () {
      return {
        x: 10,
        y: 20,
        width: 800,
        height: 800,
        top: 20,
        bottom: 820,
        left: 10,
        right: 810,
        toJSON: () => ({}),
      } as DOMRect;
    };
    const primaryRecord = resizeRecords[0];
    act(() => {
      primaryRecord.callback(
        [
          {
            target: container.firstElementChild as HTMLElement,
            contentRect: new DOMRectReadOnly(10, 20, 800, 800),
            borderBoxSize: [],
            contentBoxSize: [],
            devicePixelContentBoxSize: [],
          } as unknown as ResizeObserverEntry,
        ],
        primaryRecord.observer,
      );
    });

    await waitFor(() => {
      expect(thumb).toHaveStyle({ height: "20%" });
    });
  });

  it("skips the rejected zero-area measurement and reports the first real one", async () => {
    // A freshly split pane measures zero-area mid-layout. The compositor rejects
    // those dimensions, so the pane must not spend a request on them, and the
    // skipped measurement must not be cached as already-sent, or the pane would
    // stay blank with no bounds and no presentation signal.
    const session = createSession("term-session-1");
    let rect = {
      x: 0,
      y: 0,
      width: 0,
      height: 0,
      top: 0,
      bottom: 0,
      left: 0,
      right: 0,
      toJSON: () => ({}),
    } as DOMRect;
    HTMLElement.prototype.getBoundingClientRect = function () {
      return rect;
    };

    const { container } = render(
      <NativeTerminalPane sessionId="term-session-1" session={session} />,
    );

    const boundsCalls = () =>
      tauriCoreMocks.invoke.mock.calls.filter(
        ([command]) => command === "cmd_native_terminal_set_bounds",
      );

    await waitFor(() => {
      expect(resizeRecords.length).toBeGreaterThan(0);
    });
    expect(boundsCalls()).toHaveLength(0);

    const observedElement = container.firstElementChild as HTMLElement;
    rect = {
      x: 236,
      y: 32,
      width: 522,
      height: 818,
      top: 32,
      bottom: 850,
      left: 236,
      right: 758,
      toJSON: () => ({}),
    } as DOMRect;
    const zeroAreaRecord = resizeRecords[0];
    act(() => {
      zeroAreaRecord.callback(
        [
          {
            target: observedElement,
            contentRect: new DOMRectReadOnly(236, 32, 522, 818),
            borderBoxSize: [],
            contentBoxSize: [],
            devicePixelContentBoxSize: [],
          } as unknown as ResizeObserverEntry,
        ],
        zeroAreaRecord.observer,
      );
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_set_bounds", {
        sessionId: "term-session-1",
        bounds: { x: 236, y: 32, width: 522, height: 818 },
        scaleFactor: 2,
      });
    });
    expect(boundsCalls()).toHaveLength(1);
  });

  it("retries an identical measurement after a rejected bounds request", async () => {
    // The cached geometry means "the compositor already has this". A failed
    // request must clear it, otherwise a retry carrying the same rect is deduped
    // away and the surface never receives bounds.
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const session = createSession("term-session-1");
    let failBounds = true;
    tauriCoreMocks.invoke.mockImplementation(async (command) => {
      if (command === "cmd_native_terminal_set_bounds" && failBounds) {
        throw new Error("invalid dimensions");
      }
      return undefined;
    });

    const { container } = render(
      <NativeTerminalPane sessionId="term-session-1" session={session} />,
    );

    const boundsCalls = () =>
      tauriCoreMocks.invoke.mock.calls.filter(
        ([command]) => command === "cmd_native_terminal_set_bounds",
      );
    await waitFor(() => {
      expect(boundsCalls()).toHaveLength(1);
    });

    failBounds = false;
    const observedElement = container.firstElementChild as HTMLElement;
    const retryRecord = resizeRecords[0];
    act(() => {
      retryRecord.callback(
        [
          {
            target: observedElement,
            contentRect: new DOMRectReadOnly(10, 20, 800, 600),
            borderBoxSize: [],
            contentBoxSize: [],
            devicePixelContentBoxSize: [],
          } as unknown as ResizeObserverEntry,
        ],
        retryRecord.observer,
      );
    });

    await waitFor(() => {
      expect(boundsCalls()).toHaveLength(2);
    });
    consoleSpy.mockRestore();
  });

  it("disconnects ResizeObserver and invokes detach on unmount", async () => {
    const session = createSession("term-session-1");

    const { unmount } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);

    await waitFor(() => {
      expect(resizeRecords.length).toBeGreaterThan(0);
    });
    const primaryRecord = resizeRecords[0];

    unmount();

    expect(primaryRecord.observer.disconnect).toHaveBeenCalled();
    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_detach", {
        sessionId: "term-session-1",
      });
    });
  });

  it("presents the replacement terminal before detaching the outgoing terminal", async () => {
    const firstSession = createSession("term-session-1");
    const secondSession = createSession("term-session-2");
    const { rerender } = render(
      <NativeTerminalPane sessionId={firstSession.id} session={firstSession} />,
    );

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
        "cmd_native_terminal_set_bounds",
        expect.objectContaining({ sessionId: firstSession.backendSessionId }),
      );
    });
    tauriCoreMocks.invoke.mockClear();

    rerender(<NativeTerminalPane sessionId={secondSession.id} session={secondSession} />);

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
        "cmd_native_terminal_set_bounds",
        expect.objectContaining({ sessionId: secondSession.backendSessionId }),
      );
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_detach", {
        sessionId: firstSession.backendSessionId,
      });
    });

    const commands = tauriCoreMocks.invoke.mock.calls.map(([command]) => command);
    expect(commands.indexOf("cmd_native_terminal_set_bounds")).toBeLessThan(
      commands.indexOf("cmd_native_terminal_detach"),
    );
  });

  it("keeps the outgoing terminal until the last rapid replacement is presented", async () => {
    const firstSession = createSession("term-session-1");
    const secondSession = createSession("term-session-2");
    const thirdSession = createSession("term-session-3");
    let resolveSecondBounds!: (value: undefined) => void;
    const secondBounds = new Promise<undefined>((resolve) => {
      resolveSecondBounds = resolve;
    });
    tauriCoreMocks.invoke.mockImplementation(async (command, args) => {
      if (
        command === "cmd_native_terminal_set_bounds" &&
        args?.sessionId === secondSession.backendSessionId
      ) {
        return secondBounds;
      }
      return undefined;
    });

    const { rerender } = render(
      <NativeTerminalPane sessionId={firstSession.id} session={firstSession} />,
    );
    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
        "cmd_native_terminal_set_bounds",
        expect.objectContaining({ sessionId: firstSession.backendSessionId }),
      );
    });
    tauriCoreMocks.invoke.mockClear();

    rerender(<NativeTerminalPane sessionId={secondSession.id} session={secondSession} />);
    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
        "cmd_native_terminal_set_bounds",
        expect.objectContaining({ sessionId: secondSession.backendSessionId }),
      );
    });
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith("cmd_native_terminal_detach", {
      sessionId: firstSession.backendSessionId,
    });

    rerender(<NativeTerminalPane sessionId={thirdSession.id} session={thirdSession} />);
    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
        "cmd_native_terminal_set_bounds",
        expect.objectContaining({ sessionId: thirdSession.backendSessionId }),
      );
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_detach", {
        sessionId: firstSession.backendSessionId,
      });
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_detach", {
        sessionId: secondSession.backendSessionId,
      });
    });

    const commands = tauriCoreMocks.invoke.mock.calls.map(([command]) => command);
    const thirdBoundsIndex = commands.lastIndexOf("cmd_native_terminal_set_bounds");
    expect(thirdBoundsIndex).toBeLessThan(commands.indexOf("cmd_native_terminal_detach"));

    resolveSecondBounds(undefined);
  });

  it("does not report Tauri geometry bounds in remote/web non-Tauri mode", async () => {
    tauriCoreMocks.isTauri.mockReturnValue(false);
    const session = createSession("term-session-1");

    render(<NativeTerminalPane sessionId="term-session-1" session={session} />);

    expect(tauriCoreMocks.invoke).not.toHaveBeenCalled();
  });

  it("converts physical native cursor cells to a high-DPI IME anchor", async () => {
    tauriCoreMocks.invoke.mockResolvedValue({
      cols: 160,
      rows: 60,
      rebuiltRows: 1,
      reusedRows: 59,
      cursorCol: 3,
      cursorRow: 2,
      cellWidthPx: 10,
      cellHeightPx: 20,
    });
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;

    await waitFor(() => {
      expect(textarea.style.left).toBe("15px");
      expect(textarea.style.top).toBe("20px");
      expect(textarea.style.width).toBe("5px");
      expect(textarea.style.height).toBe("10px");
    });
  });
});

describe("NativeTerminalPane focus, keyboard, and IME prototype contract", () => {
  let restorePaneRect: () => void;
  beforeEach(() => {
    restorePaneRect = stubPaneRect();
    tauriCoreMocks.invoke.mockReset();
    tauriCoreMocks.invoke.mockResolvedValue(undefined);
    tauriCoreMocks.isTauri.mockReset();
    tauriCoreMocks.isTauri.mockReturnValue(true);
    vi.stubGlobal("ResizeObserver", MockResizeObserver);
  });

  afterEach(() => {
    restorePaneRect();
    cleanup();
    vi.unstubAllGlobals();
  });

  it("focuses the hidden input sink and notifies native focus after successful attach", async () => {
    const session = createSession("term-session-1", "daemon-pty-123");
    const { getByTestId } = render(
      <NativeTerminalPane sessionId="term-session-1" session={session} />,
    );
    const textarea = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;

    await waitFor(() => {
      expect(document.activeElement).toBe(textarea);
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_set_focus", {
        sessionId: "daemon-pty-123",
        focused: true,
      });
    });
  });

  it("positions the IME input sink at the native cursor receipt", async () => {
    tauriCoreMocks.invoke.mockResolvedValue({
      cols: 80,
      rows: 24,
      rebuiltRows: 1,
      reusedRows: 23,
      cursorCol: 3,
      cursorRow: 2,
      cellWidthPx: 10,
      cellHeightPx: 20,
    });
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;

    await waitFor(() => {
      expect(textarea.style.left).toBe("30px");
      expect(textarea.style.top).toBe("40px");
      expect(textarea.style.width).toBe("10px");
      expect(textarea.style.height).toBe("20px");
    });
  });

  it("forwards a primary pointer press for native selection without cancelling the gesture", async () => {
    tauriCoreMocks.invoke.mockResolvedValue({
      cols: 80,
      rows: 24,
      rebuiltRows: 1,
      reusedRows: 23,
      cursorCol: 3,
      cursorRow: 2,
      cellWidthPx: 10,
      cellHeightPx: 20,
    });
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);

    const textarea = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;
    expect(textarea).toBeInTheDocument();
    expect(textarea.tagName).toBe("TEXTAREA");
    await waitFor(() => {
      expect(textarea.style.width).toBe("10px");
      expect(textarea.style.height).toBe("20px");
    });

    const container = getByTestId("native-terminal-pane");
    tauriCoreMocks.invoke.mockClear();
    const pointerEvent = new MouseEvent("pointerdown", {
      bubbles: true,
      button: 0,
      cancelable: true,
      clientX: 24,
      clientY: 36,
    });

    act(() => {
      container.dispatchEvent(pointerEvent);
    });

    expect(pointerEvent.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(textarea);
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_mouse", {
      sessionId: "term-session-1",
      event: expect.objectContaining({ action: "Press", button: "Left" }),
    });
  });

  it("forwards drag motion and release for native selection without cancelling pointer events", async () => {
    tauriCoreMocks.invoke.mockResolvedValue({
      cols: 80,
      rows: 24,
      rebuiltRows: 1,
      reusedRows: 23,
      cursorCol: 3,
      cursorRow: 2,
      cellWidthPx: 10,
      cellHeightPx: 20,
    });
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;
    await waitFor(() => {
      expect(textarea.style.width).toBe("10px");
      expect(textarea.style.height).toBe("20px");
    });
    const container = getByTestId("native-terminal-pane");
    tauriCoreMocks.invoke.mockClear();

    const press = new MouseEvent("pointerdown", {
      bubbles: true,
      button: 0,
      cancelable: true,
      clientX: 24,
      clientY: 36,
    });
    Object.defineProperty(press, "pointerId", { value: 7 });
    const move = new MouseEvent("pointermove", {
      bubbles: true,
      cancelable: true,
      clientX: 72,
      clientY: 54,
    });
    Object.defineProperty(move, "pointerId", { value: 7 });
    const release = new MouseEvent("pointerup", {
      bubbles: true,
      cancelable: true,
      clientX: 72,
      clientY: 54,
    });
    Object.defineProperty(release, "pointerId", { value: 7 });

    act(() => {
      container.dispatchEvent(press);
      document.dispatchEvent(move);
      document.dispatchEvent(release);
    });

    expect(press.defaultPrevented).toBe(false);
    expect(move.defaultPrevented).toBe(false);
    expect(release.defaultPrevented).toBe(false);
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_mouse", {
      sessionId: "term-session-1",
      event: expect.objectContaining({
        action: "Motion",
        button: null,
        size: expect.objectContaining({ cellWidth: 10, cellHeight: 20 }),
      }),
    });
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_mouse", {
      sessionId: "term-session-1",
      event: expect.objectContaining({ action: "Release", button: null }),
    });
  });

  it("sends typed native focus and blur intent through IPC", () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);

    const textarea = getByTestId("native-terminal-focus-sink");

    act(() => {
      textarea.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    });

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_set_focus", {
      sessionId: "term-session-1",
      focused: true,
    });

    act(() => {
      textarea.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    });

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_set_focus", {
      sessionId: "term-session-1",
      focused: false,
    });
  });

  it("reports rejected native input IPC commands", async () => {
    const session = createSession("term-session-1");
    const error = new Error("native surface unavailable");
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;
    tauriCoreMocks.invoke.mockClear();
    tauriCoreMocks.invoke.mockRejectedValue(error);

    try {
      act(() => {
        textarea.value = "a";
        textarea.dispatchEvent(new Event("input", { bubbles: true }));
      });

      await waitFor(() => {
        expect(consoleErrorSpy).toHaveBeenCalledWith("Native terminal IPC command failed", {
          command: "cmd_native_terminal_send_input",
          error,
        });
      });
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it("self-heals detached session on send_input error by re-attaching and retrying input once", async () => {
    const session = createSession("term-session-self-heal");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-self-heal" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_attach", expect.anything());
    });

    tauriCoreMocks.invoke.mockClear();

    let sendInputAttempts = 0;
    let attachCount = 0;
    tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_send_input") {
        sendInputAttempts++;
        if (sendInputAttempts === 1) {
          throw new Error("NoValue");
        }
        return { cursorCol: 5, cursorRow: 10, cellWidthPx: 8, cellHeightPx: 16 };
      }
      if (cmd === "cmd_native_terminal_attach") {
        attachCount++;
        return undefined;
      }
      return undefined;
    });

    act(() => {
      textarea.value = "x";
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });

    await waitFor(() => {
      expect(sendInputAttempts).toBe(2);
      expect(attachCount).toBe(1);
    });
  });

  it("does not loop infinitely when input retry repeatedly fails", async () => {
    const session = createSession("term-session-retry-fail");
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { getByTestId, findByRole } = render(
      <NativeTerminalPane sessionId="term-session-retry-fail" session={session} />,
    );
    const textarea = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_attach", expect.anything());
    });

    tauriCoreMocks.invoke.mockClear();

    let sendInputAttempts = 0;
    let attachCount = 0;
    tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_send_input") {
        sendInputAttempts++;
        throw new Error("PermanentDetachedError");
      }
      if (cmd === "cmd_native_terminal_attach") {
        attachCount++;
        return undefined;
      }
      return undefined;
    });

    try {
      act(() => {
        textarea.value = "z";
        textarea.dispatchEvent(new Event("input", { bubbles: true }));
      });

      const alert = await findByRole("alert");
      expect(alert).toHaveTextContent("Failed to send terminal input");
      expect(sendInputAttempts).toBe(2);
      expect(attachCount).toBe(1);
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it("encodes and forwards non-printable control keys on keydown without duplicating printable input", () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);

    const textarea = getByTestId("native-terminal-focus-sink");

    // Enter key
    act(() => {
      textarea.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Enter",
          code: "Enter",
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
      sessionId: "term-session-1",
      input: {
        keyEvent: {
          key: "Enter",
          action: "Press",
          modifiers: {
            shift: false,
            ctrl: false,
            alt: false,
            superKey: false,
            capsLock: false,
            numLock: false,
          },
          utf8: null,
        },
      },
    });

    // ArrowUp with Shift modifier
    act(() => {
      textarea.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "ArrowUp",
          code: "ArrowUp",
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
      sessionId: "term-session-1",
      input: {
        keyEvent: {
          key: "ArrowUp",
          action: "Press",
          modifiers: {
            shift: true,
            ctrl: false,
            alt: false,
            superKey: false,
            capsLock: false,
            numLock: false,
          },
          utf8: null,
        },
      },
    });
  });

  it("forwards macOS word-navigation and word-deletion chords instead of letting the focus sink edit itself", () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");

    const chords = [
      { key: "ArrowLeft", altKey: true, expected: { alt: true, superKey: false } },
      { key: "ArrowRight", altKey: true, expected: { alt: true, superKey: false } },
      { key: "ArrowLeft", metaKey: true, expected: { alt: false, superKey: true } },
      { key: "Backspace", altKey: true, expected: { alt: true, superKey: false } },
      { key: "Backspace", metaKey: true, expected: { alt: false, superKey: true } },
      { key: "Delete", altKey: true, expected: { alt: true, superKey: false } },
    ] as const;

    for (const chord of chords) {
      tauriCoreMocks.invoke.mockClear();
      const event = new KeyboardEvent("keydown", {
        key: chord.key,
        code: chord.key,
        altKey: "altKey" in chord ? chord.altKey : false,
        metaKey: "metaKey" in chord ? chord.metaKey : false,
        bubbles: true,
        cancelable: true,
      });

      act(() => {
        textarea.dispatchEvent(event);
      });

      expect(event.defaultPrevented).toBe(true);
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
        sessionId: "term-session-1",
        input: {
          keyEvent: {
            key: chord.key,
            action: "Press",
            modifiers: {
              shift: false,
              ctrl: false,
              alt: chord.expected.alt,
              superKey: chord.expected.superKey,
              capsLock: false,
              numLock: false,
            },
            utf8: null,
          },
        },
      });
    }
  });

  it("recovers the physical key when macOS translates an Option chord into a glyph", () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");

    const chords = [
      { key: "\u0192", code: "KeyF", shiftKey: false, expected: "f" },
      { key: "\u222B", code: "KeyB", shiftKey: false, expected: "b" },
      { key: "\u00C1", code: "KeyF", shiftKey: true, expected: "F" },
      { key: "\u00BA", code: "Digit1", shiftKey: false, expected: "1" },
    ] as const;

    for (const chord of chords) {
      tauriCoreMocks.invoke.mockClear();

      act(() => {
        textarea.dispatchEvent(
          new KeyboardEvent("keydown", {
            key: chord.key,
            code: chord.code,
            altKey: true,
            shiftKey: chord.shiftKey,
            bubbles: true,
            cancelable: true,
          }),
        );
      });

      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
        sessionId: "term-session-1",
        input: {
          keyEvent: {
            key: chord.expected,
            action: "Press",
            modifiers: {
              shift: chord.shiftKey,
              ctrl: false,
              alt: true,
              superKey: false,
              capsLock: false,
              numLock: false,
            },
            utf8: null,
          },
        },
      });
    }
  });

  it("forwards modified character keys through the typed key IPC payload", () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");
    tauriCoreMocks.invoke.mockClear();

    act(() => {
      textarea.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "c",
          ctrlKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
      sessionId: "term-session-1",
      input: {
        keyEvent: {
          key: "c",
          action: "Press",
          modifiers: {
            shift: false,
            ctrl: true,
            alt: false,
            superKey: false,
            capsLock: false,
            numLock: false,
          },
          utf8: null,
        },
      },
    });
  });

  it("sends DOM text paste as one bracketed paste IPC payload", () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");
    tauriCoreMocks.invoke.mockClear();

    const pasteEvent = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(pasteEvent, "clipboardData", {
      value: { getData: () => "pasted text from clipboard" },
    });
    act(() => {
      textarea.dispatchEvent(pasteEvent);
    });

    expect(pasteEvent.defaultPrevented).toBe(true);
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
      sessionId: "term-session-1",
      text: "pasted text from clipboard",
    });
  });

  it("leaves Cmd+V un-cancelled so the DOM paste event handles clipboard data", () => {
    const session = createSession("term-session-dom-paste");
    const read = vi.fn();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { read, readText: vi.fn(), writeText: vi.fn() },
    });
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-dom-paste" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");
    tauriCoreMocks.invoke.mockClear();

    const pasteShortcut = new KeyboardEvent("keydown", {
      key: "v",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });

    act(() => {
      textarea.dispatchEvent(pasteShortcut);
    });

    expect(pasteShortcut.defaultPrevented).toBe(false);
    expect(read).not.toHaveBeenCalled();
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_paste",
      expect.anything(),
    );
  });

  it("forwards a DOM image paste through the agent's Ctrl+V shortcut", () => {
    const session = createSession("term-session-image-dom");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-image-dom" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");
    tauriCoreMocks.invoke.mockClear();

    const pasteEvent = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(pasteEvent, "clipboardData", {
      value: { getData: () => "" },
    });

    act(() => {
      textarea.dispatchEvent(pasteEvent);
    });

    expect(pasteEvent.defaultPrevented).toBe(true);
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
      sessionId: "term-session-image-dom",
      input: {
        keyEvent: {
          key: "v",
          action: "Press",
          modifiers: {
            shift: false,
            ctrl: true,
            alt: false,
            superKey: false,
            capsLock: false,
            numLock: false,
          },
          utf8: null,
        },
      },
    });
  });

  it("forwards a fixture PNG paste at the DOM paste seam through the agent's Ctrl+V shortcut", () => {
    const session = createSession("term-session-fixture-png");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-fixture-png" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");
    tauriCoreMocks.invoke.mockClear();

    const pngBytes = new Uint8Array([
      0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
      0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
      0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
      0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
      0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41,
      0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
      0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00,
      0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
      0x42, 0x60, 0x82,
    ]);
    const file = new File([pngBytes], "c2-image-fixture.png", { type: "image/png" });

    const pasteEvent = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(pasteEvent, "clipboardData", {
      value: {
        types: ["Files", "image/png"],
        files: [file],
        items: [{ kind: "file", type: "image/png", getAsFile: () => file }],
        getData: (type: string) => (type === "text" || type === "text/plain" ? "" : ""),
      },
    });

    act(() => {
      textarea.dispatchEvent(pasteEvent);
    });

    expect(pasteEvent.defaultPrevented).toBe(true);
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
      sessionId: "term-session-fixture-png",
      input: {
        keyEvent: {
          key: "v",
          action: "Press",
          modifiers: {
            shift: false,
            ctrl: true,
            alt: false,
            superKey: false,
            capsLock: false,
            numLock: false,
          },
          utf8: null,
        },
      },
    });
  });


  it("handles onWheel scrolling by issuing native scroll IPC command", async () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const pane = getByTestId("native-terminal-pane");
    tauriCoreMocks.invoke.mockClear();

    act(() => {
      pane.dispatchEvent(new WheelEvent("wheel", { deltaY: 60, bubbles: true }));
    });

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_scroll", {
      sessionId: "term-session-1",
      behavior: { type: "delta", rows: 3 },
    });

    tauriCoreMocks.invoke.mockClear();
    act(() => {
      pane.dispatchEvent(new WheelEvent("wheel", { deltaY: -60, bubbles: true }));
    });

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_scroll", {
      sessionId: "term-session-1",
      behavior: { type: "delta", rows: -3 },
    });
  });

  it("shows, repositions, and drags the native scrollback thumb", async () => {
    const session = createSession("term-session-scrollbar");
    tauriCoreMocks.invoke.mockImplementation(async (command) => {
      if (command === "cmd_native_terminal_scrollbar") {
        return { total: 200, offset: 0, len: 20 };
      }
      return undefined;
    });
    const { getByRole, getByTestId } = render(
      <NativeTerminalPane sessionId="term-session-scrollbar" session={session} />,
    );

    const track = await waitFor(() => getByRole("scrollbar", { name: "Terminal scrollback" }));
    const thumb = getByTestId("native-terminal-scrollbar-thumb");
    expect(track).toHaveAttribute("aria-valuemin", "0");
    expect(track).toHaveAttribute("aria-valuemax", "180");
    expect(track).toHaveAttribute("aria-valuenow", "0");
    expect(thumb).toHaveStyle({ top: "0%", height: "10%" });

    act(() => {
      nativeTerminalEventMocks.scrollbarListener?.({
        sessionId: "term-session-scrollbar",
        total: 200,
        offset: 90,
        len: 20,
      });
    });
    expect(track).toHaveAttribute("aria-valuenow", "90");
    expect(thumb).toHaveStyle({ top: "50%" });

    tauriCoreMocks.invoke.mockClear();
    fireEvent.pointerDown(track, { clientY: 620, pointerId: 7 });
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_scroll", {
      sessionId: "term-session-scrollbar",
      behavior: { type: "row", offset: 180 },
    });

    tauriCoreMocks.invoke.mockClear();
    fireEvent.pointerDown(thumb, { clientY: 290, pointerId: 8 });
    fireEvent.pointerMove(window, { clientY: 620, pointerId: 8 });
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_scroll", {
      sessionId: "term-session-scrollbar",
      behavior: { type: "row", offset: 180 },
    });
    fireEvent.pointerUp(window, { clientY: 620, pointerId: 8 });
  });

  it("copies native selection to navigator.clipboard on platform copy shortcut", async () => {
    const session = createSession("term-session-1");
    const writeTextSpy = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { readText: vi.fn(), writeText: writeTextSpy },
    });

    tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_copy_selection") {
        return "selected native text";
      }
      return undefined;
    });

    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");

    const copyShortcut = new KeyboardEvent("keydown", {
      key: "c",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    act(() => {
      textarea.dispatchEvent(copyShortcut);
    });

    expect(copyShortcut.defaultPrevented).toBe(true);
    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_copy_selection", {
        sessionId: "term-session-1",
      });
      expect(writeTextSpy).toHaveBeenCalledWith("selected native text");
    });
  });

  it("does not send standalone modifier and lock keys through IPC", () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");
    tauriCoreMocks.invoke.mockClear();

    for (const key of ["Shift", "Control", "Alt", "Meta", "CapsLock", "NumLock"]) {
      act(() => {
        textarea.dispatchEvent(
          new KeyboardEvent("keydown", {
            key,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
    }

    expect(tauriCoreMocks.invoke).not.toHaveBeenCalled();
  });

  it("ignores compositionupdate and commits UTF-8 text exactly once at compositionend", () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);

    const textarea = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;

    // Composition start
    act(() => {
      textarea.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "" }));
    });

    // Composition update (in-flight Hangul/CJK syllable)
    act(() => {
      textarea.value = "안";
      textarea.dispatchEvent(new CompositionEvent("compositionupdate", { bubbles: true, data: "안" }));
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });

    // Must NOT have committed input during update
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.anything(),
    );

    // Composition end (committed syllable)
    act(() => {
      textarea.value = "안녕";
      textarea.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "안녕" }));
    });

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
      sessionId: "term-session-1",
      input: {
        text: "안녕",
      },
    });

    // Textarea value should be reset so subsequent inputs don't accumulate
    expect(textarea.value).toBe("");
  });

  it("clears a cancelled composition when focus leaves the input sink", () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;
    tauriCoreMocks.invoke.mockClear();

    act(() => {
      textarea.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
      textarea.value = "안";
      textarea.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    });

    expect(textarea.value).toBe("");

    act(() => {
      textarea.value = "a";
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
      sessionId: "term-session-1",
      input: { text: "a" },
    });
  });

  it("forwards ordinary non-composed printable text on input event and clears the sink", () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);

    const textarea = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;

    act(() => {
      textarea.value = "a";
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
      sessionId: "term-session-1",
      input: {
        text: "a",
      },
    });
    expect(textarea.value).toBe("");
  });

  it("forwards printable character keydown directly and prevents default without firing input event", () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");
    tauriCoreMocks.invoke.mockClear();

    const keyEvent = new KeyboardEvent("keydown", {
      key: "a",
      code: "KeyA",
      bubbles: true,
      cancelable: true,
    });

    act(() => {
      textarea.dispatchEvent(keyEvent);
    });

    expect(keyEvent.defaultPrevented).toBe(true);
    expect(tauriCoreMocks.invoke).toHaveBeenCalledTimes(1);
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
      sessionId: "term-session-1",
      input: {
        text: "a",
      },
    });
  });

  it("includes terminal-host class name on container to allow global shortcut handlers to target terminal", () => {
    const session = createSession("term-session-1");
    const { getByTestId, rerender } = render(
      <NativeTerminalPane sessionId="term-session-1" session={session} />,
    );

    const container = getByTestId("native-terminal-pane");
    expect(container.classList.contains("terminal-host")).toBe(true);

    rerender(
      <NativeTerminalPane
        sessionId="term-session-1"
        session={session}
        className="custom-terminal-pane"
      />,
    );
    expect(container.classList.contains("terminal-host")).toBe(true);
    expect(container.classList.contains("custom-terminal-pane")).toBe(true);
  });

  it("does not forward keyboard events when default has already been prevented by global shortcuts", () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");
    tauriCoreMocks.invoke.mockClear();

    const shortcutEvent = new KeyboardEvent("keydown", {
      key: "t",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    shortcutEvent.preventDefault();

    act(() => {
      textarea.dispatchEvent(shortcutEvent);
    });

    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.anything(),
    );
  });

  it("does not forward split-pane shortcut key when default has been prevented", () => {
    const session = createSession("term-session-1");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-1" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");
    tauriCoreMocks.invoke.mockClear();

    const splitEvent = new KeyboardEvent("keydown", {
      key: "d",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    splitEvent.preventDefault();

    act(() => {
      textarea.dispatchEvent(splitEvent);
    });

    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.anything(),
    );
  });

  it("integrates with useShortcuts: New Tab and Split shortcuts trigger app handlers without leaking to native input", () => {
    const onNewTerminal = vi.fn();
    const onSplitRight = vi.fn();

    function TestAppWithShortcuts() {
      useShortcuts(
        {
          "tab.newTerminal": onNewTerminal,
          "terminal.splitRight": onSplitRight,
        },
        { isMac: true },
      );

      return <NativeTerminalPane sessionId="term-session-1" session={createSession("term-session-1")} />;
    }

    const { getByTestId } = render(<TestAppWithShortcuts />);
    const textarea = getByTestId("native-terminal-focus-sink");
    tauriCoreMocks.invoke.mockClear();

    // Trigger Cmd+T (New Tab shortcut)
    act(() => {
      textarea.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "t",
          code: "KeyT",
          metaKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    expect(onNewTerminal).toHaveBeenCalledTimes(1);
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.anything(),
    );

    // Trigger Cmd+D (Split right shortcut)
    act(() => {
      textarea.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "d",
          code: "KeyD",
          metaKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    expect(onSplitRight).toHaveBeenCalledTimes(1);
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.anything(),
    );
  });
});

describe("NativeTerminalPane daemon and session identity mapping", () => {
  let restorePaneRect: () => void;
  beforeEach(() => {
    restorePaneRect = stubPaneRect();
    tauriCoreMocks.invoke.mockReset();
    tauriCoreMocks.invoke.mockResolvedValue(undefined);
    tauriCoreMocks.isTauri.mockReset();
    tauriCoreMocks.isTauri.mockReturnValue(true);
    resizeRecords.length = 0;
    vi.stubGlobal("ResizeObserver", MockResizeObserver);
  });

  afterEach(() => {
    restorePaneRect();
    cleanup();
    vi.unstubAllGlobals();
  });

  it("routes attach, bounds, focus, input, and detach to backendSessionId when id and backendSessionId intentionally differ", async () => {
    const frontendId = "frontend-pane-leaf-abc";
    const daemonSessionId = "daemon-pty-xyz-999";
    const session = createSession(frontendId, daemonSessionId);

    const { getByTestId, unmount } = render(
      <NativeTerminalPane sessionId={frontendId} session={session} />,
    );

    // 1. Native attach must use backendSessionId
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
      "cmd_native_terminal_attach",
      expect.objectContaining({
        sessionId: daemonSessionId,
      }),
    );
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_attach",
      expect.objectContaining({
        sessionId: frontendId,
      }),
    );

    // 2. Native bounds must use backendSessionId
    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
        "cmd_native_terminal_set_bounds",
        expect.objectContaining({
          sessionId: daemonSessionId,
        }),
      );
    });
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_set_bounds",
      expect.objectContaining({
        sessionId: frontendId,
      }),
    );

    // 3. Native focus must use backendSessionId
    const textarea = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;
    act(() => {
      textarea.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    });
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_set_focus", {
      sessionId: daemonSessionId,
      focused: true,
    });
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith("cmd_native_terminal_set_focus", {
      sessionId: frontendId,
      focused: true,
    });

    // 4. Native input must use backendSessionId
    act(() => {
      textarea.value = "echo hi\n";
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
      sessionId: daemonSessionId,
      input: { text: "echo hi\n" },
    });
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.objectContaining({
        sessionId: frontendId,
      }),
    );

    // 5. Native detach must use backendSessionId
    unmount();
    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_detach", {
        sessionId: daemonSessionId,
      });
    });
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith("cmd_native_terminal_detach", {
      sessionId: frontendId,
    });
  });

  it("preserves fallback behavior when callers only supply sessionId without session object", () => {
    render(<NativeTerminalPane sessionId="legacy-caller-supplied-id" />);

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
      "cmd_native_terminal_attach",
      expect.objectContaining({
        sessionId: "legacy-caller-supplied-id",
      }),
    );
  });

  it("makes no attach, focus, or input IPC calls when session has null backendSessionId, then attaches once upon receiving backendSessionId", async () => {
    const frontendId = "frontend-pane-1";
    const sessionWithoutBackend = createSession(frontendId, null);

    const { getByTestId, rerender } = render(
      <NativeTerminalPane sessionId={frontendId} session={sessionWithoutBackend} />,
    );

    // 1. Native attach and bounds must NOT be called when backendSessionId is null
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith("cmd_native_terminal_attach", expect.anything());
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith("cmd_native_terminal_set_bounds", expect.anything());

    // 2. Native focus must NOT be sent when backendSessionId is null
    const textarea = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;
    act(() => {
      textarea.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    });
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith("cmd_native_terminal_set_focus", expect.anything());

    // 3. Native input must NOT be sent when backendSessionId is null
    act(() => {
      textarea.value = "echo hi\n";
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith("cmd_native_terminal_send_input", expect.anything());

    expect(tauriCoreMocks.invoke).not.toHaveBeenCalled();

    // 4. Rerender with rebound backendSessionId from daemon recovery
    const reboundSession = { ...sessionWithoutBackend, backendSessionId: "daemon-pty-fresh-123" };
    rerender(<NativeTerminalPane sessionId={frontendId} session={reboundSession} />);

    // Now it attaches exactly once with the rebound daemon ID, never with frontendId
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
      "cmd_native_terminal_attach",
      expect.objectContaining({
        sessionId: "daemon-pty-fresh-123",
      }),
    );
    const attachCalls = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd]) => cmd === "cmd_native_terminal_attach",
    );
    expect(attachCalls).toHaveLength(1);
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_attach",
      expect.objectContaining({
        sessionId: frontendId,
      }),
    );
  });

  it("retries mount attach on rejection with exponential backoff and attaches on eventual success", async () => {
    vi.useFakeTimers();
    try {
      const session = createSession("retry-mount-session");
      let attachCount = 0;
      tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
        if (cmd === "cmd_native_terminal_attach") {
          attachCount++;
          if (attachCount < 3) {
            throw new Error("mount attach failed");
          }
          return undefined;
        }
        return undefined;
      });

      const { queryByRole } = render(
        <NativeTerminalPane sessionId="retry-mount-session" session={session} />,
      );

      // Initial attempt (attempt 1)
      await act(async () => {
        await Promise.resolve();
      });
      expect(attachCount).toBe(1);

      // Backoff 1: 250ms -> attempt 2
      await act(async () => {
        await vi.advanceTimersByTimeAsync(250);
      });
      expect(attachCount).toBe(2);

      // Backoff 2: 500ms -> attempt 3 (succeeds)
      await act(async () => {
        await vi.advanceTimersByTimeAsync(500);
      });
      expect(attachCount).toBe(3);

      // Verify no further retries once attached
      await act(async () => {
        await vi.advanceTimersByTimeAsync(5000);
      });
      expect(attachCount).toBe(3);
      expect(queryByRole("alert")).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("redirects printable keydown fallback when activeElement is document.body", async () => {
    const session = createSession("term-session-capture-fallback");
    render(<NativeTerminalPane sessionId="term-session-capture-fallback" session={session} />);

    tauriCoreMocks.invoke.mockClear();
    tauriCoreMocks.invoke.mockResolvedValue(undefined);

    (document.activeElement as HTMLElement)?.blur?.();
    expect(document.activeElement).toBe(document.body);

    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "a",
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
        sessionId: "term-session-capture-fallback",
        input: { text: "a" },
      });
    });
  });

  it("forwards Enter through the keydown fallback when activeElement is document.body", async () => {
    const session = createSession("term-session-fallback-enter");
    render(<NativeTerminalPane sessionId="term-session-fallback-enter" session={session} />);

    tauriCoreMocks.invoke.mockClear();
    tauriCoreMocks.invoke.mockResolvedValue(undefined);

    (document.activeElement as HTMLElement)?.blur?.();
    expect(document.activeElement).toBe(document.body);

    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
      );
    });

    await waitFor(() => {
      const sent = tauriCoreMocks.invoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === "cmd_native_terminal_send_input" &&
          (args as { input?: { keyEvent?: { key?: string } } })?.input?.keyEvent?.key === "Enter",
      );
      expect(sent).toBe(true);
    });
  });

  it("forwards Ctrl+C through the keydown fallback when activeElement is document.body", async () => {
    const session = createSession("term-session-fallback-ctrlc");
    render(<NativeTerminalPane sessionId="term-session-fallback-ctrlc" session={session} />);

    tauriCoreMocks.invoke.mockClear();
    tauriCoreMocks.invoke.mockResolvedValue(undefined);

    (document.activeElement as HTMLElement)?.blur?.();
    expect(document.activeElement).toBe(document.body);

    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "c",
          ctrlKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    await waitFor(() => {
      const sent = tauriCoreMocks.invoke.mock.calls.some(([cmd, args]) => {
        const keyEvent = (args as { input?: { keyEvent?: { key?: string; modifiers?: { ctrl?: boolean } } } })
          ?.input?.keyEvent;
        return (
          cmd === "cmd_native_terminal_send_input" &&
          keyEvent?.key === "c" &&
          keyEvent?.modifiers?.ctrl === true
        );
      });
      expect(sent).toBe(true);
    });
  });

  it("delivers a fallback Enter to exactly one pane when two split panes are mounted", async () => {
    const left = createSession("split-pane-left");
    const right = createSession("split-pane-right");
    render(
      <>
        <NativeTerminalPane sessionId="split-pane-left" session={left} />
        <NativeTerminalPane sessionId="split-pane-right" session={right} />
      </>,
    );

    tauriCoreMocks.invoke.mockClear();
    tauriCoreMocks.invoke.mockResolvedValue(undefined);

    (document.activeElement as HTMLElement)?.blur?.();
    expect(document.activeElement).toBe(document.body);

    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
      );
    });

    await waitFor(() => {
      const sends = tauriCoreMocks.invoke.mock.calls.filter(
        ([cmd]) => cmd === "cmd_native_terminal_send_input",
      );
      expect(sends.length).toBeGreaterThan(0);
    });

    // preventDefault() by the first capture listener must suppress every sibling pane, otherwise
    // one Enter would be delivered to every visible split pane at once.
    const sends = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd]) => cmd === "cmd_native_terminal_send_input",
    );
    expect(sends).toHaveLength(1);
  });

  it("routes a keystroke to the swapped-in session after a workspace switch, never the outgoing one", async () => {
    const outgoing = createSession("switch-session-outgoing");
    const { rerender } = render(
      <NativeTerminalPane sessionId="switch-session-outgoing" session={outgoing} />,
    );
    await waitFor(() => {
      const attached = tauriCoreMocks.invoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === "cmd_native_terminal_attach" &&
          JSON.stringify(args ?? {}).includes("switch-session-outgoing"),
      );
      expect(attached).toBe(true);
    });

    const incoming = createSession("switch-session-incoming");
    rerender(<NativeTerminalPane sessionId="switch-session-incoming" session={incoming} />);
    await waitFor(() => {
      const attached = tauriCoreMocks.invoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === "cmd_native_terminal_attach" &&
          JSON.stringify(args ?? {}).includes("switch-session-incoming"),
      );
      expect(attached).toBe(true);
    });

    tauriCoreMocks.invoke.mockClear();
    (document.activeElement as HTMLElement)?.blur?.();

    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "x", bubbles: true, cancelable: true }),
      );
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
        sessionId: "switch-session-incoming",
        input: { text: "x" },
      });
    });

    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.objectContaining({ sessionId: "switch-session-outgoing" }),
    );
  });
});
