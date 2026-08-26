import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { NativeTerminalBellPayload, NativeTerminalTitlePayload, Worktree } from "../lib/types";

/**
 * Guards the defect a green suite missed: activity was fed by the legacy `terminal_output` OSC
 * scanner, which native-ghostty sessions never stream. Only the native events reach background
 * tabs, whose panes `TerminalSplitView` unmounts.
 */
const nativeListeners = vi.hoisted(() => ({
  title: new Set<(payload: NativeTerminalTitlePayload) => void>(),
  bell: new Set<(payload: NativeTerminalBellPayload) => void>(),
}));

vi.mock("../lib/tauri", () => ({
  DEFAULT_WORKSPACE_ID: "default",
  spawnTerminal: vi.fn(async () => "backend-1"),
  closeTerminal: vi.fn(async () => undefined),
  getTerminalCwd: vi.fn(async () => "/repo/main"),
  waitForTerminalExit: vi.fn(async () => undefined),
  onNativeTerminalTitle: vi.fn(async (handler: (payload: NativeTerminalTitlePayload) => void) => {
    nativeListeners.title.add(handler);
    return () => nativeListeners.title.delete(handler);
  }),
  onNativeTerminalBell: vi.fn(async (handler: (payload: NativeTerminalBellPayload) => void) => {
    nativeListeners.bell.add(handler);
    return () => nativeListeners.bell.delete(handler);
  }),
}));

const { useWorkspaceStore } = await import("./workspaceStore");
type WorkspaceServices = import("./workspaceStore").WorkspaceServices;

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/orca/ws-main/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function services(): WorkspaceServices {
  return {
    ensureTerminalEvents: vi.fn(async () => undefined),
    spawnTerminal: vi.fn(async () => "backend-1"),
    getTerminalCwd: vi.fn(async () => worktree.path),
    closeTerminal: vi.fn(async () => undefined),
    waitForTerminalExit: vi.fn(async () => undefined),
  };
}

function emitNativeTitle(payload: NativeTerminalTitlePayload): void {
  for (const listener of nativeListeners.title) listener(payload);
}

function emitNativeBell(payload: NativeTerminalBellPayload): void {
  for (const listener of nativeListeners.bell) listener(payload);
}

describe("workspace store native activity subscription", () => {
  beforeEach(() => {
    nativeListeners.title.clear();
    nativeListeners.bell.clear();
  });

  it("turns a native title event addressed by BACKEND session id into tab activity", async () => {
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services: services() }));

    let tabId = "";
    await act(async () => {
      const opened = await result.current.openTab(worktree);
      if (!opened) throw new Error("expected openTab to return a tab id");
      tabId = opened;
    });

    await waitFor(() => expect(nativeListeners.title.size).toBeGreaterThan(0));

    act(() => {
      emitNativeTitle({ sessionId: "backend-1", title: "\u280b codex: running tests" });
    });

    expect(result.current.tabActivity[tabId]).toMatchObject({ hasWorking: true });
    expect(result.current.activityNotificationTargets).toHaveLength(1);
    expect(result.current.activityNotificationTargets[0]).toMatchObject({
      tabId,
      state: "working",
      agentLabel: "Codex",
    });
  });

  it("ignores a native title event for a backend session no local session owns", async () => {
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services: services() }));

    await act(async () => {
      await result.current.openTab(worktree);
    });
    await waitFor(() => expect(nativeListeners.title.size).toBeGreaterThan(0));

    act(() => {
      emitNativeTitle({ sessionId: "backend-does-not-exist", title: "\u280b omo: working" });
    });

    expect(result.current.activityNotificationTargets).toHaveLength(0);
  });

  it("delivers a native bell to subscribers with the LOCAL session id and its tab id", async () => {
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services: services() }));

    let tabId = "";
    await act(async () => {
      const opened = await result.current.openTab(worktree);
      if (!opened) throw new Error("expected openTab to return a tab id");
      tabId = opened;
    });

    const openedTab = result.current.state.layout.tabs.find((candidate) => candidate.id === tabId);
    const localSessionId = openedTab && openedTab.kind !== "browser" ? openedTab.sessionId : "";
    expect(localSessionId).not.toBe("");

    await waitFor(() => expect(nativeListeners.bell.size).toBeGreaterThan(0));

    const received: Array<[string, string]> = [];
    let unsubscribe = () => undefined as void;
    act(() => {
      unsubscribe = result.current.subscribeTerminalBell((sessionId, bellTabId) => {
        received.push([sessionId, bellTabId]);
      });
    });

    act(() => {
      emitNativeBell({ sessionId: "backend-1", count: 1 });
    });

    expect(received).toEqual([[localSessionId, tabId]]);
    expect(localSessionId).not.toBe("backend-1");

    act(() => {
      unsubscribe();
      emitNativeBell({ sessionId: "backend-1", count: 2 });
    });

    expect(received).toHaveLength(1);
  });
});
