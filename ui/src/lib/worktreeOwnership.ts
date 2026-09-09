import { worktreeIdentity, type RegisteredProject, type Worktree } from "./types";

/** Paths may carry mixed separators or trailing slashes; comparisons must not. */
function normalizePath(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  return /^[a-z]:\//i.test(normalized) || normalized.startsWith("//")
    ? normalized.toLowerCase()
    : normalized;
}

function ownsPath(root: string, path: string): boolean {
  const normalizedRoot = normalizePath(root);
  const normalizedPath = normalizePath(path);
  return normalizedPath === normalizedRoot || normalizedPath.startsWith(`${normalizedRoot}/`);
}

function uniqueDeepestRemoteOwner(
  projects: readonly RegisteredProject[],
  path: string,
): RegisteredProject | undefined {
  const candidates = projects
    .filter((project) => project.target?.kind === "ssh" && ownsPath(project.repoRoot, path))
    .sort((left, right) => normalizePath(right.repoRoot).length - normalizePath(left.repoRoot).length);
  if (candidates.length === 0) return undefined;

  const deepestLength = normalizePath(candidates[0].repoRoot).length;
  const deepest = candidates.filter((project) => normalizePath(project.repoRoot).length === deepestLength);
  return deepest.length === 1 ? deepest[0] : undefined;
}

/**
 * A worktree names its owner in an `orca/<wsId>/<slug>` branch. Anything without that
 * identity is attributed by its registered root. Remote rows returned by the backend do
 * not currently carry `workspaceId`, so a unique SSH repo-root match must be recovered
 * before the local-project fallback; otherwise a Windows path can be routed to a local
 * macOS workspace and fail local absolute-path validation.
 */
export function resolveWorktreeOwnerId(
  worktree: Worktree,
  projects: readonly RegisteredProject[],
  fallbackProjectId?: string,
): string | undefined {
  if (worktree.workspaceId && projects.some((project) => project.workspaceId === worktree.workspaceId)) {
    return worktree.workspaceId;
  }

  const remoteOwner = uniqueDeepestRemoteOwner(projects, worktree.path);
  if (remoteOwner) return remoteOwner.workspaceId;

  const localProjects = projects.filter((project) => project.target?.kind !== "ssh");
  const identityOwner = worktreeIdentity(worktree)?.wsId;
  if (
    identityOwner &&
    localProjects.some((project) => project.workspaceId === identityOwner) &&
    // A managed worktree legitimately lives inside some project's root
    // (`.orca-worktrees/`), so only an exact root match outranks its branch.
    !localProjects.some(
      (project) =>
        project.workspaceId !== identityOwner &&
        normalizePath(project.repoRoot) === normalizePath(worktree.path),
    )
  ) {
    return identityOwner;
  }

  const pathOwner = localProjects
    .filter((project) => ownsPath(project.repoRoot, worktree.path))
    .sort((left, right) => normalizePath(right.repoRoot).length - normalizePath(left.repoRoot).length)[0];
  if (pathOwner) return pathOwner.workspaceId;

  return fallbackProjectId;
}
