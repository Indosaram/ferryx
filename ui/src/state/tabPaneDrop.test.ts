import { describe, expect, it } from "vitest";

import type { ChangesTab, FileTab, LayoutState, TerminalTab } from "../lib/types";
import { createLayoutState } from "./layout";
import { moveTabIntoPaneSplit } from "./tabPaneDrop";

const terminal: TerminalTab = { id: "tab-term", label: "shell", sessionId: "session-a" };
const changes: ChangesTab = {
  kind: "changes",
  id: "tab-changes",
  label: "Changes",
  workspaceId: "workspace-main",
  worktreePath: "/repo/wt/feature",
  worktree: { wsId: "ws", slug: "feature" },
};
const file: FileTab = {
  kind: "file",
  id: "tab-file",
  label: "a.ts",
  path: "/repo/a.ts",
  backendSessionId: "backend-1",
  line: null,
  col: null,
  workspaceId: "workspace-main",
  previewId: "tab-file",
};

function leafOf(state: LayoutState, tabId: string): string {
  const root = state.layoutsByTabId[tabId].root;
  if (root.type !== "leaf") throw new Error("expected a single leaf");
  return root.leafId;
}

describe("moveTabIntoPaneSplit with a Changes tab", () => {
  it("refuses to split a terminal into a Changes tab's pane", () => {
    const state = createLayoutState([terminal, changes], terminal.id);
    const next = moveTabIntoPaneSplit(state, terminal.id, changes.id, leafOf(state, changes.id), "horizontal");
    expect(next.tabs.map((tab) => tab.id)).toEqual([terminal.id, changes.id]);
    expect(next.layoutsByTabId[changes.id].root.type).toBe("leaf");
    expect(next.layoutsByTabId[terminal.id].sessionIdsByLeafId[leafOf(next, terminal.id)]).toBe("session-a");
  });

  it("refuses to move a Changes tab into another tab's pane", () => {
    const state = createLayoutState([terminal, changes], terminal.id);
    const next = moveTabIntoPaneSplit(state, changes.id, terminal.id, leafOf(state, terminal.id), "horizontal");
    expect(next.tabs.map((tab) => tab.id)).toEqual([terminal.id, changes.id]);
    expect(next.layoutsByTabId[terminal.id].root.type).toBe("leaf");
  });

  it("still lets a terminal split into a file tab's pane", () => {
    const state = createLayoutState([terminal, file], terminal.id);
    const next = moveTabIntoPaneSplit(state, terminal.id, file.id, leafOf(state, file.id), "horizontal");
    expect(next.tabs.map((tab) => tab.id)).toEqual([file.id]);
    expect(next.layoutsByTabId[file.id].root.type).toBe("split");
  });
});
