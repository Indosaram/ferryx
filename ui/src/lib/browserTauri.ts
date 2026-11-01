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

export const BROWSER_SESSION_CREATED_EVENT = "browser_session_created";
export const BROWSER_OPEN_REQUESTED_EVENT = "browser_open_requested";
export const BROWSER_DOWNLOAD_REQUESTED_EVENT = "browser_download_requested";
export const BROWSER_DOWNLOAD_UPDATED_EVENT = "browser_download_updated";
export const BROWSER_SHORTCUT_REQUESTED_EVENT = "browser_shortcut_requested";
export const BROWSER_ELEMENT_PICKED_EVENT = "browser_element_picked";
export const BROWSER_LINK_CLICKED_EVENT = "browser_link_clicked";
export const BROWSER_SHORTCUT_EVENT = "ferryx:browser-shortcut";

export type BrowserSessionCreatedPayload = {
  browser: {
    browserId: string;
    webviewLabel: string;
    workspaceId?: string | null;
    worktreePath?: string | null;
    profileId: string;
    generation: number;
    url: string;
    title?: string | null;
    loading: boolean;
    canGoBack: boolean;
    canGoForward: boolean;
    zoomFactor: number;
    loadError?: string | null;
    visible: boolean;
    engine?: import("./types").BrowserEngine;
  };
  workspaceId?: string | null;
};

export type BrowserOpenRequestedPayload = {
  browserId: string;
  targetUrl: string;
  profileId: string;
  worktreePath?: string | null;
};

export type BrowserDownloadRequestedPayload = {
  browserId: string;
  targetUrl: string;
  /** The host could not attribute the request to the user: nothing is fetched until the user
   * confirms with Save. Absent means a user-initiated download. */
  requiresConfirmation?: boolean;
};

export type DownloadStatus = "pending" | "inProgress" | "paused" | "completed" | "failed" | "cancelled";
export type DownloadFailureKind = "diskFull" | "accessDenied" | "network" | "server" | "other";

export interface DownloadRecord {
  id: string;
  url: string;
  filePath: string;
  status: DownloadStatus;
  totalBytes?: number | null;
  receivedBytes: number;
  error?: string | null;
  createdAtMs: number;
  updatedAtMs: number;
  /** False for a system-webview download, which the engine gives no way to stop. */
  canCancel?: boolean;
  canPause?: boolean;
  failure?: DownloadFailureKind | null;
}

export type BrowserShortcutAction =
  | "focus-address"
  | "bookmark-page"
  | "reload"
  | "reload-hard"
  | "back"
  | "forward"
  | "find"
  | "tab-next"
  | "tab-previous"
  | "tab-select-1"
  | "tab-select-2"
  | "tab-select-3"
  | "tab-select-4"
  | "tab-select-5"
  | "tab-select-6"
  | "tab-select-7"
  | "tab-select-8"
  | "tab-select-9"
  | "tab-new-terminal"
  | "tab-close"
  | "command-palette"
  | "sidebar-toggle"
  | "settings-toggle"
  | "split-right"
  | "split-down"
  | `workspace-select-${1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9}`;

export const BROWSER_TAB_SHORTCUT_EVENT = "ferryx:browser-tab-shortcut";

export type BrowserTabShortcutDetail =
  | { action: "tab-next" }
  | { action: "tab-previous" }
  | { action: `tab-select-${1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9}` };

export function isBrowserTabShortcutAction(action: string): action is BrowserTabShortcutDetail["action"] {
  return (
    action === "tab-next" ||
    action === "tab-previous" ||
    (action.startsWith("tab-select-") &&
      (() => {
        const digit = Number(action.slice("tab-select-".length));
        return Number.isInteger(digit) && digit >= 1 && digit <= 9;
      })())
  );
}

export function browserTabSelectIndex(action: string): number | null {
  if (!action.startsWith("tab-select-")) return null;
  const digit = Number(action.slice("tab-select-".length));
  return Number.isInteger(digit) && digit >= 1 && digit <= 9 ? digit - 1 : null;
}

