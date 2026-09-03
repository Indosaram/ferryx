import { describe, expect, it } from "vitest";
import { getGroupForTab, layoutReducer, normalizeLayout } from "../state/layout";
import type { WorkspaceState } from "../state/workspaceStore";
import { saveBrowserSettings } from "./browserSettings";
import {
  deserializeWorkspaceState,
  migrateLegacyAgentType,
  serializeWorkspaceState,
  WORKSPACE_SESSION_VERSION,
} from "./sessionPersistence";

function workspaceState(): WorkspaceState {
  return {
    worktrees: [
      {
        path: "/workspace/main",
        head: "123456",
        branch: "main",
        bare: false,
        detached: false,
        locked: null,
        prunable: null,
      },
    ],
    activeWorktreePath: "/workspace/main",
    sessions: {
      "sess-1": {
        id: "sess-1",
        cwd: "/workspace/main/packages/api",
        worktreePath: "/workspace/main",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "backend-1",
        lifecycle: "working",
      },
      "sess-2": {
        id: "sess-2",
        cwd: "/workspace/main",
        worktreePath: "/workspace/main",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "backend-2",
        lifecycle: "working",
      },
      "sess-3": {
        id: "sess-3",
        cwd: "/workspace/main/packages/web",
        worktreePath: "/workspace/main",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "backend-3",
        lifecycle: "working",
      },
    },
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
    layout: {
      tabs: [
        { id: "tab-1", label: "main", sessionId: "sess-1", pinned: true },
        { id: "tab-2", label: "feature", sessionId: "sess-2" },
      ],
      primaryTabId: "tab-1",
      secondaryTabId: null,
      split: "horizontal",
      nestedSplit: null,
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-1" },
            second: { type: "leaf", leafId: "leaf-2" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-2",
          expandedLeafId: "leaf-2",
          sessionIdsByLeafId: {
            "leaf-1": "sess-1",
            "leaf-2": "sess-3",
          },
        },
        "tab-2": {
          root: { type: "leaf", leafId: "leaf-feature" },
          activeLeafId: "leaf-feature",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-feature": "sess-2" },
        },
      },
    },
  };
}

