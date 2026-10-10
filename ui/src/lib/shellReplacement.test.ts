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

    it("falls back to openNewShell when backend session is not live on daemon", async () => {
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

      expect(openNewShell).toHaveBeenCalledWith("pane");
      expect(dispatch).not.toHaveBeenCalled();
    });

    it("falls back to openNewShell when list throws an error", async () => {
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

      expect(openNewShell).toHaveBeenCalledWith("pane");
      expect(dispatch).not.toHaveBeenCalled();
    });
  });
});
