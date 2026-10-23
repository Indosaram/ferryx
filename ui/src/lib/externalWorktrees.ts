import { EXTERNAL_WORKTREE_PROMPTED_STORAGE_KEY } from "./storageKeys";
import type { ExternalWorktree, RegisteredProject } from "./tauri";

type PromptStorage = Pick<Storage, "getItem" | "setItem">;

function defaultStorage(): PromptStorage | null {
  return typeof window !== "undefined" && window.localStorage ? window.localStorage : null;
}

function normalizePath(path: string): string {
  return path.replace(/[\\/]+$/, "");
}

/** Paths the user already answered (imported or declined), so the prompt never repeats them. */
export function loadPromptedExternalWorktrees(storage: PromptStorage | null = defaultStorage()): Set<string> {
  try {
    const parsed: unknown = JSON.parse(storage?.getItem(EXTERNAL_WORKTREE_PROMPTED_STORAGE_KEY) ?? "[]");
    return new Set(Array.isArray(parsed) ? parsed.filter((value): value is string => typeof value === "string") : []);
  } catch {
    return new Set();
  }
}

export function rememberPromptedExternalWorktrees(
  paths: readonly string[],
  storage: PromptStorage | null = defaultStorage(),
): void {
  const next = loadPromptedExternalWorktrees(storage);
  for (const path of paths) next.add(normalizePath(path));
  try {
    storage?.setItem(EXTERNAL_WORKTREE_PROMPTED_STORAGE_KEY, JSON.stringify([...next]));
  } catch {
    // Without persistence the prompt may reappear next launch; the import itself still stands.
  }
}

/**
 * Outside-project worktrees still worth asking about: not already a local project, not answered
 * before, and not hidden for this session.
 */
export function pendingExternalWorktrees(
  discovered: readonly ExternalWorktree[],
  projects: readonly RegisteredProject[],
  prompted: ReadonlySet<string>,
  hiddenThisSession: ReadonlySet<string> = new Set(),
): ExternalWorktree[] {
  const localRoots = new Set(
    projects
      .filter((project) => !project.target || project.target.kind === "local")
      .map((project) => normalizePath(project.repoRoot)),
  );
  return discovered.filter((worktree) => {
    const path = normalizePath(worktree.path);
    return !localRoots.has(path) && !prompted.has(path) && !hiddenThisSession.has(path);
  });
}
