import { workspaceName } from "../../lib/branchFilter";
import type { ProjectGroup } from "../../lib/projectGrouping";
import type { RegisteredProject } from "../../lib/tauri";
import { worktreeIdentity, type Worktree } from "../../lib/types";
import type { RemoteHostState } from "../../state/remoteHostStore";

export type SidebarHost = { readonly id: string; readonly label: string };

/** Where host and machine names come from when a sidebar label is computed. */
export type SidebarLabelContext = {
  readonly hosts: readonly SidebarHost[];
  readonly remoteState: RemoteHostState;
};

export const SIDEBAR_EMPTY_TEXT = {
  noProjects: "No projects registered yet.",
  noWorktrees: "No worktrees listed yet.",
} as const;

/** The name a worktree row shows: its managed slug, else the workspace title. */
export function worktreeLabel(worktree: Worktree): string {
  return worktreeIdentity(worktree)?.slug ?? workspaceName(worktree);
}

export function isRemoteWorktreeRow(worktree: Pick<Worktree, "workspaceId">): boolean {
  return Boolean(worktree.workspaceId?.startsWith("ssh:") || worktree.workspaceId?.startsWith("daemon:"));
}

/** Machine name a remote row carries in its state slot; undefined for local rows. */
export function worktreeRowHost(worktree: Worktree): string | undefined {
  return isRemoteWorktreeRow(worktree) ? (worktree.hostLabel ?? worktree.hostSummary ?? "Remote") : undefined;
}

function projectFolderName(project: RegisteredProject): string {
  return project.repoRoot.replace(/[\\/]+$/, "").split(/[\\/]/).filter(Boolean).at(-1) ?? "/";
}

/** Host or machine name of a remote project; null for a local one. */
export function projectHostLabel(project: RegisteredProject, context: SidebarLabelContext): string | null {
  const remote = project.target && project.target.kind !== "local" ? project.target : null;
  if (!remote) return null;
  return remote.kind === "pairedDaemon"
    ? (context.remoteState.hosts[remote.hostId]?.name ?? project.hostLabel ?? remote.hostId)
    : (context.hosts.find((host) => host.id === remote.hostId)?.label ?? project.hostLabel ?? remote.hostId);
}

/** The project header text. A remote project shows its folder, plus the host when it stands alone. */
export function projectDisplayLabel(project: RegisteredProject, hostLabel: string | null, showHost: boolean): string {
  if (!project.target || project.target.kind === "local") return project.workspaceId;
  const folderName = projectFolderName(project);
  return showHost && hostLabel ? `${folderName} (${hostLabel})` : folderName;
}

/** A lone SSH project renders as one root row instead of a worktree list. */
export function isStandaloneRemoteGroup(group: ProjectGroup): boolean {
  return group.primaryProject.target?.kind === "ssh" &&
    group.memberProjects.length === 1 &&
    !group.memberProjects.some((p) => p.target?.kind !== "ssh");
}

/** A lone SSH or paired project names its host in the header. */
export function groupHeaderShowsHost(group: ProjectGroup): boolean {
  return isStandaloneRemoteGroup(group) ||
    (group.primaryProject.target?.kind === "pairedDaemon" && group.memberProjects.length === 1);
}

/** Mirrors the project header label so the filter matches what the user reads. */
export function projectFilterLabels(group: ProjectGroup, context: SidebarLabelContext): string[] {
  return group.memberProjects.flatMap((project) => {
    const hostLabel = projectHostLabel(project, context);
    return hostLabel === null ? [project.workspaceId] : [projectFolderName(project), hostLabel];
  });
}

export type FilteredEntry<G, R> = {
  readonly group: G;
  readonly rows: R[];
  /** True while a non-empty filter matched one of this project's worktrees. */
  readonly forceExpanded: boolean;
};

/**
 * Case-insensitive substring filter over project labels and worktree display names. A project
 * whose label matches keeps all its rows; otherwise only matching rows stay and the project is
 * forced open so they are visible.
 */
export function filterSidebarEntries<G, R>(
  entries: readonly { readonly group: G; readonly rows: R[] }[],
  query: string,
  labels: { readonly project: (group: G) => readonly string[]; readonly row: (row: R) => string },
): FilteredEntry<G, R>[] {
  const needle = query.trim().toLowerCase();
  const all = entries.map((entry) => ({ group: entry.group, rows: entry.rows, forceExpanded: false }));
  if (!needle) return all;
  return all.flatMap((entry) => {
    const matchingRows = entry.rows.filter((row) => labels.row(row).toLowerCase().includes(needle));
    const projectMatches = labels.project(entry.group).some((label) => label.toLowerCase().includes(needle));
    if (projectMatches) return [{ ...entry, forceExpanded: matchingRows.length > 0 }];
    if (matchingRows.length === 0) return [];
    return [{ ...entry, rows: matchingRows, forceExpanded: true }];
  });
}
