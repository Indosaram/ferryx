// Shared tab transitions for the Maho browser sidebar.
//
// Favorites, Pinned, Folders and Today all edit the same `browserStore`, so every
// move lives here exactly once: a click handler and a drag-and-drop hand-off call
// the same function, and the only place that knows how a full Favorites grid
// reports itself is `addTabToFavorites`. Components stay presentational.

import { toast } from "sonner";
import { closeBrowser, createBrowser } from "../../../lib/browserTauri";

import {
  selectFavoriteTabs,
  selectTabsInSection,
  type BrowserSection,
  type BrowserStore,
  type BrowserWorktreeTag,
} from "../../../state/browserStore";

const sessionTransitions = new Map<string, Promise<void>>();

export function suspendMahoTab(store: BrowserStore, browserId: string): Promise<void> {
  const existing = sessionTransitions.get(browserId);
  if (existing) return existing;
  const tab = store.getState().tabs[browserId];
  if (!tab || tab.suspended) return Promise.resolve();
  // Clear active ownership before closing so the canvas releases its native surface.
  store.setSuspended(browserId, true);
  const task = closeBrowser(browserId).catch((error: unknown) => {
    store.setSuspended(browserId, false);
    throw error;
  }).finally(() => sessionTransitions.delete(browserId));
  sessionTransitions.set(browserId, task);
  return task;
}

export async function activateMahoTab(store: BrowserStore, browserId: string, onActivate?: (id: string) => void): Promise<void> {
  const pending = sessionTransitions.get(browserId);
  if (pending) await pending;
  const tab = store.getState().tabs[browserId];
  if (!tab) return;
  if (tab.suspended) {
    let task = sessionTransitions.get(browserId);
    if (!task) {
      task = createBrowser({ browserId, url: tab.url, profile: tab.profileId, visible: false })
        .then(() => { store.setSuspended(browserId, false); })
        .finally(() => sessionTransitions.delete(browserId));
      sessionTransitions.set(browserId, task);
    }
    await task;
  }
  if (onActivate) onActivate(browserId);
  else { store.touch(browserId); store.setActiveSpace(tab.spaceId); }
}

/** Maho's 4x3 grid holds twelve favorites; the thirteenth add is refused with this toast. */
export const FAVORITE_LIMIT_MESSAGE = "Favorites are full (12 max). Unpin a tile to make room.";

/** Moves a tab into the shared Favorites grid, toasting when the grid is full. */
export function addTabToFavorites(store: BrowserStore, browserId: string): boolean {
  const result = store.moveTab(browserId, { section: "favorite", folderId: null });
  if (result.ok) return true;
  if (result.error === "favorite-limit") toast.error(FAVORITE_LIMIT_MESSAGE);
  return false;
}

/** Pins a tab, optionally straight into a folder. */
export function moveTabToPinned(store: BrowserStore, browserId: string, folderId: string | null = null): boolean {
  return store.moveTab(browserId, { section: "pinned", folderId }).ok;
}

/** Sends a pinned/favorite tab back to Today. */
export function moveTabToToday(store: BrowserStore, browserId: string): boolean {
  return store.moveTab(browserId, { section: "today", folderId: null }).ok;
}

/**
 * Clears Today for one Space and offers an Undo that restores exactly the tabs this
 * call archived (archive entries are prepended, so the newest `count` are ours).
 */
export function clearTodayWithUndo(store: BrowserStore, spaceId: string): number {
  const count = store.clearToday(spaceId);
  if (count === 0) return 0;
  const cleared = store.getState().archive.slice(0, count).map((entry) => entry.tab.browserId);
  toast.success(`Cleared ${count} ${count === 1 ? "tab" : "tabs"} from Today`, {
    action: {
      label: "Undo",
      onClick: () => {
        for (const browserId of cleared) store.restoreFromArchive(browserId);
      },
    },
  });
  return count;
}

/** What the URL pill shows: the bare host, Maho-style, never the full URL. */
export function displayHost(url: string): string {
  if (!url || url === "about:blank") return "New Tab";
  try {
    const parsed = new URL(url);
    if (parsed.protocol === "file:") {
      const name = parsed.pathname.split("/").filter(Boolean).pop();
      return name ? decodeURIComponent(name) : "File";
    }
    const host = parsed.hostname.replace(/^www\./, "");
    return host || parsed.protocol.replace(/:$/, "");
  } catch {
    return url;
  }
}

