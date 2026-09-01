import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { TerminalSession, TerminalTab, Worktree } from "../lib/types";
import { useWorkspaceStore, type WorkspaceServices, type WorkspaceState } from "./workspaceStore";

afterEach(cleanup);

const targetWorktree: Worktree = {
  path: "/repo/target",
  head: "target-head",
  branch: "refs/heads/target",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const sourceWorktree: Worktree = {
  ...targetWorktree,
  path: "/repo/source",
  head: "source-head",
  branch: "refs/heads/source",
};

function terminalSession(
  id: string,
  cwd: string,
  agentType?: string,
  agentSessionId?: string,
): TerminalSession {
  return {
    id,
    cwd,
    worktreePath: cwd,
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: cwd.endsWith("source") ? "source" : "target" },
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
    agentType: agentType ?? null,
    agentSessionId: agentSessionId ?? null,
  };
}

function services(): WorkspaceServices {
  return {
    ensureTerminalEvents: vi.fn(async () => undefined),
    spawnTerminal: vi.fn(async () => "unexpected-backend"),
    getTerminalCwd: vi.fn(async () => null),
    closeTerminal: vi.fn(async () => undefined),
    waitForTerminalExit: vi.fn(async () => undefined),
  };
}

function stateWithSplitSource(): WorkspaceState {
  const targetTab: TerminalTab = { id: "tab-target", label: "target", sessionId: "session-target" };
  const sourceTab: TerminalTab = { id: "tab-source", label: "source", sessionId: "session-source-primary" };
  return {
    worktrees: [targetWorktree, sourceWorktree],
    activeWorktreePath: sourceWorktree.path,
    sessions: {
      "session-target": terminalSession("session-target", targetWorktree.path),
      "session-source-primary": terminalSession("session-source-primary", sourceWorktree.path),
      "session-source-active": terminalSession(
        "session-source-active",
        sourceWorktree.path,
        "claude",
        "claude-session-rep-1",
      ),
    },
    layout: {
      tabs: [targetTab, sourceTab],
      activeTabId: "tab-source",
      tabGroups: {
        "group-default": {
          id: "group-default",
          tabIds: [targetTab.id, sourceTab.id],
          activeTabId: sourceTab.id,
        },
      },
      tabGroupLayout: { type: "group", groupId: "group-default" },
      focusedGroupId: "group-default",
      layoutsByTabId: {
        [targetTab.id]: {
          root: { type: "leaf", leafId: "leaf-target" },
          activeLeafId: "leaf-target",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-target": "session-target" },
        },
        [sourceTab.id]: {
          root: {
            type: "split",
            direction: "vertical",
            first: { type: "leaf", leafId: "leaf-source-primary" },
            second: { type: "leaf", leafId: "leaf-source-active" },
            ratio: 0.35,
          },
          activeLeafId: "leaf-source-active",
          expandedLeafId: null,
          sessionIdsByLeafId: {
            "leaf-source-primary": "session-source-primary",
            "leaf-source-active": "session-source-active",
          },
        },
      },
    },
    unreadTabIds: { "tab-source": true },
    unreadWorktreePaths: {},
    activityBySessionId: {},
  };
}

function collectLeafIds(node: WorkspaceState["layout"]["layoutsByTabId"][string]["root"]): string[] {
  if (node.type === "leaf") return [node.leafId];
  return [...collectLeafIds(node.first), ...collectLeafIds(node.second)];
}