describe("sessionPersistence v2 serialization and migration", () => {
  it("serializes typed terminal tabs, pane ownership, pinned/expanded state, and separate local/backend identities", () => {
    const serialized = serializeWorkspaceState("default", "/workspace/main", workspaceState());

    expect(serialized.version).toBe(WORKSPACE_SESSION_VERSION);
    expect(serialized.version).toBe(2);
    const workspace = serialized.workspaces.default;
    expect(workspace.repoRoot).toBe("/workspace/main");
    expect(workspace.layout.activeTabId).toBe("tab-1");

    const savedTab = workspace.layout.tabs[0];
    expect(savedTab.kind).toBe("terminal");
    expect(savedTab.pinned).toBe(true);
    expect(savedTab.terminal?.paneTree.type).toBe("split");
    expect(savedTab.terminal?.activeLeafId).toBe("leaf-2");
    expect(savedTab.terminal?.expandedLeafId).toBe("leaf-2");
    expect(savedTab.terminal?.sessionIdsByLeafId).toEqual({ "leaf-1": "sess-1", "leaf-2": "sess-3" });

    expect(workspace.terminalSessions["sess-1"]).toMatchObject({
      localSessionId: "sess-1",
      backendSessionId: "backend-1",
      worktreePath: "/workspace/main",
      cwd: "/workspace/main/packages/api",
    });
  });

  it("does not serialize terminal sessions that no tab or pane references", () => {
    const state = workspaceState();
    state.sessions["sess-orphan"] = {
      id: "sess-orphan",
      cwd: "/workspace/main",
      worktreePath: "/workspace/main",
      workspaceId: "default",
      worktree: null,
      backendSessionId: "backend-orphan",
      lifecycle: "working",
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);

    expect(serialized.workspaces.default.terminalSessions).not.toHaveProperty("sess-orphan");
  });

  it("round-trips Orca-style split tab groups without changing either tab's pane ownership", () => {
    const state = workspaceState();
    state.layout = normalizeLayout(state.layout);
    const targetGroupId = getGroupForTab(state.layout, "tab-1")!.id;
    state.layout = layoutReducer(state.layout, {
      type: "MOVE_TAB_TO_SPLIT",
      sourceTabId: "tab-2",
      targetGroupId,
      direction: "horizontal",
      position: "second",
    });

    const beforeTab1Group = getGroupForTab(state.layout, "tab-1");
    const beforeTab2Group = getGroupForTab(state.layout, "tab-2");
    expect(beforeTab1Group?.id).not.toBe(beforeTab2Group?.id);

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);
    expect(serialized.workspaces.default.layout.tabGroups).toHaveLength(2);
    expect(serialized.workspaces.default.layout.tabGroupLayout).toMatchObject({ type: "split", direction: "horizontal" });

    const restored = deserializeWorkspaceState("default", serialized);
    expect(restored).not.toBeNull();
    expect(getGroupForTab(restored!.layout, "tab-1")?.id).not.toBe(getGroupForTab(restored!.layout, "tab-2")?.id);
    expect(restored!.layout.tabGroupLayout).toEqual(state.layout.tabGroupLayout);
    expect(restored!.layout.focusedGroupId).toBe(state.layout.focusedGroupId);
    expect(restored!.layout.layoutsByTabId["tab-1"]).toEqual(state.layout.layoutsByTabId["tab-1"]);
    expect(restored!.layout.layoutsByTabId["tab-2"]).toEqual(state.layout.layoutsByTabId["tab-2"]);
  });

  it("matches live native PTYs by backendSessionId instead of the frontend-local session id", () => {
    const serialized = serializeWorkspaceState("default", "/workspace/main", workspaceState());
    const restored = deserializeWorkspaceState("default", serialized, new Set(["backend-1", "backend-3"]));

    expect(restored).not.toBeNull();
    expect(restored!.sessions["sess-1"].backendSessionId).toBe("backend-1");
    expect(restored!.sessions["sess-2"].backendSessionId).toBeNull();
    expect(restored!.sessions["sess-3"].backendSessionId).toBe("backend-3");
    expect(restored!.sessions["sess-1"].cwd).toBe("/workspace/main/packages/api");
    expect(restored!.sessions["sess-1"].worktreePath).toBe("/workspace/main");
  });

  it("drops persisted terminal sessions that no restored tab or pane references", () => {
    const serialized = serializeWorkspaceState("default", "/workspace/main", workspaceState());
    serialized.workspaces.default.terminalSessions["sess-orphan"] = {
      localSessionId: "sess-orphan",
      backendSessionId: null,
      worktreePath: "/workspace/main",
      cwd: "/workspace/main",
      createdAt: Date.now(),
    };

    const restored = deserializeWorkspaceState("default", serialized, new Set());

    expect(restored).not.toBeNull();
    expect(restored!.sessions).not.toHaveProperty("sess-orphan");
  });

  it("round-trips explicit browser metadata rather than guessing browser kind from a pane tree", () => {
    saveBrowserSettings({ restoreTabsOnLaunch: true });
    const state = workspaceState();
    state.layout.tabs.push({
      kind: "browser",
      id: "tab-browser",
      label: "Docs",
      browserId: "browser-42",
      url: "https://example.com/docs",
      title: "Example Docs",
      loading: false,
      canGoBack: true,
      canGoForward: false,
      pinned: true,
    });
    state.layout.layoutsByTabId["tab-browser"] = {
      root: { type: "leaf", leafId: "leaf-browser" },
      activeLeafId: "leaf-browser",
      expandedLeafId: null,
      sessionIdsByLeafId: { "leaf-browser": "" },
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);
    const savedBrowser = serialized.workspaces.default.layout.tabs.find((tab) => tab.id === "tab-browser");
    expect(savedBrowser).toMatchObject({
      kind: "browser",
      pinned: true,
      browser: {
        browserId: "browser-42",
        url: "https://example.com/docs",
        title: "Example Docs",
        canGoBack: true,
        canGoForward: false,
      },
    });

    const restored = deserializeWorkspaceState("default", serialized)!;
    expect(restored.layout.tabs.find((tab) => tab.id === "tab-browser")).toMatchObject({
      kind: "browser",
      browserId: "browser-42",
      url: "https://example.com/docs",
      pinned: true,
    });
  });

  it("migrates v1 worktree identity and legacy session identifiers", () => {
    const serialized = {
      version: 1,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: "/workspace/main",
          worktrees: [
            { path: "/workspace/main", branch: "refs/heads/main", head: "111", isMain: true, isLocked: false },
            { path: "/workspace/feature", branch: "refs/heads/orca/default/feature-branch", head: "222", isMain: false, isLocked: false },
          ],
          activeWorktreePath: "/workspace/feature",
          layout: {
            splitMode: "none" as const,
            primaryTabId: "tab-feature",
            secondaryTabId: null,
            activeTabId: "tab-feature",
            tabs: [
              { id: "tab-feature", sessionId: "sess-feature", label: "feature-branch", worktreePath: "/workspace/feature" },
              { id: "tab-main", sessionId: "sess-main", label: "main", worktreePath: "/workspace/main" },
            ],
          },
          terminalSessions: {
            "sess-feature": { sessionId: "sess-feature", cwd: "/workspace/feature", worktreePath: "/workspace/feature", createdAt: Date.now() },
            "sess-main": { sessionId: "sess-main", cwd: "/workspace/main", worktreePath: "/workspace/main", createdAt: Date.now() },
          },
        },
      },
    };

    const restored = deserializeWorkspaceState("default", serialized as any);
    expect(restored).not.toBeNull();
    expect(restored!.sessions["sess-feature"].worktree).toEqual({ wsId: "default", slug: "feature-branch" });
    expect(restored!.sessions["sess-main"].worktree).toBeNull();
    expect(restored!.sessions["sess-feature"].backendSessionId).toBe("sess-feature");
  });

  it("maps a v1 pane tree with no per-leaf mapping by leafId rather than tab id", () => {
    const serialized = {
      version: 1,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: "/workspace/main",
          worktrees: [{ path: "/workspace/main", branch: "refs/heads/main", head: "111", isMain: true, isLocked: false }],
          activeWorktreePath: "/workspace/main",
          layout: {
            splitMode: "none" as const,
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [
              {
                id: "tab-1",
                sessionId: "sess-1",
                label: "terminal",
                worktreePath: "/workspace/main",
                paneTree: {
                  type: "split" as const,
                  direction: "horizontal" as const,
                  first: { type: "leaf" as const, leafId: "leaf-alpha" },
                  second: { type: "leaf" as const, leafId: "leaf-beta" },
                  ratio: 0.5,
                },
              },
            ],
          },
          terminalSessions: {
            "sess-1": { sessionId: "sess-1", cwd: "/workspace/main", worktreePath: "/workspace/main", createdAt: Date.now() },
          },
        },
      },
    };

    const restored = deserializeWorkspaceState("default", serialized as any)!;
    expect(restored.layout.layoutsByTabId["tab-1"].sessionIdsByLeafId).toEqual({
      "leaf-alpha": "sess-1",
      "leaf-beta": "sess-1",
    });
    expect(restored.layout.layoutsByTabId["tab-1"].sessionIdsByLeafId["tab-1"]).toBeUndefined();
  });

  it("round-trips multi-worktree workspace state with active and parked worktree layouts without session leakage", () => {
    saveBrowserSettings({ restoreTabsOnLaunch: true });

    const state: WorkspaceState = {
      worktrees: [
        {
          path: "/workspace/main",
          head: "111111",
          branch: "main",
          bare: false,
          detached: false,
          locked: null,
          prunable: null,
        },
        {
          path: "/workspace/feature",
          head: "222222",
          branch: "refs/heads/orca/default/feature-x",
          bare: false,
          detached: false,
          locked: null,
          prunable: null,
        },
      ],
      activeWorktreePath: "/workspace/main",
      sessions: {
        "sess-main": {
          id: "sess-main",
          cwd: "/workspace/main/services/api",
          worktreePath: "/workspace/main",
          workspaceId: "default",
          worktree: null,
          backendSessionId: "backend-main",
          lifecycle: "working",
        },
        "sess-feature": {
          id: "sess-feature",
          cwd: "/workspace/feature/frontend",
          worktreePath: "/workspace/feature",
          workspaceId: "default",
          worktree: { wsId: "default", slug: "feature-x" },
          backendSessionId: "backend-feature",
          lifecycle: "working",
        },
        "sess-orphan": {
          id: "sess-orphan",
          cwd: "/workspace/main",
          worktreePath: "/workspace/main",
          workspaceId: "default",
          worktree: null,
          backendSessionId: "backend-orphan",
          lifecycle: "working",
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
      layout: {
        tabs: [
          { id: "tab-main-1", label: "main terminal", sessionId: "sess-main", pinned: true },
        ],
        primaryTabId: "tab-main-1",
        secondaryTabId: null,
        split: "none",
        nestedSplit: null,
        activeTabId: "tab-main-1",
        layoutsByTabId: {
          "tab-main-1": {
            root: { type: "leaf", leafId: "leaf-main-1" },
            activeLeafId: "leaf-main-1",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-main-1": "sess-main" },
          },
        },
      },
      worktreeLayouts: {
        "/workspace/feature": {
          tabs: [
            { id: "tab-feat-term", label: "feature terminal", sessionId: "sess-feature" },
            {
              kind: "browser",
              id: "tab-feat-browser",
              label: "Feature Preview",
              browserId: "browser-feat-1",
              url: "http://localhost:5173",
              title: "Feature App",
              loading: false,
              canGoBack: true,
              canGoForward: false,
              pinned: false,
            },
          ],
          primaryTabId: "tab-feat-term",
          secondaryTabId: null,
          split: "none",
          nestedSplit: null,
          activeTabId: "tab-feat-term",
          layoutsByTabId: {
            "tab-feat-term": {
              root: { type: "leaf", leafId: "leaf-feat-1" },
              activeLeafId: "leaf-feat-1",
              expandedLeafId: null,
              sessionIdsByLeafId: { "leaf-feat-1": "sess-feature" },
            },
            "tab-feat-browser": {
              root: { type: "leaf", leafId: "leaf-browser-feat" },
              activeLeafId: "leaf-browser-feat",
              expandedLeafId: null,
              sessionIdsByLeafId: { "leaf-browser-feat": "" },
            },
          },
        },
      },
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);

    expect(serialized.workspaces.default.worktreeLayouts).toBeDefined();
    expect(serialized.workspaces.default.worktreeLayouts?.["/workspace/feature"]).toBeDefined();
    const parkedSerialized = serialized.workspaces.default.worktreeLayouts!["/workspace/feature"];
    expect(parkedSerialized.tabs).toHaveLength(2);
    expect(parkedSerialized.tabs[0]).toMatchObject({
      id: "tab-feat-term",
      kind: "terminal",
      terminal: {
        primarySessionId: "sess-feature",
        sessionIdsByLeafId: { "leaf-feat-1": "sess-feature" },
      },
    });
    expect(parkedSerialized.tabs[1]).toMatchObject({
      id: "tab-feat-browser",
      kind: "browser",
      browser: {
        browserId: "browser-feat-1",
        url: "http://localhost:5173",
      },
    });

    // Both referenced sessions must be persisted; orphan session must be dropped (no leakage)
    expect(serialized.workspaces.default.terminalSessions["sess-main"]).toBeDefined();
    expect(serialized.workspaces.default.terminalSessions["sess-feature"]).toBeDefined();
    expect(serialized.workspaces.default.terminalSessions).not.toHaveProperty("sess-orphan");

    const restored = deserializeWorkspaceState(
      "default",
      serialized,
      new Set(["backend-main", "backend-feature"]),
    );
    expect(restored).not.toBeNull();
    if (!restored) return;

    expect(restored.activeWorktreePath).toBe("/workspace/main");
    expect(restored.layout.tabs).toHaveLength(1);
    expect(restored.layout.tabs[0].id).toBe("tab-main-1");
    expect(restored.layout.layoutsByTabId["tab-main-1"].sessionIdsByLeafId).toEqual({
      "leaf-main-1": "sess-main",
    });

    expect(restored.worktreeLayouts).toBeDefined();
    expect(restored.worktreeLayouts?.["/workspace/feature"]).toBeDefined();
    const parkedRestored = restored.worktreeLayouts!["/workspace/feature"];
    expect(parkedRestored.tabs).toHaveLength(2);
    expect(parkedRestored.tabs[0]).toMatchObject({
      id: "tab-feat-term",
      kind: "terminal",
      sessionId: "sess-feature",
    });
    expect(parkedRestored.tabs[1]).toMatchObject({
      id: "tab-feat-browser",
      kind: "browser",
      browserId: "browser-feat-1",
      url: "http://localhost:5173",
    });
    expect(parkedRestored.layoutsByTabId["tab-feat-term"].sessionIdsByLeafId).toEqual({
      "leaf-feat-1": "sess-feature",
    });

    // Verify session integrity and no orphan session leakage
    expect(Object.keys(restored.sessions).sort()).toEqual(["sess-feature", "sess-main"]);
    expect(restored.sessions["sess-main"]).toMatchObject({
      id: "sess-main",
      cwd: "/workspace/main/services/api",
      worktreePath: "/workspace/main",
      backendSessionId: "backend-main",
      lifecycle: "working",
      worktree: null,
    });
    expect(restored.sessions["sess-feature"]).toMatchObject({
      id: "sess-feature",
      cwd: "/workspace/feature/frontend",
      worktreePath: "/workspace/feature",
      backendSessionId: "backend-feature",
      lifecycle: "working",
      worktree: { wsId: "default", slug: "feature-x" },
    });
  });

  it("deserializes existing v2 terminal sessionIdsByLeafId into terminal PaneContent for every leaf", () => {
    const v2Serialized = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: "/workspace/main",
          worktrees: [
            { path: "/workspace/main", branch: "main", head: "111", isMain: true, isLocked: false },
          ],
          activeWorktreePath: "/workspace/main",
          layout: {
            splitMode: "none" as const,
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [
              {
                id: "tab-1",
                kind: "terminal" as const,
                label: "terminal",
                terminal: {
                  primarySessionId: "sess-1",
                  paneTree: {
                    type: "split" as const,
                    direction: "horizontal" as const,
                    first: { type: "leaf" as const, leafId: "leaf-1" },
                    second: { type: "leaf" as const, leafId: "leaf-2" },
                    ratio: 0.5,
                  },
                  sessionIdsByLeafId: {
                    "leaf-1": "sess-1",
                    "leaf-2": "sess-2",
                  },
                  activeLeafId: "leaf-2",
                  expandedLeafId: null,
                },
              },
            ],
          },
          terminalSessions: {
            "sess-1": {
              localSessionId: "sess-1",
              backendSessionId: "backend-1",
              cwd: "/workspace/main/pkg-a",
              worktreePath: "/workspace/main",
              createdAt: Date.now(),
            },
            "sess-2": {
              localSessionId: "sess-2",
              backendSessionId: "backend-2",
              cwd: "/workspace/main/pkg-b",
              worktreePath: "/workspace/main",
              createdAt: Date.now(),
            },
          },
        },
      },
    };

    const restored = deserializeWorkspaceState("default", v2Serialized as any, ["backend-1", "backend-2"]);
    expect(restored).not.toBeNull();
    if (!restored) return;

    const tabLayout = restored.layout.layoutsByTabId["tab-1"] as any;
    expect(tabLayout).toBeDefined();
    expect(tabLayout.contentsByLeafId).toBeDefined();
    expect(tabLayout.contentsByLeafId["leaf-1"]).toEqual({
      kind: "terminal",
      sessionId: "sess-1",
    });
    expect(tabLayout.contentsByLeafId["leaf-2"]).toEqual({
      kind: "terminal",
      sessionId: "sess-2",
    });
    expect(Object.keys(tabLayout.contentsByLeafId).sort()).toEqual(["leaf-1", "leaf-2"]);
  });

  it("round-trips mixed terminal/browser content preserving tree, browser metadata, and terminal session ownership with runtime browserId policy", () => {
    saveBrowserSettings({ restoreTabsOnLaunch: true });

    const state: any = {
      worktrees: [
        {
          path: "/workspace/main",
          head: "123456",
          branch: "main",
          bare: false,
          detached: false,
          locked: null,
          prunable: null,
        },
      ],
      activeWorktreePath: "/workspace/main",
      sessions: {
        "sess-term": {
          id: "sess-term",
          cwd: "/workspace/main/services/api",
          worktreePath: "/workspace/main",
          workspaceId: "default",
          worktree: null,
          backendSessionId: "backend-term",
          lifecycle: "working",
        },
        "sess-orphan": {
          id: "sess-orphan",
          cwd: "/workspace/main",
          worktreePath: "/workspace/main",
          workspaceId: "default",
          worktree: null,
          backendSessionId: "backend-orphan",
          lifecycle: "working",
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
      layout: {
        tabs: [
          {
            id: "tab-mixed",
            label: "Dev & Preview",
            sessionId: "sess-term",
            pinned: false,
          },
        ],
        primaryTabId: "tab-mixed",
        secondaryTabId: null,
        split: "none",
        nestedSplit: null,
        activeTabId: "tab-mixed",
        layoutsByTabId: {
          "tab-mixed": {
            root: {
              type: "split",
              direction: "horizontal",
              first: { type: "leaf", leafId: "leaf-term" },
              second: { type: "leaf", leafId: "leaf-browser" },
              ratio: 0.6,
            },
            activeLeafId: "leaf-browser",
            expandedLeafId: null,
            sessionIdsByLeafId: {
              "leaf-term": "sess-term",
              "leaf-browser": "",
            },
            contentsByLeafId: {
              "leaf-term": {
                kind: "terminal",
                sessionId: "sess-term",
              },
              "leaf-browser": {
                kind: "browser",
                browserId: "browser-preview-1",
                url: "http://localhost:5173/preview",
                title: "Vite App Preview",
                loading: false,
                canGoBack: true,
                canGoForward: false,
                profileId: "default",
                worktreePath: "/workspace/main",
                worktreeLabel: "main",
              },
            },
          },
        },
      },
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);
    expect(serialized.workspaces.default.layout.tabs).toHaveLength(1);
    expect(serialized.workspaces.default.terminalSessions["sess-term"]).toBeDefined();
    expect(serialized.workspaces.default.terminalSessions).not.toHaveProperty("sess-orphan");

    const restored = deserializeWorkspaceState(
      "default",
      serialized,
      new Set(["backend-term"]),
    );
    expect(restored).not.toBeNull();
    if (!restored) return;

    const restoredLayout = restored.layout.layoutsByTabId["tab-mixed"] as any;
    expect(restoredLayout).toBeDefined();
    expect(restoredLayout.root).toMatchObject({
      type: "split",
      direction: "horizontal",
      first: { type: "leaf", leafId: "leaf-term" },
      second: { type: "leaf", leafId: "leaf-browser" },
      ratio: 0.6,
    });
    expect(restoredLayout.contentsByLeafId).toBeDefined();
    expect(restoredLayout.contentsByLeafId["leaf-term"]).toEqual({
      kind: "terminal",
      sessionId: "sess-term",
    });
    expect(restoredLayout.contentsByLeafId["leaf-browser"]).toMatchObject({
      kind: "browser",
      url: "http://localhost:5173/preview",
      title: "Vite App Preview",
      canGoBack: true,
      canGoForward: false,
    });
    expect(restoredLayout.contentsByLeafId["leaf-browser"].browserId).toMatch(/^(browser-preview-1|restored-browser:.*)$/);

    // Verify terminal session ownership and orphan session cleanup
    expect(restored.sessions["sess-term"]).toMatchObject({
      id: "sess-term",
      cwd: "/workspace/main/services/api",
      worktreePath: "/workspace/main",
      backendSessionId: "backend-term",
      lifecycle: "working",
    });
    expect(restored.sessions).not.toHaveProperty("sess-orphan");
  });

  it("persists decimal-string daemonEpoch and lastOutputSequence separately from local/backend IDs", () => {
    const state = workspaceState();
    state.sessions["sess-1"] = {
      ...state.sessions["sess-1"],
      daemonEpoch: "1710000000000",
      lastOutputSequence: "9007199254740997",
    };
    state.sessions["sess-2"] = {
      ...state.sessions["sess-2"],
      daemonEpoch: "1710000000000",
      lastOutputSequence: "0",
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);
    const sess1 = serialized.workspaces.default.terminalSessions["sess-1"];
    const sess2 = serialized.workspaces.default.terminalSessions["sess-2"];

    expect(sess1.daemonEpoch).toBe("1710000000000");
    expect(sess1.lastOutputSequence).toBe("9007199254740997");
    expect(sess1.localSessionId).toBe("sess-1");
    expect(sess1.backendSessionId).toBe("backend-1");

    expect(sess2.daemonEpoch).toBe("1710000000000");
    expect(sess2.lastOutputSequence).toBe("0");
  });

  it("reconciles only when both daemon epoch and backendSessionId match current daemon ListSessions result", () => {
    const state = workspaceState();
    state.sessions["sess-1"] = {
      ...state.sessions["sess-1"],
      daemonEpoch: "epoch-100",
      lastOutputSequence: "1050",
    };
    state.sessions["sess-2"] = {
      ...state.sessions["sess-2"],
      daemonEpoch: "epoch-100",
      lastOutputSequence: "200",
    };
    state.sessions["sess-3"] = {
      ...state.sessions["sess-3"],
      daemonEpoch: "epoch-100",
      lastOutputSequence: "300",
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);

    // Live daemon has epoch-100 and live sessions backend-1, backend-2 (backend-3 is missing from live)
    const liveSessions = [
      { sessionId: "backend-1", daemonEpoch: "epoch-100" },
      { sessionId: "backend-2", daemonEpoch: "epoch-100" },
    ];

    const restored = deserializeWorkspaceState("default", serialized, liveSessions);
    expect(restored).not.toBeNull();
    if (!restored) return;

    // sess-1: matching epoch + live backend ID -> preserved
    expect(restored.sessions["sess-1"]).toMatchObject({
      id: "sess-1",
      backendSessionId: "backend-1",
      lifecycle: "working",
      daemonEpoch: "epoch-100",
      lastOutputSequence: "1050",
    });

    // sess-2: matching epoch + live backend ID -> preserved
    expect(restored.sessions["sess-2"]).toMatchObject({
      id: "sess-2",
      backendSessionId: "backend-2",
      lifecycle: "working",
      daemonEpoch: "epoch-100",
      lastOutputSequence: "200",
    });

    // sess-3: missing backend ID in live list -> marked exited/lost without respawn
    expect(restored.sessions["sess-3"]).toMatchObject({
      id: "sess-3",
      backendSessionId: null,
      lifecycle: "exited",
      daemonEpoch: null,
      lastOutputSequence: null,
    });
    // Pane tree structure & mappings for sess-3 still preserved in layout
    expect(restored.layout.layoutsByTabId["tab-1"].sessionIdsByLeafId["leaf-2"]).toBe("sess-3");
  });

  it("marks sessions exited/lost on daemon epoch change without auto-respawn", () => {
    const state = workspaceState();
    state.sessions["sess-1"] = {
      ...state.sessions["sess-1"],
      daemonEpoch: "epoch-OLD",
      lastOutputSequence: "500",
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);

    // Live daemon restarted with epoch-NEW, even though backend-1 ID appears in live list
    const liveSessions = [
      { sessionId: "backend-1", daemonEpoch: "epoch-NEW" },
    ];

    const restored = deserializeWorkspaceState("default", serialized, liveSessions);
    expect(restored).not.toBeNull();
    if (!restored) return;

    // On epoch mismatch, session must be marked exited/lost without auto-respawn
    expect(restored.sessions["sess-1"]).toMatchObject({
      id: "sess-1",
      backendSessionId: null,
      lifecycle: "exited",
      daemonEpoch: null,
      lastOutputSequence: null,
    });
    expect(restored.layout.layoutsByTabId["tab-1"].sessionIdsByLeafId["leaf-1"]).toBe("sess-1");
  });

  it("persists and restores activityBySessionId for active referenced tabs and drops orphan session activity", () => {
    const state = workspaceState();
    state.activityBySessionId = {
      "sess-1": {
        state: "working",
        title: "omo: refactoring session persistence",
        isAgent: true,
        agentType: "omo",
        source: "title",
      },
      "sess-2": {
        state: "done",
        title: "claude: done testing",
        isAgent: true,
        agentType: "claude",
        source: "screen",
      },
      "sess-orphan": {
        state: "waiting",
        title: "orphan: waiting",
        isAgent: true,
        agentType: "codex",
      },
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);
    const workspace = serialized.workspaces["default"];

    // 1. Serialization includes referenced sessions and excludes orphan
    expect(workspace.activityBySessionId).toBeDefined();
    expect(workspace.activityBySessionId?.["sess-1"]).toEqual({
      state: "working",
      title: "omo: refactoring session persistence",
      isAgent: true,
      agentType: "omo",
      source: "title",
    });
    expect(workspace.activityBySessionId?.["sess-2"]).toEqual({
      state: "done",
      title: "claude: done testing",
      isAgent: true,
      agentType: "claude",
      source: "screen",
    });
    expect(workspace.activityBySessionId?.["sess-orphan"]).toBeUndefined();

    // 2. Deserialization restores activityBySessionId cleanly
    const restored = deserializeWorkspaceState("default", serialized, [
      { sessionId: "backend-1" },
      { sessionId: "backend-2" },
    ]);
    expect(restored).not.toBeNull();
    if (!restored) return;

    expect(restored.activityBySessionId).toEqual({
      "sess-1": {
        // `working` was an in-flight claim; after a restart nothing is running, so it settles to done.
        state: "done",
        title: "omo: refactoring session persistence",
        isAgent: true,
        agentType: "omo",
        source: "title",
      },
      "sess-2": {
        state: "done",
        title: "claude: done testing",
        isAgent: true,
        agentType: "claude",
        source: "screen",
      },
    });

    // 3. Backward compatibility: older session without activityBySessionId deserializes with empty object
    const legacySerialized = {
      ...serialized,
      workspaces: {
        default: {
          ...workspace,
          activityBySessionId: undefined,
        },
      },
    };
    const legacyRestored = deserializeWorkspaceState("default", legacySerialized as any);
    expect(legacyRestored?.activityBySessionId).toEqual({});
  });

  it("does not restore an in-flight working state, because no agent is running after a restart", () => {
    const state = workspaceState();
    state.activityBySessionId = {
      "sess-1": {
        state: "working",
        title: "OmO - orca-lite",
        isAgent: true,
        agentType: "omo",
        source: "screen",
      },
      "sess-2": {
        state: "waiting",
        title: "codex: needs input",
        isAgent: true,
        agentType: "codex",
        source: "screen",
      },
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);
    const restored = deserializeWorkspaceState("default", serialized, [
      { sessionId: "backend-1" },
      { sessionId: "backend-2" },
    ]);
    expect(restored).not.toBeNull();
    if (!restored) return;

    expect(restored.activityBySessionId?.["sess-1"]?.state).not.toBe("working");
    expect(restored.activityBySessionId?.["sess-2"]?.state).not.toBe("waiting");
    // The agent identity is still known, so the tab keeps its icon; only the live claim is dropped.
    expect(restored.activityBySessionId?.["sess-1"]?.agentType).toBe("omo");
    expect(restored.activityBySessionId?.["sess-1"]?.isAgent).toBe(true);
  });

  it("persists and restores agentType and agentSessionId across serialization and deserialization", () => {
    const state = workspaceState();
    state.sessions["sess-1"] = {
      ...state.sessions["sess-1"],
      agentType: "omo",
      agentSessionId: "omo-sess-agent-generated-1234",
    };
    state.sessions["sess-2"] = {
      ...state.sessions["sess-2"],
      agentType: "claude",
      agentSessionId: "c18f-uuid-agent-generated-5678",
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);
    const sess1 = serialized.workspaces.default.terminalSessions["sess-1"];
    const sess2 = serialized.workspaces.default.terminalSessions["sess-2"];

    expect(sess1.agentType).toBe("omo");
    expect(sess1.agentSessionId).toBe("omo-sess-agent-generated-1234");
    expect(sess2.agentType).toBe("claude");
    expect(sess2.agentSessionId).toBe("c18f-uuid-agent-generated-5678");

    const restored = deserializeWorkspaceState("default", serialized, [
      { sessionId: "backend-1" },
      { sessionId: "backend-2" },
    ]);
    expect(restored).not.toBeNull();
    if (!restored) return;

    expect(restored.sessions["sess-1"].agentType).toBe("omo");
    expect(restored.sessions["sess-1"].agentSessionId).toBe("omo-sess-agent-generated-1234");
    expect(restored.sessions["sess-2"].agentType).toBe("claude");
    expect(restored.sessions["sess-2"].agentSessionId).toBe("c18f-uuid-agent-generated-5678");
  });

  it("preserves agentType and agentSessionId when daemon epoch mismatch marks session exited and nulls backendSessionId", () => {
    const state = workspaceState();
    state.sessions["sess-1"] = {
      ...state.sessions["sess-1"],
      daemonEpoch: "epoch-OLD",
      lastOutputSequence: "500",
      agentType: "claude",
      agentSessionId: "claude-session-uuid-9999",
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);

    // Live daemon has a new epoch, causing an epoch mismatch for backend-1
    const liveSessions = [
      { sessionId: "backend-1", daemonEpoch: "epoch-NEW" },
    ];

    const restored = deserializeWorkspaceState("default", serialized, liveSessions);
    expect(restored).not.toBeNull();
    if (!restored) return;

    // Backend session is nulled and lifecycle is exited because PTY is dead
    expect(restored.sessions["sess-1"].backendSessionId).toBeNull();
    expect(restored.sessions["sess-1"].lifecycle).toBe("exited");
    expect(restored.sessions["sess-1"].daemonEpoch).toBeNull();

    // But agentType and agentSessionId MUST survive so the agent conversation can be resumed
    expect(restored.sessions["sess-1"].agentType).toBe("claude");
    expect(restored.sessions["sess-1"].agentSessionId).toBe("claude-session-uuid-9999");
    expect(restored.sessions["sess-1"].providerSession).toEqual({
      key: "session_id",
      id: "claude-session-uuid-9999",
    });
  });

  it("loads legacy save files without agent fields without throwing and sets agentType/agentSessionId to null", () => {
    const legacySerialized = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: "/workspace/main",
          worktrees: [
            { path: "/workspace/main", branch: "main", head: "111", isMain: true, isLocked: false },
          ],
          activeWorktreePath: "/workspace/main",
          layout: {
            splitMode: "none" as const,
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [
              {
                id: "tab-1",
                kind: "terminal" as const,
                label: "terminal",
                terminal: {
                  primarySessionId: "sess-1",
                  paneTree: { type: "leaf" as const, leafId: "leaf-1" },
                  sessionIdsByLeafId: { "leaf-1": "sess-1" },
                  activeLeafId: "leaf-1",
                  expandedLeafId: null,
                },
              },
            ],
          },
          terminalSessions: {
            "sess-1": {
              localSessionId: "sess-1",
              backendSessionId: "backend-1",
              cwd: "/workspace/main",
              worktreePath: "/workspace/main",
              createdAt: Date.now(),
              // Note: no agentType or agentSessionId present
            },
          },
        },
      },
    };

    const restored = deserializeWorkspaceState("default", legacySerialized as any, ["backend-1"]);
    expect(restored).not.toBeNull();
    if (!restored) return;

    expect(restored.sessions["sess-1"]).toBeDefined();
    expect(restored.sessions["sess-1"].agentType).toBeNull();
    expect(restored.sessions["sess-1"].agentSessionId).toBeNull();
  });

  it("keeps live backend identity, pane leaf ownership, agent metadata, CWD, and output sequence across warm disk restore", () => {
    // Given: a persisted workspace state containing live daemon PTYs with backendSessionId, daemonEpoch,
    // lastOutputSequence, CWD, agent metadata, and tab/leaf ownership in layoutsByTabId.
    const state = workspaceState();
    state.sessions["sess-1"] = {
      ...state.sessions["sess-1"],
      daemonEpoch: "1700000000",
      lastOutputSequence: "450",
      agentType: "claude",
      agentSessionId: "claude-session-uuid-1234",
    };
    state.sessions["sess-3"] = {
      ...state.sessions["sess-3"],
      daemonEpoch: "1700000000",
      lastOutputSequence: "820",
      agentType: "omo",
      agentSessionId: "omo-session-uuid-5678",
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);
    const liveSessions = [
      { sessionId: "backend-1", daemonEpoch: "1700000000" },
      { sessionId: "backend-3", daemonEpoch: "1700000000" },
    ];

    // When: deserializing with live backend sessions matching the current daemon epoch
    const restored = deserializeWorkspaceState("default", serialized, liveSessions);

    // Then: live backend identity, local session id, CWD, leaf ownership, agent metadata, and sequence are preserved
    expect(restored).not.toBeNull();
    if (!restored) return;

    expect(restored.sessions["sess-1"]).toMatchObject({
      id: "sess-1",
      backendSessionId: "backend-1",
      lifecycle: "working",
      cwd: "/workspace/main/packages/api",
      worktreePath: "/workspace/main",
      daemonEpoch: "1700000000",
      lastOutputSequence: "450",
      agentType: "claude",
      agentSessionId: "claude-session-uuid-1234",
    });

    expect(restored.sessions["sess-3"]).toMatchObject({
      id: "sess-3",
      backendSessionId: "backend-3",
      lifecycle: "working",
      cwd: "/workspace/main/packages/web",
      worktreePath: "/workspace/main",
      daemonEpoch: "1700000000",
      lastOutputSequence: "820",
      agentType: "omo",
      agentSessionId: "omo-session-uuid-5678",
    });

    // sess-2 was not in liveSessions, so its backendSessionId is nulled and lifecycle is exited
    expect(restored.sessions["sess-2"].backendSessionId).toBeNull();
    expect(restored.sessions["sess-2"].lifecycle).toBe("exited");

    // Layout tab and leaf ownership are preserved
    const tab1Layout = restored.layout.layoutsByTabId["tab-1"];
    expect(tab1Layout).toBeDefined();
    expect(tab1Layout.sessionIdsByLeafId).toEqual({
      "leaf-1": "sess-1",
      "leaf-2": "sess-3",
    });
    expect(tab1Layout.activeLeafId).toBe("leaf-2");
    expect(tab1Layout.expandedLeafId).toBe("leaf-2");
  });

  it("persists providerSession and never serializes transient reconnect locks or in-flight state", () => {
    const state = workspaceState();
    state.sessions["sess-1"] = {
      ...state.sessions["sess-1"],
      agentType: "claude",
      agentSessionId: "c18f-uuid-456",
      providerSession: { key: "session_id", id: "c18f-uuid-456" },
      reconnectLifecycle: "spawning",
      reconnectError: { code: "SOME_ERROR", message: "temporary error" },
      reconnectRequestId: "transient-request-id",
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);
    const persistedSession = serialized.workspaces["default"]?.terminalSessions["sess-1"];
    expect(persistedSession).toBeDefined();
    expect(persistedSession?.providerSession).toEqual({ key: "session_id", id: "c18f-uuid-456" });
    expect(persistedSession?.agentType).toBe("claude");
    expect(persistedSession?.agentSessionId).toBe("c18f-uuid-456");

    // Serialization must NOT contain transient reconnect locks, request IDs, or errors
    const rawJson = JSON.stringify(serialized);
    expect(rawJson).not.toContain("reconnectLifecycle");
    expect(rawJson).not.toContain("reconnectError");
    expect(rawJson).not.toContain("spawning");
    expect(rawJson).not.toContain("clientRequestId");
    expect(rawJson).not.toContain("reconnectRequestId");

    // Deserialization on cold loss initializes transient reconnect state to idle and null error
    const restored = deserializeWorkspaceState("default", serialized, []);
    expect(restored).not.toBeNull();
    const restoredSession = restored?.sessions["sess-1"];
    expect(restoredSession?.providerSession).toEqual({ key: "session_id", id: "c18f-uuid-456" });
    expect(restoredSession?.reconnectLifecycle).toBe("idle");
    expect(restoredSession?.reconnectError).toBeNull();
    expect(restoredSession?.reconnectRequestId).toBeNull();
    expect(restoredSession?.lifecycle).toBe("exited");
    expect(restoredSession?.backendSessionId).toBeNull();
  });

  it("falls back to activityBySessionId agentType during serialization and deserialization when session.agentType is unset", () => {
    const state = workspaceState();
    // sess-1 has no agentType on session, but has agent activity in activityBySessionId
    state.sessions["sess-1"] = {
      ...state.sessions["sess-1"],
      agentType: undefined,
      agentSessionId: null,
      providerSession: { key: "session_id", id: "omo-sess-123" },
    };
    state.activityBySessionId = {
      "sess-1": {
        state: "working",
        title: "OmO working",
        isAgent: true,
        agentType: "omo",
        source: "screen",
      },
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", state);
    const persistedSession = serialized.workspaces["default"]?.terminalSessions["sess-1"];
    expect(persistedSession?.agentType).toBe("omo");

    // Clear sess.agentType in serialized JSON to simulate older/missing session agentType with activity fallback
    if (persistedSession) {
      persistedSession.agentType = null;
    }

    const restored = deserializeWorkspaceState("default", serialized, []);
    expect(restored).not.toBeNull();
    expect(restored?.sessions["sess-1"]?.agentType).toBe("omo");
  });

  describe("migrateLegacyAgentType", () => {
    it("migrates legacy gemini agent type to antigravity and preserves other inputs", () => {
      expect(migrateLegacyAgentType("gemini")).toBe("antigravity");
      expect(migrateLegacyAgentType(" Gemini ")).toBe("antigravity");
      expect(migrateLegacyAgentType("claude")).toBe("claude");
      expect(migrateLegacyAgentType(null)).toBeNull();
      expect(migrateLegacyAgentType(undefined)).toBeUndefined();
    });
  });

  it("migrates legacy gemini agentType to antigravity and generates providerSession during restore", () => {
    const legacySerialized = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: "/workspace/main",
          worktrees: [
            { path: "/workspace/main", branch: "main", head: "111", isMain: true, isLocked: false },
          ],
          activeWorktreePath: "/workspace/main",
          layout: {
            splitMode: "none" as const,
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [
              {
                id: "tab-1",
                kind: "terminal" as const,
                label: "terminal",
                terminal: {
                  primarySessionId: "sess-1",
                  paneTree: { type: "leaf" as const, leafId: "leaf-1" },
                  sessionIdsByLeafId: { "leaf-1": "sess-1" },
                  activeLeafId: "leaf-1",
                  expandedLeafId: null,
                },
              },
            ],
          },
          terminalSessions: {
            "sess-1": {
              localSessionId: "sess-1",
              backendSessionId: "backend-1",
              cwd: "/workspace/main",
              worktreePath: "/workspace/main",
              createdAt: Date.now(),
              agentType: "gemini",
              agentSessionId: "c562c206-80a7-4235-9ecf-8d13984183cd",
              // No providerSession
            },
          },
        },
      },
    };

    const restored = deserializeWorkspaceState("default", legacySerialized as any, []);
    expect(restored).not.toBeNull();
    if (!restored) return;

    expect(restored.sessions["sess-1"]).toBeDefined();
    expect(restored.sessions["sess-1"].agentType).toBe("antigravity");
    expect(restored.sessions["sess-1"].agentSessionId).toBe("c562c206-80a7-4235-9ecf-8d13984183cd");
    expect(restored.sessions["sess-1"].providerSession).toEqual({
      key: "session_id",
      id: "c562c206-80a7-4235-9ecf-8d13984183cd",
    });
  });

  it("migrates restored activityBySessionId agentType from gemini to antigravity while leaving non-legacy untouched", () => {
    const legacySerialized = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: "/workspace/main",
          worktrees: [
            { path: "/workspace/main", branch: "main", head: "111", isMain: true, isLocked: false },
          ],
          activeWorktreePath: "/workspace/main",
          layout: {
            splitMode: "none" as const,
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [
              {
                id: "tab-1",
                kind: "terminal" as const,
                label: "terminal",
                terminal: {
                  primarySessionId: "sess-1",
                  paneTree: {
                    type: "split" as const,
                    direction: "horizontal" as const,
                    first: { type: "leaf" as const, leafId: "leaf-1" },
                    second: { type: "leaf" as const, leafId: "leaf-2" },
                    ratio: 0.5,
                  },
                  sessionIdsByLeafId: { "leaf-1": "sess-1", "leaf-2": "sess-2" },
                  activeLeafId: "leaf-1",
                  expandedLeafId: null,
                },
              },
            ],
          },
          terminalSessions: {
            "sess-1": {
              localSessionId: "sess-1",
              backendSessionId: "backend-1",
              cwd: "/workspace/main",
              worktreePath: "/workspace/main",
              createdAt: Date.now(),
              agentType: "gemini",
              agentSessionId: "gemini-sess-1",
            },
            "sess-2": {
              localSessionId: "sess-2",
              backendSessionId: "backend-2",
              cwd: "/workspace/main",
              worktreePath: "/workspace/main",
              createdAt: Date.now(),
              agentType: "claude",
              agentSessionId: "claude-sess-2",
            },
          },
          activityBySessionId: {
            "sess-1": {
              state: "done" as const,
              title: "Gemini finished",
              isAgent: true,
              agentType: "gemini",
            },
            "sess-2": {
              state: "done" as const,
              title: "Claude finished",
              isAgent: true,
              agentType: "claude",
            },
          },
        },
      },
    };

    const restored = deserializeWorkspaceState("default", legacySerialized as any, []);
    expect(restored).not.toBeNull();
    if (!restored) return;

    expect(restored.activityBySessionId?.["sess-1"]).toEqual({
      state: "done",
      title: "Gemini finished",
      isAgent: true,
      agentType: "antigravity",
    });
    expect(restored.activityBySessionId?.["sess-2"]).toEqual({
      state: "done",
      title: "Claude finished",
      isAgent: true,
      agentType: "claude",
    });
  });
});