/** Host plus path, for the tile tooltip and the pill's title attribute. */
export function displayUrl(url: string): string {
  if (!url || url === "about:blank") return "about:blank";
  try {
    const parsed = new URL(url);
    const path = parsed.pathname === "/" ? "" : parsed.pathname.replace(/\/$/, "");
    return `${parsed.host}${path}${parsed.search}`;
  } catch {
    return url;
  }
}

/** Short badge text for a tab attached to a worktree. */
export function worktreeTagLabel(tag: BrowserWorktreeTag): string {
  if (tag.label) return tag.label;
  const parts = tag.worktreePath.split(/[\\/]/).filter(Boolean);
  return parts.pop() ?? tag.worktreePath;
}

/** What a draggable/droppable carries through dnd-kit. */
export type MahoDropTarget =
  | { kind: "tab"; tabId: string }
  | { kind: "section"; section: BrowserSection; folderId?: string | null };

const TAB_DRAG_PREFIX = "maho-tab:";
const SECTION_DRAG_PREFIX = "maho-section:";
const FOLDER_DRAG_PREFIX = "maho-folder:";

/** A stable dnd-kit id for a tab, a section body, or a folder row. */
export function mahoDragId(target: MahoDropTarget): string {
  if (target.kind === "tab") return `${TAB_DRAG_PREFIX}${target.tabId}`;
  if (target.folderId) return `${FOLDER_DRAG_PREFIX}${target.folderId}`;
  return `${SECTION_DRAG_PREFIX}${target.section}`;
}

/** Inverse of `mahoDragId`; an unknown id is ignored instead of moving a tab at random. */
export function parseMahoDragId(id: string): MahoDropTarget | null {
  if (id.startsWith(TAB_DRAG_PREFIX)) return { kind: "tab", tabId: id.slice(TAB_DRAG_PREFIX.length) };
  if (id.startsWith(FOLDER_DRAG_PREFIX)) {
    return { kind: "section", section: "pinned", folderId: id.slice(FOLDER_DRAG_PREFIX.length) };
  }
  if (id.startsWith(SECTION_DRAG_PREFIX)) {
    const section = id.slice(SECTION_DRAG_PREFIX.length);
    if (section === "favorite" || section === "pinned" || section === "today") {
      return { kind: "section", section };
    }
  }
  return null;
}

/**
 * Resolves one drop: a drag onto a section body moves the tab to that section (or
 * folder); a drag onto another tab seats the dragged tab at that tab's slot, which
 * is exactly `arrayMove` semantics because `moveTab` removes the tab before seating.
 */
export function applyMahoDrop(store: BrowserStore, active: MahoDropTarget, over: MahoDropTarget): boolean {
  if (active.kind !== "tab") return false;
  const state = store.getState();
  if (!state.tabs[active.tabId]) return false;

  if (over.kind === "section") {
    if (over.section === "favorite") return addTabToFavorites(store, active.tabId);
    const folderId = over.section === "pinned" ? over.folderId ?? null : null;
    return store.moveTab(active.tabId, { section: over.section, folderId }).ok;
  }

  if (over.tabId === active.tabId) return false;
  const overTab = state.tabs[over.tabId];
  if (!overTab) return false;
  const group = overTab.section === "favorite"
    ? selectFavoriteTabs(state)
    : selectTabsInSection(
      state,
      overTab.spaceId,
      overTab.section,
      overTab.section === "pinned" ? overTab.folderId : undefined,
    );
  const index = group.findIndex((tab) => tab.browserId === over.tabId);
  if (index < 0) return false;

  const result = store.moveTab(active.tabId, {
    section: overTab.section,
    folderId: overTab.section === "pinned" ? overTab.folderId ?? null : null,
    index,
  });
  if (result.ok) return true;
  if (result.error === "favorite-limit") toast.error(FAVORITE_LIMIT_MESSAGE);
  return false;
}
