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
});
