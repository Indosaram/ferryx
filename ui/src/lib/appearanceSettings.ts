import { useCallback, useEffect, useState } from "react";

import { APPEARANCE_SETTINGS_STORAGE_KEY } from "./storageKeys";

export type AppearanceTheme = "charcoal" | "dark" | "light" | "system";
export type AppearanceAccent = "default" | "blue" | "emerald" | "purple" | "amber" | "rose";
export type AppearanceDensity = "compact" | "comfortable";

export type AppearanceSettingsState = {
  theme: AppearanceTheme;
  accentColor: AppearanceAccent;
  density: AppearanceDensity;
};

export const DEFAULT_APPEARANCE_SETTINGS: AppearanceSettingsState = {
  theme: "charcoal",
  accentColor: "default",
  density: "compact",
};

export const APPEARANCE_SETTINGS_EVENT = "ferryx:appearance-settings";

const THEMES = new Set<AppearanceTheme>(["charcoal", "dark", "light", "system"]);
const ACCENTS = new Set<AppearanceAccent>(["default", "blue", "emerald", "purple", "amber", "rose"]);
const DENSITIES = new Set<AppearanceDensity>(["compact", "comfortable"]);

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function normalizeAppearanceSettings(value: unknown): AppearanceSettingsState {
  const source = value && typeof value === "object" ? (value as Partial<AppearanceSettingsState>) : {};
  return {
    theme: typeof source.theme === "string" && THEMES.has(source.theme as AppearanceTheme)
      ? (source.theme as AppearanceTheme)
      : DEFAULT_APPEARANCE_SETTINGS.theme,
    accentColor: typeof source.accentColor === "string" && ACCENTS.has(source.accentColor as AppearanceAccent)
      ? (source.accentColor as AppearanceAccent)
      : DEFAULT_APPEARANCE_SETTINGS.accentColor,
    density: typeof source.density === "string" && DENSITIES.has(source.density as AppearanceDensity)
      ? (source.density as AppearanceDensity)
      : DEFAULT_APPEARANCE_SETTINGS.density,
  };
}

export function loadAppearanceSettings(storage: Storage | null = browserStorage()): AppearanceSettingsState {
  if (!storage) return { ...DEFAULT_APPEARANCE_SETTINGS };
  try {
    const raw = storage.getItem(APPEARANCE_SETTINGS_STORAGE_KEY);
    if (!raw) return { ...DEFAULT_APPEARANCE_SETTINGS };
    return normalizeAppearanceSettings(JSON.parse(raw));
  } catch {
    return { ...DEFAULT_APPEARANCE_SETTINGS };
  }
}

export function saveAppearanceSettings(
  patch: Partial<AppearanceSettingsState>,
  storage: Storage | null = browserStorage(),
): AppearanceSettingsState {
  const next = normalizeAppearanceSettings({ ...loadAppearanceSettings(storage), ...patch });
  try {
    storage?.setItem(APPEARANCE_SETTINGS_STORAGE_KEY, JSON.stringify(next));
  } catch {
    // Keep in-memory state usable even if storage is unavailable.
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<AppearanceSettingsState>(APPEARANCE_SETTINGS_EVENT, { detail: next }));
  }
  return next;
}

export function resetAppearanceSettings(storage: Storage | null = browserStorage()): AppearanceSettingsState {
  try {
    storage?.removeItem(APPEARANCE_SETTINGS_STORAGE_KEY);
  } catch {
    // ignore storage failures
  }
  const next = { ...DEFAULT_APPEARANCE_SETTINGS };
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<AppearanceSettingsState>(APPEARANCE_SETTINGS_EVENT, { detail: next }));
  }
  return next;
}

function resolvedTheme(theme: AppearanceTheme): Exclude<AppearanceTheme, "system"> {
  if (theme !== "system") return theme;
  if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
    return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  }
  return "dark";
}

export function applyAppearanceSettings(
  settings: AppearanceSettingsState,
  root: HTMLElement | null = typeof document !== "undefined" ? document.documentElement : null,
) {
  if (!root) return;
  const theme = resolvedTheme(settings.theme);
  root.dataset.theme = theme;
  root.dataset.themePreference = settings.theme;
  root.dataset.accent = settings.accentColor;
  root.dataset.density = settings.density;
  root.style.colorScheme = theme === "light" ? "light" : "dark";
}

export function useAppearanceSettings() {
  const [settings, setSettings] = useState<AppearanceSettingsState>(loadAppearanceSettings);

  useEffect(() => {
    const sync = (event: Event) => {
      const detail = (event as CustomEvent<AppearanceSettingsState>).detail;
      setSettings(detail ? normalizeAppearanceSettings(detail) : loadAppearanceSettings());
    };
    const storage = (event: StorageEvent) => {
      if (event.key === APPEARANCE_SETTINGS_STORAGE_KEY) setSettings(loadAppearanceSettings());
    };
    window.addEventListener(APPEARANCE_SETTINGS_EVENT, sync);
    window.addEventListener("storage", storage);
    return () => {
      window.removeEventListener(APPEARANCE_SETTINGS_EVENT, sync);
      window.removeEventListener("storage", storage);
    };
  }, []);

  const updateSettings = useCallback((patch: Partial<AppearanceSettingsState>) => {
    const next = saveAppearanceSettings(patch);
    setSettings(next);
    return next;
  }, []);

  const resetSettings = useCallback(() => {
    const next = resetAppearanceSettings();
    setSettings(next);
    return next;
  }, []);

  return { settings, updateSettings, resetSettings };
}

export function useApplyAppearanceSettings() {
  const { settings } = useAppearanceSettings();

  useEffect(() => {
    applyAppearanceSettings(settings);
    if (settings.theme !== "system" || typeof window.matchMedia !== "function") return;
    const query = window.matchMedia("(prefers-color-scheme: light)");
    const handleChange = () => applyAppearanceSettings(settings);
    query.addEventListener?.("change", handleChange);
    return () => query.removeEventListener?.("change", handleChange);
  }, [settings]);

  return settings;
}
