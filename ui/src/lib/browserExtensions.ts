import { invoke } from "@tauri-apps/api/core";

import type { BrowserState } from "./types";

export const CHROME_WEB_STORE_URL = "https://chromewebstore.google.com/category/extensions";
export const OPEN_SETTINGS_EVENT = "ferryx:open-settings";
export const OPEN_BROWSER_URL_EVENT = "ferryx:open-browser-url";
/** Fired after the engine is installed or removed so sibling settings groups can reload. */
export const BROWSER_ENGINE_CHANGED_EVENT = "ferryx:browser-engine-changed";

export type InstalledExtension = {
  id: string;
  name: string;
  version: string;
  description: string | null;
  enabled: boolean;
  iconDataUrl: string | null;
  optionsUrl: string | null;
  homepageUrl: string | null;
  fromWebStore: boolean;
  /** Whether the toolbar button is pinned; null when unknown (the engine is not running). */
  pinned?: boolean | null;
  /** False for extensions Chrome will not let the user remove. */
  removable: boolean;
};

export type BrowserExtensionListing = {
  profile: string;
  engineInstalled: boolean;
  /** The list came from the profile's running engine rather than its files. */
  engineRunning: boolean;
  supported: boolean;
  extensions: InstalledExtension[];
};

export type BrowserExtensionPage =
  | { kind: "manage" }
  | { kind: "details"; id: string }
  | { kind: "options"; id: string };

export type BrowserEnginePaths = {
  installDir: string;
  profilesDir: string;
};

export type OpenSettingsDetail = { section: "browser"; anchor?: string };
export type OpenBrowserUrlDetail = { url: string; profileId?: string };

export async function listBrowserExtensions(profile?: string): Promise<BrowserExtensionListing> {
  return invoke<BrowserExtensionListing>("cmd_browser_extensions_list", { profile });
}

/** Changes go through the profile's engine (started without a tab when needed); each returns the fresh listing. */
export async function setBrowserExtensionEnabled(
  extensionId: string,
  enabled: boolean,
  profile?: string,
): Promise<BrowserExtensionListing> {
  return invoke<BrowserExtensionListing>("cmd_browser_extensions_set_enabled", { profile, extensionId, enabled });
}

/** Chrome also asks the user to confirm in its own dialog; cancelling rejects with details.reason "cancelled". */
export async function removeBrowserExtension(extensionId: string, profile?: string): Promise<BrowserExtensionListing> {
  return invoke<BrowserExtensionListing>("cmd_browser_extensions_remove", { profile, extensionId });
}

export async function setBrowserExtensionPinned(
  extensionId: string,
  pinned: boolean,
  profile?: string,
): Promise<BrowserExtensionListing> {
  return invoke<BrowserExtensionListing>("cmd_browser_extensions_set_pinned", { profile, extensionId, pinned });
}

/** The backend emits browser_session_created and the app adopts the new tab itself. */
export async function openBrowserExtensionPage(
  page: BrowserExtensionPage,
  profile?: string,
): Promise<BrowserState> {
  return invoke<BrowserState>("cmd_browser_extensions_open_page", { profile, page });
}

export async function getBrowserEnginePaths(): Promise<BrowserEnginePaths> {
  return invoke<BrowserEnginePaths>("cmd_browser_engine_paths");
}

/** Asks the app shell to open Settings on `section`, optionally scrolling to an element id. */
export function openSettingsSection(section: "browser", anchor?: string): void {
  window.dispatchEvent(
    new CustomEvent<OpenSettingsDetail>(OPEN_SETTINGS_EVENT, { detail: { section, anchor } }),
  );
}

/** Asks the app shell to open `url` in a new browser tab on `profileId`. */
export function openBrowserUrlInNewTab(url: string, profileId?: string): void {
  window.dispatchEvent(
    new CustomEvent<OpenBrowserUrlDetail>(OPEN_BROWSER_URL_EVENT, { detail: { url, profileId } }),
  );
}
