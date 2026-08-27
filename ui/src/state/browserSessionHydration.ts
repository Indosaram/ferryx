import { useEffect, useRef } from "react";

import { loadBrowserSettings } from "../lib/browserSettings";
import { ensureBrowser } from "../lib/browserTauri";
import type { BrowserPaneState, BrowserTab, LayoutState } from "../lib/types";
import type { WorkspaceState } from "./workspaceStore";

export type BrowserRestoreTarget = {
  browserId: string;
  url: string;
  profileId?: string;
  zoomFactor?: number;
  worktreePath?: string;
};

function addTarget(targets: Map<string, BrowserRestoreTarget>, target: BrowserRestoreTarget) {
  if (!target.browserId || targets.has(target.browserId)) return;
  targets.set(target.browserId, target);
}

function targetFromTab(tab: BrowserTab): BrowserRestoreTarget {
  return {
    browserId: tab.browserId,
    url: tab.url || "about:blank",
    profileId: tab.profileId,
    zoomFactor: tab.zoomFactor,
    worktreePath: tab.worktreePath,
  };
}

function targetFromPane(browser: BrowserPaneState): BrowserRestoreTarget {
  return {
    browserId: browser.browserId,
    url: browser.url || "about:blank",
    profileId: browser.profileId,
    zoomFactor: browser.zoomFactor,
    worktreePath: browser.worktreePath,
  };
}

function collectLayoutBrowsers(layout: LayoutState, targets: Map<string, BrowserRestoreTarget>) {
  for (const tab of layout.tabs) {
    if (tab.kind === "browser") addTarget(targets, targetFromTab(tab));
  }
  for (const tabLayout of Object.values(layout.layoutsByTabId)) {
    for (const content of Object.values(tabLayout.contentsByLeafId ?? {})) {
      if (content.kind === "browser" && content.browser) addTarget(targets, targetFromPane(content.browser));
    }
  }
}

export function collectBrowserRestoreTargets(state: WorkspaceState): BrowserRestoreTarget[] {
  const targets = new Map<string, BrowserRestoreTarget>();
  collectLayoutBrowsers(state.layout, targets);
  for (const layout of Object.values(state.worktreeLayouts ?? {})) {
    collectLayoutBrowsers(layout, targets);
  }
  return [...targets.values()];
}

export async function hydrateRestoredBrowserSessions(
  state: WorkspaceState,
  workspaceId: string,
  hydratedKeys: Set<string> = new Set<string>(),
): Promise<void> {
  const settings = loadBrowserSettings();
  if (!settings.restoreTabsOnLaunch) return;

  const tasks: Promise<void>[] = [];
  for (const target of collectBrowserRestoreTargets(state)) {
    const key = `${workspaceId}:${target.browserId}`;
    if (hydratedKeys.has(key)) continue;
    hydratedKeys.add(key);
    tasks.push(
      ensureBrowser({
        browserId: target.browserId,
        workspaceId,
        worktreePath: target.worktreePath,
        url: target.url,
        profile: target.profileId,
        zoomFactor: target.zoomFactor,
        visible: false,
      })
        .then(() => undefined)
        .catch((error: unknown) => {
          hydratedKeys.delete(key);
          throw error;
        }),
    );
  }
  await Promise.all(tasks);
}

export function useBrowserSessionHydration(state: WorkspaceState, workspaceId: string) {
  const hydratedKeysRef = useRef(new Set<string>());

  useEffect(() => {
    let disposed = false;
    void hydrateRestoredBrowserSessions(state, workspaceId, hydratedKeysRef.current).catch((error: unknown) => {
      if (!disposed) console.warn("Browser session restore failed:", error);
    });
    return () => {
      disposed = true;
    };
  }, [state.layout, state.worktreeLayouts, workspaceId]);
}
