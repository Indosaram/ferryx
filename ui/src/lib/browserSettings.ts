import { useCallback, useEffect, useState } from "react";

import { BROWSER_SETTINGS_STORAGE_KEY } from "./storageKeys";

export type BrowserSearchEngine = "duckduckgo" | "google" | "bing" | "brave";

export type BrowserProfile = {
  id: string;
  name: string;
};

export type BrowserSettingsState = {
  homePage: string;
  searchEngine: BrowserSearchEngine;
  defaultZoom: number;
  restoreTabsOnLaunch: boolean;
  openLinksInBuiltInBrowser: boolean;
  shiftOpensSystemBrowser: boolean;
  showTerminalLinkActions: boolean;
  localhostWorktreeLabels: boolean;
  profiles: BrowserProfile[];
  defaultProfileId: string;
};

export const DEFAULT_BROWSER_PROFILE: BrowserProfile = { id: "default", name: "Default" };

export const DEFAULT_BROWSER_SETTINGS: BrowserSettingsState = {
  homePage: "",
  searchEngine: "duckduckgo",
  defaultZoom: 100,
  restoreTabsOnLaunch: false,
  openLinksInBuiltInBrowser: true,
  shiftOpensSystemBrowser: true,
  showTerminalLinkActions: true,
  localhostWorktreeLabels: true,
  profiles: [DEFAULT_BROWSER_PROFILE],
  defaultProfileId: DEFAULT_BROWSER_PROFILE.id,
};

export const BROWSER_SETTINGS_EVENT = "ferryx:browser-settings";

const SEARCH_ENGINES = new Set<BrowserSearchEngine>(["duckduckgo", "google", "bing", "brave"]);
export const BROWSER_ZOOM_LEVELS = [75, 80, 90, 100, 110, 125, 150, 175, 200] as const;
const ZOOM_LEVELS = new Set<number>(BROWSER_ZOOM_LEVELS);
const PROFILE_ID_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function normalizeProfile(input: unknown): BrowserProfile | null {
  if (!input || typeof input !== "object") return null;
  const source = input as Partial<BrowserProfile>;
  const id = typeof source.id === "string" ? source.id.trim() : "";
  const name = typeof source.name === "string" ? source.name.trim() : "";
  if (!PROFILE_ID_PATTERN.test(id) || !name) return null;
  return { id, name: name.slice(0, 80) };
}

function normalizeProfiles(value: unknown): BrowserProfile[] {
  const normalized: BrowserProfile[] = [];
  const seen = new Set<string>();
  if (Array.isArray(value)) {
    for (const candidate of value) {
      const profile = normalizeProfile(candidate);
      if (!profile || seen.has(profile.id)) continue;
      seen.add(profile.id);
      normalized.push(profile);
    }
  }
  if (!seen.has(DEFAULT_BROWSER_PROFILE.id)) normalized.unshift(DEFAULT_BROWSER_PROFILE);
  return normalized.length > 0 ? normalized : [DEFAULT_BROWSER_PROFILE];
}

export function normalizeHomePageInput(input: string): string {
  const trimmed = input.trim();
  if (!trimmed) return "";
  if (/\s/.test(trimmed)) throw new Error("Home page must be a URL without spaces.");
  const localhostInput = /^(?:localhost|127\.0\.0\.1)(?::\d+)?(?:\/|$)/i.test(trimmed);
  if (!localhostInput && /^[A-Za-z][A-Za-z0-9+.-]*:/.test(trimmed) && !/^https?:\/\//i.test(trimmed)) {
    throw new Error("Home page must use http:// or https://.");
  }

  const withScheme = /^(?:https?):\/\//i.test(trimmed)
    ? trimmed
    : localhostInput
      ? `http://${trimmed}`
      : `https://${trimmed}`;

  let parsed: URL;
  try {
    parsed = new URL(withScheme);
  } catch {
    throw new Error("Enter a valid http(s) URL.");
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new Error("Home page must use http:// or https://.");
  }
  return parsed.toString();
}

