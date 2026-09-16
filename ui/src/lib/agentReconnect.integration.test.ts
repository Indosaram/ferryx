import { describe, expect, it, vi } from "vitest";
import { reconnectAgentSession, clearAgentReconnectInflightForTests } from "./agentReconnect";
import { scheduleAgentAutoResume, resetAgentAutoResumeGuard } from "./agentAutoResume";
import { saveGeneralSettings, resetGeneralSettings } from "./generalSettings";
import { workspaceReducer } from "../state/workspaceStore";
import { deserializeWorkspaceState, serializeWorkspaceState } from "./sessionPersistence";
import { createAppReconnectDependencies } from "./appReconnectDependencies";
import { describeTerminal, closeTerminal, type TerminalDescribeResult } from "./tauri";
import type { TerminalSession } from "./types";
import type { PersistedWorkspaceSession } from "./types";
import type { WorkspaceState } from "../state/workspaceStore";

vi.mock("./tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./tauri")>();
  return {
    ...actual,
    describeTerminal: vi.fn(),
    closeTerminal: vi.fn(async () => undefined),
  };
});

function coldAgent(): TerminalSession {
  return {
    id: "local-agent", workspaceId: "ws", worktree: null, cwd: "/repo",
    backendSessionId: null, lifecycle: "exited", agentType: "claude",
    providerSession: { key: "session_id", id: "provider-1" }, reconnectLifecycle: "idle",
    daemonEpoch: "old", lastOutputSequence: "900",
  };
}

