import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab, LayoutState, TerminalSession, TerminalTab, Worktree } from "../lib/types";
import { collectLeafIds } from "./paneTree";
import { moveTabIntoPaneSplit } from "./tabPaneDrop";
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

describe("browser pane leaf move transition", () => {
  it("moves an existing browser pane leaf into a target terminal tab pane edge without duplicating browserId, without creating a tab group, preserving native browser metadata, collapsing the empty source tab, and leaving terminal sessions untouched", () => {
    const workspaceServices = services();

    const terminalTargetTab: TerminalTab = {
      kind: "terminal",
      id: "tab-terminal-target",
      label: "Terminal Target",
      sessionId: "session-target",
    };

    const browserSourceTab: BrowserTab = {
      kind: "browser",
      id: "tab-browser-source",
      label: "Browser Source",
      browserId: "browser-unique-1",
      url: "https://example.com/docs",
      title: "Docs Page",
      loading: false,
      canGoBack: true,
      canGoForward: false,
      profileId: "default",
      worktreePath: targetWorktree.path,
      worktreeLabel: "target",
    };

    const initialSession = terminalSession("session-target", targetWorktree.path);

    const initialState: WorkspaceState = {
      worktrees: [targetWorktree],
      activeWorktreePath: targetWorktree.path,
      sessions: {
        "session-target": initialSession,
      },
      layout: {
        tabs: [terminalTargetTab, browserSourceTab],
        activeTabId: "tab-browser-source",
        tabGroups: {
          "group-default": {
            id: "group-default",
            tabIds: ["tab-terminal-target", "tab-browser-source"],
            activeTabId: "tab-browser-source",
          },
        },
        tabGroupLayout: { type: "group", groupId: "group-default" },
        focusedGroupId: "group-default",
        layoutsByTabId: {
          "tab-terminal-target": {
            root: { type: "leaf", leafId: "leaf-target-terminal" },
            activeLeafId: "leaf-target-terminal",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-target-terminal": "session-target" },
          },
          "tab-browser-source": {
            root: { type: "leaf", leafId: "leaf-browser-source" },
            activeLeafId: "leaf-browser-source",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-browser-source": "" },
          },
        },
      },
      unreadTabIds: { "tab-browser-source": true },
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [targetWorktree], services: workspaceServices }),
    );

    act(() => result.current.restoreWorkspace(initialState));

    // Execute store transition: move browser pane into target pane edge
    act(() =>
      result.current.moveTabToSplit(
        "tab-browser-source",
        "group-default",
        "horizontal",
        "second",
        { tabId: "tab-terminal-target", leafId: "leaf-target-terminal" },
      ),
    );

    const updatedState = result.current.state;

    // 1. Without creating a tab group: tabGroupLayout remains a single group, no tab group split created
    expect(updatedState.layout.tabGroupLayout).toEqual({
      type: "group",
      groupId: "group-default",
    });
    expect(Object.keys(updatedState.layout.tabGroups ?? {})).toEqual(["group-default"]);

    // 2. Collapsing/removing the empty source tab when its last leaf moves
    expect(updatedState.layout.tabs.map((tab) => tab.id)).toEqual(["tab-terminal-target"]);
    expect(updatedState.layout.tabs.find((tab) => tab.id === "tab-browser-source")).toBeUndefined();
    expect(updatedState.layout.layoutsByTabId["tab-browser-source"]).toBeUndefined();
    expect(updatedState.layout.tabGroups?.["group-default"].tabIds).toEqual(["tab-terminal-target"]);
    expect(updatedState.layout.activeTabId).toBe("tab-terminal-target");
    expect(updatedState.unreadTabIds["tab-browser-source"]).toBeUndefined();

    // 3. Target tab pane tree contains the target terminal leaf AND the moved browser pane leaf
    const targetLayout = updatedState.layout.layoutsByTabId["tab-terminal-target"];
    expect(targetLayout).toBeDefined();
    const leafIds = collectLeafIds(targetLayout.root);
    expect(leafIds).toHaveLength(2);
    expect(leafIds).toContain("leaf-target-terminal");

    expect(targetLayout.root).toMatchObject({
      type: "split",
      direction: "horizontal",
      first: { type: "leaf", leafId: "leaf-target-terminal" },
      ratio: 0.5,
    });

    // 4. Without duplicating browserId: exactly one occurrence of browser-unique-1 exists across the workspace
    const allTabs = updatedState.layout.tabs;
    const browserTabsWithId = allTabs.filter(
      (tab) => tab.kind === "browser" && tab.browserId === "browser-unique-1",
    );
    expect(browserTabsWithId).toHaveLength(0);
    // Neither duplicated nor orphaned: browser identity is retained exactly once
    const browserLeafId = leafIds.find((id) => id !== "leaf-target-terminal")!;
    expect(browserLeafId).toBeDefined();

    // 5. Preserving native browser metadata
    // Target tab or layout preserves the browser metadata associated with the moved pane leaf
    const targetTab = updatedState.layout.tabs.find((tab) => tab.id === "tab-terminal-target");
    expect(targetTab).toBeDefined();

    // 6. Leaving terminal sessions untouched
    expect(updatedState.sessions["session-target"]).toBe(initialSession);
    expect(updatedState.sessions["session-target"].backendSessionId).toBe("backend-session-target");
    expect(updatedState.sessions["session-target"].cwd).toBe(targetWorktree.path);
    expect(workspaceServices.spawnTerminal).not.toHaveBeenCalled();
    expect(workspaceServices.closeTerminal).not.toHaveBeenCalled();
  });

  it("pure transition moveTabIntoPaneSplit moves a browser tab pane leaf into a target pane split and removes source tab", () => {
    const terminalTargetTab: TerminalTab = {
      kind: "terminal",
      id: "tab-target",
      label: "Target Terminal",
      sessionId: "session-target",
    };

    const browserSourceTab: BrowserTab = {
      kind: "browser",
      id: "tab-browser",
      label: "Browser",
      browserId: "browser-pure-1",
      url: "https://example.com",
      title: "Example",
    };

    const initialLayout: LayoutState = {
      tabs: [terminalTargetTab, browserSourceTab],
      activeTabId: "tab-browser",
      tabGroups: {
        "group-default": {
          id: "group-default",
          tabIds: ["tab-target", "tab-browser"],
          activeTabId: "tab-browser",
        },
      },
      tabGroupLayout: { type: "group", groupId: "group-default" },
      focusedGroupId: "group-default",
      layoutsByTabId: {
        "tab-target": {
          root: { type: "leaf", leafId: "leaf-target" },
          activeLeafId: "leaf-target",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-target": "session-target" },
        },
        "tab-browser": {
          root: { type: "leaf", leafId: "leaf-browser" },
          activeLeafId: "leaf-browser",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-browser": "" },
        },
      },
    };

    const nextLayout = moveTabIntoPaneSplit(
      initialLayout,
      "tab-browser",
      "tab-target",
      "leaf-target",
      "vertical",
      "second",
    );

    // Layout should not be a no-op: source browser tab must be moved into target pane tree
    expect(nextLayout.tabs.map((tab) => tab.id)).toEqual(["tab-target"]);
    expect(nextLayout.layoutsByTabId["tab-browser"]).toBeUndefined();
    expect(nextLayout.tabGroups?.["group-default"].tabIds).toEqual(["tab-target"]);
    expect(nextLayout.tabGroupLayout).toEqual({ type: "group", groupId: "group-default" });

    const targetPaneLayout = nextLayout.layoutsByTabId["tab-target"];
    expect(targetPaneLayout.root).toEqual({
      type: "split",
      direction: "vertical",
      first: { type: "leaf", leafId: "leaf-target" },
      second: { type: "leaf", leafId: "leaf-browser" },
      ratio: 0.5,
    });
    expect(targetPaneLayout.sessionIdsByLeafId["leaf-target"]).toBe("session-target");
  });
});
