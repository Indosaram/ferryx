import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { Worktree } from "../../lib/types";
import {
  resetAttentionEngagementClocksForTests,
  selectWorktreeActivitySummaries,
  workspaceReducer,
  type WorkspaceState,
} from "../../state/workspaceStore";
import { worktreeRowLive } from "./sidebarModel";
import { SidebarWorktreeRow } from "./SidebarWorktreeRow";

beforeEach(() => resetAttentionEngagementClocksForTests());
afterEach(cleanup);

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/orca/ws-main/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

// session-a lives in tab-a; tab-b is active, so tab-a's completion lands unseen.
function background(): WorkspaceState {
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
      activeTabId: "tab-b",
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

function frame(state: "working" | "blocked" | "idle", extra: { outcome?: "done" | "error"; kind?: "permission" | "question" | "auth"; detail?: string } = {}) {
  return { type: "SESSION_SCREEN_ACTIVITY", tabId: "tab-a", sessionId: "session-a", state, ruleId: "", manifestId: "omo", ...extra } as const;
}

function finish(outcome?: "done" | "error"): WorkspaceState {
  const working = workspaceReducer(background(), frame("working"));
  return workspaceReducer(working, frame("idle", outcome ? { outcome } : {}));
}

function renderRow(state: WorkspaceState) {
  const live = worktreeRowLive({ path: worktree.path }, false, {
    activityByWorktreePath: selectWorktreeActivitySummaries(state),
    unreadWorktreePaths: state.unreadWorktreePaths,
  });
  render(<SidebarWorktreeRow node={{ path: worktree.path, label: "main", hostLabel: null, stale: false }} live={live} />);
  return live;
}

describe("program outcome mark on sidebar rows", () => {
  it.each(["done", "error"] as const)("shows %s on the idle row until the pane is focused", (outcome) => {
    const finished = finish(outcome);
    expect(finished.activityBySessionId?.["session-a"]).toMatchObject({ state: "done", outcome, seen: false });
    renderRow(finished);
    const mark = screen.getByTestId("worktree-outcome-mark");
    expect(mark).toHaveAttribute("data-outcome", outcome);
    expect(mark).toHaveTextContent(outcome);
    cleanup();

    const focused = workspaceReducer(finished, { type: "FOCUS_PANE", tabId: "tab-a", leafId: "leaf-a" });
    const live = renderRow(focused);
    expect(live.outcome).toBeUndefined();
    expect(screen.queryByTestId("worktree-outcome-mark")).toBeNull();
  });

  it("renders a frame without outcome exactly as before", () => {
    const live = renderRow(finish());
    expect(live).toEqual({ active: false, slot: "unread", supervisorTyping: false });
    expect(screen.queryByTestId("worktree-outcome-mark")).toBeNull();
    expect(screen.getByTestId("worktree-status-dot")).toHaveAttribute("data-slot-state", "unread");
  });

  it("drops the outcome when the program starts a new turn", () => {
    const again = workspaceReducer(finish("error"), frame("working"));
    expect(again.activityBySessionId?.["session-a"]?.outcome).toBeUndefined();
    expect(worktreeRowLive({ path: worktree.path }, false, { activityByWorktreePath: selectWorktreeActivitySummaries(again) }).outcome).toBeUndefined();
  });

  it("keeps a blocked kind and its detail as given", () => {
    const blocked = workspaceReducer(background(), frame("blocked", { kind: "permission", detail: "<b>x</b>" }));
    expect(blocked.activityBySessionId?.["session-a"]).toMatchObject({ state: "waiting", kind: "permission", detail: "<b>x</b>" });
  });
});
