import { describe, expect, it } from "vitest";
import { deriveFocusedTerminal } from "../App";
import type { WorkspaceState } from "../state/workspaceStore";

function workspace(id: string, state: "working" | "waiting" | "done", seen = false): WorkspaceState {
  return {
    workspaceId: id, activeWorktreePath: `/${id}`, unreadTabIds: {}, unreadWorktreePaths: {},
    worktrees: [{ path: `/${id}`, head: "abc", branch: "main", bare: false, detached: false, locked: null, prunable: null }],
    sessions: { [id]: { id, cwd: `/${id}`, workspaceId: id, backendSessionId: `backend-${id}`, worktree: null, lifecycle: "working" } },
    layout: { tabs: [{ id, label: id, sessionId: id }], activeTabId: id, layoutsByTabId: {} },
    activityBySessionId: { [id]: { state, seen, title: "", isAgent: true } },
  };
}

describe("remote attention publication", () => {
  it("publishes parked project attention without changing the selected terminal", () => {
    const selected = workspace("selected", "working");
    const parked = workspace("parked", "done");
    const payload = deriveFocusedTerminal("selected", selected, [["parked", parked]]);
    expect(payload?.backendSessionId).toBe("backend-selected");
    expect(payload?.attentionInventory).toEqual([
      { workspaceId: "selected", worktreeSlug: null, worktreeLabel: "main", state: "working" },
      { workspaceId: "parked", worktreeSlug: null, worktreeLabel: "main", state: "done" },
    ]);
  });

  it("does not publish acknowledged waiting as remote attention", () => {
    const selected = workspace("selected", "waiting", true);
    const payload = deriveFocusedTerminal("selected", selected, []);
    expect(payload?.terminalTabs?.[0].activityState).toBeUndefined();
    expect(payload?.attentionInventory?.[0].state).toBeNull();
  });
});
