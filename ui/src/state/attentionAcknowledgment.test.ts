import { describe, expect, it } from "vitest";
import { summarizeActivities } from "../lib/activity";
import { createLayoutState } from "./layout";
import { workspaceReducer, type WorkspaceState } from "./workspaceStore";

function fixture(): WorkspaceState {
  let state: WorkspaceState = {
    workspaceId: "attention-test",
    worktrees: [{ path: "/repo", head: "abc", branch: "main", bare: false, detached: false, locked: null, prunable: null }],
    activeWorktreePath: "/repo",
    sessions: {}, layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {},
  };
  for (const id of ["a", "b"]) {
    state = workspaceReducer(state, { type: "ADD_TAB_WITH_SESSION",
      tab: { id, label: id, sessionId: id },
      session: { id, cwd: "/repo", workspaceId: "attention-test", backendSessionId: `backend-${id}`, worktree: null, lifecycle: "working" },
    });
  }
  return state;
}

describe("attention acknowledgment episodes", () => {
  it("excludes acknowledged waiting from summaries", () => {
    expect(summarizeActivities([{ state: "waiting", title: "", isAgent: true, seen: true }]).waitingCount).toBe(0);
  });

  it("preserves acknowledgment on title refresh but resets it after new work", () => {
    let state = fixture();
    const activity = (value: "working" | "idle") => {
      state = workspaceReducer(state, { type: "SESSION_SCREEN_ACTIVITY", tabId: "a", sessionId: "a", state: value, ruleId: "test", manifestId: "omo" });
    };
    activity("working"); activity("idle");
    state = workspaceReducer(state, { type: "MARK_SESSION_ACTIVITY_SEEN", sessionId: "a" });
    state = workspaceReducer(state, { type: "SESSION_TITLE_ACTIVITY", tabId: "a", sessionId: "a", title: "OmO - review" });
    expect(state.activityBySessionId?.a.seen).toBe(true);
    activity("working"); activity("idle");
    expect(state.activityBySessionId?.a.seen).toBe(false);
  });

  it("acknowledges the focused pane through portable navigation", () => {
    let state = fixture();
    state = workspaceReducer(state, { type: "ACTIVATE_TAB", tabId: "a" });
    state = workspaceReducer(state, { type: "SPLIT_PANE", tabId: "a", direction: "horizontal", newLeafId: "second",
      session: { id: "c", cwd: "/repo", workspaceId: "attention-test", backendSessionId: "backend-c", worktree: null, lifecycle: "working" } });
    state = { ...state, activityBySessionId: { c: { state: "waiting", title: "", isAgent: true, seen: false } } };
    state = workspaceReducer(state, { type: "FOCUS_PANE", tabId: "a", leafId: "second" });
    expect(state.activityBySessionId?.c.seen).toBe(true);
  });

  it("worktree selection reads only the active tab, retaining hidden bell unread", () => {
    let state = fixture();
    state = { ...state, unreadTabIds: { a: true, b: true }, unreadWorktreePaths: { "/repo": true },
      activityBySessionId: { b: { state: "done", title: "", isAgent: true, seen: false } } };
    state = workspaceReducer(state, { type: "SELECT_WORKTREE", path: "/repo" });
    expect(state.activityBySessionId?.b.seen).toBe(true);
    expect(state.unreadTabIds).toEqual({ a: true });
    expect(state.unreadWorktreePaths["/repo"]).toBe(true);
  });
});