export function browserWorkspaceSelectIndex(action: string): number | null {
  if (!action.startsWith("workspace-select-")) return null;
  const digit = Number(action.slice("workspace-select-".length));
  return Number.isInteger(digit) && digit >= 1 && digit <= 9 ? digit - 1 : null;
}

export type BrowserShortcutRequestedPayload = {
  browserId: string;
  action: BrowserShortcutAction;
};

export type BrowserElementPickedPayload = {
  browserId: string;
};

export type BrowserLinkClickedPayload = {
  browserId: string;
  targetUrl: string;
  modifier: boolean;
  profileId?: string;
  worktreePath?: string;
};

/** App-webview shortcuts always address one native browser, never broadcast an action. */
export type BrowserShortcutDomEvent = CustomEvent<BrowserShortcutRequestedPayload>;

export type BrowserFindResult = {
  matchCount: number;
  found: boolean;
};

export type BrowserSnapshotCapability = {
  supported: boolean;
  formats: string[];
  /** Element picking: native snapshots AND this tab's isolated-world bridge. */
  pick: boolean;
};
export type BrowserDesignDomElement = { id: string; tag: string; bounds: [number, number, number, number]; text?: string | null };
export type BrowserDesignSnapshot = {
  session_id: string;
  timestamp_ms: number;
  screenshot_png_base64: string;
  outer_html?: string;
  css?: string;
  dom_elements: BrowserDesignDomElement[];
};
export type DesignFeedbackDelivery = { pngPath: string; prompt: string; bytesWritten: number };
export type DesignFeedbackTarget = { sessionId: string; label: string; workspaceId: string };
export async function deliverDesignFeedback(request: { sessionId: string; memo: string; snapshot: BrowserDesignSnapshot; workspaceId?: string }): Promise<DesignFeedbackDelivery> {
  return invoke<DesignFeedbackDelivery>("cmd_design_feedback_deliver", request);
}

const browserLifecycleQueues = new Map<string, Promise<void>>();

