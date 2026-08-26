import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  BROWSER_HISTORY_EVENT,
  BROWSER_HISTORY_LIMIT,
  clearBrowserHistory,
  loadBrowserHistory,
  recordBrowserHistory,
  type BrowserHistoryEntry,
} from "./browserHistory";
import { BROWSER_HISTORY_STORAGE_KEY, BROWSER_SETTINGS_STORAGE_KEY } from "./storageKeys";

function memoryStorage(seed: Record<string, string> = {}): Storage {
  const map = new Map(Object.entries(seed));
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (key: string) => map.get(key) ?? null,
    key: (index: number) => [...map.keys()][index] ?? null,
    removeItem: (key: string) => void map.delete(key),
    setItem: (key: string, value: string) => void map.set(key, value),
  } as Storage;
}

function entry(overrides: Partial<BrowserHistoryEntry> = {}): BrowserHistoryEntry {
  return {
    id: "b1:1000:https://example.com/",
    browserId: "b1",
    url: "https://example.com/",
    title: "Example",
    visitedAt: 1000,
    ...overrides,
  };
}

function storageWith(entries: unknown): Storage {
  return memoryStorage({ [BROWSER_HISTORY_STORAGE_KEY]: JSON.stringify(entries) });
}

describe("loadBrowserHistory", () => {
  it("returns nothing when storage is unavailable", () => {
    expect(loadBrowserHistory(null)).toEqual([]);
  });

  it("returns nothing when no history has been written", () => {
    expect(loadBrowserHistory(memoryStorage())).toEqual([]);
  });

  it("returns nothing when the stored payload is malformed JSON", () => {
    expect(loadBrowserHistory(memoryStorage({ [BROWSER_HISTORY_STORAGE_KEY]: "{oops" }))).toEqual(
      [],
    );
  });

  it("returns nothing when the stored payload is not an array", () => {
    expect(loadBrowserHistory(storageWith({ id: "b1" }))).toEqual([]);
  });

  it("orders entries by most recent visit", () => {
    const loaded = loadBrowserHistory(
      storageWith([
        entry({ id: "old", visitedAt: 10 }),
        entry({ id: "new", visitedAt: 900 }),
        entry({ id: "mid", visitedAt: 500 }),
      ]),
    );
    expect(loaded.map((item) => item.id)).toEqual(["new", "mid", "old"]);
  });

  it("drops entries whose required fields are missing or non-http", () => {
    const loaded = loadBrowserHistory(
      storageWith([
        entry({ id: "keep" }),
        entry({ id: "", browserId: "b1" }),
        entry({ id: "no-browser", browserId: "" }),
        entry({ id: "ftp", url: "ftp://example.com/file" }),
        entry({ id: "unvisited", visitedAt: 0 }),
        "not-an-object",
        null,
      ]),
    );
    expect(loaded.map((item) => item.id)).toEqual(["keep"]);
  });

  it("normalizes a blank title to null and trims a padded one", () => {
    const loaded = loadBrowserHistory(
      storageWith([
        entry({ id: "blank", title: "   ", visitedAt: 20 }),
        entry({ id: "padded", title: "  Docs  ", visitedAt: 10 }),
      ]),
    );
    expect(loaded.map((item) => item.title)).toEqual([null, "Docs"]);
  });

  it("caps the returned history at the documented limit", () => {
    const overflowing = Array.from({ length: BROWSER_HISTORY_LIMIT + 25 }, (_unused, index) =>
      entry({ id: `e${index}`, url: `https://example.com/${index}`, visitedAt: index + 1 }),
    );
    expect(loadBrowserHistory(storageWith(overflowing))).toHaveLength(BROWSER_HISTORY_LIMIT);
  });
});

