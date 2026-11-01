import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** What a site asks to use; mirrors the host's `PermissionKind` names. */
export type SitePermissionKind =
  | "camera"
  | "microphone"
  | "screen"
  | "notifications"
  | "geolocation"
  | "clipboard"
  | "other";

export type SitePermissionDecision = "allow" | "block";

/** A request waiting for the user's answer in a browser pane. */
export interface BrowserPermissionPrompt {
  requestId: number;
  browserId: string;
  profile: string;
  origin: string;
  kinds: SitePermissionKind[];
  canRemember: boolean;
}

/** A remembered per-site decision (Settings > Browser > Site settings). */
export interface SitePermission {
  profile: string;
  origin: string;
  kind: SitePermissionKind;
  decision: SitePermissionDecision;
}

/** What a tab is capturing right now, for the in-use indicator. */
export interface BrowserMediaCapture {
  browserId: string;
  origin: string | null;
  camera: boolean;
  microphone: boolean;
  screen: boolean;
}

export const BROWSER_PERMISSION_PROMPT_EVENT = "browser_permission_prompt";
export const BROWSER_PERMISSION_PROMPT_CLOSED_EVENT = "browser_permission_prompt_closed";
export const BROWSER_MEDIA_CAPTURE_EVENT = "browser_media_capture_changed";
export const BROWSER_SITE_PERMISSIONS_CHANGED_EVENT = "browser_site_permissions_changed";

/** The structured IPC error code ({ code, message, details }) of a failed call. */
export function sitePermissionErrorCode(error: unknown): string {
  if (error && typeof error === "object") {
    const { code, message } = error as { code?: unknown; message?: unknown };
    if (typeof code === "string" && code.trim()) return code.trim();
    if (typeof message === "string" && message.trim()) return message.trim();
  }
  if (typeof error === "string" && error.trim()) return error.trim();
  return "UNKNOWN_ERROR";
}

export const SITE_PERMISSION_LABELS: Record<SitePermissionKind, string> = {
  camera: "Camera",
  microphone: "Microphone",
  screen: "Screen sharing",
  notifications: "Notifications",
  geolocation: "Location",
  clipboard: "Clipboard",
  other: "Other",
};

/** "Camera and microphone", "Location", "Camera, microphone and screen sharing". */
export function describeSitePermissionKinds(kinds: SitePermissionKind[]): string {
  const labels = kinds.map((kind, index) => {
    const label = SITE_PERMISSION_LABELS[kind];
    return index === 0 ? label : label.toLowerCase();
  });
  if (labels.length <= 1) return labels[0] ?? "";
  return `${labels.slice(0, -1).join(", ")} and ${labels[labels.length - 1]}`;
}

/** Answers a prompt; resolves false when the request is no longer pending. */
export function decideBrowserPermission(requestId: number, allow: boolean, remember: boolean): Promise<boolean> {
  return invoke<boolean>("cmd_browser_permission_decide", { requestId, allow, remember });
}

export function getPendingBrowserPermissions(browserId: string): Promise<BrowserPermissionPrompt[]> {
  return invoke<BrowserPermissionPrompt[]>("cmd_browser_permission_pending", { browserId });
}

export function getBrowserMediaCapture(browserId: string): Promise<BrowserMediaCapture> {
  return invoke<BrowserMediaCapture>("cmd_browser_media_capture", { browserId });
}

export function listSitePermissions(): Promise<SitePermission[]> {
  return invoke<SitePermission[]>("cmd_browser_site_permissions_list");
}

/** Forgets the site's decision for `kind`, or every decision for the site when omitted. */
export function revokeSitePermission(profile: string, origin: string, kind?: SitePermissionKind): Promise<boolean> {
  return invoke<boolean>("cmd_browser_site_permission_revoke", { profile, origin, kind: kind ?? null });
}

export function onBrowserPermissionPrompt(handler: (prompt: BrowserPermissionPrompt) => void): Promise<UnlistenFn> {
  return listen<BrowserPermissionPrompt>(BROWSER_PERMISSION_PROMPT_EVENT, (event) => handler(event.payload));
}

export function onBrowserPermissionPromptClosed(
  handler: (closed: { browserId: string; requestId: number }) => void,
): Promise<UnlistenFn> {
  return listen<{ browserId: string; requestId: number }>(BROWSER_PERMISSION_PROMPT_CLOSED_EVENT, (event) =>
    handler(event.payload),
  );
}

export function onBrowserMediaCapture(handler: (capture: BrowserMediaCapture) => void): Promise<UnlistenFn> {
  return listen<BrowserMediaCapture>(BROWSER_MEDIA_CAPTURE_EVENT, (event) => handler(event.payload));
}

export function onSitePermissionsChanged(handler: () => void): Promise<UnlistenFn> {
  return listen(BROWSER_SITE_PERMISSIONS_CHANGED_EVENT, () => handler());
}
