import { DEFAULT_TERMINAL_FONT_STACK } from "./tauri";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { getMigratedItem, TERMINAL_SETTINGS_STORAGE_KEY } from "./storageKeys";
import {
  applyTerminalOverrides,
  getTerminalPreferences,
  type TerminalPreferences,
  type TerminalThemeColors,
} from "./tauri";

export type TerminalSettings = {
  fontFamily: string | null;
  macosOptionAsAlt: boolean | null;
  fontSize: number | null;
  scrollback: number;
  shell: string | null;
};

export type TerminalSettingSource = "local" | "ghostty" | "fallback";

export type EffectiveTerminalSettings = {
  fontFamily: string;
  macosOptionAsAlt: boolean;
  fontSize: number;
  scrollback: number;
  cursorStyle: "bar" | "block" | "underline";
  theme: TerminalThemeColors;
  fontFamilySource: TerminalSettingSource;
  macosOptionAsAltSource: TerminalSettingSource;
  fontSizeSource: TerminalSettingSource;
};

export type ResolvedTerminalSettings = EffectiveTerminalSettings;

export { TERMINAL_SETTINGS_STORAGE_KEY };
export const TERMINAL_BACKGROUND_STORAGE_KEY = "ferryx.terminal.background";
export const DEFAULT_TERMINAL_SETTINGS: TerminalSettings = {
  fontFamily: null,
  macosOptionAsAlt: null,
  fontSize: null,
  scrollback: 10_000,
  shell: null,
};

const TERMINAL_SETTINGS_EVENT = "orca:terminal-settings";
const FONT_SIZE_MIN = 10;
const FONT_SIZE_MAX = 36;
const SCROLLBACK_MIN = 1_000;
const SCROLLBACK_MAX = 100_000;

export const DEFAULT_TERMINAL_THEME: TerminalThemeColors = {
  background: "#282c34",
  foreground: "#ffffff",
  cursor: "#ffffff",
  cursorAccent: "#282c34",
  selectionBackground: "#52525299",
  black: "#1d1f21",
  red: "#cc6666",
  green: "#b5bd68",
  yellow: "#f0c674",
  blue: "#81a2be",
  magenta: "#b294bb",
  cyan: "#8abeb7",
  white: "#c5c8c6",
  brightBlack: "#666666",
  brightRed: "#d54e53",
  brightGreen: "#b9ca4a",
  brightYellow: "#e7c547",
  brightBlue: "#7aa6da",
  brightMagenta: "#c397d8",
  brightCyan: "#70c0b1",
  brightWhite: "#eaeaea",
  extendedAnsi: [],
};

export const FALLBACK_PREFERENCES: TerminalPreferences = {
  fontFamily: DEFAULT_TERMINAL_FONT_STACK,
  fontSize: 13,
  macosOptionAsAlt: false,
  cursorStyle: "block",
  theme: DEFAULT_TERMINAL_THEME,
  source: "defaults",
  status: "absent",
  sourcePath: null,
  defaultShell: null,
};

export function loadTerminalSettings(storage: Pick<Storage, "getItem" | "setItem"> | null = browserStorage()): TerminalSettings {
  if (!storage) return DEFAULT_TERMINAL_SETTINGS;
  try {
    const raw = getMigratedItem(TERMINAL_SETTINGS_STORAGE_KEY, storage);
    if (!raw) return DEFAULT_TERMINAL_SETTINGS;
    return normalizeTerminalSettings(JSON.parse(raw) as Partial<TerminalSettings>);
  } catch {
    return DEFAULT_TERMINAL_SETTINGS;
  }
}

export function saveTerminalSettings(
  next: Partial<TerminalSettings>,
  storage: Pick<Storage, "setItem"> | null = browserStorage(),
): TerminalSettings {
  const normalized = normalizeTerminalSettings(next);
  try {
    storage?.setItem(TERMINAL_SETTINGS_STORAGE_KEY, JSON.stringify(normalized));
  } catch {
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<TerminalSettings>(TERMINAL_SETTINGS_EVENT, { detail: normalized }));
  }
  return normalized;
}

