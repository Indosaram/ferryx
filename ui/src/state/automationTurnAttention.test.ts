import { beforeEach, describe, expect, it } from "vitest";
import { createLayoutState } from "./layout";
import {
  markSessionEngagementForAttention,
  resetAttentionEngagementClocksForTests,
  workspaceReducer,
  type WorkspaceState,
} from "./workspaceStore";

function fixture(): WorkspaceState {
  let state: WorkspaceState = {
    workspaceId: "automation-attention-test",
    worktrees: [{ path: "/repo", head: "abc", branch: "main", bare: false, detached: false, locked: null, prunable: null }],
    activeWorktreePath: "/repo",
    sessions: {}, layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {},
  };
  // Two tabs so the observed session's tab stays backgrounded (the second ADD activates "b"):
  // assertions here must not ride on active-tab auto-acknowledgment.
  for (const id of ["a", "b"] as const) {
    state = workspaceReducer(state, {
      type: "ADD_TAB_WITH_SESSION",
      tab: { id, label: id, sessionId: id },
      session: { id, cwd: "/repo", workspaceId: "automation-attention-test", backendSessionId: `backend-${id}`, worktree: null, lifecycle: "working" },
    });
  }
  return state;
}

const screen = (state: WorkspaceState, value: "working" | "blocked" | "idle") =>
  workspaceReducer(state, { type: "SESSION_SCREEN_ACTIVITY", tabId: "a", sessionId: "a", state: value, ruleId: "test", manifestId: "omo" });

describe("automation turn attention quieting", () => {
  beforeEach(() => {
    resetAttentionEngagementClocksForTests();
  });

  it("notifies the first completion but stores later unattended completions quietly", () => {
    let state = fixture();
    state = screen(state, "working");
    state = screen(state, "idle");
    expect(state.activityBySessionId?.a).toMatchObject({ state: "done", seen: false, notificationSuppressed: false });
    state = screen(state, "working");
    state = screen(state, "idle");
    expect(state.activityBySessionId?.a).toMatchObject({ state: "done", seen: true, notificationSuppressed: true });
  });

  it("keeps a completion loud when the user engaged after the previous episode", () => {
    let state = fixture();
    state = screen(state, "working");
    state = screen(state, "idle");
    markSessionEngagementForAttention("a");
    state = screen(state, "working");
    state = screen(state, "idle");
    expect(state.activityBySessionId?.a).toMatchObject({ state: "done", seen: false, notificationSuppressed: false });
  });

  it("never quiets waiting even without engagement", () => {
    let state = fixture();
    state = screen(state, "working");
    state = screen(state, "idle");
    state = screen(state, "working");
    state = screen(state, "blocked");
    expect(state.activityBySessionId?.a).toMatchObject({ state: "waiting", seen: false, notificationSuppressed: false });
  });

  it("records a fresh episode after waiting so the next unattended done is quiet", () => {
    let state = fixture();
    state = screen(state, "working");
    state = screen(state, "blocked");
    expect(state.activityBySessionId?.a).toMatchObject({ state: "waiting", seen: false });
    state = screen(state, "working");
    state = screen(state, "idle");
    expect(state.activityBySessionId?.a).toMatchObject({ state: "done", seen: true, notificationSuppressed: true });
  });

  it("does not let a same-state refresh erase the episode clock for an earlier engagement", () => {
    let state = fixture();
    state = screen(state, "working");
    state = screen(state, "idle");
    markSessionEngagementForAttention("a");
    state = screen(state, "working");
    state = screen(state, "idle");
    expect(state.activityBySessionId?.a).toMatchObject({ seen: false, notificationSuppressed: false });
  });
});
