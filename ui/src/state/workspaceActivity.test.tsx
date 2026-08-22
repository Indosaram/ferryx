import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import {
  selectTabActivitySummaries,
  selectWorktreeActivitySummaries,
  useWorkspaceStore,
  type WorkspaceServices,
  type WorkspaceState,
} from "./workspaceStore";

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/orca/ws-main/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const featureWorktree: Worktree = {
  ...worktree,
  path: "/repo/feature",
  branch: "refs/heads/orca/ws-main/feature",
};

function services(): WorkspaceServices {
  let session = 0;
  return {
    ensureTerminalEvents: vi.fn(async () => undefined),
    spawnTerminal: vi.fn(async () => `backend-${++session}`),
    getTerminalCwd: vi.fn(async () => worktree.path),
    closeTerminal: vi.fn(async () => undefined),
    waitForTerminalExit: vi.fn(async () => undefined),
  };
}

describe("workspace activity tracking", () => {
  it("aggregates every child pane session into its tab and worktree summary without double-counting", () => {
    const state: WorkspaceState = {
      worktrees: [featureWorktree],
      activeWorktreePath: featureWorktree.path,
      sessions: {
        "session-a": {
          id: "session-a",
          cwd: featureWorktree.path,
          workspaceId: "default",
          worktree: { wsId: "ws-main", slug: "feature" },
          backendSessionId: "backend-a",
          lifecycle: "working",
        },
        "session-b": {
          id: "session-b",
          cwd: featureWorktree.path,
          workspaceId: "default",
          worktree: { wsId: "ws-main", slug: "feature" },
          backendSessionId: "backend-b",
          lifecycle: "working",
        },
      },
      layout: {
        tabs: [{ id: "tab-a", label: "feature", sessionId: "session-a" }],
        activeTabId: "tab-a",
        layoutsByTabId: {
          "tab-a": {
            root: {
              type: "split",
              direction: "horizontal",
              ratio: 0.5,
              first: { type: "leaf", leafId: "leaf-a" },
              second: { type: "leaf", leafId: "leaf-b" },
            },
            activeLeafId: "leaf-a",
            expandedLeafId: null,
            sessionIdsByLeafId: {
              "leaf-a": "session-a",
              "leaf-b": "session-b",
            },
          },
        },
      },
      unreadTabIds: { "tab-a": true },
      unreadWorktreePaths: { [featureWorktree.path]: true },
      activityBySessionId: {
        "session-a": { state: "working", title: "⠋ omo: build", isAgent: true, agentType: "omo" },
        "session-b": { state: "waiting", title: "✋ codex: permission", isAgent: true, agentType: "codex" },
      },
    };

    const tabSummary = selectTabActivitySummaries(state)["tab-a"];
    expect(tabSummary).toMatchObject({
      workingCount: 1,
      waitingCount: 1,
      runningCount: 1,
      hasWorking: true,
      hasWaiting: true,
      hasUnread: true,
    });

    const worktreeSummary = selectWorktreeActivitySummaries(state)[featureWorktree.path];
    expect(worktreeSummary).toMatchObject({
      workingCount: 1,
      waitingCount: 1,
      runningCount: 1,
      hasWorking: true,
      hasWaiting: true,
      hasUnread: true,
    });
  });

  it("turns a background working-to-done title transition into unread and clears it on activation", async () => {
    const workspaceServices = services();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree, featureWorktree], services: workspaceServices }),
    );

    let backgroundTabId = "";
    await act(async () => {
      backgroundTabId = await result.current.openTab(worktree);
      await result.current.openTab(featureWorktree);
    });

    act(() => {
      result.current.updateSessionTitleActivity(backgroundTabId, "⠋ omo: building status indicators");
    });
    expect(result.current.tabActivity[backgroundTabId]).toMatchObject({ hasWorking: true, hasUnread: false });

    act(() => {
      result.current.updateSessionTitleActivity(backgroundTabId, "✳ omo: done");
    });

    expect(result.current.state.unreadTabIds[backgroundTabId]).toBe(true);
    expect(result.current.state.unreadWorktreePaths[worktree.path]).toBe(true);
    expect(result.current.tabActivity[backgroundTabId]).toMatchObject({ hasDone: true, hasUnread: true });
    expect(result.current.worktreeActivity[worktree.path]).toMatchObject({ hasDone: true, hasUnread: true });

    act(() => {
      result.current.activateTab(backgroundTabId);
    });

    expect(result.current.state.unreadTabIds[backgroundTabId]).toBeUndefined();
    expect(result.current.state.unreadWorktreePaths[worktree.path]).toBeUndefined();
    expect(result.current.tabActivity[backgroundTabId].hasUnread).toBe(false);
  });

  it("does not treat ordinary shell titles as agent activity", async () => {
    const workspaceServices = services();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services: workspaceServices }));

    let tabId = "";
    await act(async () => {
      tabId = await result.current.openTab(worktree);
    });
    act(() => {
      result.current.updateSessionTitleActivity(tabId, "zsh /repo/main");
    });

    expect(result.current.tabActivity[tabId]).toMatchObject({
      workingCount: 0,
      waitingCount: 0,
      doneCount: 0,
      hasWorking: false,
      hasWaiting: false,
      hasDone: false,
    });
  });
});