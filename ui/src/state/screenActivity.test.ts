import { describe, expect, it } from "vitest";

import { resolveActivityIndicator } from "../lib/activity";
import type { Worktree } from "../lib/types";
import {
  selectTabActivitySummaries,
  selectWorktreeActivitySummaries,
  workspaceReducer,
  type WorkspaceState,
} from "./workspaceStore";

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/orca/ws-main/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function stateWithSession(activeTabId: string | null): WorkspaceState {
  return {
    worktrees: [worktree],
    activeWorktreePath: worktree.path,
    sessions: {
      "session-a": {
        id: "session-a",
        cwd: worktree.path,
        workspaceId: "default",
        worktree: { wsId: "ws-main", slug: "main" },
        backendSessionId: "backend-a",
        lifecycle: "working",
      },
    },
    layout: {
      tabs: [
        { id: "tab-a", label: "main", sessionId: "session-a" },
        { id: "tab-b", label: "other", sessionId: "session-b" },
      ],
      activeTabId,
      layoutsByTabId: {
        "tab-a": {
          root: { type: "leaf", leafId: "leaf-a" },
          activeLeafId: "leaf-a",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-a": "session-a" },
        },
      },
    },
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  } as unknown as WorkspaceState;
}

function screenAction(
  state: "working" | "blocked" | "idle",
  ruleId = "test_rule",
  manifestId?: string,
) {
  return {
    type: "SESSION_SCREEN_ACTIVITY",
    tabId: "tab-a",
    sessionId: "session-a",
    state,
    ruleId,
    manifestId,
  } as const;
}

function titleAction(title: string) {
  return {
    type: "SESSION_TITLE_ACTIVITY",
    tabId: "tab-a",
    sessionId: "session-a",
    title,
  } as const;
}

function twoSessionState(activeTabId: string, sessionBTabId: "tab-a" | "tab-b"): WorkspaceState {
  const base = stateWithSession(activeTabId);
  const sessionBLayouts = {
    ...base.layout.layoutsByTabId,
    [sessionBTabId]: {
      ...base.layout.layoutsByTabId!["tab-a"],
      sessionIdsByLeafId: {
        ...base.layout.layoutsByTabId!["tab-a"].sessionIdsByLeafId,
        "leaf-b": "session-b",
      },
    },
  };
  return {
    ...base,
    sessions: {
      ...base.sessions,
      "session-b": {
        ...base.sessions["session-a"],
        id: "session-b",
        backendSessionId: "backend-b",
      },
    },
    layout: {
      ...base.layout,
      layoutsByTabId: sessionBLayouts,
    },
  } as unknown as WorkspaceState;
}

function screenActionForSession(
  tabId: string,
  sessionId: string,
  state: "working" | "blocked" | "idle",
  ruleId = "test_rule",
  manifestId?: string,
) {
  return {
    type: "SESSION_SCREEN_ACTIVITY",
    tabId,
    sessionId,
    state,
    ruleId,
    manifestId,
  } as const;
}

