import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type NativeMenuPoint = { x: number; y: number };

export type NativeMenuItemSpec = {
  id: string;
  label: string;
  enabled?: boolean;
  icon?: string;
  shortcut?: string;
};

export type NativeMenuEntry =
  | ({ kind: "item" } & NativeMenuItemSpec)
  | { kind: "separator" }
  | { kind: "submenu"; label: string; items: NativeMenuEntry[] };

type NativeMenuCommand =
  | "cmd_native_terminal_context_menu"
  | "cmd_native_tab_context_menu"
  | "cmd_native_new_tab_menu"
  | "cmd_native_sidebar_context_menu";

const MENU_ACTION_EVENT = "ferryx://menu-action";

export async function openNativePopupMenu(
  command: NativeMenuCommand,
  items: NativeMenuEntry[],
  position: NativeMenuPoint,
  onAction: (id: string) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) {
    return () => undefined;
  }

  let cleanedUp = false;
  let unlistenFn: UnlistenFn | null = null;
  let dismissTimer: ReturnType<typeof setTimeout> | null = null;

  const cleanup = () => {
    if (cleanedUp) return;
    cleanedUp = true;
    if (dismissTimer !== null) {
      clearTimeout(dismissTimer);
      dismissTimer = null;
    }
    if (unlistenFn) {
      unlistenFn();
      unlistenFn = null;
    }
  };

  const unlisten = await listen<{ id: string }>(MENU_ACTION_EVENT, (event) => {
    try {
      onAction(event.payload.id);
    } finally {
      cleanup();
    }
  });

  if (cleanedUp) {
    unlisten();
    return () => undefined;
  }
  unlistenFn = unlisten;

  try {
    await invoke(command, { items, position });
    // Once invoke resolves, the OS native popup has closed (either via selection or dismissal).
    // Allow a short window for any queued action event to dispatch, then automatically tear down.
    dismissTimer = setTimeout(cleanup, 200);
  } catch (error) {
    cleanup();
    throw error;
  }

  return cleanup;
}