describe("useWorkspaceStore pane-targeted tab drop", () => {
  it("moves the existing source pane subtree into the exact target leaf without respawning or closing PTYs", () => {
    const workspaceServices = services();
    const restored = stateWithSplitSource();
    const sourcePrimary = restored.sessions["session-source-primary"];
    const sourceActive = restored.sessions["session-source-active"];
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [targetWorktree, sourceWorktree], services: workspaceServices }),
    );

    act(() => result.current.restoreWorkspace(restored));
    act(() =>
      result.current.moveTabToSplit("tab-source", "group-default", "horizontal", "first", {
        tabId: "tab-target",
        leafId: "leaf-target",
      }),
    );

    expect(result.current.state.layout.tabs.map((tab) => tab.id)).toEqual(["tab-target"]);
    expect(result.current.state.layout.layoutsByTabId["tab-source"]).toBeUndefined();
    expect(result.current.state.layout.activeTabId).toBe("tab-target");
    expect(result.current.state.layout.focusedGroupId).toBe("group-default");
    expect(result.current.state.layout.tabGroups?.["group-default"].tabIds).toEqual(["tab-target"]);

    const targetLayout = result.current.state.layout.layoutsByTabId["tab-target"];
    expect(targetLayout.root).toEqual({
      type: "split",
      direction: "horizontal",
      first: {
        type: "split",
        direction: "vertical",
        first: { type: "leaf", leafId: "leaf-source-primary" },
        second: { type: "leaf", leafId: "leaf-source-active" },
        ratio: 0.35,
      },
      second: { type: "leaf", leafId: "leaf-target" },
      ratio: 0.5,
    });
    expect(targetLayout.activeLeafId).toBe("leaf-source-active");
    expect(targetLayout.sessionIdsByLeafId).toEqual({
      "leaf-target": "session-target",
      "leaf-source-primary": "session-source-primary",
      "leaf-source-active": "session-source-active",
    });

    expect(result.current.state.sessions["session-source-primary"]).toBe(sourcePrimary);
    expect(result.current.state.sessions["session-source-active"]).toBe(sourceActive);
    expect(result.current.state.sessions["session-source-active"].cwd).toBe(sourceWorktree.path);
    expect(result.current.state.sessions["session-source-active"].backendSessionId).toBe("backend-session-source-active");
    expect(result.current.state.sessions["session-source-active"].agentType).toBe("claude");
    expect(result.current.state.sessions["session-source-active"].agentSessionId).toBe("claude-session-rep-1");
    expect(result.current.state.unreadTabIds["tab-source"]).toBeUndefined();
    expect(workspaceServices.spawnTerminal).not.toHaveBeenCalled();
    expect(workspaceServices.closeTerminal).not.toHaveBeenCalled();
  });

  it("retains a freshly rebound backend when its pane subtree is transplanted", () => {
    const workspaceServices = services();
    const restored = stateWithSplitSource();
    restored.sessions["session-source-active"] = {
      ...restored.sessions["session-source-active"],
      backendSessionId: "backend-resumed-1",
      daemonEpoch: "epoch-resumed",
      lastOutputSequence: null,
      reconnectLifecycle: "idle",
    };
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [targetWorktree, sourceWorktree], services: workspaceServices }),
    );

    act(() => result.current.restoreWorkspace(restored));
    act(() =>
      result.current.moveTabToSplit("tab-source", "group-default", "horizontal", "first", {
        tabId: "tab-target",
        leafId: "leaf-target",
      }),
    );

    expect(result.current.state.sessions["session-source-active"]).toEqual(
      expect.objectContaining({
        backendSessionId: "backend-resumed-1",
        daemonEpoch: "epoch-resumed",
        lastOutputSequence: null,
        agentType: "claude",
        agentSessionId: "claude-session-rep-1",
      }),
    );
    expect(workspaceServices.spawnTerminal).not.toHaveBeenCalled();
    expect(workspaceServices.closeTerminal).not.toHaveBeenCalled();
  });

  it("remaps a colliding legacy leaf id while preserving the local session ownership", () => {
    const workspaceServices = services();
    const restored = stateWithSplitSource();
    restored.layout.layoutsByTabId["tab-target"] = {
      root: { type: "leaf", leafId: "leaf-shared" },
      activeLeafId: "leaf-shared",
      expandedLeafId: null,
      sessionIdsByLeafId: { "leaf-shared": "session-target" },
    };
    restored.layout.layoutsByTabId["tab-source"] = {
      root: { type: "leaf", leafId: "leaf-shared" },
      activeLeafId: "leaf-shared",
      expandedLeafId: null,
      sessionIdsByLeafId: { "leaf-shared": "session-source-primary" },
    };

    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [targetWorktree, sourceWorktree], services: workspaceServices }),
    );
    act(() => result.current.restoreWorkspace(restored));
    act(() =>
      result.current.moveTabToSplit("tab-source", "group-default", "vertical", "second", {
        tabId: "tab-target",
        leafId: "leaf-shared",
      }),
    );

    const targetLayout = result.current.state.layout.layoutsByTabId["tab-target"];
    const leafIds = collectLeafIds(targetLayout.root);
    expect(new Set(leafIds).size).toBe(2);
    expect(leafIds).toContain("leaf-shared");
    expect(Object.values(targetLayout.sessionIdsByLeafId)).toEqual(
      expect.arrayContaining(["session-target", "session-source-primary"]),
    );
    expect(targetLayout.sessionIdsByLeafId[targetLayout.activeLeafId!]).toBe("session-source-primary");
    expect(workspaceServices.spawnTerminal).not.toHaveBeenCalled();
    expect(workspaceServices.closeTerminal).not.toHaveBeenCalled();
  });
});
