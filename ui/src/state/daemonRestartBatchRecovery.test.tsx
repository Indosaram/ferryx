import { renderHook, act } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useWorkspaceStore, type WorkspaceServices, type WorkspaceState } from "./workspaceStore";
import type { TerminalSession } from "../lib/types";

describe("Daemon restart batch recovery & fresh shell fallback", () => {
  it("spawns fresh shells and clears dead agent state when fallbackToShell is true", async () => {
    const spawnTerminal = vi.fn(async ({ cwd }: { cwd?: string }) => `backend-spawned-${cwd ?? "root"}`);

    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => {}),
      spawnTerminal: spawnTerminal as any,
      getTerminalCwd: vi.fn(async () => "/repo/test"),
      closeTerminal: vi.fn(async () => {}),
      waitForTerminalExit: vi.fn(async () => {}),
    };

    const initialWorktrees = [
      { path: "/repo/main", branch: "main", head: "111", bare: false, detached: false, locked: null, prunable: null },
      { path: "/repo/wt-1", branch: "orca/test/wt-1", head: "222", bare: false, detached: false, locked: null, prunable: null },
    ];

    const { result } = renderHook(() =>
      useWorkspaceStore({
        workspaceId: "test-ws",
        initialWorktrees,
        services,
      }),
    );

    const shellSession: TerminalSession = {
      id: "shell-sess",
      backendSessionId: null,
      lifecycle: "exited",
      worktreePath: "/repo/main",
      cwd: "/repo/main",
      workspaceId: "test-ws",
      worktree: null,
    };

    const agentSession: TerminalSession = {
      id: "agent-sess",
      backendSessionId: null,
      lifecycle: "exited",
      worktreePath: "/repo/wt-1",
      cwd: "/repo/wt-1",
      workspaceId: "test-ws",
      worktree: null,
      agentType: "unsupported-agent",
      agentSessionId: "legacy-agent-id",
      providerSession: { key: "session_id", id: "uuid-old" },
    };

    const initialRestoredState: WorkspaceState = {
      workspaceId: "test-ws",
      worktrees: initialWorktrees,
      activeWorktreePath: "/repo/main",
      layout: {
        tabs: [{ id: "tab-1", sessionId: "shell-sess", label: "Shell", kind: "terminal" }],
        activeTabId: "tab-1",
        layoutsByTabId: {},
      },
      worktreeLayouts: {
        "/repo/wt-1": {
          tabs: [{ id: "tab-2", sessionId: "agent-sess", label: "Agent", kind: "terminal" }],
          activeTabId: "tab-2",
          layoutsByTabId: {},
        },
      },
      sessions: {
        "shell-sess": shellSession,
        "agent-sess": agentSession,
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {
        "agent-sess": { state: "working", seen: false, title: "agent", isAgent: true },
      },
    };

    act(() => {
      result.current.restoreWorkspace(initialRestoredState);
    });

    expect(result.current.state.sessions["agent-sess"].lifecycle).toBe("exited");
    expect(result.current.state.sessions["shell-sess"].lifecycle).toBe("exited");

    await act(async () => {
      await result.current.ensureSessionBackends(["shell-sess", "agent-sess"], { fallbackToShell: true });
    });

    const finalSessions = result.current.state.sessions;

    expect(finalSessions["shell-sess"].backendSessionId).toBe("backend-spawned-/repo/main");
    expect(finalSessions["shell-sess"].lifecycle).toBe("running");

    expect(finalSessions["agent-sess"].backendSessionId).toBe("backend-spawned-/repo/wt-1");
    expect(finalSessions["agent-sess"].lifecycle).toBe("running");
    expect(finalSessions["agent-sess"].agentType).toBeNull();
    expect(finalSessions["agent-sess"].agentSessionId).toBeNull();
    expect(finalSessions["agent-sess"].providerSession).toBeNull();

    expect(result.current.state.activityBySessionId?.["agent-sess"]).toBeUndefined();
  });

  it("REBIND_SESSION_BACKEND with clearAgent purges agent metadata and activity", () => {
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId: "ws" }));

    const sessionWithAgent: TerminalSession = {
      id: "sess-agent",
      backendSessionId: null,
      lifecycle: "exited",
      workspaceId: "ws",
      worktree: null,
      cwd: "/repo",
      agentType: "claude",
      agentSessionId: "123",
      providerSession: { key: "session_id", id: "uuid-123" },
    };

    act(() => {
      result.current.restoreWorkspace({
        workspaceId: "ws",
        worktrees: [],
        activeWorktreePath: null,
        layout: { tabs: [], activeTabId: null, layoutsByTabId: {} },
        sessions: { "sess-agent": sessionWithAgent },
        unreadTabIds: {},
        unreadWorktreePaths: {},
        activityBySessionId: {
          "sess-agent": { state: "working", seen: false, title: "agent", isAgent: true },
        },
      });
    });

    act(() => {
      result.current.dispatchWorkspaceAction({
        type: "REBIND_SESSION_BACKEND",
        sessionId: "sess-agent",
        backendSessionId: "fresh-backend-id",
        clearAgent: true,
      });
    });

    const updated = result.current.state.sessions["sess-agent"];
    expect(updated.backendSessionId).toBe("fresh-backend-id");
    expect(updated.lifecycle).toBe("running");
    expect(updated.agentType).toBeNull();
    expect(updated.agentSessionId).toBeNull();
    expect(updated.providerSession).toBeNull();
    expect(result.current.state.activityBySessionId?.["sess-agent"]).toBeUndefined();
  });

  it("ensureSessionBackends automatically recovers unresumable agents as shells while preserving resumable agents", async () => {
    const spawnTerminal = vi.fn(async ({ cwd }: { cwd?: string }) => `backend-spawned-${cwd ?? "root"}`);

    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => {}),
      spawnTerminal: spawnTerminal as any,
      getTerminalCwd: vi.fn(async () => "/repo/test"),
      closeTerminal: vi.fn(async () => {}),
      waitForTerminalExit: vi.fn(async () => {}),
    };

    const { result } = renderHook(() =>
      useWorkspaceStore({
        workspaceId: "ws-test",
        services,
      }),
    );

    const resumableSession: TerminalSession = {
      id: "sess-resumable",
      backendSessionId: null,
      lifecycle: "exited",
      workspaceId: "ws-test",
      worktree: null,
      cwd: "/repo/resumable",
      agentType: "claude",
      agentSessionId: "claude-session-id",
      providerSession: { key: "session_id", id: "uuid-claude" },
    };

    const unresumableSession: TerminalSession = {
      id: "sess-unresumable",
      backendSessionId: null,
      lifecycle: "exited",
      workspaceId: "ws-test",
      worktree: null,
      cwd: "/repo/unresumable",
      agentType: "unknown-agent",
      agentSessionId: "unknown-id",
    };

    act(() => {
      result.current.restoreWorkspace({
        workspaceId: "ws-test",
        worktrees: [],
        activeWorktreePath: null,
        layout: { tabs: [], activeTabId: null, layoutsByTabId: {} },
        sessions: {
          "sess-resumable": resumableSession,
          "sess-unresumable": unresumableSession,
        },
        unreadTabIds: {},
        unreadWorktreePaths: {},
      });
    });

    await act(async () => {
      await result.current.ensureSessionBackends(["sess-resumable", "sess-unresumable"]);
    });

    const sessions = result.current.state.sessions;

    expect(sessions["sess-resumable"].backendSessionId).toBeNull();
    expect(sessions["sess-resumable"].lifecycle).toBe("exited");
    expect(sessions["sess-resumable"].agentType).toBe("claude");

    expect(sessions["sess-unresumable"].backendSessionId).toBe("backend-spawned-/repo/unresumable");
    expect(sessions["sess-unresumable"].lifecycle).toBe("running");
    expect(sessions["sess-unresumable"].agentType).toBeNull();
  });
});
