import type { WorkspaceState } from "./workspaceStore";

const snapshots = new Map<string, WorkspaceState>();

export function getWorkspaceSnapshot(workspaceId: string): WorkspaceState | null {
  return snapshots.get(workspaceId) ?? null;
}

export function setWorkspaceSnapshot(workspaceId: string, state: WorkspaceState): void {
  snapshots.set(workspaceId, state);
}

export function clearWorkspaceSnapshot(workspaceId?: string): void {
  if (workspaceId) {
    snapshots.delete(workspaceId);
    return;
  }
  snapshots.clear();
}