describe("agent reconnect cross-layer contracts", () => {
  it("persists recovered OMO CWD and transcript without changing the workspace root", async () => {
    const providerSession = {
      key: "session_id",
      id: "omo-provider",
      transcriptPath: "/home/user/.omo/agent/sessions/project/session.jsonl",
    } as const;
    const persisted: PersistedWorkspaceSession = {
      version: 2, timestamp: 1, activeWorkspaceId: "ws",
      workspaces: {
        ws: {
          workspaceId: "ws", repoRoot: "/repo", worktrees: [], activeWorktreePath: "/repo",
          layout: {
            splitMode: "none", primaryTabId: "tab", secondaryTabId: null, activeTabId: "tab",
            tabs: [{
              id: "tab", kind: "terminal", label: "OMO",
              terminal: {
                primarySessionId: "local-agent", paneTree: { type: "leaf", leafId: "leaf" },
                sessionIdsByLeafId: { leaf: "local-agent" }, activeLeafId: "leaf", expandedLeafId: null,
              },
            }],
          },
          terminalSessions: {
            "local-agent": {
              localSessionId: "local-agent", backendSessionId: null, cwd: "/repo",
              worktreePath: "/repo", createdAt: 1, agentType: "omo", providerSession,
            },
          },
        },
      },
    };
    const restored = deserializeWorkspaceState("ws", persisted, []);
    if (!restored) throw new Error("Missing restored workspace");
    let state = restored;
    const spawn = vi.fn(async () => ({
      sessionId: "new-backend", daemonEpoch: "new",
      session: {
        sessionId: "new-backend", workspaceId: "ws", worktree: null,
        cwd: "/repo/nested", cols: 80, rows: 24, running: true,
      },
    }));
    await reconnectAgentSession("local-agent", {
      getSessions: () => state.sessions,
      dispatch: (action) => { state = workspaceReducer(state, action); },
      spawn, attach: async () => undefined, createRequestId: () => "omo-cwd-repair",
    });
    expect(spawn).toHaveBeenCalledWith(expect.objectContaining({
      startup: { kind: "agentResume", agentType: "omo", providerSession },
    }));
    const saved = serializeWorkspaceState("ws", "/repo", state, persisted);
    expect(saved.workspaces.ws?.terminalSessions["local-agent"]).toMatchObject({
      cwd: "/repo/nested", worktreePath: "/repo", providerSession,
    });
    expect(saved.workspaces.ws?.repoRoot).toBe("/repo");
  });

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

  it("auto-resumes deserialized exited agent sessions through scheduleAgentAutoResume in Eager mode", async () => {
    vi.useFakeTimers();
    saveGeneralSettings({ sessionRestorePolicy: "eager" });
    resetAgentAutoResumeGuard();
    clearAgentReconnectInflightForTests();

    const persisted: PersistedWorkspaceSession = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "ws-auto",
      workspaces: {
        "ws-auto": {
          workspaceId: "ws-auto",
          repoRoot: "/repo",
          worktrees: [{ path: "/repo", branch: "main", head: "123", isMain: true, isLocked: false }],
          activeWorktreePath: "/repo",
          layout: {
            splitMode: "none", primaryTabId: "tab-1", secondaryTabId: null, activeTabId: "tab-1",
            tabs: [
              { id: "tab-1", kind: "terminal", label: "Agent 1", terminal: { primarySessionId: "agent-1", paneTree: { type: "leaf", leafId: "leaf-1" }, sessionIdsByLeafId: { "leaf-1": "agent-1" }, activeLeafId: "leaf-1", expandedLeafId: null } },
              { id: "tab-2", kind: "terminal", label: "Agent 2", terminal: { primarySessionId: "agent-2", paneTree: { type: "leaf", leafId: "leaf-2" }, sessionIdsByLeafId: { "leaf-2": "agent-2" }, activeLeafId: "leaf-2", expandedLeafId: null } },
              { id: "tab-3", kind: "terminal", label: "Shell", terminal: { primarySessionId: "shell-1", paneTree: { type: "leaf", leafId: "leaf-3" }, sessionIdsByLeafId: { "leaf-3": "shell-1" }, activeLeafId: "leaf-3", expandedLeafId: null } },
            ],
          },
          terminalSessions: {
            "agent-1": { localSessionId: "agent-1", backendSessionId: null, cwd: "/repo", worktreePath: "/repo", createdAt: 1, agentType: "claude", providerSession: { key: "session_id", id: "uuid-1" } },
            "agent-2": { localSessionId: "agent-2", backendSessionId: null, cwd: "/repo", worktreePath: "/repo", createdAt: 2, agentType: "claude", providerSession: { key: "session_id", id: "uuid-2" } },
            "shell-1": { localSessionId: "shell-1", backendSessionId: null, cwd: "/repo", worktreePath: "/repo", createdAt: 3 },
          },
        },
      },
    };

    const restored = deserializeWorkspaceState("ws-auto", persisted, []);
    expect(restored).not.toBeNull();
    if (!restored) return;

    let state = restored;
    const spawn = vi.fn(async ({ startup }) => ({
      sessionId: `backend-${startup.providerSession.id}`,
      daemonEpoch: "epoch-new",
      session: { sessionId: `backend-${startup.providerSession.id}`, workspaceId: "ws-auto", worktree: null, cwd: "/repo", cols: 80, rows: 24, running: true },
    }));

    const dispatch = (action: any) => {
      state = workspaceReducer(state, action);
    };

    scheduleAgentAutoResume({
      workspaceId: "ws-auto",
      state: restored,
      recoveredFromHmr: false,
      reconnect: (sessionId) => reconnectAgentSession(sessionId, {
        getSessions: () => state.sessions,
        dispatch,
        spawn,
        attach: vi.fn(async () => undefined),
      }),
    });

    await vi.advanceTimersByTimeAsync(0);
    expect(spawn).toHaveBeenCalledTimes(1);
    expect(spawn).toHaveBeenCalledWith(expect.objectContaining({ startup: expect.objectContaining({ providerSession: { key: "session_id", id: "uuid-1" } }) }));
    await vi.advanceTimersByTimeAsync(400);
    expect(spawn).toHaveBeenCalledTimes(2);
    expect(spawn).toHaveBeenLastCalledWith(expect.objectContaining({ startup: expect.objectContaining({ providerSession: { key: "session_id", id: "uuid-2" } }) }));
    await vi.advanceTimersByTimeAsync(2000);
    expect(spawn).toHaveBeenCalledTimes(2);
    expect(state.sessions["agent-1"].backendSessionId).toBe("backend-uuid-1");
    expect(state.sessions["agent-2"].backendSessionId).toBe("backend-uuid-2");
    expect(state.sessions["shell-1"].backendSessionId).toBeNull();

    resetGeneralSettings();
    vi.useRealTimers();
  });

  it("adopts live existing daemon session on AGENT_SESSION_CONFLICT without calling closeTerminal", async () => {
    const session = coldAgent();
    let state: WorkspaceState = {
      sessions: { [session.id]: session },
    } as any;

    const existingBackendId = "existing-daemon-session-999";
    const conflictError = {
      code: "AGENT_SESSION_CONFLICT",
      message: "Agent provider session is already owned by another terminal",
      details: {
        agentType: "claude",
        providerKey: "session_id",
        providerId: "provider-1",
        existingSessionId: existingBackendId,
      },
    };

    const spawn = vi.fn(async () => {
      throw conflictError;
    });
    const dispatch = vi.fn((action: any) => {
      state = workspaceReducer(state, action);
    });

    const describedSession: TerminalDescribeResult = {
      sessionId: existingBackendId,
      workspaceId: session.workspaceId,
      worktree: null,
      cwd: "/repo/adopted",
      cols: 120,
      rows: 40,
      running: true,
    };

    vi.mocked(describeTerminal).mockResolvedValue(describedSession);
    vi.mocked(closeTerminal).mockClear();

    const deps = createAppReconnectDependencies({
      getSessions: () => state.sessions,
      dispatch,
      spawn,
    });

    const attachSpy = vi.fn(deps.attach);
    deps.attach = attachSpy;

    const result = await reconnectAgentSession(session.id, deps);

    expect(result.sessionId).toBe(existingBackendId);
    expect(attachSpy).toHaveBeenCalledTimes(1);
    expect(attachSpy).toHaveBeenCalledWith(
      expect.objectContaining({ sessionId: existingBackendId }),
      expect.objectContaining({ id: session.id }),
    );
    expect(closeTerminal).not.toHaveBeenCalled();
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "REBIND_SESSION_BACKEND",
        sessionId: session.id,
        backendSessionId: existingBackendId,
      }),
    );
    expect(state.sessions[session.id].backendSessionId).toBe(existingBackendId);
    expect(state.sessions[session.id].lifecycle).toBe("running");
  });
});
