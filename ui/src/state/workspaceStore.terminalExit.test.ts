import { describe, expect, it } from "vitest";

import { workspaceReducer, type WorkspaceState } from "./workspaceStore";
import { createLayoutState } from "./layout";
import type { TerminalSession, TerminalTab } from "../lib/types";

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

describe("workspaceStore SESSION_BACKEND_UNAVAILABLE compare-and-set reducer", () => {
  it("transitions a local session to exited and clears backendSessionId when backendSessionId matches", () => {
    const session: TerminalSession = {
      id: "term-1",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-live-1",
      lifecycle: "working",
    };

    const state = createInitialState(session);
    const next = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-1",
      backendSessionId: "backend-live-1",
      reason: "daemon-attach-not-found",
    });

    expect(next.sessions["term-1"].lifecycle).toBe("exited");
    expect(next.sessions["term-1"].backendSessionId).toBeNull();
    expect(next.sessions["term-1"].reconnectLifecycle).toBe("idle");
  });

  it("is a no-op when backendSessionId has already re-bound to a different ID", () => {
    const session: TerminalSession = {
      id: "term-1",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-new-2",
      lifecycle: "working",
    };

    const state = createInitialState(session);
    const next = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-1",
      backendSessionId: "backend-stale-1",
      reason: "daemon-attach-not-found",
    });

    expect(next).toBe(state);
    expect(next.sessions["term-1"].lifecycle).toBe("working");
    expect(next.sessions["term-1"].backendSessionId).toBe("backend-new-2");
  });

  it("marks remote workspace session disconnected with a failure", () => {
    const session: TerminalSession = {
      id: "term-remote",
      cwd: "/remote/path",
      workspaceId: "ssh:remote-host-1",
      worktree: { wsId: "ssh:remote-host-1", slug: "remote" },
      backendSessionId: "backend-remote-1",
      lifecycle: "working",
    };

    const state = createInitialState(session);
    const next = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-remote",
      backendSessionId: "backend-remote-1",
      reason: "daemon-attach-not-found",
    });

    expect(next.sessions["term-remote"].lifecycle).toBe("exited");
    expect(next.sessions["term-remote"].backendSessionId).toBe("backend-remote-1");
    expect(next.sessions["term-remote"].remoteConnectionState).toBe("disconnected");
    expect(next.sessions["term-remote"].remoteGeneration).toBeNull();
    expect(next.sessions["term-remote"].remoteFailure).toEqual({
      kind: "network",
      message: "daemon-attach-not-found",
    });
  });

  it("sets active activity to done when local session exits", () => {
    const session: TerminalSession = {
      id: "term-activity",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-act-1",
      lifecycle: "working",
    };

    const state: WorkspaceState = {
      ...createInitialState(session),
      activityBySessionId: {
        "term-activity": {
          state: "working",
          title: "Running build",
          isAgent: false,
        },
      },
    };

    const next = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-activity",
      backendSessionId: "backend-act-1",
      reason: "daemon-attach-not-found",
    });

    expect(next.sessions["term-activity"].lifecycle).toBe("exited");
    expect(next.activityBySessionId?.["term-activity"]?.state).toBe("done");
  });

  it("is a no-op when bindingKey does not match the current session binding epoch", () => {
    const session: TerminalSession = {
      id: "term-1",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-1",
      daemonEpoch: "2",
      remoteGeneration: 0,
      lifecycle: "working",
    };

    const state = createInitialState(session);
    const next = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-1",
      backendSessionId: "backend-1",
      bindingKey: "backend-1:1:0:",
      reason: "daemon-attach-not-found",
    });

    expect(next).toBe(state);
    expect(next.sessions["term-1"].lifecycle).toBe("working");
    expect(next.sessions["term-1"].backendSessionId).toBe("backend-1");
  });
});
