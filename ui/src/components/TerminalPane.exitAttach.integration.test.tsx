import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TerminalSession, TerminalTab } from "../lib/types";
import { resetNativeTerminalLifecycleForTest } from "../lib/nativeTerminalLifecycle";
import { resetNativeTerminalPaneForTest } from "./NativeTerminalPane";
import { TerminalPane } from "./TerminalPane";
import { workspaceReducer, type WorkspaceState } from "../state/workspaceStore";
import { createLayoutState } from "../state/layout";

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

describe("TerminalPane exit attach integration", () => {
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

  it("propagates session unavailable on attach failure, transitions session to exited, and displays Shell exited overlay without error badge", async () => {
    let currentSession: TerminalSession = {
      id: "term-pane-1",
      cwd: "/repo/path",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-dead-pty",
      lifecycle: "working",
    };

    tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_attach") {
        throw {
          code: "SESSION_NOT_FOUND",
          message: "Session 'backend-dead-pty' not found",
          details: {
            source: "daemon_attach",
            kind: "session_not_found",
            sessionId: "backend-dead-pty",
          },
        };
      }
      if (cmd === "cmd_native_terminal_set_bounds") return PRESENTED;
      return undefined;
    });

    const onOpenNewShell = vi.fn();
    const handleUnavailable = vi.fn((sessionId: string, backendSessionId: string, reason: string) => {
      const tab: TerminalTab = {
        id: "tab-1",
        label: "Test",
        sessionId: currentSession.id,
      };
      const state: WorkspaceState = {
        workspaceId: "ws-local",
        layout: createLayoutState([tab], "tab-1"),
        worktrees: [],
        activeWorktreePath: "/repo/path",
        worktreeLayouts: {},
        unreadTabIds: {},
        unreadWorktreePaths: {},
        sessions: {
          [currentSession.id]: currentSession,
        },
        activityBySessionId: {},
      };

      const next = workspaceReducer(state, {
        type: "SESSION_BACKEND_UNAVAILABLE",
        sessionId,
        backendSessionId,
        reason,
      });

      currentSession = next.sessions[sessionId];
    });

    const { rerender } = render(
      <TerminalPane
        session={currentSession}
        active={true}
        onOpenNewShell={onOpenNewShell}
        onBackendSessionUnavailable={handleUnavailable}
      />,
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(500);
    });

    expect(handleUnavailable).toHaveBeenCalledWith("term-pane-1", "backend-dead-pty", "daemon-attach-not-found");
    expect(currentSession.lifecycle).toBe("exited");
    expect(currentSession.backendSessionId).toBeNull();

    rerender(
      <TerminalPane
        session={currentSession}
        active={true}
        onOpenNewShell={onOpenNewShell}
        onBackendSessionUnavailable={handleUnavailable}
      />,
    );

    expect(screen.getByText("Shell exited")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /open new shell/i })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /open new shell/i }));
    expect(onOpenNewShell).toHaveBeenCalledWith("term-pane-1");
  });
});
