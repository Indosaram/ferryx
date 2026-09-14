export const PROJECTS_STORAGE_KEY = "ferryx.projects";
export const ACTIVE_PROJECT_STORAGE_KEY = "ferryx.active-project";
export const SIDEBAR_OPEN_STORAGE_KEY = "ferryx.sidebar.open";
export const SIDEBAR_WIDTH_STORAGE_KEY = "ferryx.sidebar.width";
export const SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY = "ferryx.sidebar.collapsedProjects";
export const SIDEBAR_WORKTREE_ORDER_STORAGE_KEY = "ferryx.sidebar.worktreeOrder";
export const TERMINAL_SETTINGS_STORAGE_KEY = "ferryx.terminal.settings";
export const NOTIFICATION_SETTINGS_STORAGE_KEY = "ferryx.settings.notifications:v1";
export const NOTIFICATION_HISTORY_STORAGE_KEY = "ferryx.notifications.history:v1";
export const APPEARANCE_SETTINGS_STORAGE_KEY = "ferryx.settings.appearance";
export const BROWSER_SETTINGS_STORAGE_KEY = "ferryx.settings.browser";
export const BROWSER_HISTORY_STORAGE_KEY = "ferryx.browser.history";
export const BROWSER_HISTORY_ENABLED_STORAGE_KEY = "ferryx.browser.history.enabled";
export const GENERAL_SETTINGS_STORAGE_KEY = "ferryx.settings.general";
export const DISMISSED_UPDATE_VERSION_STORAGE_KEY = "ferryx.update.dismissedVersion";
export const PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY = "ferryx.permissions.onboarding-dismissed";
export const SSH_CONFIG_PATH_STORAGE_KEY = "ferryx.ssh.configPath";
export const REMOTE_INSTALLATION_ID_STORAGE_KEY = "ferryx.remote.installation-id";
export const WORKTREE_DISK_UNUSED_DAYS_KEY = "ferryx.worktree-disk.unused-days";

export const LEGACY_STORAGE_KEY_MAP: Record<string, string[]> = {
  [PROJECTS_STORAGE_KEY]: ["rorca.projects", "orca.projects"],
  [ACTIVE_PROJECT_STORAGE_KEY]: ["rorca.active-project", "orca.active-project"],
  [SIDEBAR_OPEN_STORAGE_KEY]: ["orca.sidebar.open", "rorca.sidebar.open"],
  [SIDEBAR_WIDTH_STORAGE_KEY]: ["orca.sidebar.width", "rorca.sidebar.width"],
  [SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY]: ["rorca.sidebar.collapsedProjects", "orca.sidebar.collapsedProjects"],
  [TERMINAL_SETTINGS_STORAGE_KEY]: ["orca.terminal.settings", "rorca.terminal.settings"],
  [NOTIFICATION_SETTINGS_STORAGE_KEY]: ["rorca:settings:notifications:v1", "orca:settings:notifications:v1"],
  [NOTIFICATION_HISTORY_STORAGE_KEY]: ["rorca.notifications.history:v1", "orca.notifications.history:v1"],
  [GENERAL_SETTINGS_STORAGE_KEY]: ["rorca.settings.general", "orca.settings.general"],
  [REMOTE_INSTALLATION_ID_STORAGE_KEY]: ["rorca.remote.installation-id", "orca.remote.installation-id"],
};

export function getMigratedItem(
  key: string,
  // `removeItem` is optional: callers legitimately pass narrower storage shims
  // (see notificationCenterPersistence.ts and terminalSettings.ts). Migration
  // consumes the legacy key when the shim supports it and otherwise degrades to
  // the old copy-forward behaviour rather than forcing every caller to widen.
  storage: (Pick<Storage, "getItem" | "setItem"> & Partial<Pick<Storage, "removeItem">>) | null = typeof window !== "undefined" && window.localStorage ? window.localStorage : null,
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
        // Migration is ONE-SHOT: consume the legacy entry once it has been copied
        // forward. Leaving it behind makes every reset*() helper ineffective --
        // they delete only the canonical key, so the next load re-reads the legacy
        // blob and resurrects the user's pre-upgrade settings. "Reset to defaults"
        // could never complete for anyone upgrading from an orca/rorca build.
        storage.removeItem?.(legacyKey);
      } catch {
        // ignore quota or disabled storage error
      }
      return legacyVal;
    }
  }
  return null;
}

export function getOrCreateInstallationId(
  storage: (Pick<Storage, "getItem" | "setItem"> & Partial<Pick<Storage, "removeItem">>) | null = typeof window !== "undefined" && window.localStorage ? window.localStorage : null,
): string {
  const existing = getMigratedItem(REMOTE_INSTALLATION_ID_STORAGE_KEY, storage);
  if (existing && existing.trim().length > 0) {
    return existing.trim();
  }
  const next = typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  if (storage) {
    try {
      storage.setItem(REMOTE_INSTALLATION_ID_STORAGE_KEY, next);
    } catch {
      // ignore quota or disabled storage error
    }
  }
  return next;
}

