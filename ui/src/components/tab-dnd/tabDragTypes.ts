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
  leafId: string;
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

export type WorkspaceDragData = TabDragData | PaneDragData;
export type WorkspaceDropData = TabDragData | GroupBodyDropData | GroupEdgeDropData | PaneLeafDropData;

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
  const candidate = value as Partial<WorkspaceDragData>;
  if (candidate.type === "tab") {
    return typeof candidate.tabId === "string" && typeof candidate.groupId === "string" && typeof candidate.index === "number";
  }
  if (candidate.type === "pane") {
    return typeof candidate.tabId === "string" && typeof candidate.leafId === "string";
  }
  return false;
}

export function isWorkspaceDropData(value: unknown): value is WorkspaceDropData {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<WorkspaceDropData>;
  switch (candidate.type) {
    case "tab":
      return typeof candidate.tabId === "string" && typeof candidate.groupId === "string" && typeof candidate.index === "number";
    case "group-body":
      return typeof candidate.groupId === "string";
    case "group-edge":
      return typeof candidate.groupId === "string" && isTabDropEdge(candidate.edge);
    case "pane-leaf":
      return typeof candidate.tabId === "string" && typeof candidate.leafId === "string";
    default:
      return false;
  }
}

export function resolveWorkspaceDropCommand(
  active: WorkspaceDragData,
  over: WorkspaceDropData | null,
): WorkspaceDropCommand | null {
  if (!over) return null;

  if (active.type === "tab") {
    switch (over.type) {
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
    switch (over.type) {
      case "tab":
        return {
          type: "detach-pane-to-tab",
          sourceTabId: active.tabId,
          leafId: active.leafId,
          targetGroupId: over.groupId,
          targetIndex: over.index + 1,
        };
      case "group-body":
        return {
          type: "detach-pane-to-tab",
          sourceTabId: active.tabId,
          leafId: active.leafId,
          targetGroupId: over.groupId,
        };
      case "pane-leaf":
        if (over.tabId !== active.tabId || over.leafId === active.leafId) return null;
        return {
          type: "swap-panes",
          tabId: active.tabId,
          sourceLeafId: active.leafId,
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
    if (over.type === "group-edge") return over.edge === "left" || over.edge === "right" ? 0 : 1;
    if (over.type === "tab") return 2;
    if (over.type === "group-body") return 3;
    return 100;
  }

  if (over.type === "tab") return 0;
  if (over.type === "group-body") return 1;
  if (over.type === "pane-leaf") return 2;
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
