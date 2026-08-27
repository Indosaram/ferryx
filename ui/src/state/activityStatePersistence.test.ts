import { describe, expect, it } from "vitest";

import { resolveAgentLogo, SUPPORTED_AGENT_LOGOS } from "../lib/agentIcon";
import type { Worktree } from "../lib/types";
import { selectTabActivitySummaries, workspaceReducer, type WorkspaceState } from "./workspaceStore";

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

function titleAction(title: string) {
  return { type: "SESSION_TITLE_ACTIVITY", tabId: "tab-a", sessionId: "session-a", title } as const;
}

describe("agent activity survives non-status terminal titles", () => {
  it("keeps the working state when the agent writes a title carrying no status word", () => {
    let state = stateWithSession("tab-b");
    state = workspaceReducer(state, titleAction("\u280b codex: running tests"));
    expect(state.activityBySessionId?.["session-a"]?.state).toBe("working");

    // An agent that is still working frequently rewrites its title with progress detail that
    // carries no status keyword. The session is still working; the state must not be erased.
    state = workspaceReducer(state, titleAction("codex: src/lib/activity.ts"));

    expect(state.activityBySessionId?.["session-a"]?.state).toBe("working");
  });

  it("keeps the working state when a shell prompt repaints the title", () => {
    let state = stateWithSession("tab-b");
    state = workspaceReducer(state, titleAction("\u280b omo: building"));
    expect(state.activityBySessionId?.["session-a"]?.state).toBe("working");

    // A shell prompt redraw (cwd-style title) must not wipe an in-flight agent state.
    state = workspaceReducer(state, titleAction("~/code/project/orca-lite"));

    expect(state.activityBySessionId?.["session-a"]?.state).toBe("working");
  });

  it("still records the attention marker when a background session reaches done", () => {
    let state = stateWithSession("tab-b");
    state = workspaceReducer(state, titleAction("\u280b codex: running tests"));
    state = workspaceReducer(state, titleAction("codex: done"));

    expect(state.activityBySessionId?.["session-a"]?.state).toBe("done");
    expect(state.unreadTabIds["tab-a"]).toBe(true);
    expect(state.unreadWorktreePaths[worktree.path]).toBe(true);
  });

  it("clears prior agentType when a completed agent session returns to a non-agent shell title", () => {
    let state = stateWithSession("tab-a");
    // 1. Session runs Codex
    state = workspaceReducer(state, titleAction("\u280b codex: running tests"));
    expect(selectTabActivitySummaries(state)["tab-a"]?.agentType).toBe("codex");
    expect(resolveAgentLogo(selectTabActivitySummaries(state)["tab-a"]?.agentType)).toBe(SUPPORTED_AGENT_LOGOS.codex);

    // 2. Session finishes
    state = workspaceReducer(state, titleAction("codex: done"));
    expect(selectTabActivitySummaries(state)["tab-a"]?.agentType).toBe("codex");

    // 3. Shell returns with a non-agent title (e.g. zsh)
    state = workspaceReducer(state, titleAction("zsh"));

    // The prior Codex agentType must NOT persist; tab activity must have no agentType and fall back to terminal icon
    const tabActivity = selectTabActivitySummaries(state)["tab-a"];
    expect(tabActivity?.agentType).toBeUndefined();
    expect(resolveAgentLogo(tabActivity?.agentType)).toBeNull();
  });

  it("preserves screen-derived status state when returning to a shell title but drops the stale agent brand", () => {
    let state = stateWithSession("tab-a");
    // 1. Session has screen-derived working state with a Codex title
    state = workspaceReducer(state, {
      type: "SESSION_SCREEN_ACTIVITY",
      tabId: "tab-a",
      sessionId: "session-a",
      state: "working",
      ruleId: "screen_working_fallback",
    });
    state = workspaceReducer(state, titleAction("\u280b codex: running tests"));
    expect(state.activityBySessionId?.["session-a"]?.state).toBe("working");
    expect(selectTabActivitySummaries(state)["tab-a"]?.agentType).toBe("codex");

    // 2. Terminal returns to normal shell title while screen status remains
    state = workspaceReducer(state, titleAction("zsh"));

    // Screen-derived status state is preserved, but the stale brand is not carried through a non-agent title
    expect(state.activityBySessionId?.["session-a"]?.state).toBe("working");
    expect(state.activityBySessionId?.["session-a"]?.source).toBe("screen");
    const tabActivity = selectTabActivitySummaries(state)["tab-a"];
    expect(tabActivity?.agentType).toBeUndefined();
    expect(resolveAgentLogo(tabActivity?.agentType)).toBeNull();
  });
});
