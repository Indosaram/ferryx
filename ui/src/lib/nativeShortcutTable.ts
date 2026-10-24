import { invoke } from "@tauri-apps/api/core";

import {
  SHORTCUT_OVERRIDES_EVENT,
  SHORTCUT_PRESET_EVENT,
  isShortcutPreset,
  loadShortcutOverrides,
  loadShortcutPreset,
  nativeShortcutTableFor,
  normalizeShortcutOverrides,
  type ShortcutOverrides,
} from "./shortcutOverrides";
import type { ShortcutPreset } from "./shortcuts";
import { SHORTCUT_OVERRIDES_STORAGE_KEY, SHORTCUT_PRESET_STORAGE_KEY } from "./storageKeys";

/**
 * Pushes the user's shortcut overrides to the native key sources (macOS key monitor and menu,
 * browser-guest key handlers on every platform). Rust resolves the sparse table against the
 * built-in defaults, so an empty table restores today's behavior.
 */
function pushNativeShortcutTable(table: ShortcutOverrides) {
  invoke("cmd_shortcuts_set_native_table", { table }).catch((error) => {
    console.warn("Failed to push native shortcut table", error);
  });
}

export function installNativeShortcutTablePush(): () => void {
  // The preset's bindings travel as overrides so native key sources (browser guests on every
  // platform) stop claiming the chords the terminal-first preset hands to the terminal.
  let overrides: ShortcutOverrides = loadShortcutOverrides();
  let preset: ShortcutPreset = loadShortcutPreset();
  const push = () => pushNativeShortcutTable(nativeShortcutTableFor(overrides, preset));
  push();
  const onOverrides = (event: Event) => {
    const detail = event instanceof CustomEvent ? event.detail : undefined;
    overrides = normalizeShortcutOverrides(detail);
    push();
  };
  const onPreset = (event: Event) => {
    const detail = event instanceof CustomEvent ? event.detail : undefined;
    if (!isShortcutPreset(detail)) return;
    preset = detail;
    push();
  };
  const onStorage = (event: StorageEvent) => {
    if (event.key === SHORTCUT_OVERRIDES_STORAGE_KEY || event.key === SHORTCUT_PRESET_STORAGE_KEY || event.key === null) {
      overrides = loadShortcutOverrides();
      preset = loadShortcutPreset();
      push();
    }
  };
  window.addEventListener(SHORTCUT_OVERRIDES_EVENT, onOverrides);
  window.addEventListener(SHORTCUT_PRESET_EVENT, onPreset);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(SHORTCUT_OVERRIDES_EVENT, onOverrides);
    window.removeEventListener(SHORTCUT_PRESET_EVENT, onPreset);
    window.removeEventListener("storage", onStorage);
  };
}
