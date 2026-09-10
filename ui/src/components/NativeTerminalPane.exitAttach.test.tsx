import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TerminalSession } from "../lib/types";
import { resetNativeTerminalLifecycleForTest } from "../lib/nativeTerminalLifecycle";
import { NativeTerminalPane, resetNativeTerminalPaneForTest } from "./NativeTerminalPane";

const tauriCoreMocks = vi.hoisted(() => ({
  invoke: vi.fn<(cmd: string, args?: any) => Promise<any>>(async () => undefined),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCoreMocks.invoke,
  isTauri: tauriCoreMocks.isTauri,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onDragDropEvent: async () => () => undefined,
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: async () => () => undefined,
}));

vi.mock("../lib/tauri", async (original) => ({
  ...(await original<typeof import("../lib/tauri")>()),
  onNativeTerminalFocus: async () => () => undefined,
  onNativeTerminalPaste: async () => () => undefined,
  onNativeTerminalCopyOrInterrupt: async () => () => undefined,
  onNativeTerminalScrollbar: async () => () => undefined,
  setNativeTerminalScrollbarOverlay: async () => undefined,
  setNativeTerminalAttentionFrame: async () => undefined,
}));

const PRESENTED = {
  presented: true,
  cursorCol: 0,
  cursorRow: 0,
  cellWidthPx: 8,
  cellHeightPx: 16,
};

function createSession(
  id = "term-pane-test",
  backendSessionId: string | null = "backend-target-1",
  lifecycle: TerminalSession["lifecycle"] = "working",
): TerminalSession {
  return {
    id,
    cwd: "/workspace/project",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId,
    lifecycle,
  };
}

describe("NativeTerminalPane abnormal exit attach handling", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("navigator", { platform: "MacIntel", userAgent: "Macintosh" });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
      new DOMRect(10, 20, 800, 600),
    );
    resetNativeTerminalLifecycleForTest();
    resetNativeTerminalPaneForTest();
    tauriCoreMocks.invoke.mockReset();
    tauriCoreMocks.isTauri.mockReturnValue(true);
    tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_set_bounds") return PRESENTED;
      return undefined;
    });
  });

  afterEach(async () => {
    await act(async () => {
      cleanup();
    });
    vi.clearAllTimers();
    vi.useRealTimers();
    resetNativeTerminalLifecycleForTest();
    resetNativeTerminalPaneForTest();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("suppresses error badge and cancels retry when cmd_native_terminal_attach returns SESSION_NOT_FOUND with typed details", async () => {
    const session = createSession("pane-1", "dead-backend-session");
    const onUnavailable = vi.fn();

    tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_attach") {
        throw {
          code: "SESSION_NOT_FOUND",
          message: "Session 'dead-backend-session' not found",
          details: {
            source: "daemon_attach",
            kind: "session_not_found",
            sessionId: "dead-backend-session",
          },
        };
      }
      if (cmd === "cmd_native_terminal_set_bounds") return PRESENTED;
      return undefined;
    });

    const { queryByRole } = render(
      <NativeTerminalPane
        sessionId="pane-1"
        session={session}
        onBackendSessionUnavailable={onUnavailable}
      />,
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });

    const alert = queryByRole("alert");
    expect(alert).toBeNull();
    expect(onUnavailable).toHaveBeenCalledWith("dead-backend-session", "daemon-attach-not-found");
  });

  it("ignores late attach rejection without surfacing alert badge when session exits during in-flight attach", async () => {
    let rejectAttach: ((err: any) => void) | null = null;
    const attachPromise = new Promise((_resolve, reject) => {
      rejectAttach = reject;
    });

    tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_attach") return attachPromise;
      if (cmd === "cmd_native_terminal_set_bounds") return PRESENTED;
      return undefined;
    });

    const liveSession = createSession("pane-1", "backend-1", "working");
    const { rerender, queryByRole } = render(
      <NativeTerminalPane sessionId="pane-1" session={liveSession} />,
    );

    await act(async () => {
      await Promise.resolve();
    });

    const exitedSession = createSession("pane-1", "backend-1", "exited");
    rerender(<NativeTerminalPane sessionId="pane-1" session={exitedSession} />);

    await act(async () => {
      rejectAttach?.({
        code: "SESSION_NOT_FOUND",
        message: "Session 'backend-1' not found",
      });
      await vi.advanceTimersByTimeAsync(1000);
    });

    expect(queryByRole("alert")).toBeNull();
  });

  it("suppresses error badge for legacy INTERNAL_ERROR with exact Session not found message", async () => {
    const session = createSession("pane-legacy", "legacy-dead-session");
    const onUnavailable = vi.fn();

    tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_attach") {
        throw {
          code: "INTERNAL_ERROR",
          message: "Session 'legacy-dead-session' not found",
        };
      }
      if (cmd === "cmd_native_terminal_set_bounds") return PRESENTED;
      return undefined;
    });

    const { queryByRole } = render(
      <NativeTerminalPane
        sessionId="pane-legacy"
        session={session}
        onBackendSessionUnavailable={onUnavailable}
      />,
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });

    expect(queryByRole("alert")).toBeNull();
    expect(onUnavailable).toHaveBeenCalledWith("legacy-dead-session", "legacy-internal-error");
  });

  it("preserves visible error badge for genuine operational errors like IO_ERROR or GPU surface failure", async () => {
    const session = createSession("pane-op", "live-backend-session");

    tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_attach") {
        throw {
          code: "IO_ERROR",
          message: "Connection refused to daemon socket",
        };
      }
      if (cmd === "cmd_native_terminal_set_bounds") return PRESENTED;
      return undefined;
    });

    const { queryByRole } = render(
      <NativeTerminalPane sessionId="pane-op" session={session} />,
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(800);
    });

    const alert = queryByRole("alert");
    expect(alert).not.toBeNull();
    expect(alert).toHaveTextContent("Failed to attach native terminal");
  });
});
