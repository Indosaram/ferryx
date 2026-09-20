import type { TerminalSession } from "./types";

export function isLocalDagProject(project: { readonly target?: { readonly kind: string } | null } | null | undefined): boolean {
  if (!project) return true;
  return project.target?.kind !== "ssh" && project.target?.kind !== "pairedDaemon";
}

export function remoteProjectsWatchKey(
  projects: readonly {
    readonly workspaceId: string;
    readonly repoRoot: string;
    readonly target?: { readonly kind: string } | null;
  }[]
): string {
  return projects
    .filter((project) => !isLocalDagProject(project))
    .map((project) => `${project.workspaceId}:${project.repoRoot}:${project.target?.kind ?? ""}`)
    .sort()
    .join("\n");
}

/**
 * A dag journal is written under the directory its agent runs in - the session's own root,
 * which is often not a registered project. Watching only project roots hides every other
 * pane's runs from the dag store, so those panes can never show a badge.
 */
export function collectDagWatchRoots(input: {
  readonly projectRoots: readonly string[];
  readonly worktreePaths: readonly string[];
  readonly sessions: Readonly<Record<string, TerminalSession>> | readonly TerminalSession[];
}): string[] {
  const roots = new Set<string>();

  const add = (path: string | null | undefined): void => {
    if (typeof path !== "string") return;
    const trimmed = path.trim();
    if (trimmed.length > 0) roots.add(trimmed);
  };

  for (const path of input.projectRoots) add(path);
  for (const path of input.worktreePaths) add(path);

  const sessions = Array.isArray(input.sessions)
    ? input.sessions
    : Object.values(input.sessions);
  for (const session of sessions) {
    add(session.worktreePath);
    add(session.cwd);
  }

  return [...roots];
}
