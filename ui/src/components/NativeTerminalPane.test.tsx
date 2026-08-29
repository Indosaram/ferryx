import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TerminalSession } from "../lib/types";
import { resetNativeTerminalLifecycleForTest } from "../lib/nativeTerminalLifecycle";
import { useShortcuts } from "../lib/shortcuts";
import {
  NATIVE_TERMINAL_BOTTOM_INSET_PX,
  NATIVE_TERMINAL_HANDLE_INSET_PX,
  NativeTerminalPane,
  resetNativeTerminalPaneForTest,
} from "./NativeTerminalPane";

const tauriCoreMocks = vi.hoisted(() => ({
  invoke: vi.fn<(cmd: string, args?: any) => Promise<any>>(async () => undefined),
  isTauri: vi.fn(() => true),
}));

const tauriWindowMocks = vi.hoisted(() => {
  let dragDropListeners: Array<(event: { payload: any }) => void> = [];
  const unlisten = vi.fn();
  const onDragDropEvent = vi.fn(async (handler: (event: { payload: any }) => void) => {
    dragDropListeners.push(handler);
    return unlisten;
  });
  return {
    onDragDropEvent,
    unlisten,
    getDragDropListener: () => dragDropListeners.at(-1) ?? null,
    getDragDropListeners: () => [...dragDropListeners],
    reset: () => {
      dragDropListeners = [];
      unlisten.mockClear();
      onDragDropEvent.mockClear();
    },
  };
});

