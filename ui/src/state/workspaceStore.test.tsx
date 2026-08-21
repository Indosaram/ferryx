import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/orca/ws-main/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const featureWorktree: Worktree = {
  ...worktree,
  path: "/repo/feature",
  branch: "refs/heads/orca/ws-main/feature",
};

function createServices(): WorkspaceServices {
  let sessionNumber = 0;
  return {
    ensureTerminalEvents: vi.fn(async () => undefined),
    spawnTerminal: vi.fn(async () => `backend-${++sessionNumber}`),
    closeTerminal: vi.fn(async () => undefined),
  };
}

describe("useWorkspaceStore terminal ownership", () => {
  it("creates exactly one backend PTY per logical tab when single-tab split is enabled", async () => {
    const services = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    await act(async () => {
      await result.current.openTab(worktree);
    });
    await act(async () => {
      await result.current.enableSplit("horizontal");
    });

    expect(result.current.state.layout.tabs).toHaveLength(2);
    expect(result.current.state.layout.secondaryTabId).not.toBeNull();
    expect(result.current.state.layout.split).toBe("horizontal");
    expect(services.spawnTerminal).toHaveBeenCalledTimes(2);
    expect(services.spawnTerminal).toHaveBeenCalledWith({
      workspaceId: "default",
      worktree: { wsId: "ws-main", slug: "main" },
    });
  });

  it("preserves backend session ids when only split orientation changes", async () => {
    const services = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    await act(async () => {
      await result.current.openTab(worktree);
      await result.current.enableSplit("horizontal");
    });

    const before = Object.values(result.current.state.sessions).map((session) => session.backendSessionId);
    act(() => result.current.rotateSplit());
    const after = Object.values(result.current.state.sessions).map((session) => session.backendSessionId);

    expect(result.current.state.layout.split).toBe("vertical");
    expect(after).toEqual(before);
    expect(services.spawnTerminal).toHaveBeenCalledTimes(2);
  });

  it("closes the last tab by atomically replacing it with a valid spawned tab", async () => {
    const services = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    await act(async () => {
      await result.current.openTab(worktree);
    });
    const closingTabId = result.current.state.layout.primaryTabId!;
    const closingSessionId = result.current.state.layout.tabs[0].sessionId;
    const closingBackendId = result.current.state.sessions[closingSessionId].backendSessionId!;

    await act(async () => {
      await result.current.closeTab(closingTabId);
    });

    expect(result.current.state.layout.tabs).toHaveLength(1);
    expect(result.current.state.layout.primaryTabId).toBe(result.current.state.layout.tabs[0].id);
    expect(result.current.state.layout.tabs[0].id).not.toBe(closingTabId);
    expect(result.current.state.layout.secondaryTabId).toBeNull();
    expect(services.spawnTerminal).toHaveBeenCalledTimes(2);
    expect(services.closeTerminal).toHaveBeenCalledWith(closingBackendId);
  });

  it("keeps layout invariants valid when the secondary tab closes", async () => {
    const services = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    await act(async () => {
      await result.current.openTab(worktree);
      await result.current.enableSplit("horizontal");
    });
    const secondaryTabId = result.current.state.layout.secondaryTabId!;

    await act(async () => {
      await result.current.closeTab(secondaryTabId);
    });

    expect(result.current.state.layout.tabs).toHaveLength(1);
    expect(result.current.state.layout.primaryTabId).toBe(result.current.state.layout.tabs[0].id);
    expect(result.current.state.layout.secondaryTabId).toBeNull();
    expect(result.current.state.layout.split).toBe("none");
  });

  it("derives visible agents only from live terminal session metadata", async () => {
    const services = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    expect(result.current.agents).toEqual([]);
    await act(async () => {
      await result.current.openTab(worktree);
    });

    expect(result.current.agents).toEqual([
      expect.objectContaining({
        id: "backend-1",
        sessionId: "backend-1",
        state: "working",
        worktree: { wsId: "ws-main", slug: "main" },
        worktreePath: worktree.path,
        task: "orca/ws-main/main",
      }),
    ]);
  });

  it("removes deleted-worktree tabs, sessions, and agents during synchronization", async () => {
    const services = createServices();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree, featureWorktree], services }),
    );

    await act(async () => {
      await result.current.openTab(worktree);
      await result.current.openTab(featureWorktree);
    });
    expect(result.current.agents).toHaveLength(2);

    await act(async () => {
      await result.current.syncWorktrees([worktree]);
    });

    expect(result.current.state.worktrees).toEqual([worktree]);
    expect(Object.values(result.current.state.sessions).every((session) => session.cwd === worktree.path)).toBe(true);
    expect(result.current.state.layout.tabs).toHaveLength(1);
    expect(result.current.agents).toHaveLength(1);
    expect(services.closeTerminal).toHaveBeenCalledWith("backend-2");
  });
});
