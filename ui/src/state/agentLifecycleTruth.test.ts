import { describe, expect, it } from "vitest";

import { resolveActivityIndicator } from "../lib/activity";
import type { LayoutState, TerminalSession, TerminalTab, Worktree } from "../lib/types";
import {
  selectAgents,
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

const tab: TerminalTab = { id: "tab-a", label: "main", sessionId: "session-a" };
const otherTab: TerminalTab = { id: "tab-other", label: "other", sessionId: "session-other" };

const layout: LayoutState = {
  tabs: [tab, otherTab],
  // A different tab is active so nothing is auto-acknowledged as observed.
  activeTabId: otherTab.id,
  layoutsByTabId: {
    [tab.id]: {
      root: { type: "leaf", leafId: "leaf-a" },
      activeLeafId: "leaf-a",
      expandedLeafId: null,
      sessionIdsByLeafId: { "leaf-a": "session-a" },
    },
  },
};

function stateWithWorkingSession(session: Partial<TerminalSession> = {}): WorkspaceState {
  const terminalSession: TerminalSession = {
    id: "session-a",
    cwd: worktree.path,
    worktreePath: worktree.path,
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId: "backend-a",
    lifecycle: "working",
    ...session,
  };
  return {
    workspaceId: terminalSession.workspaceId,
    worktrees: [worktree],
    activeWorktreePath: worktree.path,
    sessions: { "session-a": terminalSession },
    layout,
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {
      "session-a": {
        state: "working",
        title: "\u280b omo: building",
        isAgent: true,
        agentType: "omo",
        source: "screen",
        agentSource: "screen",
      },
    },
  };
}

function remoteState(): WorkspaceState {
  return stateWithWorkingSession({ workspaceId: "ssh:build", cwd: "/srv/repo", worktreePath: worktree.path });
}

function exitedState(lifecycle: "exited" | "failed" = "exited"): WorkspaceState {
  return workspaceReducer(stateWithWorkingSession(), {
    type: "SESSION_LIFECYCLE",
    backendSessionId: "backend-a",
    lifecycle,
  });
}

describe("remote status death clears live agent work", () => {
  it.each(["expired", "legacyLost"] as const)(
    "settles the working activity when the daemon reports the remote session %s",
    (lostState) => {
      const next = workspaceReducer(remoteState(), {
        type: "SESSION_REMOTE_STATUS",
        status: { sessionId: "backend-a", state: lostState, generation: 0, failure: null, replayGap: null },
      });

      expect(next.sessions["session-a"].lifecycle).toBe("exited");
      expect(next.activityBySessionId?.["session-a"]?.state).toBe("done");
      expect(resolveActivityIndicator(selectWorktreeActivitySummaries(next)[worktree.path])).not.toBe("working");
    },
  );

  it("keeps live remote work when the queried daemon merely does not know the session", () => {
    // `missing` is not death: a draining predecessor daemon may still own this session, or restore
    // has not completed. Only `expired` is a daemon-confirmed process exit.
    const next = workspaceReducer(remoteState(), {
      type: "SESSION_REMOTE_STATUS",
      status: { sessionId: "backend-a", state: "missing", generation: 0, failure: null, replayGap: null },
    });

    expect(next.sessions["session-a"].lifecycle).toBe("working");
    expect(next.sessions["session-a"].remoteConnectionState).toBe("missing");
    expect(next.activityBySessionId?.["session-a"]?.state).toBe("working");
    expect(resolveActivityIndicator(selectWorktreeActivitySummaries(next)[worktree.path])).toBe("working");
  });

  it.each(["disconnected", "reconnecting"] as const)(
    "keeps live remote work while the transport is merely %s",
    (transportState) => {
      const next = workspaceReducer(remoteState(), {
        type: "SESSION_REMOTE_STATUS",
        status: { sessionId: "backend-a", state: transportState, generation: 3, failure: null, replayGap: null },
      });

      // A dropped SSH control channel says nothing about the remote process: the agent keeps working.
      expect(next.sessions["session-a"].lifecycle).toBe("working");
      expect(next.activityBySessionId?.["session-a"]?.state).toBe("working");
    },
  );
});

describe("confirmed-missing remote backend clears live agent work", () => {
  // SESSION_BACKEND_UNAVAILABLE is only dispatched for a `confirmed-missing` attach classification
  // (the daemon answered that the session does not exist) or a spawn that produced no PTY.
  // Operational/network errors classify as `operational-error` and never reach this reducer.
  it("settles the working activity for a remote session whose daemon session is gone", () => {
    const next = workspaceReducer(remoteState(), {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "session-a",
      backendSessionId: "backend-a",
      reason: "daemon-attach-not-found",
    });

    expect(next.sessions["session-a"].lifecycle).toBe("exited");
    expect(next.activityBySessionId?.["session-a"]?.state).toBe("done");
    expect(resolveActivityIndicator(selectWorktreeActivitySummaries(next)[worktree.path])).not.toBe("working");
  });
});

describe("a failed PTY settles live work just like a clean exit", () => {
  it("settles the working activity when the backend reports failure", () => {
    const next = exitedState("failed");

    expect(next.sessions["session-a"].lifecycle).toBe("failed");
    expect(next.activityBySessionId?.["session-a"]?.state).toBe("done");
    expect(resolveActivityIndicator(selectWorktreeActivitySummaries(next)[worktree.path])).not.toBe("working");
  });
});

describe("activity cannot resurrect a dead session", () => {
  it.each(["working", "blocked"] as const)("ignores a late %s screen report after the PTY exited", (late) => {
    const exited = exitedState();
    expect(exited.sessions["session-a"].lifecycle).toBe("exited");
    expect(exited.activityBySessionId?.["session-a"]?.state).toBe("done");

    const next = workspaceReducer(exited, {
      type: "SESSION_SCREEN_ACTIVITY",
      tabId: tab.id,
      sessionId: "session-a",
      state: late,
      ruleId: "extension",
      manifestId: "omo",
    });

    expect(next.activityBySessionId?.["session-a"]?.state).toBe("done");
    expect(resolveActivityIndicator(selectWorktreeActivitySummaries(next)[worktree.path])).not.toBe("working");
  });

  it.each([
    ["working", "\u280b omo: building"],
    ["waiting", "\u270b omo: permission required"],
  ] as const)("ignores a late title claiming %s after the PTY exited", (_label, title) => {
    const exited = exitedState();

    const next = workspaceReducer(exited, {
      type: "SESSION_TITLE_ACTIVITY",
      tabId: tab.id,
      sessionId: "session-a",
      title,
    });

    expect(next.activityBySessionId?.["session-a"]?.state).toBe("done");
    expect(resolveActivityIndicator(selectWorktreeActivitySummaries(next)[worktree.path])).not.toBe("working");
  });

  it.each([
    ["working", "\u280b omo: building"],
    ["waiting", "\u270b omo: permission required"],
  ] as const)("ignores a late title-sourced %s claim after the PTY exited", (_label, title) => {
    const titleSourced: WorkspaceState = {
      ...stateWithWorkingSession(),
      activityBySessionId: {
        "session-a": { state: "working", title: "\u280b omo: building", isAgent: true, agentType: "omo", source: "title", agentSource: "title" },
      },
    };
    const exited = workspaceReducer(titleSourced, {
      type: "SESSION_LIFECYCLE",
      backendSessionId: "backend-a",
      lifecycle: "exited",
    });
    expect(exited.activityBySessionId?.["session-a"]?.state).toBe("done");

    const next = workspaceReducer(exited, {
      type: "SESSION_TITLE_ACTIVITY",
      tabId: tab.id,
      sessionId: "session-a",
      title,
    });

    expect(next.activityBySessionId?.["session-a"]?.state).toBe("done");
    expect(resolveActivityIndicator(selectWorktreeActivitySummaries(next)[worktree.path])).not.toBe("working");
  });

  it("still lets a title settle a dead session and refresh its label", () => {
    const exited = exitedState();

    const next = workspaceReducer(exited, {
      type: "SESSION_TITLE_ACTIVITY",
      tabId: tab.id,
      sessionId: "session-a",
      title: "omo: done",
    });

    expect(next.activityBySessionId?.["session-a"]?.state).toBe("done");
  });

  it("accepts screen work again once the session is rebound to a live backend", () => {
    const rebound = workspaceReducer(exitedState(), {
      type: "REBIND_SESSION_BACKEND",
      sessionId: "session-a",
      backendSessionId: "backend-b",
    });

    const next = workspaceReducer(rebound, {
      type: "SESSION_SCREEN_ACTIVITY",
      tabId: tab.id,
      sessionId: "session-a",
      state: "working",
      ruleId: "extension",
      manifestId: "omo",
    });

    expect(next.activityBySessionId?.["session-a"]?.state).toBe("working");
  });
});

describe("replacing a backend keeps activity honest", () => {
  // Every local rebind caller (ensureSessionBackends, replaceExitedShellSession,
  // reconnectAgentSession) requires backendSessionId === null first, so a local pane never swaps
  // one LIVE backend for another. The reachable local case is the rebind of an exited pane, whose
  // claim SESSION_LIFECYCLE already settled.
  it("does not carry a working claim from the dead process into the replacement", () => {
    const next = workspaceReducer(exitedState(), {
      type: "REBIND_SESSION_BACKEND",
      sessionId: "session-a",
      backendSessionId: "backend-replacement",
    });

    expect(next.sessions["session-a"].backendSessionId).toBe("backend-replacement");
    expect(next.activityBySessionId?.["session-a"]?.state).not.toBe("working");
    expect(resolveActivityIndicator(selectWorktreeActivitySummaries(next)[worktree.path])).not.toBe("working");
  });

  it("drops the replaced remote process's activity entirely", () => {
    const next = workspaceReducer(remoteState(), {
      type: "REBIND_SESSION_BACKEND",
      sessionId: "session-a",
      backendSessionId: "backend-replacement",
    });

    expect(next.activityBySessionId?.["session-a"]).toBeUndefined();
    expect(resolveActivityIndicator(selectWorktreeActivitySummaries(next)[worktree.path])).not.toBe("working");
  });

  it("keeps activity for a same-process reattach", () => {
    const next = workspaceReducer(stateWithWorkingSession(), {
      type: "REBIND_SESSION_BACKEND",
      sessionId: "session-a",
      backendSessionId: "backend-a",
      daemonEpoch: "epoch-2",
    });

    expect(next.activityBySessionId?.["session-a"]?.state).toBe("working");
  });
});

describe("selectAgents reports agent work only from observed activity", () => {
  it("does not claim a bare live PTY is a working agent", () => {
    const withoutActivity: WorkspaceState = { ...stateWithWorkingSession(), activityBySessionId: {} };

    expect(selectAgents(withoutActivity)[0]?.state).not.toBe("working");
  });

  it("still reports work that the activity tracker actually observed", () => {
    expect(selectAgents(stateWithWorkingSession())[0]?.state).toBe("working");
  });

  it("reports an exited session as exited", () => {
    const withoutActivity: WorkspaceState = { ...exitedState(), activityBySessionId: {} };

    expect(selectAgents(withoutActivity)[0]?.state).toBe("exited");
  });
});
