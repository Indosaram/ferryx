import { describe, expect, it } from "vitest";

import { deserializeWorkspaceState, serializeWorkspaceState } from "./sessionPersistence";
import type { WorkspaceState } from "../state/workspaceStore";

const DAEMON_EPOCH = "1787837134521";

/**
 * A workspace holding one running agent, shaped the way the store holds it at save time.
 *
 * `daemonEpoch` is deliberately absent: no reducer ever writes it onto a session, which is the
 * condition this suite pins.
 */
function workspaceWithRunningAgent(): WorkspaceState {
  return {
    workspaceId: "maho-workspace",
    repoRoot: "/repo/maho",
    worktrees: [
      { path: "/repo/maho", head: "abc", branch: "refs/heads/main", bare: false, detached: false, locked: null, prunable: null },
    ],
    activeWorktreePath: "/repo/maho",
    sessions: {
      "session:ui-1": {
        id: "session:ui-1",
        cwd: "/repo/maho",
        worktreePath: "/repo/maho",
        workspaceId: "maho-workspace",
        worktree: null,
        backendSessionId: "backend-alive",
        lifecycle: "running",
        lastOutputSequence: "42",
      },
    },
    layout: {
      tabs: [{ id: "tab:1", kind: "terminal", label: "main", sessionId: "session:ui-1" }],
      activeTabId: "tab:1",
      layoutsByTabId: {
        "tab:1": {
          root: { type: "leaf", leafId: "leaf:1" },
          activeLeafId: "leaf:1",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf:1": "session:ui-1" },
        },
      },
    },
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  } as unknown as WorkspaceState;
}

function restore(
  liveEpoch: string,
  options: { persistedEpoch?: string; liveSessionId?: string } = {},
) {
  const state = workspaceWithRunningAgent();
  if (options.persistedEpoch !== undefined) {
    state.sessions["session:ui-1"]!.daemonEpoch = options.persistedEpoch;
  }
  const persisted = serializeWorkspaceState("maho-workspace", "/repo/maho", state);
  return deserializeWorkspaceState("maho-workspace", persisted, [
    {
      sessionId: options.liveSessionId ?? "backend-alive",
      daemonEpoch: liveEpoch,
      worktreePath: "/repo/maho",
    },
  ]);
}

describe("restoring a session whose PTY is still alive in the daemon", () => {
  it("reattaches the live PTY even though no reducer ever recorded a daemon epoch", () => {
    const session = restore(DAEMON_EPOCH)?.sessions["session:ui-1"];

    expect(session?.backendSessionId).toBe("backend-alive");
    expect(session?.lifecycle).not.toBe("exited");
    expect(session?.lastOutputSequence).toBe("42");
  });

  it("adopts the live epoch so the next save can detect a real daemon restart", () => {
    const restored = restore(DAEMON_EPOCH);
    expect(restored?.sessions["session:ui-1"]?.daemonEpoch).toBe(DAEMON_EPOCH);

    const resaved = serializeWorkspaceState("maho-workspace", "/repo/maho", restored!);
    expect(resaved.workspaces["maho-workspace"]?.terminalSessions?.["session:ui-1"]?.daemonEpoch).toBe(
      DAEMON_EPOCH,
    );

    const afterDaemonRestart = deserializeWorkspaceState("maho-workspace", resaved, [
      { sessionId: "backend-alive", daemonEpoch: "9999999999999", worktreePath: "/repo/maho" },
    ]);
    expect(afterDaemonRestart?.sessions["session:ui-1"]?.backendSessionId).toBeNull();
  });

  it("abandons a session whose recorded epoch belongs to an older daemon", () => {
    const session = restore(DAEMON_EPOCH, { persistedEpoch: "1000000000000" })?.sessions["session:ui-1"];

    expect(session?.backendSessionId).toBeNull();
    expect(session?.lifecycle).toBe("exited");
  });

  it("abandons a session the daemon no longer lists", () => {
    const session = restore(DAEMON_EPOCH, { liveSessionId: "someone-else" })?.sessions["session:ui-1"];

    expect(session?.backendSessionId).toBeNull();
    expect(session?.lifecycle).toBe("exited");
  });
});
