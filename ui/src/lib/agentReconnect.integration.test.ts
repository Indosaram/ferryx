import { describe, expect, it, vi } from "vitest";
import { reconnectAgentSession, clearAgentReconnectInflightForTests } from "./agentReconnect";
import { workspaceReducer } from "../state/workspaceStore";
import { deserializeWorkspaceState, serializeWorkspaceState } from "./sessionPersistence";
import type { TerminalSession } from "./types";
import type { PersistedWorkspaceSession } from "./types";
import type { WorkspaceState } from "../state/workspaceStore";

function coldAgent(): TerminalSession {
  return {
    id: "local-agent", workspaceId: "ws", worktree: null, cwd: "/repo",
    backendSessionId: null, lifecycle: "exited", agentType: "claude",
    providerSession: { key: "session_id", id: "provider-1" }, reconnectLifecycle: "idle",
    daemonEpoch: "old", lastOutputSequence: "900",
  };
}

describe("agent reconnect cross-layer contracts", () => {
  it("spans stale persisted load through typed reconnect and saved snapshot", async () => {
    const persisted = {
      version: 2, timestamp: 1, activeWorkspaceId: "ws",
      workspaces: {
        ws: {
          workspaceId: "ws", repoRoot: "/repo", worktrees: [], activeWorktreePath: null,
          layout: { splitMode: "none", primaryTabId: "tab", secondaryTabId: null, activeTabId: "tab", tabs: [{ id: "tab", kind: "terminal", label: "Claude", terminal: { primarySessionId: "local-agent", paneTree: { type: "leaf", leafId: "leaf" }, sessionIdsByLeafId: { leaf: "local-agent" }, activeLeafId: "leaf", expandedLeafId: null } }] },
          terminalSessions: { "local-agent": { localSessionId: "local-agent", backendSessionId: "backend-old", cwd: "/repo", worktreePath: "/repo", createdAt: 1, agentType: "claude", providerSession: { key: "session_id", id: "provider-1" }, daemonEpoch: "old", lastOutputSequence: "900" } },
        },
      },
    } as PersistedWorkspaceSession;
    const restored = deserializeWorkspaceState("ws", persisted, { epoch: "new-daemon", sessions: [] });
    expect(restored).not.toBeNull();
    if (!restored) return;
    expect(restored.sessions["local-agent"]).toMatchObject({ backendSessionId: null, lifecycle: "exited", providerSession: { key: "session_id", id: "provider-1" } });

    let state = restored;
    const spawn = vi.fn(async () => ({ sessionId: "backend-new", daemonEpoch: "new-daemon", session: { sessionId: "backend-new", workspaceId: "ws", worktree: null, cwd: "/repo", cols: 80, rows: 24, running: true } }));
    const persist = vi.fn(async (result, localSession) => {
      const proposed = workspaceReducer(state, { type: "REBIND_SESSION_BACKEND", sessionId: localSession.id, backendSessionId: result.sessionId, cwd: result.session.cwd ?? localSession.cwd, daemonEpoch: result.daemonEpoch });
      const saved = serializeWorkspaceState("ws", "/repo", proposed, persisted);
      const savedSession = saved.workspaces.ws?.terminalSessions["local-agent"];
      expect(savedSession).toMatchObject({ localSessionId: "local-agent", backendSessionId: "backend-new", providerSession: { key: "session_id", id: "provider-1" }, daemonEpoch: "new-daemon", lastOutputSequence: null });
      expect(JSON.stringify(savedSession)).not.toContain("reconnectRequestId");
    });
    await reconnectAgentSession("local-agent", { getSessions: () => state.sessions, dispatch: (action) => { state = workspaceReducer(state, action); }, spawn, attach: vi.fn(async () => undefined), persist, createRequestId: () => "stable-request" });

    expect(spawn).toHaveBeenCalledWith(expect.objectContaining({ clientRequestId: "stable-request", startup: { kind: "agentResume", agentType: "claude", providerSession: { key: "session_id", id: "provider-1" } } }));
    expect(persist).toHaveBeenCalledOnce();
    expect(state.sessions["local-agent"]).toMatchObject({ id: "local-agent", backendSessionId: "backend-new", providerSession: { key: "session_id", id: "provider-1" }, daemonEpoch: "new-daemon", lastOutputSequence: null });
  });

  it("loads a legacy agentSessionId and completes reconnect with the canonical provider identity", async () => {
    const restored = deserializeWorkspaceState("ws", {
      version: 2,
      timestamp: 1,
      activeWorkspaceId: "ws",
      workspaces: {
        ws: {
          workspaceId: "ws", repoRoot: "/repo", worktrees: [], activeWorktreePath: null,
          layout: { splitMode: "none", primaryTabId: "tab", secondaryTabId: null, activeTabId: "tab", tabs: [{ id: "tab", kind: "terminal", label: "legacy", terminal: { primarySessionId: "local-agent", paneTree: { type: "leaf", leafId: "leaf" }, sessionIdsByLeafId: { leaf: "local-agent" }, activeLeafId: "leaf", expandedLeafId: null } }] },
          terminalSessions: { "local-agent": { localSessionId: "local-agent", backendSessionId: null, cwd: "/repo", worktreePath: "/repo", createdAt: 1, agentType: "claude", agentSessionId: "provider-1" } },
        },
      },
    });
    expect(restored).not.toBeNull();
    if (!restored) return;
    let state = restored;
    const spawn = vi.fn(async () => ({ sessionId: "backend-new", daemonEpoch: "new", session: { sessionId: "backend-new", workspaceId: "ws", worktree: null, cwd: "/repo", cols: 80, rows: 24, running: true } }));
    await reconnectAgentSession("local-agent", { getSessions: () => state.sessions, dispatch: (action) => { state = workspaceReducer(state, action); }, spawn, attach: vi.fn(async () => undefined), createRequestId: () => "legacy-request" });
    expect(spawn).toHaveBeenCalledWith(expect.objectContaining({ startup: { kind: "agentResume", agentType: "claude", providerSession: { key: "session_id", id: "provider-1" } } }));
    expect(state.sessions["local-agent"]).toMatchObject({ backendSessionId: "backend-new", providerSession: { key: "session_id", id: "provider-1" } });
  });

  it("runs typed spawn, attach, proposed persistence, and same-local-id rebind", async () => {
    clearAgentReconnectInflightForTests();
    const session = coldAgent(); let state = { sessions: { [session.id]: session } } as unknown as WorkspaceState;
    const dispatch = vi.fn((action) => { state = workspaceReducer(state, action); });
    const attach = vi.fn(async () => undefined); const persist = vi.fn(async () => undefined);
    const spawn = vi.fn(async () => ({ sessionId: "backend-new", daemonEpoch: "new", session: { sessionId: "backend-new", workspaceId: "ws", worktree: null, cwd: "/repo", cols: 80, rows: 24, running: true } }));
    await reconnectAgentSession(session.id, { getSessions: () => state.sessions, dispatch, spawn, attach, persist, createRequestId: () => "stable-request" });
    expect(spawn).toHaveBeenCalledWith(expect.objectContaining({ clientRequestId: "stable-request", startup: { kind: "agentResume", agentType: "claude", providerSession: session.providerSession } }));
    expect(attach).toHaveBeenCalledOnce(); expect(persist).toHaveBeenCalledOnce();
    expect(state.sessions[session.id]).toMatchObject({ id: session.id, backendSessionId: "backend-new", daemonEpoch: "new", lastOutputSequence: null, providerSession: session.providerSession });
  });

  it("keeps cold identity and structured failure when attach rejects", async () => {
    clearAgentReconnectInflightForTests();
    const session = coldAgent(); let state = { sessions: { [session.id]: session } } as unknown as WorkspaceState;
    const close = vi.fn(async () => undefined);
    await expect(reconnectAgentSession(session.id, {
      getSessions: () => state.sessions,
      dispatch: (action) => { state = workspaceReducer(state, action); },
      spawn: vi.fn(async () => ({ sessionId: "backend-doomed", daemonEpoch: "new", session: { sessionId: "backend-doomed", workspaceId: "ws", worktree: null, cwd: "/repo", cols: 80, rows: 24, running: true } })),
      attach: vi.fn(async () => { throw { code: "AGENT_SESSION_CONFLICT", message: "conflict", details: { existingSessionId: "other" } }; }), close,
    })).rejects.toMatchObject({ code: "AGENT_SESSION_CONFLICT" });
    expect(close).toHaveBeenCalledWith("backend-doomed");
    expect(state.sessions[session.id]).toMatchObject({ backendSessionId: null, lifecycle: "exited", providerSession: session.providerSession, reconnectLifecycle: "failed" });
  });
});
