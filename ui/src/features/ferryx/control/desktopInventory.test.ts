import { describe, expect, it } from "vitest";
import {
  buildDesktopInventory,
  isUnreadAgent,
  resolveLocalSessionKey,
  type DesktopWorkspace,
} from "./desktopInventory";
import type { WorkspaceState } from "../../../state/workspaceStore";

const session = (id: string, backendSessionId: string | null, lifecycle: string) => ({
  id,
  cwd: "/repo",
  workspaceId: "ws-1",
  worktree: null,
  backendSessionId,
  lifecycle,
});

function workspace(): DesktopWorkspace {
  const state = {
    workspaceId: "ws-1",
    worktrees: [],
    activeWorktreePath: null,
    sessions: {
      "s-1": session("s-1", "b-1", "working"),
      "s-2": session("s-2", "b-2", "exited"),
      "s-3": session("s-3", null, "working"),
    },
    layout: {
      tabs: [{ id: "t-1", sessionId: "s-1", label: "Agent A" }],
      layoutsByTabId: {},
    },
    activityBySessionId: {
      "s-1": { state: "waiting", seen: false },
      "s-2": { state: "done", seen: true },
    },
  } as unknown as WorkspaceState;
  return { workspaceId: "ws-1", hostId: "local", state };
}

describe("buildDesktopInventory", () => {
  it("emits one row per session that has activity and skips the rest", () => {
    const snapshot = buildDesktopInventory([workspace()]);
    expect(snapshot.items.map((item) => item.target.backendSessionId)).toEqual(["b-1", "b-2"]);
    expect(snapshot.completeness).toBe("complete");
    expect(snapshot.unavailableHosts).toEqual([]);
  });

  it("labels a row from its owning tab and falls back to the session id", () => {
    const snapshot = buildDesktopInventory([workspace()]);
    const labels = Object.fromEntries(snapshot.items.map((item) => [item.target.backendSessionId, item.label]));
    expect(labels).toEqual({ "b-1": "Agent A", "b-2": "s-2" });
  });

  it("keeps waiting rows first, reports an exited lifecycle, and numbers revisions from 1", () => {
    const snapshot = buildDesktopInventory([workspace()]);
    expect(snapshot.items[0].state).toBe("waiting");
    expect(snapshot.items[1].state).toBe("exited");
    expect(snapshot.items.map((item) => item.revision)).toEqual([1, 2]);
    expect(snapshot.revision).toBe(2);
    expect(snapshot.items[0].target).toEqual({ hostId: "local", ownerId: "ws-1", epoch: "0", backendSessionId: "b-1" });
  });

  it("falls back to the local session id when backendSessionId is null", () => {
    const base = workspace();
    const withNullBackend = {
      ...base,
      state: {
        ...base.state,
        activityBySessionId: { ...base.state.activityBySessionId, "s-3": { state: "working", seen: true } },
      } as unknown as WorkspaceState,
    };
    const snapshot = buildDesktopInventory([withNullBackend]);
    expect(snapshot.items.some((item) => item.target.backendSessionId === "s-3")).toBe(true);
  });

  it("yields completeness partial and unavailableHosts when unavailableHosts is non-empty", () => {
    const spaces = [workspace()];
    const snapshot = buildDesktopInventory(spaces, ["host-a"]);
    expect(snapshot.completeness).toBe("partial");
    expect(snapshot.unavailableHosts).toEqual(["host-a"]);
  });
});

describe("resolveLocalSessionKey", () => {
  it("returns the local session key for an agent row and null for an unknown workspace", () => {
    const spaces = [workspace()];
    const snapshot = buildDesktopInventory(spaces);
    const agentB2 = snapshot.items.find((item) => item.target.backendSessionId === "b-2")!;
    expect(resolveLocalSessionKey(agentB2, spaces)).toBe("s-2");
    expect(resolveLocalSessionKey(agentB2, [])).toBeNull();
  });
});

describe("isUnreadAgent", () => {
  it("is true only for agents whose activity has not been seen", () => {
    const spaces = [workspace()];
    const snapshot = buildDesktopInventory(spaces);
    const unread = snapshot.items.filter((item) => isUnreadAgent(item, spaces));
    expect(unread).toEqual([]);
  });

  it("returns true for a done and unseen agent", () => {
    const base = workspace();
    const withUnreadDone = {
      ...base,
      state: {
        ...base.state,
        sessions: {
          ...base.state.sessions,
          "s-4": session("s-4", "b-4", "working"),
        },
        activityBySessionId: {
          ...base.state.activityBySessionId,
          "s-4": { state: "done", seen: false },
        },
      } as unknown as WorkspaceState,
    };
    const spaces = [withUnreadDone];
    const snapshot = buildDesktopInventory(spaces);
    const unread = snapshot.items.filter((item) => isUnreadAgent(item, spaces));
    expect(unread.map((item) => item.target.backendSessionId)).toEqual(["b-4"]);
  });

  it("is false for an agent whose workspace is unknown", () => {
    const snapshot = buildDesktopInventory([workspace()]);
    expect(isUnreadAgent(snapshot.items[0], [])).toBe(false);
  });
});

