import { describe, expect, it } from "vitest";

import { getGroupForTab, layoutReducer, normalizeLayout } from "../state/layout";
import type { WorkspaceState } from "../state/workspaceStore";
import { deserializeWorkspaceState, serializeWorkspaceState, WORKSPACE_SESSION_VERSION } from "./sessionPersistence";

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

  it("round-trips explicit browser metadata rather than guessing browser kind from a pane tree", () => {
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
});
