import type { RegisteredProject, Worktree } from "./types";

/** Persisted data is untrusted: an invalid SSH target must never become local. */
export function hasValidProjectTarget(project: { workspaceId: string; target?: unknown }): boolean {
  const target = project.target;
  if (target === undefined) return !project.workspaceId.startsWith("ssh:");
  if (!target || typeof target !== "object" || !("kind" in target)) return false;
  if (target.kind === "local") return !project.workspaceId.startsWith("ssh:");
  return target.kind === "ssh" && "hostId" in target &&
    typeof target.hostId === "string" && target.hostId.trim().length > 0;
}

export function projectRootWorktree(project: RegisteredProject, hostLabel?: string): Worktree {
  return {
    ...(project.target?.kind === "ssh" ? { workspaceId: project.workspaceId, hostLabel } : {}),
    path: project.repoRoot, head: "", branch: null, bare: false,
    detached: false, locked: null, prunable: null,
  };
}
