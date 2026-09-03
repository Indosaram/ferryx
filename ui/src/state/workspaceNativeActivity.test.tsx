import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { NativeTerminalAgentStatePayload, NativeTerminalBellPayload, NativeTerminalTitlePayload, Worktree } from "../lib/types";

/**
 * Guards the defect a green suite missed: activity was fed by the legacy `terminal_output` OSC
 * scanner, which native-ghostty sessions never stream. Only the native events reach background
 * tabs, whose panes `TerminalSplitView` unmounts.
 */
const nativeListeners = vi.hoisted(() => ({
  title: new Set<(payload: NativeTerminalTitlePayload) => void>(),
  bell: new Set<(payload: NativeTerminalBellPayload) => void>(),
  agentState: new Set<(payload: NativeTerminalAgentStatePayload) => void>(),
  focus: new Set<(sessionId: string) => void>(),
}));

vi.mock("../lib/tauri", () => ({
  DEFAULT_WORKSPACE_ID: "default",
  spawnTerminal: vi.fn(async () => "backend-1"),
  closeTerminal: vi.fn(async () => undefined),
  getTerminalCwd: vi.fn(async () => "/repo/main"),
  waitForTerminalExit: vi.fn(async () => undefined),
  discoverAgentProviderSession: vi.fn(async () => null),
  onNativeTerminalTitle: vi.fn(async (handler: (payload: NativeTerminalTitlePayload) => void) => {
    nativeListeners.title.add(handler);
    return () => nativeListeners.title.delete(handler);
  }),
  onNativeTerminalBell: vi.fn(async (handler: (payload: NativeTerminalBellPayload) => void) => {
    nativeListeners.bell.add(handler);
    return () => nativeListeners.bell.delete(handler);
  }),
  onNativeTerminalAgentState: vi.fn(async (handler: (payload: NativeTerminalAgentStatePayload) => void) => {
    nativeListeners.agentState.add(handler);
    return () => nativeListeners.agentState.delete(handler);
  }),
  onNativeTerminalFocus: vi.fn(async (handler: (sessionId: string) => void) => {
    nativeListeners.focus.add(handler);
    return () => nativeListeners.focus.delete(handler);
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
  let backendCounter = 1;
  return {
    ensureTerminalEvents: vi.fn(async () => undefined),
    spawnTerminal: vi.fn(async () => `backend-${backendCounter++}`),
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

function emitNativeAgentState(payload: NativeTerminalAgentStatePayload): void {
  for (const listener of nativeListeners.agentState) listener(payload);
}

function emitNativeFocus(sessionId: string): void {
  for (const listener of nativeListeners.focus) listener(sessionId);
}

describe("workspace store native activity subscription", () => {
  beforeEach(() => {
    nativeListeners.title.clear();
    nativeListeners.bell.clear();
    nativeListeners.agentState.clear();
    nativeListeners.focus.clear();
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

  it("marks a finished session's activity as seen when native focus event is emitted for its backend session", async () => {
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services: services() }));

    let tab1Id = "";
    await act(async () => {
      const opened = await result.current.openTab(worktree);
      if (!opened) throw new Error("expected openTab to return a tab id");
      tab1Id = opened;
    });

    const tab1 = result.current.state.layout.tabs.find((candidate) => candidate.id === tab1Id);
    const session1Id = tab1 && tab1.kind !== "browser" ? tab1.sessionId : "";
    expect(session1Id).not.toBe("");

    // Open a second tab so tab1 is inactive (backgrounded)
    await act(async () => {
      await result.current.openTab(worktree);
    });

    await waitFor(() => expect(nativeListeners.focus.size).toBeGreaterThan(0));

    // Agent finishes in tab1 while backgrounded -> unseen done activity
    act(() => {
      emitNativeAgentState({ sessionId: "backend-1", state: "working", ruleId: "r1", manifestId: "omo" });
      emitNativeAgentState({ sessionId: "backend-1", state: "idle", ruleId: "r2", manifestId: "omo" });
    });

    expect(result.current.state.activityBySessionId?.[session1Id]?.state).toBe("done");
    expect(result.current.state.activityBySessionId?.[session1Id]?.seen).toBeFalsy();

    // Clicking / focusing the pane emits native focus with the backend session ID
    act(() => {
      emitNativeFocus("backend-1");
    });

    expect(result.current.state.activityBySessionId?.[session1Id]?.seen).toBe(true);
  });
});
