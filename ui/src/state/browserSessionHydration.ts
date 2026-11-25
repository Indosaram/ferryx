import { useEffect, useRef } from "react";

import {
  isNavigableBrowserUrl,
  loadBrowserSettings,
} from "../lib/browserSettings";
import {
  getBrowserAgentOverview,
  onBrowserAgentApprovals,
  type BrowserAgentOverview,
} from "../lib/browserAgent";
import { getBrowserScroll, recordBrowserScroll, type BrowserScrollPosition } from "../lib/browserHistory";
import { browserTabNavigation } from "../lib/browserTabNavigation";
import {
  ensureBrowser,
  extractBrowserErrorCode,
  onBrowserStateChanged,
  type BrowserStateChangedPayload,
} from "../lib/browserTauri";
import { isPrivateBrowserProfileId, readScrollPosition } from "../lib/sessionPersistence";
import type { BrowserPaneState, BrowserState, BrowserTab, LayoutState } from "../lib/types";
import {
  browserStore,
  runBrowserStoreMigrationOnce,
  selectActiveBrowserId,
  selectSpaceList,
  type BrowserStore,
  type BrowserStoreState,
  type BrowserStoreTab,
} from "./browserStore";
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
      .then((browserState) => {
        // The engine starts with an empty history, so the tab's saved back/forward stack
        // takes over; a tab that survived in the host keeps its engine history.
        browserTabNavigation.restore(
          target.browserId,
          target.url,
          Boolean(browserState?.canGoBack || browserState?.canGoForward),
        );
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

/**
 * A global store tab that must exist as a native session after startup. Startup is lazy: one
 * target per Space (its active tab) plus whatever a worktree split currently references, never the
 * whole Today list. Background tabs are created on demand when the user opens them.
 */
export type GlobalBrowserRestoreTarget = {
  browserId: string;
  url: string;
  spaceId: string;
  workspaceId?: string;
  worktreePath?: string;
};

export type CollectGlobalRestoreTargetsOptions = {
  /** Narrows the scan to these Spaces; every Space by default. */
  spaceIds?: readonly string[];
  /** `false` hydrates only active tabs, ignoring worktree split references. */
  includeSplitAttachments?: boolean;
};

function addGlobalTarget(
  targets: Map<string, GlobalBrowserRestoreTarget>,
  tab: BrowserStoreTab | undefined,
) {
  if (!tab) return;
  // Deduplicate by browserId: a tab referenced by both a Space and a worktree split is one session.
  if (targets.has(tab.browserId)) return;
  // Private tabs are never resurrected (privacy policy).
  if (tab.private) return;
  // Stale state: an unnavigable URL has nothing to load, so it must not spawn a session.
  if (!isNavigableBrowserUrl(tab.url)) return;

  const target: GlobalBrowserRestoreTarget = { browserId: tab.browserId, url: tab.url, spaceId: tab.spaceId };
  if (tab.worktreeTag?.workspaceId) target.workspaceId = tab.worktreeTag.workspaceId;
  if (tab.worktreeTag?.worktreePath) target.worktreePath = tab.worktreeTag.worktreePath;
  targets.set(tab.browserId, target);
}

/**
 * The eager startup set: the active tab of each Space plus every tab a worktree split still
 * references, deduplicated by `browserId`. Inactive background tabs are deliberately excluded so a
 * restart with nine tabs across three Spaces spawns three webviews, not nine.
 */
export function collectGlobalBrowserRestoreTargets(
  storeState: BrowserStoreState,
  options: CollectGlobalRestoreTargetsOptions = {},
): GlobalBrowserRestoreTarget[] {
  const targets = new Map<string, GlobalBrowserRestoreTarget>();
  const spaceIds = options.spaceIds ?? selectSpaceList(storeState).map((space) => space.id);

  for (const spaceId of spaceIds) {
    const activeBrowserId = selectActiveBrowserId(storeState, spaceId);
    if (!activeBrowserId) continue;
    addGlobalTarget(targets, storeState.tabs[activeBrowserId]);
  }

  if (options.includeSplitAttachments !== false) {
    for (const browserId of Object.keys(storeState.splitAttachments)) {
      addGlobalTarget(targets, storeState.tabs[browserId]);
    }
  }

  return [...targets.values()];
}

/** Hydration of every eager target already ran in this session (survives React remounts). */
const hydratedGlobalBrowserIds = new Set<string>();

export type HydrateGlobalBrowserStoreOptions = CollectGlobalRestoreTargetsOptions & {
  /** Legacy persisted workspace session, migrated into the store before the first hydration. */
  persistedWorkspaceState?: unknown;
  storage?: Storage | null;
  now?: number;
};

export type GlobalHydrationResult = {
  hydrated: string[];
  failed: Array<{ browserId: string; code: string }>;
  skipped: string[];
};

/**
 * Marks a tab whose backend session could not be created. `ensureBrowser` runs first, so a tab
 * that is already alive in the host keeps its state; a tab whose session is gone shows its reload
 * affordance instead of throwing through the caller.
 */
function markTabHydrationFailure(store: BrowserStore, browserId: string, code: string): void {
  if (!store.getState().tabs[browserId]) return;
  if (store.getState().tabs[browserId]?.hydrationError === code) return;
  store.setState((prev) => {
    const current = prev.tabs[browserId];
    if (!current || current.hydrationError === code) return prev;
    return { ...prev, tabs: { ...prev.tabs, [browserId]: { ...current, hydrationError: code } } };
  });
}

function applyHydratedSession(store: BrowserStore, browserId: string, browserState: BrowserState | null): void {
  const url = browserState?.url?.trim();
  const title = browserState?.title?.trim();
  const engine = browserState?.engine;
  store.setState((prev) => {
    const current = prev.tabs[browserId];
    if (!current) return prev;
    const next: BrowserStoreTab = { ...current };
    if (url) next.url = url;
    if (title) next.title = title;
    if (engine) next.engine = engine;
    delete next.hydrationError;
    const unchanged =
      next.url === current.url &&
      next.title === current.title &&
      next.engine === current.engine &&
      current.hydrationError === undefined;
    return unchanged ? prev : { ...prev, tabs: { ...prev.tabs, [browserId]: next } };
  });
}

/**
 * Startup hydration of the global store: migrates legacy tabs once, then lazily calls
 * `ensureBrowser` for the eager targets. A failing session is recorded on its tab and never
 * aborts the others, and the in-flight map keeps concurrent or re-entrant calls from spawning a
 * second webview for the same `browserId`.
 */
export async function hydrateGlobalBrowserStore(
  store: BrowserStore = browserStore,
  options: HydrateGlobalBrowserStoreOptions = {},
): Promise<GlobalHydrationResult> {
  const result: GlobalHydrationResult = { hydrated: [], failed: [], skipped: [] };

  if (options.persistedWorkspaceState !== undefined && options.persistedWorkspaceState !== null) {
    runBrowserStoreMigrationOnce({
      persistedWorkspaceState: options.persistedWorkspaceState,
      store,
      storage: options.storage,
      now: options.now,
    });
  }

  const settings = loadBrowserSettings();
  if (!settings.restoreTabsOnLaunch) return result;

  const targets = collectGlobalBrowserRestoreTargets(store.getState(), options);
  const tasks: Promise<void>[] = [];

  for (const target of targets) {
    if (hydratedGlobalBrowserIds.has(target.browserId)) {
      result.skipped.push(target.browserId);
      continue;
    }

    const existing = inFlightHydrations.get(target.browserId);
    if (existing) {
      result.skipped.push(target.browserId);
      tasks.push(existing);
      continue;
    }

    const task = ensureBrowser({
      browserId: target.browserId,
      workspaceId: target.workspaceId,
      worktreePath: target.worktreePath,
      url: target.url,
      visible: false,
    })
      .then((browserState) => {
        hydratedGlobalBrowserIds.add(target.browserId);
        applyHydratedSession(store, target.browserId, browserState);
        result.hydrated.push(target.browserId);
      })
      .catch((error: unknown) => {
        // Structured IPC code only: the message wording is not part of the contract.
        const code = extractBrowserErrorCode(error);
        markTabHydrationFailure(store, target.browserId, code);
        result.failed.push({ browserId: target.browserId, code });
      })
      .finally(() => {
        inFlightHydrations.delete(target.browserId);
      });

    inFlightHydrations.set(target.browserId, task);
    tasks.push(task);
  }

  await Promise.all(tasks);
  return result;
}

/** `browser_state_changed` as the store consumes it: the payload plus the fields the toolbar needs. */
export type BrowserLiveStatePayload = BrowserStateChangedPayload & {
  canGoBack?: boolean;
  canGoForward?: boolean;
  faviconUrl?: string | null;
};

function mergeLiveTabState(tab: BrowserStoreTab, payload: BrowserLiveStatePayload): BrowserStoreTab | null {
  const next: BrowserStoreTab = { ...tab };
  const url = payload.url?.trim();
  if (url) next.url = url;
  // WKWebView reports an empty title while a document commits; keep the last real one.
  const title = payload.title?.trim();
  if (title) next.title = title;
  if (payload.engine) next.engine = payload.engine;
  if (typeof payload.loading === "boolean") next.loading = payload.loading;
  if (typeof payload.canGoBack === "boolean") next.canGoBack = payload.canGoBack;
  if (typeof payload.canGoForward === "boolean") next.canGoForward = payload.canGoForward;
  const faviconUrl = payload.faviconUrl?.trim();
  if (faviconUrl) next.faviconUrl = faviconUrl;

  const unchanged =
    next.url === tab.url &&
    next.title === tab.title &&
    next.engine === tab.engine &&
    next.loading === tab.loading &&
    next.canGoBack === tab.canGoBack &&
    next.canGoForward === tab.canGoForward &&
    next.faviconUrl === tab.faviconUrl;
  return unchanged ? null : next;
}

/**
 * Applies one host state change to whichever store tab owns that `browserId`, background or
 * foreground. Returns false for a browser the store does not know, so a CLI-created session can
 * never inject a tab into the sidebar.
 */
export function applyBrowserStateChangedToStore(
  store: BrowserStore,
  payload: BrowserLiveStatePayload,
): boolean {
  if (!payload?.browserId) return false;
  if (!store.getState().tabs[payload.browserId]) return false;
  let applied = false;
  store.setState((prev) => {
    const current = prev.tabs[payload.browserId];
    if (!current) return prev;
    const next = mergeLiveTabState(current, payload);
    if (!next) return prev;
    applied = true;
    return { ...prev, tabs: { ...prev.tabs, [payload.browserId]: next } };
  });
  return applied;
}

function originOf(url: string): string | null {
  try {
    const parsed = new URL(url.trim());
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
    return parsed.origin;
  } catch {
    return null;
  }
}

/**
 * Refreshes `agentControlled` on every tab from the agent authority: a tab is agent controlled
 * while a live (non-revoked) controller holds a grant on its origin. One store commit, no writes
 * when nothing changed, so background tabs keep an honest badge after an approval round-trip.
 */
export function applyAgentOverviewToStore(store: BrowserStore, overview: BrowserAgentOverview | null): number {
  const origins = new Set<string>();
  for (const controller of overview?.controllers ?? []) {
    if (controller.revoked) continue;
    for (const grant of controller.grants) {
      const origin = grant?.origin?.trim();
      if (origin) origins.add(origin);
    }
  }

  const prev = store.getState();
  const tabs: Record<string, BrowserStoreTab> = {};
  let changed = 0;
  for (const [browserId, tab] of Object.entries(prev.tabs)) {
    const origin = tab.private ? null : originOf(tab.url);
    const controlled = origin !== null && origins.has(origin);
    if (controlled !== (tab.agentControlled ?? false)) {
      tabs[browserId] = { ...tab, agentControlled: controlled };
      changed += 1;
    }
  }
  if (changed === 0) return 0;
  store.setState((current) => ({ ...current, tabs: { ...current.tabs, ...tabs } }));
  return changed;
}

/**
 * Keeps the global store live while the app runs: hydrates the eager startup sessions once, then
 * subscribes to host state changes (background tabs included) and to agent approval changes so
 * `agentControlled` stays accurate. Everything is unsubscribed on unmount or when disabled.
 */
export function useGlobalBrowserStoreSync(store: BrowserStore = browserStore, enabled = true) {
  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let unlistenState: (() => void) | undefined;
    let unlistenAgent: (() => void) | undefined;

    void hydrateGlobalBrowserStore(store).catch((error: unknown) => {
      if (!disposed) console.warn("Global browser hydration failed:", error);
    });

    const refreshAgentFlags = () => {
      void getBrowserAgentOverview()
        .then((overview) => {
          if (!disposed) applyAgentOverviewToStore(store, overview);
        })
        .catch(() => {
          // The authority is optional at startup (older builds, remote workspaces); flags stay put.
        });
    };

    void onBrowserStateChanged((payload) => {
      applyBrowserStateChangedToStore(store, payload);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlistenState = cleanup;
    }).catch((error: unknown) => {
      if (!disposed) console.warn("Browser live state subscription failed:", error);
    });

    void onBrowserAgentApprovals(() => refreshAgentFlags()).then((cleanup) => {
      if (disposed) cleanup();
      else unlistenAgent = cleanup;
    }).catch(() => {
      // No approval bridge in this build: the badge simply never turns on.
    });

    refreshAgentFlags();

    return () => {
      disposed = true;
      unlistenState?.();
      unlistenAgent?.();
    };
  }, [store, enabled]);
}

/** Test seam: forget startup hydration so a test can hydrate a fresh store. */
export function resetGlobalBrowserHydration(): void {
  hydratedGlobalBrowserIds.clear();
}
