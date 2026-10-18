import { useCallback, useEffect, useState } from "react";

import { SHORTCUTS, type ShortcutActionId, type ShortcutBinding } from "./shortcuts";
import { SHORTCUT_OVERRIDES_STORAGE_KEY, getMigratedItem } from "./storageKeys";

/**
 * Sparse user remaps of the shortcut registry. An entry replaces the action's primary binding
 * (and drops its aliases); `null` unbinds the action so its chord reaches the terminal. Bindings
 * stay abstract (`mod` = Cmd on macOS, Ctrl elsewhere) and never carry a `nonMac` variant.
 */
export type ShortcutOverrides = Partial<Record<ShortcutActionId, ShortcutBinding | null>>;

export const SHORTCUT_OVERRIDES_EVENT = "ferryx:shortcut-overrides";

const MODIFIERS = ["mod", "control", "alt", "shift"] as const;

function normalizeBinding(value: unknown): ShortcutBinding | undefined {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return undefined;
  const source = value as Record<string, unknown>;
  if (typeof source.key !== "string" || source.key.length === 0 || source.key.length > 32) return undefined;
  const binding: { -readonly [K in keyof ShortcutBinding]: ShortcutBinding[K] } = { key: source.key };
  for (const modifier of MODIFIERS) {
    const flag = source[modifier];
    if (flag === undefined) continue;
    if (typeof flag !== "boolean") return undefined;
    if (flag) binding[modifier] = true;
  }
  return binding;
}

export function normalizeShortcutOverrides(value: unknown): ShortcutOverrides {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return {};
  const known = new Set<string>(SHORTCUTS.map((shortcut) => shortcut.id));
  const next: ShortcutOverrides = {};
  for (const [id, raw] of Object.entries(value as Record<string, unknown>)) {
    if (!known.has(id)) continue;
    if (raw === null) {
      next[id as ShortcutActionId] = null;
      continue;
    }
    const binding = normalizeBinding(raw);
    if (binding) next[id as ShortcutActionId] = binding;
  }
  return next;
}

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function loadShortcutOverrides(storage: Storage | null = browserStorage()): ShortcutOverrides {
  try {
    const raw = getMigratedItem(SHORTCUT_OVERRIDES_STORAGE_KEY, storage);
    return raw ? normalizeShortcutOverrides(JSON.parse(raw)) : {};
  } catch {
    return {};
  }
}

function announce(next: ShortcutOverrides) {
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<ShortcutOverrides>(SHORTCUT_OVERRIDES_EVENT, { detail: next }));
  }
}

export function saveShortcutOverrides(
  overrides: ShortcutOverrides,
  storage: Storage | null = browserStorage(),
): ShortcutOverrides {
  const next = normalizeShortcutOverrides(overrides);
  try {
    if (Object.keys(next).length === 0) storage?.removeItem(SHORTCUT_OVERRIDES_STORAGE_KEY);
    else storage?.setItem(SHORTCUT_OVERRIDES_STORAGE_KEY, JSON.stringify(next));
  } catch (error) {
    console.warn("Failed to persist shortcut overrides", error);
  }
  announce(next);
  return next;
}

export function resetShortcutOverrides(storage: Storage | null = browserStorage()): ShortcutOverrides {
  try {
    storage?.removeItem(SHORTCUT_OVERRIDES_STORAGE_KEY);
  } catch (error) {
    console.warn("Failed to reset shortcut overrides", error);
  }
  const next: ShortcutOverrides = {};
  announce(next);
  return next;
}

export function useShortcutOverrides() {
  const [overrides, setOverrides] = useState<ShortcutOverrides>(() => loadShortcutOverrides());

  useEffect(() => {
    const sync = (event: Event) => {
      const detail = event instanceof CustomEvent ? event.detail : undefined;
      setOverrides(normalizeShortcutOverrides(detail));
    };
    const syncStorage = (event: StorageEvent) => {
      if (event.key === SHORTCUT_OVERRIDES_STORAGE_KEY || event.key === null) setOverrides(loadShortcutOverrides());
    };
    window.addEventListener(SHORTCUT_OVERRIDES_EVENT, sync);
    window.addEventListener("storage", syncStorage);
    return () => {
      window.removeEventListener(SHORTCUT_OVERRIDES_EVENT, sync);
      window.removeEventListener("storage", syncStorage);
    };
  }, []);

  /** `undefined` restores the default binding, `null` unbinds the action. */
  const setOverride = useCallback((id: ShortcutActionId, binding: ShortcutBinding | null | undefined) => {
    const current = loadShortcutOverrides();
    const { [id]: _previous, ...rest } = current;
    const next = saveShortcutOverrides(binding === undefined ? rest : { ...rest, [id]: binding });
    setOverrides(next);
    return next;
  }, []);

  const resetOverrides = useCallback(() => {
    const next = resetShortcutOverrides();
    setOverrides(next);
    return next;
  }, []);

  return { overrides, setOverride, resetOverrides };
}
