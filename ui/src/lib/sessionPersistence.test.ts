import { describe, expect, it } from "vitest";
import type { WorkspaceState } from "../state/workspaceStore";
import { serializeWorkspaceState } from "./sessionPersistence";

describe("sessionPersistence serialization", () => {
  it("serializes workspace state into PersistedWorkspaceSession structure", () => {
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
      },
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
        split: "none",
        nestedSplit: null,
        activeTabId: "tab-1",
        layoutsByTabId: {},
      },
    };

    const serialized = serializeWorkspaceState("default", "/workspace/main", mockState);
    expect(serialized.version).toBe(1);
    expect(serialized.activeWorkspaceId).toBe("default");
    expect(serialized.workspaces["default"]).toBeDefined();
    expect(serialized.workspaces["default"].repoRoot).toBe("/workspace/main");
    expect(serialized.workspaces["default"].worktrees.length).toBe(1);
    expect(serialized.workspaces["default"].layout.tabs.length).toBe(1);
    expect(serialized.workspaces["default"].terminalSessions["sess-1"].cwd).toBe("/workspace/main");
  });
});
