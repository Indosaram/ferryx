import { describe, expect, it } from "vitest";

import type { TerminalSession, Worktree } from "../lib/types";
import { planOrphanAdoption } from "./orphanAdoption";
import type { WorkspaceState } from "./workspaceStore";

const worktree = (path: string): Worktree => ({ path, head: "abc", branch: null, bare: false, detached: false, locked: null, prunable: null });

function state(sessions: Record<string, Partial<TerminalSession>>): WorkspaceState {
  return {
    worktrees: [worktree("/repo"), worktree("/repo/feature")],
    activeWorktreePath: "/repo",
    sessions,
  } as unknown as WorkspaceState;
}

describe("planOrphanAdoption", () => {
  it("adopts live sessions into their own worktree with the daemon attach identity", () => {
    const plan = planOrphanAdoption(["b1"], state({}), [
      { sessionId: "b1", worktreePath: "/repo/feature", daemonEpoch: "e1", incarnation: "i1", running: true },
    ]);
    expect(plan.adopt).toEqual([
      { worktree: worktree("/repo/feature"), backend: { sessionId: "b1", daemonEpoch: "e1", incarnation: "i1" } },
    ]);
  });

  it("passes the source window's attach generation through as a floor", () => {
    const plan = planOrphanAdoption(["b1", "b2"], state({}), [
      { sessionId: "b1", worktreePath: "/repo", daemonEpoch: "e1", incarnation: "i1" },
      { sessionId: "b2", worktreePath: "/repo" },
    ], { b1: 4 });
    expect(plan.adopt.map((entry) => entry.backend)).toEqual([
      { sessionId: "b1", daemonEpoch: "e1", incarnation: "i1", attemptGeneration: 4 },
      { sessionId: "b2", daemonEpoch: null, incarnation: null },
    ]);
  });

  it("falls back to the active worktree for an unknown worktree path", () => {
    const plan = planOrphanAdoption(["b1"], state({}), [{ sessionId: "b1", worktreePath: "/elsewhere" }]);
    expect(plan.adopt[0].worktree.path).toBe("/repo");
  });

  it("skips sessions already attached here and sessions the daemon no longer runs", () => {
    const plan = planOrphanAdoption(["b1", "b2", "b3", "b1"], state({ s1: { id: "s1", backendSessionId: "b1" } }), [
      { sessionId: "b1", worktreePath: "/repo" },
      { sessionId: "b3", worktreePath: "/repo", running: false },
    ]);
    expect(plan.adopt).toEqual([]);
    expect(plan.alreadyAttached).toEqual(["b1"]);
    expect(plan.missing).toEqual(["b2", "b3"]);
  });
});
