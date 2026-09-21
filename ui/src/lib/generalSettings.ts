import { useCallback, useEffect, useState } from "react";

import {
  GENERAL_SETTINGS_STORAGE_KEY,
  SIDEBAR_OPEN_STORAGE_KEY,
  getMigratedItem,
} from "./storageKeys";

export type SessionRestorePolicy = "lazy" | "activeOnly" | "eager";

export type GeneralSettings = {
  confirmCloseTab: boolean;
  sessionRestorePolicy: SessionRestorePolicy;
  /** Minutes of idle inactivity before auto-suspend. 0 disables auto-suspend. */
  sessionIdleTimeoutMinutes: number;
};

/** `sessionIdleTimeoutMinutes` value that turns auto-suspend off. */
export const SESSION_IDLE_TIMEOUT_OFF_MINUTES = 0;
export const MIN_SESSION_IDLE_TIMEOUT_MINUTES = 1;
export const MAX_SESSION_IDLE_TIMEOUT_MINUTES = 24 * 60;

export const DEFAULT_GENERAL_SETTINGS: GeneralSettings = {
  confirmCloseTab: false,
  sessionRestorePolicy: "lazy",
  sessionIdleTimeoutMinutes: 30,
};

export const GENERAL_SETTINGS_EVENT = "ferryx:general-settings";

function normalizeGeneralSettings(value: unknown): GeneralSettings {
  const source = typeof value === "object" && value !== null
    ? value as {
        confirmCloseTab?: unknown;
        sessionRestorePolicy?: unknown;
        sessionIdleTimeoutMinutes?: unknown;
      }
    : {};
  const sessionRestorePolicy: SessionRestorePolicy = source.sessionRestorePolicy === "activeOnly" || source.sessionRestorePolicy === "eager"
    ? source.sessionRestorePolicy
    : "lazy";
  const timeout = typeof source.sessionIdleTimeoutMinutes === "number" && Number.isFinite(source.sessionIdleTimeoutMinutes)
    ? Math.round(source.sessionIdleTimeoutMinutes)
    : DEFAULT_GENERAL_SETTINGS.sessionIdleTimeoutMinutes;
  return {
    confirmCloseTab: typeof source.confirmCloseTab === "boolean"
      ? source.confirmCloseTab
      : DEFAULT_GENERAL_SETTINGS.confirmCloseTab,
    sessionRestorePolicy,
    // 0 (and any non-positive value) means auto-suspend is disabled; positive
    // values clamp into the 1..MAX minute range.
    sessionIdleTimeoutMinutes: Math.min(
      MAX_SESSION_IDLE_TIMEOUT_MINUTES,
      Math.max(SESSION_IDLE_TIMEOUT_OFF_MINUTES, timeout),
    ),
  };
}

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function loadGeneralSettings(storage: Storage | null = browserStorage()): GeneralSettings {
  try {
    const raw = getMigratedItem(GENERAL_SETTINGS_STORAGE_KEY, storage);
    return raw ? normalizeGeneralSettings(JSON.parse(raw)) : { ...DEFAULT_GENERAL_SETTINGS };
  } catch {
    return { ...DEFAULT_GENERAL_SETTINGS };
  }
}

export function saveGeneralSettings(
  patch: Partial<GeneralSettings>,
  storage: Storage | null = browserStorage(),
): GeneralSettings {
  const next = normalizeGeneralSettings({ ...loadGeneralSettings(storage), ...patch });
  try {
    storage?.setItem(GENERAL_SETTINGS_STORAGE_KEY, JSON.stringify(next));
  } catch (error) {
    console.warn("Failed to persist general settings", error);
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<GeneralSettings>(GENERAL_SETTINGS_EVENT, { detail: next }));
  }
  return next;
}

export function resetGeneralSettings(storage: Storage | null = browserStorage()): GeneralSettings {
  try {
    storage?.removeItem(GENERAL_SETTINGS_STORAGE_KEY);
  } catch (error) {
    console.warn("Failed to reset general settings", error);
  }
  const next = { ...DEFAULT_GENERAL_SETTINGS };
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<GeneralSettings>(GENERAL_SETTINGS_EVENT, { detail: next }));
  }
  return next;
}

export function loadSidebarOpenStartup(storage: Storage | null = browserStorage()): boolean {
  try {
    const raw = getMigratedItem(SIDEBAR_OPEN_STORAGE_KEY, storage);
    return raw !== null ? raw !== "false" : true;
  } catch {
    return true;
  }
}

export function saveSidebarOpenStartup(open: boolean, storage: Storage | null = browserStorage()): boolean {
  try {
    storage?.setItem(SIDEBAR_OPEN_STORAGE_KEY, String(open));
  } catch (error) {
    console.warn("Failed to persist sidebar open startup preference", error);
  }
  return open;
}

export function useGeneralSettings() {
  const [settings, setSettings] = useState<GeneralSettings>(loadGeneralSettings);

  useEffect(() => {
    const sync = (event: Event) => {
      const detail = event instanceof CustomEvent ? event.detail : undefined;
      setSettings(normalizeGeneralSettings(detail));
    };
    const syncStorage = (event: StorageEvent) => {
      if (event.key === GENERAL_SETTINGS_STORAGE_KEY) setSettings(loadGeneralSettings());
    };
    window.addEventListener(GENERAL_SETTINGS_EVENT, sync);
    window.addEventListener("storage", syncStorage);
    return () => {
      window.removeEventListener(GENERAL_SETTINGS_EVENT, sync);
      window.removeEventListener("storage", syncStorage);
    };
  }, []);

  const updateSettings = useCallback((patch: Partial<GeneralSettings>) => {
    const next = saveGeneralSettings(patch);
    setSettings(next);
    return next;
  }, []);

  const resetSettings = useCallback(() => {
    const next = resetGeneralSettings();
    setSettings(next);
    return next;
  }, []);

  return { settings, updateSettings, resetSettings };
}
