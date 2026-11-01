import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { clearBrowserHistory, fuzzyMatchScore, isSupportedHistoryUrl, loadBrowserHistory } from "./browserHistory";
import { BROWSER_HISTORY_STORAGE_KEY } from "./storageKeys";

/** Host-side bookmarks and history (src-tauri/src/ipc/browser_library.rs). */

export const BOOKMARK_BAR_ROOT = "bar";
export const BOOKMARK_OTHER_ROOT = "other";
export const BROWSER_LIBRARY_CHANGED_EVENT = "browser_library_changed";
/** DOM event a pane listens to for opening its Library (bookmarks/history manager). */
export const BROWSER_LIBRARY_OPEN_EVENT = "ferryx:browser-library-open";

export type BookmarkFolder = { id: string; name: string; parentId: string; position: number; createdAt: number };
export type Bookmark = { id: string; title: string; url: string; parentId: string; position: number; createdAt: number };
export type BookmarkTree = { version: number; folders: BookmarkFolder[]; bookmarks: Bookmark[] };
export type BookmarksView = { tree: BookmarkTree; recovery: "recoveredFromBackup" | "resetCorrupt" | null };
export type HostHistoryEntry = {
  url: string;
  title: string | null;
  visitCount: number;
  lastVisitedAt: number;
  frecency: number;
};
export type BrowserLibraryOpenDetail = { browserId: string; view: "bookmarks" | "history" };

export const EMPTY_BOOKMARK_TREE: BookmarkTree = { version: 1, folders: [], bookmarks: [] };

export function getBookmarks(): Promise<BookmarksView> {
  return invoke<BookmarksView>("cmd_browser_bookmarks_get");
}
export function addBookmark(title: string, url: string, parentId: string = BOOKMARK_BAR_ROOT): Promise<Bookmark> {
  return invoke<Bookmark>("cmd_browser_bookmark_add", { title, url, parentId });
}
export function createBookmarkFolder(name: string, parentId: string = BOOKMARK_BAR_ROOT): Promise<BookmarkFolder> {
  return invoke<BookmarkFolder>("cmd_browser_bookmark_folder_create", { name, parentId });
}
export function updateBookmark(id: string, changes: { title?: string; url?: string }): Promise<BookmarksView> {
  return invoke<BookmarksView>("cmd_browser_bookmark_update", { id, title: changes.title ?? null, url: changes.url ?? null });
}
export function moveBookmark(id: string, parentId: string, index?: number): Promise<BookmarksView> {
  return invoke<BookmarksView>("cmd_browser_bookmark_move", { id, parentId, index: index ?? null });
}
export function deleteBookmark(id: string): Promise<BookmarksView> {
  return invoke<BookmarksView>("cmd_browser_bookmark_delete", { id });
}
export function listHostHistory(query = "", before?: number, limit = 200): Promise<HostHistoryEntry[]> {
  return invoke<HostHistoryEntry[]>("cmd_browser_history_list", { query, before: before ?? null, limit });
}
export function suggestHostHistory(query: string, limit = 8): Promise<HostHistoryEntry[]> {
  return invoke<HostHistoryEntry[]>("cmd_browser_history_suggest", { query, limit });
}
export function deleteHostHistoryUrl(url: string): Promise<boolean> {
  return invoke<boolean>("cmd_browser_history_delete", { url });
}
/** Clears visits in [from, to); no bounds clears everything. */
export function clearHostHistory(from?: number, to?: number): Promise<void> {
  return invoke<void>("cmd_browser_history_clear", { from: from ?? null, to: to ?? null });
}
export function setHostHistoryEnabled(enabled: boolean): Promise<void> {
  return invoke<void>("cmd_browser_history_set_enabled", { enabled });
}
export function fetchSearchSuggestions(engine: string, query: string): Promise<string[]> {
  return invoke<string[]>("cmd_browser_search_suggest", { engine, query });
}

export function onBrowserLibraryChanged(
  listener: (kind: "bookmarks" | "history") => void,
): Promise<UnlistenFn> {
  return listen<{ kind: "bookmarks" | "history" }>(BROWSER_LIBRARY_CHANGED_EVENT, (event) => listener(event.payload.kind));
}

