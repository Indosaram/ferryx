import type { WorkspaceState } from "./workspaceStore";

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
