import { JSDOM } from "jsdom";

if (typeof window === "undefined") {
  const dom = new JSDOM("<!DOCTYPE html><html><body></body></html>", { url: "http://localhost:3000" });
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  globalThis.navigator = dom.window.navigator;
  globalThis.HTMLElement = dom.window.HTMLElement;
  globalThis.HTMLInputElement = dom.window.HTMLInputElement;
  globalThis.HTMLTextAreaElement = dom.window.HTMLTextAreaElement;
  globalThis.MutationObserver = dom.window.MutationObserver;
  globalThis.Element = dom.window.Element;
  globalThis.Node = dom.window.Node;
  globalThis.Event = dom.window.Event;
  globalThis.CustomEvent = dom.window.CustomEvent;
  globalThis.MouseEvent = dom.window.MouseEvent;
  globalThis.KeyboardEvent = dom.window.KeyboardEvent;
  globalThis.dispatchEvent = dom.window.dispatchEvent.bind(dom.window);
  globalThis.addEventListener = dom.window.addEventListener.bind(dom.window);
  globalThis.removeEventListener = dom.window.removeEventListener.bind(dom.window);
}

const { StrictMode } = await import("react");
const { act, cleanup, fireEvent, render, waitFor } = await import("@testing-library/react");
const { afterEach, beforeEach, describe, expect, it, vi } = await import("vitest");
await import("../test/setup");

const { NativeTerminalVisibilityProvider } = await import("../lib/nativeTerminalVisibility");
const { NativeTerminalPane, resetNativeTerminalPaneForTest } = await import("./NativeTerminalPane");
const { resetNativeTerminalLifecycleForTest } = await import("../lib/nativeTerminalLifecycle");
import type { TerminalSession } from "../lib/types";

const tauriInvoke = vi.fn<(cmd: string, args?: any) => Promise<any>>(async () => undefined);
const tauriIsTauri = vi.fn(() => true);
const tauriListen = vi.fn<(event?: string, handler?: any) => Promise<any>>(async () => () => undefined);

const tauriCoreMocks = {
  invoke: tauriInvoke,
  isTauri: tauriIsTauri,
  listen: tauriListen,
};

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: any) => tauriInvoke(cmd, args),
  isTauri: () => tauriIsTauri(),
}));

// isTauri() is mocked true, so event subscriptions pass their runtime guard and
// would reach the real bridge, which has no __TAURI_INTERNALS__ under jsdom.
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: any) => tauriListen(event, handler),
}));

class TestResizeObserver implements ResizeObserver {
  readonly observe = vi.fn();
  readonly unobserve = vi.fn();
  readonly disconnect = vi.fn();

  constructor(readonly callback: ResizeObserverCallback) {}
}

function session(id: string): TerminalSession {
  return {
    id: `frontend-${id}`,
    cwd: "/workspace/orca-lite",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId: id,
    lifecycle: "working",
  };
}

