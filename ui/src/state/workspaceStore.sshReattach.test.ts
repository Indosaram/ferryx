import { describe, expect, it } from "vitest";
import { workspaceReducer, type WorkspaceState } from "./workspaceStore";

describe("SSH same-process reattachment", () => {
  it("preserves agent identity, activity and pane layout when the stable backend is reattached", () => {
    const state: WorkspaceState = {
      workspaceId: "ssh:host-one:project",
      worktrees: [],
      activeWorktreePath: "/srv/project",
      sessions: {
        pane: {
          id: "pane",
          workspaceId: "ssh:host-one:project",
          cwd: "/srv/project",
          worktreePath: "/srv/project",
          worktree: null,
          backendSessionId: "stable-backend",
          lifecycle: "exited",
          agentType: "claude",
          agentSessionId: "remote-agent-session",
          providerSession: { key: "session_id", id: "remote-agent-session" },
          daemonEpoch: "old-local-epoch",
          lastOutputSequence: "44",
        },
      },
      layout: {
        tabs: [{ id: "tab", sessionId: "pane", label: "SSH" }],
        activeTabId: "tab",
        primaryTabId: "tab",
        secondaryTabId: null,
        split: "none",
        layoutsByTabId: {
          tab: {
            root: { type: "leaf", leafId: "leaf" },
            activeLeafId: "leaf",
            expandedLeafId: null,
            sessionIdsByLeafId: { leaf: "pane" },
          },
        },
      },
      activityBySessionId: {
        pane: { state: "working", title: "Claude", isAgent: true, agentType: "claude" },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
    };

    const rebound = workspaceReducer(state, {
      type: "REBIND_SESSION_BACKEND",
      sessionId: "pane",
      backendSessionId: "stable-backend",
      daemonEpoch: "new-local-epoch",
      cwd: "/srv/project",
    });

    expect(rebound.sessions.pane).toMatchObject({
      id: "pane",
      backendSessionId: "stable-backend",
      lifecycle: "running",
      daemonEpoch: "new-local-epoch",
      lastOutputSequence: null,
      agentType: "claude",
      agentSessionId: "remote-agent-session",
      providerSession: { key: "session_id", id: "remote-agent-session" },
    });
    expect(rebound.layout).toBe(state.layout);
    expect(rebound.activityBySessionId?.pane).toEqual(state.activityBySessionId?.pane);
  });

  it("preserves remote generation and failure details on SESSION_REMOTE_STATUS disconnection event", () => {
    const state: WorkspaceState = {
      workspaceId: "ssh:host-one:project",
      worktrees: [],
      activeWorktreePath: "/srv/project",
      sessions: {
        pane: {
          id: "pane",
          workspaceId: "ssh:host-one:project",
          cwd: "/srv/project",
          worktreePath: "/srv/project",
          worktree: null,
          backendSessionId: "active-backend",
          lifecycle: "working",
          remoteGeneration: 7,
          remoteConnectionState: "connected",
        },
      },
      layout: {
        tabs: [{ id: "tab", sessionId: "pane", label: "SSH" }],
        activeTabId: "tab",
        primaryTabId: "tab",
        secondaryTabId: null,
        split: "none",
        layoutsByTabId: {},
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
    };

    const disconnected = workspaceReducer(state, {
      type: "SESSION_REMOTE_STATUS",
      status: {
        sessionId: "active-backend",
        state: "disconnected",
        generation: 7,
        failure: { kind: "network", message: "Remote terminal control connection timed out" },
        replayGap: null,
      },
    });

    expect(disconnected.sessions.pane).toMatchObject({
      id: "pane",
      backendSessionId: "active-backend",
      remoteConnectionState: "disconnected",
      remoteGeneration: 7,
      remoteFailure: { kind: "network", message: "Remote terminal control connection timed out" },
    });
  });

  it("ignores stale SESSION_BACKEND_UNAVAILABLE with backendSessionId: null when session is already bound", () => {
    const state: WorkspaceState = {
      workspaceId: "ssh:host-one:project",
      worktrees: [],
      activeWorktreePath: "/srv/project",
      sessions: {
        pane: {
          id: "pane",
          workspaceId: "ssh:host-one:project",
          cwd: "/srv/project",
          worktreePath: "/srv/project",
          worktree: null,
          backendSessionId: "reconnected-backend",
          lifecycle: "working",
          remoteConnectionState: "connected",
        },
      },
      layout: {
        tabs: [{ id: "tab", sessionId: "pane", label: "SSH" }],
        activeTabId: "tab",
        primaryTabId: "tab",
        secondaryTabId: null,
        split: "none",
        layoutsByTabId: {},
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
    };

    const result = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "pane",
      backendSessionId: null,
      reason: "Stale split spawn failure",
    });

    // Must be ignored: session is already bound to reconnected-backend
    expect(result.sessions.pane.backendSessionId).toBe("reconnected-backend");
    expect(result.sessions.pane.lifecycle).toBe("working");
    expect(result.sessions.pane.remoteConnectionState).toBe("connected");
  });
});