describe("recordBrowserHistory", () => {
  beforeEach(() => {
    vi.useRealTimers();
  });

  it("stores a visit and announces it on the history event", () => {
    const storage = memoryStorage();
    const listener = vi.fn();
    window.addEventListener(BROWSER_HISTORY_EVENT, listener);

    const next = recordBrowserHistory(
      { browserId: "b1", url: "https://example.com/docs", title: "Docs", visitedAt: 4242 },
      storage,
    );

    window.removeEventListener(BROWSER_HISTORY_EVENT, listener);

    expect(next).toEqual([
      {
        id: "b1:4242:https://example.com/docs",
        browserId: "b1",
        url: "https://example.com/docs",
        title: "Docs",
        visitedAt: 4242,
      },
    ]);
    expect(loadBrowserHistory(storage)).toEqual(next);
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("stamps the visit time from the clock when the caller omits it", () => {
    const storage = memoryStorage();
    vi.useFakeTimers();
    vi.setSystemTime(new Date(1_700_000_000_000));

    const [recorded] = recordBrowserHistory(
      { browserId: "b1", url: "https://example.com/", title: null },
      storage,
    );

    vi.useRealTimers();
    expect(recorded.visitedAt).toBe(1_700_000_000_000);
  });

  it("moves a repeat visit of the same url to the front instead of duplicating it", () => {
    const storage = memoryStorage();
    recordBrowserHistory(
      { browserId: "b1", url: "https://example.com/a", title: "A", visitedAt: 100 },
      storage,
    );
    recordBrowserHistory(
      { browserId: "b1", url: "https://example.com/b", title: "B", visitedAt: 200 },
      storage,
    );
    const next = recordBrowserHistory(
      { browserId: "b1", url: "https://example.com/a", title: "A again", visitedAt: 300 },
      storage,
    );

    expect(next.map((item) => item.url)).toEqual([
      "https://example.com/a",
      "https://example.com/b",
    ]);
    expect(next[0].title).toBe("A again");
  });

  it("keeps the same url recorded separately per browser", () => {
    const storage = memoryStorage();
    recordBrowserHistory(
      { browserId: "b1", url: "https://example.com/", title: null, visitedAt: 100 },
      storage,
    );
    const next = recordBrowserHistory(
      { browserId: "b2", url: "https://example.com/", title: null, visitedAt: 200 },
      storage,
    );

    expect(next.map((item) => item.browserId)).toEqual(["b2", "b1"]);
  });

  it("does not record anything when history is turned off in settings", () => {
    const storage = memoryStorage({
      [BROWSER_SETTINGS_STORAGE_KEY]: JSON.stringify({ rememberBrowsingHistory: false }),
    });

    const next = recordBrowserHistory(
      { browserId: "b1", url: "https://example.com/", title: null, visitedAt: 100 },
      storage,
    );

    expect(next).toEqual([]);
    expect(storage.getItem(BROWSER_HISTORY_STORAGE_KEY)).toBeNull();
  });

  it("refuses a non-http url and leaves existing history untouched", () => {
    const storage = memoryStorage();
    recordBrowserHistory(
      { browserId: "b1", url: "https://example.com/", title: null, visitedAt: 100 },
      storage,
    );

    const next = recordBrowserHistory(
      { browserId: "b1", url: "javascript:alert(1)", title: null, visitedAt: 200 },
      storage,
    );

    expect(next.map((item) => item.url)).toEqual(["https://example.com/"]);
  });

  it("prunes the oldest visit once the limit is exceeded", () => {
    const seeded = Array.from({ length: BROWSER_HISTORY_LIMIT }, (_unused, index) =>
      entry({ id: `e${index}`, url: `https://example.com/${index}`, visitedAt: index + 1 }),
    );
    const storage = storageWith(seeded);

    const next = recordBrowserHistory(
      { browserId: "b1", url: "https://example.com/newest", title: null, visitedAt: 10_000 },
      storage,
    );

    expect(next).toHaveLength(BROWSER_HISTORY_LIMIT);
    expect(next[0].url).toBe("https://example.com/newest");
    expect(next.some((item) => item.url === "https://example.com/0")).toBe(false);
  });

  it("still reports the recorded history when the storage write is rejected", () => {
    const storage = memoryStorage();
    storage.setItem = () => {
      throw new Error("quota exceeded");
    };

    const next = recordBrowserHistory(
      { browserId: "b1", url: "https://example.com/", title: null, visitedAt: 100 },
      storage,
    );

    expect(next.map((item) => item.url)).toEqual(["https://example.com/"]);
  });
});

describe("clearBrowserHistory", () => {
  it("removes stored history and announces an empty list", () => {
    const storage = storageWith([entry()]);
    const listener = vi.fn();
    window.addEventListener(BROWSER_HISTORY_EVENT, listener);

    clearBrowserHistory(storage);

    window.removeEventListener(BROWSER_HISTORY_EVENT, listener);

    expect(storage.getItem(BROWSER_HISTORY_STORAGE_KEY)).toBeNull();
    expect(loadBrowserHistory(storage)).toEqual([]);
    expect(listener).toHaveBeenCalledTimes(1);
    expect((listener.mock.calls[0][0] as CustomEvent<BrowserHistoryEntry[]>).detail).toEqual([]);
  });

  it("still announces an empty list when storage rejects the removal", () => {
    const storage = memoryStorage();
    storage.removeItem = () => {
      throw new Error("storage disabled");
    };
    const listener = vi.fn();
    window.addEventListener(BROWSER_HISTORY_EVENT, listener);

    clearBrowserHistory(storage);

    window.removeEventListener(BROWSER_HISTORY_EVENT, listener);
    expect(listener).toHaveBeenCalledTimes(1);
  });
});