function lifecycleCalls(): Array<[string, string | undefined]> {
  return tauriCoreMocks.invoke.mock.calls
    .filter(([cmd]) => cmd === "cmd_native_terminal_attach" || cmd === "cmd_native_terminal_detach")
    .map(([cmd, args]) => [cmd, args?.sessionId]);
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

function deferred<T = void>() {
  let resolve = (_value: T) => {};
  let reject = (_error: Error) => {};
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

const PRESENTED = {
  presented: true, cursorCol: 2, cursorRow: 3, cellWidthPx: 10, cellHeightPx: 20,
};

function inputRecoveryBoundary() {
  const inputStarted = deferred();
  const input = deferred<typeof PRESENTED>();
  const boundsStarted = deferred();
  const bounds = deferred<typeof PRESENTED>();
  const detached = deferred();
  const recoveryStarted = deferred();
  const recovery = deferred();
  let holdRecovery = false;
  let attaches = 0;
  let inputs = 0;
  tauriInvoke.mockImplementation(async (command, args) => {
    if (command === "cmd_native_terminal_set_bounds") {
      if (args.sessionId === "owner-b") {
        boundsStarted.resolve();
        return bounds.promise;
      }
      return PRESENTED;
    }
    if (command === "cmd_native_terminal_attach" && args.sessionId === "owner-a") {
      if (++attaches > 1) {
        recoveryStarted.resolve();
        if (holdRecovery) return recovery.promise;
      }
    }
    if (command === "cmd_native_terminal_send_input" && args.sessionId === "owner-a") {
      if (++inputs === 1) {
        inputStarted.resolve();
        return input.promise;
      }
      return { ...PRESENTED, cursorCol: 99 };
    }
    if (command === "cmd_native_terminal_detach" && args.sessionId === "owner-a") detached.resolve();
    return undefined;
  });
  return {
    inputStarted, input, boundsStarted, bounds, detached, recoveryStarted, recovery,
    holdRecovery: () => { holdRecovery = true; },
  };
}

async function startPendingInput(view: ReturnType<typeof render>, boundary: ReturnType<typeof inputRecoveryBoundary>) {
  await act(async () => {
    fireEvent.input(view.getByTestId("native-terminal-focus-sink"), { target: { value: "x" } });
    await boundary.inputStarted.promise;
  });
}

describe("NativeTerminalPane compositor ownership lifecycle", () => {
  let restorePaneRect: () => void;
  const originalResizeObserver = globalThis.ResizeObserver;

  beforeEach(() => {
    resetNativeTerminalLifecycleForTest();
    resetNativeTerminalPaneForTest();
    const originalRect = HTMLElement.prototype.getBoundingClientRect;
    HTMLElement.prototype.getBoundingClientRect = function () {
      return PANE_RECT;
    };
    restorePaneRect = () => {
      HTMLElement.prototype.getBoundingClientRect = originalRect;
    };
    tauriCoreMocks.invoke.mockReset();
    tauriCoreMocks.invoke.mockResolvedValue(undefined);
    tauriCoreMocks.isTauri.mockReset();
    tauriCoreMocks.isTauri.mockReturnValue(true);
    Object.defineProperty(globalThis, "ResizeObserver", {
      configurable: true,
      writable: true,
      value: TestResizeObserver,
    });
    document.querySelectorAll('[role="dialog"], [role="search"]').forEach((node) => node.remove());
  });

  afterEach(async () => {
    restorePaneRect();
    await act(async () => { cleanup(); });
    Object.defineProperty(globalThis, "ResizeObserver", {
      configurable: true,
      writable: true,
      value: originalResizeObserver,
    });
    if (typeof vi.unstubAllGlobals === "function") {
      vi.unstubAllGlobals();
    }
  });

  it.each([
    ["uses effective native scale for IME anchor at fractional webview density", 1.5, 2, 16, 48, 8, 16],
    ["keeps ordinary equal-scale IME anchors", 2, 2, 16, 48, 8, 16],
    ["falls back to raw density for legacy IME receipts", 1.5, undefined, 32 / 1.5, 64, 16 / 1.5, 32 / 1.5],
    ["falls back to raw density when native geometry is absent", 1.5, null, 32 / 1.5, 64, 16 / 1.5, 32 / 1.5],
  ])("%s", async (_name, dpr, effectiveScaleFactor, left, top, width, height) => {
    const originalDpr = Object.getOwnPropertyDescriptor(window, "devicePixelRatio");
    Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: dpr });
    const boundsStarted = deferred();
    const bounds = deferred();
    tauriInvoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_set_bounds") {
        boundsStarted.resolve();
        await bounds.promise;
        return { presented: true, cursorCol: 2, cursorRow: 3, cellWidthPx: 16, cellHeightPx: 32, effectiveScaleFactor };
      }
      return undefined;
    });
    try {
      const view = render(<NativeTerminalPane session={session("ime-scale")} />);
      await act(async () => { await boundsStarted.promise; bounds.resolve(); });
      const sink = view.getByTestId("native-terminal-focus-sink");
      expect(sink.style.left).toBe(`${left}px`);
      expect(sink.style.top).toBe(`${top}px`);
      expect(sink.style.width).toBe(`${width}px`);
      expect(sink.style.height).toBe(`${height}px`);
      for (const cmd of ["cmd_native_terminal_attach", "cmd_native_terminal_set_bounds"]) {
        expect(tauriInvoke).toHaveBeenCalledWith(cmd, expect.objectContaining({ scaleFactor: dpr }));
      }
    } finally {
      if (originalDpr) Object.defineProperty(window, "devicePixelRatio", originalDpr);
    }
  });

  it("does not reclaim the outgoing surface when input fails after tab replacement", async () => {
    const boundary = inputRecoveryBoundary();
    const view = render(<NativeTerminalPane session={session("owner-a")} />);
    await act(async () => {});
    await startPendingInput(view, boundary);

    HTMLElement.prototype.getBoundingClientRect = () => ({ ...PANE_RECT, x: 90, y: 100, width: 400, height: 300 });
    await act(async () => { view.rerender(<NativeTerminalPane session={session("owner-b")} />); });
    await boundary.boundsStarted.promise;
    expect(lifecycleCalls()).not.toContainEqual(["cmd_native_terminal_detach", "owner-a"]);
    await act(async () => { boundary.input.reject(new Error("departed input")); });

    expect(lifecycleCalls().filter(([cmd, id]) => cmd === "cmd_native_terminal_attach" && id === "owner-a")).toHaveLength(1);
    expect(tauriInvoke.mock.calls.filter(([cmd]) => cmd === "cmd_native_terminal_send_input")).toHaveLength(1);
    expect(tauriInvoke).toHaveBeenCalledWith("cmd_native_terminal_attach", expect.objectContaining({ sessionId: "owner-b", bounds: { x: 90, y: 100, width: 400, height: 300 } }));
    await act(async () => { boundary.bounds.resolve(PRESENTED); await boundary.detached.promise; });
    expect(lifecycleCalls().filter(([cmd]) => cmd === "cmd_native_terminal_detach")).toEqual([["cmd_native_terminal_detach", "owner-a"]]);
    expect(view.queryByRole("alert")).toBeNull();
  });

  it("does not recover input when its owner becomes hidden", async () => {
    const boundary = inputRecoveryBoundary();
    const pane = (visible: boolean) => <NativeTerminalVisibilityProvider visible={visible}><NativeTerminalPane session={session("owner-a")} /></NativeTerminalVisibilityProvider>;
    const view = render(pane(true));
    await act(async () => {});
    await startPendingInput(view, boundary);

    await act(async () => { view.rerender(pane(false)); });
    await boundary.detached.promise;
    await act(async () => { boundary.input.reject(new Error("hidden input")); });

    expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "owner-a"], ["cmd_native_terminal_detach", "owner-a"]]);
    expect(tauriInvoke.mock.calls.filter(([cmd]) => cmd === "cmd_native_terminal_send_input")).toHaveLength(1);
    expect(view.queryByRole("alert")).toBeNull();
  });

  it("does not retry input after its owner leaves during recovery", async () => {
    const boundary = inputRecoveryBoundary();
    boundary.holdRecovery();
    const view = render(<NativeTerminalPane session={session("owner-a")} />);
    await act(async () => {});
    await startPendingInput(view, boundary);
    await act(async () => { boundary.input.reject(new Error("recoverable input")); await boundary.recoveryStarted.promise; });

    HTMLElement.prototype.getBoundingClientRect = () => ({ ...PANE_RECT, x: 90, y: 100, width: 400, height: 300 });
    await act(async () => { view.rerender(<NativeTerminalPane session={session("owner-b")} />); });
    await boundary.boundsStarted.promise;
    await act(async () => { boundary.bounds.resolve(PRESENTED); });
    const anchor = view.getByTestId("native-terminal-focus-sink").getAttribute("style");
    await act(async () => { boundary.recovery.resolve(); await boundary.detached.promise; });

    expect(tauriInvoke.mock.calls.filter(([cmd]) => cmd === "cmd_native_terminal_send_input")).toHaveLength(1);
    expect(view.getByTestId("native-terminal-focus-sink").getAttribute("style")).toBe(anchor);
    expect(view.queryByRole("alert")).toBeNull();
    expect(lifecycleCalls().filter(([cmd]) => cmd === "cmd_native_terminal_detach")).toEqual([["cmd_native_terminal_detach", "owner-a"]]);
  });

  it.each([false, true])("guards queued recovery attachment while preserving a returning owner (returns=%s)", async (returns) => {
    const boundary = inputRecoveryBoundary();
    const initialAttach = deferred();
    const invokeBoundary = tauriInvoke.getMockImplementation();
    let initial = true;
    tauriInvoke.mockImplementation(async (cmd, args) => {
      const result = invokeBoundary?.(cmd, args);
      if (cmd === "cmd_native_terminal_attach" && args.sessionId === "owner-a" && initial) {
        initial = false;
        await initialAttach.promise;
      }
      return result;
    });
    const view = render(<NativeTerminalPane session={session("owner-a")} />);
    await startPendingInput(view, boundary);
    await act(async () => { boundary.input.reject(new Error("queue recovery behind initial attach")); });
    expect(lifecycleCalls().filter(([cmd]) => cmd === "cmd_native_terminal_attach")).toHaveLength(1);

    if (returns) {
      await act(async () => { view.rerender(<NativeTerminalPane session={session("owner-b")} />); });
      await boundary.boundsStarted.promise;
      HTMLElement.prototype.getBoundingClientRect = () => ({ ...PANE_RECT, x: 150, width: 500 });
      await act(async () => { view.rerender(<NativeTerminalPane session={session("owner-a")} />); });
    } else {
      await act(async () => { view.unmount(); });
    }
    await act(async () => { initialAttach.resolve(); boundary.bounds.resolve(PRESENTED); });

    expect(tauriInvoke.mock.calls.filter(([cmd]) => cmd === "cmd_native_terminal_send_input")).toHaveLength(1);
    const attaches = tauriInvoke.mock.calls.filter(([cmd, args]) => cmd === "cmd_native_terminal_attach" && args.sessionId === "owner-a");
    expect(attaches).toHaveLength(returns ? 2 : 1);
    if (returns) {
      expect(attaches[1]?.[1].bounds).toEqual({ x: 150, y: 20, width: 500, height: 600 });
      expect(lifecycleCalls()).not.toContainEqual(["cmd_native_terminal_detach", "owner-a"]);
      expect(tauriInvoke).toHaveBeenCalledWith("cmd_native_terminal_set_bounds", expect.objectContaining({ sessionId: "owner-a" }));
    } else {
      await boundary.detached.promise;
    }
  });

  it.each(["input receipt", "recovery error", "retry receipt", "retry error"])("does not publish a stale %s after replacement", async (outcome) => {
    const boundary = inputRecoveryBoundary();
    boundary.holdRecovery();
    const retry = deferred<typeof PRESENTED>();
    const retryStarted = deferred();
    const invokeBoundary = tauriInvoke.getMockImplementation();
    let inputs = 0;
    tauriInvoke.mockImplementation(async (cmd, args) => {
      if (cmd === "cmd_native_terminal_send_input" && ++inputs === 2) {
        retryStarted.resolve();
        return retry.promise;
      }
      return invokeBoundary?.(cmd, args);
    });
    const view = render(<NativeTerminalPane session={session("owner-a")} />);
    await act(async () => {});
    await startPendingInput(view, boundary);
    if (outcome !== "input receipt") {
      await act(async () => { boundary.input.reject(new Error("recover input")); await boundary.recoveryStarted.promise; });
      if (outcome !== "recovery error") {
        await act(async () => { boundary.recovery.resolve(); await retryStarted.promise; });
      }
    }
    await act(async () => { view.rerender(<NativeTerminalPane session={session("owner-b")} />); });
    await boundary.boundsStarted.promise;
    await act(async () => { boundary.bounds.resolve(PRESENTED); });
    const anchor = view.getByTestId("native-terminal-focus-sink").getAttribute("style");

    await act(async () => {
      switch (outcome) {
        case "input receipt": boundary.input.resolve({ ...PRESENTED, cursorCol: 99 }); break;
        case "recovery error": boundary.recovery.reject(new Error("obsolete recovery failed")); break;
        case "retry receipt": retry.resolve({ ...PRESENTED, cursorCol: 99 }); break;
        case "retry error": retry.reject(new Error("obsolete retry failed")); break;
      }
    });
    expect(view.getByTestId("native-terminal-focus-sink").getAttribute("style")).toBe(anchor);
    expect(view.queryByRole("alert")).toBeNull();
  });

  it("shares one recovery for concurrent inputs from a live owner", async () => {
    const boundary = inputRecoveryBoundary();
    boundary.holdRecovery();
    const secondInput = deferred<typeof PRESENTED>();
    const secondStarted = deferred();
    const invokeBoundary = tauriInvoke.getMockImplementation();
    let inputs = 0;
    tauriInvoke.mockImplementation(async (cmd, args) => {
      if (cmd === "cmd_native_terminal_send_input" && ++inputs === 2) {
        secondStarted.resolve();
        return secondInput.promise;
      }
      return invokeBoundary?.(cmd, args);
    });
    const view = render(<NativeTerminalPane session={session("owner-a")} />);
    await act(async () => {});
    await startPendingInput(view, boundary);
    await act(async () => {
      fireEvent.input(view.getByTestId("native-terminal-focus-sink"), { target: { value: "y" } });
      await secondStarted.promise;
      boundary.input.reject(new Error("first input failure"));
      secondInput.reject(new Error("second input failure"));
      await boundary.recoveryStarted.promise;
    });
    expect(lifecycleCalls().filter(([cmd]) => cmd === "cmd_native_terminal_attach")).toHaveLength(2);
    await act(async () => { boundary.recovery.resolve(); });
    expect(inputs).toBe(4);
    expect(view.queryByRole("alert")).toBeNull();
  });

  it("retains the outgoing pane until a dropped frame is retried and presented", async () => {
    const frames = new Map<number, FrameRequestCallback>();
    let nextFrame = 0;
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.set(++nextFrame, callback);
      return nextFrame;
    });
    vi.stubGlobal("cancelAnimationFrame", (id: number) => { frames.delete(id); });
    let attempts = 0;
    tauriInvoke.mockImplementation(async (command, args) => {
      if (command !== "cmd_native_terminal_set_bounds") return undefined;
      const presented = args?.sessionId !== "frame-incoming" || ++attempts > 1;
      return { presented, cols: 80, rows: 24, cursorCol: 0, cursorRow: 0, cellWidthPx: 10, cellHeightPx: 20 };
    });
    const view = render(<NativeTerminalPane session={session("frame-outgoing")} />);
    await act(async () => {});

    await act(async () => {
      view.rerender(<NativeTerminalPane session={session("frame-incoming")} />);
    });

    expect(attempts).toBe(1);
    expect(lifecycleCalls()).not.toContainEqual(["cmd_native_terminal_detach", "frame-outgoing"]);
    await act(async () => {
      const callbacks = [...frames.values()];
      frames.clear();
      for (const callback of callbacks) callback(16);
    });
    expect(attempts).toBe(2);
    expect(lifecycleCalls()).toContainEqual(["cmd_native_terminal_detach", "frame-outgoing"]);
  });

  it("cancels dropped-frame retries when the pane unmounts", async () => {
    const frames = new Map<number, FrameRequestCallback>();
    let nextFrame = 0;
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.set(++nextFrame, callback);
      return nextFrame;
    });
    vi.stubGlobal("cancelAnimationFrame", (id: number) => { frames.delete(id); });
    tauriInvoke.mockImplementation(async (command) => command === "cmd_native_terminal_set_bounds"
      ? { presented: false, cols: 80, rows: 24, cursorCol: 0, cursorRow: 0, cellWidthPx: 10, cellHeightPx: 20 }
      : undefined);
    const view = render(<NativeTerminalPane session={session("frame-unmount")} />);
    await act(async () => {});
    expect(frames.size).toBeGreaterThan(0);

    await act(async () => { view.unmount(); });
    tauriInvoke.mockClear();
    await act(async () => {
      for (const callback of frames.values()) callback(16);
    });

    expect(tauriInvoke).not.toHaveBeenCalledWith("cmd_native_terminal_set_bounds", expect.anything());
  });

  it("does not block a new split pane attach behind an unrelated session", async () => {
    let resolveOldAttach: (() => void) | undefined;
    const oldAttach = new Promise<void>((resolve) => {
      resolveOldAttach = resolve;
    });

    tauriCoreMocks.invoke.mockImplementation(async (cmd, args) => {
      if (cmd === "cmd_native_terminal_attach" && args?.sessionId === "backend-a") {
        await oldAttach;
      }
      return undefined;
    });

    const view = render(<NativeTerminalPane session={session("backend-a")} />);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-a"]]);
    });

    view.rerender(<NativeTerminalPane session={session("backend-b")} />);
    await Promise.resolve();

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-a"],
        ["cmd_native_terminal_attach", "backend-b"],
      ]);
    });

    resolveOldAttach?.();

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-a"],
        ["cmd_native_terminal_attach", "backend-b"],
        ["cmd_native_terminal_detach", "backend-a"],
      ]);
    });
  });

  it("keeps the existing surface attached when a split reparents its pane", async () => {
    const view = render(<NativeTerminalPane session={session("backend-reparented")} />);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-reparented"]]);
    });

    view.rerender(
      <div>
        <NativeTerminalPane session={session("backend-reparented")} />
      </div>,
    );

    await Promise.resolve();

    expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-reparented"]]);
  });

  it("detaches and blocks input while the full-screen Settings surface covers the active pane", async () => {
    const view = render(<NativeTerminalPane session={session("backend-settings")} />);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-settings"]]);
    });
    expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
      "data-native-terminal-visible",
      "true",
    );

    const settings = document.createElement("div");
    settings.setAttribute("role", "dialog");
    settings.setAttribute("aria-label", "Settings");
    document.body.appendChild(settings);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-settings"],
        ["cmd_native_terminal_detach", "backend-settings"],
      ]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "false",
      );
    });

    tauriCoreMocks.invoke.mockClear();
    const sink = view.getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;
    fireEvent.input(sink, { target: { value: "must-not-reach-pty" } });
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.anything(),
    );

    settings.remove();
    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-settings"]]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "true",
      );
    });
  });

  it("detaches and blocks input while a mounted role=dialog New Tab menu covers the active pane", async () => {
    const view = render(<NativeTerminalPane session={session("backend-newtab")} />);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-newtab"]]);
    });
    expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
      "data-native-terminal-visible",
      "true",
    );

    const newTabMenu = document.createElement("div");
    newTabMenu.setAttribute("role", "dialog");
    newTabMenu.setAttribute("aria-label", "New tab menu");
    document.body.appendChild(newTabMenu);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-newtab"],
        ["cmd_native_terminal_detach", "backend-newtab"],
      ]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "false",
      );
    });

    tauriCoreMocks.invoke.mockClear();
    const sink = view.getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;
    fireEvent.input(sink, { target: { value: "must-not-reach-pty" } });
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.anything(),
    );

    newTabMenu.remove();
    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-newtab"]]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "true",
      );
    });
  });

  it("detaches and blocks input while a mounted role=search Terminal search overlay covers the active pane", async () => {
    const view = render(<NativeTerminalPane session={session("backend-search")} />);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-search"]]);
    });
    expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
      "data-native-terminal-visible",
      "true",
    );

    const searchOverlay = document.createElement("div");
    searchOverlay.setAttribute("role", "search");
    searchOverlay.setAttribute("aria-label", "Terminal search");
    document.body.appendChild(searchOverlay);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-search"],
        ["cmd_native_terminal_detach", "backend-search"],
      ]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "false",
      );
    });

    tauriCoreMocks.invoke.mockClear();
    const sink = view.getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;
    fireEvent.input(sink, { target: { value: "must-not-reach-pty" } });
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.anything(),
    );

    searchOverlay.remove();
    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-search"]]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "true",
      );
    });
  });

  it("suppresses and restores native surface via NativeTerminalVisibilityProvider", async () => {
    const view = render(
      <NativeTerminalVisibilityProvider visible={false}>
        <NativeTerminalPane session={session("backend-provider")} />
      </NativeTerminalVisibilityProvider>,
    );

    await Promise.resolve();
    expect(lifecycleCalls()).toEqual([]);
    expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
      "data-native-terminal-visible",
      "false",
    );

    view.rerender(
      <NativeTerminalVisibilityProvider visible={true}>
        <NativeTerminalPane session={session("backend-provider")} />
      </NativeTerminalVisibilityProvider>,
    );

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-provider"]]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "true",
      );
    });

    view.rerender(
      <NativeTerminalVisibilityProvider visible={false}>
        <NativeTerminalPane session={session("backend-provider")} />
      </NativeTerminalVisibilityProvider>,
    );

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-provider"],
        ["cmd_native_terminal_detach", "backend-provider"],
      ]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "false",
      );
    });
  });

  it("updates device scale without reattaching when monitor density changes at fixed logical bounds", async () => {
    const queries: EventTarget[] = [];
    vi.stubGlobal("matchMedia", vi.fn(() => {
      const query = new EventTarget();
      queries.push(query);
      return query;
    }));
    vi.stubGlobal("devicePixelRatio", 1);
    const view = render(<NativeTerminalPane session={session("backend-scale")} />);
    await act(async () => {});
    tauriInvoke.mockClear();
    vi.stubGlobal("devicePixelRatio", 2);
    await act(async () => { queries[queries.length - 1]?.dispatchEvent(new Event("change")); });
    expect(tauriInvoke).toHaveBeenCalledWith("cmd_native_terminal_set_bounds", expect.objectContaining({ scaleFactor: 2 }));
    expect(lifecycleCalls()).toEqual([]);
    tauriInvoke.mockClear();
    vi.stubGlobal("devicePixelRatio", 1);
    await act(async () => { queries[queries.length - 1]?.dispatchEvent(new Event("change")); });
    expect(tauriInvoke).toHaveBeenCalledWith("cmd_native_terminal_set_bounds", expect.objectContaining({ scaleFactor: 1 }));
    expect(lifecycleCalls()).toEqual([]);
    const activeQuery = queries[queries.length - 1];
    const remove = vi.spyOn(activeQuery, "removeEventListener");
    await act(async () => { view.unmount(); });
    expect(remove).toHaveBeenCalledWith("change", expect.any(Function));
    tauriInvoke.mockClear();
    await act(async () => { activeQuery.dispatchEvent(new Event("change")); });
    expect(tauriInvoke).not.toHaveBeenCalledWith("cmd_native_terminal_set_bounds", expect.anything());
  });

  it("handles React StrictMode remount cleanly preserving attach sequence", async () => {
    render(
      <StrictMode>
        <NativeTerminalPane session={session("backend-strict")} />
      </StrictMode>,
    );

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-strict"],
      ]);
    });
  });

  it("skips stale detachment when remount occurs while detachment is in flight and remains attached", async () => {
    const view = render(<NativeTerminalPane session={session("backend-stale-race")} />);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-stale-race"]]);
    });

    let resolveSlowDetach: (() => void) | undefined;
    const slowDetach = new Promise<void>((resolve) => {
      resolveSlowDetach = resolve;
    });

    let detachInvoked = false;
    tauriCoreMocks.invoke.mockImplementation(async (cmd, args) => {
      if (cmd === "cmd_native_terminal_detach" && args?.sessionId === "backend-stale-race") {
        detachInvoked = true;
        await slowDetach;
      }
      return undefined;
    });

    // Unmount triggers cleanup
    view.unmount();

    // Remount reusing same backendSessionId
    render(<NativeTerminalPane session={session("backend-stale-race")} />);

    // Resolve stale detach promise last
    resolveSlowDetach?.();
    await Promise.resolve();

    await waitFor(() => {
      expect(lifecycleCalls().filter(([cmd]) => cmd === "cmd_native_terminal_detach")).toEqual([]);
      expect(detachInvoked).toBe(false);
    });
  });
});
