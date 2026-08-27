import { describe, expect, it } from "vitest";

import { resolveActivityIndicator } from "../lib/activity";
import type { Worktree } from "../lib/types";
import {
  selectTabActivitySummaries,
  selectWorktreeActivitySummaries,
  selectWorktreeActivitySummariesAcrossWorkspaces,
  workspaceReducer,
  type WorkspaceState,
} from "./workspaceStore";
import { clearWorkspaceSnapshot, setWorkspaceSnapshot } from "./workspaceSnapshotCache";

const alpha: Worktree = {
  path: "/repo/alpha",
  head: "aaa",
  branch: "refs/heads/orca/ws/alpha",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const beta: Worktree = {
  path: "/repo/beta",
  head: "bbb",
  branch: "refs/heads/orca/ws/beta",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function stateWithAgentInAlpha(): WorkspaceState {
  return {
    worktrees: [alpha, beta],
    activeWorktreePath: alpha.path,
    sessions: {
      "session-alpha": {
        id: "session-alpha",
        cwd: alpha.path,
        worktreePath: alpha.path,
        workspaceId: "default",
        worktree: { wsId: "ws", slug: "alpha" },
        backendSessionId: "backend-alpha",
        lifecycle: "running",
      },
    },
    layout: {
      tabs: [{ id: "tab-alpha", label: "alpha", sessionId: "session-alpha" }],
      activeTabId: "tab-alpha",
      layoutsByTabId: {
        "tab-alpha": {
          root: { type: "leaf", leafId: "leaf-alpha" },
          activeLeafId: "leaf-alpha",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-alpha": "session-alpha" },
        },
      },
    },
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  } as unknown as WorkspaceState;
}

describe("worktree activity survives a worktree switch", () => {
  it("keeps the working spinner on the row of a worktree the user switched away from", () => {
    let state = stateWithAgentInAlpha();
    state = workspaceReducer(state, {
      type: "SESSION_SCREEN_ACTIVITY",
      tabId: "tab-alpha",
      sessionId: "session-alpha",
      state: "working",
      ruleId: "spinner_working",
      manifestId: "omo",
    });

    expect(resolveActivityIndicator(selectTabActivitySummaries(state)["tab-alpha"])).toBe("working");
    expect(resolveActivityIndicator(selectWorktreeActivitySummaries(state)[alpha.path])).toBe("working");

    state = workspaceReducer(state, { type: "SELECT_WORKTREE", path: beta.path });

    expect(resolveActivityIndicator(selectWorktreeActivitySummaries(state)[alpha.path])).toBe("working");
  });

  it("exposes a working agent from a project the user switched away from", () => {
    clearWorkspaceSnapshot();

    let parked = stateWithAgentInAlpha();
    parked = { ...parked, workspaceId: "project-a" } as WorkspaceState;
    parked = workspaceReducer(parked, {
      type: "SESSION_SCREEN_ACTIVITY",
      tabId: "tab-alpha",
      sessionId: "session-alpha",
      state: "working",
      ruleId: "spinner_working",
      manifestId: "omo",
    });
    setWorkspaceSnapshot("project-a", parked);

    const summaries = selectWorktreeActivitySummariesAcrossWorkspaces("project-b");
    expect(resolveActivityIndicator(summaries[alpha.path])).toBe("working");
  });

  it("lets a parked workspace advance so its row does not spin forever", () => {
    clearWorkspaceSnapshot();

    let parked = stateWithAgentInAlpha();
    parked = { ...parked, workspaceId: "project-a" } as WorkspaceState;
    parked = workspaceReducer(parked, {
      type: "SESSION_SCREEN_ACTIVITY",
      tabId: "tab-alpha",
      sessionId: "session-alpha",
      state: "working",
      ruleId: "spinner_working",
      manifestId: "omo",
    });
    setWorkspaceSnapshot("project-a", parked);
    expect(
      resolveActivityIndicator(selectWorktreeActivitySummariesAcrossWorkspaces("project-b")[alpha.path]),
    ).toBe("working");

    const finished = workspaceReducer(parked, {
      type: "SESSION_SCREEN_ACTIVITY",
      tabId: "tab-alpha",
      sessionId: "session-alpha",
      state: "idle",
      ruleId: "prompt_idle",
      manifestId: "omo",
    });
    setWorkspaceSnapshot("project-a", finished);

    // tab-alpha is that workspace's active tab, so the completion is acknowledged on arrival and
    // the row goes quiet rather than spinning forever.
    expect(
      resolveActivityIndicator(selectWorktreeActivitySummariesAcrossWorkspaces("project-b")[alpha.path]),
    ).toBeNull();
  });

  it("prefers the live state over a stale snapshot of the same workspace", () => {
    clearWorkspaceSnapshot();

    let stale = stateWithAgentInAlpha();
    stale = { ...stale, workspaceId: "project-a" } as WorkspaceState;
    stale = workspaceReducer(stale, {
      type: "SESSION_SCREEN_ACTIVITY",
      tabId: "tab-alpha",
      sessionId: "session-alpha",
      state: "working",
      ruleId: "spinner_working",
      manifestId: "omo",
    });
    setWorkspaceSnapshot("project-a", stale);

    const summaries = selectWorktreeActivitySummariesAcrossWorkspaces("project-a");
    expect(summaries[alpha.path]).toBeUndefined();
  });
});
