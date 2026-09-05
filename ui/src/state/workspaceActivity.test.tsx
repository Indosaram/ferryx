import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import {
  selectActivityNotificationTargets,
  selectGlobalUnreadBadgeCount,
  selectTabActivitySummaries,
  selectWorktreeActivitySummaries,
  useWorkspaceStore,
  workspaceReducer,
  type WorkspaceServices,
  type WorkspaceState,
} from "./workspaceStore";
import { clearWorkspaceSnapshot, setWorkspaceSnapshot } from "./workspaceSnapshotCache";

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

function openedTab(tabId: string | null): string {
  if (tabId === null) throw new Error("expected openTab to return a tab id");
  return tabId;
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
      backgroundTabId = openedTab(await result.current.openTab(worktree));
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
      tabId = openedTab(await result.current.openTab(worktree));
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

  it("does not set isAgent for shell titles carrying activity words like make: running tests", async () => {
    const workspaceServices = services();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree], services: workspaceServices }),
    );

    let tabId = "";
    await act(async () => {
      tabId = openedTab(await result.current.openTab(worktree));
    });
    const tab = result.current.state.layout.tabs.find((t) => t.id === tabId);
    if (!tab || tab.kind === "browser") throw new Error("expected terminal tab");
    const sessionId = tab.sessionId;

    act(() => {
      result.current.updateSessionTitleActivity(tabId, "make: running tests");
    });

    expect(result.current.state.activityBySessionId?.[sessionId]).toMatchObject({
      isAgent: false,
      state: "working",
    });
  });

  it("does not mark a tab unread when it is the activeTabId of a secondary visible tab group", () => {
    const state: WorkspaceState = {
      worktrees: [worktree],
      activeWorktreePath: worktree.path,
      sessions: {
        "session-a": {
          id: "session-a",
          cwd: worktree.path,
          workspaceId: "default",
          worktree: { wsId: "ws-main", slug: "main" },
          backendSessionId: "backend-a",
          lifecycle: "working",
        },
        "session-b": {
          id: "session-b",
          cwd: worktree.path,
          workspaceId: "default",
          worktree: { wsId: "ws-main", slug: "main" },
          backendSessionId: "backend-b",
          lifecycle: "working",
        },
        "session-c": {
          id: "session-c",
          cwd: worktree.path,
          workspaceId: "default",
          worktree: { wsId: "ws-main", slug: "main" },
          backendSessionId: "backend-c",
          lifecycle: "working",
        },
      },
      layout: {
        tabs: [
          { id: "tab-a", label: "tab-a", sessionId: "session-a" },
          { id: "tab-b", label: "tab-b", sessionId: "session-b" },
          { id: "tab-c", label: "tab-c", sessionId: "session-c" },
        ],
        activeTabId: "tab-a",
        focusedGroupId: "group-a",
        tabGroups: {
          "group-a": { id: "group-a", tabIds: ["tab-a"], activeTabId: "tab-a" },
          "group-b": { id: "group-b", tabIds: ["tab-b"], activeTabId: "tab-b" },
        },
        layoutsByTabId: {},
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {
        "session-b": { state: "working", title: "⠋ omo: build", isAgent: true, agentType: "omo" },
        "session-c": { state: "working", title: "⠋ omo: build", isAgent: true, agentType: "omo" },
      },
    };

    const nextStateB = workspaceReducer(state, {
      type: "SESSION_TITLE_ACTIVITY",
      tabId: "tab-b",
      sessionId: "session-b",
      title: "✳ omo: done",
    });
    expect(nextStateB.unreadTabIds["tab-b"]).toBeUndefined();

    const nextStateC = workspaceReducer(state, {
      type: "SESSION_TITLE_ACTIVITY",
      tabId: "tab-c",
      sessionId: "session-c",
      title: "✳ omo: done",
    });
    expect(nextStateC.unreadTabIds["tab-c"]).toBe(true);
  });

  it("resolves notification targets with worktreeLabel, agentLabel, and skips unowned sessions", () => {
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
        "session-unowned": {
          id: "session-unowned",
          cwd: featureWorktree.path,
          workspaceId: "default",
          worktree: { wsId: "ws-main", slug: "feature" },
          backendSessionId: "backend-unowned",
          lifecycle: "working",
        },
      },
      layout: {
        tabs: [{ id: "tab-a", label: "feature", sessionId: "session-a" }],
        activeTabId: "tab-a",
        layoutsByTabId: {},
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {
        "session-a": { state: "working", title: "⠋ omo: build", isAgent: true, agentType: "omo" },
        "session-unowned": { state: "waiting", title: "✋ codex: permission", isAgent: true, agentType: "codex" },
      },
    };

    const targets = selectActivityNotificationTargets(state);
    expect(targets).toEqual([
      {
        sessionId: "session-a",
        tabId: "tab-a",
        worktreePath: featureWorktree.path,
        worktreeLabel: "feature",
        agentLabel: "OMO",
        terminalTitle: "⠋ omo: build",
        state: "working",
      },
    ]);
  });

  it("labels a notification target from agentType when the extension reported state and the title carries no agent name", () => {
    const state: WorkspaceState = {
      workspaceId: "default",
      worktrees: [featureWorktree],
      activeWorktreePath: featureWorktree.path,
      sessions: {
        "session-a": {
          id: "session-a",
          cwd: featureWorktree.path,
          workspaceId: "default",
          worktree: { wsId: "default", slug: "feature" },
          backendSessionId: "backend-a",
          lifecycle: "working",
        },
      },
      layout: {
        tabs: [{ id: "tab-a", label: "feature", sessionId: "session-a" }],
        activeTabId: "tab-a",
        layoutsByTabId: {},
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {
        // The extension reports state directly; omo's own title is just a bare name, so the
        // notification label can only come from agentType.
        "session-a": { state: "done", title: "", isAgent: true, agentType: "omo", source: "screen" },
      },
    } as unknown as WorkspaceState;

    const targets = selectActivityNotificationTargets(state);
    expect(targets).toHaveLength(1);
    expect(targets[0]?.agentLabel).toBe("OMO");
  });

  it("memoizes selector outputs when renderedState does not change", () => {
    const workspaceServices = services();
    const { result, rerender } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree], services: workspaceServices }),
    );

    const initialAgents = result.current.agents;
    const initialTabActivity = result.current.tabActivity;
    const initialWorktreeActivity = result.current.worktreeActivity;
    const initialTargets = result.current.activityNotificationTargets;

    rerender();

    expect(result.current.agents).toBe(initialAgents);
    expect(result.current.tabActivity).toBe(initialTabActivity);
    expect(result.current.worktreeActivity).toBe(initialWorktreeActivity);
    expect(result.current.activityNotificationTargets).toBe(initialTargets);
  });

  it("retains unseen activity on a completion in an unfocused split pane of an active tab so the pane gets attention highlight", () => {
    const initialState: WorkspaceState = {
      workspaceId: "default",
      worktrees: [worktree],
      activeWorktreePath: worktree.path,
      sessions: {
        "session-active": {
          id: "session-active",
          cwd: worktree.path,
          workspaceId: "default",
          backendSessionId: "backend-active",
          lifecycle: "running",
        },
        "session-background": {
          id: "session-background",
          cwd: worktree.path,
          workspaceId: "default",
          backendSessionId: "backend-background",
          lifecycle: "running",
        },
      },
      layout: {
        tabs: [{ id: "tab-1", label: "main", sessionId: "session-active" }],
        activeTabId: "tab-1",
        layoutsByTabId: {
          "tab-1": {
            root: {
              type: "split",
              id: "split-root",
              direction: "horizontal",
              ratio: 0.5,
              first: { type: "leaf", id: "leaf-active", leafId: "leaf-active" },
              second: { type: "leaf", id: "leaf-bg", leafId: "leaf-bg" },
            },
            activeLeafId: "leaf-active",
            sessionIdsByLeafId: {
              "leaf-active": "session-active",
              "leaf-bg": "session-background",
            },
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {
        "session-background": {
          state: "working",
          title: "agent",
          isAgent: true,
          agentType: "claude",
          seen: false,
        },
      },
    } as unknown as WorkspaceState;

    const nextState = workspaceReducer(initialState, {
      type: "SESSION_SCREEN_ACTIVITY",
      sessionId: "session-background",
      tabId: "tab-1",
      state: "idle",
      ruleId: "idle-rule",
    });

    const bgActivity = nextState.activityBySessionId?.["session-background"];
    expect(bgActivity?.state).toBe("done");
    expect(bgActivity?.seen).toBe(false);
  });

  it("marks background tab and worktree unread when an agent enters waiting state", () => {
    const initialState: WorkspaceState = {
      workspaceId: "default",
      worktrees: [worktree],
      activeWorktreePath: worktree.path,
      sessions: {
        "session-active": {
          id: "session-active",
          cwd: worktree.path,
          workspaceId: "default",
          backendSessionId: "backend-active",
          lifecycle: "running",
        },
        "session-bg-tab": {
          id: "session-bg-tab",
          cwd: worktree.path,
          workspaceId: "default",
          backendSessionId: "backend-bg-tab",
          lifecycle: "running",
        },
      },
      layout: {
        tabs: [
          { id: "tab-1", label: "main", sessionId: "session-active" },
          { id: "tab-2", label: "bg-tab", sessionId: "session-bg-tab" },
        ],
        activeTabId: "tab-1",
        layoutsByTabId: {},
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {
        "session-bg-tab": {
          state: "working",
          title: "agent",
          isAgent: true,
          agentType: "claude",
        },
      },
    } as unknown as WorkspaceState;

    const nextState = workspaceReducer(initialState, {
      type: "SESSION_SCREEN_ACTIVITY",
      sessionId: "session-bg-tab",
      tabId: "tab-2",
      state: "blocked",
      ruleId: "blocked-rule",
    });

    const bgActivity = nextState.activityBySessionId?.["session-bg-tab"];
    expect(bgActivity?.state).toBe("waiting");
    expect(nextState.unreadTabIds["tab-2"]).toBe(true);
    expect(nextState.unreadWorktreePaths[worktree.path]).toBe(true);
  });

  it("aggregates unread tab count across current and parked workspace snapshots for badge", () => {
    clearWorkspaceSnapshot();
    const currentState: WorkspaceState = {
      workspaceId: "ws-1",
      worktrees: [worktree],
      activeWorktreePath: worktree.path,
      sessions: {},
      layout: { tabs: [], activeTabId: "", layoutsByTabId: {} },
      unreadTabIds: { "tab-1": true, "tab-2": true },
      unreadWorktreePaths: {},
    } as unknown as WorkspaceState;

    const parkedState: WorkspaceState = {
      workspaceId: "ws-2",
      worktrees: [featureWorktree],
      activeWorktreePath: featureWorktree.path,
      sessions: {},
      layout: { tabs: [], activeTabId: "", layoutsByTabId: {} },
      unreadTabIds: { "tab-parked-1": true },
      unreadWorktreePaths: {},
    } as unknown as WorkspaceState;

    setWorkspaceSnapshot("ws-2", parkedState);

    const totalBadgeCount = selectGlobalUnreadBadgeCount(currentState, "ws-1");
    expect(totalBadgeCount).toBe(3);

    clearWorkspaceSnapshot();
  });
});