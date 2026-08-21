import { describe, expect, it } from "vitest";
import type { WorkspaceState } from "../state/workspaceStore";
import { serializeWorkspaceState } from "./sessionPersistence";

describe("sessionPersistence serialization and paneTree support", () => {
  it("serializes workspace state including nested pane tree and active tab metadata", () => {
    const mockState: WorkspaceState = {
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
          cwd: "/workspace/main",
          workspaceId: "default",
          worktree: null,
          backendSessionId: "backend-1",
          lifecycle: "working",
        },
        "sess-2": {
          id: "sess-2",
          cwd: "/workspace/main",
          workspaceId: "default",
          worktree: null,
          backendSessionId: "backend-2",
          lifecycle: "working",
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      layout: {
        tabs: [
          {
            id: "tab-1",
            label: "main",
            sessionId: "sess-1",
          },
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
            expandedLeafId: null,
            sessionIdsByLeafId: {
              "leaf-1": "sess-1",
              "leaf-2": "sess-2",
            },
          },
        },
      },
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", mockState);
    expect(serialized.version).toBe(1);
    expect(serialized.activeWorkspaceId).toBe("default");
    expect(serialized.workspaces["default"]).toBeDefined();
    expect(serialized.workspaces["default"].repoRoot).toBe("/workspace/main");
    expect(serialized.workspaces["default"].layout.activeTabId).toBe("tab-1");
    expect(serialized.workspaces["default"].layout.tabs.length).toBe(1);

    const savedTab = serialized.workspaces["default"].layout.tabs[0];
    expect(savedTab.paneTree?.type).toBe("split");
    expect(savedTab.activeLeafId).toBe("leaf-2");
    expect(savedTab.sessionIdsByLeafId?.["leaf-2"]).toBe("sess-2");
  });
});
