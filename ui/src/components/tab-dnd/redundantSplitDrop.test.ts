import { describe, expect, it, vi } from "vitest";

import { isRedundantSplit, type PaneNode } from "../../state/paneTree";
import { createWorkspaceCollisionDetection } from "../TerminalSplitView";
import {
  resolveWorkspaceDropCommand,
  type PaneDragData,
  type PaneEdgeDropData,
  type RedundantSplitCheck,
} from "./tabDragTypes";

const twoPaneVertical: PaneNode = {
  type: "split",
  direction: "vertical",
  first: { type: "leaf", leafId: "leaf-a" },
  second: { type: "leaf", leafId: "leaf-b" },
  ratio: 0.5,
};

const layoutCheck: RedundantSplitCheck = ({ sourceLeafId, targetLeafId, direction, position }) =>
  isRedundantSplit(twoPaneVertical, sourceLeafId, targetLeafId, direction, position);

const dragLeafA: PaneDragData = {
  type: "pane",
  tabId: "tab-main",
  sourcePaneId: "leaf-a",
  leafId: "leaf-a",
};

function paneEdge(leafId: string, edge: PaneEdgeDropData["edge"]): PaneEdgeDropData {
  return { type: "pane-edge", tabId: "tab-main", leafId, edge };
}

describe("redundant pane-edge drops", () => {
  it("resolves no command when the drop would rebuild the current layout", () => {
    expect(resolveWorkspaceDropCommand(dragLeafA, paneEdge("leaf-b", "top"), layoutCheck)).toBeNull();
  });

  it("still resolves the sibling swap and the perpendicular re-split", () => {
    expect(resolveWorkspaceDropCommand(dragLeafA, paneEdge("leaf-b", "bottom"), layoutCheck)).toMatchObject({
      type: "move-pane-to-pane-split",
      direction: "vertical",
      position: "second",
    });
    expect(resolveWorkspaceDropCommand(dragLeafA, paneEdge("leaf-b", "left"), layoutCheck)).toMatchObject({
      type: "move-pane-to-pane-split",
      direction: "horizontal",
      position: "first",
    });
  });

  it("resolves the redundant drop when no layout check is supplied", () => {
    expect(resolveWorkspaceDropCommand(dragLeafA, paneEdge("leaf-b", "top"))).toMatchObject({
      type: "move-pane-to-pane-split",
    });
  });

  it("drops redundant targets out of collision results so no split preview is shown", () => {
    const containers = (["top", "bottom"] as const).map((edge) => ({
      id: `pane-edge:tab-main:leaf-b:${edge}`,
      data: { current: paneEdge("leaf-b", edge) },
      rect: { current: { top: 0, left: 0, right: 100, bottom: 100, width: 100, height: 100 } },
      disabled: false,
      node: { current: null },
    }));

    const args = {
      active: { id: "pane:tab-main:leaf-a", data: { current: dragLeafA }, rect: { current: { initial: null, translated: null } } },
      droppableContainers: containers,
      pointerCoordinates: { x: 50, y: 10 },
      collisionRect: { top: 0, left: 0, right: 100, bottom: 100, width: 100, height: 100 },
      droppableRects: new Map(containers.map((container) => [container.id, container.rect.current])),
    };

    // The pointer sits in the top wedge, so only the top edge survives center-line
    // resolution -- and the guard then removes it because that drop is a no-op.
    const guarded = createWorkspaceCollisionDetection(layoutCheck)(args as never).map((collision) => collision.id);
    expect(guarded).toEqual([]);

    const unguarded = createWorkspaceCollisionDetection()(args as never).map((collision) => collision.id);
    expect(unguarded).toEqual(["pane-edge:tab-main:leaf-b:top"]);
  });

  it("keeps the layout check off the hot path when the drag is not a pane drag", () => {
    const check = vi.fn(() => true);
    const tabDrag = { type: "tab", tabId: "tab-main", groupId: "group-main", index: 0 } as const;
    expect(resolveWorkspaceDropCommand(tabDrag, paneEdge("leaf-b", "top"), check)).toMatchObject({
      type: "move-tab-to-pane-split",
    });
    expect(check).not.toHaveBeenCalled();
  });
});
