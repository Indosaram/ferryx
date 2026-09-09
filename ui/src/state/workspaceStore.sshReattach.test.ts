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
});
