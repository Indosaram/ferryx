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
const { previousNativeFocusForSnapshot } = await import("./NativeTerminalPane");
const { resetNativeTerminalLifecycleForTest, registerDurableNativeBinding, getDurableNativeBinding, settleNativeTerminalExit } = await import("../lib/nativeTerminalLifecycle");
const { classifyNativeTerminalAttachError } = await import("../lib/nativeTerminalAttachPolicy");
const shortcuts = await import("../lib/shortcuts");
const { setLocalSplitPersistence } = await import("../lib/localSplitLifecycle");
const nativeQaReceipts = await import("../lib/nativeQaReceipts");
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
  invoke: async (cmd: string, args?: any) => {
    const result = await tauriInvoke(cmd, args);
    // Native bounds replies echo the tuple captured at dispatch. Preserve an
    // explicitly supplied stale tuple so identity rejection remains testable.
    if (cmd === "cmd_native_terminal_set_bounds" && result && typeof result === "object") {
      return { attachTuple: args?.attachTuple, ...result };
    }
    return result;
  },
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
  registerDurableNativeBinding({
    backendSessionId: id, incarnation: null, daemonEpoch: "",
    frontendSessionId: `frontend-${id}`, paneIdentity: `frontend-${id}`,
    bindingKey: `${id}::0:`, attemptGeneration: 0,
  });
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

beforeEach(() => {
  resetNativeTerminalLifecycleForTest();
});

it("settles only the current matching incarnation and leaves stale exits retryable", () => {
  const original = { frontendSessionId: "front-a", paneIdentity: "pane-a", backendSessionId: "backend-a",
    incarnation: "inc-a", daemonEpoch: "epoch-a", bindingKey: "bind-a", attemptGeneration: 3 };
  registerDurableNativeBinding(original);
  expect(settleNativeTerminalExit("front-a", { ...original, incarnation: "inc-old" })).toBe(false);
  expect(settleNativeTerminalExit("front-a", original)).toBe(true);
  expect(settleNativeTerminalExit("front-a", original)).toBe(true);
});

it("does not accept a stale affirmative attach exit for the pane's newer durable incarnation", async () => {
  const newer = { frontendSessionId: "front-newer", paneIdentity: "pane-newer", backendSessionId: "backend-newer",
    incarnation: "inc-new", daemonEpoch: "epoch-new", bindingKey: "bind-new", attemptGeneration: 4 };
  registerDurableNativeBinding(newer);
  const error = { code: "SESSION_NOT_FOUND", message: "Session 'backend-newer' not found", details: {
    source: "daemon_attach", kind: "session_not_found", sessionId: "backend-newer", affirmativeExit: true, incarnation: "inc-old",
  } };
  expect(settleNativeTerminalExit("front-newer", { ...newer, incarnation: "inc-old" })).toBe(false);
  expect(getDurableNativeBinding("backend-newer")).toEqual(newer);
  expect(classifyNativeTerminalAttachError(error, "backend-newer", newer.incarnation, newer.daemonEpoch).status).toBe("unverified-missing");
});

it("does not accept affirmative exit when daemon epoch mismatches despite matching incarnation", async () => {
  const current = { frontendSessionId: "front-epoch", paneIdentity: "pane-epoch", backendSessionId: "backend-epoch",
    incarnation: "inc-current", daemonEpoch: "epoch-current", bindingKey: "bind-current", attemptGeneration: 5 };
  registerDurableNativeBinding(current);
  const staleEpochExit = { frontendSessionId: "front-epoch", paneIdentity: "pane-epoch", backendSessionId: "backend-epoch",
    incarnation: "inc-current", daemonEpoch: "epoch-stale", bindingKey: "bind-current", attemptGeneration: 5 };
  expect(settleNativeTerminalExit("front-epoch", staleEpochExit)).toBe(false);
  expect(getDurableNativeBinding("backend-epoch")).toEqual(current);
  const error = { code: "SESSION_NOT_FOUND", message: "Session 'backend-epoch' not found", details: {
    source: "daemon_attach", kind: "session_not_found", sessionId: "backend-epoch", affirmativeExit: true, incarnation: "inc-current", daemonEpoch: "epoch-other",
  } };
  expect(classifyNativeTerminalAttachError(error, "backend-epoch", current.incarnation, current.daemonEpoch).status).toBe("unverified-missing");
});

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
  let reject = (_error: unknown) => {};
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

const PRESENTED = {
  presented: true, cursorCol: 2, cursorRow: 3, cellWidthPx: 10, cellHeightPx: 20,
};

