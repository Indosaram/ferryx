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
  attachTerminal,
  createWorktree,
  deleteWorktree,
  deleteWorktreeDestructive,
  getInitialProject,
  getTerminalPreferences,
  getWorktreeStatus,
  listProjectBranches,
  listTerminalSessions,
  onNewTerminalTabMenu,
  onCloseTabMenu,
  listWorktrees,
  previewWorktreeDelete,
  registerProject,
  signalTerminal,
  spawnTerminal,
  publishFocusedTerminal,
  onRemoteSelectionRequested,
  normalizeBadgeCount,
  setBadgeCount,
  toIpcError,
} from "./tauri";

describe("Tauri IPC wrapper contract", () => {
  beforeEach(() => {
    core.invoke.mockReset();
    events.listen.mockReset();
    core.isTauri.mockReturnValue(true);
  });

  it("gets the native initial project without a request", async () => {
    core.invoke.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });

    await expect(getInitialProject()).resolves.toEqual({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });
    expect(core.invoke).toHaveBeenCalledWith("cmd_project_initial", undefined);
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

  it("spawns terminals with caller-stable clientRequestId, workspace/worktree identity, and an optional cwd slot", async () => {
    core.invoke.mockResolvedValue({ sessionId: "backend-session-1" });

    await expect(
      spawnTerminal({
        workspaceId: "workspace-main",
        worktree: { wsId: "ws-main", slug: "main" },
        clientRequestId: "spawn-logical-action-1",
      }),
    ).resolves.toBe("backend-session-1");

    expect(core.invoke).toHaveBeenCalledWith("cmd_terminal_spawn", {
      request: {
        workspaceId: "workspace-main",
        worktree: { wsId: "ws-main", slug: "main" },
        cwd: null,
        clientRequestId: "spawn-logical-action-1",
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

  it("bridges the native Cmd+W menu event into the frontend callback", async () => {
    const unlisten = vi.fn();
    let listener: ((event: { payload: void }) => void) | null = null;
    events.listen.mockImplementation(async (eventName: string, callback: (event: { payload: void }) => void) => {
      expect(eventName).toBe("menu_close_tab");
      listener = callback;
      return unlisten;
    });
    const handler = vi.fn();

    await expect(onCloseTabMenu(handler)).resolves.toBe(unlisten);
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

  it("attaches to a terminal session with typed protocol-v2 DTO and optional resume sequence", async () => {
    const mockAttachResponse = {
      sessionId: "term-session-1",
      daemonEpoch: "epoch-100",
      historyStartSequence: "1",
      historyEndSequence: "42",
      history: "aGVsbG8gd29ybGQ=",
      gap: null,
    };
    core.invoke.mockResolvedValue(mockAttachResponse);

    const result = await attachTerminal({ sessionId: "term-session-1", afterSequence: "10" });
    expect(result).toEqual(mockAttachResponse);
    expect(core.invoke).toHaveBeenCalledWith("cmd_terminal_attach", {
      sessionId: "term-session-1",
      afterSequence: "10",
    });

    // Also supports string sessionId overload
    await attachTerminal("term-session-1");
    expect(core.invoke).toHaveBeenCalledWith("cmd_terminal_attach", {
      sessionId: "term-session-1",
      afterSequence: null,
    });
  });

  it("handles non-tauri runtime fallback for attachTerminal", async () => {
    core.isTauri.mockReturnValue(false);

    const result = await attachTerminal({ sessionId: "preview-sess", afterSequence: "5" });
    expect(result).toEqual({
      sessionId: "preview-sess",
      daemonEpoch: null,
      historyStartSequence: null,
      historyEndSequence: null,
      history: "",
      gap: null,
    });
  });

  it("publishes focused terminal payload to native IPC", async () => {
    core.invoke.mockResolvedValue(undefined);

    await publishFocusedTerminal({
      workspaceId: "orca-lite",
      worktreeSlug: "main",
      worktreeLabel: "main",
      backendSessionId: "pty-1",
      activeTabId: "tab-1",
      tabs: [{ id: "tab-1", label: "main" }],
    });

    expect(core.invoke).toHaveBeenCalledWith("cmd_remote_set_active_selection", {
      request: {
        workspaceId: "orca-lite",
        worktreeSlug: "main",
        worktreeLabel: "main",
        sessionId: "pty-1",
        tabId: "tab-1",
        activeTabId: "tab-1",
        tabs: [{ id: "tab-1", label: "main" }],
        terminalTabs: [{ id: "tab-1", label: "main" }],
      },
    });

    // Clear active selection (null payload)
    await publishFocusedTerminal(null);
    expect(core.invoke).toHaveBeenCalledWith("cmd_remote_set_active_selection", {
      request: {
        workspaceId: null,
        worktreeSlug: null,
        worktreeLabel: null,
        sessionId: null,
        tabId: null,
        activeTabId: null,
        tabs: [],
        terminalTabs: [],
      },
    });
  });

  it("listens for remote selection requests and triggers callback", async () => {
    const unlisten = vi.fn();
    let listener: ((event: { payload: any }) => void) | null = null;
    events.listen.mockImplementation(async (eventName: string, callback: (event: { payload: any }) => void) => {
      expect(eventName).toBe("remote_selection_requested");
      listener = callback;
      return unlisten;
    });
    const handler = vi.fn();

    await expect(onRemoteSelectionRequested(handler)).resolves.toBe(unlisten);
    expect(listener).toBeTypeOf("function");
    if (typeof listener === "function") {
      (listener as (event: { payload: any }) => void)({
        payload: { workspaceId: "orca-lite", worktreeSlug: "feature-1", tabId: "tab-2" },
      });
    }
    expect(handler).toHaveBeenCalledWith({ workspaceId: "orca-lite", worktreeSlug: "feature-1", tabId: "tab-2" });
  });

  it("sets native app badge count with normalized integer payload", async () => {
    core.invoke.mockResolvedValue({ supported: true, count: 5, badgeLabel: "5" });

    const result = await setBadgeCount(5);
    expect(result).toEqual({ supported: true, count: 5, badgeLabel: "5" });
    expect(core.invoke).toHaveBeenCalledWith("cmd_notification_set_badge_count", {
      count: 5,
    });
  });

  it("normalizes boundary badge counts safely within u32 bounds", async () => {
    expect(normalizeBadgeCount(0)).toBe(0);
    expect(normalizeBadgeCount(3.8)).toBe(3);
    expect(normalizeBadgeCount(-10)).toBe(0);
    expect(normalizeBadgeCount(Number.NaN)).toBe(0);
    expect(normalizeBadgeCount(Number.POSITIVE_INFINITY)).toBe(0);
    expect(normalizeBadgeCount(Number.NEGATIVE_INFINITY)).toBe(0);
    expect(normalizeBadgeCount(5_000_000_000)).toBe(4_294_967_295);

    core.invoke.mockResolvedValue({ supported: true, count: 4_294_967_295 });
    await setBadgeCount(5_000_000_000);
    expect(core.invoke).toHaveBeenCalledWith("cmd_notification_set_badge_count", {
      count: 4_294_967_295,
    });
  });

  it("handles non-tauri fallback for setBadgeCount", async () => {
    core.isTauri.mockReturnValue(false);

    const result = await setBadgeCount(2);
    expect(result).toEqual({ supported: false, count: 2 });
    expect(core.invoke).not.toHaveBeenCalled();
  });
});