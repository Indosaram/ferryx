import { beforeEach, describe, expect, it, vi } from "vitest";

const core = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));

const events = vi.hoisted(() => ({
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("@tauri-apps/api/event", () => events);

import {
  createWorktree,
  deleteWorktree,
  deleteWorktreeDestructive,
  getTerminalPreferences,
  getWorktreeStatus,
  listProjectBranches,
  listTerminalSessions,
  onNewTerminalTabMenu,
  listWorktrees,
  previewWorktreeDelete,
  registerProject,
  signalTerminal,
  spawnTerminal,
  toIpcError,
} from "./tauri";

describe("Tauri IPC wrapper contract", () => {
  beforeEach(() => {
    core.invoke.mockReset();
    events.listen.mockReset();
    core.isTauri.mockReturnValue(true);
  });

  it("registers projects and lists real local branches through typed native DTOs", async () => {
    core.invoke
      .mockResolvedValueOnce({ workspaceId: "ferryx", repoRoot: "/repo/ferryx" })
      .mockResolvedValueOnce([
        { name: "feature/a", isCurrent: false },
        { name: "main", isCurrent: true },
      ]);

    await expect(registerProject({ workspaceId: "ferryx", repoPath: "/repo/ferryx" })).resolves.toEqual({
      workspaceId: "ferryx",
      repoRoot: "/repo/ferryx",
    });
    await expect(listProjectBranches("ferryx")).resolves.toEqual([
      { name: "feature/a", isCurrent: false },
      { name: "main", isCurrent: true },
    ]);

    expect(core.invoke).toHaveBeenNthCalledWith(1, "cmd_project_register", {
      request: { workspaceId: "ferryx", repoPath: "/repo/ferryx" },
    });
    expect(core.invoke).toHaveBeenNthCalledWith(2, "cmd_project_branches", {
      request: { workspaceId: "ferryx" },
    });
  });

  it("fetches the native effective Ghostty terminal preferences", async () => {
    const preferences = {
      fontFamily: "Noto Sans KR",
      macosOptionAsAlt: true,
      source: "ghostty",
      status: "imported",
      sourcePath: "/Users/test/.config/ghostty/config",
    };
    core.invoke.mockResolvedValue(preferences);

    await expect(getTerminalPreferences()).resolves.toEqual(preferences);
    expect(core.invoke).toHaveBeenCalledWith("cmd_terminal_preferences", undefined);
  });

  it("spawns terminals with workspace/worktree identity and an optional cwd slot", async () => {
    core.invoke.mockResolvedValue({ sessionId: "backend-session-1" });

    await expect(
      spawnTerminal({ workspaceId: "workspace-main", worktree: { wsId: "ws-main", slug: "main" } }),
    ).resolves.toBe("backend-session-1");

    expect(core.invoke).toHaveBeenCalledWith("cmd_terminal_spawn", {
      request: {
        workspaceId: "workspace-main",
        worktree: { wsId: "ws-main", slug: "main" },
        cwd: null,
      },
    });
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

  it("bridges the native Cmd+T menu event into the frontend callback", async () => {
    const unlisten = vi.fn();
    let listener: ((event: { payload: void }) => void) | null = null;
    events.listen.mockImplementation(async (eventName: string, callback: (event: { payload: void }) => void) => {
      expect(eventName).toBe("menu_new_terminal_tab");
      listener = callback;
      return unlisten;
    });
    const handler = vi.fn();

    await expect(onNewTerminalTabMenu(handler)).resolves.toBe(unlisten);
    expect(listener).toBeTypeOf("function");
    if (typeof listener === "function") {
      (listener as (event: { payload: void }) => void)({ payload: undefined });
    }
    expect(handler).toHaveBeenCalledOnce();
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