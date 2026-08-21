import { DEFAULT_TERMINAL_FONT_STACK } from "./tauri";
import { useCallback, useEffect, useMemo, useState } from "react";

import { getTerminalPreferences, type TerminalPreferences, type TerminalThemeColors } from "./tauri";

export type TerminalSettings = {
  fontFamily: string | null;
  macosOptionAsAlt: boolean | null;
  fontSize: number | null;
  scrollback: number;
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

export const TERMINAL_SETTINGS_STORAGE_KEY = "orca.terminal.settings";
export const DEFAULT_TERMINAL_SETTINGS: TerminalSettings = {
  fontFamily: null,
  macosOptionAsAlt: null,
  fontSize: null,
  scrollback: 10_000,
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
};

type TerminalOptionsTarget = {
  options: {
    fontFamily?: string;
    macOptionIsMeta?: boolean;
    fontSize?: number;
    scrollback?: number;
    cursorStyle?: "bar" | "block" | "underline";
    theme?: import("@xterm/xterm").ITheme;
  };
};

export function loadTerminalSettings(storage: Pick<Storage, "getItem"> | null = browserStorage()): TerminalSettings {
  if (!storage) return DEFAULT_TERMINAL_SETTINGS;
  try {
    const raw = storage.getItem(TERMINAL_SETTINGS_STORAGE_KEY);
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

export function applyTerminalSettings(terminal: TerminalOptionsTarget, settings: EffectiveTerminalSettings) {
  terminal.options.fontFamily = settings.fontFamily;
  terminal.options.macOptionIsMeta = settings.macosOptionAsAlt;
  terminal.options.fontSize = settings.fontSize;
  terminal.options.scrollback = settings.scrollback;
  terminal.options.cursorStyle = settings.cursorStyle;
  terminal.options.theme = {
    background: settings.theme.background,
    foreground: settings.theme.foreground,
    cursor: settings.theme.cursor,
    cursorAccent: settings.theme.cursorAccent,
    selectionBackground: settings.theme.selectionBackground,
    selectionForeground: settings.theme.selectionForeground,
    black: settings.theme.black,
    red: settings.theme.red,
    green: settings.theme.green,
    yellow: settings.theme.yellow,
    blue: settings.theme.blue,
    magenta: settings.theme.magenta,
    cyan: settings.theme.cyan,
    white: settings.theme.white,
    brightBlack: settings.theme.brightBlack,
    brightRed: settings.theme.brightRed,
    brightGreen: settings.theme.brightGreen,
    brightYellow: settings.theme.brightYellow,
    brightBlue: settings.theme.brightBlue,
    brightMagenta: settings.theme.brightMagenta,
    brightCyan: settings.theme.brightCyan,
    brightWhite: settings.theme.brightWhite,
    extendedAnsi: settings.theme.extendedAnsi,
  };
}

let globalPreferencesPromise: Promise<TerminalPreferences> | null = null;

export function resetTerminalPreferencesCache() {
  globalPreferencesPromise = null;
}

export async function fetchCachedNativePreferences(force = false): Promise<TerminalPreferences> {
  if (!globalPreferencesPromise || force) {
    globalPreferencesPromise = getTerminalPreferences().catch(() => FALLBACK_PREFERENCES);
  }
  return globalPreferencesPromise;
}

export function useTerminalSettings() {
  const [localSettings, setLocalSettings] = useState<TerminalSettings>(() => loadTerminalSettings());
  const [nativePreferences, setNativePreferences] = useState<TerminalPreferences>(FALLBACK_PREFERENCES);

  const refreshNativePreferences = useCallback(async (force = true): Promise<TerminalPreferences> => {
    const preferences = await fetchCachedNativePreferences(force);
    setNativePreferences(preferences);
    return preferences;
  }, []);

  useEffect(() => {
    void refreshNativePreferences(true);
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
  };
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