const INPUT_NOT_WRITTEN = {
  code: "INTERNAL_ERROR",
  message: "Native input surface is detached",
  details: { inputWritten: false },
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
    setLocalSplitPersistence(async () => {});
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
    setLocalSplitPersistence(undefined);
    restorePaneRect();
    try {
      await act(async () => { cleanup(); });
    } finally {
      vi.useRealTimers();
    }
    Object.defineProperty(globalThis, "ResizeObserver", {
      configurable: true,
      writable: true,
      value: originalResizeObserver,
    });
    if (typeof vi.unstubAllGlobals === "function") {
      vi.unstubAllGlobals();
    }
    vi.restoreAllMocks();
  });

  it("uses the previous focused pane point, never the target pane's own focus point", () => {
    const previousPanePoint = { x: 40, y: 60 };
    expect(previousNativeFocusForSnapshot({ paneIdentity: "pane-a", point: previousPanePoint }, "pane-b"))
      .toEqual(previousPanePoint);
    expect(previousNativeFocusForSnapshot({ paneIdentity: "pane-b", point: { x: 200, y: 300 } }, "pane-b"))
      .toBeNull();
    expect(previousNativeFocusForSnapshot(null, "pane-b")).toBeNull();
  });

  it("recovers after 24 failed attaches with capped delay and no mutation replay", async () => {
    vi.useFakeTimers();
    const attempts = Array.from({ length: 25 }, () => deferred());
    let count = 0;
    tauriInvoke.mockImplementation(async command => {
      if (command === "cmd_native_terminal_attach") {
        attempts[count].resolve();
        if (++count <= 24) throw new Error("transport unavailable");
      }
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    const view = render(<NativeTerminalPane session={session("long-outage")} active={false} />);
    try {
      await act(async () => { await attempts[0].promise; });
      for (let index = 1; index < attempts.length; index++) {
        const delay = index <= 4 ? 250 * 2 ** (index - 1) : 4000;
        await act(async () => {
          await vi.advanceTimersByTimeAsync(delay - 1);
        });
        expect(count).toBe(index);
        await act(async () => {
          await vi.advanceTimersByTimeAsync(1);
        });
        expect(count).toBe(index + 1);
      }
      expect(count).toBe(25);
      expect(tauriInvoke.mock.calls.some(([command]) => command.includes("spawn") || command === "cmd_native_terminal_send_input")).toBe(false);
      expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach").every(([, id]) => id === "long-outage")).toBe(true);
    } finally {
      await act(async () => { view.unmount(); });
      vi.useRealTimers();
    }
  });

  it("cancels pending attach on close and ignores late completion", async () => {
    const started = deferred(); const held = deferred(); const detached = deferred();
    tauriInvoke.mockImplementation(async command => {
      if (command === "cmd_native_terminal_attach") { started.resolve(); return held.promise; }
      if (command === "cmd_native_terminal_detach") detached.resolve();
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    const view = render(<NativeTerminalPane session={session("pending-close")} />);
    await act(async () => { await started.promise; });
    await act(async () => { view.unmount(); });
    await act(async () => { await detached.promise; });
    await act(async () => { held.resolve(); });
    expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "pending-close"], ["cmd_native_terminal_detach", "pending-close"]]);
  });

  it("records native QA persistence only after the durable binding is registered", async () => {
    const binding = session("qa-persistence");
    resetNativeTerminalLifecycleForTest();
    const persistenceStarted = deferred();
    const persistence = deferred<void>();
    const observeStarted = deferred();
    const observation = deferred<void>();
    const tuple = {
      backendSessionId: "qa-persistence", incarnation: null, daemonEpoch: "",
      frontendSessionId: binding.id, paneIdentity: binding.id,
      bindingKey: "qa-persistence::0:", attemptGeneration: 0,
    };
    setLocalSplitPersistence(async () => { persistenceStarted.resolve(); await persistence.promise; });
    const observe = vi.spyOn(nativeQaReceipts, "recordNativeBindingPersisted").mockImplementation(async received => {
      observeStarted.resolve();
      expect(received).toEqual(tuple);
      await observation.promise;
    });
    tauriInvoke.mockImplementation(async command => command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined);

    const view = render(<NativeTerminalPane session={binding} active={false} />);
    await act(async () => { await persistenceStarted.promise; });
    expect(observe).not.toHaveBeenCalled();

    await act(async () => { persistence.resolve(); await observeStarted.promise; });
    expect(observe).toHaveBeenCalledTimes(1);
    expect(observe).toHaveBeenCalledWith(tuple);
    expect(tauriInvoke.mock.calls.some(([command]) => command === "cmd_native_terminal_attach")).toBe(false);

    await act(async () => { observation.resolve(); });
    await act(async () => { await waitFor(() => expect(lifecycleCalls()).toContainEqual(["cmd_native_terminal_attach", "qa-persistence"])); });
    await act(async () => { view.unmount(); });
  });

  it.each(["rejected", "stale-owner"] as const)("does not record native QA persistence when persistence is %s", async outcome => {
    const binding = session(`qa-persistence-${outcome}`);
    resetNativeTerminalLifecycleForTest();
    const persistenceStarted = deferred();
    const persistence = deferred<void>();
    setLocalSplitPersistence(async () => { persistenceStarted.resolve(); await persistence.promise; });
    const observe = vi.spyOn(nativeQaReceipts, "recordNativeBindingPersisted");
    tauriInvoke.mockImplementation(async command => command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined);

    const view = render(<NativeTerminalPane session={binding} active={false} />);
    await act(async () => { await persistenceStarted.promise; });
    if (outcome === "stale-owner") {
      await act(async () => { view.unmount(); });
      persistence.resolve();
    } else {
      persistence.reject(new Error("persistence failed"));
    }
    await act(async () => { await Promise.resolve(); });

    expect(observe).not.toHaveBeenCalled();
    expect(tauriInvoke.mock.calls.some(([command]) => command === "cmd_native_terminal_attach")).toBe(false);
    if (outcome === "rejected") await act(async () => { view.unmount(); });
  });

  it("rebind cancels held old attach without detaching the replacement", async () => {
    const oldStarted = deferred(); const old = deferred(); const nextStarted = deferred();
    tauriInvoke.mockImplementation(async (command, args) => {
      if (command === "cmd_native_terminal_attach" && args.sessionId === "held-old") { oldStarted.resolve(); return old.promise; }
      if (command === "cmd_native_terminal_attach" && args.sessionId === "ready-next") nextStarted.resolve();
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    const view = render(<NativeTerminalPane session={session("held-old")} />);
    await act(async () => { await oldStarted.promise; });
    await act(async () => { view.rerender(<NativeTerminalPane session={session("ready-next")} />); });
    await act(async () => { await nextStarted.promise; });
    await act(async () => { old.resolve(); });
    expect(lifecycleCalls().filter(([command, id]) => command === "cmd_native_terminal_detach" && id === "ready-next")).toEqual([]);
    await act(async () => { view.unmount(); });
  });

  it("keeps a hidden surface dormant until it becomes visible", async () => {
    const attached = deferred();
    tauriInvoke.mockImplementation(async command => {
      if (command === "cmd_native_terminal_attach") attached.resolve();
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    const retained = session("hidden-recovery");
    const pane = (visible: boolean) => <NativeTerminalVisibilityProvider visible={visible}><NativeTerminalPane session={retained} /></NativeTerminalVisibilityProvider>;
    const view = render(pane(false));
    expect(lifecycleCalls()).toEqual([]);
    await act(async () => { view.rerender(pane(true)); });
    await act(async () => { await attached.promise; });
    expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toHaveLength(1);
    await act(async () => { view.unmount(); });
  });

  it("presentation refusal never becomes transport reattachment", async () => {
    vi.useFakeTimers();
    const bounds = deferred();
    tauriInvoke.mockImplementation(async command => {
      if (command === "cmd_native_terminal_set_bounds") { bounds.resolve(); return { ...PRESENTED, presented: false }; }
      return undefined;
    });
    const retained = session("presentation-only");
    const view = render(<NativeTerminalPane session={retained} />);
    try {
      await act(async () => { await bounds.promise; await vi.advanceTimersByTimeAsync(60000); });
      expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toHaveLength(1);
      expect(retained.backendSessionId).toBe("presentation-only");
      expect(tauriInvoke.mock.calls.some(([command]) => command.includes("spawn"))).toBe(false);
    } finally {
      await act(async () => { view.unmount(); }); vi.useRealTimers();
    }
  });

  it("recovers after 24 stream ends with one listener and no recovery focus", async () => {
    let ended: ((event: { payload: { sessionId: string } }) => void) | undefined;
    const registered = deferred();
    const attempts = Array.from({ length: 25 }, () => deferred());
    let count = 0;
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") { ended = handler; registered.resolve(); }
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async command => {
      if (command === "cmd_native_terminal_attach") attempts[count++].resolve();
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    const view = render(<NativeTerminalPane session={session("many-streams")} active={false} />);
    await act(async () => { await registered.promise; await attempts[0].promise; });
    const other = document.createElement("input"); document.body.append(other); other.focus();
    for (let index = 1; index < attempts.length; index++) {
      await act(async () => { ended!({ payload: { sessionId: "many-streams" } }); await attempts[index].promise; });
    }
    expect(count).toBe(25);
    expect(tauriListen.mock.calls.filter(([event]) => event === "native_terminal_stream_ended")).toHaveLength(1);
    expect(document.activeElement).toBe(other);
    expect(tauriInvoke.mock.calls.some(([command]) => command === "cmd_native_terminal_send_input")).toBe(false);
    await act(async () => { view.unmount(); }); other.remove();
    tauriListen.mockImplementation(async () => () => undefined);
  });

  it("preserves paired blocked admission reason and stops attachment retries", async () => {
    // Given: stream and admission signals are registered before triggering recovery.
    vi.useFakeTimers();
    let ended: ((event: { payload: { sessionId: string } }) => void) | undefined;
    let publish: ((value: import("../state/workspaceStore").LocalRecoveryAdmissionResult) => void) | undefined;
    const registered = deferred(); const attached = deferred(); const started = deferred();
    const unsubscribe = vi.fn();
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") { ended = handler; registered.resolve(); }
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async command => {
      if (command === "cmd_native_terminal_attach") attached.resolve();
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    const refresh = vi.fn((_id: string, result: (value: import("../state/workspaceStore").LocalRecoveryAdmissionResult) => void) => {
      publish = result; result({ kind: "pending" }); started.resolve(); return unsubscribe;
    });
    const retained = { ...session("paired-blocked"), incarnation: "process" };
    const view = render(<NativeTerminalPane session={retained} onRefreshSessionIdentity={refresh} />);
    try {
      await act(async () => { await registered.promise; await attached.promise; });
      const emit = ended;
      if (!emit) throw new Error("Missing stream subscription");
      await act(async () => { emit({ payload: { sessionId: "paired-blocked" } }); await started.promise; });
      const result = publish;
      if (!result) throw new Error("Missing admission subscription");

      // When: the paired owner rejects recovery with its exact typed code.
      await act(async () => { result({ kind: "blocked", reason: "PAIRED_HOST_UNAUTHORIZED" }); });

      // Then: the reason survives the adapter and no timer or repeated stream event retries attachment.
      expect(view.container.textContent).toContain("PAIRED_HOST_UNAUTHORIZED");
      await act(async () => {
        await vi.advanceTimersByTimeAsync(100_000);
        emit({ payload: { sessionId: "paired-blocked" } });
      });
      expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toHaveLength(1);
      expect(unsubscribe).toHaveBeenCalledOnce();
      expect(tauriInvoke.mock.calls.some(([command]) => command.includes("spawn"))).toBe(false);
    } finally {
      await act(async () => { view.unmount(); });
      vi.useRealTimers();
      tauriListen.mockImplementation(async () => () => undefined);
    }
  });

  it("keeps owner admission pending beyond transport deadlines then attaches only its ready tuple", async () => {
    vi.useFakeTimers();
    let ended: ((event: { payload: { sessionId: string } }) => void) | undefined;
    let publish!: (value: import("../state/workspaceStore").LocalRecoveryAdmissionResult) => void;
    const started = deferred(); const attached = deferred();
    const unsubscribe = vi.fn();
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") ended = handler;
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async (command, args) => {
      if (command === "cmd_native_terminal_attach" && args.attachTuple.daemonEpoch === "admitted") attached.resolve();
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    const retained = { ...session("owner-pending"), incarnation: "process" };
    const refresh = vi.fn((_id: string, result: typeof publish) => {
      publish = result; result({ kind: "pending" }); started.resolve(); return unsubscribe;
    });
    const view = render(<NativeTerminalPane session={retained} onRefreshSessionIdentity={refresh} />);
    await act(async () => {});
    await act(async () => { ended!({ payload: { sessionId: "owner-pending" } }); await started.promise; });
    await act(async () => { await vi.advanceTimersByTimeAsync(100_000); });
    expect(refresh).toHaveBeenCalledOnce();
    expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toHaveLength(1);
    await act(async () => { publish({ kind: "ready", session: { ...retained, daemonEpoch: "admitted" } }); await attached.promise; });
    expect(unsubscribe).toHaveBeenCalledOnce();
    expect(tauriInvoke.mock.calls.some(([command]) => command.includes("spawn"))).toBe(false);
    await act(async () => { view.unmount(); });
    vi.useRealTimers();
  });

  it("cancels held admission on incarnation-only replacement and ignores its late ready result", async () => {
    let ended!: (event: { payload: { sessionId: string } }) => void;
    let publish!: (value: import("../state/workspaceStore").LocalRecoveryAdmissionResult) => void;
    const registered = deferred(); const admission = deferred(); const replacementAttached = deferred();
    const unsubscribe = vi.fn();
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") { ended = handler; registered.resolve(); }
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async (command, args) => {
      if (command === "cmd_native_terminal_attach" && args.attachTuple.incarnation === "replacement") replacementAttached.resolve();
      return command === "cmd_native_terminal_set_bounds" ? { ...PRESENTED, attachTuple: args.attachTuple } : undefined;
    });
    const original = { ...session("incarnation-admission"), incarnation: "original" };
    const refresh = vi.fn((_id: string, result: typeof publish) => {
      publish = result; result({ kind: "pending" }); admission.resolve(); return unsubscribe;
    });
    const view = render(<NativeTerminalPane session={original} onRefreshSessionIdentity={refresh} />);
    await act(async () => { await registered.promise; });
    const oldEnded = ended;
    await act(async () => { oldEnded({ payload: { sessionId: original.backendSessionId! } }); await admission.promise; });
    await act(async () => {
      view.rerender(<NativeTerminalPane session={{ ...original, incarnation: "replacement" }} onRefreshSessionIdentity={refresh} />);
    });
    expect(unsubscribe).toHaveBeenCalledOnce();
    await act(async () => { await replacementAttached.promise; });
    const attaches = lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach");
    await act(async () => {
      publish({ kind: "ready", session: original });
      oldEnded({ payload: { sessionId: original.backendSessionId! } });
    });
    expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toEqual(attaches);
    expect(getDurableNativeBinding(original.backendSessionId!)?.incarnation).toBe("replacement");
    expect(view.queryByRole("alert")).toBeNull();
    expect(refresh).toHaveBeenCalledOnce();
    await act(async () => { view.unmount(); });
    tauriListen.mockImplementation(async () => () => undefined);
  });

  it("coalesces stream events while refresh is pending and blocks conflicting incarnation", async () => {
    const refreshing = deferred();
    const refreshed = deferred<TerminalSession>();
    let ended: ((event: { payload: { sessionId: string } }) => void) | undefined;
    const registered = deferred();
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") { ended = handler; registered.resolve(); }
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async command => command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined);
    const retained = { ...session("conflict"), incarnation: "original" };
    const refresh = vi.fn((_id: string, result: (value: import("../state/workspaceStore").LocalRecoveryAdmissionResult) => void) => {
      refreshing.resolve(); let live = true;
      void refreshed.promise.then(session => { if (live) result({ kind: "ready", session: session! }); });
      return () => { live = false; };
    });
    const view = render(<NativeTerminalPane session={retained} onRefreshSessionIdentity={refresh} />);
    await act(async () => { await registered.promise; });
    await act(async () => { ended!({ payload: { sessionId: "conflict" } }); await refreshing.promise; });
    await act(async () => {
      for (let index = 0; index < 24; index++) ended!({ payload: { sessionId: "conflict" } });
      refreshed.resolve({ ...retained, incarnation: "different" });
    });
    expect(refresh).toHaveBeenCalledOnce();
    expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toHaveLength(1);
    expect(tauriInvoke.mock.calls.some(([command]) => command.includes("spawn"))).toBe(false);
    await act(async () => { view.unmount(); });
    tauriListen.mockImplementation(async () => () => undefined);
  });

  it("reattaches the same backend when its output stream ends without user input", async () => {
    let ended: ((event: { payload: { sessionId: string } }) => void) | undefined;
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") ended = handler;
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async (command) =>
      command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined,
    );
    const view = render(<NativeTerminalPane session={session("stream-ended")} />);
    await act(async () => {});
    expect(ended).toBeTypeOf("function");
    await act(async () => { ended!({ payload: { sessionId: "other-backend" } }); });
    expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toHaveLength(1);
    await act(async () => { ended!({ payload: { sessionId: "stream-ended" } }); });
    expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toEqual([
      ["cmd_native_terminal_attach", "stream-ended"],
      ["cmd_native_terminal_attach", "stream-ended"],
    ]);
    expect(tauriInvoke.mock.calls.some(([command]) => command === "cmd_native_terminal_send_input")).toBe(false);
    await act(async () => { view.unmount(); });
    const attachedBeforeStaleEvent = lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach").length;
    await act(async () => { ended!({ payload: { sessionId: "stream-ended" } }); });
    expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toHaveLength(attachedBeforeStaleEvent);
    tauriListen.mockImplementation(async () => () => undefined);
  });

  it("invokes onRefreshSessionIdentity and reattaches with refreshed epoch and retained incarnation on stream-ended event", async () => {
    let ended: ((event: { payload: { sessionId: string } }) => void) | undefined;
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") ended = handler;
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async (command) =>
      command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined,
    );

    const initialSession = {
      ...session("stream-ended-epoch"),
      daemonEpoch: "epoch-v1",
      incarnation: "inc-v1",
    };

    const onRefreshSessionIdentity = vi.fn().mockImplementation((_sessionId: string, result: (value: import("../state/workspaceStore").LocalRecoveryAdmissionResult) => void) => {
      result({ kind: "ready", session: {
        ...initialSession,
        daemonEpoch: "epoch-v2",
        incarnation: "inc-v1",
      } }); return () => {};
    });

    const view = render(
      <NativeTerminalPane
        session={initialSession}
        onRefreshSessionIdentity={onRefreshSessionIdentity}
      />,
    );
    await act(async () => {});
    expect(ended).toBeTypeOf("function");

    // First attach used initial epoch
    const initialAttach = tauriInvoke.mock.calls.find(
      ([cmd, args]) => cmd === "cmd_native_terminal_attach" && args.sessionId === "stream-ended-epoch",
    );
    expect(initialAttach).toBeDefined();
    expect(initialAttach![1].attachTuple.daemonEpoch).toBe("epoch-v1");
    expect(initialAttach![1].attachTuple.incarnation).toBe("inc-v1");

    // Trigger stream ended
    await act(async () => {
      ended!({ payload: { sessionId: "stream-ended-epoch" } });
    });

    // Authoritative refresh callback was invoked before reattach
    expect(onRefreshSessionIdentity).toHaveBeenCalledWith(initialSession.id, expect.any(Function));

    // Reattach used the refreshed epoch and incarnation
    const attaches = tauriInvoke.mock.calls.filter(
      ([cmd, args]) => cmd === "cmd_native_terminal_attach" && args.sessionId === "stream-ended-epoch",
    );
    expect(attaches).toHaveLength(2);
    expect(attaches[1][1].attachTuple.daemonEpoch).toBe("epoch-v2");
    expect(attaches[1][1].attachTuple.incarnation).toBe("inc-v1");

    await act(async () => { view.unmount(); });
    tauriListen.mockImplementation(async () => () => undefined);
  });

  it("refreshes a stale daemon epoch and re-attaches when attach refuses the tuple as detached", async () => {
    vi.useFakeTimers();
    // Given: the pane still carries the epoch of a daemon that a rolling handover replaced, so
    // the first attach is refused as detached without proof that the PTY is gone.
    let attaches = 0;
    tauriInvoke.mockImplementation(async (command) => {
      if (command === "cmd_native_terminal_attach" && ++attaches === 1) {
        throw { code: "SESSION_NOT_FOUND", message: "Native terminal session 'handover-epoch' is detached" };
      }
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    const stale = { ...session("handover-epoch"), daemonEpoch: "epoch-old", incarnation: "inc-1" };
    const onRefreshSessionIdentity = vi.fn((_id: string, result: (value: import("../state/workspaceStore").LocalRecoveryAdmissionResult) => void) => {
      result({ kind: "ready", session: { ...stale, daemonEpoch: "epoch-new" } }); return () => {};
    });

    // When: the pane mounts.
    const view = render(<NativeTerminalPane session={stale} onRefreshSessionIdentity={onRefreshSessionIdentity} />);
    await act(async () => { await vi.advanceTimersByTimeAsync(250); });

    // Then: it refreshed the identity once and re-attached with the live epoch, no banner.
    expect(onRefreshSessionIdentity).toHaveBeenCalledTimes(1);
    const calls = tauriInvoke.mock.calls.filter(([cmd]) => cmd === "cmd_native_terminal_attach");
    expect(calls).toHaveLength(2);
    expect(calls[0][1].attachTuple.daemonEpoch).toBe("epoch-old");
    expect(calls[1][1].attachTuple.daemonEpoch).toBe("epoch-new");
    expect(view.queryByText("Failed to attach native terminal")).toBeNull();
    await act(async () => { view.unmount(); });
    vi.useRealTimers();
  });

  it("negative control: stale or failing onRefreshSessionIdentity does not spawn new shell or lose session identity", async () => {
    let ended: ((event: { payload: { sessionId: string } }) => void) | undefined;
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") ended = handler;
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async (command) =>
      command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined,
    );

    const preservedSession = {
      ...session("stream-stale-callback"),
      daemonEpoch: "epoch-v1",
      incarnation: "inc-v1",
    };

    // Stale/failing callback: backend is dead or daemon unreachable, returns null without respawning
    const onRefreshSessionIdentity = vi.fn((_id: string, result: (value: import("../state/workspaceStore").LocalRecoveryAdmissionResult) => void) => {
      result({ kind: "blocked", reason: "identity_conflict" }); return () => {};
    });

    const view = render(
      <NativeTerminalPane
        session={preservedSession}
        onRefreshSessionIdentity={onRefreshSessionIdentity}
      />,
    );
    await act(async () => {});
    expect(ended).toBeTypeOf("function");

    // Fire stream ended
    await act(async () => {
      ended!({ payload: { sessionId: "stream-stale-callback" } });
    });

    expect(onRefreshSessionIdentity).toHaveBeenCalledWith(preservedSession.id, expect.any(Function));

    // Verify session identity was preserved (no new shell spawned, same backend ID attempted)
    const attaches = tauriInvoke.mock.calls.filter(
      ([cmd, args]) => cmd === "cmd_native_terminal_attach" && args.sessionId === "stream-stale-callback",
    );
    expect(attaches).toHaveLength(1);
    expect(attaches[0][1].attachTuple.daemonEpoch).toBe("epoch-v1");
    expect(attaches[0][1].attachTuple.backendSessionId).toBe("stream-stale-callback");
    expect(attaches[0][1].attachTuple.frontendSessionId).toBe(preservedSession.id);

    // Negative control: verify no spawn or shell replacement IPC was ever invoked
    expect(
      tauriInvoke.mock.calls.some(([cmd]) => cmd === "cmd_terminal_spawn" || cmd === "cmd_terminal_spawn_detailed"),
    ).toBe(false);

    await act(async () => { view.unmount(); });
    tauriListen.mockImplementation(async () => () => undefined);
  });

  it("negative control: stream-ended event for mismatched backend ignores recovery and skips refresh", async () => {
    let ended: ((event: { payload: { sessionId: string } }) => void) | undefined;
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") ended = handler;
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async (command) =>
      command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined,
    );

    const onRefreshSessionIdentity = vi.fn();
    const view = render(
      <NativeTerminalPane
        session={session("stream-target")}
        onRefreshSessionIdentity={onRefreshSessionIdentity}
      />,
    );
    await act(async () => {});

    // Mismatched backend session ID
    await act(async () => {
      ended!({ payload: { sessionId: "foreign-session-id" } });
    });

    expect(onRefreshSessionIdentity).not.toHaveBeenCalled();
    const attaches = tauriInvoke.mock.calls.filter(
      ([cmd, args]) => cmd === "cmd_native_terminal_attach" && args.sessionId === "stream-target",
    );
    expect(attaches).toHaveLength(1);

    await act(async () => { view.unmount(); });
    tauriListen.mockImplementation(async () => () => undefined);
  });

  it("deferred onRefreshSessionIdentity callback after replacement does not attach old backend and preserves next live recovery", async () => {
    let ended: ((event: { payload: { sessionId: string } }) => void) | undefined;
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") ended = handler;
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async (command) =>
      command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined,
    );

    const sessionA = {
      ...session("stream-deferred-a"),
      daemonEpoch: "epoch-a1",
      incarnation: "inc-a1",
    };

    const sessionB = {
      ...session("stream-deferred-b"),
      daemonEpoch: "epoch-b1",
      incarnation: "inc-b1",
    };

    const refreshAStarted = deferred<void>();
    const refreshADeferred = deferred<TerminalSession | null>();
    const refreshBStarted = deferred<void>();

    const onRefreshSessionIdentity = vi.fn().mockImplementation((sessionId: string, result: (value: import("../state/workspaceStore").LocalRecoveryAdmissionResult) => void) => {
      let live = true;
      if (sessionId === sessionA.id) {
        refreshAStarted.resolve();
        void refreshADeferred.promise.then(session => { if (live) result(session ? { kind: "ready", session } : { kind: "cancelled" }); });
      }
      if (sessionId === sessionB.id) {
        refreshBStarted.resolve();
        result({ kind: "ready", session: {
          ...sessionB,
          daemonEpoch: "epoch-b2",
          incarnation: "inc-b1",
        } });
      }
      return () => { live = false; };
    });

    const view = render(
      <NativeTerminalPane
        session={sessionA}
        onRefreshSessionIdentity={onRefreshSessionIdentity}
      />,
    );
    await act(async () => {});
    expect(ended).toBeTypeOf("function");
    const endedA = ended!;

    const initialAttachA = tauriInvoke.mock.calls.find(
      ([cmd, args]) => cmd === "cmd_native_terminal_attach" && args.sessionId === "stream-deferred-a",
    );
    expect(initialAttachA).toBeDefined();
    expect(initialAttachA![1].attachTuple.daemonEpoch).toBe("epoch-a1");
    expect(initialAttachA![1].attachTuple.incarnation).toBe("inc-a1");

    // Trigger stream ended for session A and hold the refresh callback in flight
    await act(async () => {
      endedA({ payload: { sessionId: "stream-deferred-a" } });
    });
    await act(async () => {
      await refreshAStarted.promise;
    });
    expect(onRefreshSessionIdentity).toHaveBeenCalledWith(sessionA.id, expect.any(Function));

    // Rerender replacement with a different session and backend while refresh A is still held
    await act(async () => {
      view.rerender(
        <NativeTerminalPane
          session={sessionB}
          onRefreshSessionIdentity={onRefreshSessionIdentity}
        />,
      );
    });

    const initialAttachB = tauriInvoke.mock.calls.find(
      ([cmd, args]) => cmd === "cmd_native_terminal_attach" && args.sessionId === "stream-deferred-b",
    );
    expect(initialAttachB).toBeDefined();
    expect(initialAttachB![1].attachTuple.daemonEpoch).toBe("epoch-b1");
    expect(initialAttachB![1].attachTuple.incarnation).toBe("inc-b1");

    expect(ended).toBeTypeOf("function");
    const endedB = ended!;
    expect(endedB).not.toBe(endedA);

    // Resolve the stale callback from session A with refreshed epoch/incarnation
    await act(async () => {
      refreshADeferred.resolve({
        ...sessionA,
        daemonEpoch: "epoch-a2",
        incarnation: "inc-a2",
      });
    });

    // Guards prevent stale callback from reattaching old backend or clobbering identity
    const attachesA = tauriInvoke.mock.calls.filter(
      ([cmd, args]) => cmd === "cmd_native_terminal_attach" && args.sessionId === "stream-deferred-a",
    );
    expect(attachesA).toHaveLength(1);
    expect(
      tauriInvoke.mock.calls.some(
        ([cmd, args]) => cmd === "cmd_native_terminal_attach" && args.attachTuple?.daemonEpoch === "epoch-a2",
      ),
    ).toBe(false);

    // Trigger stream ended for session B to prove next live recovery remains usable
    await act(async () => {
      endedB({ payload: { sessionId: "stream-deferred-b" } });
    });
    await act(async () => {
      await refreshBStarted.promise;
    });
    expect(onRefreshSessionIdentity).toHaveBeenCalledWith(sessionB.id, expect.any(Function));

    const attachesB = tauriInvoke.mock.calls.filter(
      ([cmd, args]) => cmd === "cmd_native_terminal_attach" && args.sessionId === "stream-deferred-b",
    );
    expect(attachesB).toHaveLength(2);
    expect(attachesB[1][1].attachTuple.daemonEpoch).toBe("epoch-b2");
    expect(attachesB[1][1].attachTuple.incarnation).toBe("inc-b1");
    expect(attachesB[1][1].attachTuple.backendSessionId).toBe("stream-deferred-b");

    await act(async () => { view.unmount(); });
    tauriListen.mockImplementation(async () => () => undefined);
  });

  it.each(["stale-positive", "missing", "deferred"] as const)("held %s bounds cannot release outgoing detach before current presentation", async (receiptKind) => {
    const lifecycle = await import("../lib/nativeTerminalLifecycle");
    const started = deferred();
    const held = deferred<(typeof PRESENTED & { attachTuple: import("../lib/types").PaneAttachTuple }) | { presented: false; renderDeferred: true } | undefined>();
    const confirmed = deferred();
    const currentStarted = deferred();
    const currentBounds = deferred<typeof PRESENTED & { attachTuple: import("../lib/types").PaneAttachTuple }>();
    let first = true;
    tauriInvoke.mockImplementation(async (command) => {
      if (command === "cmd_native_terminal_set_bounds") {
        if (first) { first = false; started.resolve(); return held.promise; }
        currentStarted.resolve();
        return currentBounds.promise;
      }
      return undefined;
    });
    const current = { ...session("held-receipt"), incarnation: "life-1", daemonEpoch: "7" };
    lifecycle.resetNativeTerminalLifecycleForTest();
    const tuple = { backendSessionId: "held-receipt", incarnation: "life-1", daemonEpoch: "7",
      frontendSessionId: current.id, paneIdentity: current.id, bindingKey: "held-receipt:7:0::life-1", attemptGeneration: 1 };
    lifecycle.registerDurableNativeBinding(tuple);
    await lifecycle.attachNativeTerminalLifecycle("outgoing-receipt", async () => {});
    const detachOutgoing = vi.fn(async () => {});
    const detached = lifecycle.detachNativeTerminalLifecycle("outgoing-receipt", detachOutgoing);
    await lifecycle.attachNativeTerminalLifecycle("held-receipt", async () => {});
    const receipts: number[] = [];
    const unsubscribe = lifecycle.subscribeNativeTerminalPresentation({}, (receipt) => {
      receipts.push(receipt.attemptGeneration);
      confirmed.resolve();
    });
    const view = render(<NativeTerminalPane session={current} splitAttempt={{ frontendSessionId: current.id, generation: 1 }} />);
    await act(async () => { await started.promise; });
    await act(async () => {
      lifecycle.registerDurableNativeBinding({ ...tuple, attemptGeneration: 2 });
      view.rerender(<NativeTerminalPane session={current} splitAttempt={{ frontendSessionId: current.id, generation: 2 }} />);
      held.resolve(receiptKind === "missing" ? undefined : receiptKind === "deferred"
        ? { presented: false, renderDeferred: true }
        : { ...PRESENTED, attachTuple: tuple });
    });
    await act(async () => { await currentStarted.promise; });
    expect(detachOutgoing).not.toHaveBeenCalled();
    expect(receipts).toEqual([]);
    await act(async () => {
      currentBounds.resolve({ ...PRESENTED, attachTuple: { ...tuple, attemptGeneration: 2 } });
      await confirmed.promise;
      await detached;
    });
    expect(detachOutgoing).toHaveBeenCalledOnce();
    expect(receipts).toEqual([2]);
    expect(view.queryByTestId("native-terminal-error-backing")).toBeNull();
    unsubscribe();
  });

  it("recovers a stream that ends while its recovery attachment is pending", async () => {
    let ended: ((event: { payload: { sessionId: string } }) => void) | undefined;
    const recovery = deferred<void>();
    let attachments = 0;
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") ended = handler;
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async (command) => {
      if (command === "cmd_native_terminal_attach" && ++attachments === 2) return recovery.promise;
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    const view = render(<NativeTerminalPane session={session("overlapping-stream-end")} />);
    await act(async () => {});
    await act(async () => { ended!({ payload: { sessionId: "overlapping-stream-end" } }); });
    expect(attachments).toBe(2);
    await act(async () => {
      ended!({ payload: { sessionId: "overlapping-stream-end" } });
      recovery.resolve();
    });
    expect(attachments).toBe(3);
    await act(async () => { view.unmount(); });
    tauriListen.mockImplementation(async () => () => undefined);
  });

  it("reuses a legitimate A-B-A surface while B presentation is held", async () => {
    const aPresented = deferred();
    const bBoundsStarted = deferred();
    const bBounds = deferred<typeof PRESENTED>();
    const returnedBounds = deferred();
    let aBounds = 0;
    tauriInvoke.mockImplementation(async (command, args) => {
      if (command === "cmd_native_terminal_set_bounds") {
        if (args.sessionId === "reuse-b") {
          bBoundsStarted.resolve();
          return bBounds.promise;
        }
        if (args.sessionId === "reuse-a") {
          if (++aBounds === 1) aPresented.resolve();
          else returnedBounds.resolve();
        }
        return PRESENTED;
      }
      return undefined;
    });
    const a = session("reuse-a");
    const b = session("reuse-b");
    const view = render(<NativeTerminalPane session={a} active={false} />);
    await act(async () => { await aPresented.promise; });
    await act(async () => {
      view.rerender(<NativeTerminalPane session={b} active={false} />);
    });
    await bBoundsStarted.promise;
    await act(async () => {
      view.rerender(<NativeTerminalPane session={a} active={false} />);
    });
    await act(async () => { await returnedBounds.promise; });
    expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toEqual([
      ["cmd_native_terminal_attach", "reuse-a"],
      ["cmd_native_terminal_attach", "reuse-b"],
    ]);
    await act(async () => { bBounds.resolve(PRESENTED); });
    expect(lifecycleCalls()).not.toContainEqual(["cmd_native_terminal_detach", "reuse-a"]);
    expect(tauriInvoke.mock.calls.filter(([command]) => command.includes("spawn") || command === "cmd_native_terminal_send_input" || command === "cmd_native_terminal_set_focus")).toEqual([]);
  });

  it("recovers after 24 stream ends without exhaustion or mutation replay", async () => {
    let ended: ((event: { payload: { sessionId: string } }) => void) | undefined;
    const attached = Array.from({ length: 25 }, () => deferred());
    let attachmentCount = 0;
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") ended = handler;
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async (command) => {
      if (command === "cmd_native_terminal_attach") attached[attachmentCount++].resolve();
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    const view = render(<NativeTerminalPane session={session("bounded-stream")} active={false} />);
    await act(async () => { await attached[0].promise; });
    for (let index = 1; index < attached.length; index += 1) {
      await act(async () => {
        if (!ended) throw new Error("Stream recovery listener is unavailable");
        ended({ payload: { sessionId: "bounded-stream" } });
        await attached[index].promise;
      });
    }
    const attaches = () => lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach");
    expect(attaches()).toHaveLength(25);
    expect(attaches().every(([, id]) => id === "bounded-stream")).toBe(true);
    expect(view.queryByRole("alert")).toBeNull();
    expect(tauriInvoke.mock.calls.filter(([command]) => command === "cmd_native_terminal_set_focus")).toEqual([]);
    expect(tauriInvoke.mock.calls.some(([command]) => command.includes("spawn") || command === "cmd_native_terminal_send_input" || command === "cmd_native_terminal_set_focus")).toBe(false);
    expect(lifecycleCalls()).not.toContainEqual(["cmd_native_terminal_detach", "bounded-stream"]);
    await act(async () => { view.unmount(); });
    tauriListen.mockImplementation(async () => () => undefined);
  });

  it("does not attach without an output recovery listener when registration fails", async () => {
    // Given: the event transport rejects recovery listener registration.
    tauriListen.mockImplementation(async (event) => {
      if (event === "native_terminal_stream_ended") throw new Error("listener unavailable");
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async (command) =>
      command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined,
    );
    // When: the pane attempts its initial attachment.
    const view = render(<NativeTerminalPane session={session("listener-failure")} />);
    try {
      await act(async () => {});
      // Then: it remains actionable rather than starting an unmonitored stream.
      expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toHaveLength(0);
      expect(view.getByRole("alert")).toBeInTheDocument();
      tauriListen.mockImplementation(async () => () => undefined);
      await act(async () => { fireEvent.click(view.getByRole("alert")); });
      expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toEqual([
        ["cmd_native_terminal_attach", "listener-failure"],
      ]);
      expect(view.queryByRole("alert")).toBeNull();
    } finally {
      await act(async () => { view.unmount(); });
      tauriListen.mockImplementation(async () => () => undefined);
    }
  });

  it("waits for the output recovery listener before starting the stream", async () => {
    const registration = deferred<() => void>();
    tauriListen.mockImplementation(async (event) =>
      event === "native_terminal_stream_ended" ? registration.promise : () => undefined,
    );
    const view = render(<NativeTerminalPane session={session("listener-barrier")} />);
    await act(async () => {});
    expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toHaveLength(0);
    await act(async () => { registration.resolve(() => undefined); });
    expect(lifecycleCalls().filter(([command]) => command === "cmd_native_terminal_attach")).toEqual([
      ["cmd_native_terminal_attach", "listener-barrier"],
    ]);
    await act(async () => { view.unmount(); });
    tauriListen.mockImplementation(async () => () => undefined);
  });

  it("retries failed bounds with a fresh attachment and waits for presentation", async () => {
    // Given: attach succeeded, but the native surface cannot accept bounds until reattached.
    const failure = { code: "INTERNAL_ERROR", message: "native surface unavailable" };
    const recoveredBounds = deferred<typeof PRESENTED>();
    let attachments = 0;
    tauriInvoke.mockImplementation(async (command) => {
      if (command === "cmd_native_terminal_attach") attachments += 1;
      if (command === "cmd_native_terminal_set_bounds") {
        if (attachments < 2) throw failure;
        return recoveredBounds.promise;
      }
      return undefined;
    });
    const view = render(<NativeTerminalPane session={session("bounds-retry")} />);
    await act(async () => {});

    // When: the user requests recovery without changing the pane geometry.
    await act(async () => { fireEvent.click(view.getByRole("alert")); });

    // Then: recovery stays actionable without covering the last terminal frame.
    expect(attachments).toBe(2);
    expect(view.getByRole("alert")).toBeInTheDocument();
    expect(view.queryByTestId("native-terminal-error-backing")).toBeNull();
    await act(async () => { recoveredBounds.resolve(PRESENTED); });
    expect(view.queryByRole("alert")).toBeNull();
    expect(view.queryByTestId("native-terminal-error-backing")).toBeNull();
    expect(lifecycleCalls()).not.toContainEqual(["cmd_native_terminal_detach", "bounds-retry"]);
  });

  it("preserves the structured cause of a bounds failure in the visible error", async () => {
    // Given: the backend supplies a typed render failure.
    const failure = { code: "INVALID_ARGUMENT", message: "Native terminal viewport is outside the configured surface" };
    tauriInvoke.mockImplementation(async (command) => {
      if (command === "cmd_native_terminal_set_bounds") throw failure;
      return undefined;
    });

    // When: a visible pane reports its bounds.
    const view = render(<NativeTerminalPane session={session("bounds-cause")} />);
    await act(async () => {});

    // Then: both backend fields survive the UI boundary instead of becoming a generic banner.
    expect(view.getByRole("alert")).toHaveTextContent(failure.code);
    expect(view.getByRole("alert")).toHaveTextContent(failure.message);
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
    await act(async () => { boundary.bounds.resolve(PRESENTED); });
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
    await act(async () => { boundary.input.reject(INPUT_NOT_WRITTEN); await boundary.recoveryStarted.promise; });

    HTMLElement.prototype.getBoundingClientRect = () => ({ ...PANE_RECT, x: 90, y: 100, width: 400, height: 300 });
    await act(async () => { view.rerender(<NativeTerminalPane session={session("owner-b")} />); });
    await boundary.boundsStarted.promise;
    await act(async () => { boundary.bounds.resolve(PRESENTED); });
    const anchor = view.getByTestId("native-terminal-focus-sink").getAttribute("style");
    await act(async () => { boundary.recovery.resolve(); });

    expect(tauriInvoke.mock.calls.filter(([cmd]) => cmd === "cmd_native_terminal_send_input")).toHaveLength(1);
    expect(view.getByTestId("native-terminal-focus-sink").getAttribute("style")).toBe(anchor);
    expect(view.queryByRole("alert")).toBeNull();
    expect(lifecycleCalls().filter(([cmd]) => cmd === "cmd_native_terminal_detach")).toEqual([["cmd_native_terminal_detach", "owner-a"]]);
  });

  it.each([false, true])("guards queued recovery attachment while preserving a returning owner (returns=%s)", async (returns) => {
    const boundary = inputRecoveryBoundary();
    const initialAttach = deferred();
    const returningBounds = deferred();
    const invokeBoundary = tauriInvoke.getMockImplementation();
    let initial = true;
    tauriInvoke.mockImplementation(async (cmd, args) => {
      if (cmd === "cmd_native_terminal_set_bounds" && args.sessionId === "owner-a") returningBounds.resolve();
      const result = invokeBoundary?.(cmd, args);
      if (cmd === "cmd_native_terminal_attach" && args.sessionId === "owner-a" && initial) {
        initial = false;
        await initialAttach.promise;
      }
      return result;
    });
    const view = render(<NativeTerminalPane session={session("owner-a")} />);
    await startPendingInput(view, boundary);
    await act(async () => { boundary.input.reject(INPUT_NOT_WRITTEN); });
    expect(lifecycleCalls().filter(([cmd]) => cmd === "cmd_native_terminal_attach")).toHaveLength(1);

    if (returns) {
      await act(async () => { view.rerender(<NativeTerminalPane session={session("owner-b")} />); });
      await act(async () => { await boundary.boundsStarted.promise; });
      HTMLElement.prototype.getBoundingClientRect = () => ({ ...PANE_RECT, x: 150, width: 500 });
      await act(async () => { view.rerender(<NativeTerminalPane session={session("owner-a")} />); });
    } else {
      await act(async () => { view.unmount(); });
    }
    // The owner returns before the replacement presents (base ordering): once owner-b has
    // presented, releasing owner-a's held detachment is the correct lifecycle outcome.
    await act(async () => { initialAttach.resolve(); boundary.bounds.resolve(PRESENTED); });
    if (returns) await act(async () => { await returningBounds.promise; });

    expect(tauriInvoke.mock.calls.filter(([cmd]) => cmd === "cmd_native_terminal_send_input")).toHaveLength(1);
    const attaches = tauriInvoke.mock.calls.filter(([cmd, args]) => cmd === "cmd_native_terminal_attach" && args.sessionId === "owner-a");
    expect(attaches).toHaveLength(returns ? 2 : 1);
    if (returns) {
      expect(attaches[1]?.[1].bounds).toEqual({ x: 150, y: 20, width: 500, height: 600 });
      expect(lifecycleCalls()).not.toContainEqual(["cmd_native_terminal_detach", "owner-a"]);
      expect(tauriInvoke).toHaveBeenCalledWith("cmd_native_terminal_set_bounds", expect.objectContaining({ sessionId: "owner-a" }));
    } else {
      await act(async () => {
        await waitFor(() => expect(lifecycleCalls()).toContainEqual(["cmd_native_terminal_detach", "owner-a"]));
      });
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
      await act(async () => { boundary.input.reject(INPUT_NOT_WRITTEN); await boundary.recoveryStarted.promise; });
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

  it("retries an unwritten input before any later queued input, with one recovery attach", async () => {
    // ABC with A failing must stay ABC: the failed keystroke keeps the session lane
    // through recovery, so later keystrokes cannot overtake its retry.
    const boundary = inputRecoveryBoundary();
    boundary.holdRecovery();
    const view = render(<NativeTerminalPane session={session("owner-a")} />);
    await act(async () => {});
    await startPendingInput(view, boundary);
    fireEvent.input(view.getByTestId("native-terminal-focus-sink"), { target: { value: "y" } });
    await act(async () => {
      boundary.input.reject(INPUT_NOT_WRITTEN);
      await boundary.recoveryStarted.promise;
    });
    const sentTexts = () => tauriInvoke.mock.calls
      .filter(([cmd]) => cmd === "cmd_native_terminal_send_input")
      .map(([, args]) => args.input.text);
    // "y" is held behind the recovering "x".
    expect(sentTexts()).toEqual(["x"]);
    await act(async () => { boundary.recovery.resolve(); });
    await waitFor(() => expect(sentTexts()).toEqual(["x", "x", "y"]));
    expect(lifecycleCalls().filter(([cmd]) => cmd === "cmd_native_terminal_attach")).toHaveLength(2);
    expect(view.queryByRole("alert")).toBeNull();
  });

  it.each([
    new Error("Input receipt failed after delivery"),
    { code: "INTERNAL_ERROR", message: "Receipt unavailable" },
    { code: "PTY_IO_ERROR", message: "Write acknowledgement unavailable", details: { inputWritten: true } },
  ])("does not replay Ctrl+C when delivery is ambiguous: %j", async (failure) => {
    // Given: the write succeeds, but its IPC response fails afterward.
    const writes: string[] = [];
    tauriInvoke.mockImplementation(async (command, args) => {
      if (command === "cmd_native_terminal_set_bounds") return PRESENTED;
      if (command === "cmd_native_terminal_send_input") {
        writes.push(args.input.keyEvent.key);
        throw failure;
      }
      return undefined;
    });
    const view = render(<NativeTerminalPane session={session("accepted-input")} />);
    await act(async () => {});

    // When: one physical Ctrl+C reaches the focus sink.
    await act(async () => {
      fireEvent.keyDown(view.getByTestId("native-terminal-focus-sink"), {
        key: "c", code: "KeyC", ctrlKey: true,
      });
    });

    // Then: no reattachment or replay can turn cancellation into termination.
    expect(writes).toEqual(["c"]);
    expect(lifecycleCalls().filter(([cmd]) => cmd === "cmd_native_terminal_attach")).toHaveLength(1);
    if (typeof failure === "object" && "details" in failure && failure.details?.inputWritten === true) {
      // The bytes reached the PTY; the failure is a lost receipt only, so the pane
      // must suppress the error banner (real-surface regression: "failed to send
      // to terminal" shown after successful PTY writes).
      expect(view.queryByRole("alert")).toBeNull();
    } else {
      expect(view.getByRole("alert")).toBeInTheDocument();
    }
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
    const incomingPresented = deferred();
    const outgoingDetached = deferred();
    tauriInvoke.mockImplementation(async (command, args) => {
      if (command === "cmd_native_terminal_detach" && args?.sessionId === "frame-outgoing") outgoingDetached.resolve();
      if (command !== "cmd_native_terminal_set_bounds") return undefined;
      const presented = args?.sessionId !== "frame-incoming" || ++attempts > 1;
      const receipt = { presented, cols: 80, rows: 24, cursorCol: 0, cursorRow: 0, cellWidthPx: 10, cellHeightPx: 20 };
      if (args?.sessionId === "frame-incoming" && presented) incomingPresented.resolve();
      return receipt;
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
      await incomingPresented.promise;
    });
    expect(attempts).toBe(2);
    await act(async () => { await outgoingDetached.promise; });
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

  it("retries a cancelled recovery admission instead of latching the pane blank", async () => {
    vi.useFakeTimers();
    let ended: ((event: { payload: { sessionId: string } }) => void) | undefined;
    const registered = deferred(); const admitted = deferred();
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") { ended = handler; registered.resolve(); }
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async (command, args) => {
      if (command === "cmd_native_terminal_attach" && args.attachTuple.daemonEpoch === "admitted") admitted.resolve();
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    const retained = { ...session("cancelled-admission"), incarnation: "process" };
    let calls = 0;
    const refresh = vi.fn((_id: string, result: (value: import("../state/workspaceStore").LocalRecoveryAdmissionResult) => void) => {
      result(++calls === 1 ? { kind: "cancelled" } : { kind: "ready", session: { ...retained, daemonEpoch: "admitted" } });
      return () => {};
    });
    const view = render(<NativeTerminalPane session={retained} onRefreshSessionIdentity={refresh} />);
    try {
      await act(async () => { await registered.promise; });
      await act(async () => { ended!({ payload: { sessionId: "cancelled-admission" } }); });
      expect(refresh).toHaveBeenCalledTimes(1);

      await act(async () => { await vi.advanceTimersByTimeAsync(500); await admitted.promise; });

      expect(refresh).toHaveBeenCalledTimes(2);
      expect(tauriInvoke.mock.calls.some(([command]) => command.includes("spawn"))).toBe(false);
    } finally {
      await act(async () => { view.unmount(); });
      vi.useRealTimers();
      tauriListen.mockImplementation(async () => () => undefined);
    }
  });

  it("shows a clickable error when an exited admission latches recovery", async () => {
    let ended: ((event: { payload: { sessionId: string } }) => void) | undefined;
    const registered = deferred(); const started = deferred();
    tauriListen.mockImplementation(async (event, handler) => {
      if (event === "native_terminal_stream_ended") { ended = handler; registered.resolve(); }
      return () => undefined;
    });
    tauriInvoke.mockImplementation(async command => command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined);
    const retained = { ...session("exited-admission"), incarnation: "process" };
    const refresh = vi.fn((_id: string, result: (value: import("../state/workspaceStore").LocalRecoveryAdmissionResult) => void) => {
      started.resolve(); result({ kind: "exited" }); return () => {};
    });
    const view = render(<NativeTerminalPane session={retained} onRefreshSessionIdentity={refresh} />);
    await act(async () => { await registered.promise; });
    await act(async () => { ended!({ payload: { sessionId: "exited-admission" } }); await started.promise; });

    expect(view.getByRole("alert").textContent).toContain("Terminal session ended");
    await act(async () => { view.unmount(); });
    tauriListen.mockImplementation(async () => () => undefined);
  });

  it("surfaces exhausted unpresented retries and re-reports bounds on click", async () => {
    vi.useFakeTimers();
    const frames = new Map<number, FrameRequestCallback>();
    let nextFrame = 0;
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.set(++nextFrame, callback);
      return nextFrame;
    });
    vi.stubGlobal("cancelAnimationFrame", (id: number) => { frames.delete(id); });
    const settle = async () => {
      // Each round runs the queued frame callbacks, then the longest retry backoff (2s).
      for (let round = 0; round < 20; round++) {
        await act(async () => {
          const callbacks = [...frames.values()];
          frames.clear();
          for (const callback of callbacks) callback(16);
          await vi.advanceTimersByTimeAsync(2_000);
        });
      }
    };
    tauriInvoke.mockImplementation(async command => command === "cmd_native_terminal_set_bounds"
      ? { presented: false, cols: 80, rows: 24, cursorCol: 0, cursorRow: 0, cellWidthPx: 10, cellHeightPx: 20 }
      : undefined);
    const boundsCalls = () => tauriInvoke.mock.calls.filter(([command]) => command === "cmd_native_terminal_set_bounds").length;
    const view = render(<NativeTerminalPane session={session("unpresented-exhausted")} />);
    try {
      await settle();
      const exhausted = boundsCalls();
      expect(exhausted).toBeGreaterThanOrEqual(11);
      const alert = view.getByRole("alert");
      await settle();
      expect(boundsCalls()).toBe(exhausted);

      await act(async () => { fireEvent.click(alert); });

      expect(boundsCalls()).toBeGreaterThan(exhausted);
    } finally {
      await act(async () => { view.unmount(); });
      vi.useRealTimers();
    }
  });

  it("re-sends bounds with the current tuple after a stale_tuple refusal", async () => {
    const resent = deferred();
    const boundsTuples: number[] = [];
    tauriInvoke.mockImplementation(async (command, args) => {
      if (command !== "cmd_native_terminal_set_bounds") return undefined;
      boundsTuples.push(args.attachTuple.attemptGeneration);
      if (boundsTuples.length === 1) {
        // The pane rotated its tuple while this request was in flight.
        registerDurableNativeBinding({ ...args.attachTuple, attemptGeneration: args.attachTuple.attemptGeneration + 1 });
        throw { code: "SESSION_NOT_FOUND", message: "bound to a newer attach tuple",
          details: { reason: "stale_tuple", sessionId: args.sessionId } };
      }
      resent.resolve();
      return PRESENTED;
    });
    const view = render(<NativeTerminalPane session={session("stale-bounds")} />);
    await act(async () => { await resent.promise; });

    expect(boundsTuples).toEqual([0, 1]);
    expect(view.queryByRole("alert")).toBeNull();
    await act(async () => { view.unmount(); });
  });

  it("retries attach when an exit proof names a tuple the pane already rotated away from", async () => {
    vi.useFakeTimers();
    const id = "rotated-exit";
    const tuple = { backendSessionId: id, incarnation: "inc", daemonEpoch: "7", frontendSessionId: `frontend-${id}`,
      paneIdentity: `frontend-${id}`, bindingKey: `${id}:7:0::inc`, attemptGeneration: 0 };
    registerDurableNativeBinding(tuple);
    const retried = deferred();
    let attaches = 0;
    tauriInvoke.mockImplementation(async (command, args) => {
      if (command === "cmd_native_terminal_attach") {
        if (++attaches === 1) {
          registerDurableNativeBinding({ ...args.attachTuple, attemptGeneration: args.attachTuple.attemptGeneration + 1 });
          throw { code: "SESSION_NOT_FOUND", message: `Session '${id}' not found`, details: {
            source: "daemon_attach", kind: "session_not_found", sessionId: id, affirmativeExit: true,
            incarnation: "inc", daemonEpoch: "7",
          } };
        }
        retried.resolve();
      }
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    const unavailable = vi.fn();
    const pane: TerminalSession = { id: `frontend-${id}`, cwd: "/workspace/orca-lite", workspaceId: "ws-main",
      worktree: { wsId: "ws-main", slug: "main" }, backendSessionId: id, lifecycle: "working",
      daemonEpoch: "7", incarnation: "inc" };
    const view = render(<NativeTerminalPane session={pane} onBackendSessionUnavailable={unavailable} />);
    try {
      await act(async () => {});
      expect(attaches).toBe(1);

      await act(async () => { await vi.advanceTimersByTimeAsync(250); await retried.promise; });

      expect(attaches).toBe(2);
      expect(unavailable).not.toHaveBeenCalled();
    } finally {
      await act(async () => { view.unmount(); });
      vi.useRealTimers();
    }
  });

  it("does not block a new split pane attach behind an unrelated session", async () => {
    const oldAttach = deferred();
    const oldStarted = deferred();
    const newStarted = deferred();
    const newBounds = deferred<typeof PRESENTED>();
    const newBoundsStarted = deferred();
    const oldDetached = deferred();
    const newPresented = deferred();

    tauriCoreMocks.invoke.mockImplementation(async (cmd, args) => {
      if (cmd === "cmd_native_terminal_attach" && args?.sessionId === "backend-a") {
        oldStarted.resolve();
        await oldAttach.promise;
      }
      if (cmd === "cmd_native_terminal_attach" && args?.sessionId === "backend-b") newStarted.resolve();
      if (cmd === "cmd_native_terminal_set_bounds" && args?.sessionId === "backend-b") {
        newBoundsStarted.resolve();
        const receipt = await newBounds.promise;
        newPresented.resolve();
        return receipt;
      }
      if (cmd === "cmd_native_terminal_detach" && args?.sessionId === "backend-a") oldDetached.resolve();
      return undefined;
    });

    const view = render(<NativeTerminalPane session={session("backend-a")} />);

    await act(async () => { await oldStarted.promise; });
    expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-a"]]);

    await act(async () => { view.rerender(<NativeTerminalPane session={session("backend-b")} />); });
    await act(async () => { await newStarted.promise; await newBoundsStarted.promise; });

    expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-a"],
        ["cmd_native_terminal_attach", "backend-b"],
      ]);

    await act(async () => { newBounds.resolve(PRESENTED); await newPresented.promise; });
    await act(async () => { oldAttach.resolve(); await oldDetached.promise; });

    expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-a"],
        ["cmd_native_terminal_attach", "backend-b"],
        ["cmd_native_terminal_detach", "backend-a"],
      ]);
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

  it("detaches and blocks input outside macOS while the full-screen Settings surface covers the active pane", async () => {
    vi.spyOn(shortcuts, "isMacShortcutPlatform").mockReturnValue(false);
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

  it("detaches and blocks input outside macOS while a mounted role=dialog New Tab menu covers the active pane", async () => {
    vi.spyOn(shortcuts, "isMacShortcutPlatform").mockReturnValue(false);
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

  it("detaches and blocks input outside macOS while a mounted role=search Terminal search overlay covers the active pane", async () => {
    vi.spyOn(shortcuts, "isMacShortcutPlatform").mockReturnValue(false);
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
