import type { PairedHostReachabilityView } from "../../lib/pairedHostReachability";
import { projectRootWorktree } from "../../lib/projectIdentity";
import { groupProjects, isProjectGroupActive, type ProjectGroup } from "../../lib/projectGrouping";
import type { WorktreeOrder } from "../../lib/sidebarPersistence";
import type { RegisteredProject } from "../../lib/tauri";
import type { Worktree } from "../../lib/types";
import { resolveWorktreeOwnerId } from "../../lib/worktreeOwnership";
import type { SidebarLabelContext } from "./sidebarLabels";
import { pairedSlotStatus } from "./sidebarSlots";

export type SidebarRowsInput = SidebarLabelContext & {
  readonly worktrees: readonly Worktree[];
  readonly projects: RegisteredProject[];
  readonly activeProjectId: string | undefined;
  readonly inactiveProjectWorktrees?: Readonly<Record<string, Worktree[]>>;
  /** Read once per build; the caller rebuilds when reachability changes. */
  readonly reachability: (hostId: string) => PairedHostReachabilityView;
};

export function worktreeSortableId(workspaceId: string, worktreePath: string, rowWorkspaceId?: string) {
  return rowWorkspaceId
    ? `sidebar-worktree:${workspaceId}:${rowWorkspaceId}:${worktreePath}`
    : `sidebar-worktree:${workspaceId}:${worktreePath}`;
}

/** Groups never toggled default to collapsed unless they hold the active project. */
export function seedCollapsedProjects(
  collapsed: Set<string>,
  projects: RegisteredProject[],
  activeProjectId: string | undefined,
) {
  const groups = groupProjects(projects);
  const activeGroup = groups.find((g) => isProjectGroupActive(g, activeProjectId));
  const seeded = groups.filter(
    (group) => group.groupId !== activeGroup?.groupId && !collapsed.has(group.groupId),
  );
  if (seeded.length === 0) return collapsed;
  const next = new Set(collapsed);
  for (const group of seeded) next.add(group.groupId);
  return next;
}

/** Groups whose every member has no worktrees at all; they always start collapsed. */
export function emptyProjectGroupIds(groups: readonly ProjectGroup[], emptyWorkspaceIds: readonly string[] | undefined) {
  return new Set(groups
    .filter((group) => group.memberProjects.every((member) => emptyWorkspaceIds?.includes(member.workspaceId)))
    .map((group) => group.groupId));
}

/**
 * Once the user has toggled anything the persisted set is authoritative; only a first run
 * (nothing persisted) falls back to "everything but the active project starts collapsed".
 */
export function initialCollapsedProjects(input: {
  readonly stored: Iterable<string> | null;
  readonly projects: RegisteredProject[];
  readonly activeProjectId: string | undefined;
  readonly emptyGroupIds: ReadonlySet<string>;
}): Set<string> {
  return new Set([
    ...(input.stored ?? seedCollapsedProjects(new Set<string>(), input.projects, input.activeProjectId)),
    ...input.emptyGroupIds,
  ]);
}

export function applyWorktreeOrder(grouped: Map<string, Worktree[]>, order: WorktreeOrder) {
  const ordered = new Map<string, Worktree[]>();
  for (const [workspaceId, rows] of grouped) {
    const rowKey = (r: Worktree) => worktreeSortableId(workspaceId, r.path, r.workspaceId);
    const byKey = new Map(rows.map((row) => [rowKey(row), row]));
    const seen = new Set<string>();
    const next: Worktree[] = [];
    for (const path of order[workspaceId] ?? []) {
      // Prefer the qualified ID, then accept previously stored member keys or paths.
      const row = byKey.get(path) ?? rows.find((r) =>
        (r.path === path || (r.workspaceId && `${r.workspaceId}:${r.path}` === path)) && !seen.has(rowKey(r)));
      if (!row || seen.has(rowKey(row))) continue;
      seen.add(rowKey(row));
      next.push(row);
    }
    for (const row of rows) {
      const key = rowKey(row);
      if (seen.has(key)) continue;
      seen.add(key);
      next.push(row);
    }
    ordered.set(workspaceId, next);
  }
  return ordered;
}

const sameRoot = (a: string, b: string) =>
  a.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase() === b.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();

/**
 * Worktrees carry their owning project inside their `orca/<wsId>/<slug>` branch name. Anything
 * without that identity is attributed by path - a project root, or a path nested under one - so a
 * plain-folder project's root row never lands under whichever project happens to be active.
 */
