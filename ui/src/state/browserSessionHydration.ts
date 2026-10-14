import { useEffect, useRef } from "react";

import {
  isNavigableBrowserUrl,
  loadBrowserSettings,
} from "../lib/browserSettings";
import { getBrowserScroll, recordBrowserScroll, type BrowserScrollPosition } from "../lib/browserHistory";
import { ensureBrowser } from "../lib/browserTauri";
import { isPrivateBrowserProfileId, readScrollPosition } from "../lib/sessionPersistence";
import type { BrowserPaneState, BrowserTab, LayoutState } from "../lib/types";
import type { WorkspaceState } from "./workspaceStore";

export const BROWSER_SCROLL_RESTORED_EVENT = "ferryx:browser-scroll-restored";

export type { BrowserScrollPosition };

export type BrowserRestoreTarget = {
  browserId: string;
  url: string;
  profileId?: string;
  zoomFactor?: number;
  worktreePath?: string;
  scrollPosition?: BrowserScrollPosition;
  scrollX?: number;
  scrollY?: number;
};

// Double-restore guard: global in-flight tracking prevents concurrent or re-entrant
// hydration runs from spawning duplicate webviews for the same session.
const inFlightHydrations = new Map<string, Promise<void>>();

function addTarget(
  targets: Map<string, BrowserRestoreTarget>,
  target: BrowserRestoreTarget,
  knownWorktreePaths?: Set<string>,
) {
  if (!target.browserId || targets.has(target.browserId)) return;

  // Private-profile tabs must be excluded from restore (privacy policy)
  if (isPrivateBrowserProfileId(target.profileId)) return;

  // Stale state: skip invalid or unnavigable URLs without breaking the rest of restore
  if (!isNavigableBrowserUrl(target.url)) return;

  // Stale state: skip tabs whose worktree no longer exists on disk
  if (
    target.worktreePath &&
    knownWorktreePaths &&
    knownWorktreePaths.size > 0 &&
    !knownWorktreePaths.has(target.worktreePath)
  ) {
    return;
  }

  targets.set(target.browserId, target);
}

function targetFromTab(tab: BrowserTab, defaultWorktreePath?: string): BrowserRestoreTarget {
  const scrollPosition: BrowserScrollPosition | undefined =
    readScrollPosition(tab) ?? getBrowserScroll(tab.browserId, tab.url) ?? undefined;

  return {
    browserId: tab.browserId,
    url: tab.url || "about:blank",
    profileId: tab.profileId,
    zoomFactor: tab.zoomFactor,
    worktreePath: tab.worktreePath || defaultWorktreePath,
    scrollPosition,
    scrollX: scrollPosition?.x,
    scrollY: scrollPosition?.y,
  };
}

function targetFromPane(browser: BrowserPaneState, defaultWorktreePath?: string): BrowserRestoreTarget {
  const scrollPosition: BrowserScrollPosition | undefined =
    readScrollPosition(browser) ?? getBrowserScroll(browser.browserId, browser.url) ?? undefined;

  return {
    browserId: browser.browserId,
    url: browser.url || "about:blank",
    profileId: browser.profileId,
    zoomFactor: browser.zoomFactor,
    worktreePath: browser.worktreePath || defaultWorktreePath,
    scrollPosition,
    scrollX: scrollPosition?.x,
    scrollY: scrollPosition?.y,
  };
}

function collectLayoutBrowsers(
  layout: LayoutState,
  targets: Map<string, BrowserRestoreTarget>,
  defaultWorktreePath?: string,
  knownWorktreePaths?: Set<string>,
) {
  for (const tab of layout.tabs) {
    if (tab.kind === "browser") {
      addTarget(targets, targetFromTab(tab, defaultWorktreePath), knownWorktreePaths);
    }
  }
  for (const tabLayout of Object.values(layout.layoutsByTabId)) {
    for (const content of Object.values(tabLayout.contentsByLeafId ?? {})) {
      if (content.kind === "browser" && content.browser) {
        addTarget(targets, targetFromPane(content.browser, defaultWorktreePath), knownWorktreePaths);
      }
    }
  }
}

