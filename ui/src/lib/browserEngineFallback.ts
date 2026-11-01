import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const BROWSER_ENGINE_FALLBACK_EVENT = "browser_engine_fallback";

/** A tab that should have used Chromium opened in the system webview; `message` says why. */
export type BrowserEngineFallbackNotice = { browserId: string; message: string };

export function onBrowserEngineFallback(
  listener: (notice: BrowserEngineFallbackNotice) => void,
): Promise<UnlistenFn> {
  return listen<BrowserEngineFallbackNotice>(BROWSER_ENGINE_FALLBACK_EVENT, (event) => listener(event.payload));
}

/** The fallback notice of a tab, once (the event may have fired before the pane listened). */
export async function takeBrowserEngineFallback(browserId: string): Promise<BrowserEngineFallbackNotice | null> {
  return invoke<BrowserEngineFallbackNotice | null>("cmd_browser_engine_take_fallback", { browserId });
}
