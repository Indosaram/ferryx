import { describe, expect, it } from "vitest";

import type { Worktree } from "../lib/types";
import { workspaceReducer, type WorkspaceState } from "./workspaceStore";

const alpha: Worktree = {
  path: "/repo/alpha",
  head: "aaa",
  branch: "refs/heads/ferryx/ws/alpha",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const beta: Worktree = {
  path: "/repo/beta",
  head: "bbb",
  branch: "refs/heads/ferryx/ws/beta",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function stateWithVisibleTab(
  worktreePath: string,
  options: { activeWorktreePath: string | null; backendSessionId?: string },
): WorkspaceState {
  return {
    worktrees: [alpha, beta],
    activeWorktreePath: options.activeWorktreePath,
    sessions: {
      "session-live": {
        id: "session-live",
        cwd: worktreePath,
        worktreePath,
        workspaceId: "default",
        worktree: { wsId: "ws", slug: "live" },
        backendSessionId: options.backendSessionId ?? "backend-live",
        lifecycle: "running",
      },
    },
    layout: {
      tabs: [{ id: "tab-live", label: "live", sessionId: "session-live" }],
      activeTabId: "tab-live",
      layoutsByTabId: {
        "tab-live": {
          root: { type: "leaf", leafId: "leaf-live" },
          activeLeafId: "leaf-live",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-live": "session-live" },
        },
      },
    },
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  } as unknown as WorkspaceState;
}

const tabIds = (state: WorkspaceState) => state.layout.tabs.map((tab) => tab.id);
const parkedTabIds = (state: WorkspaceState, path: string) =>
  state.worktreeLayouts?.[path]?.tabs.map((tab) => tab.id);

describe("clicking a worktree never destroys the panes on screen", () => {
  it("keeps the visible tabs when the clicked worktree owns them but the active row is gone", () => {
    const state = stateWithVisibleTab(alpha.path, { activeWorktreePath: null });

    const next = workspaceReducer(state, { type: "SELECT_WORKTREE", path: alpha.path });

    expect(tabIds(next)).toEqual(["tab-live"]);
    expect(next.activeWorktreePath).toBe(alpha.path);
    expect(next.sessions["session-live"]).toBe(state.sessions["session-live"]);
  });

  it("parks the visible tabs under their own worktree so switching back restores them", () => {
    const state = stateWithVisibleTab(alpha.path, { activeWorktreePath: null });

    const switched = workspaceReducer(state, { type: "SELECT_WORKTREE", path: beta.path });
    expect(tabIds(switched)).toEqual([]);
    expect(switched.activeWorktreePath).toBe(beta.path);
    expect(parkedTabIds(switched, alpha.path)).toEqual(["tab-live"]);

    const back = workspaceReducer(switched, { type: "SELECT_WORKTREE", path: alpha.path });
    expect(tabIds(back)).toEqual(["tab-live"]);
    expect(back.activeWorktreePath).toBe(alpha.path);
  });

  it("never parks the outgoing layout under the worktree it is switching to", () => {
    const state = stateWithVisibleTab(beta.path, { activeWorktreePath: null });

    const next = workspaceReducer(state, { type: "SELECT_WORKTREE", path: beta.path });

    expect(tabIds(next)).toEqual(["tab-live"]);
    expect(next.worktreeLayouts?.[beta.path]).toBeUndefined();
  });
});

describe("a worktree refresh never destroys a session the daemon still runs", () => {
  it("keeps the session and its tab when the listing no longer names its worktree", () => {
    const state = stateWithVisibleTab(alpha.path, { activeWorktreePath: alpha.path });

    const next = workspaceReducer(state, {
      type: "SET_WORKTREES",
      worktrees: [beta],
      retainedSessionIds: ["backend-live"],
    });

    expect(next.sessions["session-live"]).toBe(state.sessions["session-live"]);
    expect(tabIds(next)).toEqual(["tab-live"]);
    expect(next.activeWorktreePath).toBe(alpha.path);
  });

  it("prunes a session the daemon no longer runs", () => {
    const state = stateWithVisibleTab(alpha.path, { activeWorktreePath: alpha.path });

    const next = workspaceReducer(state, {
      type: "SET_WORKTREES",
      worktrees: [beta],
      retainedSessionIds: [],
    });

    expect(next.sessions["session-live"]).toBeUndefined();
    expect(tabIds(next)).toEqual([]);
    expect(next.activeWorktreePath).toBe(beta.path);
  });

  it("keeps a parked layout whose tab is still running", () => {
    const state = stateWithVisibleTab(alpha.path, { activeWorktreePath: null });
    const parked: WorkspaceState = {
      ...state,
      layout: {
        tabs: [],
        activeTabId: null,
        layoutsByTabId: {},
      } as unknown as WorkspaceState["layout"],
      worktreeLayouts: { [alpha.path]: state.layout },
    };

    const next = workspaceReducer(parked, {
      type: "SET_WORKTREES",
      worktrees: [beta],
      retainedSessionIds: ["backend-live"],
    });

    expect(parkedTabIds(next, alpha.path)).toEqual(["tab-live"]);
    expect(next.sessions["session-live"]).toBeDefined();
  });
});
