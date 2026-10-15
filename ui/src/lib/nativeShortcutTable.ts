import { invoke } from "@tauri-apps/api/core";

import {
  SHORTCUT_OVERRIDES_EVENT,
  loadShortcutOverrides,
  normalizeShortcutOverrides,
  type ShortcutOverrides,
} from "./shortcutOverrides";
import { SHORTCUT_OVERRIDES_STORAGE_KEY } from "./storageKeys";

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
  pushNativeShortcutTable(loadShortcutOverrides());
  const onOverrides = (event: Event) => {
    const detail = event instanceof CustomEvent ? event.detail : undefined;
    pushNativeShortcutTable(normalizeShortcutOverrides(detail));
  };
  const onStorage = (event: StorageEvent) => {
    if (event.key === SHORTCUT_OVERRIDES_STORAGE_KEY || event.key === null) {
      pushNativeShortcutTable(loadShortcutOverrides());
    }
  };
  window.addEventListener(SHORTCUT_OVERRIDES_EVENT, onOverrides);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(SHORTCUT_OVERRIDES_EVENT, onOverrides);
    window.removeEventListener("storage", onStorage);
  };
}
