import { invoke } from "@tauri-apps/api/core";
import type {
  BrowserSessionSummary,
  BrowserState,
  CreateBrowserRequest,
  LogicalRect,
} from "./types";

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

export async function createBrowser(request: CreateBrowserRequest): Promise<BrowserState> {
  return invoke<BrowserState>("cmd_browser_create", { request });
}

export async function navigateBrowser(browserId: string, url: string): Promise<void> {
  return invoke<void>("cmd_browser_navigate", { browserId, url });
}

export async function reloadBrowser(browserId: string): Promise<void> {
  return invoke<void>("cmd_browser_reload", { browserId });
}

export async function setBrowserBounds(browserId: string, bounds: LogicalRect): Promise<void> {
  return invoke<void>("cmd_browser_set_bounds", { browserId, bounds });
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
    request: { browserId: profileId, filePath },
  });
  return result.importedCount;
}

export async function openExternalUrl(url: string): Promise<void> {
  return invoke<void>("cmd_browser_open_external", { url });
}