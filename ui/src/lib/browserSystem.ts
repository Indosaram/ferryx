// Downloads, print/PDF, find events, per-site zoom for browser tabs (both engines).
import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { BROWSER_SETTINGS_EVENT, loadBrowserSettings, normalizeBrowserSettings, type BrowserSettingsState } from "./browserSettings";

export const BROWSER_FIND_RESULT_EVENT = "browser_find_result";
export const BROWSER_PDF_SAVED_EVENT = "browser_pdf_saved";

export type BrowserZoomDirection = "in" | "out" | "reset";

export type BrowserFindResultPayload = {
  browserId: string;
  matchCount: number;
  activeMatch: number;
  finalUpdate: boolean;
};

export type BrowserPdfSavedPayload = {
  browserId: string;
  ok: boolean;
  path: string;
};

export type BrowserDownloadPreferences = {
  askWhereToSave: boolean;
  directory?: string | null;
};

/** Steps the tab's zoom (Cmd/Ctrl +, -, 0); the host remembers it for the page's site. */
export function stepBrowserZoom(browserId: string, direction: BrowserZoomDirection): Promise<number> {
  return invoke<number>("cmd_browser_zoom_step", { browserId, direction });
}

export function printBrowser(browserId: string): Promise<void> {
  return invoke<void>("cmd_browser_print", { browserId });
}

/** Resolves false when the user cancelled the save dialog. */
export function saveBrowserPdf(browserId: string): Promise<boolean> {
  return invoke<boolean>("cmd_browser_save_pdf", { browserId });
}

export function setBrowserDownloadPreferences(preferences: BrowserDownloadPreferences): Promise<void> {
  return invoke<void>("cmd_browser_set_download_preferences", { preferences });
}

/** Keeps the host's download preferences in step with the browser settings. */
export function useBrowserDownloadPreferencesSync(onError: (error: unknown) => void): void {
  useEffect(() => {
    const push = (settings: BrowserSettingsState) => {
      void setBrowserDownloadPreferences({ askWhereToSave: settings.askWhereToSave }).catch(onError);
    };
    push(loadBrowserSettings());
    const sync = (event: Event) => {
      const detail = (event as CustomEvent<BrowserSettingsState>).detail;
      push(detail ? normalizeBrowserSettings(detail) : loadBrowserSettings());
    };
    window.addEventListener(BROWSER_SETTINGS_EVENT, sync);
    return () => window.removeEventListener(BROWSER_SETTINGS_EVENT, sync);
  }, [onError]);
}

/** Pause (true) or resume (false); resolves false when the download cannot be paused. */
export function pauseBrowserDownload(downloadId: string, pause: boolean): Promise<boolean> {
  return invoke<boolean>("cmd_browser_download_pause", { downloadId, pause });
}

/** Native find results of a Chromium tab, which arrive after `findBrowser` resolves. */
export function onBrowserFindResult(listener: (payload: BrowserFindResultPayload) => void): Promise<UnlistenFn> {
  return listen<BrowserFindResultPayload>(BROWSER_FIND_RESULT_EVENT, (event) => listener(event.payload));
}

export function onBrowserPdfSaved(listener: (payload: BrowserPdfSavedPayload) => void): Promise<UnlistenFn> {
  return listen<BrowserPdfSavedPayload>(BROWSER_PDF_SAVED_EVENT, (event) => listener(event.payload));
}

/** A short reason for a failed download the user can act on. */
export function downloadFailureLabel(failure: string | null | undefined, error: string | null | undefined): string {
  switch (failure) {
    case "diskFull":
      return "Disk full";
    case "accessDenied":
      return "Folder not writable";
    case "network":
      return "Network error";
    case "server":
      return "Server refused";
    default:
      return error || "Failed";
  }
}
