import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { usePendingWorktree } from "./pendingWorktree";

describe("usePendingWorktree", () => {
  it("drops the pending agent when the pending worktree path is cleared", () => {
    const { result } = renderHook(() => usePendingWorktree());
    act(() => result.current.setPendingWorktreePath("/repos/beta/wt"));
    result.current.pendingWorktreeAgentRef.current = { path: "/repos/beta/wt", agentId: "claude" };

    act(() => result.current.setPendingWorktreePath(null));

    expect(result.current.pendingWorktree).toBeNull();
    expect(result.current.pendingWorktreeAgentRef.current).toBeNull();
  });

  it("keeps the pending agent while the same path stays pending", () => {
    const { result } = renderHook(() => usePendingWorktree());
    act(() => result.current.setPendingWorktreePath("/repos/beta/wt"));
    const pending = result.current.pendingWorktree;
    result.current.pendingWorktreeAgentRef.current = { path: "/repos/beta/wt", agentId: "claude" };

    act(() => result.current.setPendingWorktreePath("/repos/beta/wt"));

    expect(result.current.pendingWorktree).toBe(pending);
    expect(result.current.pendingWorktreeAgentRef.current).toEqual({ path: "/repos/beta/wt", agentId: "claude" });
  });
});
