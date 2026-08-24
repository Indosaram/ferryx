export const PROJECTS_STORAGE_KEY = "ferryx.projects";
export const ACTIVE_PROJECT_STORAGE_KEY = "ferryx.active-project";
export const SIDEBAR_OPEN_STORAGE_KEY = "ferryx.sidebar.open";
export const SIDEBAR_WIDTH_STORAGE_KEY = "ferryx.sidebar.width";
export const SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY = "ferryx.sidebar.collapsedProjects";
export const TERMINAL_SETTINGS_STORAGE_KEY = "ferryx.terminal.settings";
export const NOTIFICATION_SETTINGS_STORAGE_KEY = "ferryx.settings.notifications:v1";
export const APPEARANCE_SETTINGS_STORAGE_KEY = "ferryx.settings.appearance";
export const BROWSER_SETTINGS_STORAGE_KEY = "ferryx.settings.browser";
export const BROWSER_HISTORY_STORAGE_KEY = "ferryx.browser.history";
export const BROWSER_HISTORY_ENABLED_STORAGE_KEY = "ferryx.browser.history.enabled";
export const GENERAL_SETTINGS_STORAGE_KEY = "ferryx.settings.general";

export const LEGACY_STORAGE_KEY_MAP: Record<string, string[]> = {
  [PROJECTS_STORAGE_KEY]: ["rorca.projects", "orca.projects"],
  [ACTIVE_PROJECT_STORAGE_KEY]: ["rorca.active-project", "orca.active-project"],
  [SIDEBAR_OPEN_STORAGE_KEY]: ["orca.sidebar.open", "rorca.sidebar.open"],
  [SIDEBAR_WIDTH_STORAGE_KEY]: ["orca.sidebar.width", "rorca.sidebar.width"],
  [SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY]: ["rorca.sidebar.collapsedProjects", "orca.sidebar.collapsedProjects"],
  [TERMINAL_SETTINGS_STORAGE_KEY]: ["orca.terminal.settings", "rorca.terminal.settings"],
  [NOTIFICATION_SETTINGS_STORAGE_KEY]: ["rorca:settings:notifications:v1", "orca:settings:notifications:v1"],
  [GENERAL_SETTINGS_STORAGE_KEY]: ["rorca.settings.general", "orca.settings.general"],
};

export function getMigratedItem(
  key: string,
  storage: Pick<Storage, "getItem" | "setItem"> | null = typeof window !== "undefined" && window.localStorage ? window.localStorage : null,
): string | null {
  if (!storage) return null;
  const value = storage.getItem(key);
  if (value !== null) return value;

  const legacyKeys = LEGACY_STORAGE_KEY_MAP[key] ?? [];
  for (const legacyKey of legacyKeys) {
    const legacyVal = storage.getItem(legacyKey);
    if (legacyVal !== null) {
      try {
        storage.setItem(key, legacyVal);
      } catch {
        // ignore quota or disabled storage error
      }
      return legacyVal;
    }
  }
  return null;
}
