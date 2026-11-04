import { describe, expect, it } from "vitest";
import type { WorkspaceState } from "../state/workspaceStore";
import { computeTerminalTabDisplay } from "../state/tabDisplay";
import { deserializeWorkspaceState, serializeWorkspaceState } from "./sessionPersistence";

function workspaceState(customLabel?: string): WorkspaceState {
  return {
    worktrees: [
      { path: "/workspace/main", head: "123456", branch: "main", bare: false, detached: false, locked: null, prunable: null },
    ],
    activeWorktreePath: "/workspace/main",
    sessions: {
      "sess-1": {
        id: "sess-1",
        cwd: "/workspace/main",
        worktreePath: "/workspace/main",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "backend-1",
        lifecycle: "working",
      },
    },
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
    layout: {
      tabs: [{ id: "tab-1", label: "main", sessionId: "sess-1", ...(customLabel ? { customLabel } : {}) }],
      primaryTabId: "tab-1",
      secondaryTabId: null,
      split: "horizontal",
      nestedSplit: null,
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: { type: "leaf", leafId: "leaf-1" },
          activeLeafId: "leaf-1",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-1": "sess-1" },
        },
      },
    },
  };
}

describe("terminal tab custom label persistence", () => {
  it("round-trips explicit file labels and remote provenance into the restored pane", () => {
    const state = workspaceState();
    const remote = { workspaceId: "daemon:host", path: "/srv/src/index.ts", sessionId: "remote-session" };
    state.layout.tabs = [{
      kind: "file", id: "file", label: "index.ts", customLabel: "src/index.ts",
      path: "/cache/copy/index.ts", backendSessionId: "backend-1", line: null, col: null,
      workspaceId: remote.workspaceId, previewId: "file", remote,
    }];
    state.layout.activeTabId = "file";
    state.layout.primaryTabId = "file";
    state.layout.layoutsByTabId = {};
    const saved = serializeWorkspaceState("default", "/workspace/main", state);
    const restored = deserializeWorkspaceState("default", saved, ["backend-1"]);
    expect(restored?.layout.tabs[0]).toMatchObject({ customLabel: "src/index.ts", remote });
    const panes = restored?.layout.layoutsByTabId.file?.contentsByLeafId ?? {};
    expect(Object.values(panes)).toContainEqual(expect.objectContaining({ kind: "file", customLabel: "src/index.ts", remote }));
  });

  it("round-trips a user rename so it survives a restart", () => {
    const saved = serializeWorkspaceState("default", "/workspace/main", workspaceState("release build"));
    const restored = deserializeWorkspaceState("default", saved, ["backend-1"])!;

    expect(restored.layout.tabs[0]).toMatchObject({ id: "tab-1", label: "main", customLabel: "release build" });
  });

  it("restores an unrenamed tab without a custom label", () => {
    const saved = serializeWorkspaceState("default", "/workspace/main", workspaceState());
    const restored = deserializeWorkspaceState("default", saved, ["backend-1"])!;

    expect(restored.layout.tabs[0]).not.toHaveProperty("customLabel");
  });

  it("shows a new session's readable name in its tab and keeps it across a restart", () => {
    const state = workspaceState();
    state.sessions["sess-1"] = { ...state.sessions["sess-1"], name: "brisk-otter" };
    const saved = serializeWorkspaceState("default", "/workspace/main", state);
    const restored = deserializeWorkspaceState("default", saved, ["backend-1"])!;

    expect(restored.sessions["sess-1"]).toMatchObject({ name: "brisk-otter" });
    const display = computeTerminalTabDisplay(restored.layout.tabs, restored.layout, restored.sessions);
    expect(display["tab-1"]?.text).toBe("brisk-otter");
  });

  it("lets a user rename win over the readable name", () => {
    const state = workspaceState("release build");
    state.sessions["sess-1"] = { ...state.sessions["sess-1"], name: "brisk-otter" };
    const display = computeTerminalTabDisplay(state.layout.tabs, state.layout, state.sessions);

    expect(display["tab-1"]?.text).toBe("release build");
  });

  it("does not rename an existing session that has no readable name", () => {
    const saved = serializeWorkspaceState("default", "/workspace/main", workspaceState());
    const restored = deserializeWorkspaceState("default", saved, ["backend-1"])!;

    expect(restored.sessions["sess-1"]).not.toHaveProperty("name");
    const display = computeTerminalTabDisplay(restored.layout.tabs, restored.layout, restored.sessions);
    expect(display["tab-1"]?.text).toBe("main");
  });
});
