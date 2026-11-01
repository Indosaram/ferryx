import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Emitted by the backend whenever the pending approval list changes. */
export const BROWSER_AGENT_APPROVALS_EVENT = "browser_agent_approvals_changed";

export type GrantScope = "read" | "interact" | "eval";

/** One pending consent request (backend `PendingApproval`, kind flattened in). */
export type BrowserAgentApproval =
  | { id: string; kind: "pairController"; controller: string; label: string; expiresInMs: number }
  | { id: string; kind: "originGrant"; controller: string; origin: string; scopes: GrantScope[]; expiresInMs: number }
  | { id: string; kind: "fileCapability"; controller: string; tool: string; expiresInMs: number }
  | { id: string; kind: "nativeCapability"; controller: string; tool: string; expiresInMs: number };

export type BrowserAgentGrant = { origin: string; scopes: GrantScope[]; persistent: boolean };

export type BrowserAgentController = {
  id: string;
  label: string;
  revoked: boolean;
  grants: BrowserAgentGrant[];
};

export type BrowserAgentSettings = { requirePairing: boolean };

export type BrowserAgentOverview = {
  settings: BrowserAgentSettings;
  pending: BrowserAgentApproval[];
  controllers: BrowserAgentController[];
};

export type ApprovalResolution = "approveOnce" | "approvePersistent" | "deny";

export function getBrowserAgentOverview(): Promise<BrowserAgentOverview> {
  return invoke<BrowserAgentOverview>("cmd_browser_agent_overview");
}

export function resolveBrowserAgentApproval(approvalId: string, resolution: ApprovalResolution): Promise<void> {
  return invoke("cmd_browser_agent_resolve", { approvalId, resolution });
}

/** Revokes one origin grant, or the whole controller when `origin` is omitted. */
export function revokeBrowserAgent(controllerId: string, origin?: string): Promise<boolean> {
  return invoke<boolean>("cmd_browser_agent_revoke", { controllerId, origin: origin ?? null });
}

export function setBrowserAgentSettings(settings: BrowserAgentSettings): Promise<void> {
  return invoke("cmd_browser_agent_set_settings", { settings });
}

export function onBrowserAgentApprovals(listener: (pending: BrowserAgentApproval[]) => void): Promise<UnlistenFn> {
  return listen<BrowserAgentApproval[]>(BROWSER_AGENT_APPROVALS_EVENT, (event) => listener(event.payload));
}

/** Short label of the controller an approval belongs to, from the known controllers. */
export function controllerName(controllers: BrowserAgentController[], id: string): string {
  return controllers.find((controller) => controller.id === id)?.label ?? `controller ${id.slice(0, 8)}`;
}

const SCOPE_TEXT: Record<GrantScope, string> = {
  read: "read pages",
  interact: "click and type",
  eval: "run scripts",
};

export function describeScopes(scopes: GrantScope[]): string {
  return scopes.map((scope) => SCOPE_TEXT[scope]).join(", ");
}
