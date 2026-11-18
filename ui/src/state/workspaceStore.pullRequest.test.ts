import { describe, expect, it } from "vitest";
import type { LanePullRequestView } from "../lib/sidebarPersistence";
import { createLayoutState } from "./layout";
import { workspaceReducer, type WorkspaceState } from "./workspaceStore";

const key = "w:repo/review";
const pr: LanePullRequestView = {
  laneKey: "review", repo: "owner/repo", prNumber: 42, state: "open",
  url: "https://github.com/owner/repo/pull/42", title: "Review lane",
  headBranch: "lane/review", observedAt: "2026-11-15T00:00:00Z",
};
function initial(): WorkspaceState {
  return { worktrees: [], activeWorktreePath: null, sessions: {}, layout: createLayoutState([]), unreadTabIds: {}, unreadWorktreePaths: {} };
}

describe("server-owned PR observations in workspace state", () => {
  it("keeps the read model's PR and removes it when a later read has none", () => {
    const loaded = workspaceReducer(initial(), {
      type: "SET_LANE_LIFECYCLE_MODEL", rows: { [key]: "active" }, pinnedTabs: {}, order: { repo: [key] }, pullRequests: { [key]: pr },
    });
    expect(loaded.lanePullRequests?.[key]).toEqual(pr);
    const cleared = workspaceReducer(loaded, {
      type: "SET_LANE_LIFECYCLE_MODEL", rows: { [key]: "active" }, pinnedTabs: {}, order: { repo: [key] }, pullRequests: { [key]: null },
    });
    expect(cleared.lanePullRequests?.[key]).toBeNull();
  });

  // Deleting this loses ordering coverage across read/frame/write folds, including unversioned reads.
  it("retains newer row fields and order while accepting equal-revision frames", () => {
    const merged = { ...pr, state: "merged" as const };
    const loaded = workspaceReducer(initial(), {
      type: "SET_LANE_LIFECYCLE_MODEL", rows: { [key]: "pinned" }, pinnedTabs: { [key]: ["tab"] },
      order: { repo: [key] }, pullRequests: { [key]: merged }, revisions: { [key]: 2 },
    });
    for (const revisions of [{ [key]: 1 }, {}]) {
      const stale = workspaceReducer(loaded, {
        type: "SET_LANE_LIFECYCLE_MODEL", rows: { [key]: "active" }, pinnedTabs: {},
        order: { repo: [] }, pullRequests: { [key]: pr }, revisions,
      });
      expect(stale.laneLifecycle).toEqual(loaded.laneLifecycle);
      expect(stale.lanePinnedTabs).toEqual(loaded.lanePinnedTabs);
      expect(stale.lanePullRequests).toEqual(loaded.lanePullRequests);
      expect(stale.laneRevisions).toEqual(loaded.laneRevisions);
      expect(stale.laneOrder).toEqual(loaded.laneOrder);
    }
    const read = { type: "SET_LANE_LIFECYCLE_MODEL" as const, rows: { [key]: "active" as const }, pinnedTabs: {},
      order: { repo: [key] }, pullRequests: { [key]: pr }, revisions: { [key]: 2 } };
    expect(workspaceReducer(loaded, { ...read, frameRevisions: { [key]: 2 } }).lanePullRequests?.[key]).toEqual(merged);
    expect(workspaceReducer(loaded, read).lanePullRequests?.[key]).toEqual(pr);
  });

  it("uses the PR observation returned with a lifecycle write, including absence", () => {
    const merged = { ...pr, state: "merged" as const };
    const committed = workspaceReducer(initial(), {
      type: "COMMIT_LANE_LIFECYCLE", worktreeKey: key, state: "settled", revision: 2, pullRequest: merged,
    });
    expect(committed.lanePullRequests?.[key]).toEqual(merged);
    const cleared = workspaceReducer(committed, {
      type: "COMMIT_LANE_LIFECYCLE", worktreeKey: key, state: "active", revision: 3, pullRequest: null,
    });
    expect(cleared.lanePullRequests?.[key]).toBeNull();
  });
});
