import type { PaneDirection } from "../../state/paneTree";

export type TabDropEdge = "left" | "right" | "top" | "bottom";

export type TabDragData = {
  type: "tab";
  tabId: string;
  groupId: string;
  index: number;
};

export type PaneDragData = {
  type: "pane";
  tabId: string;
  sourcePaneId?: string;
  sourceSessionId?: string;
  leafId?: string;
};

export type GroupBodyDropData = {
  type: "group-body";
  groupId: string;
};

export type GroupEdgeDropData = {
  type: "group-edge";
  groupId: string;
  edge: TabDropEdge;
};

export type PaneLeafDropData = {
  type: "pane-leaf";
  tabId: string;
  leafId: string;
};

export type PaneEdgeDropData = {
  type: "pane-edge";
  tabId: string;
  leafId: string;
  edge: TabDropEdge;
};

export type WorkspaceDragData = TabDragData | PaneDragData;
export type WorkspaceDropData = TabDragData | GroupBodyDropData | GroupEdgeDropData | PaneLeafDropData | PaneEdgeDropData;

export type WorkspaceDropCommand =
  | {
      type: "move-tab-to-group";
      tabId: string;
      targetGroupId: string;
      targetIndex?: number;
    }
  | {
      type: "move-tab-to-split";
      tabId: string;
      targetGroupId: string;
      direction: PaneDirection;
      position: "first" | "second";
    }
  | {
      type: "move-tab-to-pane-split";
      sourceTabId: string;
      targetTabId: string;
      targetLeafId: string;
      direction: PaneDirection;
      position: "first" | "second";
    }
  | {
      type: "move-pane-to-pane-split";
      sourceTabId: string;
      sourceLeafId: string;
      targetTabId: string;
      targetLeafId: string;
      direction: PaneDirection;
      position: "first" | "second";
    }
  | {
      type: "detach-pane-to-tab";
      sourceTabId: string;
      leafId: string;
      targetGroupId: string;
      targetIndex?: number;
    }
  | {
      type: "swap-panes";
      tabId: string;
      sourceLeafId: string;
      targetLeafId: string;
    };

export function isWorkspaceDragData(value: unknown): value is WorkspaceDragData {
  if (!value || typeof value !== "object") return false;
  if (!("type" in value)) return false;
  const type = value.type;
  if (type === "tab") {
    return (
      "tabId" in value &&
      typeof value.tabId === "string" &&
      "groupId" in value &&
      typeof value.groupId === "string" &&
      "index" in value &&
      typeof value.index === "number"
    );
  }
  if (type === "pane") {
    return (
      "tabId" in value &&
      typeof value.tabId === "string" &&
      (("sourcePaneId" in value && typeof value.sourcePaneId === "string") ||
        ("leafId" in value && typeof value.leafId === "string"))
    );
  }
  return false;
}

export function isWorkspaceDropData(value: unknown): value is WorkspaceDropData {
  if (!value || typeof value !== "object") return false;
  if (!("type" in value)) return false;
  const type = value.type;
  switch (type) {
    case "tab":
      return (
        "tabId" in value &&
        typeof value.tabId === "string" &&
        "groupId" in value &&
        typeof value.groupId === "string" &&
        "index" in value &&
        typeof value.index === "number"
      );
    case "group-body":
      return "groupId" in value && typeof value.groupId === "string";
    case "group-edge":
      return "groupId" in value && typeof value.groupId === "string" && "edge" in value && isTabDropEdge(value.edge);
    case "pane-leaf":
      return "tabId" in value && typeof value.tabId === "string" && "leafId" in value && typeof value.leafId === "string";
    case "pane-edge":
      return (
        "tabId" in value &&
        typeof value.tabId === "string" &&
        "leafId" in value &&
        typeof value.leafId === "string" &&
        "edge" in value &&
        isTabDropEdge(value.edge)
      );
    default:
      return false;
  }
}

/**
 * Reports whether a pane-edge drop would rebuild the tree it started from. Supplied by
 * the layout owner because drop metadata alone cannot tell where a leaf sits in the tree.
 */
