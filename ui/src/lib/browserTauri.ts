import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { loadBrowserSettings, resolveSupportedBrowserProfileId } from "./browserSettings";
import type {
  BrowserAutomationRequest,
  BrowserAutomationSnapshot,
  BrowserSessionSummary,
  BrowserState,
  CreateBrowserRequest,
  LogicalRect,
} from "./types";

export const BROWSER_OPEN_REQUESTED_EVENT = "browser_open_requested";
export const BROWSER_DOWNLOAD_REQUESTED_EVENT = "browser_download_requested";
export const BROWSER_SHORTCUT_REQUESTED_EVENT = "browser_shortcut_requested";
export const BROWSER_SHORTCUT_EVENT = "ferryx:browser-shortcut";

export type BrowserOpenRequestedPayload = {
  browserId: string;
  targetUrl: string;
  profileId: string;
  worktreePath?: string | null;
};

export type BrowserDownloadRequestedPayload = {
  browserId: string;
  targetUrl: string;
};

export type BrowserShortcutAction = "focus-address" | "reload" | "back" | "forward" | "find";

export type BrowserShortcutRequestedPayload = {
  browserId: string;
  action: BrowserShortcutAction;
};

export type BrowserFindResult = {
  matchCount: number;
  found: boolean;
};

const browserLifecycleQueues = new Map<string, Promise<void>>();

function enqueueBrowserLifecycle(browserId: string, operation: () => Promise<void>): Promise<void> {
  const previous = browserLifecycleQueues.get(browserId) ?? Promise.resolve();
  const next = previous.catch(() => undefined).then(operation);
  browserLifecycleQueues.set(browserId, next);

  const clearIfCurrent = () => {
    if (browserLifecycleQueues.get(browserId) === next) {
      browserLifecycleQueues.delete(browserId);
    }
  };
  void next.then(clearIfCurrent, clearIfCurrent);

  return next;
}

function isBrowserNotFoundError(err: unknown): boolean {
  if (!err) return false;
  if (typeof err === "object") {
    const obj = err as Record<string, unknown>;
    if (obj.code === "BROWSER_NOT_FOUND") return true;
    if (typeof obj.message === "string" && obj.message.includes("Browser not found")) return true;
  }
  if (typeof err === "string" && (err.includes("BROWSER_NOT_FOUND") || err.includes("Browser not found"))) {
    return true;
  }
  return false;
}

export async function createBrowser(request: CreateBrowserRequest): Promise<BrowserState> {
  const settings = loadBrowserSettings();
  const profile = resolveSupportedBrowserProfileId(request.profile ?? settings.defaultProfileId, settings);
  const zoomFactor = request.zoomFactor ?? settings.defaultZoom / 100;
  return invoke<BrowserState>("cmd_browser_create", {
    request: { ...request, profile, zoomFactor },
  });
}

export async function ensureBrowser(request: CreateBrowserRequest & { browserId: string }): Promise<BrowserState> {
  try {
    return await getBrowserState(request.browserId);
  } catch (error) {
    if (!isBrowserNotFoundError(error)) throw error;
  }
  return createBrowser(request);
}

export async function navigateBrowser(browserId: string, url: string): Promise<void> {
  return invoke<void>("cmd_browser_navigate", { browserId, url });
}

export async function goBackBrowser(browserId: string): Promise<void> {
  return invoke<void>("cmd_browser_go_back", { browserId });
}

export async function goForwardBrowser(browserId: string): Promise<void> {
  return invoke<void>("cmd_browser_go_forward", { browserId });
}

export async function reloadBrowser(browserId: string): Promise<void> {
  return invoke<void>("cmd_browser_reload", { browserId });
}

function isWebviewNotFoundError(err: unknown): boolean {
  if (!err) return false;
  if (typeof err === "object") {
    const obj = err as Record<string, unknown>;
    if (obj.code === "WEBVIEW_NOT_FOUND") return true;
    if (typeof obj.message === "string" && obj.message.includes("Webview not found")) return true;
  }
  if (typeof err === "string" && (err.includes("WEBVIEW_NOT_FOUND") || err.includes("Webview not found"))) {
    return true;
  }
  return false;
}