export function syncTerminalBackground(background: string) {
  if (typeof document !== "undefined") {
    document.documentElement.style.setProperty("--terminal", background);
    const channels = hexColorChannels(background);
    if (channels) document.documentElement.style.setProperty("--terminal-rgb", channels);
  }
  try {
    browserStorage()?.setItem(TERMINAL_BACKGROUND_STORAGE_KEY, background);
  } catch {
  }
}

export function applyCachedTerminalBackground() {
  try {
    const background = browserStorage()?.getItem(TERMINAL_BACKGROUND_STORAGE_KEY) ?? null;
    if (background && typeof document !== "undefined") {
      document.documentElement.style.setProperty("--terminal", background);
      const channels = hexColorChannels(background);
      if (channels) document.documentElement.style.setProperty("--terminal-rgb", channels);
    }
    return background;
  } catch {
    return null;
  }
}

function hexColorChannels(background: string) {
  const match = /^#([\da-f]{2})([\da-f]{2})([\da-f]{2})$/i.exec(background);
  if (!match) return null;
  return `${Number.parseInt(match[1], 16)} ${Number.parseInt(match[2], 16)} ${Number.parseInt(match[3], 16)}`;
}

export function resolveTerminalSettings(
  local: TerminalSettings,
  nativePreferences: TerminalPreferences = FALLBACK_PREFERENCES,
): EffectiveTerminalSettings {
  const ghosttyImported = nativePreferences.source === "ghostty" && nativePreferences.status === "imported";
  const localFontFamily = normalizeOptionalFontFamily(local.fontFamily);
  const hasLocalOptionOverride = typeof local.macosOptionAsAlt === "boolean";
  const hasLocalFontSizeOverride = typeof local.fontSize === "number" && Number.isFinite(local.fontSize);

  const cursorStyle: "bar" | "block" | "underline" =
    nativePreferences.cursorStyle === "bar" || nativePreferences.cursorStyle === "underline"
      ? nativePreferences.cursorStyle
      : "block";

  const effectiveFontSize = hasLocalFontSizeOverride
    ? local.fontSize!
    : nativePreferences.fontSize ?? 13;

  return {
    fontFamily: localFontFamily ?? (nativePreferences.fontFamily || DEFAULT_TERMINAL_FONT_STACK),
    macosOptionAsAlt: hasLocalOptionOverride ? local.macosOptionAsAlt! : nativePreferences.macosOptionAsAlt,
    fontSize: effectiveFontSize,
    scrollback: local.scrollback,
    cursorStyle,
    theme: nativePreferences.theme ?? DEFAULT_TERMINAL_THEME,
    fontFamilySource: localFontFamily ? "local" : ghosttyImported ? "ghostty" : "fallback",
    macosOptionAsAltSource: hasLocalOptionOverride ? "local" : ghosttyImported ? "ghostty" : "fallback",
    fontSizeSource: hasLocalFontSizeOverride ? "local" : ghosttyImported ? "ghostty" : "fallback",
  };
}

let globalPreferencesPromise: Promise<TerminalPreferences> | null = null;
let lastResolvedPreferences: TerminalPreferences | null = null;
let pushedOverrides: string | null = null;

export function resetTerminalPreferencesCache() {
  globalPreferencesPromise = null;
  lastResolvedPreferences = null;
  pushedOverrides = null;
}

async function syncNativeOverrides(settings: TerminalSettings): Promise<void> {
  const payload = {
    fontFamily: settings.fontFamily,
    fontSize: settings.fontSize,
    macosOptionAsAlt: settings.macosOptionAsAlt,
    shell: settings.shell,
  };
  const serialized = JSON.stringify(payload);
  if (pushedOverrides === serialized) return;
  pushedOverrides = serialized;
  try {
    await applyTerminalOverrides(payload);
  } catch {
    pushedOverrides = null;
  }
}

