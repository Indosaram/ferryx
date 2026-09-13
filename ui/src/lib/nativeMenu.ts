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
  signal?: AbortSignal,
): Promise<UnlistenFn> {
  if (!isTauri() || signal?.aborted) {
    return () => undefined;
  }

  // The backend round-trips opaque item IDs, including submenu children.
  const popupId = crypto.randomUUID();
  const actionIds = new Map<string, string>();
  const correlate = (entries: NativeMenuEntry[]): NativeMenuEntry[] => entries.map((entry) => {
    if (entry.kind === "submenu") return { ...entry, items: correlate(entry.items) };
    if (entry.kind !== "item") return entry;
    const id = `${popupId}:${actionIds.size}`;
    actionIds.set(id, entry.id);
    return { ...entry, id };
  });
  const popupItems = correlate(items);
  let cleanedUp = false;
  let unlistenFn: UnlistenFn | null = null;
  let dismissTimer: ReturnType<typeof setTimeout> | null = null;

  const cleanup = () => {
    if (cleanedUp) return;
    cleanedUp = true;
    signal?.removeEventListener("abort", cleanup);
    if (dismissTimer !== null) {
      clearTimeout(dismissTimer);
      dismissTimer = null;
    }
    if (unlistenFn) {
      unlistenFn();
      unlistenFn = null;
    }
  };

  signal?.addEventListener("abort", cleanup, { once: true });
  try {
    const unlisten = await listen<{ id: string }>(MENU_ACTION_EVENT, (event) => {
      if (cleanedUp) return;
      const id = actionIds.get(event.payload.id);
      if (id === undefined) return;
      // Revoke ownership before user code can throw, reopen, or dispatch again.
      cleanup();
      onAction(id);
    });

    if (cleanedUp) {
      unlisten();
      return cleanup;
    }
    unlistenFn = unlisten;
    await invoke(command, { items: popupItems, position });
    // Once invoke resolves, the OS native popup has closed (either via selection or dismissal).
    // Allow a short window for any queued action event to dispatch, then automatically tear down.
    if (!cleanedUp) dismissTimer = setTimeout(cleanup, 200);
  } catch (error) {
    cleanup();
    throw error;
  }

  return cleanup;
}
