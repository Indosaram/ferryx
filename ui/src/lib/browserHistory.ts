import { isHttpUrl, loadBrowserSettings } from "./browserSettings";
import { BROWSER_HISTORY_STORAGE_KEY } from "./storageKeys";

export const BROWSER_HISTORY_EVENT = "ferryx:browser-history";
export const BROWSER_HISTORY_LIMIT = 100;

export type BrowserHistoryEntry = {
  id: string;
  browserId: string;
  url: string;
  title: string | null;
  visitedAt: number;
};

function historyStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function normalizeEntry(value: unknown): BrowserHistoryEntry | null {
  if (!value || typeof value !== "object") return null;
  const source = value as Partial<BrowserHistoryEntry>;
  const id = typeof source.id === "string" ? source.id : "";
  const browserId = typeof source.browserId === "string" ? source.browserId : "";
  const url = typeof source.url === "string" ? source.url : "";
  const title = typeof source.title === "string" && source.title.trim() ? source.title.trim() : null;
  const visitedAt = typeof source.visitedAt === "number" && Number.isFinite(source.visitedAt)
    ? source.visitedAt
    : 0;
  if (!id || !browserId || !isHttpUrl(url) || visitedAt <= 0) return null;
  return { id, browserId, url, title, visitedAt };
}

export function loadBrowserHistory(storage: Storage | null = historyStorage()): BrowserHistoryEntry[] {
  if (!storage) return [];
  try {
    const raw = storage.getItem(BROWSER_HISTORY_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .map(normalizeEntry)
      .filter((entry): entry is BrowserHistoryEntry => entry !== null)
      .sort((left, right) => right.visitedAt - left.visitedAt)
      .slice(0, BROWSER_HISTORY_LIMIT);
  } catch {
    return [];
  }
}

function writeBrowserHistory(entries: BrowserHistoryEntry[], storage: Storage | null): void {
  try {
    storage?.setItem(BROWSER_HISTORY_STORAGE_KEY, JSON.stringify(entries.slice(0, BROWSER_HISTORY_LIMIT)));
  } catch {
    // Ignore disabled storage and quota failures.
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<BrowserHistoryEntry[]>(BROWSER_HISTORY_EVENT, { detail: entries }));
  }
}

export function recordBrowserHistory(
  entry: Omit<BrowserHistoryEntry, "id" | "visitedAt"> & { visitedAt?: number },
  storage: Storage | null = historyStorage(),
): BrowserHistoryEntry[] {
  if (!loadBrowserSettings(storage).rememberBrowsingHistory || !isHttpUrl(entry.url)) {
    return loadBrowserHistory(storage);
  }

  const visitedAt = entry.visitedAt ?? Date.now();
  const nextEntry: BrowserHistoryEntry = {
    id: `${entry.browserId}:${visitedAt}:${entry.url}`,
    browserId: entry.browserId,
    url: entry.url,
    title: entry.title?.trim() || null,
    visitedAt,
  };
  const previous = loadBrowserHistory(storage).filter(
    (candidate) => !(candidate.browserId === nextEntry.browserId && candidate.url === nextEntry.url),
  );
  const next = [nextEntry, ...previous].slice(0, BROWSER_HISTORY_LIMIT);
  writeBrowserHistory(next, storage);
  return next;
}

export function clearBrowserHistory(storage: Storage | null = historyStorage()): void {
  try {
    storage?.removeItem(BROWSER_HISTORY_STORAGE_KEY);
  } catch {
    // Ignore disabled storage failures.
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<BrowserHistoryEntry[]>(BROWSER_HISTORY_EVENT, { detail: [] }));
  }
}
