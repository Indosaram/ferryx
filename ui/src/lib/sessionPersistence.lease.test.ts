import { describe, expect, it } from "vitest";
import type { WorkspaceState } from "../state/workspaceStore";
import { deserializeWorkspaceState, serializeWorkspaceState } from "./sessionPersistence";
import type { TerminalSession } from "./types";

type SessionWithLease = TerminalSession & { remoteLease?: unknown };

function state(): WorkspaceState {
  return {
    worktrees: [],
    activeWorktreePath: "/workspace",
    sessions: {
      local: {
        id: "local",
        cwd: "/workspace",
        worktreePath: "/workspace",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "remote-pty",
        lifecycle: "working",
        remoteLease: {
          sessionId: "remote-pty",
          targetId: "host-a",
          leafId: "leaf-a",
          state: "detached",
          pendingKill: true,
          createdAt: 10,
          updatedAt: 20,
          detachedAt: 20,
          clientInstanceId: "client-a",
          generation: 7,
        },
      } as SessionWithLease,
    },
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
    layout: {
      tabs: [{ id: "tab-a", label: "remote", sessionId: "local" }],
      primaryTabId: "tab-a",
      secondaryTabId: null,
      split: "none",
      nestedSplit: null,
      activeTabId: "tab-a",
      layoutsByTabId: {
        "tab-a": {
          root: { type: "leaf", leafId: "leaf-a" },
          activeLeafId: "leaf-a",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-a": "local" },
        },
      },
    },
  };
}

describe("remote PTY lease persistence", () => {
  it("round-trips the optional lease mirror without bumping session v3", () => {
    const serialized = serializeWorkspaceState("default", "/workspace", state());
    expect(serialized.version).toBe(3);
    expect(serialized.workspaces.default.terminalSessions.local.remoteLease).toMatchObject({
      sessionId: "remote-pty",
      state: "detached",
      pendingKill: true,
      generation: 7,
    });

    const restored = deserializeWorkspaceState("default", serialized);
    expect((restored!.sessions.local as SessionWithLease).remoteLease).toEqual(
      serialized.workspaces.default.terminalSessions.local.remoteLease,
    );
  });

  it("loads pre-lease v3 sessions with a null mirror", () => {
    const serialized = serializeWorkspaceState("default", "/workspace", state());
    delete serialized.workspaces.default.terminalSessions.local.remoteLease;

    const restored = deserializeWorkspaceState("default", serialized);
    expect((restored!.sessions.local as SessionWithLease).remoteLease).toBeNull();
  });
});
