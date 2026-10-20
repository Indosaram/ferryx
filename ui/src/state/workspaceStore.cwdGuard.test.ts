import { describe, expect, it } from "vitest";

import type { TerminalSession, TerminalTab } from "../lib/types";
import { createLayoutState } from "./layout";
import { workspaceReducer, type WorkspaceState } from "./workspaceStore";

function createInitialState(session: TerminalSession): WorkspaceState {
  const tab: TerminalTab = {
    id: "tab-1",
    label: "Test Tab",
    sessionId: session.id,
  };
  return {
    workspaceId: session.workspaceId,
    worktrees: [],
    activeWorktreePath: "/repo",
    layout: createLayoutState([tab], "tab-1"),
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    sessions: {
      [session.id]: session,
    },
    activityBySessionId: {},
  };
}

const POISONED_CWD = "cwd|rtd info error: No such file or directory";

describe("workspaceStore backend failure reason", () => {
  it("keeps the local failure reason on the session so the pane can explain itself", () => {
    const session: TerminalSession = {
      id: "term-1",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-1",
      lifecycle: "working",
    };

    const next = workspaceReducer(createInitialState(session), {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-1",
      backendSessionId: "backend-1",
      reason: `CWD does not exist: ${POISONED_CWD}`,
    });

    expect(next.sessions["term-1"].lifecycle).toBe("exited");
    expect(next.sessions["term-1"].backendUnavailableReason).toBe(`CWD does not exist: ${POISONED_CWD}`);
  });
});

describe("workspaceStore REBIND_SESSION_BACKEND cwd guard", () => {
  it("ignores a rebind cwd that is not an absolute path", () => {
    const session: TerminalSession = {
      id: "term-1",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: null,
      lifecycle: "exited",
    };

    const next = workspaceReducer(createInitialState(session), {
      type: "REBIND_SESSION_BACKEND",
      sessionId: "term-1",
      backendSessionId: "backend-new",
      cwd: POISONED_CWD,
    });

    expect(next.sessions["term-1"].backendSessionId).toBe("backend-new");
    expect(next.sessions["term-1"].cwd).toBe("/repo");
  });

  it("accepts an absolute rebind cwd and clears the stored failure reason", () => {
    const session: TerminalSession = {
      id: "term-1",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: null,
      lifecycle: "exited",
      backendUnavailableReason: "Failed to spawn terminal",
    };

    const next = workspaceReducer(createInitialState(session), {
      type: "REBIND_SESSION_BACKEND",
      sessionId: "term-1",
      backendSessionId: "backend-new",
      cwd: "/repo/sub",
    });

    expect(next.sessions["term-1"].cwd).toBe("/repo/sub");
    expect(next.sessions["term-1"].backendUnavailableReason).toBeNull();
  });
});
