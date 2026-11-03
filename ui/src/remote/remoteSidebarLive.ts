// Live signals for one machine's sidebar (sidebarLiveState, called by RemoteSidebar with its
// filter). A desktop-published snapshot carries the desktop's own reduced signals, activity
// included, and those are used as-is so both apps draw from identical inputs. Only a snapshot
// without published activity (the inventory fallback, or a desktop older than that field) has its
// activity rebuilt from the session inventory, matched to model rows by workspace id and slug.
import type { SidebarLiveSignals, SidebarModel } from "../components/sidebar/sidebarModel";
import { supervisorTypingIn } from "../components/sidebar/sidebarSupervisor";
import { actingSessionIds, type SupervisorSnapshot } from "../lib/supervisorModel";
import { summarizeActivities, type ActivitySummary, type TerminalActivity } from "../lib/activity";
import type { RemoteSessionInventory } from "./remoteSessionInventory";

/** The worktree the remote is attached to, as the inventory names it. */
export type RemoteSidebarSelection = { readonly workspaceId: string; readonly worktreeSlug: string | null };

/** A machine's hub supervisor snapshot and the clock that expires its acting window. */
export type RemoteSidebarSupervisor = { readonly snapshot: SupervisorSnapshot; readonly now: number };

/** The Supervisor tab's typing mark: any machine's hub typed within the acting window. */
export function remoteSupervisorTyping(supervisors: Readonly<Record<string, RemoteSidebarSupervisor>>): boolean {
  return Object.values(supervisors).some(({ snapshot, now }) => actingSessionIds(snapshot, now).size > 0);
}

export function remoteSidebarSignals(
  model: SidebarModel,
  inventory: RemoteSessionInventory | null,
  selection: RemoteSidebarSelection | null,
  supervisor: RemoteSidebarSupervisor | null = null,
): SidebarLiveSignals {
  const published = model.signals?.activityByWorktreePath;
  const activityByWorktreePath: Record<string, ActivitySummary> = {};
  let activeWorkspaceId: string | undefined;
  let activePath: string | undefined;
  for (const project of model.projects) {
    for (const node of project.worktrees) {
      if (selection && node.workspaceId === selection.workspaceId && node.worktreeSlug === selection.worktreeSlug) {
        activeWorkspaceId = node.workspaceId;
        activePath = node.path;
      }
      if (published) continue;
      const activities: TerminalActivity[] = (inventory?.entries ?? []).flatMap((entry) =>
        entry.activityState && entry.workspaceId === node.workspaceId && entry.worktreeSlug === node.worktreeSlug
          ? [{
              state: entry.activityState,
              title: entry.title ?? "",
              isAgent: entry.agentType !== null,
              ...(entry.agentType ? { agentType: entry.agentType } : {}),
              ...(entry.activityOutcome ? { outcome: entry.activityOutcome } : {}),
            }]
          : []);
      if (activities.length > 0) activityByWorktreePath[node.path] = summarizeActivities(activities);
    }
  }
  return {
    activityByWorktreePath,
    ...(activeWorkspaceId === undefined ? {} : { activeWorkspaceId }),
    ...(activePath === undefined ? {} : { activePath }),
    // The desktop's activity, bell marks and bare agent states, published with the snapshot; its
    // activity replaces the inventory reconstruction above whenever it is present.
    ...(model.signals ?? {}),
    ...(supervisor ? { isSupervisorTyping: (path: string) => supervisorTypingIn(supervisor.snapshot, supervisor.now, path) } : {}),
  };
}
