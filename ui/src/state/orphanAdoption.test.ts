import { describe, expect, it } from "vitest";

import type { TerminalSession, Worktree } from "../lib/types";
import { planOrphanAdoption, planWorktreeSessionAdoption } from "./orphanAdoption";
import type { WorkspaceState } from "./workspaceStore";

const worktree = (path: string): Worktree => ({ path, head: "abc", branch: null, bare: false, detached: false, locked: null, prunable: null });

function state(sessions: Record<string, Partial<TerminalSession>>, worktreePaths: string[] = ["/repo", "/repo/feature"]): WorkspaceState {
  return {
    worktrees: worktreePaths.map(worktree),
    activeWorktreePath: worktreePaths[0] ?? null,
    sessions,
  } as unknown as WorkspaceState;
}

/** A workspace whose active layout shows one terminal tab for `sessionId`. */
function stateShowingTab(sessions: Record<string, Partial<TerminalSession>>, sessionId: string): WorkspaceState {
  return {
    ...state(sessions),
    layout: {
      tabs: [{ id: `tab-${sessionId}`, kind: "terminal", sessionId, label: "shell" }],
      activeTabId: `tab-${sessionId}`,
      layoutsByTabId: {},
    },
  } as unknown as WorkspaceState;
}

describe("planOrphanAdoption", () => {
  it("matches Windows orphan paths across case, slash and trailing separator changes", () => {
    const snapshot = state({});
    const feature = worktree("C:\\Repo\\.ferryx-worktrees\\ws\\wt");
    snapshot.worktrees.push(feature);
    const plan = planOrphanAdoption(["b1"], snapshot, [
      { sessionId: "b1", worktreePath: "c:/repo/.ferryx-worktrees/ws/wt/", running: true },
    ]);
    expect(plan.adopt[0].worktree).toBe(feature);
  });

  it("normalizes the active path when an orphan needs a fallback", () => {
    const snapshot = state({});
    const active = worktree("C:/Repo/feature");
    snapshot.worktrees.push(active);
    snapshot.activeWorktreePath = "c:\\repo\\FEATURE\\";
    const plan = planOrphanAdoption(["b1"], snapshot, [{ sessionId: "b1", worktreePath: "/unknown" }]);
    expect(plan.adopt[0].worktree).toBe(active);
  });

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
    const plan = planOrphanAdoption(["b1", "b2", "b3", "b1"], stateShowingTab({ s1: { id: "s1", backendSessionId: "b1" } }, "s1"), [
      { sessionId: "b1", worktreePath: "/repo" },
      { sessionId: "b3", worktreePath: "/repo", running: false },
    ]);
    expect(plan.adopt).toEqual([]);
    expect(plan.alreadyAttached).toEqual(["b1"]);
    expect(plan.missing).toEqual(["b2", "b3"]);
  });

  // Parking or closing a layout drops the tab but keeps the session entry (and its PTY). Reading
  // that entry as "already on screen" made the click that recovers it return silently.
  it("adopts a session whose entry survives but no tab shows any more", () => {
    const plan = planOrphanAdoption(["b1"], state({ s1: { id: "s1", backendSessionId: "b1" } }), [
      { sessionId: "b1", worktreePath: "/repo", running: true },
    ]);
    expect(plan.alreadyAttached).toEqual([]);
    expect(plan.adopt.map((entry) => entry.backend.sessionId)).toEqual(["b1"]);
  });
});

describe("planWorktreeSessionAdoption", () => {
  it("re-attaches a running session of the clicked worktree instead of spawning a second shell", () => {
    const entry = planWorktreeSessionAdoption("/repo/feature", state({}), [
      { sessionId: "b1", worktreePath: "/repo", daemonEpoch: "e1", incarnation: "i1", running: true },
      { sessionId: "b2", worktreePath: "/repo/feature", daemonEpoch: "e2", incarnation: "i2", running: true },
    ]);
    expect(entry).toEqual({
      worktree: worktree("/repo/feature"),
      backend: { sessionId: "b2", daemonEpoch: "e2", incarnation: "i2" },
    });
  });

  it("ignores sessions of another worktree, sessions a pane already shows, and exited sessions", () => {
    expect(planWorktreeSessionAdoption("/repo/feature", state({}), [
      { sessionId: "b1", worktreePath: "/repo", running: true },
    ])).toBeNull();
    expect(planWorktreeSessionAdoption("/repo/feature", stateShowingTab({ s1: { id: "s1", backendSessionId: "b2" } }, "s1"), [
      { sessionId: "b2", worktreePath: "/repo/feature", running: true },
    ])).toBeNull();
    expect(planWorktreeSessionAdoption("/repo/feature", state({}), [
      { sessionId: "b3", worktreePath: "/repo/feature", running: false },
    ])).toBeNull();
  });

  it("re-attaches a session whose entry survived a layout change with no tab showing it", () => {
    const entry = planWorktreeSessionAdoption("/repo/feature", state({ s1: { id: "s1", backendSessionId: "b2" } }), [
      { sessionId: "b2", worktreePath: "/repo/feature", running: true },
    ]);
    expect(entry?.backend.sessionId).toBe("b2");
  });

  it("matches a Windows worktree path regardless of separator, case, or trailing slash", () => {
    const entry = planWorktreeSessionAdoption("C:\\Repo\\Feature\\", state({}, ["C:\\Repo", "C:\\Repo\\Feature"]), [
      { sessionId: "b1", worktreePath: "c:/repo/feature", running: true },
    ]);
    expect(entry?.backend.sessionId).toBe("b1");
    expect(entry?.worktree.path).toBe("C:\\Repo\\Feature");
  });

  it("returns null for a worktree this workspace does not know", () => {
    expect(planWorktreeSessionAdoption("/repo/unknown", state({}), [
      { sessionId: "b1", worktreePath: "/repo/unknown", running: true },
    ])).toBeNull();
  });
});
