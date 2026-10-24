// Opens outbound links. Inside the native mobile companion a webview `window.open` goes nowhere,
// so links go through the Tauri opener plugin; everywhere else a new tab. Only https/mailto.
import type { MouseEvent } from "react";
import { isMobileCompanion } from "./clientRuntime";

const ALLOWED_PROTOCOLS: ReadonlySet<string> = new Set(["https:", "mailto:"]);

/** The URL when it is an absolute https: or mailto: link, else null. */
export function safeExternalUrl(url: string): string | null {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch (err) {
    if (err instanceof TypeError) return null;
    throw err;
  }
  return ALLOWED_PROTOCOLS.has(parsed.protocol) ? parsed.href : null;
}

function hasTauriInternals(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Opens `url` outside the app. Resolves false when the URL is refused. */
export async function openExternal(url: string): Promise<boolean> {
  const safe = safeExternalUrl(url);
  if (!safe) return false;
  if (isMobileCompanion() && hasTauriInternals()) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(safe);
    return true;
  }
  window.open(safe, "_blank", "noopener,noreferrer");
  return true;
}

/**
 * onClick for an `<a href target="_blank">`: the href stays for long-press and accessibility,
 * the click itself routes through openExternal. Modified clicks and refused schemes keep browser behavior.
 */
export function handleExternalLinkClick(event: MouseEvent<HTMLAnchorElement>): void {
  if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  // A refused scheme (blob:, data:, relative) keeps the browser's own handling.
  const href = event.currentTarget.href;
  if (!safeExternalUrl(href)) return;
  event.preventDefault();
  void openExternal(href).catch((err: unknown) => {
    console.warn("Failed to open external link", err instanceof Error ? err.message : String(err));
  });
}
