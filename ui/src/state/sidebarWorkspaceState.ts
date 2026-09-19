import type { WorkspaceState } from "./workspaceStore";

/**
 * A worktree is a shortcut navigation target only when its layout currently owns
 * tabs: the live layout belongs to the state's active worktree, and every other
 * worktree keeps its tabs parked in `worktreeLayouts`.
 */
export function worktreeHasOpenTabs(state: WorkspaceState | undefined, path: string): boolean {
  if (!state) return false;
  if (state.activeWorktreePath === path) return state.layout.tabs.length > 0;
  if ((state.worktreeLayouts?.[path]?.tabs.length ?? 0) > 0) return true;
  // A workspace-level layout with no active worktree cannot be attributed to a
  // single row; keep every row navigable rather than dropping live tabs.
  return state.activeWorktreePath === null && state.layout.tabs.length > 0;
}

export function emptySidebarWorkspaceIds(
  projects: readonly { workspaceId: string }[],
  activeWorkspaceId: string,
  liveState: WorkspaceState,
  snapshots: ReadonlyArray<readonly [string, WorkspaceState]>,
): string[] {
  const states = new Map(snapshots);
  // During a project switch the live state still belongs to the outgoing project.
  states.set(liveState.workspaceId ?? activeWorkspaceId, liveState);
  return projects
    .filter((project) => {
      const state = states.get(project.workspaceId);
      return !state || (
        state.layout.tabs.length === 0 &&
        !Object.values(state.worktreeLayouts ?? {}).some((layout) => layout.tabs.length > 0)
      );
    })
    .map((project) => project.workspaceId);
}