export function openBrowserLibrary(detail: BrowserLibraryOpenDetail): void {
  window.dispatchEvent(new CustomEvent<BrowserLibraryOpenDetail>(BROWSER_LIBRARY_OPEN_EVENT, { detail }));
}

/** Clears every trace of history: the host store and the legacy local copy with scroll positions. */
export async function clearAllBrowserHistory(): Promise<void> {
  clearBrowserHistory();
  await clearHostHistory();
}

let migration: Promise<number> | null = null;

/**
 * Moves the old localStorage history (cap 100) into the host store once. The host marks the
 * migration done, so another window or a later launch imports nothing; the local copy is
 * removed only after the host accepted it.
 */
export function migrateLegacyBrowserHistory(storage: Storage | null = safeLocalStorage()): Promise<number> {
  if (migration) return migration;
  const legacy = loadBrowserHistory(storage);
  const hasLegacyKey = Boolean(storage?.getItem(BROWSER_HISTORY_STORAGE_KEY));
  if (!hasLegacyKey) return Promise.resolve(0);
  migration = invoke<{ imported: number; alreadyMigrated: boolean }>("cmd_browser_history_migrate_legacy", {
    visits: legacy.map((entry) => ({ url: entry.url, title: entry.title, visitedAt: entry.visitedAt, visitCount: 1 })),
  }).then((result) => {
    storage?.removeItem(BROWSER_HISTORY_STORAGE_KEY);
    return result.imported;
  }).catch((error: unknown) => {
    migration = null;
    throw error;
  });
  return migration;
}

function safeLocalStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** Live bookmark tree; refreshes on `browser_library_changed`. */
export function useBookmarks(): { tree: BookmarkTree; error: string | null; refresh: () => void } {
  const [tree, setTree] = useState<BookmarkTree>(EMPTY_BOOKMARK_TREE);
  const [error, setError] = useState<string | null>(null);
  const refresh = useCallback(() => {
    void Promise.resolve()
      .then(() => getBookmarks())
      .then((view) => {
        setTree(view?.tree ?? EMPTY_BOOKMARK_TREE);
        setError(view?.recovery ?? null);
      })
      .catch((err: unknown) => setError(errorCode(err)));
  }, []);
  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    refresh();
    void Promise.resolve()
      .then(() => onBrowserLibraryChanged((kind) => {
        if (kind === "bookmarks") refresh();
      }))
      .then((cleanup) => {
        if (disposed) cleanup?.();
        else unlisten = cleanup;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refresh]);
  return { tree, error, refresh };
}

export function errorCode(err: unknown): string {
  if (err && typeof err === "object" && typeof (err as { code?: unknown }).code === "string") {
    return (err as { code: string }).code;
  }
  return "UNKNOWN_ERROR";
}

export function bookmarkChildren(tree: BookmarkTree, parentId: string): Array<
  { kind: "folder"; item: BookmarkFolder } | { kind: "bookmark"; item: Bookmark }
> {
  return [
    ...tree.folders.filter((f) => f.parentId === parentId).map((item) => ({ kind: "folder" as const, item })),
    ...tree.bookmarks.filter((b) => b.parentId === parentId).map((item) => ({ kind: "bookmark" as const, item })),
  ].sort((a, b) => a.item.position - b.item.position);
}

