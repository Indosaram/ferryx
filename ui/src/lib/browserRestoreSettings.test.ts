import { beforeEach, describe, expect, it } from "vitest";

import type { WorkspaceState } from "../state/workspaceStore";
import { saveBrowserSettings } from "./browserSettings";
import { deserializeWorkspaceState, serializeWorkspaceState } from "./sessionPersistence";

function stateWithBrowser(): WorkspaceState {
  return {
    worktrees: [{
      path: "/workspace/main",
      head: "abc",
      branch: "main",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    }],
    activeWorktreePath: "/workspace/main",
    sessions: {
      terminal: {
        id: "terminal",
        cwd: "/workspace/main",
        worktreePath: "/workspace/main",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "backend-terminal",
        lifecycle: "working",
      },
    },
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
    layout: {
      tabs: [
        { id: "tab-terminal", label: "Terminal", sessionId: "terminal" },
        {
          id: "tab-browser",
          kind: "browser",
          label: "Docs",
          browserId: "browser-persisted",
          url: "http://localhost:3000/docs",
          title: "Docs",
          loading: false,
          canGoBack: false,
          canGoForward: false,
          profileId: "work",
          worktreePath: "/workspace/main",
          worktreeLabel: "feature/login",
        },
      ],
      activeTabId: "tab-browser",
      primaryTabId: "tab-terminal",
      secondaryTabId: null,
      split: "none",
      nestedSplit: null,
      layoutsByTabId: {
        "tab-terminal": {
          root: { type: "leaf", leafId: "leaf-terminal" },
          activeLeafId: "leaf-terminal",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-terminal": "terminal" },
        },
        "tab-browser": {
          root: { type: "leaf", leafId: "leaf-browser" },
          activeLeafId: "leaf-browser",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-browser": "" },
        },
      },
    },
  };
}

beforeEach(() => localStorage.clear());

describe("browser restore setting persistence policy", () => {
  it("omits browser tabs from session persistence when restore is disabled", () => {
    saveBrowserSettings({ restoreTabsOnLaunch: false });
    const persisted = serializeWorkspaceState("default", "/workspace/main", stateWithBrowser());
    expect(persisted.workspaces.default.layout.tabs.map((tab) => tab.id)).toEqual(["tab-terminal"]);
    expect(persisted.workspaces.default.layout.activeTabId).toBe("tab-terminal");
  });

  it("preserves profile and worktree metadata when restore is enabled", () => {
    saveBrowserSettings({
      restoreTabsOnLaunch: true,
      profiles: [
        { id: "default", name: "Default" },
        { id: "work", name: "Work" },
      ],
      defaultProfileId: "work",
    });
    const persisted = serializeWorkspaceState("default", "/workspace/main", stateWithBrowser());
    const browser = persisted.workspaces.default.layout.tabs.find((tab) => tab.id === "tab-browser");
    expect(browser).toMatchObject({
      kind: "browser",
      browser: {
        browserId: "browser-persisted",
        url: "http://localhost:3000/docs",
        profileId: "work",
        worktreePath: "/workspace/main",
        worktreeLabel: "feature/login",
      },
    });

    const restored = deserializeWorkspaceState("default", persisted, ["backend-terminal"]);
    expect(restored?.layout.tabs.find((tab) => tab.id === "tab-browser")).toMatchObject({
      kind: "browser",
      profileId: "work",
      worktreePath: "/workspace/main",
      worktreeLabel: "feature/login",
    });
  });

  it("falls a persisted tab back to the current default when its profile was deleted", () => {
    saveBrowserSettings({
      restoreTabsOnLaunch: true,
      profiles: [
        { id: "default", name: "Default" },
        { id: "work", name: "Work" },
      ],
      defaultProfileId: "work",
    });
    const persisted = serializeWorkspaceState("default", "/workspace/main", stateWithBrowser());

    saveBrowserSettings({
      restoreTabsOnLaunch: true,
      profiles: [
        { id: "default", name: "Default" },
        { id: "personal", name: "Personal" },
      ],
      defaultProfileId: "personal",
    });
    const restored = deserializeWorkspaceState("default", persisted, ["backend-terminal"]);
    expect(restored?.layout.tabs.find((tab) => tab.id === "tab-browser")).toMatchObject({
      kind: "browser",
      profileId: "personal",
    });
  });

  it("honors the current restore preference when reading an older session", () => {
    saveBrowserSettings({ restoreTabsOnLaunch: true });
    const persisted = serializeWorkspaceState("default", "/workspace/main", stateWithBrowser());
    expect(persisted.workspaces.default.layout.tabs.some((tab) => tab.kind === "browser")).toBe(true);

    saveBrowserSettings({ restoreTabsOnLaunch: false });
    const restored = deserializeWorkspaceState("default", persisted, ["backend-terminal"]);
    expect(restored?.layout.tabs.map((tab) => tab.id)).toEqual(["tab-terminal"]);
    expect(restored?.layout.activeTabId).toBe("tab-terminal");
    expect(restored?.layout.layoutsByTabId["tab-browser"]).toBeUndefined();
  });
});
