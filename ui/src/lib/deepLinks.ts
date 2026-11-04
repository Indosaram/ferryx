import type { SectionId } from "../components/settings/types";
import { ONBOARDING_STEP_ORDER, type OnboardingStepId } from "./onboarding";

export type DeepLinkRoute =
  | { kind: "settings"; section: SectionId }
  | { kind: "onboarding"; step: OnboardingStepId }
  | { kind: "workspace"; workspaceId: string };

const sections: readonly string[] = [
  "general", "appearance", "terminal", "shortcuts", "agents", "browser",
  "notifications", "remote", "ssh", "permissions", "pane-access",
];

export function parseDeepLink(raw: string): DeepLinkRoute | null {
  try {
    const url = new URL(raw);
    if (raw.length > 2048 || url.protocol !== "ferryx:" || url.username || url.password || url.port || url.hash) return null;
    const path = decodeURIComponent(url.pathname).replace(/^\//, "");
    if (url.hostname === "settings" && !url.search && sections.includes(path)) {
      return { kind: "settings", section: path as SectionId };
    }
    if (url.hostname === "onboarding" && !url.search && ONBOARDING_STEP_ORDER.includes(path as OnboardingStepId)) {
      return { kind: "onboarding", step: path as OnboardingStepId };
    }
    if (url.hostname === "open" && !path && [...url.searchParams.keys()].length === 1) {
      const workspaceId = url.searchParams.get("workspace");
      if (workspaceId?.trim()) return { kind: "workspace", workspaceId };
    }
  } catch {
    // External URLs are untrusted input, including invalid percent escapes.
  }
  return null;
}

export interface DeepLinkActions {
  hasWorkspace(id: string): boolean;
  confirm(route: DeepLinkRoute): Promise<boolean>;
  settings(section: SectionId): void;
  onboarding(step: OnboardingStepId): void;
  workspace(id: string): void;
  toast(message: string): void;
}

export async function routeDeepLink(raw: string, actions: DeepLinkActions): Promise<void> {
  const route = parseDeepLink(raw);
  if (!route) { actions.toast("Unknown Ferryx link"); return; }
  if (route.kind === "workspace" && !actions.hasWorkspace(route.workspaceId)) {
    actions.toast("Unknown workspace");
    return;
  }
  // Even navigation from an external application requires an explicit in-app action.
  if (!await actions.confirm(route)) return;
  if (route.kind === "settings") actions.settings(route.section);
  if (route.kind === "onboarding") actions.onboarding(route.step);
  if (route.kind === "workspace") {
    if (actions.hasWorkspace(route.workspaceId)) actions.workspace(route.workspaceId);
    else actions.toast("Unknown workspace");
  }
}