export function collectBrowserRestoreTargets(
  state: WorkspaceState,
  filterWorktreePath?: string | null,
): BrowserRestoreTarget[] {
  const targets = new Map<string, BrowserRestoreTarget>();
  const knownWorktreePaths = new Set((state.worktrees ?? []).map((wt) => wt.path));

  if (filterWorktreePath !== undefined) {
    const activePath = state.activeWorktreePath ?? null;
    const requestedPath = filterWorktreePath ?? null;

    if (requestedPath === activePath) {
      collectLayoutBrowsers(state.layout, targets, state.activeWorktreePath ?? undefined, knownWorktreePaths);
    } else if (filterWorktreePath && state.worktreeLayouts?.[filterWorktreePath]) {
      collectLayoutBrowsers(state.worktreeLayouts[filterWorktreePath], targets, filterWorktreePath, knownWorktreePaths);
    }

    return [...targets.values()].filter((target) => {
      const targetPath = target.worktreePath ?? null;
      return targetPath === requestedPath;
    });
  }

  // Restore across layouts, keeping worktree partitioning intact
  collectLayoutBrowsers(state.layout, targets, state.activeWorktreePath ?? undefined, knownWorktreePaths);
  for (const [wtPath, layout] of Object.entries(state.worktreeLayouts ?? {})) {
    if (layout) {
      collectLayoutBrowsers(layout, targets, wtPath, knownWorktreePaths);
    }
  }
  return [...targets.values()];
}

export async function hydrateRestoredBrowserSessions(
  state: WorkspaceState,
  workspaceId: string,
  hydratedKeys: Set<string> = new Set<string>(),
  worktreePath?: string | null,
): Promise<void> {
  const settings = loadBrowserSettings();
  if (!settings.restoreTabsOnLaunch) return;

  const targets = collectBrowserRestoreTargets(state, worktreePath);
  const tasks: Promise<void>[] = [];

  for (const target of targets) {
    // Private-profile tabs must be excluded from restore (privacy policy)
    if (isPrivateBrowserProfileId(target.profileId)) continue;

    const key = `${workspaceId}:${target.worktreePath ?? "default"}:${target.browserId}`;

    // Double-restore guard: check if already hydrated in this session
    if (hydratedKeys.has(key)) continue;

    // Check if another concurrent hydration is already executing for this tab
    const existingPromise = inFlightHydrations.get(key);
    if (existingPromise) {
      tasks.push(existingPromise);
      continue;
    }

    hydratedKeys.add(key);

    const task = ensureBrowser({
      browserId: target.browserId,
      workspaceId,
      worktreePath: target.worktreePath,
      url: target.url,
      profile: target.profileId,
      zoomFactor: target.zoomFactor,
      visible: false,
    })
      .then(() => {
        // Restore scroll position if previously recorded or present in history
        const effectiveScroll = target.scrollPosition ?? (
          typeof target.scrollX === "number" || typeof target.scrollY === "number"
            ? { x: target.scrollX ?? 0, y: target.scrollY ?? 0 }
            : getBrowserScroll(target.browserId, target.url)
        );
        if (effectiveScroll) {
          recordBrowserScroll(target.browserId, target.url, effectiveScroll);
          if (typeof window !== "undefined") {
            window.dispatchEvent(
              new CustomEvent(BROWSER_SCROLL_RESTORED_EVENT, {
                detail: {
                  browserId: target.browserId,
                  url: target.url,
                  scrollPosition: effectiveScroll,
                  scrollX: effectiveScroll.x,
                  scrollY: effectiveScroll.y,
                },
              }),
            );
          }
        }
      })
      .catch((error: unknown) => {
        hydratedKeys.delete(key);
        throw error;
      })
      .finally(() => {
        inFlightHydrations.delete(key);
      });

    inFlightHydrations.set(key, task);
    tasks.push(task);
  }

  await Promise.all(tasks);
}

export function useBrowserSessionHydration(
  state: WorkspaceState,
  workspaceId: string,
  activeWorktreePath?: string | null,
) {
  const hydratedKeysRef = useRef(new Set<string>());

  useEffect(() => {
    let disposed = false;
    void hydrateRestoredBrowserSessions(
      state,
      workspaceId,
      hydratedKeysRef.current,
      activeWorktreePath ?? state.activeWorktreePath,
    ).catch((error: unknown) => {
      if (!disposed) console.warn("Browser session restore failed:", error);
    });
    return () => {
      disposed = true;
    };
  }, [state.layout, state.worktreeLayouts, state.worktrees, workspaceId, activeWorktreePath, state.activeWorktreePath]);
}
