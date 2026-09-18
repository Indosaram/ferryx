import { afterEach, describe, expect, it, vi } from "vitest";
import { clearShellReplacementInflightForTests, replaceExitedShellSession } from "./shellReplacement";
import type { TerminalSession } from "./types";

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
});
