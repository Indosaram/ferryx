import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab, TerminalSession, TerminalTab, Worktree } from "../lib/types";
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

function terminalSession(id: string, cwd: string): TerminalSession {
  return {
    id,
    cwd,
    worktreePath: cwd,
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "target" },
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

function groupForTab(state: WorkspaceState, tabId: string) {
  return Object.values(state.layout.tabGroups ?? {}).find((group) => group.tabIds.includes(tabId));
}

describe("useWorkspaceStore browser tab to terminal pane drop", () => {
  it("splits the browser tab into an adjacent tab group without merging into terminal pane layout", () => {
    const workspaceServices = services();
    const terminalTargetTab: TerminalTab = {
      kind: "terminal",
      id: "terminal-target",
      label: "Terminal Target",
      sessionId: "session-target",
    };

    const browserSourceTab: BrowserTab = {
      kind: "browser",
      id: "browser-source",
      label: "Browser Source",
      browserId: "browser-1",
      url: "https://example.com",
    };

    const initialState: WorkspaceState = {
      worktrees: [targetWorktree],
      activeWorktreePath: targetWorktree.path,
      sessions: {
        "session-target": terminalSession("session-target", targetWorktree.path),
      },
      layout: {
        tabs: [terminalTargetTab, browserSourceTab],
        activeTabId: "terminal-target",
        tabGroups: {
          "group-default": {
            id: "group-default",
            tabIds: [terminalTargetTab.id, browserSourceTab.id],
            activeTabId: terminalTargetTab.id,
          },
        },
        tabGroupLayout: { type: "group", groupId: "group-default" },
        focusedGroupId: "group-default",
        layoutsByTabId: {
          [terminalTargetTab.id]: {
            root: { type: "leaf", leafId: "leaf-target" },
            activeLeafId: "leaf-target",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-target": "session-target" },
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [targetWorktree], services: workspaceServices }),
    );

    act(() => result.current.restoreWorkspace(initialState));

    const targetGroupId = "group-default";
    act(() =>
      result.current.moveTabToSplit("browser-source", targetGroupId, "horizontal", "second"),
    );

    // Tab membership assertions: browser is moved into its own tab group split
    const targetGroup = groupForTab(result.current.state, "terminal-target");
    const sourceGroup = groupForTab(result.current.state, "browser-source");

    expect(targetGroup).toBeDefined();
    expect(sourceGroup).toBeDefined();
    expect(sourceGroup?.id).not.toBe(targetGroup?.id);
    expect(targetGroup?.tabIds).toEqual(["terminal-target"]);
    expect(sourceGroup?.tabIds).toEqual(["browser-source"]);

    // Two-layer tab group layout split
    expect(result.current.state.layout.tabGroupLayout).toMatchObject({
      type: "split",
      direction: "horizontal",
      first: { type: "group", groupId: targetGroup?.id },
      second: { type: "group", groupId: sourceGroup?.id },
    });

    // Browser tab remains exactly once in the workspace and retains metadata
    const browserTabs = result.current.state.layout.tabs.filter((tab) => tab.id === "browser-source");
    expect(browserTabs).toHaveLength(1);
    expect(browserTabs[0]).toEqual(browserSourceTab);

    // Terminal target pane layout remains unchanged (NOT merged into TabPaneLayout)
    const targetLayout = result.current.state.layout.layoutsByTabId["terminal-target"];
    expect(targetLayout.root).toEqual({ type: "leaf", leafId: "leaf-target" });
    expect(targetLayout.sessionIdsByLeafId).toEqual({ "leaf-target": "session-target" });

    // Focus / active state
    expect(result.current.state.layout.activeTabId).toBe("browser-source");
    expect(result.current.state.layout.focusedGroupId).toBe(sourceGroup?.id);

    // Backend / PTY assertions: no spawning or killing terminals
    expect(result.current.state.sessions["session-target"]).toBeDefined();
    expect(workspaceServices.spawnTerminal).not.toHaveBeenCalled();
    expect(workspaceServices.closeTerminal).not.toHaveBeenCalled();
  });

  it("merges a dropped top-level browser tab into target terminal pane leaf with browser metadata", () => {
    const workspaceServices = services();
    const terminalTargetTab: TerminalTab = {
      kind: "terminal",
      id: "terminal-target",
      label: "Terminal Target",
      sessionId: "session-target",
    };

    const browserSourceTab: BrowserTab = {
      kind: "browser",
      id: "browser-source",
      label: "Browser Source",
      browserId: "browser-1",
      url: "https://example.com",
      title: "Example Domain",
      profileId: "default",
    };

    const initialState: WorkspaceState = {
      worktrees: [targetWorktree],
      activeWorktreePath: targetWorktree.path,
      sessions: {
        "session-target": terminalSession("session-target", targetWorktree.path),
      },
      layout: {
        tabs: [terminalTargetTab, browserSourceTab],
        activeTabId: "terminal-target",
        tabGroups: {
          "group-default": {
            id: "group-default",
            tabIds: [terminalTargetTab.id, browserSourceTab.id],
            activeTabId: terminalTargetTab.id,
          },
        },
        tabGroupLayout: { type: "group", groupId: "group-default" },
        focusedGroupId: "group-default",
        layoutsByTabId: {
          [terminalTargetTab.id]: {
            root: { type: "leaf", leafId: "leaf-target" },
            activeLeafId: "leaf-target",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-target": "session-target" },
            contentsByLeafId: {
              "leaf-target": { kind: "terminal", sessionId: "session-target" },
            },
          } as any,
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [targetWorktree], services: workspaceServices }),
    );

    act(() => result.current.restoreWorkspace(initialState));

    act(() =>
      result.current.moveTabToSplit("browser-source", "group-default", "horizontal", "second", {
        tabId: "terminal-target",
        leafId: "leaf-target",
      }),
    );

    // 1. Browser source tab removed from layout.tabs and source group
    expect(result.current.state.layout.tabs.map((tab) => tab.id)).toEqual(["terminal-target"]);
    expect(result.current.state.layout.tabs.find((tab) => tab.id === "browser-source")).toBeUndefined();
    expect(result.current.state.layout.tabGroups?.["group-default"].tabIds).toEqual(["terminal-target"]);

    // 2. Target group's tabGroupLayout is unchanged and no new tab group exists
    expect(result.current.state.layout.tabGroupLayout).toEqual({
      type: "group",
      groupId: "group-default",
    });
    expect(Object.keys(result.current.state.layout.tabGroups ?? {})).toEqual(["group-default"]);

    // 3. Target terminal tab's pane tree gains a new leaf at exact direction/position
    const targetLayout = result.current.state.layout.layoutsByTabId["terminal-target"];
    expect(targetLayout).toBeDefined();
    expect(targetLayout.root).toMatchObject({
      type: "split",
      direction: "horizontal",
      first: { type: "leaf", leafId: "leaf-target" },
      second: { type: "leaf", leafId: expect.any(String) },
      ratio: 0.5,
    });

    const newLeafId = (targetLayout.root as any).second.leafId;
    expect(newLeafId).toBeDefined();
    expect(newLeafId).not.toBe("leaf-target");

    // 4. Target layout owns browser content with browserId/url/title/profile metadata
    expect((targetLayout as any).contentsByLeafId).toBeDefined();
    expect((targetLayout as any).contentsByLeafId[newLeafId]).toEqual({
      kind: "browser",
      browser: expect.objectContaining({
        browserId: "browser-1",
        url: "https://example.com",
        title: "Example Domain",
        profileId: "default",
      }),
    });
    expect((targetLayout as any).contentsByLeafId["leaf-target"]).toEqual({
      kind: "terminal",
      sessionId: "session-target",
    });

    // 5. Browser ID appears exactly once across entire layout
    const browserTabs = result.current.state.layout.tabs.filter(
      (tab) => (tab as any).browserId === "browser-1",
    );
    expect(browserTabs).toHaveLength(0);

    const matchingBrowserLeaves: any[] = [];
    for (const layout of Object.values(result.current.state.layout.layoutsByTabId)) {
      const contents = (layout as any).contentsByLeafId ?? {};
      for (const content of Object.values(contents) as any[]) {
        if (
          content?.kind === "browser" &&
          (content.browser?.browserId === "browser-1" || content.browserId === "browser-1")
        ) {
          matchingBrowserLeaves.push(content);
        }
      }
    }
    expect(matchingBrowserLeaves).toHaveLength(1);

    // Terminal PTY session preserved without spawning or closing
    expect(result.current.state.sessions["session-target"]).toBeDefined();
    expect(workspaceServices.spawnTerminal).not.toHaveBeenCalled();
    expect(workspaceServices.closeTerminal).not.toHaveBeenCalled();
  });

});
