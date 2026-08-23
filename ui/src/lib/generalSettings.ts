import { useCallback, useEffect, useState } from "react";

import { GENERAL_SETTINGS_STORAGE_KEY, getMigratedItem } from "./storageKeys";

export type GeneralSettings = {
  confirmCloseTab: boolean;
};

export const DEFAULT_GENERAL_SETTINGS: GeneralSettings = {
  confirmCloseTab: false,
};

export const GENERAL_SETTINGS_EVENT = "ferryx:general-settings";

function hasConfirmCloseTab(value: unknown): value is { confirmCloseTab?: unknown } {
  return typeof value === "object" && value !== null;
}

function normalizeGeneralSettings(value: unknown): GeneralSettings {
  return {
    confirmCloseTab:
      hasConfirmCloseTab(value) && typeof value.confirmCloseTab === "boolean"
        ? value.confirmCloseTab
        : DEFAULT_GENERAL_SETTINGS.confirmCloseTab,
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
