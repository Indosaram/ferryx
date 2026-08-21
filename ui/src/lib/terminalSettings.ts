import { useCallback, useEffect, useState } from "react";

export type TerminalSettings = {
  fontSize: number;
  scrollback: number;
};

export const TERMINAL_SETTINGS_STORAGE_KEY = "orca.terminal.settings";
export const DEFAULT_TERMINAL_SETTINGS: TerminalSettings = {
  fontSize: 13,
  scrollback: 10_000,
};

const TERMINAL_SETTINGS_EVENT = "orca:terminal-settings";
const FONT_SIZE_MIN = 10;
const FONT_SIZE_MAX = 24;
const SCROLLBACK_MIN = 1_000;
const SCROLLBACK_MAX = 100_000;

type TerminalOptionsTarget = {
  options: {
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

export function applyTerminalSettings(terminal: TerminalOptionsTarget, settings: TerminalSettings) {
  terminal.options.fontSize = settings.fontSize;
  terminal.options.scrollback = settings.scrollback;
}

export function useTerminalSettings() {
  const [settings, setSettings] = useState<TerminalSettings>(() => loadTerminalSettings());

  useEffect(() => {
    const handleSettings = (event: Event) => {
      const detail = (event as CustomEvent<TerminalSettings>).detail;
      if (detail) setSettings(normalizeTerminalSettings(detail));
    };
    const handleStorage = (event: StorageEvent) => {
      if (event.key === TERMINAL_SETTINGS_STORAGE_KEY) setSettings(loadTerminalSettings());
    };
    window.addEventListener(TERMINAL_SETTINGS_EVENT, handleSettings);
    window.addEventListener("storage", handleStorage);
    return () => {
      window.removeEventListener(TERMINAL_SETTINGS_EVENT, handleSettings);
      window.removeEventListener("storage", handleStorage);
    };
  }, []);

  const updateSettings = useCallback((patch: Partial<TerminalSettings>) => {
    setSettings((current) => saveTerminalSettings({ ...current, ...patch }));
  }, []);

  return { settings, updateSettings };
}

function normalizeTerminalSettings(settings: Partial<TerminalSettings>): TerminalSettings {
  return {
    fontSize: clampInteger(settings.fontSize, DEFAULT_TERMINAL_SETTINGS.fontSize, FONT_SIZE_MIN, FONT_SIZE_MAX),
    scrollback: clampInteger(settings.scrollback, DEFAULT_TERMINAL_SETTINGS.scrollback, SCROLLBACK_MIN, SCROLLBACK_MAX),
  };
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
