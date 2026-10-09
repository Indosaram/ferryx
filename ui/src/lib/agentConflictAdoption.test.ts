import { describe, expect, it, vi } from "vitest";
import {
  extractConflictingBackendSessionId,
  withAgentConflictAdoption,
} from "./agentConflictAdoption";
import type { AgentReconnectDependencies } from "./agentReconnect";
import { reconnectAgentSession } from "./agentReconnect";
import type { TerminalSession } from "./types";
import type { TerminalDescribeResult } from "./tauri";

function mockSession(overrides: Partial<TerminalSession> = {}): TerminalSession {
  return {
    id: "session-1",
    workspaceId: "ws-1",
    worktree: null,
    cwd: "/repo",
    backendSessionId: null,
    lifecycle: "exited",
    agentType: "claude",
    providerSession: { key: "session_id", id: "provider-1" },
    reconnectLifecycle: "idle",
    ...overrides,
  };
}

describe("agentConflictAdoption", () => {
  it.each(["attach", "persist"] as const)("preserves the adopted live session when %s fails", async (stage) => {
    // Given: the daemon already owns the requested conversation.
    const session = mockSession({ id: `adopt-failure-${stage}` });
    const liveSessions = new Set(["existing-backend"]);
    const failure = { code: "IO_ERROR", message: "Recovery transport failed" };
    const dependencies = withAgentConflictAdoption({
      getSessions: () => ({ [session.id]: session }),
      dispatch: vi.fn(),
      spawn: async () => {
        throw { code: "AGENT_SESSION_CONFLICT", message: "Already running", details: { existingSessionId: "existing-backend" } };
      },
      attach: async () => { if (stage === "attach") throw failure; },
      persist: async () => { if (stage === "persist") throw failure; },
      close: async (id) => { liveSessions.delete(id); },
    }, {
      describe: async () => ({ sessionId: "existing-backend", workspaceId: "ws-1", worktree: null, cwd: "/repo", cols: 80, rows: 24, running: true }),
    });

    // When: recovery fails after adopting the pre-existing session.
    await expect(reconnectAgentSession(session.id, dependencies)).rejects.toMatchObject(failure);

    // Then: the conversation remains alive for a subsequent reconnect.
    expect([...liveSessions]).toEqual(["existing-backend"]);
  });

  describe("extractConflictingBackendSessionId", () => {
    it("(i) extracts existingSessionId from structured conflict error", () => {
      const error = {
        code: "AGENT_SESSION_CONFLICT",
        message: "Agent provider session is already owned by another terminal",
        details: {
          agentType: "claude",
          providerKey: "session_id",
          providerId: "provider-1",
          existingSessionId: "backend-existing-123",
          command: "cmd_terminal_spawn",
        },
      };
      expect(extractConflictingBackendSessionId(error)).toBe("backend-existing-123");

      // Nested inside details.raw
      const errorWithRaw = {
        code: "AGENT_SESSION_CONFLICT",
        message: "Agent provider session is already owned by another terminal",
        details: {
          command: "cmd_terminal_spawn",
          raw: JSON.stringify({
            existingSessionId: "backend-from-raw",
          }),
        },
      };
      expect(extractConflictingBackendSessionId(errorWithRaw)).toBe("backend-from-raw");
    });

    it("(ii) returns null for other codes", () => {
      const nonConflictError = {
        code: "AGENT_RESUME_INVALID",
        message: "Resume configuration invalid",
        details: {
          existingSessionId: "backend-existing-123",
        },
      };
      expect(extractConflictingBackendSessionId(nonConflictError)).toBeNull();

      const genericError = new Error("Agent provider session is already owned by another terminal");
      expect(extractConflictingBackendSessionId(genericError)).toBeNull();
    });
  });

  describe("withAgentConflictAdoption", () => {
    const conflictError = {
      code: "AGENT_SESSION_CONFLICT",
      message: "Agent provider session is already owned by another terminal",
      details: {
        agentType: "claude",
        providerKey: "session_id",
        providerId: "provider-1",
        existingSessionId: "backend-live-456",
      },
    };

    const spawnRequest = {
      workspaceId: "ws-1",
      worktree: null,
      cwd: "/repo",
      clientRequestId: "req-1",
      startup: {
        kind: "agentResume" as const,
        agentType: "claude",
        providerSession: { key: "session_id" as const, id: "provider-1" },
      },
    };

    it("(iii) adoption returns adopted result mapping describe fields and the wrapped spawn resolves with sessionId === existingSessionId", async () => {
      const session = mockSession({ id: "session-1", reconnectRequestId: "req-1", daemonEpoch: "epoch-1" });
      const spawn = vi.fn(async () => {
        throw conflictError;
      });

      const described: TerminalDescribeResult = {
        sessionId: "backend-live-456",
        workspaceId: "ws-1",
        worktree: null,
        cwd: "/repo/sub",
        cols: 100,
        rows: 30,
        running: true,
        incarnation: "incarnation-live",
      };
      const describe = vi.fn(async () => described);

      const baseDeps: AgentReconnectDependencies = {
        getSessions: () => ({ [session.id]: session }),
        dispatch: vi.fn(),
        spawn,
        attach: vi.fn(async () => undefined),
      };

      const wrappedDeps = withAgentConflictAdoption(baseDeps, { describe });
      const result = await wrappedDeps.spawn!(spawnRequest);

      expect(result).toEqual({
        sessionId: "backend-live-456",
        daemonEpoch: "epoch-1",
        session: {
          sessionId: "backend-live-456",
          workspaceId: "ws-1",
          worktree: null,
          cwd: "/repo/sub",
          cols: 100,
          rows: 30,
          running: true,
          incarnation: "incarnation-live",
        },
      });
      expect(describe).toHaveBeenCalledWith("backend-live-456");
    });

    it("(iv) double-bind case rethrows friendly structured error and never calls describeTerminal", async () => {
      const session = mockSession({ id: "session-1" });
      const otherSession = mockSession({
        id: "session-2",
        backendSessionId: "backend-live-456",
      });
      const spawn = vi.fn(async () => {
        throw conflictError;
      });
      const describe = vi.fn(async () => null);

      const baseDeps: AgentReconnectDependencies = {
        getSessions: () => ({ [session.id]: session, [otherSession.id]: otherSession }),
        dispatch: vi.fn(),
        spawn,
        attach: vi.fn(async () => undefined),
      };

      const wrappedDeps = withAgentConflictAdoption(baseDeps, { describe });

      await expect(wrappedDeps.spawn!(spawnRequest)).rejects.toMatchObject({
        code: "AGENT_SESSION_CONFLICT",
        message: "This conversation is already open in another terminal — switch to that tab or close it to resume here.",
        details: conflictError.details,
      });

      expect(describe).not.toHaveBeenCalled();
    });

    it("(v) workspace mismatch rethrows original error", async () => {
      const session = mockSession({ id: "session-1", workspaceId: "ws-1" });
      const spawn = vi.fn(async () => {
        throw conflictError;
      });

      const described: TerminalDescribeResult = {
        sessionId: "backend-live-456",
        workspaceId: "ws-other",
        worktree: null,
        cwd: "/repo",
        cols: 80,
        rows: 24,
        running: true,
      };
      const describe = vi.fn(async () => described);

      const baseDeps: AgentReconnectDependencies = {
        getSessions: () => ({ [session.id]: session }),
        dispatch: vi.fn(),
        spawn,
        attach: vi.fn(async () => undefined),
      };

      const wrappedDeps = withAgentConflictAdoption(baseDeps, { describe });

      await expect(wrappedDeps.spawn!(spawnRequest)).rejects.toBe(conflictError);
    });

    it("(vi) describeTerminal throwing rethrows original error", async () => {
      const session = mockSession({ id: "session-1" });
      const spawn = vi.fn(async () => {
        throw conflictError;
      });
      const describe = vi.fn(async () => {
        throw new Error("daemon failure");
      });

      const baseDeps: AgentReconnectDependencies = {
        getSessions: () => ({ [session.id]: session }),
        dispatch: vi.fn(),
        spawn,
        attach: vi.fn(async () => undefined),
      };

      const wrappedDeps = withAgentConflictAdoption(baseDeps, { describe });

      await expect(wrappedDeps.spawn!(spawnRequest)).rejects.toBe(conflictError);
    });

    it("(vii) non-conflict spawn errors pass through untouched", async () => {
      const otherError = {
        code: "SHELL_NOT_FOUND",
        message: "Failed to spawn shell",
      };
      const spawn = vi.fn(async () => {
        throw otherError;
      });
      const describe = vi.fn();

      const baseDeps: AgentReconnectDependencies = {
        getSessions: () => ({}),
        dispatch: vi.fn(),
        spawn,
        attach: vi.fn(async () => undefined),
      };

      const wrappedDeps = withAgentConflictAdoption(baseDeps, { describe });

      await expect(wrappedDeps.spawn!(spawnRequest)).rejects.toBe(otherError);
      expect(describe).not.toHaveBeenCalled();
    });
  });
});
