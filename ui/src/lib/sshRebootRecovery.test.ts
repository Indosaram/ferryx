import { afterEach, describe, expect, it, vi } from "vitest";
import {
  clearSshReconnectInflightForTests,
  reconnectSshSession,
} from "./sshRebootRecovery";
import type { RemoteFailure, StructuredIpcError, TerminalSession } from "./types";

function testToIpcError(error: unknown): StructuredIpcError {
  if (error && typeof error === "object") {
    const candidate = error as Record<string, unknown>;
    if (typeof candidate.code === "string" && typeof candidate.message === "string") {
      return {
        code: candidate.code,
        message: candidate.message,
        details: candidate.details && typeof candidate.details === "object" ? (candidate.details as Record<string, unknown>) : {},
      };
    }
  }
  return {
    code: "UNKNOWN",
    message: error instanceof Error ? error.message : "Unknown error",
    details: {},
  };
}

function mockSession(overrides: Partial<TerminalSession> = {}): TerminalSession {
  return {
    id: "pane-1",
    workspaceId: "ssh:server:repo",
    cwd: "/srv/repo",
    worktree: null,
    backendSessionId: "stable-backend",
    lifecycle: "working",
    remoteConnectionState: "connected",
    remoteGeneration: 1,
    agentType: "claude",
    agentSessionId: "agent-session-123",
    providerSession: { key: "session_id", id: "agent-session-123" },
    daemonEpoch: "epoch-1",
    ...overrides,
  };
}

afterEach(() => {
  clearSshReconnectInflightForTests();
});

describe("reconnectSshSession", () => {
  it("calls retryRemoteSession with backendSessionId and retains IDs without spawning", async () => {
    const session = mockSession();
    const retryRemoteSession = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    const reportRuntimeError = vi.fn();

    await reconnectSshSession("pane-1", {
      getSessions: () => ({ "pane-1": session }),
      retryRemoteSession,
      toIpcError: testToIpcError,
      reportRuntimeError,
    });

    expect(retryRemoteSession).toHaveBeenCalledExactlyOnceWith("stable-backend");
    expect(reportRuntimeError).not.toHaveBeenCalled();
    expect(session.id).toBe("pane-1");
    expect(session.backendSessionId).toBe("stable-backend");
  });

  it("propagates typed error on remoteSessionError and reports error exactly once", async () => {
    const session = mockSession();
    const failure: RemoteFailure = { kind: "missing", message: "Remote PTY not found after host reboot" };
    const retryRemoteSession = vi.fn(async () => ({
      type: "remoteSessionError" as const,
      failure,
    }));
    const reportRuntimeError = vi.fn();

    await expect(
      reconnectSshSession("pane-1", {
        getSessions: () => ({ "pane-1": session }),
        retryRemoteSession,
        toIpcError: testToIpcError,
        reportRuntimeError,
      }),
    ).rejects.toMatchObject({
      code: "REMOTE_MISSING",
      message: failure.message,
    });

    expect(retryRemoteSession).toHaveBeenCalledExactlyOnceWith("stable-backend");
    expect(reportRuntimeError).toHaveBeenCalledOnce();
    expect(reportRuntimeError).toHaveBeenCalledWith(
      expect.objectContaining({
        code: "REMOTE_MISSING",
        message: failure.message,
      }),
    );
    expect(session.id).toBe("pane-1");
    expect(session.backendSessionId).toBe("stable-backend");
  });

  it("propagates typed error when retryRemoteSession throws and reports error exactly once", async () => {
    const session = mockSession();
    const throwErr = new Error("Transport socket disconnected");
    const retryRemoteSession = vi.fn(async () => {
      throw throwErr;
    });
    const reportRuntimeError = vi.fn();

    await expect(
      reconnectSshSession("pane-1", {
        getSessions: () => ({ "pane-1": session }),
        retryRemoteSession,
        toIpcError: testToIpcError,
        reportRuntimeError,
      }),
    ).rejects.toMatchObject({
      code: "UNKNOWN",
      message: throwErr.message,
    });

    expect(retryRemoteSession).toHaveBeenCalledExactlyOnceWith("stable-backend");
    expect(reportRuntimeError).toHaveBeenCalledOnce();
    expect(reportRuntimeError).toHaveBeenCalledWith(
      expect.objectContaining({
        code: "UNKNOWN",
        message: throwErr.message,
      }),
    );
    expect(session.id).toBe("pane-1");
    expect(session.backendSessionId).toBe("stable-backend");
  });

  it("coalesces duplicate concurrent calls into single execution", async () => {
    const session = mockSession();
    const target = new EventTarget();
    const deferred = new Promise<{ type: "retryRemoteSessionOk" }>((resolve) => {
      target.addEventListener("resolve", () => resolve({ type: "retryRemoteSessionOk" }), { once: true });
    });
    const retryRemoteSession = vi.fn(() => deferred);

    const deps = {
      getSessions: () => ({ "pane-1": session }),
      retryRemoteSession,
      toIpcError: testToIpcError,
    };

    const task1 = reconnectSshSession("pane-1", deps);
    const task2 = reconnectSshSession("pane-1", deps);

    expect(task1).toBe(task2);
    expect(retryRemoteSession).toHaveBeenCalledOnce();

    target.dispatchEvent(new Event("resolve"));
    await Promise.all([task1, task2]);

    expect(retryRemoteSession).toHaveBeenCalledOnce();
  });

  it("fails closed when backendSessionId is missing even with valid-looking provider reference", async () => {
    const session = mockSession({
      backendSessionId: null,
      agentType: "claude",
      agentSessionId: "remote-agent-ref",
      providerSession: { key: "session_id", id: "remote-agent-ref" },
    });
    const retryRemoteSession = vi.fn();
    const reportRuntimeError = vi.fn();

    await expect(
      reconnectSshSession("pane-1", {
        getSessions: () => ({ "pane-1": session }),
        retryRemoteSession,
        toIpcError: testToIpcError,
        reportRuntimeError,
      }),
    ).rejects.toMatchObject({
      code: "REMOTE_SESSION_CANNOT_RESTORE",
    });

    expect(retryRemoteSession).not.toHaveBeenCalled();
    expect(reportRuntimeError).toHaveBeenCalledOnce();
    expect(reportRuntimeError).toHaveBeenCalledWith(
      expect.objectContaining({
        code: "REMOTE_SESSION_CANNOT_RESTORE",
      }),
    );
    expect(session.backendSessionId).toBeNull();
    expect(session.id).toBe("pane-1");
  });

  it("fails closed when session is completely unknown", async () => {
    const retryRemoteSession = vi.fn();
    const reportRuntimeError = vi.fn();

    await expect(
      reconnectSshSession("pane-unknown", {
        getSessions: () => ({}),
        retryRemoteSession,
        toIpcError: testToIpcError,
        reportRuntimeError,
      }),
    ).rejects.toMatchObject({
      code: "SESSION_NOT_FOUND",
    });

    expect(retryRemoteSession).not.toHaveBeenCalled();
    expect(reportRuntimeError).toHaveBeenCalledOnce();
  });
});
