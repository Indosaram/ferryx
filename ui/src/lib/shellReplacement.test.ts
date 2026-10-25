import { afterEach, describe, expect, it, vi } from "vitest";
import {
  clearShellReplacementInflightForTests,
  reconnectLocalSession,
  replaceExitedShellSession,
} from "./shellReplacement";
import type { TerminalSession } from "./types";
import { PROJECTS_STORAGE_KEY } from "./storageKeys";

afterEach(clearShellReplacementInflightForTests);
describe("shell replacement", () => {
  it("replaces a local session stuck reconnecting to a backend that never comes back", async () => {
    const session: TerminalSession = { id: "pane", workspaceId: "local-ws", cwd: "/repo", worktree: null, backendSessionId: "dead-pty", lifecycle: "running", remoteConnectionState: "reconnecting", agentType: "omo" };
    const spawn = vi.fn().mockResolvedValue({ sessionId: "new-pty", session: { cwd: "/repo" }, daemonEpoch: "epoch-1" });
    const dispatch = vi.fn();
    const result = await replaceExitedShellSession("pane", { getSessions: () => ({ pane: session }), spawn, dispatch }, { clearAgent: true });
    expect(result.sessionId).toBe("new-pty");
    expect(dispatch).toHaveBeenCalledWith(expect.objectContaining({ type: "REBIND_SESSION_BACKEND", sessionId: "pane", backendSessionId: "new-pty", clearAgent: true }));
  });

  it("discards the new shell when recovery reattaches the reconnecting session first", async () => {
    let session: TerminalSession = { id: "pane", workspaceId: "local-ws", cwd: "/repo", worktree: null, backendSessionId: "live-pty", lifecycle: "running", remoteConnectionState: "reconnecting" };
    const spawn = vi.fn().mockImplementation(async () => {
      session = { ...session, remoteConnectionState: undefined };
      return { sessionId: "new-pty", session: { cwd: "/repo" }, daemonEpoch: "epoch-1" };
    });
    const close = vi.fn().mockResolvedValue(undefined);
    const dispatch = vi.fn();
    await expect(replaceExitedShellSession("pane", { getSessions: () => ({ pane: session }), spawn, dispatch, close })).rejects.toMatchObject({ code: "AGENT_RESUME_INVALID" });
    expect(close).toHaveBeenCalledWith("new-pty");
    expect(dispatch).not.toHaveBeenCalled();
  });

  it.each([null, "claude"])("never replaces an SSH shell or agent (%s)", async agentType => {
    const session: TerminalSession = { id: "pane", workspaceId: "ssh:host:project", cwd: "/srv", worktree: null, backendSessionId: null, lifecycle: "exited", agentType };
    const spawn = vi.fn();
    const dispatch = vi.fn();
    await expect(replaceExitedShellSession("pane", { getSessions: () => ({ pane: session }), spawn, dispatch })).rejects.toMatchObject({ code: "AGENT_RESUME_INVALID" });
    expect(spawn).not.toHaveBeenCalled();
    expect(dispatch).not.toHaveBeenCalled();
  });

  it("replaces an exited local shell session when backendSessionId is still set", async () => {
    const session: TerminalSession = { id: "pane", workspaceId: "local-ws", cwd: "/repo", worktree: null, backendSessionId: "exited-pty", lifecycle: "exited" };
    const spawn = vi.fn().mockResolvedValue({ sessionId: "new-pty", session: { cwd: "/repo" }, daemonEpoch: "epoch-1" });
    const dispatch = vi.fn();
    const result = await replaceExitedShellSession("pane", { getSessions: () => ({ pane: session }), spawn, dispatch });
    expect(result.sessionId).toBe("new-pty");
    expect(dispatch).toHaveBeenCalledWith({
      type: "REBIND_SESSION_BACKEND",
      sessionId: "pane",
      backendSessionId: "new-pty",
      cwd: "/repo",
      daemonEpoch: "epoch-1",
      clearAgent: undefined,
    });
  });

  it("allows replacing an exited agent session when clearAgent is true", async () => {
    const session: TerminalSession = { id: "pane", workspaceId: "local-ws", cwd: "/repo", worktree: null, backendSessionId: null, lifecycle: "exited", agentType: "copilot" };
    const spawn = vi.fn().mockResolvedValue({ sessionId: "new-pty", session: { cwd: "/repo" }, daemonEpoch: "epoch-1" });
    const dispatch = vi.fn();
    const result = await replaceExitedShellSession("pane", { getSessions: () => ({ pane: session }), spawn, dispatch }, { clearAgent: true });
    expect(result.sessionId).toBe("new-pty");
    expect(dispatch).toHaveBeenCalledWith({
      type: "REBIND_SESSION_BACKEND",
      sessionId: "pane",
      backendSessionId: "new-pty",
      cwd: "/repo",
      daemonEpoch: "epoch-1",
      clearAgent: true,
    });
  });

  it("replaces an exited paired shell session with pairedDaemon startup", async () => {
    const pairedWs = "daemon:111122223333444455556666777788889999aaaabbbbccccddddeeeeffff0000";
    localStorage.setItem(
      PROJECTS_STORAGE_KEY,
      JSON.stringify([
        {
          workspaceId: pairedWs,
          remoteWorkspaceId: "project-remote-1",
          target: { kind: "pairedDaemon", hostId: "host-relay-1" },
        },
      ]),
    );
    const session: TerminalSession = {
      id: "pane",
      workspaceId: pairedWs,
      cwd: "/remote/path",
      worktree: null,
      backendSessionId: null,
      lifecycle: "exited",
    };
    const spawn = vi.fn().mockResolvedValue({
      sessionId: "daemon-session:abc",
      session: { cwd: "/remote/path" },
      daemonEpoch: "epoch-remote",
    });
    const dispatch = vi.fn();
    const result = await replaceExitedShellSession("pane", {
      getSessions: () => ({ pane: session }),
      spawn,
      dispatch,
    });
    expect(result.sessionId).toBe("daemon-session:abc");
    expect(spawn).toHaveBeenCalledWith(
      expect.objectContaining({
        workspaceId: pairedWs,
        startup: {
          kind: "pairedDaemon",
          hostId: "host-relay-1",
          remoteWorkspaceId: "project-remote-1",
        },
      }),
    );
    expect(dispatch).toHaveBeenCalledWith({
      type: "REBIND_SESSION_BACKEND",
      sessionId: "pane",
      backendSessionId: "daemon-session:abc",
      cwd: "/remote/path",
      daemonEpoch: "epoch-remote",
      clearAgent: undefined,
    });
  });

  describe("reconnectLocalSession", () => {
    it("rebinds with live daemonEpoch and incarnation without dispatching LOCAL_SESSIONS_RECONCILED when backend is alive", async () => {
      const session: TerminalSession = {
        id: "pane",
        workspaceId: "local-ws",
        cwd: "/repo",
        worktree: null,
        backendSessionId: "live-backend-pty",
        lifecycle: "exited",
        daemonEpoch: "epoch-old",
        incarnation: "inc-1",
      };
      const list = vi.fn().mockResolvedValue([
        {
          sessionId: "live-backend-pty",
          daemonEpoch: "epoch-new",
          incarnation: "inc-1",
          running: true,
        },
      ]);
      const dispatch = vi.fn();
      const persist = vi.fn();
      const openNewShell = vi.fn();

      await reconnectLocalSession("pane", {
        getSessions: () => ({ pane: session }),
        list,
        dispatch,
        persist,
        openNewShell,
      });

      expect(dispatch).toHaveBeenCalledWith({
        type: "REBIND_SESSION_BACKEND",
        sessionId: "pane",
        backendSessionId: "live-backend-pty",
        cwd: "/repo",
        daemonEpoch: "epoch-new",
        incarnation: "inc-1",
      });
      expect(persist).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "pane",
          backendSessionId: "live-backend-pty",
          daemonEpoch: "epoch-new",
          incarnation: "inc-1",
        }),
      );
      expect(openNewShell).not.toHaveBeenCalled();
      expect(
        dispatch.mock.calls.some(([action]) => (action as any).type === "LOCAL_SESSIONS_RECONCILED"),
      ).toBe(false);
    });

    it("does not fall back to openNewShell when backend session is absent or not running on daemon (zero spawn INV-1)", async () => {
      const session: TerminalSession = {
        id: "pane",
        workspaceId: "local-ws",
        cwd: "/repo",
        worktree: null,
        backendSessionId: "dead-backend-pty",
        lifecycle: "exited",
      };
      const list = vi.fn().mockResolvedValue([
        { sessionId: "dead-backend-pty", running: false },
      ]);
      const dispatch = vi.fn();
      const openNewShell = vi.fn();

      await reconnectLocalSession("pane", {
        getSessions: () => ({ pane: session }),
        list,
        dispatch,
        openNewShell,
      });

      expect(openNewShell).not.toHaveBeenCalled();
      expect(dispatch).not.toHaveBeenCalled();
    });

    it("does not fall back to openNewShell when list throws an error (query error zero spawn)", async () => {
      const session: TerminalSession = {
        id: "pane",
        workspaceId: "local-ws",
        cwd: "/repo",
        worktree: null,
        backendSessionId: "any-pty",
        lifecycle: "exited",
      };
      const list = vi.fn().mockRejectedValue(new Error("daemon unreachable"));
      const dispatch = vi.fn();
      const openNewShell = vi.fn();

      await reconnectLocalSession("pane", {
        getSessions: () => ({ pane: session }),
        list,
        dispatch,
        openNewShell,
      });

      expect(openNewShell).not.toHaveBeenCalled();
      expect(dispatch).not.toHaveBeenCalled();
    });

    it("falls back to openNewShell only when session has no backendSessionId (legitimate never-created session)", async () => {
      const session: TerminalSession = {
        id: "pane",
        workspaceId: "local-ws",
        cwd: "/repo",
        worktree: null,
        backendSessionId: null,
        lifecycle: "exited",
      };
      const list = vi.fn().mockResolvedValue([]);
      const dispatch = vi.fn();
      const openNewShell = vi.fn();

      await reconnectLocalSession("pane", {
        getSessions: () => ({ pane: session }),
        list,
        dispatch,
        openNewShell,
      });

      expect(openNewShell).toHaveBeenCalledWith("pane");
      expect(dispatch).not.toHaveBeenCalled();
    });

    it("rejects an observed different incarnation rather than adopting it (identity conflict)", async () => {
      const session: TerminalSession = {
        id: "pane",
        workspaceId: "local-ws",
        cwd: "/repo",
        worktree: null,
        backendSessionId: "b-1",
        incarnation: "inc-expected-1",
        lifecycle: "working",
      };
      const list = vi.fn().mockResolvedValue([
        { sessionId: "b-1", running: true, incarnation: "inc-different-2", daemonEpoch: "epoch-1" },
      ]);
      const dispatch = vi.fn();
      const persist = vi.fn();

      await reconnectLocalSession("pane", {
        getSessions: () => ({ pane: session }),
        list,
        dispatch,
        persist,
      });

      expect(dispatch).not.toHaveBeenCalled();
      expect(persist).not.toHaveBeenCalled();
    });

    it("fences rebind when session backendId changes post-query or post-persist", async () => {
      let currentSession: TerminalSession = {
        id: "pane",
        workspaceId: "local-ws",
        cwd: "/repo",
        worktree: null,
        backendSessionId: "b-1",
        incarnation: "inc-1",
        lifecycle: "working",
      };
      const list = vi.fn().mockImplementation(async () => {
        currentSession = { ...currentSession, backendSessionId: "b-new-2" };
        return [{ sessionId: "b-1", running: true, incarnation: "inc-1" }];
      });
      const dispatch = vi.fn();
      const persist = vi.fn();

      await reconnectLocalSession("pane", {
        getSessions: () => ({ pane: currentSession }),
        list,
        dispatch,
        persist,
      });

      expect(dispatch).not.toHaveBeenCalled();
      expect(persist).not.toHaveBeenCalled();
    });

    it("does not fall back to openNewShell when session has historically retained null backendSessionId", async () => {
      const session: TerminalSession = {
        id: "pane",
        workspaceId: "local-ws",
        cwd: "/repo",
        worktree: null,
        backendSessionId: null,
        recoveryKey: "local:ws:b-old:inc-old",
        remoteConnectionState: "reconnecting",
        lifecycle: "working",
      };
      const dispatch = vi.fn();
      const openNewShell = vi.fn();

      await reconnectLocalSession("pane", {
        getSessions: () => ({ pane: session }),
        dispatch,
        openNewShell,
      });

      expect(openNewShell).not.toHaveBeenCalled();
      expect(dispatch).not.toHaveBeenCalled();
    });
  });
});
