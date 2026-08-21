import { beforeEach, describe, expect, it, vi } from "vitest";

const core = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import {
  createWorktree,
  deleteWorktree,
  deleteWorktreeDestructive,
  getWorktreeStatus,
  listTerminalSessions,
  listWorktrees,
  previewWorktreeDelete,
  signalTerminal,
  spawnTerminal,
  toIpcError,
} from "./tauri";

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

  it("wraps worktree status, preview, safe delete, and destructive delete commands", async () => {
    core.invoke
      .mockResolvedValueOnce({ isDirty: true, files: [] })
      .mockResolvedValueOnce({ branch: "feature", head: "abc", upstream: null, merged: false, ahead: 1, behind: 0 })
      .mockResolvedValue(undefined);
    const request = { workspaceId: "workspace-main", worktree: { wsId: "ws-main", slug: "feature" } };

    await getWorktreeStatus(request);
    await previewWorktreeDelete(request);
    await deleteWorktree({ ...request, deleteBranch: true });
    await deleteWorktreeDestructive({ ...request, deleteBranch: true });

    expect(core.invoke).toHaveBeenNthCalledWith(1, "cmd_worktree_status", { request });
    expect(core.invoke).toHaveBeenNthCalledWith(2, "cmd_worktree_delete_preview", { request });
    expect(core.invoke).toHaveBeenNthCalledWith(3, "cmd_worktree_delete", {
      request: { ...request, deleteBranch: true },
    });
    expect(core.invoke).toHaveBeenNthCalledWith(4, "cmd_worktree_delete_destructive", {
      request: { ...request, deleteBranch: true },
    });
  });

  it("wraps terminal signal and terminal session listing", async () => {
    core.invoke.mockResolvedValueOnce(undefined).mockResolvedValueOnce([
      { sessionId: "terminal-1", worktreePath: "/repo/feature" },
    ]);

    await signalTerminal({ sessionId: "terminal-1", signal: "interrupt" });
    await expect(listTerminalSessions()).resolves.toEqual([
      { sessionId: "terminal-1", worktreePath: "/repo/feature" },
    ]);

    expect(core.invoke).toHaveBeenNthCalledWith(1, "cmd_terminal_signal", {
      sessionId: "terminal-1",
      signal: "interrupt",
    });
    expect(core.invoke).toHaveBeenNthCalledWith(2, "cmd_terminal_list", undefined);
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
