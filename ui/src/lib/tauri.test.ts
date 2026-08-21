import { beforeEach, describe, expect, it, vi } from "vitest";

const core = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { createWorktree, listWorktrees, spawnTerminal, toIpcError } from "./tauri";

describe("Tauri IPC wrapper contract", () => {
  beforeEach(() => {
    core.invoke.mockReset();
    core.isTauri.mockReturnValue(true);
  });

  it("spawns terminals with only workspace and worktree identities", async () => {
    core.invoke.mockResolvedValue({ sessionId: "backend-session-1" });

    await expect(
      spawnTerminal({ workspaceId: "workspace-main", worktree: { wsId: "ws-main", slug: "main" } }),
    ).resolves.toBe("backend-session-1");

    expect(core.invoke).toHaveBeenCalledWith("cmd_terminal_spawn", {
      request: { workspaceId: "workspace-main", worktree: { wsId: "ws-main", slug: "main" } },
    });
    expect(core.invoke.mock.calls[0][1]).not.toHaveProperty("cwd");
    expect(core.invoke.mock.calls[0][1]).not.toHaveProperty("command");
  });

  it("sends worktree create DTOs in camelCase", async () => {
    core.invoke.mockResolvedValue({});

    await createWorktree({
      workspaceId: "workspace-main",
      worktree: { wsId: "ws-main", slug: "feature" },
      baseRef: "HEAD",
    });

    expect(core.invoke).toHaveBeenCalledWith("cmd_worktree_create", {
      request: {
        workspaceId: "workspace-main",
        worktree: { wsId: "ws-main", slug: "feature" },
        baseRef: "HEAD",
      },
    });
  });

  it("lists worktrees through a registered workspace identity", async () => {
    core.invoke.mockResolvedValue([]);

    await listWorktrees("workspace-main");

    expect(core.invoke).toHaveBeenCalledWith("cmd_worktree_list", { workspaceId: "workspace-main" });
  });

  it("normalizes rejected command invocations at the wrapper boundary", async () => {
    core.invoke.mockRejectedValue("backend exploded");

    await expect(
      createWorktree({ workspaceId: "workspace-main", worktree: { wsId: "ws-main", slug: "feature" } }),
    ).rejects.toEqual({
      code: "UNKNOWN",
      message: "Unknown IPC error",
      details: {},
    });
  });

  it("preserves structured errors and does not parse message strings", () => {
    const structured = { code: "DIRTY_WORKTREE", message: "dirty", details: { worktreeId: "wt-main" } };
    expect(toIpcError(structured)).toStrictEqual(structured);
    const noDetails = { code: "GIT_ERROR", message: "git failed" };
    expect(toIpcError(noDetails)).toEqual({ ...noDetails, details: {} });
    expect(toIpcError("DIRTY_WORKTREE: dirty")).toEqual({
      code: "UNKNOWN",
      message: "Unknown IPC error",
      details: {},
    });
  });
});