function enqueueBrowserLifecycle(browserId: string, operation: () => Promise<void>): Promise<void> {
  const previous = browserLifecycleQueues.get(browserId) ?? Promise.resolve();
  // `previous` is awaited only for ordering, never to inspect its outcome: its rejection was
  // already delivered to the caller that enqueued that step (setBrowserVisible's only caller is
  // BrowserPane's updateVisibility, which reports it as a display error, and every closeBrowser
  // caller awaits the returned promise or collects it with Promise.allSettled). Dropping it here
  // therefore cannot hide a failure from anyone, while a failed step must not stall the queue.
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
  // The backend serializes IpcErrorCode as SCREAMING_SNAKE_CASE
  // (src-tauri/src/ipc/error.rs:7), so the structured code is authoritative.
  // Matching on message prose would silently stop working the moment the
  // wording changes, which this project's IPC contract forbids.
  if (!err) return false;
  if (typeof err === "object") {
    return (err as Record<string, unknown>).code === "BROWSER_NOT_FOUND";
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

/** `ignoreCache` asks the platform engine to revalidate instead of serving the HTTP cache
 * (macOS `reloadFromOrigin`, Linux `reload_bypass_cache`; the backend refuses it elsewhere). */
export type BrowserReloadOptions = {
  ignoreCache?: boolean;
};

export async function reloadBrowser(browserId: string, options?: BrowserReloadOptions): Promise<void> {
  return invoke<void>("cmd_browser_reload", { browserId, ignoreCache: options?.ignoreCache ?? false });
}

function isWebviewNotFoundError(err: unknown): boolean {
  // Structured code only, for the same reason as isBrowserNotFoundError.
  if (!err) return false;
  if (typeof err === "object") {
    return (err as Record<string, unknown>).code === "WEBVIEW_NOT_FOUND";
  }
  return false;
}

export type BrowserDisplayOperation = "bounds" | "visibility";

export type BrowserDisplayError = {
  operation: BrowserDisplayOperation;
  code: string;
};

export type BrowserCookieImportFailureReason =
  | "no-profile"
  | "no-tab"
  | "keychain-denied"
  | "no-cookies"
  | "unsupported-platform"
  | "cookie-import-failed";

export type BrowserCookieImportErrorPayload = {
  code?: string;
  message?: string;
  details?: {
    reason?: BrowserCookieImportFailureReason | string;
    [key: string]: unknown;
  } | null;
};

export function extractBrowserCookieImportReason(err: unknown): BrowserCookieImportFailureReason | string | null {
  if (!err || typeof err !== "object") return null;
  const details = (err as Record<string, unknown>).details;
  if (details && typeof details === "object" && details !== null) {
    const reason = (details as Record<string, unknown>).reason;
    if (typeof reason === "string" && reason.trim()) {
      return reason.trim();
    }
  }
  return null;
}

export function getBrowserCookieImportActionableMessage(
  reason: BrowserCookieImportFailureReason | string,
): string | null {
  switch (reason) {
    case "no-tab":
      return "Open a browser tab using this profile before importing cookies.";
    case "keychain-denied":
      return "Allow Keychain access in the macOS prompt to import cookies.";
    case "no-profile":
      return "Browser profile directory or cookie database was not found.";
    case "no-cookies":
      return "No importable cookies found in the selected profile.";
    case "unsupported-platform":
      return "Installed browser cookie import is not supported on this platform.";
    case "cookie-import-failed":
      return "Failed to import cookies from browser profile.";
    default:
      return null;
  }
}

export function formatBrowserCookieImportError(err: unknown): string {
  if (!err) return "UNKNOWN_ERROR";

  if (typeof err === "object" && err !== null) {
    const errObj = err as Record<string, unknown>;
    const reason = extractBrowserCookieImportReason(err);

    if (reason) {
      const actionable = getBrowserCookieImportActionableMessage(reason);
      return actionable ? `${reason}: ${actionable}` : reason;
    }

    const code = typeof errObj.code === "string" ? errObj.code.trim() : undefined;
    if (code) {
      return code;
    }

    if (err instanceof Error && err.message.trim()) {
      return err.message.trim();
    }

    const message = typeof errObj.message === "string" ? errObj.message.trim() : undefined;
    if (message) {
      return message;
    }
  }

  if (typeof err === "string" && err.trim()) {
    return err.trim();
  }

  return "UNKNOWN_ERROR";
}

export function extractBrowserErrorCode(err: unknown): string {
  if (!err) return "UNKNOWN_ERROR";
  if (typeof err === "object") {
    const code = (err as Record<string, unknown>).code;
    if (typeof code === "string" && code.trim()) return code.trim();
    if (err instanceof Error && err.message) return err.message.trim();
    const message = (err as Record<string, unknown>).message;
    if (typeof message === "string" && message.trim()) return message.trim();
  }
  if (typeof err === "string" && err.trim()) {
    return err.trim();
  }
  return "UNKNOWN_ERROR";
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

export async function openBrowserDevtools(browserId: string): Promise<void> {
  return invoke<void>("cmd_browser_open_devtools", { browserId });
}

export async function injectBrowserElementPicker(browserId: string): Promise<void> {
  return invoke<void>("cmd_browser_inject_element_picker", { browserId });
}

export async function removeBrowserElementPicker(browserId: string): Promise<void> {
  return invoke<void>("cmd_browser_remove_element_picker", { browserId });
}

export async function finishBrowserElementPick(browserId: string): Promise<BrowserDesignSnapshot> {
  return invoke<BrowserDesignSnapshot>("cmd_browser_finish_element_pick", { browserId });
}

/** Reports whether this build can capture native webview snapshots (macOS today) and which
 * encodings it supports, so platform-limited affordances can be gated instead of offered dead. */
export async function getBrowserSnapshotCapability(browserId?: string): Promise<BrowserSnapshotCapability> {
  return invoke<BrowserSnapshotCapability>("cmd_browser_snapshot_capability", { browserId: browserId ?? null });
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

/** The on-demand Chromium engine: "unsupported" on platforms without a build. */
export type BrowserEngineStatus =
  | { state: "unsupported" }
  | { state: "notInstalled" }
  | { state: "installed"; version: string; previous?: string | null };

export async function getBrowserEngineStatus(): Promise<BrowserEngineStatus> {
  return invoke<BrowserEngineStatus>("cmd_browser_engine_status");
}

export async function installBrowserEngine(): Promise<BrowserEngineStatus> {
  return invoke<BrowserEngineStatus>("cmd_browser_engine_install");
}

export async function removeBrowserEngine(): Promise<BrowserEngineStatus> {
  return invoke<BrowserEngineStatus>("cmd_browser_engine_remove");
}

/** Makes the previously installed engine active again. */
export async function rollbackBrowserEngine(): Promise<BrowserEngineStatus> {
  return invoke<BrowserEngineStatus>("cmd_browser_engine_rollback");
}

/**
 * Why an engine install failed, from the structured IPC error: `details.reason` of a
 * BROWSER_ENGINE_INSTALL_FAILED error (badSignature, unsigned, checksumMismatch, partial,
 * oversized, diskFull, versionMismatch, network, ...), else the error code.
 */
export type BrowserEngineFailure = { code: string; reason: string | null; message: string | null };

export function parseBrowserEngineFailure(err: unknown): BrowserEngineFailure {
  const code = extractBrowserErrorCode(err);
  if (!err || typeof err !== "object") return { code, reason: null, message: null };
  const record = err as Record<string, unknown>;
  const details = record.details;
  const reason =
    details && typeof details === "object" && typeof (details as Record<string, unknown>).reason === "string"
      ? ((details as Record<string, unknown>).reason as string)
      : null;
  const message = typeof record.message === "string" && record.message.trim() ? record.message.trim() : null;
  return { code, reason, message };
}

export async function importBrowserCookies(profileId: string, filePath: string): Promise<number> {
  const result = await invoke<{ importedCount: number }>("cmd_browser_import_cookies", {
    request: { profileId, filePath },
  });
  return result.importedCount;
}

export async function importInstalledBrowserCookies(
  profileId: string,
  source: "chrome" | "edge",
  sourceProfile?: string,
): Promise<{ importedCount: number; skippedCount: number }> {
  return invoke<{ importedCount: number; skippedCount: number }>(
    "cmd_browser_import_installed_cookies",
    {
      request: {
        profileId,
        source,
        sourceProfile,
      },
    },
  );
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

export async function downloadBrowserUrlWithSession(
  browserId: string,
  url: string,
  filePath: string,
): Promise<DownloadRecord> {
  return invoke<DownloadRecord>("cmd_browser_download_with_cookies", {
    request: { url, filePath, browserId },
  });
}

export function onBrowserDownloadUpdated(
  listener: (payload: DownloadRecord) => void,
): Promise<UnlistenFn> {
  return listen<DownloadRecord>(BROWSER_DOWNLOAD_UPDATED_EVENT, (event) => listener(event.payload));
}

export function onBrowserOpenRequested(
  listener: (payload: BrowserOpenRequestedPayload) => void,
): Promise<UnlistenFn> {
  return listen<BrowserOpenRequestedPayload>(BROWSER_OPEN_REQUESTED_EVENT, (event) => listener(event.payload));
}

export function onBrowserSessionCreated(
  listener: (payload: BrowserSessionCreatedPayload) => void,
): Promise<UnlistenFn> {
  return listen<BrowserSessionCreatedPayload>(BROWSER_SESSION_CREATED_EVENT, (event) => listener(event.payload));
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

export function onBrowserElementPicked(
  listener: (payload: BrowserElementPickedPayload) => void,
): Promise<UnlistenFn> {
  return listen<BrowserElementPickedPayload>(BROWSER_ELEMENT_PICKED_EVENT, (event) => listener(event.payload));
}

export function onBrowserLinkClicked(
  listener: (payload: BrowserLinkClickedPayload) => void,
): Promise<UnlistenFn> {
  return listen<BrowserLinkClickedPayload>(BROWSER_LINK_CLICKED_EVENT, (event) => listener(event.payload));
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

export type BrowserOpenerLink = { browserId: string; handle: string };

/** The opener fields a popup-open event carries; absent for a plain open. */
export type BrowserPopupOpenRequestedPayload = BrowserOpenRequestedPayload & {
  openerBrowserId?: string | null;
  popupHandle?: string | null;
};

export const BROWSER_CLOSE_REQUESTED_EVENT = "browser_close_requested";

export type BrowserCloseRequestedPayload = {
  browserId: string;
};

/**
 * The popup link a popup-open event carries, or undefined when the open was not a popup.
 * Without both fields the tab is created as a normal tab, which is what keeps
 * `window.opener` null in a tab a page did not open. */
export function popupOpenerLink(payload: BrowserOpenRequestedPayload): BrowserOpenerLink | undefined {
  const popup = payload as BrowserPopupOpenRequestedPayload;
  const browserId = popup.openerBrowserId ?? null;
  const handle = popup.popupHandle ?? null;
  if (!browserId || !handle) return undefined;
  return { browserId, handle };
}

/**
 * Creates the tab for a page's `window.open`, linked back to the tab that called it, so the
 * OAuth callback can be routed through the host. */
export async function createPopupBrowser(
  request: CreateBrowserRequest,
  opener: BrowserOpenerLink,
): Promise<BrowserState> {
  const settings = loadBrowserSettings();
  const profile = resolveSupportedBrowserProfileId(request.profile ?? settings.defaultProfileId, settings);
  const zoomFactor = request.zoomFactor ?? settings.defaultZoom / 100;
  return invoke<BrowserState>("cmd_browser_create", {
    request: { ...request, profile, zoomFactor },
    opener,
  });
}

/** The id of the browser tab showing `browserId`, so a host close request can close it. */
export function browserTabIdForBrowserId(
  tabs: ReadonlyArray<{ id: string; kind?: string; browserId?: string }>,
  browserId: string,
): string | null {
  for (const tab of tabs) {
    if (tab.kind === "browser" && tab.browserId === browserId) return tab.id;
  }
  return null;
}

/** A popup's own page called `window.close()`; the host closes that tab. */
export function onBrowserPopupCloseRequested(
  listener: (payload: BrowserCloseRequestedPayload) => void,
): Promise<UnlistenFn> {
  return listen<BrowserCloseRequestedPayload>(BROWSER_CLOSE_REQUESTED_EVENT, (event) => listener(event.payload));
}

export type BrowserStateChangedPayload = {
  browserId: string;
  url: string;
  title?: string | null;
  engine?: import("./types").BrowserEngine;
};

/** The host's view of a browser tab changed: url, title, loading or navigation state. */
export function onBrowserStateChanged(
  listener: (payload: BrowserStateChangedPayload) => void,
): Promise<UnlistenFn> {
  return listen<BrowserStateChangedPayload>("browser_state_changed", (event) => listener(event.payload));
}

export const BROWSER_TAB_SWITCH_EVENT = "browser_tab_switch";

export type BrowserTabSwitchPayload = {
  browserId: string;
  index: number;
};

/**
 * Tells the backend whether this window actually put a CLI-opened browser session into a tab.
 * `adopted: false` is what makes `ferryx browser open` report the tab was not shown.
 */
export async function reportBrowserAdoption(
  browserId: string,
  adopted: boolean,
  reason?: string,
): Promise<boolean> {
  return invoke<boolean>("cmd_browser_session_adoption", { browserId, adopted, reason });
}

/**
 * `tab switch` addresses a browser session, but only this window knows which layout tab owns it.
 */
export function onBrowserTabSwitch(
  listener: (payload: BrowserTabSwitchPayload) => void,
): Promise<UnlistenFn> {
  return listen<BrowserTabSwitchPayload>(BROWSER_TAB_SWITCH_EVENT, (event) => listener(event.payload));
}
