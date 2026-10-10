import "@testing-library/jest-dom/vitest";
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TerminalSession } from "../lib/types";
import {
  resetNativeTerminalLifecycleForTest,
  getDurableNativeBinding,
} from "../lib/nativeTerminalLifecycle";
import {
  NativeTerminalPane,
  resetNativeTerminalPaneForTest,
} from "./NativeTerminalPane";

const tauriCoreMocks = vi.hoisted(() => ({
  invoke: vi.fn<(cmd: string, args?: any) => Promise<any>>(async () => undefined),
  isTauri: vi.fn(() => true),
}));

const tauriEventMocks = vi.hoisted(() => ({
  listeners: new Map<string, Set<(event: { payload: unknown }) => void>>(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    const listeners = tauriEventMocks.listeners.get(name) ?? new Set();
    listeners.add(handler);
    tauriEventMocks.listeners.set(name, listeners);
    return () => {
      listeners.delete(handler);
    };
  }),
}));

const toastMocks = vi.hoisted(() => ({
  error: vi.fn(),
  info: vi.fn(),
  loading: vi.fn(),
  success: vi.fn(),
  dismiss: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    error: (...args: any[]) => toastMocks.error(...args),
    info: (...args: any[]) => toastMocks.info(...args),
    loading: (...args: any[]) => toastMocks.loading(...args),
    success: (...args: any[]) => toastMocks.success(...args),
    dismiss: (...args: any[]) => toastMocks.dismiss(...args),
  },
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
  inputReceiptListeners: [] as Array<
    (payload: {
      sessionId: string;
      presented: boolean;
      cursorCol: number;
      cursorRow: number;
      cellWidthPx: number;
      cellHeightPx: number;
    }) => void
  >,
  focusListeners: [] as Array<(sessionId: string) => void>,
  pasteListeners: [] as Array<() => void>,
  copyOrInterruptListeners: [] as Array<() => void>,
  scrollbarOverlayCalls: [] as Array<{ sessionId: string; visible: boolean }>,
  setNativeTerminalScrollbarOverlay: vi.fn(async (sessionId: string, visible: boolean) => {
    nativeTerminalEventMocks.scrollbarOverlayCalls.push({ sessionId, visible });
  }),
  attentionFrameCalls: [] as Array<{ sessionId: string; attention: boolean }>,
  setNativeTerminalAttentionFrame: vi.fn(async (sessionId: string, attention: boolean) => {
    nativeTerminalEventMocks.attentionFrameCalls.push({ sessionId, attention });
  }),
  onNativeTerminalScrollbar: vi.fn(async (handler: (payload: {
    sessionId: string;
    total: number;
    offset: number;
    len: number;
  }) => void) => {
    nativeTerminalEventMocks.scrollbarListener = handler;
    return () => undefined;
  }),
  onNativeTerminalInputReceipt: vi.fn(async () => () => undefined),
  onNativeTerminalFocus: vi.fn(async () => () => undefined),
  onNativeTerminalPaste: vi.fn(async () => () => undefined),
  onNativeTerminalCopyOrInterrupt: vi.fn(async () => () => undefined),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string, args?: Record<string, unknown>) => {
    const result = await (args === undefined
      ? tauriCoreMocks.invoke(command)
      : tauriCoreMocks.invoke(command, args));
    if (command === "cmd_native_terminal_set_bounds" && result && typeof result === "object") {
      return { attachTuple: args?.attachTuple, ...result };
    }
    return result;
  },
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
  onNativeTerminalCopyOrInterrupt: nativeTerminalEventMocks.onNativeTerminalCopyOrInterrupt,
  onNativeTerminalScrollbar: nativeTerminalEventMocks.onNativeTerminalScrollbar,
  onNativeTerminalInputReceipt: nativeTerminalEventMocks.onNativeTerminalInputReceipt,
  setNativeTerminalScrollbarOverlay: nativeTerminalEventMocks.setNativeTerminalScrollbarOverlay,
  setNativeTerminalAttentionFrame: nativeTerminalEventMocks.setNativeTerminalAttentionFrame,
  attachTerminal: vi.fn(async (request) => tauriCoreMocks.invoke("cmd_terminal_attach", request)),
}));

const splitLifecycleMocks = vi.hoisted(() => ({
  delayPersist: false,
  resolvePersist: null as (() => void) | null,
}));

vi.mock("../lib/localSplitLifecycle", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/localSplitLifecycle")>()),
  persistNativeBinding: async (session: TerminalSession) => {
    if (splitLifecycleMocks.delayPersist) {
      await new Promise<void>((resolve) => {
        splitLifecycleMocks.resolvePersist = resolve;
      });
    }
    await Promise.resolve();
    persistedNativeSessions.set(session.id, structuredClone(session));
  },
}));

const persistedNativeSessions = new Map<string, TerminalSession>();

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

  constructor(public callback: ResizeObserverCallback) {}
}

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

async function renderNative(...args: Parameters<typeof render>) {
  const view = render(...args);
  await act(async () => undefined);
  return view;
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

describe("NativeTerminalPane auditAttach regression tests", () => {
  let restorePaneRect: () => void;

  beforeEach(() => {
    restorePaneRect = stubPaneRect();
    resetNativeTerminalPaneForTest();
    resetNativeTerminalLifecycleForTest();
    persistedNativeSessions.clear();
    splitLifecycleMocks.delayPersist = false;
    splitLifecycleMocks.resolvePersist = null;
    tauriEventMocks.listeners.clear();
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
    vi.unstubAllGlobals();
  });

  it("cleans up native lifecycle on unmount even when teardownTuple was uninitialized, allowing remounted pane to attach", async () => {
    const targetSessionId = "backend-session-uninit-detach";
    const session = createSession("term-session-uninit-detach", targetSessionId);

    expect(getDurableNativeBinding(targetSessionId)).toBeUndefined();

    const attachCalls: Array<{ sessionId: string; attachTuple: any }> = [];
    const detachCalls: Array<{ sessionId: string; attachTuple: any }> = [];

    tauriCoreMocks.invoke.mockImplementation(async (cmd, args) => {
      if (cmd === "cmd_native_terminal_attach") {
        attachCalls.push({ sessionId: args?.sessionId, attachTuple: args?.attachTuple });
        return undefined;
      }
      if (cmd === "cmd_native_terminal_detach") {
        detachCalls.push({ sessionId: args?.sessionId, attachTuple: args?.attachTuple });
        return undefined;
      }
      return undefined;
    });

    splitLifecycleMocks.delayPersist = true;

    const { unmount } = await renderNative(
      <NativeTerminalPane sessionId={session.id} session={session} />,
    );

    expect(attachCalls).toHaveLength(0);

    act(() => {
      unmount();
    });

    await act(async () => {
      splitLifecycleMocks.resolvePersist?.();
      await Promise.resolve();
    });

    splitLifecycleMocks.delayPersist = false;
    attachCalls.length = 0;

    await renderNative(
      <NativeTerminalPane sessionId={session.id} session={session} />,
    );

    expect(attachCalls).toHaveLength(1);
    expect(attachCalls[0].sessionId).toBe(targetSessionId);
  });
});
