import { describe, expect, it } from "vitest";
import { createLayoutState } from "./layout";
import type { WorkspaceState } from "./workspaceStore";
import { emptySidebarWorkspaceIds } from "./sidebarWorkspaceState";

const projects = [{ workspaceId: "alpha" }, { workspaceId: "beta" }];

function workspace(workspaceId: string, hasTabs = false): WorkspaceState {
  return {
    workspaceId,
    worktrees: [],
    activeWorktreePath: "/repo",
    sessions: {},
    layout: createLayoutState(hasTabs ? [{
      id: "browser",
      kind: "browser",
      label: "Browser",
      url: "about:blank",
      browserId: "browser",
    }] : []),
    unreadTabIds: {},
    unreadWorktreePaths: {},
  };
}

describe("emptySidebarWorkspaceIds", () => {
  it("uses live tab closure instead of a stale saved layout", () => {
    const live = workspace("alpha");
    const snapshots = [["alpha", workspace("alpha", true)]] as const;

    expect(emptySidebarWorkspaceIds(projects, "alpha", live, snapshots)).toEqual(["alpha", "beta"]);
  });

  it("counts browser tabs and parked worktree tabs without terminal sessions", () => {
    const parked = workspace("beta");
    parked.worktreeLayouts = { "/feature": workspace("beta", true).layout };

    expect(emptySidebarWorkspaceIds(projects, "alpha", workspace("alpha", true), [["beta", parked]])).toEqual([]);
  });

  it("attributes outgoing state to its owner while switching projects", () => {
    expect(emptySidebarWorkspaceIds(projects, "beta", workspace("alpha", true), [])).toEqual(["beta"]);
  });
});
