import {
  getMigratedItem,
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_WIDTH_STORAGE_KEY,
  SIDEBAR_WORKTREE_ORDER_STORAGE_KEY,
} from "./storageKeys";

export const DEFAULT_SIDEBAR_WIDTH = 236;
export const MIN_SIDEBAR_WIDTH = 220;
export const MAX_SIDEBAR_WIDTH = 420;

export type SidebarWidthRange = { readonly min: number; readonly max: number; readonly initial: number };

export const DEFAULT_SIDEBAR_RANGE: SidebarWidthRange = {
  min: MIN_SIDEBAR_WIDTH,
  max: MAX_SIDEBAR_WIDTH,
  initial: DEFAULT_SIDEBAR_WIDTH,
};

/** The sidebar that carries the Supervisor panel: 380px, resizable between 320 and 480. */
export const SUPERVISOR_SIDEBAR_RANGE: SidebarWidthRange = { min: 320, max: 480, initial: 380 };

export type WorktreeOrder = Record<string, string[]>;

const persistenceListeners = new Set<() => void>();

/** Fires after the sidebar persists its worktree order or collapsed set in this window. */
export function subscribeSidebarPersistence(listener: () => void): () => void {
  persistenceListeners.add(listener);
  return () => { persistenceListeners.delete(listener); };
}

function notifySidebarPersistence() {
  for (const listener of persistenceListeners) listener();
}

export function clampSidebarWidth(width: number, range: SidebarWidthRange = DEFAULT_SIDEBAR_RANGE) {
  return Math.min(range.max, Math.max(range.min, Math.round(width)));
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
  notifySidebarPersistence();
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
  notifySidebarPersistence();
}

export function loadSidebarWidth(range: SidebarWidthRange = DEFAULT_SIDEBAR_RANGE) {
  try {
    const raw = getMigratedItem(SIDEBAR_WIDTH_STORAGE_KEY);
    const stored = Number(raw);
    return Number.isFinite(stored) && stored > 0 ? clampSidebarWidth(stored, range) : range.initial;
  } catch {
    return range.initial;
  }
}

export function persistSidebarWidth(width: number, range: SidebarWidthRange = DEFAULT_SIDEBAR_RANGE) {
  try {
    window.localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(clampSidebarWidth(width, range)));
  } catch {
    // A storage failure should not break pointer resizing for this session.
  }
}
