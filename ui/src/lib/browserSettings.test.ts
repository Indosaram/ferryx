import { beforeEach, describe, expect, it } from "vitest";

import {
  BROWSER_ZOOM_LEVELS,
  DEFAULT_BROWSER_SETTINGS,
  formatBrowserTabLabel,
  loadBrowserSettings,
  makeBrowserProfileId,
  newBrowserTabUrl,
  normalizeBrowserAddress,
  normalizeBrowserSettings,
  normalizeHomePageInput,
  saveBrowserSettings,
  searchUrlFor,
} from "./browserSettings";
import { BROWSER_SETTINGS_STORAGE_KEY } from "./storageKeys";

beforeEach(() => {
  localStorage.clear();
});

describe("browser settings", () => {
  it("normalizes and validates the default home page", () => {
    expect(normalizeHomePageInput("")).toBe("");
    expect(normalizeHomePageInput("example.com/path")).toBe("https://example.com/path");
    expect(normalizeHomePageInput("localhost:5173")).toBe("http://localhost:5173/");
    expect(normalizeHomePageInput("127.0.0.1:3000/app")).toBe("http://127.0.0.1:3000/app");
    expect(() => normalizeHomePageInput("ftp://example.com")).toThrow();
    expect(() => normalizeHomePageInput("not a host")).toThrow();
  });

  it("persists the home page and uses blank when unset", () => {
    expect(newBrowserTabUrl()).toBe("about:blank");
    saveBrowserSettings({ homePage: "https://example.com/home" });
    expect(loadBrowserSettings().homePage).toBe("https://example.com/home");
    expect(newBrowserTabUrl()).toBe("https://example.com/home");
  });

  it("keeps the history preference independent from partial browser-settings writes", () => {
    saveBrowserSettings({ rememberBrowsingHistory: false });
    expect(loadBrowserSettings().rememberBrowsingHistory).toBe(false);

    localStorage.setItem(BROWSER_SETTINGS_STORAGE_KEY, JSON.stringify({
      searchEngine: "google",
      defaultZoom: 125,
      restoreTabsOnLaunch: true,
    }));
    expect(loadBrowserSettings().rememberBrowsingHistory).toBe(false);

    saveBrowserSettings({ rememberBrowsingHistory: true });
    expect(loadBrowserSettings().rememberBrowsingHistory).toBe(true);
  });

  it("uses one search helper for Google, Bing, DuckDuckGo, and Brave Search", () => {
    expect(searchUrlFor("google", "orca browser")).toBe("https://www.google.com/search?q=orca%20browser");
    expect(searchUrlFor("bing", "orca browser")).toBe("https://www.bing.com/search?q=orca%20browser");
    expect(searchUrlFor("duckduckgo", "orca browser")).toBe("https://duckduckgo.com/?q=orca%20browser");
    expect(searchUrlFor("brave", "orca browser")).toBe("https://search.brave.com/search?q=orca%20browser");

    const google = normalizeBrowserSettings({ ...DEFAULT_BROWSER_SETTINGS, searchEngine: "google" });
    expect(normalizeBrowserAddress("same query", google)).toBe("https://www.google.com/search?q=same%20query");
    expect(normalizeBrowserAddress("localhost:3000", google)).toBe("http://localhost:3000");
    expect(normalizeBrowserAddress("example.com", google)).toBe("https://example.com");
  });

  it("exposes every required default zoom level", () => {
    expect(BROWSER_ZOOM_LEVELS).toEqual([75, 80, 90, 100, 110, 125, 150, 175, 200]);
    expect(normalizeBrowserSettings({ defaultZoom: 125 }).defaultZoom).toBe(125);
    expect(normalizeBrowserSettings({ defaultZoom: 123 }).defaultZoom).toBe(100);
  });

  it("shows a worktree label only for localhost URLs while enabled", () => {
    const enabled = normalizeBrowserSettings({ localhostWorktreeLabels: true });
    const disabled = normalizeBrowserSettings({ localhostWorktreeLabels: false });
    expect(formatBrowserTabLabel("localhost:3000", "http://localhost:3000", "feature/login", enabled))
      .toBe("localhost:3000 · feature/login");
    expect(formatBrowserTabLabel("localhost:3000", "http://127.0.0.1:3000", "feature/login", enabled))
      .toBe("localhost:3000 · feature/login");
    expect(formatBrowserTabLabel("Docs", "https://example.com", "feature/login", enabled)).toBe("Docs");
    expect(formatBrowserTabLabel("localhost:3000", "http://localhost:3000", "feature/login", disabled))
      .toBe("localhost:3000");
  });

  it("keeps the built-in default profile and falls back deterministically when the selected default is missing", () => {
    const normalized = normalizeBrowserSettings({
      profiles: [{ id: "work", name: "Work" }],
      defaultProfileId: "missing",
    });
    expect(normalized.profiles).toEqual([
      { id: "default", name: "Default" },
      { id: "private", name: "Private" },
      { id: "work", name: "Work" },
    ]);
    expect(normalized.defaultProfileId).toBe("default");
  });

  it("creates stable collision-free persisted profile ids", () => {
    expect(makeBrowserProfileId("Work Account", [{ id: "default", name: "Default" }])).toBe("work-account");
    expect(makeBrowserProfileId("Work Account", [
      { id: "default", name: "Default" },
      { id: "work-account", name: "Work Account" },
    ])).toBe("work-account-2");
  });
});