export function groupWorktreesByProject(input: SidebarRowsInput) {
  const { worktrees, projects, activeProjectId, inactiveProjectWorktrees, hosts, remoteState } = input;
  const rowSummary = (hostId: string) => pairedSlotStatus(hostId, remoteState, input.reachability(hostId)).label;
  const groups = groupProjects(projects);
  const grouped = new Map<string, Worktree[]>();
  for (const group of groups) {
    const primaryId = group.primaryProject.workspaceId;
    const listed = group.memberProjects.flatMap((member) => {
      const hasActiveWorktrees = worktrees.some(
        (w) => resolveWorktreeOwnerId(w, projects, activeProjectId) === member.workspaceId,
      );
      if (member.workspaceId === activeProjectId && hasActiveWorktrees) return [];
      return (inactiveProjectWorktrees?.[member.workspaceId] ?? []).map((row) =>
        (member.workspaceId === primaryId && member.target?.kind !== "pairedDaemon") || row.workspaceId
          ? row
          : { ...row, workspaceId: member.workspaceId });
    });
    grouped.set(primaryId, listed);
  }

  for (const worktree of worktrees) {
    const owner = resolveWorktreeOwnerId(worktree, projects, activeProjectId);
    if (!owner) continue;
    const targetGroup = groups.find((g) => g.memberProjects.some((m) => m.workspaceId === owner));
    const targetId = targetGroup ? targetGroup.primaryProject.workspaceId : owner;
    const bucket = grouped.get(targetId);
    const resolvedWorkspaceId = worktree.workspaceId ?? owner;
    if (
      !bucket ||
      bucket.some(
        (candidate) =>
          candidate.path === worktree.path &&
          (candidate.workspaceId ?? targetId) === resolvedWorkspaceId,
      )
    ) {
      continue;
    }
    const member = targetGroup?.memberProjects.find((m) => m.workspaceId === resolvedWorkspaceId);
    let resolvedWorktree = owner !== targetId && !worktree.workspaceId
      ? { ...worktree, workspaceId: owner }
      : worktree;
    const memberTarget = member?.target;
    if (!resolvedWorktree.hostLabel && memberTarget?.kind === "ssh") {
      const hostLabel = hosts.find((h) => h.id === memberTarget.hostId)?.label ?? memberTarget.hostId;
      resolvedWorktree = { ...resolvedWorktree, hostLabel, ...(worktree.workspaceId ? {} : { workspaceId: owner }) };
    }
    bucket.push(resolvedWorktree);
  }

  if (activeProjectId) {
    const activeGroup = groups.find((g) => g.memberProjects.some((m) => m.workspaceId === activeProjectId));
    const activePrimaryId = activeGroup ? activeGroup.primaryProject.workspaceId : activeProjectId;
    const activeBucket = grouped.get(activePrimaryId);
    if (activeBucket) {
      const activeProj = projects.find((p) => p.workspaceId === activeProjectId);
      const isLocalActive = activeProj?.target?.kind !== "ssh";
      const hasLocalActiveWorktree = activeBucket.some(
        (candidate) => (candidate.workspaceId ?? activePrimaryId) === activeProjectId && !candidate.hostLabel,
      );
      if (isLocalActive && !hasLocalActiveWorktree) {
        const cached = inactiveProjectWorktrees?.[activeProjectId] ?? inactiveProjectWorktrees?.[activePrimaryId];
        for (const row of cached ?? []) {
          if (
            !activeBucket.some(
              (candidate) =>
                candidate.path === row.path &&
                (candidate.workspaceId ?? activePrimaryId) === (row.workspaceId ?? activePrimaryId),
            )
          ) {
            activeBucket.push(row);
          }
        }
      }
    }
  }

  // A non-Git project has no git worktrees at all, so without a synthesized
  // folder row its group would render empty and the folder would be
  // unselectable - including while it is the active project.
  for (const group of groups) {
    const project = group.primaryProject;
    const bucket = grouped.get(project.workspaceId);
    if (!bucket) continue;
    if (project.gitRoot === null && project.target?.kind !== "ssh" && bucket.length === 0) {
      bucket.push(projectRootWorktree(project));
    }

    for (const member of group.memberProjects) {
      const target = member.target;
      if (target?.kind !== "ssh" && target?.kind !== "pairedDaemon") continue;
      const existingIndex = bucket.findIndex(
        (candidate) =>
          candidate.path === member.repoRoot &&
          (candidate.workspaceId ?? project.workspaceId) === member.workspaceId,
      );
      const hostLabel = (target.kind === "pairedDaemon" ? remoteState.hosts[target.hostId]?.name : hosts.find((h) => h.id === target.hostId)?.label) ?? member.hostLabel ?? target.hostId;
      if (target.kind === "pairedDaemon") {
        for (let i = 0; i < bucket.length; i++) {
          if ((bucket[i].workspaceId ?? project.workspaceId) === member.workspaceId) {
            const row = bucket[i];
            const isStale = Boolean(row.stale || row.freshness?.stale);
            const summary = isStale && row.hostSummary ? row.hostSummary : rowSummary(target.hostId);
            bucket[i] = { ...row, workspaceId: member.workspaceId, hostLabel, hostSummary: summary };
          }
        }
      }
      if (existingIndex === -1) {
        bucket.push({ ...projectRootWorktree(member, hostLabel), ...(target.kind === "pairedDaemon" ? { hostSummary: rowSummary(target.hostId) } : {}) });
      } else {
        const row = bucket[existingIndex];
        const remoteFolder = target.kind === "ssh" && member.gitRoot !== null && member.gitRoot !== undefined &&
          !sameRoot(member.gitRoot, member.repoRoot);
        const newBranch = remoteFolder ? null : member.gitBranch !== undefined ? member.gitBranch : row.branch;
        const newHead = remoteFolder ? "" : member.gitHead !== undefined ? (member.gitHead ?? "") : row.head;
        bucket[existingIndex] = {
          ...row,
          hostLabel: hostLabel ?? row.hostLabel,
          branch: newBranch,
          head: newHead,
          detached: Boolean(newHead && !newBranch),
        };
      }
    }
  }

  return grouped;
}