export function normalizeBrowserSettings(value: unknown): BrowserSettingsState {
  const source = value && typeof value === "object" ? (value as Partial<BrowserSettingsState>) : {};
  const zoom = typeof source.defaultZoom === "number" && Number.isFinite(source.defaultZoom)
    ? Math.round(source.defaultZoom)
    : DEFAULT_BROWSER_SETTINGS.defaultZoom;
  const profiles = normalizeProfiles(source.profiles);
  const profileIds = new Set(profiles.map((profile) => profile.id));
  const requestedDefault = typeof source.defaultProfileId === "string" ? source.defaultProfileId : "";
  let homePage = DEFAULT_BROWSER_SETTINGS.homePage;
  if (typeof source.homePage === "string") {
    try {
      homePage = normalizeHomePageInput(source.homePage);
    } catch {
      homePage = DEFAULT_BROWSER_SETTINGS.homePage;
    }
  }

  return {
    homePage,
    searchEngine: typeof source.searchEngine === "string" && SEARCH_ENGINES.has(source.searchEngine as BrowserSearchEngine)
      ? (source.searchEngine as BrowserSearchEngine)
      : DEFAULT_BROWSER_SETTINGS.searchEngine,
    defaultZoom: ZOOM_LEVELS.has(zoom) ? zoom : DEFAULT_BROWSER_SETTINGS.defaultZoom,
    restoreTabsOnLaunch: typeof source.restoreTabsOnLaunch === "boolean"
      ? source.restoreTabsOnLaunch
      : DEFAULT_BROWSER_SETTINGS.restoreTabsOnLaunch,
    openLinksInBuiltInBrowser: typeof source.openLinksInBuiltInBrowser === "boolean"
      ? source.openLinksInBuiltInBrowser
      : DEFAULT_BROWSER_SETTINGS.openLinksInBuiltInBrowser,
    shiftOpensSystemBrowser: typeof source.shiftOpensSystemBrowser === "boolean"
      ? source.shiftOpensSystemBrowser
      : DEFAULT_BROWSER_SETTINGS.shiftOpensSystemBrowser,
    showTerminalLinkActions: typeof source.showTerminalLinkActions === "boolean"
      ? source.showTerminalLinkActions
      : DEFAULT_BROWSER_SETTINGS.showTerminalLinkActions,
    localhostWorktreeLabels: typeof source.localhostWorktreeLabels === "boolean"
      ? source.localhostWorktreeLabels
      : DEFAULT_BROWSER_SETTINGS.localhostWorktreeLabels,
    profiles,
    defaultProfileId: profileIds.has(requestedDefault) ? requestedDefault : DEFAULT_BROWSER_PROFILE.id,
  };
}

function settingsForPersistence(settings: BrowserSettingsState): Partial<BrowserSettingsState> {
  const persisted: Partial<BrowserSettingsState> = {};
  const scalarKeys: Array<Exclude<keyof BrowserSettingsState, "profiles">> = [
    "homePage",
    "searchEngine",
    "defaultZoom",
    "restoreTabsOnLaunch",
    "openLinksInBuiltInBrowser",
    "shiftOpensSystemBrowser",
    "showTerminalLinkActions",
    "localhostWorktreeLabels",
    "defaultProfileId",
  ];
  for (const key of scalarKeys) {
    if (settings[key] !== DEFAULT_BROWSER_SETTINGS[key]) {
      (persisted as Record<string, unknown>)[key] = settings[key];
    }
  }
  if (JSON.stringify(settings.profiles) !== JSON.stringify(DEFAULT_BROWSER_SETTINGS.profiles)) {
    persisted.profiles = settings.profiles;
  }
  return persisted;
}

export function loadBrowserSettings(storage: Storage | null = browserStorage()): BrowserSettingsState {
  if (!storage) return normalizeBrowserSettings(DEFAULT_BROWSER_SETTINGS);
  try {
    const raw = storage.getItem(BROWSER_SETTINGS_STORAGE_KEY);
    return raw ? normalizeBrowserSettings(JSON.parse(raw)) : normalizeBrowserSettings(DEFAULT_BROWSER_SETTINGS);
  } catch {
    return normalizeBrowserSettings(DEFAULT_BROWSER_SETTINGS);
  }
}

export function saveBrowserSettings(
  patch: Partial<BrowserSettingsState>,
  storage: Storage | null = browserStorage(),
): BrowserSettingsState {
  const next = normalizeBrowserSettings({ ...loadBrowserSettings(storage), ...patch });
  try {
    storage?.setItem(BROWSER_SETTINGS_STORAGE_KEY, JSON.stringify(settingsForPersistence(next)));
  } catch {
    // ignore storage failures
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<BrowserSettingsState>(BROWSER_SETTINGS_EVENT, { detail: next }));
  }
  return next;
}

