import { describe, expect, it } from "vitest";

import type { TerminalSession } from "./types";
import {
  collectResumableAgentPanes,
  getAgentReconnectAffordance,
  resumableAgentPane,
  type AgentReconnectAffordance,
  type ResumableAgentPane,
} from "./agentResumeAffordance";
import { workspaceReducer, type WorkspaceState } from "../state/workspaceStore";
import { createLayoutState } from "../state/layout";

function createSession(overrides: Partial<TerminalSession> = {}): TerminalSession {
  return {
    id: "session-1",
    cwd: "/repo/workspace",
    worktreePath: "/repo/workspace",
    workspaceId: "ws-1",
    worktree: null,
    backendSessionId: null,
    lifecycle: "exited",
    agentType: "claude",
    agentSessionId: "claude-uuid-123",
    providerSession: { key: "session_id", id: "claude-uuid-123" },
    reconnectLifecycle: "idle",
    reconnectError: null,
    ...overrides,
  };
}

describe("agentResumeAffordance", () => {
describe("getAgentReconnectAffordance", () => {
  it("does not advertise reconnect for argv-only agents without authoritative capture", () => {
    const session = createSession({
      agentType: "gemini",
      providerSession: { key: "session_id", id: "gemini-session" },
    });
    expect(getAgentReconnectAffordance(session)).toMatchObject({
      status: "unsupported",
      canReconnect: false,
      reason: expect.stringContaining("authoritative session capture"),
    });
  });
    it("returns enabled Reconnect and exact local session target for an exited Claude session", () => {
      const session = createSession({
        id: "pane-term-1",
        cwd: "/repo/workspace/sub",
        lifecycle: "exited",
        backendSessionId: null,
        agentType: "claude",
        providerSession: { key: "session_id", id: "claude-session-abc" },
      });

      expect(getAgentReconnectAffordance(session)).toEqual({
        status: "idle",
        canReconnect: true,
        canRetry: false,
        isReconnecting: false,
        sessionId: "pane-term-1",
        agentType: "claude",
        providerSession: { key: "session_id", id: "claude-session-abc" },
        reconnectLifecycle: "idle",
        error: null,
        reason: null,
        conflictingSessionId: null,
        argv: ["claude", "--resume", "claude-session-abc"],
        cwd: "/repo/workspace/sub",
      } satisfies AgentReconnectAffordance);
    });

    it("returns disabled action and conflict copy when same provider reference is active elsewhere", () => {
      const exited = createSession({ id: "sess-exited", providerSession: { key: "session_id", id: "shared-id" } });
      const live = createSession({
        id: "sess-live",
        lifecycle: "working",
        backendSessionId: "pty-1",
        providerSession: { key: "session_id", id: "shared-id" },
      });

      const affordance = getAgentReconnectAffordance(exited, [exited, live]);
      expect(affordance.status).toBe("conflict");
      expect(affordance.canReconnect).toBe(false);
      expect(affordance.conflictingSessionId).toBe("sess-live");
      expect(affordance.reason).toContain("already active in another pane");
    });

    it("returns conflict when another session is in-flight reconnecting for the same provider reference", () => {
      const sA = createSession({ id: "sess-a", agentType: "omo", providerSession: { key: "session_id", id: "omo-1" } });
      const sB = createSession({ id: "sess-b", reconnectLifecycle: "spawning", agentType: "omo", providerSession: { key: "session_id", id: "omo-1" } });

      const affordance = getAgentReconnectAffordance(sA, { [sA.id]: sA, [sB.id]: sB });
      expect(affordance.status).toBe("conflict");
      expect(affordance.canReconnect).toBe(false);
      expect(affordance.conflictingSessionId).toBe("sess-b");
    });

    it("returns reconnecting status with disabled action during in-flight lifecycle phases", () => {
      for (const phase of ["validating", "spawning", "binding"] as const) {
        const session = createSession({ reconnectLifecycle: phase });
        const affordance = getAgentReconnectAffordance(session);
        expect(affordance.status).toBe("reconnecting");
        expect(affordance.isReconnecting).toBe(true);
        expect(affordance.canReconnect).toBe(false);
        expect(affordance.reconnectLifecycle).toBe(phase);
      }
    });

    it("returns failed status with retryable state and structured error", () => {
      const session = createSession({
        reconnectLifecycle: "failed",
        reconnectError: { code: "SPAWN_FAILED", message: "Daemon spawn failed" },
      });

      const affordance = getAgentReconnectAffordance(session);
      expect(affordance.status).toBe("failed");
      expect(affordance.canReconnect).toBe(true);
      expect(affordance.canRetry).toBe(true);
      expect(affordance.isReconnecting).toBe(false);
      expect(affordance.error).toEqual({ code: "SPAWN_FAILED", message: "Daemon spawn failed" });
      expect(affordance.reason).toBe("Daemon spawn failed");
    });

    it("returns missing_reference for missing, empty, or invalid provider references", () => {
      const missing = createSession({ agentSessionId: null, providerSession: null });
      const empty = createSession({ agentSessionId: "", providerSession: null });
      const invalid = createSession({ agentSessionId: "--flag", providerSession: null });

      expect(getAgentReconnectAffordance(missing).status).toBe("missing_reference");
      expect(getAgentReconnectAffordance(empty).status).toBe("missing_reference");
      expect(getAgentReconnectAffordance(invalid).status).toBe("missing_reference");
    });

    it("returns unsupported for agents that do not support session resume", () => {
      const session = createSession({ agentType: "unknown-custom-bot" });
      const affordance = getAgentReconnectAffordance(session);
      expect(affordance.status).toBe("unsupported");
      expect(affordance.canReconnect).toBe(false);
      expect(affordance.reason).toContain("does not support session resume");
    });

    it("returns none for plain terminal sessions without agentType or live sessions", () => {
      const plain = createSession({ agentType: null, agentSessionId: null, providerSession: null });
      const live = createSession({ lifecycle: "working", backendSessionId: "pty-1" });
      expect(getAgentReconnectAffordance(plain).status).toBe("none");
      expect(getAgentReconnectAffordance(live).status).toBe("none");
    });
  });

  describe("resumableAgentPane", () => {
    it("yields ResumableAgentPane with normalized providerSession and argv", () => {
      const session = createSession({
        id: "pane-term-1",
        cwd: "/repo/workspace/sub",
        lifecycle: "exited",
        backendSessionId: null,
        agentType: "claude",
        providerSession: { key: "session_id", id: "claude-session-abc" },
      });

      expect(resumableAgentPane(session)).toEqual({
        sessionId: "pane-term-1",
        agentType: "claude",
        agentSessionId: "claude-session-abc",
        providerSession: { key: "session_id", id: "claude-session-abc" },
        cwd: "/repo/workspace/sub",
        argv: ["claude", "--resume", "claude-session-abc"],
      } satisfies ResumableAgentPane);
    });

    it("yields null when session is live or in-flight reconnecting", () => {
      expect(resumableAgentPane(createSession({ lifecycle: "working", backendSessionId: "pty-9" }))).toBeNull();
      expect(resumableAgentPane(createSession({ reconnectLifecycle: "spawning" }))).toBeNull();
    });

    it("yields null when a duplicate active claim exists in allSessions", () => {
      const exited = createSession({ id: "sess-1", providerSession: { key: "session_id", id: "shared-1" } });
      const live = createSession({ id: "sess-2", lifecycle: "working", backendSessionId: "pty-2", providerSession: { key: "session_id", id: "shared-1" } });
      expect(resumableAgentPane(exited, [exited, live])).toBeNull();
    });

    it("collects resumable panes and skips conflicts", () => {
      const sessions: Record<string, TerminalSession> = {
        "sess-exited": createSession({ id: "sess-exited", providerSession: { key: "session_id", id: "claude-1" } }),
        "sess-conflict-exited": createSession({ id: "sess-conflict-exited", providerSession: { key: "session_id", id: "shared" } }),
        "sess-conflict-live": createSession({ id: "sess-conflict-live", lifecycle: "working", backendSessionId: "pty-1", providerSession: { key: "session_id", id: "shared" } }),
        "sess-plain": createSession({ id: "sess-plain", agentType: null, providerSession: null }),
      };

      const results = collectResumableAgentPanes(sessions);
      expect(results).toHaveLength(1);
      expect(results[0]?.sessionId).toBe("sess-exited");
    });
  });

  describe("workspaceReducer reconnect actions", () => {
    function createInitialWorkspace(session: TerminalSession): WorkspaceState {
      return { worktrees: [], activeWorktreePath: null, sessions: { [session.id]: session }, layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {} };
    }

    it("SET_RECONNECT_LIFECYCLE updates transient state while keeping exited lifecycle and null backend", () => {
      const initial = createSession({ id: "sess-rec" });
      const state = createInitialWorkspace(initial);

      const spawningState = workspaceReducer(state, { type: "SET_RECONNECT_LIFECYCLE", sessionId: "sess-rec", lifecycle: "spawning" });
      expect(spawningState.sessions["sess-rec"]?.reconnectLifecycle).toBe("spawning");
      expect(spawningState.sessions["sess-rec"]?.lifecycle).toBe("exited");
      expect(spawningState.sessions["sess-rec"]?.backendSessionId).toBeNull();

      const failedState = workspaceReducer(spawningState, {
        type: "SET_RECONNECT_LIFECYCLE",
        sessionId: "sess-rec",
        lifecycle: "failed",
        error: { code: "AGENT_SESSION_CONFLICT", message: "Session claimed elsewhere" },
      });
      expect(failedState.sessions["sess-rec"]?.reconnectLifecycle).toBe("failed");
      expect(failedState.sessions["sess-rec"]?.reconnectError).toEqual({ code: "AGENT_SESSION_CONFLICT", message: "Session claimed elsewhere" });
      expect(failedState.sessions["sess-rec"]?.lifecycle).toBe("exited");
    });

    it("REBIND_SESSION_BACKEND clears reconnectLifecycle and error on successful rebind", () => {
      const initial = createSession({ id: "sess-rebind", reconnectLifecycle: "binding", reconnectError: { code: "ERR", message: "err" } });
      const reboundState = workspaceReducer(createInitialWorkspace(initial), {
        type: "REBIND_SESSION_BACKEND",
        sessionId: "sess-rebind",
        backendSessionId: "pty-new-123",
        cwd: "/repo/workspace/rebound",
      });

      const session = reboundState.sessions["sess-rebind"];
      expect(session?.backendSessionId).toBe("pty-new-123");
      expect(session?.lifecycle).toBe("running");
      expect(session?.reconnectLifecycle).toBe("idle");
      expect(session?.reconnectError).toBeNull();
      expect(session?.cwd).toBe("/repo/workspace/rebound");
    });
  });
});
