import { describe, expect, it } from "vitest";

import {
  dropPriority,
  resolveWorkspaceDropCommand,
  type PaneDragData,
  type TabDragData,
  type WorkspaceDropData,
} from "./tab-dnd/tabDragTypes";

const tabDrag: TabDragData = {
  type: "tab",
  tabId: "tab-source",
  groupId: "group-source",
  index: 0,
};

const paneDrag: PaneDragData = {
  type: "pane",
  tabId: "tab-source",
  leafId: "leaf-source",
};

describe("TerminalSplitView dnd-kit drop resolver", () => {
  it("resolves same-group and cross-group tab targets to one indexed group move", () => {
    expect(
      resolveWorkspaceDropCommand(tabDrag, {
        type: "tab",
        tabId: "tab-target",
        groupId: "group-source",
        index: 2,
      }),
    ).toEqual({
      type: "move-tab-to-group",
      tabId: "tab-source",
      targetGroupId: "group-source",
      targetIndex: 2,
    });

    expect(
      resolveWorkspaceDropCommand(tabDrag, {
        type: "tab",
        tabId: "tab-other",
        groupId: "group-other",
        index: 1,
      }),
    ).toEqual({
      type: "move-tab-to-group",
      tabId: "tab-source",
      targetGroupId: "group-other",
      targetIndex: 1,
    });

    expect(resolveWorkspaceDropCommand(tabDrag, { type: "group-body", groupId: "group-other" })).toEqual({
      type: "move-tab-to-group",
      tabId: "tab-source",
      targetGroupId: "group-other",
    });
  });

  it("maps all four group edges to the canonical split direction and position", () => {
    const expected = {
      left: ["horizontal", "first"],
      right: ["horizontal", "second"],
      top: ["vertical", "first"],
      bottom: ["vertical", "second"],
    } as const;

    for (const [edge, [direction, position]] of Object.entries(expected)) {
      expect(
        resolveWorkspaceDropCommand(tabDrag, {
          type: "group-edge",
          groupId: "group-target",
          edge: edge as "left" | "right" | "top" | "bottom",
        }),
      ).toEqual({
        type: "move-tab-to-split",
        tabId: "tab-source",
        targetGroupId: "group-target",
        direction,
        position,
      });
    }
  });

  it("moves a terminal pane to a tab strip/body without cloning PTY ownership", () => {
    expect(
      resolveWorkspaceDropCommand(paneDrag, {
        type: "tab",
        tabId: "tab-target",
        groupId: "group-target",
        index: 2,
      }),
    ).toEqual({
      type: "detach-pane-to-tab",
      sourceTabId: "tab-source",
      leafId: "leaf-source",
      targetGroupId: "group-target",
      targetIndex: 3,
    });

    expect(resolveWorkspaceDropCommand(paneDrag, { type: "group-body", groupId: "group-target" })).toEqual({
      type: "detach-pane-to-tab",
      sourceTabId: "tab-source",
      leafId: "leaf-source",
      targetGroupId: "group-target",
    });
  });

  it("keeps pane swaps inside their source tab and ignores pane-to-group-edge split", () => {
    expect(
      resolveWorkspaceDropCommand(paneDrag, {
        type: "pane-leaf",
        tabId: "tab-source",
        leafId: "leaf-other",
      }),
    ).toEqual({
      type: "swap-panes",
      tabId: "tab-source",
      sourceLeafId: "leaf-source",
      targetLeafId: "leaf-other",
    });

    expect(
      resolveWorkspaceDropCommand(paneDrag, {
        type: "pane-leaf",
        tabId: "tab-other",
        leafId: "leaf-other",
      }),
    ).toBeNull();
    expect(
      resolveWorkspaceDropCommand(paneDrag, {
        type: "group-edge",
        groupId: "group-target",
        edge: "right",
      }),
    ).toBeNull();
  });

  it("prioritizes edge split over tab insertion over group-body drop for tab drags", () => {
    const edge: WorkspaceDropData = { type: "group-edge", groupId: "group-target", edge: "left" };
    const tab: WorkspaceDropData = { type: "tab", tabId: "tab-target", groupId: "group-target", index: 0 };
    const body: WorkspaceDropData = { type: "group-body", groupId: "group-target" };

    expect(dropPriority(tabDrag, edge)).toBeLessThan(dropPriority(tabDrag, tab));
    expect(dropPriority(tabDrag, tab)).toBeLessThan(dropPriority(tabDrag, body));
  });
});
