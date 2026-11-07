import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  normalizeSuggestionUrl,
  rankOmniboxSuggestions,
  type HostHistoryEntry,
} from "./browserLibrary";
import { BROWSER_HISTORY_STORAGE_KEY } from "./storageKeys";

function visit(url: string, frecency: number, title: string | null = null): HostHistoryEntry {
  return { url, title, visitCount: 1, lastVisitedAt: 1, frecency };
}

describe("rankOmniboxSuggestions", () => {
  it("dedupes by normalized url and keeps the best score", () => {
    const ranked = rankOmniboxSuggestions("exa", {
      history: [visit("https://example.com/", 10, "Example Domain"), visit("https://example.com", 500), visit("https://example.com/#x", 1)],
      bookmarks: [],
      tabs: [],
      searchSuggestions: [],
    });
    expect(ranked).toHaveLength(1);
    expect(ranked[0].title).toBe("Example Domain");
  });

  it("orders equally matching history by frecency", () => {
    const ranked = rankOmniboxSuggestions("site", {
      history: [visit("https://site-a.example/", 10), visit("https://site-b.example/", 900)],
      bookmarks: [],
      tabs: [],
      searchSuggestions: [],
    });
    expect(ranked.map((s) => s.url)).toEqual(["https://site-b.example/", "https://site-a.example/"]);
  });

  it("marks bookmarked history, never offers the current tab, and puts search suggestions last", () => {
    const ranked = rankOmniboxSuggestions("rust", {
      history: [visit("https://rust-lang.org/", 50)],
      bookmarks: [{ id: "b", title: "Rust", url: "https://rust-lang.org", parentId: "bar", position: 0, createdAt: 1 }],
      tabs: [{ browserId: "self", url: "https://rust-lang.org/learn", title: "Learn" }],
      searchSuggestions: ["rust async", "rust"],
      currentBrowserId: "self",
    });
    expect(ranked.map((s) => s.kind)).toEqual(["history", "search"]);
    expect(ranked[0].bookmarked).toBe(true);
    expect(ranked[1].query).toBe("rust async");
  });

  it("drops non-page urls", () => {
    expect(rankOmniboxSuggestions("", {
      history: [visit("about:blank", 1), visit("javascript:alert(1)", 1)],
      bookmarks: [],
      tabs: [],
      searchSuggestions: [],
    })).toEqual([]);
  });

  it("normalizes scheme/host case, fragments and trailing slashes", () => {
    expect(normalizeSuggestionUrl("HTTPS://Example.COM/a/#frag")).toBe("https://example.com/a");
  });
});

describe("migrateLegacyBrowserHistory", () => {
  beforeEach(() => {
    vi.resetModules();
    invoke.mockReset();
    localStorage.clear();
  });

  it("moves the localStorage history to the host once and removes the local copy", async () => {
    localStorage.setItem(BROWSER_HISTORY_STORAGE_KEY, JSON.stringify([
      { id: "1", browserId: "b", url: "https://a.example/", title: "A", visitedAt: 1000 },
      { id: "2", browserId: "b", url: "about:blank", title: null, visitedAt: 2000 },
    ]));
    invoke.mockResolvedValue({ imported: 1, alreadyMigrated: false });
    const { migrateLegacyBrowserHistory } = await import("./browserLibrary");

    await expect(migrateLegacyBrowserHistory()).resolves.toBe(1);
    expect(invoke).toHaveBeenCalledWith("cmd_browser_history_migrate_legacy", {
      visits: [{ url: "https://a.example/", title: "A", visitedAt: 1000, visitCount: 1 }],
    });
    expect(localStorage.getItem(BROWSER_HISTORY_STORAGE_KEY)).toBeNull();

    await migrateLegacyBrowserHistory();
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("keeps the local copy when the host refuses, so a later launch retries", async () => {
    localStorage.setItem(BROWSER_HISTORY_STORAGE_KEY, JSON.stringify([
      { id: "1", browserId: "b", url: "https://a.example/", title: "A", visitedAt: 1000 },
    ]));
    invoke.mockRejectedValue({ code: "IO_ERROR", message: "disk" });
    const { migrateLegacyBrowserHistory } = await import("./browserLibrary");

    await expect(migrateLegacyBrowserHistory()).rejects.toMatchObject({ code: "IO_ERROR" });
    expect(localStorage.getItem(BROWSER_HISTORY_STORAGE_KEY)).not.toBeNull();
  });

  it("does nothing without legacy history", async () => {
    const { migrateLegacyBrowserHistory } = await import("./browserLibrary");
    await expect(migrateLegacyBrowserHistory()).resolves.toBe(0);
    expect(invoke).not.toHaveBeenCalled();
  });
});
