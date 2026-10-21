import type { WorkspaceState } from "./workspaceStore";

const snapshots = new Map<string, WorkspaceState>();
const listeners = new Set<() => void>();

export function subscribeWorkspaceSnapshots(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

function notify(): void {
  for (const listener of [...listeners]) listener();
}

export function getWorkspaceSnapshot(workspaceId: string): WorkspaceState | null {
  return snapshots.get(workspaceId) ?? null;
}

export function setWorkspaceSnapshot(workspaceId: string, state: WorkspaceState): void {
  if (snapshots.get(workspaceId) === state) return;
  snapshots.set(workspaceId, state);
  notify();
}

export function listWorkspaceSnapshots(): ReadonlyArray<readonly [string, WorkspaceState]> {
  return [...snapshots.entries()];
}

export function clearWorkspaceSnapshot(workspaceId?: string): void {
  if (workspaceId) {
    if (!snapshots.delete(workspaceId)) return;
    notify();
    return;
  }
  if (snapshots.size === 0) return;
  snapshots.clear();
  notify();
}