export async function fetchCachedNativePreferences(force = false): Promise<TerminalPreferences> {
  if (!globalPreferencesPromise || force) {
    try {
      const promise = getTerminalPreferences();
      globalPreferencesPromise =
        promise && typeof promise.catch === "function"
          ? promise.catch(() => FALLBACK_PREFERENCES)
          : Promise.resolve(FALLBACK_PREFERENCES);
    } catch {
      globalPreferencesPromise = Promise.resolve(FALLBACK_PREFERENCES);
    }
    const tracked: Promise<TerminalPreferences> = globalPreferencesPromise.then((preferences) => {
      if (globalPreferencesPromise !== tracked) return globalPreferencesPromise ?? FALLBACK_PREFERENCES;
      lastResolvedPreferences = preferences;
      return preferences;
    });
    globalPreferencesPromise = tracked;
  }
  return globalPreferencesPromise;
}

export function useTerminalSettings() {
  const [localSettings, setLocalSettings] = useState<TerminalSettings>(() => loadTerminalSettings());
  const [resolvedPreferences, setNativePreferences] = useState<TerminalPreferences | null>(
    () => lastResolvedPreferences,
  );
  const nativePreferences = resolvedPreferences ?? FALLBACK_PREFERENCES;
  const preferenceRequest = useRef(0);

  const refreshNativePreferences = useCallback(async (force = true): Promise<TerminalPreferences> => {
    const request = ++preferenceRequest.current;
    const preferences = await fetchCachedNativePreferences(force);
    if (request === preferenceRequest.current) setNativePreferences(preferences);
    return preferences;
  }, []);

  useEffect(() => {
    void refreshNativePreferences(false);
    return () => { preferenceRequest.current += 1; };
  }, [refreshNativePreferences]);

  useEffect(() => {
    const handleSettings = (event: Event) => {
      const detail = (event as CustomEvent<TerminalSettings>).detail;
      if (detail) setLocalSettings(normalizeTerminalSettings(detail));
    };
    const handleStorage = (event: StorageEvent) => {
      if (event.key === TERMINAL_SETTINGS_STORAGE_KEY) setLocalSettings(loadTerminalSettings());
    };
    window.addEventListener(TERMINAL_SETTINGS_EVENT, handleSettings);
    window.addEventListener("storage", handleStorage);
    return () => {
      window.removeEventListener(TERMINAL_SETTINGS_EVENT, handleSettings);
      window.removeEventListener("storage", handleStorage);
    };
  }, []);

  const updateSettings = useCallback((patch: Partial<TerminalSettings>) => {
    setLocalSettings((current) => saveTerminalSettings({ ...current, ...patch }));
  }, []);

  const settings = useMemo(
    () => resolveTerminalSettings(localSettings, nativePreferences),
    [localSettings, nativePreferences],
  );

  useEffect(() => {
    void syncNativeOverrides(localSettings);
  }, [localSettings]);

  useEffect(() => {
    if (resolvedPreferences) syncTerminalBackground(settings.theme.background);
  }, [resolvedPreferences, settings.theme.background]);

  return { settings, localSettings, nativePreferences, updateSettings, refreshNativePreferences };
}

function normalizeTerminalSettings(settings: Partial<TerminalSettings>): TerminalSettings {
  return {
    fontFamily: normalizeOptionalFontFamily(settings.fontFamily),
    macosOptionAsAlt: typeof settings.macosOptionAsAlt === "boolean" ? settings.macosOptionAsAlt : null,
    fontSize: typeof settings.fontSize === "number" && Number.isFinite(settings.fontSize)
      ? clampInteger(settings.fontSize, 13, FONT_SIZE_MIN, FONT_SIZE_MAX)
      : null,
    scrollback: clampInteger(settings.scrollback, DEFAULT_TERMINAL_SETTINGS.scrollback, SCROLLBACK_MIN, SCROLLBACK_MAX),
    shell: normalizeOptionalString(settings.shell),
  };
}

function normalizeOptionalString(value: string | null | undefined) {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function normalizeOptionalFontFamily(value: string | null | undefined) {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function clampInteger(value: number | undefined, fallback: number, min: number, max: number) {
  if (!Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.round(value!)));
}

function browserStorage() {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
