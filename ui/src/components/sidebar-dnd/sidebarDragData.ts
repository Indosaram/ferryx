import type { WorktreeOrder } from "../../lib/sidebarPersistence";
import type { Worktree } from "../../lib/types";
import { worktreeSortableId } from "../sidebar/sidebarRows";

type SidebarProjectDragData = {
  type: "sidebar-project";
  workspaceId: string;
};

type SidebarWorktreeDragData = {
  type: "sidebar-worktree";
  workspaceId: string;
  worktreePath: string;
  rowWorkspaceId?: string;
};

export type SidebarDragData = SidebarProjectDragData | SidebarWorktreeDragData;

export function pruneWorktreeOrder(order: WorktreeOrder, grouped: Map<string, Worktree[]>) {
  const pruned: WorktreeOrder = {};
  for (const [workspaceId, storedPaths] of Object.entries(order)) {
    const rows = grouped.get(workspaceId) ?? [];
    const seen = new Set<string>();
    const next = storedPaths.flatMap((path) => {
      const row = rows.find((row) => worktreeSortableId(workspaceId, row.path, row.workspaceId) === path)
        ?? rows.find((row) => (row.path === path || (row.workspaceId && `${row.workspaceId}:${row.path}` === path)) &&
          !seen.has(worktreeSortableId(workspaceId, row.path, row.workspaceId)));
      if (!row) return [];
      const id = worktreeSortableId(workspaceId, row.path, row.workspaceId);
      if (seen.has(id)) return [];
      seen.add(id);
      return [id];
    });
    if (next.length > 0) pruned[workspaceId] = next;
  }
  return pruned;
}

export function worktreeOrdersEqual(left: WorktreeOrder, right: WorktreeOrder) {
  const leftEntries = Object.entries(left);
  const rightEntries = Object.entries(right);
  if (leftEntries.length !== rightEntries.length) return false;
  return leftEntries.every(([workspaceId, paths]) => {
    const other = right[workspaceId];
    return other?.length === paths.length && paths.every((path, index) => path === other[index]);
  });
}

export function readSidebarDragData(value: unknown): SidebarDragData | null {
  if (!value || typeof value !== "object" || !("type" in value)) return null;
  if (value.type === "sidebar-project") {
    return "workspaceId" in value && typeof value.workspaceId === "string"
      ? { type: value.type, workspaceId: value.workspaceId }
      : null;
  }
  if (value.type === "sidebar-worktree") {
    return "workspaceId" in value &&
      typeof value.workspaceId === "string" &&
      "worktreePath" in value &&
      typeof value.worktreePath === "string"
      ? { type: value.type, workspaceId: value.workspaceId, worktreePath: value.worktreePath,
          rowWorkspaceId: "rowWorkspaceId" in value && typeof value.rowWorkspaceId === "string" ? value.rowWorkspaceId : undefined }
      : null;
  }
  return null;
}
