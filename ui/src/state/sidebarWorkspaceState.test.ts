import { describe, expect, it } from "vitest";
import { createLayoutState } from "./layout";
import type { WorkspaceState } from "./workspaceStore";
import { emptySidebarWorkspaceIds, worktreeHasOpenTabs } from "./sidebarWorkspaceState";

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

describe("worktreeHasOpenTabs", () => {
  it("treats the active worktree as owning tabs when the live layout has tabs", () => {
    const state = workspace("alpha", true);
    expect(worktreeHasOpenTabs(state, "/repo")).toBe(true);
  });

  it("excludes the active worktree when its live layout has no tabs", () => {
    const state = workspace("alpha", false);
    expect(worktreeHasOpenTabs(state, "/repo")).toBe(false);
  });

  it("identifies parked worktrees with open tabs", () => {
    const state = workspace("alpha", false);
    state.worktreeLayouts = { "/feature": workspace("alpha", true).layout };

    expect(worktreeHasOpenTabs(state, "/feature")).toBe(true);
    expect(worktreeHasOpenTabs(state, "/other")).toBe(false);
  });

  it("keeps rows navigable when live tabs exist without an attributable active worktree", () => {
    const state = { ...workspace("alpha", true), activeWorktreePath: null };
    expect(worktreeHasOpenTabs(state, "/any-path")).toBe(true);
  });

  it("returns false for missing or unbacked workspace states", () => {
    expect(worktreeHasOpenTabs(undefined, "/repo")).toBe(false);
  });
});
