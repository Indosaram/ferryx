// Maps a machine's session inventory onto the live signals the shared sidebar derives row and
// project state from. Inventory rows name a worktree by workspace id and slug; the sidebar keys
// live state by the row's path, so each model row is matched to its inventory rows here.
import { sidebarLiveState, type SidebarLive, type SidebarModel } from "../components/sidebar/sidebarModel";
import { summarizeActivities, type ActivitySummary, type TerminalActivity } from "../lib/activity";
import type { RemoteSessionInventory } from "./remoteSessionInventory";

/** The worktree the remote is attached to, as the inventory names it. */
export type RemoteSidebarSelection = { readonly workspaceId: string; readonly worktreeSlug: string | null };

export function remoteSidebarLive(
  model: SidebarModel,
  inventory: RemoteSessionInventory | null,
  selection: RemoteSidebarSelection | null,
): SidebarLive {
  const activityByWorktreePath: Record<string, ActivitySummary> = {};
  let activeWorkspaceId: string | undefined;
  let activePath: string | undefined;
  for (const project of model.projects) {
    for (const node of project.worktrees) {
      if (selection && node.workspaceId === selection.workspaceId && node.worktreeSlug === selection.worktreeSlug) {
        activeWorkspaceId = node.workspaceId;
        activePath = node.path;
      }
      const activities: TerminalActivity[] = (inventory?.entries ?? []).flatMap((entry) =>
        entry.activityState && entry.workspaceId === node.workspaceId && entry.worktreeSlug === node.worktreeSlug
          ? [{ state: entry.activityState, title: entry.title ?? "", isAgent: entry.agentType !== null, ...(entry.agentType ? { agentType: entry.agentType } : {}) }]
          : []);
      if (activities.length > 0) activityByWorktreePath[node.path] = summarizeActivities(activities);
    }
  }
  return sidebarLiveState(model, {
    activityByWorktreePath,
    ...(activeWorkspaceId === undefined ? {} : { activeWorkspaceId }),
    ...(activePath === undefined ? {} : { activePath }),
  });
}