export type RedundantSplitCheck = (input: {
  tabId: string;
  sourceLeafId: string;
  targetLeafId: string;
  direction: PaneDirection;
  position: "first" | "second";
}) => boolean;

export function resolveWorkspaceDropCommand(
  active: WorkspaceDragData,
  over: WorkspaceDropData | null,
  isRedundantSplit?: RedundantSplitCheck,
): WorkspaceDropCommand | null {
  if (!over) return null;

  if (active.type === "tab") {
    switch (over.type) {
      case "pane-edge": {
        const { direction, position } = edgeToSplit(over.edge);
        return {
          type: "move-tab-to-pane-split",
          sourceTabId: active.tabId,
          targetTabId: over.tabId,
          targetLeafId: over.leafId,
          direction,
          position,
        };
      }
      case "group-edge": {
        const { direction, position } = edgeToSplit(over.edge);
        return {
          type: "move-tab-to-split",
          tabId: active.tabId,
          targetGroupId: over.groupId,
          direction,
          position,
        };
      }
      case "tab":
        if (over.tabId === active.tabId && over.groupId === active.groupId) return null;
        return {
          type: "move-tab-to-group",
          tabId: active.tabId,
          targetGroupId: over.groupId,
          targetIndex: over.index,
        };
      case "group-body":
        return {
          type: "move-tab-to-group",
          tabId: active.tabId,
          targetGroupId: over.groupId,
        };
      case "pane-leaf":
        return null;
    }
  }

  if (active.type === "pane") {
    const sourceLeafId = active.sourcePaneId ?? active.leafId ?? "";
    switch (over.type) {
      case "pane-edge": {
        if (over.tabId === active.tabId && over.leafId === sourceLeafId) return null;
        const { direction, position } = edgeToSplit(over.edge);
        if (
          over.tabId === active.tabId &&
          isRedundantSplit?.({
            tabId: active.tabId,
            sourceLeafId,
            targetLeafId: over.leafId,
            direction,
            position,
          })
        ) {
          return null;
        }
        return {
          type: "move-pane-to-pane-split",
          sourceTabId: active.tabId,
          sourceLeafId,
          targetTabId: over.tabId,
          targetLeafId: over.leafId,
          direction,
          position,
        };
      }
      case "tab":
        return {
          type: "detach-pane-to-tab",
          sourceTabId: active.tabId,
          leafId: sourceLeafId,
          targetGroupId: over.groupId,
          targetIndex: over.index + 1,
        };
      case "group-body":
        return {
          type: "detach-pane-to-tab",
          sourceTabId: active.tabId,
          leafId: sourceLeafId,
          targetGroupId: over.groupId,
        };
      case "pane-leaf":
        if (over.tabId !== active.tabId || over.leafId === sourceLeafId) return null;
        return {
          type: "swap-panes",
          tabId: active.tabId,
          sourceLeafId,
          targetLeafId: over.leafId,
        };
      case "group-edge":
        return null;
    }
  }

  return null;
}

export function dropPriority(active: WorkspaceDragData, over: WorkspaceDropData): number {
  if (active.type === "tab") {
    if (over.type === "pane-edge") return over.edge === "left" || over.edge === "right" ? 0 : 1;
    if (over.type === "group-edge") return over.edge === "left" || over.edge === "right" ? 2 : 3;
    if (over.type === "tab") return 4;
    if (over.type === "group-body") return 5;
    return 100;
  }

  if (over.type === "pane-edge") return over.edge === "left" || over.edge === "right" ? 0 : 1;
  if (over.type === "tab") return 2;
  if (over.type === "group-body") return 3;
  if (over.type === "pane-leaf") return 4;
  return 100;
}

export function edgeToSplit(edge: TabDropEdge): { direction: PaneDirection; position: "first" | "second" } {
  if (edge === "left") return { direction: "horizontal", position: "first" };
  if (edge === "right") return { direction: "horizontal", position: "second" };
  if (edge === "top") return { direction: "vertical", position: "first" };
  return { direction: "vertical", position: "second" };
}

function isTabDropEdge(value: unknown): value is TabDropEdge {
  return value === "left" || value === "right" || value === "top" || value === "bottom";
}
