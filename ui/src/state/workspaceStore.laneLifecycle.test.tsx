import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { toast } from "../components/ui/sonner";
import { setLaneLifecycleTransport, type LaneLifecycleTransport, type LaneLifecycleView } from "../lib/sidebarPersistence";
import { createLayoutState } from "./layout";
import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

const root = { path: "/code/ferryx", head: "abc", branch: "main", bare: false, detached: false, locked: null, prunable: null };
const saved: LaneLifecycleView = {
  worktreeKey: "p:ferryx/code/ferryx", workspaceId: "ferryx", worktreePath: root.path,
  state: "pinned", effectiveState: "pinned", pinnedTabs: [], snoozedUntil: null,
  position: 0, revision: 1, updatedAt: "2026-11-15T00:00:00Z",
};
const services: WorkspaceServices = {
  ensureTerminalEvents: vi.fn(async () => undefined), spawnTerminal: vi.fn(async () => "unused"),
  getTerminalCwd: vi.fn(async () => null), closeTerminal: vi.fn(async () => undefined),
  waitForTerminalExit: vi.fn(async () => undefined),
};
function transport(): LaneLifecycleTransport {
  return {
    read: vi.fn(async () => ({ sections: [], revision: 0 })),
    write: vi.fn(async () => saved), writePinTabs: vi.fn(async () => saved),
    importOrder: vi.fn(async () => true), reorder: vi.fn(async () => undefined),
  };
}
afterEach(() => { setLaneLifecycleTransport(null); vi.restoreAllMocks(); });

describe("project-root lifecycle persistence", () => {
  it("commits the real Pin Row payload, including the existing wire boolean", async () => {
    const api = transport();
    setLaneLifecycleTransport(api);
    const { result, unmount } = renderHook(() => useWorkspaceStore({ workspaceId: "ferryx", initialWorktrees: [root], services }));
    await act(async () => { await result.current.refreshLaneLifecycles(); });
    act(() => result.current.beginLaneLifecycle({ worktreeKey: saved.worktreeKey, workspaceId: "ferryx", path: root.path, action: "pin" }));
    await act(async () => { await result.current.commitLaneLifecycle(saved.worktreeKey); });
    expect(api.write).toHaveBeenCalledWith({
      worktreeKey: saved.worktreeKey, workspaceId: "ferryx", path: root.path,
      action: "pin", pinned: true, until: null, expectedRevision: 0,
    });
    expect(result.current.state.laneLifecycle?.[saved.worktreeKey]).toBe("pinned");
    unmount();
  });

  it("pins an unmanaged project's tab under the same key the sidebar renders", async () => {
    const api = transport();
    setLaneLifecycleTransport(api);
    const { result, unmount } = renderHook(() => useWorkspaceStore({ workspaceId: "ferryx", initialWorktrees: [root], services }));
    await act(async () => { await result.current.refreshLaneLifecycles(); });
    // Finish the listing-triggered lifecycle read before exercising the pin.
    await act(async () => { result.current.restoreWorkspace({
      workspaceId: "ferryx", worktrees: [root], activeWorktreePath: root.path, sessions: {},
      layout: createLayoutState([{ id: "tab", kind: "browser", label: "Browser", browserId: "browser", url: "about:blank", worktreePath: root.path }]),
      unreadTabIds: {}, unreadWorktreePaths: {},
    }); });
    await act(async () => { result.current.setTabPinned("tab", true); });
    expect(api.writePinTabs).toHaveBeenCalledWith({ worktreeKey: saved.worktreeKey, workspaceId: "ferryx", path: root.path, pinnedTabs: ["tab"] });
    expect(result.current.state.lanePinnedTabs?.[saved.worktreeKey]).toEqual(["tab"]);
    unmount();
  });

  it("reconciles imported paths with the listed managed identity before folding state", async () => {
    const api = transport();
    const managed = { ...root, path: "/code/managed", branch: "ferryx/ferryx/managed" };
    const canonical = { ...saved, worktreeKey: "w:ferryx/managed", worktreePath: managed.path };
    api.reconcileIdentities = vi.fn(async () => ({ sections: [{ workspaceId: "ferryx", rows: [canonical] }], revision: 1 }));
    setLaneLifecycleTransport(api);
    const { result, unmount } = renderHook(() => useWorkspaceStore({ workspaceId: "ferryx", initialWorktrees: [managed], services }));
    await act(async () => { await result.current.refreshLaneLifecycles(); });
    expect(api.reconcileIdentities).toHaveBeenCalledWith([{ worktreeKey: "w:ferryx/managed", workspaceId: "ferryx", path: managed.path }]);
    expect(api.read).not.toHaveBeenCalled();
    expect(result.current.state.laneLifecycle?.[canonical.worktreeKey]).toBe("pinned");
    expect(result.current.state.laneLifecycle?.["p:ferryx/code/managed"]).toBeUndefined();
    unmount();
  });

  it("surfaces a failed lifecycle read instead of silently pretending success", async () => {
    const api = transport();
    api.read = vi.fn(async () => { throw new Error("hub unavailable"); });
    setLaneLifecycleTransport(api);
    const errorToast = vi.spyOn(toast, "error");
    vi.spyOn(console, "error").mockImplementation(() => {});
    const { result, unmount } = renderHook(() => useWorkspaceStore({ workspaceId: "ferryx", initialWorktrees: [root], services }));
    await act(async () => { await result.current.refreshLaneLifecycles(); });
    expect(errorToast).toHaveBeenCalledTimes(1);
    // Repeated reads while still failing do not spam duplicate toasts.
    await act(async () => { await result.current.refreshLaneLifecycles(); });
    expect(errorToast).toHaveBeenCalledTimes(1);
    // Once a read succeeds, a subsequent failure notifies again.
    api.read = vi.fn(async () => ({ sections: [], revision: 0 }));
    await act(async () => { await result.current.refreshLaneLifecycles(); });
    api.read = vi.fn(async () => { throw new Error("transient failure"); });
    await act(async () => { await result.current.refreshLaneLifecycles(); });
    expect(errorToast).toHaveBeenCalledTimes(2);
    unmount();
  });

  it("keeps local sidebar order quietly on SUPERVISOR_STANDBY and SUPERVISOR_UNAVAILABLE", async () => {
    const api = transport();
    api.read = vi.fn(async () => {
      throw { code: "SUPERVISOR_STANDBY", message: "another machine runs the supervisor" };
    });
    setLaneLifecycleTransport(api);
    const errorToast = vi.spyOn(toast, "error");
    const { result, unmount } = renderHook(() => useWorkspaceStore({ workspaceId: "ferryx", initialWorktrees: [root], services }));
    await act(async () => { await result.current.refreshLaneLifecycles(); });
    expect(api.read).toHaveBeenCalled();
    expect(errorToast).not.toHaveBeenCalled();

    api.read = vi.fn(async () => {
      throw { code: "SUPERVISOR_UNAVAILABLE", message: "the supervisor hub is not open" };
    });
    await act(async () => { await result.current.refreshLaneLifecycles(); });
    expect(api.read).toHaveBeenCalled();
    expect(errorToast).not.toHaveBeenCalled();
    unmount();
  });
});