/** The comparison key for "same page": lowercase scheme/host, no fragment, no trailing slash. */
export function normalizeSuggestionUrl(raw: string): string {
  try {
    const url = new URL(raw.trim());
    url.hash = "";
    const text = url.href;
    return text.endsWith("/") ? text.slice(0, -1) : text;
  } catch {
    return raw.trim().toLowerCase().replace(/#.*$/, "").replace(/\/$/, "");
  }
}

export function findBookmarkByUrl(tree: BookmarkTree, url: string): Bookmark | null {
  const key = normalizeSuggestionUrl(url);
  return tree.bookmarks.find((b) => normalizeSuggestionUrl(b.url) === key) ?? null;
}

export type OmniboxSuggestion = {
  id: string;
  kind: "history" | "bookmark" | "tab" | "search";
  url: string;
  title: string | null;
  /** Open-tab suggestions switch to that tab instead of navigating. */
  browserId?: string;
  /** For search suggestions: the query text. */
  query?: string;
  bookmarked: boolean;
  score: number;
};

export type OmniboxSources = {
  history: HostHistoryEntry[];
  bookmarks: Bookmark[];
  tabs: Array<{ browserId: string; url: string; title: string | null }>;
  searchSuggestions: string[];
  currentBrowserId?: string;
};

function matchScore(query: string, url: string, title: string | null): number {
  if (!query.trim()) return 1;
  const compact = url.replace(/^https?:\/\/(www\.)?/i, "").replace(/^file:\/\/\/?/i, "");
  return Math.max(
    fuzzyMatchScore(query, url),
    fuzzyMatchScore(query, compact),
    title ? fuzzyMatchScore(query, title) : 0,
  );
}

/**
 * Merges the omnibox sources into one ranked list with one row per page (normalized URL). Match
 * quality decides first; then bookmarks, open tabs and frecency add weight. Search-engine
 * suggestions trail the local results.
 */
export function rankOmniboxSuggestions(query: string, sources: OmniboxSources, limit = 8): OmniboxSuggestion[] {
  const byUrl = new Map<string, OmniboxSuggestion>();
  const bookmarkedUrls = new Set(sources.bookmarks.map((b) => normalizeSuggestionUrl(b.url)));
  const put = (candidate: Omit<OmniboxSuggestion, "id" | "bookmarked">) => {
    if (!isSupportedHistoryUrl(candidate.url)) return;
    const key = normalizeSuggestionUrl(candidate.url);
    const bookmarked = bookmarkedUrls.has(key);
    const existing = byUrl.get(key);
    if (!existing) {
      byUrl.set(key, { ...candidate, id: `${candidate.kind}:${key}`, bookmarked });
      return;
    }
    // Keep one row; an open tab wins the action (switch), and the best title and score stay.
    const kind = existing.kind === "tab" || candidate.kind !== "tab" ? existing.kind : "tab";
    byUrl.set(key, {
      ...existing,
      kind,
      browserId: kind === "tab" ? existing.browserId ?? candidate.browserId : undefined,
      id: `${kind}:${key}`,
      title: existing.title || candidate.title,
      score: Math.max(existing.score, candidate.score) + 5,
    });
  };
  for (const entry of sources.history) {
    const match = matchScore(query, entry.url, entry.title);
    if (match <= 0) continue;
    put({ kind: "history", url: entry.url, title: entry.title, score: match * 10 + Math.log10(1 + Math.max(0, entry.frecency)) * 10 });
  }
  for (const mark of sources.bookmarks) {
    const match = matchScore(query, mark.url, mark.title);
    if (match <= 0 || !query.trim()) continue;
    put({ kind: "bookmark", url: mark.url, title: mark.title, score: match * 10 + 25 });
  }
  for (const tab of sources.tabs) {
    if (tab.browserId === sources.currentBrowserId) continue;
    const match = matchScore(query, tab.url, tab.title);
    if (match <= 0 || !query.trim()) continue;
    put({ kind: "tab", browserId: tab.browserId, url: tab.url, title: tab.title, score: match * 10 + 15 });
  }
  const local = [...byUrl.values()].sort((a, b) => b.score - a.score);
  const remote: OmniboxSuggestion[] = sources.searchSuggestions
    .filter((text) => text.trim() && text.trim().toLowerCase() !== query.trim().toLowerCase())
    .slice(0, 4)
    .map((text, index) => ({
      id: `search:${text}`,
      kind: "search",
      url: "",
      title: text,
      query: text,
      bookmarked: false,
      score: -index,
    }));
  const localSlots = Math.max(0, limit - Math.min(remote.length, query.trim() ? 3 : 0));
  return [...local.slice(0, localSlots), ...remote].slice(0, limit);
}
