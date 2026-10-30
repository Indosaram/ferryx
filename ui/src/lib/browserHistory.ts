import { isHttpUrl, loadBrowserSettings } from "./browserSettings";
import { BROWSER_HISTORY_STORAGE_KEY, BROWSER_SCROLL_STORAGE_KEY } from "./storageKeys";

export { BROWSER_SCROLL_STORAGE_KEY };

export const BROWSER_HISTORY_EVENT = "ferryx:browser-history";
export const BROWSER_HISTORY_LIMIT = 100;
export const BROWSER_SCROLL_LIMIT = 200;

export type BrowserScrollPosition = {
  x: number;
  y: number;
};

export type BrowserHistoryEntry = {
  id: string;
  browserId: string;
  url: string;
  title: string | null;
  visitedAt: number;
  scrollPosition?: BrowserScrollPosition;
  scrollX?: number;
  scrollY?: number;
};

export function isSupportedHistoryUrl(url: string): boolean {
  const trimmed = url.trim();
  return isHttpUrl(trimmed) || /^file:\/\//i.test(trimmed);
}

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
  if (!id || !browserId || !isSupportedHistoryUrl(url) || visitedAt <= 0) return null;
  const rawScrollX = typeof source.scrollX === "number" && Number.isFinite(source.scrollX)
    ? Math.max(0, Math.round(source.scrollX))
    : undefined;
  const rawScrollY = typeof source.scrollY === "number" && Number.isFinite(source.scrollY)
    ? Math.max(0, Math.round(source.scrollY))
    : undefined;
  const scrollPosition = source.scrollPosition && typeof source.scrollPosition.x === "number" && typeof source.scrollPosition.y === "number"
    ? {
        x: Math.max(0, Math.round(Number.isFinite(source.scrollPosition.x) ? source.scrollPosition.x : 0)),
        y: Math.max(0, Math.round(Number.isFinite(source.scrollPosition.y) ? source.scrollPosition.y : 0)),
      }
    : rawScrollX !== undefined || rawScrollY !== undefined
      ? { x: rawScrollX ?? 0, y: rawScrollY ?? 0 }
      : undefined;

  return {
    id,
    browserId,
    url,
    title,
    visitedAt,
    ...(scrollPosition ? { scrollPosition, scrollX: scrollPosition.x, scrollY: scrollPosition.y } : {}),
  };
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

export function clearBrowserHistory(storage: Storage | null = historyStorage()): void {
  try {
    storage?.removeItem(BROWSER_HISTORY_STORAGE_KEY);
  } catch {
    // Ignore disabled storage failures.
  }
  // Scroll positions are keyed by visited URL, so they are browsing history too.
  clearBrowserScroll(storage);
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<BrowserHistoryEntry[]>(BROWSER_HISTORY_EVENT, { detail: [] }));
  }
}

export function recordBrowserScroll(
  browserId: string,
  url: string,
  position: BrowserScrollPosition,
  storage: Storage | null = historyStorage(),
): void {
  if (!browserId || !url || !isSupportedHistoryUrl(url)) return;
  const cleanPos: BrowserScrollPosition = {
    x: Math.max(0, Math.round(Number.isFinite(position.x) ? position.x : 0)),
    y: Math.max(0, Math.round(Number.isFinite(position.y) ? position.y : 0)),
  };
  try {
    const raw = storage?.getItem(BROWSER_SCROLL_STORAGE_KEY);
    const map: Record<string, BrowserScrollPosition> = raw ? JSON.parse(raw) : {};
    // Delete-then-set moves the written keys to the end of insertion order, so the oldest
    // keys come first and eviction is least-recently-written.
    for (const key of [`${browserId}:${url}`, url]) {
      delete map[key];
      map[key] = cleanPos;
    }
    const keys = Object.keys(map);
    let evict = 0;
    while (keys.length - evict > BROWSER_SCROLL_LIMIT) {
      delete map[keys[evict]];
      evict += 1;
    }
    storage?.setItem(BROWSER_SCROLL_STORAGE_KEY, JSON.stringify(map));
  } catch {
    // Ignore disabled storage and quota failures.
  }
}

export function getBrowserScroll(
  browserId: string,
  url: string,
  storage: Storage | null = historyStorage(),
): BrowserScrollPosition | null {
  if (!url) return null;
  try {
    const raw = storage?.getItem(BROWSER_SCROLL_STORAGE_KEY);
    if (!raw) return null;
    const map: Record<string, BrowserScrollPosition> = JSON.parse(raw);
    const found = map[`${browserId}:${url}`] ?? map[url];
    if (found && typeof found.x === "number" && typeof found.y === "number") {
      return {
        x: Math.max(0, Math.round(Number.isFinite(found.x) ? found.x : 0)),
        y: Math.max(0, Math.round(Number.isFinite(found.y) ? found.y : 0)),
      };
    }
  } catch {
    // Ignore disabled storage or parse failures.
  }
  return null;
}

