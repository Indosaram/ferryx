import {
  getMigratedItem,
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_WIDTH_STORAGE_KEY,
  SIDEBAR_WORKTREE_ORDER_STORAGE_KEY,
} from "./storageKeys";

export const DEFAULT_SIDEBAR_WIDTH = 236;
export const MIN_SIDEBAR_WIDTH = 220;
export const MAX_SIDEBAR_WIDTH = 420;

export type WorktreeOrder = Record<string, string[]>;

export function clampSidebarWidth(width: number) {
  return Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, Math.round(width)));
}

export function loadWorktreeOrder(): WorktreeOrder {
  try {
    const raw = getMigratedItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return Object.fromEntries(
      Object.entries(parsed).flatMap(([workspaceId, paths]) => {
        if (!Array.isArray(paths)) return [];
        const uniquePaths = [...new Set(paths.filter((path): path is string => typeof path === "string"))];
        return uniquePaths.length > 0 ? [[workspaceId, uniquePaths]] : [];
      }),
    );
  } catch {
    return {};
  }
}

export function persistWorktreeOrder(order: WorktreeOrder) {
  try {
    window.localStorage.setItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY, JSON.stringify(order));
  } catch {
    // A storage failure should not break drag reordering for this session.
  }
}

/** Returns `null` when the accordion has never been persisted, so callers can apply defaults. */
export function loadCollapsedProjects(): Set<string> | null {
  try {
    const raw = getMigratedItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return null;
    return new Set(parsed.filter((id): id is string => typeof id === "string"));
  } catch {
    return null;
  }
}

export function persistCollapsedProjects(collapsed: Set<string>) {
  try {
    window.localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([...collapsed]));
  } catch {
    // A storage failure should not break accordion toggling for this session.
  }
}

export function loadSidebarWidth() {
  try {
    const raw = getMigratedItem(SIDEBAR_WIDTH_STORAGE_KEY);
    const stored = Number(raw);
    return Number.isFinite(stored) && stored > 0 ? clampSidebarWidth(stored) : DEFAULT_SIDEBAR_WIDTH;
  } catch {
    return DEFAULT_SIDEBAR_WIDTH;
  }
}

export function persistSidebarWidth(width: number) {
  try {
    window.localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(clampSidebarWidth(width)));
  } catch {
    // A storage failure should not break pointer resizing for this session.
  }
}
