import type { RegisteredProject, Worktree } from "./types";

/** Persisted data is untrusted: invalid remote targets must never become local. */
export function hasValidProjectTarget(project: { workspaceId: string; target?: unknown; remoteWorkspaceId?: unknown }): boolean {
  const target = project.target;
  const pairedId = project.workspaceId.startsWith("daemon:");
  if (target === undefined) return !pairedId && !project.workspaceId.startsWith("ssh:");
  if (!target || typeof target !== "object" || !("kind" in target)) return false;
  switch (target.kind) {
    case "local": return !pairedId && !project.workspaceId.startsWith("ssh:");
    case "ssh": return !pairedId && "hostId" in target &&
      typeof target.hostId === "string" && target.hostId.trim().length > 0;
    case "pairedDaemon": return /^daemon:[a-f0-9]{64}$/.test(project.workspaceId) &&
      "hostId" in target && typeof target.hostId === "string" && target.hostId.trim().length > 0 &&
      typeof project.remoteWorkspaceId === "string" && project.remoteWorkspaceId.trim().length > 0;
    default: return false;
  }
}

export function projectRootWorktree(project: RegisteredProject, hostLabel?: string): Worktree {
  const isRemote = project.target?.kind === "ssh";
  const branch = project.gitBranch !== undefined ? project.gitBranch : null;
  const head = project.gitHead ?? "";
  const detached = Boolean(head && !branch);
  const resolvedHostLabel = hostLabel ?? project.hostLabel;
  return {
    ...(isRemote ? { workspaceId: project.workspaceId, hostLabel: resolvedHostLabel } : {}),
    path: project.repoRoot,
    head,
    branch,
    bare: false,
    detached,
    locked: null,
    prunable: null,
  };
}