describe("screen-rule agent detection contract (ui/src/state/screenActivity.test.ts)", () => {
  it("1. SESSION_SCREEN_ACTIVITY working sets state: 'working' and source: 'screen'", () => {
    let state = stateWithSession("tab-a");
    state = workspaceReducer(state, screenAction("working", "rule_working"));

    const activity = state.activityBySessionId?.["session-a"];
    expect(activity).toBeDefined();
    expect(activity?.state).toBe("working");
    expect(activity?.source).toBe("screen");
  });

  it("2. blocked maps to waiting", () => {
    let state = stateWithSession("tab-a");
    state = workspaceReducer(state, screenAction("blocked", "rule_blocked"));

    const activity = state.activityBySessionId?.["session-a"];
    expect(activity).toBeDefined();
    expect(activity?.state).toBe("waiting");
    expect(activity?.source).toBe("screen");
  });

  it("3. sets agent identity for supported screen detection manifestId (e.g. antigravity, omo, codex, claude, opencode)", () => {
    for (const agentType of ["antigravity", "omo", "codex", "claude", "opencode"] as const) {
      const state = workspaceReducer(stateWithSession("tab-a"), screenAction("working", "rule_working", agentType));

      expect(state.activityBySessionId?.["session-a"]).toMatchObject({
        isAgent: true,
        agentType,
        source: "screen",
        state: "working",
      });
    }
  });

  it("3b. unknown or unsupported screen detection manifestId does not mint an agent brand", () => {
    for (const unknownManifest of ["unsupported-tool", "bash", "custom_script"]) {
      const state = workspaceReducer(stateWithSession("tab-a"), screenAction("working", "rule_working", unknownManifest));

      expect(state.activityBySessionId?.["session-a"]).toMatchObject({
        isAgent: false,
        source: "screen",
        state: "working",
      });
      expect(state.activityBySessionId?.["session-a"]?.agentType).toBeUndefined();
    }
  });

  it("4. idle after working maps to done and marks a NON-VISIBLE tab + its worktree unread", () => {
    // tab-b is the active tab, so tab-a is non-visible
    let state = stateWithSession("tab-b");
    state = workspaceReducer(state, screenAction("working", "rule_working"));
    expect(state.activityBySessionId?.["session-a"]?.state).toBe("working");
    expect(state.unreadTabIds["tab-a"]).toBeUndefined();

    state = workspaceReducer(state, screenAction("idle", "rule_idle"));
    const activity = state.activityBySessionId?.["session-a"];
    expect(activity?.state).toBe("done");
    expect(activity?.source).toBe("screen");
    expect(state.unreadTabIds["tab-a"]).toBe(true);
    expect(state.unreadWorktreePaths[worktree.path]).toBe(true);
  });

  it("5. idle with no previous entry creates NO entry", () => {
    let state = stateWithSession("tab-b");
    // Initial activityBySessionId is empty
    expect(state.activityBySessionId?.["session-a"]).toBeUndefined();

    state = workspaceReducer(state, screenAction("idle", "rule_idle"));
    expect(state.activityBySessionId?.["session-a"]).toBeUndefined();
    expect(state.unreadTabIds["tab-a"]).toBeUndefined();
    expect(state.unreadWorktreePaths[worktree.path]).toBeUndefined();

    // Also: if previous is already done, idle does not change state or trigger duplicate unread marks
    state = workspaceReducer(state, screenAction("working", "rule_working"));
    state = workspaceReducer(state, screenAction("idle", "rule_idle"));
    expect(state.activityBySessionId?.["session-a"]?.state).toBe("done");

    const previousState = state;
    state = workspaceReducer(state, screenAction("idle", "rule_idle"));
    expect(state).toBe(previousState);
  });

  it("6. after a screen entry exists, a SESSION_TITLE_ACTIVITY with a contradictory title does not change state", () => {
    let state = stateWithSession("tab-b");
    // Establish screen activity as working
    state = workspaceReducer(state, screenAction("working", "rule_working"));
    expect(state.activityBySessionId?.["session-a"]?.state).toBe("working");
    expect(state.activityBySessionId?.["session-a"]?.source).toBe("screen");

    // Title claims the session is done
    state = workspaceReducer(state, titleAction("codex: done"));
    const activity = state.activityBySessionId?.["session-a"];
    // State MUST remain "working" from screen evidence, source must stay "screen"
    expect(activity?.state).toBe("working");
    expect(activity?.source).toBe("screen");
    // But title and agent info can be updated
    expect(activity?.isAgent).toBe(true);
    expect(activity?.agentType).toBe("codex");

    // Bare agent title must also not delete or change state of screen-sourced activity
    state = workspaceReducer(state, titleAction("codex"));
    const afterBare = state.activityBySessionId?.["session-a"];
    expect(afterBare?.state).toBe("working");
    expect(afterBare?.source).toBe("screen");
  });

  it("clears a stale agent identity when the terminal title returns to the shell", () => {
    let state = stateWithSession("tab-a");
    state = workspaceReducer(state, titleAction("\u280b codex: running tests"));
    expect(state.activityBySessionId?.["session-a"]?.agentType).toBe("codex");

    state = workspaceReducer(state, titleAction("zsh"));

    // The stale brand must go, but the in-flight run must not: agents routinely repaint the title
    // with no status word (measured: codex emits a bare project-name title while working), so a
    // non-agent title is not evidence the run ended. Deletion stays gated on `isBareAgentTitle`.
    expect(state.activityBySessionId?.["session-a"]).toMatchObject({
      state: "working",
      source: "title",
      isAgent: false,
    });
    expect(state.activityBySessionId?.["session-a"]?.agentType).toBeUndefined();
  });

  it("keeps screen activity and its authoritative screen brand when receiving a shell title while active", () => {
    let state = stateWithSession("tab-a");
    state = workspaceReducer(state, screenAction("working", "spinner_working", "omo"));
    expect(state.activityBySessionId?.["session-a"]?.agentType).toBe("omo");
    expect(state.activityBySessionId?.["session-a"]?.isAgent).toBe(true);

    state = workspaceReducer(state, titleAction("zsh"));

    expect(state.activityBySessionId?.["session-a"]).toMatchObject({
      state: "working",
      source: "screen",
      isAgent: true,
      agentType: "omo",
    });
  });

  it("8. a completion on the tab the user is watching shows no attention dot", () => {
    // tab-a is the active tab, so the user sees the agent finish in real time. An agent that boots
    // with a spinner and settles at its prompt walks working -> idle with no user turn at all.
    let state = stateWithSession("tab-a");
    state = workspaceReducer(state, screenAction("working", "spinner_working", "omo"));
    state = workspaceReducer(state, titleAction("OmO - orca-lite"));
    state = workspaceReducer(state, screenAction("idle", "prompt_idle", "omo"));

    const summary = selectTabActivitySummaries(state)["tab-a"];
    expect(resolveActivityIndicator(summary)).toBeNull();
    // The brand icon must survive: only the attention signal is acknowledged.
    expect(summary?.agentType).toBe("omo");
  });

  it("9. activating a tab acknowledges its completion dot", () => {
    let state = stateWithSession("tab-b");
    state = workspaceReducer(state, screenAction("working", "spinner_working", "omo"));
    state = workspaceReducer(state, titleAction("OmO - orca-lite"));
    state = workspaceReducer(state, screenAction("idle", "prompt_idle", "omo"));

    expect(resolveActivityIndicator(selectTabActivitySummaries(state)["tab-a"])).toBe("unread");

    state = workspaceReducer(state, { type: "ACTIVATE_TAB", tabId: "tab-a" });

    const summary = selectTabActivitySummaries(state)["tab-a"];
    expect(resolveActivityIndicator(summary)).toBeNull();
    expect(summary?.agentType).toBe("omo");
  });

  it("10. a new turn after acknowledgement signals again", () => {
    let state = stateWithSession("tab-a");
    state = workspaceReducer(state, screenAction("working", "spinner_working", "omo"));
    state = workspaceReducer(state, titleAction("OmO - orca-lite"));
    state = workspaceReducer(state, screenAction("idle", "prompt_idle", "omo"));
    expect(resolveActivityIndicator(selectTabActivitySummaries(state)["tab-a"])).toBeNull();

    // The user submits a real turn and switches away before it finishes.
    state = workspaceReducer(state, screenAction("working", "spinner_working", "omo"));
    expect(resolveActivityIndicator(selectTabActivitySummaries(state)["tab-a"])).toBe("working");

    state = workspaceReducer(state, { type: "ACTIVATE_TAB", tabId: "tab-b" });
    state = workspaceReducer(state, screenAction("idle", "prompt_idle", "omo"));
    expect(resolveActivityIndicator(selectTabActivitySummaries(state)["tab-a"])).toBe("unread");
  });

  it("11. an unseen completion flips the tab to attention even while another session keeps working", () => {
    // Two sessions share tab-a: session-a keeps working, session-b just finished unseen
    // (tab-b is the active tab, so neither completion is acknowledged by visibility).
    let state = twoSessionState("tab-b", "tab-a");
    state = workspaceReducer(state, screenAction("working", "spinner_working", "omo"));
    state = workspaceReducer(state, screenActionForSession("tab-a", "session-b", "working", "spinner_working", "codex"));
    expect(resolveActivityIndicator(selectTabActivitySummaries(state)["tab-a"])).toBe("working");

    state = workspaceReducer(state, screenAction("idle", "prompt_idle", "omo"));

    const summary = selectTabActivitySummaries(state)["tab-a"];
    expect(summary).toMatchObject({ hasDone: true, hasWorking: true, hasUnread: true });
    // The finished session wins the indicator over the still-running one.
    expect(resolveActivityIndicator(summary)).toBe("unread");
  });

  it("12. an unseen completion flips the worktree row while another session keeps working", () => {
    // session-a works on the active tab-b; session-b shares the worktree from non-visible tab-a
    // and finishes there, so its completion is unseen.
    let state = twoSessionState("tab-b", "tab-a");
    state = workspaceReducer(state, screenAction("working", "spinner_working", "omo"));
    state = workspaceReducer(state, screenActionForSession("tab-a", "session-b", "working", "spinner_working", "codex"));
    state = workspaceReducer(state, screenActionForSession("tab-a", "session-b", "idle", "prompt_idle"));

    const summary = selectWorktreeActivitySummaries(state)[worktree.path];
    expect(summary).toMatchObject({ hasDone: true, hasWorking: true });
    expect(resolveActivityIndicator(summary)).toBe("unread");
  });

  it("7. title-only sessions keep working exactly as before (existing tests must stay green)", () => {
    let state = stateWithSession("tab-b");

    // Title sets working
    state = workspaceReducer(state, titleAction("\u280b codex: running tests"));
    expect(state.activityBySessionId?.["session-a"]?.state).toBe("working");
    expect(state.activityBySessionId?.["session-a"]?.source).toBe("title");

    // Hold-on-unclassifiable title carries previous state forward
    state = workspaceReducer(state, titleAction("codex: src/lib/activity.ts"));
    expect(state.activityBySessionId?.["session-a"]?.state).toBe("working");

    // Title sets done -> marks non-visible tab and worktree unread
    state = workspaceReducer(state, titleAction("codex: done"));
    expect(state.activityBySessionId?.["session-a"]?.state).toBe("done");
    expect(state.unreadTabIds["tab-a"]).toBe(true);
    expect(state.unreadWorktreePaths[worktree.path]).toBe(true);

    // Bare agent title deletes activity for title-only session
    state = workspaceReducer(state, titleAction("codex"));
    expect(state.activityBySessionId?.["session-a"]).toBeUndefined();
  });

  describe("MARK_SESSION_ACTIVITY_SEEN", () => {
    it("flips an unseen done activity to seen while leaving unreadTabIds and unreadWorktreePaths untouched", () => {
      let state = stateWithSession("tab-b");
      state = workspaceReducer(state, screenAction("working", "spinner_working", "omo"));
      state = workspaceReducer(state, screenAction("idle", "prompt_idle", "omo"));

      expect(state.activityBySessionId?.["session-a"]).toMatchObject({ state: "done" });
      expect(state.activityBySessionId?.["session-a"]?.seen).toBeFalsy();
      expect(state.unreadTabIds["tab-a"]).toBe(true);
      expect(state.unreadWorktreePaths[worktree.path]).toBe(true);

      const nextState = workspaceReducer(state, {
        type: "MARK_SESSION_ACTIVITY_SEEN",
        sessionId: "session-a",
      });

      expect(nextState.activityBySessionId?.["session-a"]?.seen).toBe(true);
      expect(nextState.activityBySessionId?.["session-a"]?.state).toBe("done");
      // unreadTabIds / unreadWorktreePaths are owned by tab-switch flow and must not be touched
      expect(nextState.unreadTabIds["tab-a"]).toBe(true);
      expect(nextState.unreadWorktreePaths[worktree.path]).toBe(true);
    });

    it("leaves working, already-seen entries and unknown session ids untouched (referential equality); waiting is dismissible", () => {
      let state = stateWithSession("tab-b");
      // Unknown session id
      expect(
        workspaceReducer(state, { type: "MARK_SESSION_ACTIVITY_SEEN", sessionId: "non-existent" }),
      ).toBe(state);

      // Working activity
      state = workspaceReducer(state, screenAction("working", "spinner_working", "omo"));
      expect(
        workspaceReducer(state, { type: "MARK_SESSION_ACTIVITY_SEEN", sessionId: "session-a" }),
      ).toBe(state);

      // Waiting (blocked) activity IS dismissible: a spurious blocked detection must not
      // leave an attention frame the user can never clear, so marking seen applies to waiting too.
      state = workspaceReducer(state, screenAction("blocked", "question_blocked", "omo"));
      const waitingSeen = workspaceReducer(state, {
        type: "MARK_SESSION_ACTIVITY_SEEN",
        sessionId: "session-a",
      });
      expect(waitingSeen.activityBySessionId?.["session-a"]?.seen).toBe(true);
      expect(
        workspaceReducer(waitingSeen, {
          type: "MARK_SESSION_ACTIVITY_SEEN",
          sessionId: "session-a",
        }),
      ).toBe(waitingSeen);

      // Done but already seen
      state = workspaceReducer(waitingSeen, screenAction("idle", "prompt_idle", "omo"));
      const seenState = workspaceReducer(state, {
        type: "MARK_SESSION_ACTIVITY_SEEN",
        sessionId: "session-a",
      });
      expect(seenState.activityBySessionId?.["session-a"]?.seen).toBe(true);
      expect(
        workspaceReducer(seenState, {
          type: "MARK_SESSION_ACTIVITY_SEEN",
          sessionId: "session-a",
        }),
      ).toBe(seenState);
    });
  });
});
