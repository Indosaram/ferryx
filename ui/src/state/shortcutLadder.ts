import { groupProjects } from "../lib/projectGrouping";
import { projectRootWorktree } from "../lib/projectIdentity";
import { getCachedSshHosts } from "../lib/sshHosts";
import type { RegisteredProject, Worktree } from "../lib/types";
import { resolveWorktreeOwnerId } from "../lib/worktreeOwnership";

export type ShortcutLadderInput = {
  projects: RegisteredProject[];
  activeProjectId: string;
  activeWorktrees: Worktree[];
  inactiveProjectWorktrees?: Record<string, Worktree[]>;
};

function hostLabelFor(hostId: string): string {
  return getCachedSshHosts()?.find((host) => host.id === hostId)?.label ?? hostId;
}

/**
 * The ordered worktree rows behind Cmd+1..9.
 *
 * The index space is deliberately independent of sidebar state: a digit means the same
 * worktree whether a project is collapsed, whether its snapshot has loaded yet, and whether
 * the worktree currently owns a tab. Collapse state and tab ownership still govern what the
 * sidebar renders, but they must not renumber navigation — a ladder that shifts under the
 * user is indistinguishable from a broken shortcut. Selecting a tabless row opens a tab.
 *
 * Project order matches the sidebar so the displayed shortcut hints and the actual digits
 * agree.
 */
export function listShortcutWorktrees({
  projects,
  activeProjectId,
  activeWorktrees,
  inactiveProjectWorktrees = {},
}: ShortcutLadderInput): Worktree[] {
  const ladder: Worktree[] = [];
  const push = (rows: Worktree[]) => {
    for (const row of rows) {
      if (ladder.some((candidate) => candidate.path === row.path && candidate.workspaceId === row.workspaceId)) continue;
      ladder.push(row);
    }
  };

  for (const group of groupProjects(projects)) {
    const project = group.primaryProject;
    const owned = activeWorktrees.filter(
      (worktree) => resolveWorktreeOwnerId(worktree, projects, activeProjectId) === project.workspaceId,
    );
    const cached = inactiveProjectWorktrees[project.workspaceId] ?? [];
    const rows =
      project.workspaceId === activeProjectId
        ? (owned.length > 0 ? owned : cached)
        : [...cached, ...owned];

    if (project.target?.kind === "ssh" && group.memberProjects.length === 1) {
      if (!rows.some((row) => row.path === project.repoRoot && row.workspaceId === project.workspaceId)) {
        rows.push(projectRootWorktree(project, hostLabelFor(project.target.hostId)));
      }
    } else {
      // A folder that is not a Git repository, or the project the user is already in, always
      // owns a root row: otherwise its digit would be silent until a listing arrives. An idle
      // Git project with no cached rows contributes nothing yet, because inventing a row for it
      // would push a phantom entry in front of the active project's first digit.
      if (rows.length === 0 && (project.gitRoot === null || project.workspaceId === activeProjectId)) {
        rows.push(projectRootWorktree(project));
      }
      for (const member of group.memberProjects) {
        const target = member.target;
        if (member.workspaceId !== project.workspaceId && target?.kind !== "ssh") {
          const memberRows = member.workspaceId === activeProjectId
            ? activeWorktrees.filter((row) => resolveWorktreeOwnerId(row, projects, activeProjectId) === member.workspaceId)
            : inactiveProjectWorktrees[member.workspaceId] ?? [];
          rows.push(...memberRows.map((row) => ({ ...row, workspaceId: row.workspaceId ?? member.workspaceId })));
        }
        if (target?.kind === "ssh") {
          if (!rows.some((row) => row.path === member.repoRoot && row.workspaceId === member.workspaceId)) {
            rows.push(projectRootWorktree(member, hostLabelFor(target.hostId)));
          }
        }
      }
    }

    push(rows);
  }

  return ladder;
}

/** Stable identity for debug receipts; never used for navigation. */
export function describeLadderRow(row: Worktree | undefined): string | null {
  return row ? `${row.workspaceId ?? "local"}:${row.path}` : null;
}
