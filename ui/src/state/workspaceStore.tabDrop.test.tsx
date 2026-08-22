import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab, TerminalSession, TerminalTab, Worktree } from "../lib/types";
import { useWorkspaceStore, type WorkspaceServices, type WorkspaceState } from "./workspaceStore";

afterEach(cleanup);

const targetWorktree: Worktree = {
  path: "/repo/target",
  head: "target-head",
  branch: "refs/heads/orca/ws-main/target",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const sourceWorktree: Worktree = {
  ...targetWorktree,
  path: "/repo/source",
  head: "source-head",
  branch: "refs/heads/orca/ws-main/source",
};

function session(id: string, cwd: string, slug: string): TerminalSession {
  return {
    id,
    cwd,
    worktreePath: cwd,
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug },
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
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

function restoredState(activeTabId = "tab-target"): WorkspaceState {
  const targetTab: TerminalTab = { id: "tab-target", label: "target", sessionId: "session-target" };
  const sourceTab: TerminalTab = { id: "tab-source", label: "source", sessionId: "session-source-primary" };
  return {
    worktrees: [targetWorktree, sourceWorktree],
    activeWorktreePath: targetWorktree.path,
    sessions: {
      "session-target": session("session-target", targetWorktree.path, "target"),
      "session-source-primary": session("session-source-primary", sourceWorktree.path, "source"),
      "session-source-active": session("session-source-active", sourceWorktree.path, "source"),
    },
    layout: {
      tabs: [targetTab, sourceTab],
      activeTabId,
      tabGroups: {
        "group-default": {
          id: "group-default",
          tabIds: [targetTab.id, sourceTab.id],
          activeTabId,
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
            ratio: 0.5,
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
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  };
}

function expectAllSessionsPreserved(state: WorkspaceState) {
  expect(state.sessions["session-target"]).toBeDefined();
  expect(state.sessions["session-source-primary"]).toBeDefined();
  expect(state.sessions["session-source-active"]).toBeDefined();
}

function groupForTab(state: WorkspaceState, tabId: string) {
  return Object.values(state.layout.tabGroups ?? {}).find((group) => group.tabIds.includes(tabId));
}

describe("useWorkspaceStore Orca-style tab ownership moves", () => {
  it("moves a dragged tab into a new split group while preserving its pane tree and PTYs", () => {
    const workspaceServices = services();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [targetWorktree, sourceWorktree], services: workspaceServices }),
    );

    act(() => result.current.restoreWorkspace(restoredState()));
    const targetGroupId = groupForTab(result.current.state, "tab-target")!.id;
    act(() => result.current.moveTabToSplit("tab-source", targetGroupId, "horizontal", "first"));

    expect(new Set(result.current.state.layout.tabs.map((tab) => tab.id))).toEqual(new Set(["tab-target", "tab-source"]));
    expect(result.current.state.layout.activeTabId).toBe("tab-source");
    expect(result.current.state.layout.focusedGroupId).toBe(groupForTab(result.current.state, "tab-source")?.id);

    const targetLayout = result.current.state.layout.layoutsByTabId["tab-target"];
    const sourceLayout = result.current.state.layout.layoutsByTabId["tab-source"];
    expect(targetLayout.root).toEqual({ type: "leaf", leafId: "leaf-target" });
    expect(sourceLayout.root).toEqual({
      type: "split",
      direction: "vertical",
      first: { type: "leaf", leafId: "leaf-source-primary" },
      second: { type: "leaf", leafId: "leaf-source-active" },
      ratio: 0.5,
    });
    expect(sourceLayout.sessionIdsByLeafId).toEqual({
      "leaf-source-primary": "session-source-primary",
      "leaf-source-active": "session-source-active",
    });

    const targetGroup = groupForTab(result.current.state, "tab-target");
    const sourceGroup = groupForTab(result.current.state, "tab-source");
    expect(sourceGroup?.id).not.toBe(targetGroup?.id);
    expect(sourceGroup?.tabIds).toEqual(["tab-source"]);
    expect(result.current.state.layout.tabGroupLayout).toMatchObject({
      type: "split",
      direction: "horizontal",
      first: { type: "group", groupId: sourceGroup?.id },
      second: { type: "group", groupId: targetGroup?.id },
    });

    expectAllSessionsPreserved(result.current.state);
    expect(workspaceServices.spawnTerminal).not.toHaveBeenCalled();
    expect(workspaceServices.closeTerminal).not.toHaveBeenCalled();
  });

  it("merges a split tab back into a target group at an explicit index without touching its PTYs", () => {
    const workspaceServices = services();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [targetWorktree, sourceWorktree], services: workspaceServices }),
    );
    act(() => result.current.restoreWorkspace(restoredState()));

    const initialGroupId = groupForTab(result.current.state, "tab-target")!.id;
    act(() => result.current.moveTabToSplit("tab-source", initialGroupId, "horizontal", "second"));
    const targetGroupId = groupForTab(result.current.state, "tab-target")!.id;
    act(() => result.current.moveTabToGroup("tab-source", targetGroupId, 0));

    const merged = groupForTab(result.current.state, "tab-source");
    expect(merged?.id).toBe(targetGroupId);
    expect(merged?.tabIds).toEqual(["tab-source", "tab-target"]);
    expect(result.current.state.layout.tabGroupLayout).toEqual({ type: "group", groupId: targetGroupId });
    expect(result.current.state.layout.focusedGroupId).toBe(targetGroupId);
    expect(result.current.state.layout.activeTabId).toBe("tab-source");
    expectAllSessionsPreserved(result.current.state);
    expect(workspaceServices.closeTerminal).not.toHaveBeenCalled();
  });

  it("moves browser tabs through the same tab-group split and merge reducer path", () => {
    const workspaceServices = services();
    const state = restoredState();
    const browser: BrowserTab = {
      kind: "browser",
      id: "tab-browser",
      label: "Browser",
      browserId: "browser-1",
      url: "http://localhost:3000",
    };
    state.layout.tabs.push(browser);
    state.layout.tabGroups!["group-default"].tabIds.push(browser.id);
    state.layout.layoutsByTabId[browser.id] = {
      root: { type: "leaf", leafId: "leaf-browser" },
      activeLeafId: "leaf-browser",
      expandedLeafId: null,
      sessionIdsByLeafId: { "leaf-browser": "" },
    };
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [targetWorktree, sourceWorktree], services: workspaceServices }),
    );
    act(() => result.current.restoreWorkspace(state));

    const targetGroupId = groupForTab(result.current.state, "tab-target")!.id;
    act(() => result.current.moveTabToSplit("tab-browser", targetGroupId, "vertical", "second"));
    expect(groupForTab(result.current.state, "tab-browser")?.id).not.toBe(targetGroupId);

    act(() => result.current.moveTabToGroup("tab-browser", targetGroupId));
    expect(groupForTab(result.current.state, "tab-browser")?.id).toBe(targetGroupId);
    expect(result.current.state.layout.tabs.find((tab) => tab.id === "tab-browser")?.kind).toBe("browser");
    expect(workspaceServices.spawnTerminal).not.toHaveBeenCalled();
  });

  it("detaches one terminal pane into a new tab by moving the same local/backend session ownership", () => {
    const workspaceServices = services();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [targetWorktree, sourceWorktree], services: workspaceServices }),
    );
    act(() => result.current.restoreWorkspace(restoredState("tab-source")));

    let detachedTabId: string | null = null;
    act(() => {
      detachedTabId = result.current.detachPaneToTab("tab-source", "leaf-source-active");
    });

    expect(detachedTabId).toBeTruthy();
    const sourceLayout = result.current.state.layout.layoutsByTabId["tab-source"];
    expect(sourceLayout.root).toEqual({ type: "leaf", leafId: "leaf-source-primary" });
    expect(sourceLayout.sessionIdsByLeafId).toEqual({ "leaf-source-primary": "session-source-primary" });

    const detachedLayout = result.current.state.layout.layoutsByTabId[detachedTabId!];
    expect(detachedLayout.root).toEqual({ type: "leaf", leafId: "leaf-source-active" });
    expect(detachedLayout.sessionIdsByLeafId).toEqual({ "leaf-source-active": "session-source-active" });
    const detachedTab = result.current.state.layout.tabs.find((tab) => tab.id === detachedTabId);
    expect(detachedTab?.kind).not.toBe("browser");
    if (detachedTab?.kind !== "browser") expect(detachedTab?.sessionId).toBe("session-source-active");

    expect(result.current.state.sessions["session-source-active"].backendSessionId).toBe("backend-session-source-active");
    expect(Object.keys(result.current.state.sessions)).toHaveLength(3);
    expect(workspaceServices.spawnTerminal).not.toHaveBeenCalled();
    expect(workspaceServices.closeTerminal).not.toHaveBeenCalled();
  });
});
