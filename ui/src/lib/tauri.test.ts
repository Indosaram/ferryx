import { beforeEach, describe, expect, it, vi } from "vitest";

const core = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { createWorktree, spawnTerminal, toIpcError } from "./tauri";

describe("Tauri IPC wrapper contract", () => {
  beforeEach(() => {
    core.invoke.mockReset();
    core.isTauri.mockReturnValue(true);
  });

  it("spawns terminals with only workspaceId and worktreeId", async () => {
    core.invoke.mockResolvedValue("backend-session-1");

    await expect(spawnTerminal({ workspaceId: "ws-main", worktreeId: "wt-main" })).resolves.toBe("backend-session-1");

    expect(core.invoke).toHaveBeenCalledWith("cmd_terminal_spawn", {
      request: { workspaceId: "ws-main", worktreeId: "wt-main" },
    });
    expect(core.invoke.mock.calls[0][1]).not.toHaveProperty("cwd");
    expect(core.invoke.mock.calls[0][1]).not.toHaveProperty("command");
  });

  it("sends worktree create DTOs in camelCase", async () => {
    core.invoke.mockResolvedValue({ worktreeId: "wt-feature" });

    await createWorktree({ wsId: "ws-main", slug: "feature", baseRef: "HEAD" });

    expect(core.invoke).toHaveBeenCalledWith("cmd_worktree_create", {
      request: { wsId: "ws-main", slug: "feature", baseRef: "HEAD" },
    });
  });

  it("normalizes rejected command invocations at the wrapper boundary", async () => {
    core.invoke.mockRejectedValue("backend exploded");

    await expect(createWorktree({ wsId: "ws-main", slug: "feature" })).rejects.toEqual({
      code: "UNKNOWN",
      message: "Unknown IPC error",
      details: {},
    });
  });

  it("preserves structured errors and does not parse message strings", () => {
    const structured = { code: "DIRTY_WORKTREE", message: "dirty", details: { worktreeId: "wt-main" } };
    expect(toIpcError(structured)).toBe(structured);
    expect(toIpcError("DIRTY_WORKTREE: dirty")).toEqual({
      code: "UNKNOWN",
      message: "Unknown IPC error",
      details: {},
    });
  });
});