export function clearBrowserScroll(storage: Storage | null = historyStorage()): void {
  try {
    storage?.removeItem(BROWSER_SCROLL_STORAGE_KEY);
  } catch {
    // Ignore disabled storage failures.
  }
}

export function recordBrowserHistory(
  entry: Omit<BrowserHistoryEntry, "id" | "visitedAt"> & { visitedAt?: number },
  storage: Storage | null = historyStorage(),
): BrowserHistoryEntry[] {
  if (!loadBrowserSettings(storage).rememberBrowsingHistory || !isSupportedHistoryUrl(entry.url)) {
    return loadBrowserHistory(storage);
  }

  const visitedAt = entry.visitedAt ?? Date.now();
  const rawScrollX = typeof entry.scrollX === "number" && Number.isFinite(entry.scrollX)
    ? Math.max(0, Math.round(entry.scrollX))
    : undefined;
  const rawScrollY = typeof entry.scrollY === "number" && Number.isFinite(entry.scrollY)
    ? Math.max(0, Math.round(entry.scrollY))
    : undefined;
  const scrollPosition = entry.scrollPosition && typeof entry.scrollPosition.x === "number" && typeof entry.scrollPosition.y === "number"
    ? {
        x: Math.max(0, Math.round(Number.isFinite(entry.scrollPosition.x) ? entry.scrollPosition.x : 0)),
        y: Math.max(0, Math.round(Number.isFinite(entry.scrollPosition.y) ? entry.scrollPosition.y : 0)),
      }
    : rawScrollX !== undefined || rawScrollY !== undefined
      ? { x: rawScrollX ?? 0, y: rawScrollY ?? 0 }
      : getBrowserScroll(entry.browserId, entry.url, storage) ?? undefined;

  if (scrollPosition) {
    recordBrowserScroll(entry.browserId, entry.url, scrollPosition, storage);
  }

  const nextEntry: BrowserHistoryEntry = {
    id: `${entry.browserId}:${visitedAt}:${entry.url}`,
    browserId: entry.browserId,
    url: entry.url,
    title: entry.title?.trim() || null,
    visitedAt,
    ...(scrollPosition ? { scrollPosition, scrollX: scrollPosition.x, scrollY: scrollPosition.y } : {}),
  };
  const previous = loadBrowserHistory(storage).filter(
    (candidate) => !(candidate.browserId === nextEntry.browserId && candidate.url === nextEntry.url),
  );
  const next = [nextEntry, ...previous].slice(0, BROWSER_HISTORY_LIMIT);
  writeBrowserHistory(next, storage);
  return next;
}

/**
 * Computes a fuzzy relevance score for matching an omnibox query against a candidate string.
 * Higher scores reflect closer matches:
 * - 100: exact match
 * - 80: prefix match
 * - 60: word/path boundary prefix match (e.g. "x.html" matching "/tmp/x.html")
 * - 40: substring match
 * - 20: fuzzy subsequence match (characters appear in order)
 * - 0: no match
 */
export function fuzzyMatchScore(query: string, candidate: string): number {
  const q = query.trim().toLowerCase();
  const c = candidate.toLowerCase();
  if (!q) return 1;
  if (!c) return 0;
  if (c === q) return 100;
  if (c.startsWith(q)) return 80;

  const segments = c.split(/[/.\-_ \t]+/);
  for (const seg of segments) {
    if (seg.startsWith(q)) return 60;
  }

  if (c.includes(q)) return 40;

  let qIdx = 0;
  for (let cIdx = 0; cIdx < c.length && qIdx < q.length; cIdx++) {
    if (c[cIdx] === q[qIdx]) qIdx++;
  }
  if (qIdx === q.length) return 20;

  return 0;
}

/**
 * Filters and ranks history entries using fuzzy matching against title, full URL,
 * and compact URL (scheme stripped). Ties are broken by visit recency.
 */
export function searchBrowserHistory(
  entries: BrowserHistoryEntry[],
  query: string,
  limit = 8,
): BrowserHistoryEntry[] {
  const q = query.trim().toLowerCase();
  if (!q) {
    return entries.slice(0, limit);
  }

  const scored: { entry: BrowserHistoryEntry; score: number }[] = [];

  for (const entry of entries) {
    const url = entry.url;
    const compactUrl = url.replace(/^https?:\/\//i, "").replace(/^file:\/\/\/?/i, "");
    const title = entry.title ?? "";

    const urlScore = fuzzyMatchScore(q, url);
    const compactScore = fuzzyMatchScore(q, compactUrl);
    const titleScore = title ? fuzzyMatchScore(q, title) : 0;

    const maxScore = Math.max(urlScore, compactScore, titleScore);
    if (maxScore > 0) {
      scored.push({ entry, score: maxScore });
    }
  }

  scored.sort((a, b) => {
    if (b.score !== a.score) return b.score - a.score;
    return b.entry.visitedAt - a.entry.visitedAt;
  });

  return scored.slice(0, limit).map((s) => s.entry);
}
