import { useCallback, useEffect, useMemo, useState } from "react";

import { getTerminalPreferences, type TerminalPreferences } from "./tauri";

export type TerminalSettings = {
  fontFamily: string | null;
  macosOptionAsAlt: boolean | null;
  fontSize: number;
  scrollback: number;
};

export type TerminalSettingSource = "local" | "ghostty" | "fallback";

export type EffectiveTerminalSettings = {
  fontFamily: string;
  macosOptionAsAlt: boolean;
  fontSize: number;
  scrollback: number;
  fontFamilySource: TerminalSettingSource;
  macosOptionAsAltSource: TerminalSettingSource;
};

export const TERMINAL_SETTINGS_STORAGE_KEY = "orca.terminal.settings";
export const DEFAULT_TERMINAL_SETTINGS: TerminalSettings = {
  fontFamily: null,
  macosOptionAsAlt: null,
  fontSize: 13,
  scrollback: 10_000,
};

const TERMINAL_SETTINGS_EVENT = "orca:terminal-settings";
const FONT_SIZE_MIN = 10;
const FONT_SIZE_MAX = 24;
const SCROLLBACK_MIN = 1_000;
const SCROLLBACK_MAX = 100_000;
const FALLBACK_PREFERENCES: TerminalPreferences = {
  fontFamily: "monospace",
  macosOptionAsAlt: false,
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
    // Persistence failure must not prevent applying the settings for this session.
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

  return {
    fontFamily: localFontFamily ?? (nativePreferences.fontFamily || "monospace"),
    macosOptionAsAlt: hasLocalOptionOverride ? local.macosOptionAsAlt! : nativePreferences.macosOptionAsAlt,
    fontSize: local.fontSize,
    scrollback: local.scrollback,
    fontFamilySource: localFontFamily ? "local" : ghosttyImported ? "ghostty" : "fallback",
    macosOptionAsAltSource: hasLocalOptionOverride ? "local" : ghosttyImported ? "ghostty" : "fallback",
  };
}

export function applyTerminalSettings(terminal: TerminalOptionsTarget, settings: EffectiveTerminalSettings) {
  terminal.options.fontFamily = settings.fontFamily;
  terminal.options.macOptionIsMeta = settings.macosOptionAsAlt;
  terminal.options.fontSize = settings.fontSize;
  terminal.options.scrollback = settings.scrollback;
}

export function useTerminalSettings() {
  const [localSettings, setLocalSettings] = useState<TerminalSettings>(() => loadTerminalSettings());
  const [nativePreferences, setNativePreferences] = useState<TerminalPreferences>(FALLBACK_PREFERENCES);

  const refreshNativePreferences = useCallback(async () => {
    try {
      const preferences = await getTerminalPreferences();
      setNativePreferences(preferences);
      return preferences;
    } catch {
      setNativePreferences(FALLBACK_PREFERENCES);
      return FALLBACK_PREFERENCES;
    }
  }, []);

  useEffect(() => {
    void refreshNativePreferences();
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
    fontSize: clampInteger(settings.fontSize, DEFAULT_TERMINAL_SETTINGS.fontSize, FONT_SIZE_MIN, FONT_SIZE_MAX),
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
