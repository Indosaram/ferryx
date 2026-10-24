import { useCallback, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { isDesktopShell } from "./clientRuntime";
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

const LIGHT_QUERY = "(prefers-color-scheme: light)";

/** The last OS theme the desktop window reported; null until a theme event arrives. */
let nativeOsTheme: "light" | "dark" | null = null;

export function _resetNativeOsThemeForTest() {
  nativeOsTheme = null;
}

/** True when the OS asks for light: the native window theme event wins, else prefers-color-scheme. */
export function prefersLightColorScheme(): boolean {
  if (nativeOsTheme) return nativeOsTheme === "light";
  return typeof window !== "undefined"
    && typeof window.matchMedia === "function"
    && window.matchMedia(LIGHT_QUERY).matches;
}

function hasDesktopShell(): boolean {
  return typeof window !== "undefined"
    && isDesktopShell(Boolean((window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__));
}

/**
 * Calls `onChange` whenever the OS color scheme flips: the `prefers-color-scheme` media query,
 * plus the native window theme event in the desktop shell (some WebViews do not update the media
 * query when the OS appearance changes). Returns the unsubscribe function.
 */
export function subscribeSystemColorScheme(onChange: () => void): () => void {
  if (typeof window === "undefined") return () => {};
  const media = typeof window.matchMedia === "function" ? window.matchMedia(LIGHT_QUERY) : null;
  const handleMedia = () => {
    // A media change after a native event is newer information than that event.
    nativeOsTheme = null;
    onChange();
  };
  media?.addEventListener?.("change", handleMedia);

  let active = true;
  let unlistenNative: (() => void) | null = null;
  if (hasDesktopShell()) {
    try {
      const win = getCurrentWindow();
      // Window shims without theme events (older shells, partial mocks) keep the media query only.
      if (typeof win?.onThemeChanged === "function") void win
        .onThemeChanged((event) => {
          if (event.payload !== "light" && event.payload !== "dark") return;
          nativeOsTheme = event.payload;
          onChange();
        })
        .then(
          (unlisten) => {
            if (active) unlistenNative = unlisten;
            else unlisten();
          },
          (error: unknown) => console.warn("Ferryx could not follow the OS theme event", error),
        );
    } catch (error) {
      console.warn("Ferryx could not follow the OS theme event", error);
    }
  }

  return () => {
    active = false;
    media?.removeEventListener?.("change", handleMedia);
    unlistenNative?.();
    unlistenNative = null;
  };
}

function resolvedTheme(theme: AppearanceTheme): Exclude<AppearanceTheme, "system"> {
  if (theme !== "system") return theme;
  return prefersLightColorScheme() ? "light" : "dark";
}

/** Maps the app theme preference onto the light/dark side the native terminal theme follows. */
export function resolveTerminalAppearance(theme: AppearanceTheme, prefersLight: boolean): "light" | "dark" {
  if (theme === "light") return "light";
  if (theme === "system") return prefersLight ? "light" : "dark";
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
    if (settings.theme !== "system") return;
    return subscribeSystemColorScheme(() => applyAppearanceSettings(settings));
  }, [settings]);

  return settings;
}

/** What the theme preference resolves to right now; follows OS flips while it is "system". */
export function useResolvedTheme(theme: AppearanceTheme): Exclude<AppearanceTheme, "system"> {
  const [resolved, setResolved] = useState(() => resolvedTheme(theme));

  useEffect(() => {
    setResolved(resolvedTheme(theme));
    if (theme !== "system") return;
    return subscribeSystemColorScheme(() => setResolved(resolvedTheme(theme)));
  }, [theme]);

  return resolved;
}
