import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { localRecoveryBlockReason, useWorkspaceStore, workspaceReducer, type WorkspaceServices, type WorkspaceState } from "./workspaceStore";

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

describe("worktree sync edge cases", () => {
  it("keeps parked layouts and unread marks keyed by a raw Windows path", () => {
    const win: Worktree = { ...alpha, path: "C:\\Users\\dev\\Repo" };
    const state = stateWithVisibleTab(win.path, { activeWorktreePath: null });
    const parked = {
      ...state,
      layout: { tabs: [], activeTabId: null, layoutsByTabId: {} } as unknown as WorkspaceState["layout"],
      worktreeLayouts: { [win.path]: state.layout },
      unreadWorktreePaths: { [win.path]: true },
      activeWorktreePath: beta.path,
    } as unknown as WorkspaceState;

    const next = workspaceReducer(parked, { type: "SET_WORKTREES", worktrees: [win, beta], retainedSessionIds: [] });

    expect(parkedTabIds(next, win.path)).toEqual(["tab-live"]);
    expect(next.unreadWorktreePaths[win.path]).toBe(true);
  });

  it("skips pruning and closing when the live inventory cannot be read", async () => {
    const closeTerminal = vi.fn(async () => undefined);
    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined), spawnTerminal: vi.fn(async () => "unused"),
      getTerminalCwd: vi.fn(async () => null), closeTerminal, waitForTerminalExit: vi.fn(async () => undefined),
      listTerminalSessions: vi.fn(async () => { throw new Error("inventory down"); }),
    };
    const { result, unmount } = renderHook(() => useWorkspaceStore({ workspaceId: "default", initialWorktrees: [alpha, beta], services }));
    await act(async () => {
      result.current.restoreWorkspace({ ...stateWithVisibleTab(alpha.path, { activeWorktreePath: alpha.path }), workspaceId: "default" } as WorkspaceState);
    });
    await act(async () => { await result.current.syncWorktrees([beta]); });
    expect(closeTerminal).not.toHaveBeenCalled();
    expect(result.current.state.sessions["session-live"]).toBeDefined();
    expect(result.current.state.worktrees.map((w) => w.path)).toEqual([alpha.path, beta.path]);
    unmount();
  });
});

describe("localRecoveryBlockReason", () => {
  it("knows PAIRED_PROXY_BLOCKED and keeps unknown non-empty codes verbatim", () => {
    expect(localRecoveryBlockReason("PAIRED_PROXY_BLOCKED")).toBe("PAIRED_PROXY_BLOCKED");
    expect(localRecoveryBlockReason("SOMETHING_NEW")).toBe("SOMETHING_NEW");
    expect(localRecoveryBlockReason("")).toBe("identity_conflict");
    expect(localRecoveryBlockReason(null)).toBe("identity_conflict");
  });
});
