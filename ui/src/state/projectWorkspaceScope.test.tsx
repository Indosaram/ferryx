import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { useWorkspaceRuntime, type WorkspaceRuntimeServices } from "./workspaceRuntime";
import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/orca/project-a/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

describe("registered project workspace scope", () => {
  it("spawns terminal sessions with the selected registered workspace id and worktree-root cwd", async () => {
    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      spawnTerminal: vi.fn(async () => "backend-1"),
      getTerminalCwd: vi.fn(async () => worktree.path),
      closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
    };
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree], services, workspaceId: "project-a" } as any),
    );

    await act(async () => {
      await result.current.openTab(worktree);
    });

    expect(services.spawnTerminal).toHaveBeenCalledWith({
      workspaceId: "project-a",
      worktree: { wsId: "project-a", slug: "main" },
      cwd: "/repo/main",
    });
    expect(Object.values(result.current.state.sessions)[0]).toMatchObject({ workspaceId: "project-a" });
  });

  it("lists worktrees from the selected registered workspace id", async () => {
    const services: WorkspaceRuntimeServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      listWorktrees: vi.fn(async () => [worktree]),
      onWorktreeChanged: vi.fn(async () => () => undefined),
      isTauriRuntime: vi.fn(() => true),
    };

    renderHook(() =>
      useWorkspaceRuntime({
        workspaceId: "project-a",
        activeWorktreePath: null,
        syncWorktrees: vi.fn(async () => undefined),
        ensureTabForWorktree: vi.fn(async () => "tab-1"),
        services,
      } as any),
    );

    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalled());
    expect(services.listWorktrees).toHaveBeenCalledWith("project-a");
  });
});