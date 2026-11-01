import { beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  goBackBrowser: vi.fn(async (_browserId: string) => undefined),
  goForwardBrowser: vi.fn(async (_browserId: string) => undefined),
  navigateBrowser: vi.fn(async (_browserId: string, _url: string) => undefined),
}));

vi.mock("./browserTauri", () => native);

import { saveBrowserSettings } from "./browserSettings";
import {
  BROWSER_TAB_NAVIGATION_LIMIT,
  BrowserTabNavigationStore,
  goBackInBrowserTab,
  goForwardInBrowserTab,
  parseBrowserTabNavigation,
  serializeBrowserTabNavigation,
} from "./browserTabNavigation";
import { BROWSER_TAB_NAVIGATION_STORAGE_KEY } from "./storageKeys";

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

function visit(store: BrowserTabNavigationStore, browserId: string, urls: string[], profileId = "default") {
  for (const url of urls) store.record(browserId, url, `Title ${url}`, profileId);
}

beforeEach(() => {
  native.goBackBrowser.mockClear();
  native.goForwardBrowser.mockClear();
  native.navigateBrowser.mockClear();
});

describe("browser tab navigation persistence", () => {
  it("round-trips entries, titles and the current index through serialization", () => {
    const stacks = {
      "browser-1": {
        entries: [
          { url: "https://a.test/", title: "A" },
          { url: "https://b.test/", title: null },
          { url: "https://c.test/", title: "C" },
        ],
        index: 1,
        updatedAt: 42,
      },
    };
    expect(parseBrowserTabNavigation(serializeBrowserTabNavigation(stacks))).toEqual(stacks);
  });

  it("persists under the ferryx.* key and restores the stack in a new store (restart)", () => {
    const storage = memoryStorage();
    const before = new BrowserTabNavigationStore(() => storage);
    visit(before, "browser-1", ["https://a.test/", "https://b.test/", "https://c.test/"]);
    expect(BROWSER_TAB_NAVIGATION_STORAGE_KEY).toBe("ferryx.browser.tab-navigation");
    expect(storage.getItem(BROWSER_TAB_NAVIGATION_STORAGE_KEY)).not.toBeNull();

    const after = new BrowserTabNavigationStore(() => storage);
    expect(after.snapshot("browser-1")).toEqual({
      entries: [
        { url: "https://a.test/", title: "Title https://a.test/" },
        { url: "https://b.test/", title: "Title https://b.test/" },
        { url: "https://c.test/", title: "Title https://c.test/" },
      ],
      index: 2,
    });
  });

  it("caps each tab's stack, dropping the oldest entries", () => {
    const storage = memoryStorage();
    const store = new BrowserTabNavigationStore(() => storage);
    const urls = Array.from({ length: BROWSER_TAB_NAVIGATION_LIMIT + 15 }, (_, i) => `https://site.test/${i}`);
    visit(store, "browser-1", urls);

    const restored = new BrowserTabNavigationStore(() => storage).snapshot("browser-1");
    expect(restored?.entries).toHaveLength(BROWSER_TAB_NAVIGATION_LIMIT);
    expect(restored?.entries[0].url).toBe("https://site.test/15");
    expect(restored?.entries.at(-1)?.url).toBe(urls.at(-1));
    expect(restored?.index).toBe(BROWSER_TAB_NAVIGATION_LIMIT - 1);

    const oversized = JSON.stringify({
      "browser-2": { entries: urls.map((url) => ({ url, title: null })), index: 3, updatedAt: 1 },
    });
    const parsed = parseBrowserTabNavigation(oversized)["browser-2"];
    expect(parsed.entries).toHaveLength(BROWSER_TAB_NAVIGATION_LIMIT);
    // The current entry is never the one dropped.
    expect(parsed.entries[parsed.index].url).toBe("https://site.test/3");
  });

  it("never records or persists private tabs", () => {
    const storage = memoryStorage();
    const store = new BrowserTabNavigationStore(() => storage);
    visit(store, "private-tab", ["https://secret.test/1", "https://secret.test/2"], "private");
    visit(store, "remote-tab", ["https://secret.test/3"], "remote");

    expect(store.snapshot("private-tab")).toBeNull();
    expect(store.snapshot("remote-tab")).toBeNull();
    expect(storage.getItem(BROWSER_TAB_NAVIGATION_STORAGE_KEY) ?? "").not.toContain("secret.test");
  });

  it("is not persisted or restored when tab restore is off", () => {
    const storage = memoryStorage();
    saveBrowserSettings({ restoreTabsOnLaunch: false }, storage);
    const store = new BrowserTabNavigationStore(() => storage);
    visit(store, "browser-1", ["https://a.test/", "https://b.test/"]);
    expect(storage.getItem(BROWSER_TAB_NAVIGATION_STORAGE_KEY)).toBeNull();
  });
});

describe("browser tab back/forward after restore", () => {
  function restoredStore() {
    const storage = memoryStorage();
    visit(new BrowserTabNavigationStore(() => storage), "browser-1", ["https://a.test/", "https://b.test/", "https://c.test/"]);
    const store = new BrowserTabNavigationStore(() => storage);
    store.restore("browser-1", "https://c.test/");
    return store;
  }

  it("navigates to the previous url when the engine's history is empty", async () => {
    const store = restoredStore();
    expect(store.availability("browser-1", false, false)).toMatchObject({ canGoBack: true, canGoForward: false, restoredBack: true });

    await goBackInBrowserTab("browser-1", store);
    expect(native.navigateBrowser).toHaveBeenCalledExactlyOnceWith("browser-1", "https://b.test/");
    expect(native.goBackBrowser).not.toHaveBeenCalled();

    // The page commits; the stack moves back instead of recording a new visit.
    store.record("browser-1", "https://b.test/", "B", "default");
    expect(store.snapshot("browser-1")?.index).toBe(1);
    expect(store.availability("browser-1", true, false)).toMatchObject({ canGoBack: true, canGoForward: true, restoredForward: true });

    await goForwardInBrowserTab("browser-1", store);
    expect(native.navigateBrowser).toHaveBeenLastCalledWith("browser-1", "https://c.test/");
    store.record("browser-1", "https://c.test/", "C", "default");
    expect(store.snapshot("browser-1")).toMatchObject({ index: 2 });
    expect(store.snapshot("browser-1")?.entries).toHaveLength(3);
  });

  it("lets the engine drive pages it visited itself after the restore", async () => {
    const store = restoredStore();
    store.record("browser-1", "https://d.test/", "D", "default");
    expect(store.snapshot("browser-1")?.entries.map((entry) => entry.url)).toEqual([
      "https://a.test/",
      "https://b.test/",
      "https://c.test/",
      "https://d.test/",
    ]);

    await goBackInBrowserTab("browser-1", store);
    expect(native.goBackBrowser).toHaveBeenCalledExactlyOnceWith("browser-1");
    store.record("browser-1", "https://c.test/", "C", "default");

    // c.test is the oldest page the engine knows: further back is served from the saved stack.
    await goBackInBrowserTab("browser-1", store);
    expect(native.navigateBrowser).toHaveBeenCalledExactlyOnceWith("browser-1", "https://b.test/");
  });

  it("uses the engine's own history for tabs that were never restored", async () => {
    const store = new BrowserTabNavigationStore(() => memoryStorage());
    visit(store, "browser-1", ["https://a.test/", "https://b.test/"]);
    await goBackInBrowserTab("browser-1", store);
    expect(native.goBackBrowser).toHaveBeenCalledExactlyOnceWith("browser-1");
    expect(native.navigateBrowser).not.toHaveBeenCalled();
  });
});