export function resetBrowserSettings(storage: Storage | null = browserStorage()): BrowserSettingsState {
  try {
    storage?.removeItem(BROWSER_SETTINGS_STORAGE_KEY);
  } catch {
    // ignore
  }
  const next = normalizeBrowserSettings(DEFAULT_BROWSER_SETTINGS);
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<BrowserSettingsState>(BROWSER_SETTINGS_EVENT, { detail: next }));
  }
  return next;
}

export function searchUrlFor(engine: BrowserSearchEngine, query: string): string {
  const encoded = encodeURIComponent(query.trim());
  switch (engine) {
    case "google":
      return `https://www.google.com/search?q=${encoded}`;
    case "bing":
      return `https://www.bing.com/search?q=${encoded}`;
    case "brave":
      return `https://search.brave.com/search?q=${encoded}`;
    case "duckduckgo":
    default:
      return `https://duckduckgo.com/?q=${encoded}`;
  }
}

export function normalizeBrowserAddress(
  input: string,
  settings: BrowserSettingsState = loadBrowserSettings(),
): string {
  const trimmed = input.trim();
  if (!trimmed) return "about:blank";
  if (trimmed === "about:blank" || trimmed.startsWith("http://") || trimmed.startsWith("https://")) return trimmed;
  if (trimmed.startsWith("localhost") || trimmed.startsWith("127.0.0.1")) return `http://${trimmed}`;

  const looksLikeHost = trimmed.includes(".") && !/\s/.test(trimmed);
  if (looksLikeHost) return `https://${trimmed}`;
  return searchUrlFor(settings.searchEngine, trimmed);
}

export function newBrowserTabUrl(settings: BrowserSettingsState = loadBrowserSettings()): string {
  return settings.homePage || "about:blank";
}

export function isHttpUrl(url: string): boolean {
  return /^https?:\/\//i.test(url.trim());
}

export function isLocalhostUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    return parsed.hostname === "localhost" || parsed.hostname === "127.0.0.1";
  } catch {
    return false;
  }
}

export function formatBrowserTabLabel(
  baseLabel: string,
  url: string,
  worktreeLabel: string | null | undefined,
  settings: BrowserSettingsState = loadBrowserSettings(),
): string {
  if (!settings.localhostWorktreeLabels || !worktreeLabel || !isLocalhostUrl(url)) return baseLabel;
  return `${baseLabel} · ${worktreeLabel}`;
}

export function makeBrowserProfileId(name: string, profiles: BrowserProfile[]): string {
  const stem = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^[^a-z0-9]+/, "")
    .slice(0, 40) || "profile";
  const ids = new Set(profiles.map((profile) => profile.id));
  if (!ids.has(stem) && stem !== DEFAULT_BROWSER_PROFILE.id) return stem;
  for (let index = 2; index < 10_000; index += 1) {
    const candidate = `${stem}-${index}`.slice(0, 64);
    if (!ids.has(candidate)) return candidate;
  }
  return `profile-${Date.now()}`;
}

export function useBrowserSettings() {
  const [settings, setSettings] = useState<BrowserSettingsState>(loadBrowserSettings);

  useEffect(() => {
    const sync = (event: Event) => {
      const detail = (event as CustomEvent<BrowserSettingsState>).detail;
      setSettings(detail ? normalizeBrowserSettings(detail) : loadBrowserSettings());
    };
    const storage = (event: StorageEvent) => {
      if (event.key === BROWSER_SETTINGS_STORAGE_KEY) setSettings(loadBrowserSettings());
    };
    window.addEventListener(BROWSER_SETTINGS_EVENT, sync);
    window.addEventListener("storage", storage);
    return () => {
      window.removeEventListener(BROWSER_SETTINGS_EVENT, sync);
      window.removeEventListener("storage", storage);
    };
  }, []);

  const updateSettings = useCallback((patch: Partial<BrowserSettingsState>) => {
    const next = saveBrowserSettings(patch);
    setSettings(next);
    return next;
  }, []);

  const resetSettings = useCallback(() => {
    const next = resetBrowserSettings();
    setSettings(next);
    return next;
  }, []);

  return { settings, updateSettings, resetSettings };
}
