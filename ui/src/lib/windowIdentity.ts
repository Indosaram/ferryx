import { isDesktopShell } from "./clientRuntime";

/** Mirrors `window_registry::PRIMARY_WINDOW_LABEL` on the Rust side. */
export const PRIMARY_WINDOW_LABEL = "main";
/** Mirrors `window_registry::SECONDARY_WINDOW_PREFIX` on the Rust side. */
export const SECONDARY_WINDOW_PREFIX = "ws-";

const SECONDARY_LABEL_PATTERN = /^ws-[A-Za-z0-9_-]+$/;

/**
 * Whether this webview runs inside the Ferryx desktop shell. Mirrors `isTauriRuntime()` in
 * tauri.ts, read from the injected Tauri globals so this module stays import-light (it is used by
 * module-level singletons that many tests load with a mocked `./tauri`).
 */
export function isDesktopRuntime(): boolean {
  const scope = globalThis as { isTauri?: unknown; __TAURI_INTERNALS__?: unknown };
  return isDesktopShell(Boolean(scope.isTauri || scope.__TAURI_INTERNALS__));
}

/** "main" or a well-formed "ws-<id>"; anything else is not a Ferryx desktop window. */
export function isAppWindowLabel(label: string): boolean {
  return label === PRIMARY_WINDOW_LABEL || SECONDARY_LABEL_PATTERN.test(label);
}

/**
 * The label of the desktop window this webview renders. The shell opens secondary windows with
 * `?window=ws-<id>`; the primary window (and every non-Tauri surface, such as the remote client)
 * has no such parameter and resolves to "main". A malformed value also resolves to "main" so a
 * bad URL can never point persistence at an arbitrary file.
 */
export function currentWindowLabel(search: string | null = currentSearch()): string {
  if (!isDesktopRuntime() || !search) return PRIMARY_WINDOW_LABEL;
  const label = new URLSearchParams(search).get("window");
  return label && isAppWindowLabel(label) ? label : PRIMARY_WINDOW_LABEL;
}

export function isPrimaryWindow(search: string | null = currentSearch()): boolean {
  return currentWindowLabel(search) === PRIMARY_WINDOW_LABEL;
}

function currentSearch(): string | null {
  return typeof window === "undefined" ? null : window.location?.search ?? null;
}

/**
 * Namespaces a localStorage key for a secondary window so per-window singleton state (for example
 * the active project) does not fight between windows. The primary window keeps the bare key, so
 * single-window installs read and write exactly what they did before.
 */
export function windowScopedStorageKey(key: string, label: string = currentWindowLabel()): string {
  return label === PRIMARY_WINDOW_LABEL ? key : `${key}@${label}`;
}
