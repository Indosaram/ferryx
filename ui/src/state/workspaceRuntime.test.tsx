import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Worktree, WorktreeChangedPayload } from "../lib/types";
import { useWorkspaceRuntime, type WorkspaceRuntimeServices } from "./workspaceRuntime";

const worktree: Worktree = {
  worktreeId: "wt-main",
  wsId: "ws-main",
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
  isDirty: false,
};

function createServices() {
  let worktreeChangedHandler: ((payload: WorktreeChangedPayload) => void) | null = null;
  const services: WorkspaceRuntimeServices = {
    ensureTerminalEvents: vi.fn(async () => undefined),
    listWorktrees: vi.fn(async () => [worktree]),
    onWorktreeChanged: vi.fn(async (handler) => {
      worktreeChangedHandler = handler;
      return () => undefined;
    }),
    isTauriRuntime: vi.fn(() => true),
  };
  return {
    services,
    emitWorktreeChanged: (payload: WorktreeChangedPayload) => worktreeChangedHandler?.(payload),
  };
}

describe("useWorkspaceRuntime", () => {
  it("registers terminal/worktree listeners before the initial refresh and opens a valid tab", async () => {
    const { services } = createServices();
    const syncWorktrees = vi.fn(async () => undefined);
    const ensureTabForWorktree = vi.fn(async () => "tab-main");
    const { result } = renderHook(() =>
      useWorkspaceRuntime({
        activeWorktreeId: null,
        syncWorktrees,
        ensureTabForWorktree,
        services,
      }),
    );

    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalledTimes(1));

    expect(services.ensureTerminalEvents).toHaveBeenCalledBefore(services.listWorktrees as ReturnType<typeof vi.fn>);
    expect(services.onWorktreeChanged).toHaveBeenCalledBefore(services.listWorktrees as ReturnType<typeof vi.fn>);
    expect(syncWorktrees).toHaveBeenCalledWith([worktree]);
    expect(ensureTabForWorktree).toHaveBeenCalledWith(worktree);
    expect(result.current.runtimeError).toBeNull();
  });

  it("refreshes worktrees when the window regains focus", async () => {
    const { services } = createServices();
    const { result } = renderHook(() =>
      useWorkspaceRuntime({
        activeWorktreeId: worktree.worktreeId,
        syncWorktrees: vi.fn(async () => undefined),
        ensureTabForWorktree: vi.fn(async () => "tab-main"),
        services,
      }),
    );

    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalledTimes(1));
    act(() => window.dispatchEvent(new Event("focus")));
    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalledTimes(2));

    expect(result.current.runtimeError).toBeNull();
  });

  it("refreshes worktrees when the backend emits worktree_changed", async () => {
    const { services, emitWorktreeChanged } = createServices();
    renderHook(() =>
      useWorkspaceRuntime({
        activeWorktreeId: worktree.worktreeId,
        syncWorktrees: vi.fn(async () => undefined),
        ensureTabForWorktree: vi.fn(async () => "tab-main"),
        services,
      }),
    );

    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalledTimes(1));
    act(() =>
      emitWorktreeChanged({ action: "dirty_changed", wsId: worktree.wsId, worktreeId: worktree.worktreeId }),
    );
    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalledTimes(2));
  });
});