const nativeTerminalEventMocks = vi.hoisted(() => ({
  scrollbarListener: null as ((payload: { sessionId: string; total: number; offset: number; len: number }) => void) | null,
  focusListeners: [] as Array<(sessionId: string) => void>,
  pasteListeners: [] as Array<() => void>,
  onNativeTerminalScrollbar: vi.fn(async (handler: (payload: {
    sessionId: string;
    total: number;
    offset: number;
    len: number;
  }) => void) => {
    nativeTerminalEventMocks.scrollbarListener = handler;
    return () => undefined;
  }),
  onNativeTerminalFocus: vi.fn(async (handler: (sessionId: string) => void) => {
    nativeTerminalEventMocks.focusListeners.push(handler);
    return () => {
      nativeTerminalEventMocks.focusListeners = nativeTerminalEventMocks.focusListeners.filter(
        (candidate) => candidate !== handler,
      );
    };
  }),
  onNativeTerminalPaste: vi.fn(async (handler: () => void) => {
    nativeTerminalEventMocks.pasteListeners.push(handler);
    return () => {
      nativeTerminalEventMocks.pasteListeners = nativeTerminalEventMocks.pasteListeners.filter(
        (candidate) => candidate !== handler,
      );
    };
  }),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCoreMocks.invoke,
  isTauri: tauriCoreMocks.isTauri,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onDragDropEvent: tauriWindowMocks.onDragDropEvent,
  }),
}));

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  onNativeTerminalFocus: nativeTerminalEventMocks.onNativeTerminalFocus,
  onNativeTerminalPaste: nativeTerminalEventMocks.onNativeTerminalPaste,
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
    tauriWindowMocks.reset();
    nativeTerminalEventMocks.scrollbarListener = null;
    nativeTerminalEventMocks.focusListeners = [];
    nativeTerminalEventMocks.pasteListeners = [];
    nativeTerminalEventMocks.onNativeTerminalFocus.mockClear();
    nativeTerminalEventMocks.onNativeTerminalPaste.mockClear();
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

  it("reserves the pane-handle strip at the top and bottom overlay strip in the reported native bounds", async () => {
    // The native compositor view is parented above the WKWebView, so anything
    // inside the reported bounds is painted over. The pane-drag handle only
    // stays visible if that top strip is excluded, and bottom overlays (e.g. DAG indicator)
    // stay visible if the bottom strip is excluded from surface geometry.
    const session = createSession("term-session-1");
    const paneRect = { x: 10, y: 20, width: 800, height: 600 };
    const totalInsetHeight = NATIVE_TERMINAL_HANDLE_INSET_PX + NATIVE_TERMINAL_BOTTOM_INSET_PX;
    // The viewport is offset from the pane box by the reserved handle strip, so
    // the browser measures it shorter and lower than its parent.
    // The browser applies the reservation, so both the pane box and the viewport
    // inside it measure below the strip and shorter than the pane slot.
    HTMLElement.prototype.getBoundingClientRect = function () {
      return {
        ...paneRect,
        y: paneRect.y + NATIVE_TERMINAL_HANDLE_INSET_PX,
        height: paneRect.height - totalInsetHeight,
        top: paneRect.y + NATIVE_TERMINAL_HANDLE_INSET_PX,
        bottom: paneRect.y + paneRect.height - NATIVE_TERMINAL_BOTTOM_INSET_PX,
        left: paneRect.x,
        right: paneRect.x + paneRect.width,
        toJSON: () => ({}),
      } as DOMRect;
    };

    const { getByTestId } = render(
      <NativeTerminalPane sessionId="term-session-1" session={session} />,
    );

    expect(NATIVE_TERMINAL_BOTTOM_INSET_PX).toBe(20);

    // The strip is reserved on the pane's own box, so it is not terminal area
    // in the DOM and cannot swallow the press that starts a handle drag ...
    const pane = getByTestId("native-terminal-pane");
    expect(pane.style.marginTop).toBe(`${NATIVE_TERMINAL_HANDLE_INSET_PX}px`);
    expect(pane.style.height).toBe(`calc(100% - ${totalInsetHeight}px)`);

    // ... and excluded from the geometry handed to the compositor, so the
    // native surface cannot paint over the handle or the bottom overlay strip.
    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_set_bounds", {
        sessionId: "term-session-1",
        bounds: {
          x: paneRect.x,
          y: paneRect.y + NATIVE_TERMINAL_HANDLE_INSET_PX,
          width: paneRect.width,
          height: paneRect.height - totalInsetHeight,
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
    resetNativeTerminalPaneForTest();
    resetNativeTerminalLifecycleForTest();
    restorePaneRect = stubPaneRect();
    tauriCoreMocks.invoke.mockReset();
    tauriCoreMocks.invoke.mockResolvedValue(undefined);
    tauriCoreMocks.isTauri.mockReset();
    tauriCoreMocks.isTauri.mockReturnValue(true);
    tauriWindowMocks.reset();
    vi.stubGlobal("ResizeObserver", MockResizeObserver);
  });

  afterEach(() => {
    restorePaneRect();
    cleanup();
    resetNativeTerminalPaneForTest();
    resetNativeTerminalLifecycleForTest();
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

  it.each([
    { key: "v", code: "KeyV", description: "Latin v" },
    { key: "ㅍ", code: "KeyV", description: "Korean character ㅍ" },
    { key: "Process", code: "KeyV", description: "IME Process key" },
  ])(
    "claims macOS Cmd+V ($description) on focused textarea and routes native text clipboard content without DOM paste",
    async ({ key, code }) => {
      const session = createSession(`term-session-native-paste-${key}`);
      tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
        if (cmd === "cmd_native_terminal_clipboard_content") {
          return { kind: "text", text: "native text from pasteboard" };
        }
        return undefined;
      });

      const { getByTestId } = render(
        <NativeTerminalPane sessionId={`term-session-native-paste-${key}`} session={session} />,
      );
      const textarea = getByTestId("native-terminal-focus-sink");
      textarea.focus();
      tauriCoreMocks.invoke.mockClear();

      const pasteShortcut = new KeyboardEvent("keydown", {
        key,
        code,
        metaKey: true,
        bubbles: true,
        cancelable: true,
      });

      act(() => {
        textarea.dispatchEvent(pasteShortcut);
      });

      expect(pasteShortcut.defaultPrevented).toBe(true);

      await waitFor(() => {
        expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_clipboard_content");
        expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
          sessionId: `term-session-native-paste-${key}`,
          text: "native text from pasteboard",
        });
      });

      const pasteCalls = tauriCoreMocks.invoke.mock.calls.filter(
        ([cmd]) => cmd === "cmd_native_terminal_paste",
      );
      expect(pasteCalls).toHaveLength(1);
    },
  );

  it("claims macOS Cmd+V on focused textarea and routes native image clipboard content through agent Ctrl+V shortcut without DOM paste", async () => {
    const session = createSession("term-session-native-image-paste");
    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_native_terminal_clipboard_content") {
        return { kind: "image" };
      }
      return undefined;
    });

    const { getByTestId } = render(
      <NativeTerminalPane sessionId="term-session-native-image-paste" session={session} />,
    );
    const textarea = getByTestId("native-terminal-focus-sink");
    textarea.focus();
    tauriCoreMocks.invoke.mockClear();

    const pasteShortcut = new KeyboardEvent("keydown", {
      key: "ㅍ",
      code: "KeyV",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });

    act(() => {
      textarea.dispatchEvent(pasteShortcut);
    });

    expect(pasteShortcut.defaultPrevented).toBe(true);

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_clipboard_content");
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
        sessionId: "term-session-native-image-paste",
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

    const pasteCalls = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd]) => cmd === "cmd_native_terminal_paste",
    );
    expect(pasteCalls).toHaveLength(0);
  });

  it("claims Ctrl+V with Korean layout key ㅍ on focused textarea and routes native text clipboard content", async () => {
    const session = createSession("term-session-korean-ctrl-v");
    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_native_terminal_clipboard_content") {
        return { kind: "text", text: "native text from pasteboard" };
      }
      return undefined;
    });

    const { getByTestId } = render(
      <NativeTerminalPane sessionId="term-session-korean-ctrl-v" session={session} />,
    );
    const textarea = getByTestId("native-terminal-focus-sink");
    textarea.focus();
    tauriCoreMocks.invoke.mockClear();

    const pasteShortcut = new KeyboardEvent("keydown", {
      key: "ㅍ",
      code: "KeyV",
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });

    act(() => {
      textarea.dispatchEvent(pasteShortcut);
    });

    expect(pasteShortcut.defaultPrevented).toBe(true);

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_clipboard_content");
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
        sessionId: "term-session-korean-ctrl-v",
        text: "native text from pasteboard",
      });
    });

    const pasteCalls = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd]) => cmd === "cmd_native_terminal_paste",
    );
    expect(pasteCalls).toHaveLength(1);
  });

  it("claims Ctrl+V with Latin v on focused textarea and routes native text clipboard content", async () => {
    const session = createSession("term-session-latin-ctrl-v");
    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_native_terminal_clipboard_content") {
        return { kind: "text", text: "native text from pasteboard" };
      }
      return undefined;
    });

    const { getByTestId } = render(
      <NativeTerminalPane sessionId="term-session-latin-ctrl-v" session={session} />,
    );
    const textarea = getByTestId("native-terminal-focus-sink");
    textarea.focus();
    tauriCoreMocks.invoke.mockClear();

    const pasteShortcut = new KeyboardEvent("keydown", {
      key: "v",
      code: "KeyV",
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });

    act(() => {
      textarea.dispatchEvent(pasteShortcut);
    });

    expect(pasteShortcut.defaultPrevented).toBe(true);

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_clipboard_content");
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
        sessionId: "term-session-latin-ctrl-v",
        text: "native text from pasteboard",
      });
    });

    const pasteCalls = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd]) => cmd === "cmd_native_terminal_paste",
    );
    expect(pasteCalls).toHaveLength(1);
  });

  it("claims Ctrl+V on focused textarea and routes native image clipboard content through agent Ctrl+V shortcut without DOM paste", async () => {
    const session = createSession("term-session-native-image-paste-ctrl-v");
    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_native_terminal_clipboard_content") {
        return { kind: "image" };
      }
      return undefined;
    });

    const { getByTestId } = render(
      <NativeTerminalPane sessionId="term-session-native-image-paste-ctrl-v" session={session} />,
    );
    const textarea = getByTestId("native-terminal-focus-sink");
    textarea.focus();
    tauriCoreMocks.invoke.mockClear();

    const pasteShortcut = new KeyboardEvent("keydown", {
      key: "ㅍ",
      code: "KeyV",
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });

    act(() => {
      textarea.dispatchEvent(pasteShortcut);
    });

    expect(pasteShortcut.defaultPrevented).toBe(true);

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_clipboard_content");
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
        sessionId: "term-session-native-image-paste-ctrl-v",
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

    const pasteCalls = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd]) => cmd === "cmd_native_terminal_paste",
    );
    expect(pasteCalls).toHaveLength(0);
  });

  it("routes AppKit-consumed macOS Cmd+V to the last-focused split terminal exactly once", async () => {
    const leftSession = createSession("term-session-native-menu-paste-left");
    const rightSession = createSession("term-session-native-menu-paste-right");
    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_native_terminal_clipboard_content") {
        return { kind: "image" };
      }
      return undefined;
    });

    const { getAllByTestId } = render(
      <div>
        <NativeTerminalPane sessionId="term-session-native-menu-paste-left" session={leftSession} />
        <NativeTerminalPane sessionId="term-session-native-menu-paste-right" session={rightSession} />
      </div>,
    );
    const [leftSink, rightSink] = getAllByTestId("native-terminal-focus-sink");
    act(() => {
      leftSink.focus();
      rightSink.focus();
    });
    tauriCoreMocks.invoke.mockClear();

    act(() => {
      for (const listener of nativeTerminalEventMocks.pasteListeners) {
        listener();
      }
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_clipboard_content");
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
        sessionId: "term-session-native-menu-paste-right",
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
    const leftInputs = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd, args]) =>
        cmd === "cmd_native_terminal_send_input" &&
        (args as { sessionId?: string })?.sessionId === "term-session-native-menu-paste-left",
    );
    const rightInputs = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd, args]) =>
        cmd === "cmd_native_terminal_send_input" &&
        (args as { sessionId?: string })?.sessionId === "term-session-native-menu-paste-right",
    );
    expect(leftInputs).toHaveLength(0);
    expect(rightInputs).toHaveLength(1);
  });

  it("leaves AppKit-consumed Cmd+V to an external editable instead of a terminal", () => {
    const session = createSession("term-session-native-menu-external-input");
    const { getByTestId } = render(
      <div>
        <input data-testid="external-native-paste-input" />
        <NativeTerminalPane
          sessionId="term-session-native-menu-external-input"
          session={session}
        />
      </div>,
    );
    const input = getByTestId("external-native-paste-input");
    input.focus();
    tauriCoreMocks.invoke.mockClear();

    act(() => {
      for (const listener of nativeTerminalEventMocks.pasteListeners) {
        listener();
      }
    });

    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_clipboard_content",
    );
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_paste",
      expect.anything(),
    );
  });

  it("claims macOS Cmd+V on focused textarea and sends no terminal input when clipboard is empty", async () => {
    const session = createSession("term-session-native-empty-paste");
    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_native_terminal_clipboard_content") {
        return { kind: "empty" };
      }
      return undefined;
    });

    const { getByTestId } = render(
      <NativeTerminalPane sessionId="term-session-native-empty-paste" session={session} />,
    );
    const textarea = getByTestId("native-terminal-focus-sink");
    textarea.focus();
    tauriCoreMocks.invoke.mockClear();

    const pasteShortcut = new KeyboardEvent("keydown", {
      key: "v",
      code: "KeyV",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });

    act(() => {
      textarea.dispatchEvent(pasteShortcut);
    });

    expect(pasteShortcut.defaultPrevented).toBe(true);

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_clipboard_content");
    });

    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_paste",
      expect.anything(),
    );
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.anything(),
    );
  });

  it.each([
    { key: "ㅍ", description: "Korean character ㅍ" },
    { key: "Process", description: "IME Process key" },
  ])(
    "recognizes macOS Korean layout Cmd+V ($description) through neutral document capture fallback, claims focus, and routes native paste without DOM paste",
    async ({ key }) => {
      const session = createSession(`term-session-korean-fallback-${key}`);
      tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
        if (cmd === "cmd_native_terminal_clipboard_content") {
          return { kind: "text", text: "fallback native text" };
        }
        return undefined;
      });

      const { getByTestId } = render(
        <NativeTerminalPane sessionId={`term-session-korean-fallback-${key}`} session={session} />,
      );
      const textarea = getByTestId("native-terminal-focus-sink");

      tauriCoreMocks.invoke.mockClear();
      (document.activeElement as HTMLElement)?.blur?.();
      expect(document.activeElement).toBe(document.body);

      const pasteShortcut = new KeyboardEvent("keydown", {
        key,
        code: "KeyV",
        metaKey: true,
        bubbles: true,
        cancelable: true,
      });

      act(() => {
        document.dispatchEvent(pasteShortcut);
      });

      expect(pasteShortcut.defaultPrevented).toBe(true);
      expect(document.activeElement).toBe(textarea);

      await waitFor(() => {
        expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_clipboard_content");
        expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
          sessionId: `term-session-korean-fallback-${key}`,
          text: "fallback native text",
        });
      });
    },
  );

  it("suppresses a subsequent DOM paste event exactly once after macOS Cmd+V keydown fallback", async () => {
    const session = createSession("term-session-suppress-duplicate-paste");
    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_native_terminal_clipboard_content") {
        return { kind: "text", text: "native text from keydown" };
      }
      return undefined;
    });

    const { getByTestId } = render(
      <NativeTerminalPane sessionId="term-session-suppress-duplicate-paste" session={session} />,
    );
    const textarea = getByTestId("native-terminal-focus-sink");
    textarea.focus();
    tauriCoreMocks.invoke.mockClear();

    // 1. Keydown Cmd+V fires and triggers native clipboard inspection
    const pasteShortcut = new KeyboardEvent("keydown", {
      key: "v",
      code: "KeyV",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });

    act(() => {
      textarea.dispatchEvent(pasteShortcut);
    });

    expect(pasteShortcut.defaultPrevented).toBe(true);

    // 2. WebKit also emits a DOM paste event synchronously / immediately after
    const pasteEvent = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(pasteEvent, "clipboardData", {
      value: { getData: () => "duplicate dom paste text" },
    });

    act(() => {
      textarea.dispatchEvent(pasteEvent);
    });

    // The subsequent DOM paste must be prevented and NOT routed as a second paste
    expect(pasteEvent.defaultPrevented).toBe(true);

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
        sessionId: "term-session-suppress-duplicate-paste",
        text: "native text from keydown",
      });
    });

    const pasteCalls = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd]) => cmd === "cmd_native_terminal_paste",
    );
    expect(pasteCalls).toHaveLength(1);
  });

  it("keeps duplicate-paste suppression through async clipboard inspection until KeyV keyup", async () => {
    const session = createSession("term-session-async-suppress-duplicate-paste");
    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_native_terminal_clipboard_content") {
        return { kind: "text", text: "native async text" };
      }
      return undefined;
    });

    const { getByTestId } = render(
      <NativeTerminalPane
        sessionId="term-session-async-suppress-duplicate-paste"
        session={session}
      />,
    );
    const textarea = getByTestId("native-terminal-focus-sink");
    textarea.focus();
    tauriCoreMocks.invoke.mockClear();

    const pasteShortcut = new KeyboardEvent("keydown", {
      key: "ㅍ",
      code: "KeyV",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });

    act(() => {
      textarea.dispatchEvent(pasteShortcut);
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
        sessionId: "term-session-async-suppress-duplicate-paste",
        text: "native async text",
      });
    });

    const pasteEvent = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(pasteEvent, "clipboardData", {
      value: { getData: () => "late duplicate DOM paste" },
    });

    act(() => {
      textarea.dispatchEvent(pasteEvent);
    });

    expect(pasteEvent.defaultPrevented).toBe(true);
    const pasteCallsBeforeKeyup = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd]) => cmd === "cmd_native_terminal_paste",
    );
    expect(pasteCallsBeforeKeyup).toHaveLength(1);

    act(() => {
      textarea.dispatchEvent(
        new KeyboardEvent("keyup", {
          key: "ㅍ",
          code: "KeyV",
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    const laterPasteEvent = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(laterPasteEvent, "clipboardData", {
      value: { getData: () => "independent later paste" },
    });

    act(() => {
      textarea.dispatchEvent(laterPasteEvent);
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
        sessionId: "term-session-async-suppress-duplicate-paste",
        text: "independent later paste",
      });
    });
  });

  it("clears native paste suppression when the window loses focus before KeyV keyup", async () => {
    const session = createSession("term-session-blur-clears-paste-suppression");
    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_native_terminal_clipboard_content") {
        return { kind: "empty" };
      }
      return undefined;
    });

    const { getByTestId } = render(
      <NativeTerminalPane
        sessionId="term-session-blur-clears-paste-suppression"
        session={session}
      />,
    );
    const textarea = getByTestId("native-terminal-focus-sink");
    textarea.focus();
    tauriCoreMocks.invoke.mockClear();

    act(() => {
      textarea.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "ㅍ",
          code: "KeyV",
          metaKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
        "cmd_native_terminal_clipboard_content",
      );
    });

    act(() => {
      window.dispatchEvent(new Event("blur"));
    });

    const independentPaste = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(independentPaste, "clipboardData", {
      value: { getData: () => "paste after window blur" },
    });

    act(() => {
      textarea.dispatchEvent(independentPaste);
    });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
        sessionId: "term-session-blur-clears-paste-suppression",
        text: "paste after window blur",
      });
    });
  });

  it("routes neutral BODY Cmd+V native clipboard fallback only to the last-focused split terminal", async () => {
    const leftSession = createSession("term-session-split-left");
    const rightSession = createSession("term-session-split-right");
    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_native_terminal_clipboard_content") {
        return { kind: "text", text: "split targeted native text" };
      }
      return undefined;
    });

    const { getAllByTestId } = render(
      <div>
        <NativeTerminalPane sessionId="term-session-split-left" session={leftSession} />
        <NativeTerminalPane sessionId="term-session-split-right" session={rightSession} />
      </div>,
    );

    const [leftSink, rightSink] = getAllByTestId("native-terminal-focus-sink");

    // Focus left, then focus right so right becomes last focused
    act(() => {
      leftSink.focus();
    });
    act(() => {
      rightSink.focus();
    });

    // Blur to neutral BODY
    act(() => {
      (document.activeElement as HTMLElement)?.blur?.();
    });
    expect(document.activeElement).toBe(document.body);
    tauriCoreMocks.invoke.mockClear();

    const pasteShortcut = new KeyboardEvent("keydown", {
      key: "ㅍ",
      code: "KeyV",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });

    act(() => {
      document.dispatchEvent(pasteShortcut);
    });

    expect(pasteShortcut.defaultPrevented).toBe(true);

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
        sessionId: "term-session-split-right",
        text: "split targeted native text",
      });
    });

    const leftPastes = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd, args]) =>
        cmd === "cmd_native_terminal_paste" &&
        (args as { sessionId?: string })?.sessionId === "term-session-split-left",
    );
    expect(leftPastes).toHaveLength(0);
  });

  it("does not claim macOS Cmd+V when an external editable input is focused", () => {
    const session = createSession("term-session-external-input");
    const { getByTestId } = render(
      <div>
        <input data-testid="external-search-input" />
        <NativeTerminalPane sessionId="term-session-external-input" session={session} />
      </div>,
    );

    const input = getByTestId("external-search-input");
    input.focus();
    expect(document.activeElement).toBe(input);
    tauriCoreMocks.invoke.mockClear();

    const pasteShortcut = new KeyboardEvent("keydown", {
      key: "v",
      code: "KeyV",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });

    act(() => {
      document.dispatchEvent(pasteShortcut);
    });

    expect(pasteShortcut.defaultPrevented).toBe(false);
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_clipboard_content",
    );
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_paste",
      expect.anything(),
    );
  });

  it("claims native fallback on non-macOS platforms for Ctrl+V", async () => {
    const originalPlatform = navigator.platform;
    const originalUserAgent = navigator.userAgent;
    try {
      Object.defineProperty(navigator, "platform", {
        value: "Linux x86_64",
        configurable: true,
      });
      Object.defineProperty(navigator, "userAgent", {
        value: "Mozilla/5.0 (X11; Linux x86_64)",
        configurable: true,
      });

      const session = createSession("term-session-linux-ctrl-v");
      tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
        if (cmd === "cmd_native_terminal_clipboard_content") {
          return { kind: "text", text: "linux text from pasteboard" };
        }
        return undefined;
      });

      const { getByTestId } = render(
        <NativeTerminalPane sessionId="term-session-linux-ctrl-v" session={session} />,
      );
      const textarea = getByTestId("native-terminal-focus-sink");
      textarea.focus();
      tauriCoreMocks.invoke.mockClear();

      const ctrlVShortcut = new KeyboardEvent("keydown", {
        key: "v",
        code: "KeyV",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      });

      act(() => {
        textarea.dispatchEvent(ctrlVShortcut);
      });

      expect(ctrlVShortcut.defaultPrevented).toBe(true);

      await waitFor(() => {
        expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_clipboard_content");
        expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
          sessionId: "term-session-linux-ctrl-v",
          text: "linux text from pasteboard",
        });
      });
    } finally {
      Object.defineProperty(navigator, "platform", {
        value: originalPlatform,
        configurable: true,
      });
      Object.defineProperty(navigator, "userAgent", {
        value: originalUserAgent,
        configurable: true,
      });
    }
  });

  it("ignores standalone Meta key before Korean Cmd+V (ㅍ) across capture fallback and textarea", async () => {
    const session = createSession("term-session-korean-meta-sequence");
    tauriCoreMocks.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cmd_native_terminal_clipboard_content") {
        return { kind: "text", text: "korean meta sequence text" };
      }
      return undefined;
    });

    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-korean-meta-sequence" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");

    (document.activeElement as HTMLElement)?.blur?.();
    expect(document.activeElement).toBe(document.body);
    tauriCoreMocks.invoke.mockClear();

    // 1. Standalone Meta keydown on neutral BODY
    const metaEvent = new KeyboardEvent("keydown", {
      key: "Meta",
      code: "MetaLeft",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    act(() => {
      document.dispatchEvent(metaEvent);
    });

    expect(metaEvent.defaultPrevented).toBe(false);
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalled();

    // 2. Korean Cmd+V keydown on document
    const pasteShortcut = new KeyboardEvent("keydown", {
      key: "ㅍ",
      code: "KeyV",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    act(() => {
      document.dispatchEvent(pasteShortcut);
    });

    expect(pasteShortcut.defaultPrevented).toBe(true);
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith("cmd_native_terminal_send_input", expect.anything());
    expect(document.activeElement).toBe(textarea);

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
        sessionId: "term-session-korean-meta-sequence",
        text: "korean meta sequence text",
      });
    });
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

  it("routes an image-only paste targeting the terminal pane when the focus sink is not active exclusively through Ctrl+V exactly once", () => {
    const session = createSession("term-session-image-pane-target");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-image-pane-target" session={session} />);
    const pane = getByTestId("native-terminal-pane");
    const textarea = getByTestId("native-terminal-focus-sink");
    // Ensure the textarea sink is not focused
    textarea.blur();
    expect(document.activeElement).not.toBe(textarea);
    tauriCoreMocks.invoke.mockClear();

    const imageFile = new File([new Uint8Array([137, 80, 78, 71])], "screenshot.png", { type: "image/png" });
    const pasteEvent = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(pasteEvent, "clipboardData", {
      value: {
        types: ["image/png"],
        files: [imageFile],
        items: [{ kind: "file", type: "image/png", getAsFile: () => imageFile }],
        getData: (format: string) => (format === "text" || format === "text/plain" ? "" : ""),
      },
    });

    act(() => {
      pane.dispatchEvent(pasteEvent);
    });

    expect(pasteEvent.defaultPrevented).toBe(true);
    const sendInputCalls = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd]) => cmd === "cmd_native_terminal_send_input",
    );
    expect(sendInputCalls).toHaveLength(1);
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
      sessionId: "term-session-image-pane-target",
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
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_paste",
      expect.anything(),
    );
  });

  it("routes a text paste targeting the terminal pane when the focus sink is not active to cmd_native_terminal_paste", () => {
    const session = createSession("term-session-text-pane-target");
    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-text-pane-target" session={session} />);
    const pane = getByTestId("native-terminal-pane");
    const textarea = getByTestId("native-terminal-focus-sink");
    textarea.blur();
    expect(document.activeElement).not.toBe(textarea);
    tauriCoreMocks.invoke.mockClear();

    const pasteEvent = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(pasteEvent, "clipboardData", {
      value: {
        types: ["text/plain"],
        getData: () => "echo 'hello world'",
      },
    });

    act(() => {
      pane.dispatchEvent(pasteEvent);
    });

    expect(pasteEvent.defaultPrevented).toBe(true);
    const pasteCalls = tauriCoreMocks.invoke.mock.calls.filter(
      ([cmd]) => cmd === "cmd_native_terminal_paste",
    );
    expect(pasteCalls).toHaveLength(1);
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
      sessionId: "term-session-text-pane-target",
      text: "echo 'hello world'",
    });
  });

  it("routes a body-target image paste only to the last-focused split terminal", () => {
    const leftSession = createSession("split-image-left", "daemon-image-left");
    const rightSession = createSession("split-image-right", "daemon-image-right");
    const { getAllByTestId } = render(
      <>
        <NativeTerminalPane sessionId="split-image-left" session={leftSession} />
        <NativeTerminalPane sessionId="split-image-right" session={rightSession} />
      </>,
    );
    const [, rightSink] = getAllByTestId("native-terminal-focus-sink");

    act(() => {
      rightSink.focus();
      rightSink.blur();
    });
    expect(document.activeElement).toBe(document.body);
    tauriCoreMocks.invoke.mockClear();

    const pasteEvent = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(pasteEvent, "clipboardData", {
      value: { types: ["Files", "image/png"], getData: () => "" },
    });

    act(() => {
      document.body.dispatchEvent(pasteEvent);
    });

    const sends = tauriCoreMocks.invoke.mock.calls.filter(
      ([command]) => command === "cmd_native_terminal_send_input",
    );
    expect(sends).toHaveLength(1);
    expect(sends[0]?.[1]).toMatchObject({ sessionId: "daemon-image-right" });
  });

  it("routes neutral BODY keyboard input only to the last-focused split terminal", () => {
    const leftSession = createSession("split-key-left", "daemon-key-left");
    const rightSession = createSession("split-key-right", "daemon-key-right");
    const { getAllByTestId } = render(
      <>
        <NativeTerminalPane sessionId="split-key-left" session={leftSession} />
        <NativeTerminalPane sessionId="split-key-right" session={rightSession} />
      </>,
    );
    const [, rightSink] = getAllByTestId("native-terminal-focus-sink");

    act(() => {
      rightSink.focus();
      rightSink.blur();
    });
    expect(document.activeElement).toBe(document.body);
    tauriCoreMocks.invoke.mockClear();

    act(() => {
      document.body.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "x",
          code: "KeyX",
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    const sends = tauriCoreMocks.invoke.mock.calls.filter(
      ([command]) => command === "cmd_native_terminal_send_input",
    );
    expect(sends).toHaveLength(1);
    expect(sends[0]?.[1]).toMatchObject({
      sessionId: "daemon-key-right",
      input: { text: "x" },
    });
  });

  it("gives a pane-target paste precedence over a previously focused sibling", () => {
    const leftSession = createSession("split-target-left", "daemon-target-left");
    const rightSession = createSession("split-target-right", "daemon-target-right");
    const { getAllByTestId } = render(
      <>
        <NativeTerminalPane sessionId="split-target-left" session={leftSession} />
        <NativeTerminalPane sessionId="split-target-right" session={rightSession} />
      </>,
    );
    const [leftSink] = getAllByTestId("native-terminal-focus-sink");
    const [, rightPane] = getAllByTestId("native-terminal-pane");

    act(() => {
      leftSink.focus();
      leftSink.blur();
    });
    tauriCoreMocks.invoke.mockClear();

    const pasteEvent = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(pasteEvent, "clipboardData", {
      value: { types: ["text/plain"], getData: () => "targeted paste" },
    });

    act(() => {
      rightPane.dispatchEvent(pasteEvent);
    });

    const pastes = tauriCoreMocks.invoke.mock.calls.filter(
      ([command]) => command === "cmd_native_terminal_paste",
    );
    expect(pastes).toHaveLength(1);
    expect(pastes[0]?.[1]).toEqual({
      sessionId: "daemon-target-right",
      text: "targeted paste",
    });
  });

  it("lets the sole remaining pane claim BODY paste after the previous owner unmounts", () => {
    const leftSession = createSession("split-unmount-left", "daemon-unmount-left");
    const rightSession = createSession("split-unmount-right", "daemon-unmount-right");
    const { getAllByTestId, rerender } = render(
      <>
        <NativeTerminalPane sessionId="split-unmount-left" session={leftSession} />
        <NativeTerminalPane sessionId="split-unmount-right" session={rightSession} />
      </>,
    );
    const [, rightSink] = getAllByTestId("native-terminal-focus-sink");

    act(() => {
      rightSink.focus();
      rightSink.blur();
    });
    rerender(<NativeTerminalPane sessionId="split-unmount-left" session={leftSession} />);
    tauriCoreMocks.invoke.mockClear();

    const pasteEvent = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(pasteEvent, "clipboardData", {
      value: { types: ["Files", "image/png"], getData: () => "" },
    });

    act(() => {
      document.body.dispatchEvent(pasteEvent);
    });

    const sends = tauriCoreMocks.invoke.mock.calls.filter(
      ([command]) => command === "cmd_native_terminal_send_input",
    );
    expect(sends).toHaveLength(1);
    expect(sends[0]?.[1]).toMatchObject({ sessionId: "daemon-unmount-left" });
  });

  it("uses half-open native drop bounds so a split divider belongs to only the right pane", async () => {
    const leftSession = createSession("split-drop-left", "daemon-drop-left");
    const rightSession = createSession("split-drop-right", "daemon-drop-right");
    const { getAllByTestId } = render(
      <>
        <NativeTerminalPane sessionId="split-drop-left" session={leftSession} />
        <NativeTerminalPane sessionId="split-drop-right" session={rightSession} />
      </>,
    );
    const [leftPane, rightPane] = getAllByTestId("native-terminal-pane");
    leftPane.getBoundingClientRect = () => ({
      x: 0, y: 0, width: 400, height: 600,
      top: 0, bottom: 600, left: 0, right: 400,
      toJSON: () => ({}),
    }) as DOMRect;
    rightPane.getBoundingClientRect = () => ({
      x: 400, y: 0, width: 400, height: 600,
      top: 0, bottom: 600, left: 400, right: 800,
      toJSON: () => ({}),
    }) as DOMRect;

    await waitFor(() => {
      expect(tauriWindowMocks.getDragDropListeners()).toHaveLength(2);
    });
    tauriCoreMocks.invoke.mockClear();

    act(() => {
      for (const listener of tauriWindowMocks.getDragDropListeners()) {
        listener({
          payload: {
            type: "drop",
            paths: ["/Users/indo/divider file.txt"],
            position: { x: 400, y: 100 },
          },
        });
      }
    });

    const pastes = tauriCoreMocks.invoke.mock.calls.filter(
      ([command]) => command === "cmd_native_terminal_paste",
    );
    expect(pastes).toHaveLength(1);
    expect(pastes[0]?.[1]).toEqual({
      sessionId: "daemon-drop-right",
      text: "'/Users/indo/divider file.txt'",
    });
  });

  it("subscribes to Tauri onDragDropEvent, atomically pasting dropped files inside bounds and ignoring drops outside bounds", async () => {
    const originalDpr = window.devicePixelRatio;
    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: 2,
    });

    try {
      const session = createSession("term-session-tauri-dnd");
      const { unmount } = render(<NativeTerminalPane sessionId="term-session-tauri-dnd" session={session} />);

      await waitFor(() => {
        expect(tauriWindowMocks.onDragDropEvent).toHaveBeenCalled();
      });

      const listener = tauriWindowMocks.getDragDropListener();
      expect(listener).not.toBeNull();
      tauriCoreMocks.invoke.mockClear();

      // 1. Point that would look inside if unscaled (physical x: 15, y: 35), but at DPR=2 is
      // logical (7.5, 17.5) which is outside container bounds (left: 10, top: 32). Must be ignored.
      act(() => {
        listener?.({
          payload: {
            type: "drop",
            paths: ["/Users/indo/Outside/file.txt"],
            position: { x: 15, y: 35 },
          },
        });
      });

      expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
        "cmd_native_terminal_paste",
        expect.anything(),
      );

      // 2. Event inside pane bounds -> physical (100, 100) -> logical (50, 50), inside bounds [10..810, 32..620],
      // atomically pasted with shell-safe quoting
      act(() => {
        listener?.({
          payload: {
            type: "drop",
            paths: ["/Users/indo/Documents/project report.pdf", "/Users/indo/file2.txt"],
            position: { x: 100, y: 100 },
          },
        });
      });

      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_paste", {
        sessionId: "term-session-tauri-dnd",
        text: "'/Users/indo/Documents/project report.pdf' /Users/indo/file2.txt",
      });

      // 3. Unmount -> invokes unlisten callback
      unmount();
      expect(tauriWindowMocks.unlisten).toHaveBeenCalled();
    } finally {
      Object.defineProperty(window, "devicePixelRatio", {
        configurable: true,
        value: originalDpr,
      });
    }
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

  it("copies native selection on macOS Korean layout Cmd+C (ㅊ and Process)", async () => {
    const session = createSession("term-session-korean-copy");
    const writeTextSpy = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { readText: vi.fn(), writeText: writeTextSpy },
    });

    tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_copy_selection") {
        return "selected korean copy text";
      }
      return undefined;
    });

    const { getByTestId } = render(<NativeTerminalPane sessionId="term-session-korean-copy" session={session} />);
    const textarea = getByTestId("native-terminal-focus-sink");

    const copyShortcut = new KeyboardEvent("keydown", {
      key: "ㅊ",
      code: "KeyC",
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
        sessionId: "term-session-korean-copy",
      });
      expect(writeTextSpy).toHaveBeenCalledWith("selected korean copy text");
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

  it("preserves native mouse-up focus after WebKit mouse-down handling moves focus to BODY", async () => {
    let deferredFocus: FrameRequestCallback | undefined;
    const requestFrame = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback) => {
        deferredFocus = callback;
        return 1;
      });
    const cancelFrame = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => undefined);
    const leftSession = createSession("native-focus-left", "daemon-native-focus-left");
    const rightSession = createSession("native-focus-right", "daemon-native-focus-right");
    const { getAllByTestId } = render(
      <>
        <NativeTerminalPane sessionId="native-focus-left" session={leftSession} />
        <NativeTerminalPane sessionId="native-focus-right" session={rightSession} />
      </>,
    );
    const [leftSink, rightSink] = getAllByTestId("native-terminal-focus-sink");

    await waitFor(() => {
      expect(nativeTerminalEventMocks.focusListeners).toHaveLength(2);
    });
    leftSink.focus();
    expect(document.activeElement).toBe(leftSink);

    act(() => {
      leftSink.blur();
    });
    expect(document.activeElement).toBe(document.body);

    act(() => {
      for (const listener of nativeTerminalEventMocks.focusListeners) {
        listener("daemon-native-focus-right");
      }
    });
    expect(document.activeElement).toBe(rightSink);

    act(() => {
      deferredFocus?.(performance.now());
    });
    expect(document.activeElement).toBe(rightSink);
    requestFrame.mockRestore();
    cancelFrame.mockRestore();
  });

  it("lets immediate Hangul input start composition after a native pane switch", async () => {
    const session = createSession("native-ime-switch", "daemon-native-ime-switch");
    const { getByTestId } = render(
      <NativeTerminalPane sessionId="native-ime-switch" session={session} />,
    );
    const sink = getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;
    await waitFor(() => {
      expect(nativeTerminalEventMocks.focusListeners).toHaveLength(1);
    });

    act(() => {
      sink.blur();
      for (const listener of nativeTerminalEventMocks.focusListeners) {
        listener("daemon-native-ime-switch");
      }
    });
    tauriCoreMocks.invoke.mockClear();

    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "ㄱ",
          code: "KeyR",
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    expect(document.activeElement).toBe(sink);
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.anything(),
    );

    act(() => {
      fireEvent.compositionStart(sink);
      sink.value = "가";
      fireEvent.compositionEnd(sink, { data: "가" });
    });

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_send_input", {
      sessionId: "daemon-native-ime-switch",
      input: { text: "가" },
    });
  });

});