export async function setBrowserBounds(
  browserId: string,
  bounds: LogicalRect,
  retries = 5,
  delayMs = 50,
): Promise<void> {
  for (let attempt = 0; attempt <= retries; attempt++) {
    try {
      return await invoke<void>("cmd_browser_set_bounds", { browserId, bounds });
    } catch (error) {
      if (attempt < retries && isWebviewNotFoundError(error)) {
        await new Promise((resolve) => setTimeout(resolve, delayMs));
        continue;
      }
      throw error;
    }
  }
}

export function setBrowserVisible(browserId: string, visible: boolean): Promise<void> {
  return enqueueBrowserLifecycle(browserId, () => invoke<void>("cmd_browser_set_visible", { browserId, visible }));
}

export async function setBrowserZoom(browserId: string, zoomFactor: number): Promise<number> {
  return invoke<number>("cmd_browser_set_zoom", { browserId, zoomFactor });
}

export async function focusBrowser(browserId: string): Promise<void> {
  return invoke<void>("cmd_browser_focus", { browserId });
}

export async function getBrowserState(browserId: string): Promise<BrowserState> {
  return invoke<BrowserState>("cmd_browser_get_state", { browserId });
}

export function closeBrowser(browserId: string): Promise<void> {
  return enqueueBrowserLifecycle(browserId, () => invoke<void>("cmd_browser_close", { browserId }));
}

export async function listBrowsers(): Promise<BrowserSessionSummary[]> {
  return invoke<BrowserSessionSummary[]>("cmd_browser_list");
}

export async function importBrowserCookies(profileId: string, filePath: string): Promise<number> {
  const result = await invoke<{ importedCount: number }>("cmd_browser_import_cookies", {
    request: { profileId, filePath },
  });
  return result.importedCount;
}

export async function openExternalUrl(url: string): Promise<void> {
  return invoke<void>("cmd_browser_open_external", { url });
}

export async function findBrowser(
  browserId: string,
  query: string,
  backwards = false,
): Promise<BrowserFindResult> {
  return invoke<BrowserFindResult>("cmd_browser_find", { browserId, query, backwards });
}

export async function clearBrowserFind(browserId: string): Promise<void> {
  return invoke<void>("cmd_browser_clear_find", { browserId });
}

export async function downloadBrowserUrl(url: string, filePath: string): Promise<void> {
  return invoke<void>("cmd_browser_download", { url, filePath });
}

export function onBrowserOpenRequested(
  listener: (payload: BrowserOpenRequestedPayload) => void,
): Promise<UnlistenFn> {
  return listen<BrowserOpenRequestedPayload>(BROWSER_OPEN_REQUESTED_EVENT, (event) => listener(event.payload));
}

export function onBrowserDownloadRequested(
  listener: (payload: BrowserDownloadRequestedPayload) => void,
): Promise<UnlistenFn> {
  return listen<BrowserDownloadRequestedPayload>(BROWSER_DOWNLOAD_REQUESTED_EVENT, (event) => listener(event.payload));
}

export function onBrowserShortcutRequested(
  listener: (payload: BrowserShortcutRequestedPayload) => void,
): Promise<UnlistenFn> {
  return listen<BrowserShortcutRequestedPayload>(BROWSER_SHORTCUT_REQUESTED_EVENT, (event) => listener(event.payload));
}

export async function browserAutomationSnapshot(
  browserId: string,
): Promise<BrowserAutomationSnapshot> {
  return invoke<BrowserAutomationSnapshot>("cmd_browser_automation_snapshot", { browserId });
}

export async function browserAutomationAct(
  request: BrowserAutomationRequest,
): Promise<void> {
  return invoke<void>("cmd_browser_automation_act", { request });
}
