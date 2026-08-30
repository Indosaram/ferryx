import { worktreeIdentity, type RegisteredProject, type Worktree } from "./types";

/** Paths may carry mixed separators or trailing slashes; comparisons must not. */
function normalizePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "");
}

function ownsPath(root: string, path: string): boolean {
  const normalizedRoot = normalizePath(root);
  const normalizedPath = normalizePath(path);
  return normalizedPath === normalizedRoot || normalizedPath.startsWith(`${normalizedRoot}/`);
}

/**
 * A worktree names its owner in an `orca/<wsId>/<slug>` branch. Rows without that
 * identity - plain-folder roots, main checkouts, externally created worktrees -
 * are owned by the project whose repoRoot contains their path, never by whichever
 * project happens to be active. A branch that names a project which does not own
 * the row's path is not trusted, so an external or mislabeled branch cannot move
 * a row into another project.
 */
export function resolveWorktreeOwnerId(
  worktree: Worktree,
  projects: readonly RegisteredProject[],
  fallbackProjectId?: string,
): string | undefined {
  const identityOwner = worktreeIdentity(worktree)?.wsId;
  if (
    identityOwner &&
    projects.some((project) => project.workspaceId === identityOwner) &&
    // A managed worktree legitimately lives inside some project's root
    // (`.orca-worktrees/`), so only an exact root match outranks its branch.
    !projects.some(
      (project) =>
        project.workspaceId !== identityOwner &&
        normalizePath(project.repoRoot) === normalizePath(worktree.path),
    )
  ) {
    return identityOwner;
  }

  const pathOwner = projects
    .filter((project) => ownsPath(project.repoRoot, worktree.path))
    .sort((left, right) => normalizePath(right.repoRoot).length - normalizePath(left.repoRoot).length)[0];
  if (pathOwner) return pathOwner.workspaceId;

  return fallbackProjectId;
}
