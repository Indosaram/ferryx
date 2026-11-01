import { useSyncExternalStore } from "react";

import { isSupportedHistoryUrl } from "./browserHistory";
import { isPrivateBrowserProfileId, loadBrowserSettings } from "./browserSettings";
import { goBackBrowser, goForwardBrowser, navigateBrowser } from "./browserTauri";
import { BROWSER_TAB_NAVIGATION_STORAGE_KEY } from "./storageKeys";

/** Back/forward entries kept per tab; the oldest fall off first. */
export const BROWSER_TAB_NAVIGATION_LIMIT = 50;
/** Tabs whose stacks are persisted; the least recently navigated are dropped first. */
export const BROWSER_TAB_NAVIGATION_TAB_LIMIT = 200;

export type BrowserNavigationEntry = { url: string; title: string | null };

export type PersistedBrowserNavigation = {
  entries: BrowserNavigationEntry[];
  index: number;
  updatedAt: number;
};

type TabStack = PersistedBrowserNavigation & {
  /**
   * Set once the engine's own history stops covering `entries`: after a restart (engines have
   * no history import) or after Ferryx navigated to an entry itself. Only `nativeBase..nativeTop`
   * is then reachable through the engine; the rest is reached by navigating to the entry's URL.
   */
  ferryxDriven: boolean;
  nativeBase: number;
  nativeTop: number;
  /** The entry a Ferryx-driven back/forward is loading, so its URL change is not a new visit. */
  pending: number | null;
};

export type BrowserNavigationStep = { kind: "native" } | { kind: "navigate"; url: string } | { kind: "none" };

export type BrowserNavigationAvailability = {
  canGoBack: boolean;
  canGoForward: boolean;
  /** Back/forward is possible only through Ferryx (the engine has no such entry). */
  restoredBack: boolean;
  restoredForward: boolean;
};

function defaultStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function normalizeEntry(value: unknown): BrowserNavigationEntry | null {
  if (!value || typeof value !== "object") return null;
  const source = value as Partial<BrowserNavigationEntry>;
  if (typeof source.url !== "string" || !isSupportedHistoryUrl(source.url)) return null;
  const title = typeof source.title === "string" && source.title.trim() ? source.title.trim() : null;
  return { url: source.url, title };
}

/** Keeps at most the cap, dropping the oldest entries but never the current one. */
function capEntries(entries: BrowserNavigationEntry[], index: number): { entries: BrowserNavigationEntry[]; dropped: number } {
  let start = Math.max(0, entries.length - BROWSER_TAB_NAVIGATION_LIMIT);
  if (index < start) start = index;
  return { entries: entries.slice(start, start + BROWSER_TAB_NAVIGATION_LIMIT), dropped: start };
}

export function parseBrowserTabNavigation(raw: string | null): Record<string, PersistedBrowserNavigation> {
  if (!raw) return {};
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
  const result: Record<string, PersistedBrowserNavigation> = {};
  for (const [browserId, value] of Object.entries(parsed as Record<string, unknown>)) {
    if (!browserId || !value || typeof value !== "object") continue;
    const source = value as Partial<PersistedBrowserNavigation>;
    const all = (Array.isArray(source.entries) ? source.entries : [])
      .map(normalizeEntry)
      .filter((entry): entry is BrowserNavigationEntry => entry !== null);
    if (all.length === 0) continue;
    const rawIndex = typeof source.index === "number" && Number.isInteger(source.index) ? source.index : all.length - 1;
    const index = Math.min(Math.max(rawIndex, 0), all.length - 1);
    const capped = capEntries(all, index);
    result[browserId] = {
      entries: capped.entries,
      index: index - capped.dropped,
      updatedAt: typeof source.updatedAt === "number" && Number.isFinite(source.updatedAt) ? source.updatedAt : 0,
    };
  }
  return result;
}

export function serializeBrowserTabNavigation(stacks: Record<string, PersistedBrowserNavigation>): string {
  const newest = Object.entries(stacks)
    .sort(([, left], [, right]) => right.updatedAt - left.updatedAt)
    .slice(0, BROWSER_TAB_NAVIGATION_TAB_LIMIT);
  const result: Record<string, PersistedBrowserNavigation> = {};
  for (const [browserId, stack] of newest) {
    result[browserId] = {
      entries: stack.entries.map((entry) => ({ url: entry.url, title: entry.title })),
      index: stack.index,
      updatedAt: stack.updatedAt,
    };
  }
  return JSON.stringify(result);
}

function restoredStack(persisted: PersistedBrowserNavigation): TabStack {
  return {
    ...persisted,
    ferryxDriven: true,
    nativeBase: persisted.index,
    nativeTop: persisted.index,
    pending: null,
  };
}

/**
 * Per-tab back/forward stack kept by Ferryx for both engines. Neither the system webview nor the
 * Chromium engine can import history, so after a restart the stack (persisted under
 * `ferryx.browser.tab-navigation`, keyed by the persisted browser id) drives back/forward by
 * navigating to the stored URL whenever the engine itself has no such entry.
 */
export class BrowserTabNavigationStore {
  private stacks: Map<string, TabStack> | null = null;
  private readonly listeners = new Set<() => void>();
  private version = 0;

  constructor(
    private readonly storage: () => Storage | null = defaultStorage,
    private readonly now: () => number = Date.now,
  ) {}

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  readonly getVersion = (): number => this.version;

  private load(): Map<string, TabStack> {
    if (this.stacks) return this.stacks;
    const stacks = new Map<string, TabStack>();
    const storage = this.storage();
    if (storage && loadBrowserSettings(storage).restoreTabsOnLaunch) {
      let raw: string | null = null;
      try {
        raw = storage.getItem(BROWSER_TAB_NAVIGATION_STORAGE_KEY);
      } catch {
        raw = null;
      }
      for (const [browserId, persisted] of Object.entries(parseBrowserTabNavigation(raw))) {
        stacks.set(browserId, restoredStack(persisted));
      }
    }
    this.stacks = stacks;
    return stacks;
  }

  private changed(): void {
    this.version += 1;
    const storage = this.storage();
    if (storage) {
      try {
        const stacks = this.load();
        if (stacks.size === 0 || !loadBrowserSettings(storage).restoreTabsOnLaunch) {
          storage.removeItem(BROWSER_TAB_NAVIGATION_STORAGE_KEY);
        } else {
          storage.setItem(BROWSER_TAB_NAVIGATION_STORAGE_KEY, serializeBrowserTabNavigation(Object.fromEntries(stacks)));
        }
      } catch {
        // Ignore disabled storage and quota failures; the in-memory stack still works.
      }
    }
    for (const listener of this.listeners) listener();
  }

  /** Records the tab's committed URL (and title). Private-profile tabs are never recorded. */
  record(browserId: string, url: string, title: string | null | undefined, profileId: string | null | undefined): void {
    if (!browserId || isPrivateBrowserProfileId(profileId) || !isSupportedHistoryUrl(url)) return;
    const stacks = this.load();
    const cleanTitle = title?.trim() || null;
    const stack = stacks.get(browserId);
    if (!stack) {
      stacks.set(browserId, {
        entries: [{ url, title: cleanTitle }],
        index: 0,
        updatedAt: this.now(),
        ferryxDriven: false,
        nativeBase: 0,
        nativeTop: 0,
        pending: null,
      });
      this.changed();
      return;
    }

    const pending = stack.pending;
    stack.pending = null;
    const { entries, index } = stack;
    if (pending !== null && entries[pending]?.url === url) {
      // A Ferryx-driven step landed: the engine appended this page to its own history, so
      // only this entry is reachable natively now.
      stack.index = pending;
      stack.nativeBase = pending;
      stack.nativeTop = pending;
    } else if (entries[index].url === url) {
      if (!cleanTitle || cleanTitle === entries[index].title) return;
    } else if (index > 0 && entries[index - 1].url === url && (!stack.ferryxDriven || index > stack.nativeBase)) {
      // The engine went back on its own (its own back button, a script, a shortcut). Below
      // `nativeBase` a restored engine has no entry, so a match there is a new visit.
      stack.index = index - 1;
    } else if (
      index + 1 < entries.length &&
      entries[index + 1].url === url &&
      (!stack.ferryxDriven || index < stack.nativeTop)
    ) {
      stack.index = index + 1;
    } else {
      const next = [...entries.slice(0, index + 1), { url, title: cleanTitle }];
      const capped = capEntries(next, next.length - 1);
      stack.entries = capped.entries;
      stack.index = stack.entries.length - 1;
      stack.nativeTop = stack.index;
      stack.nativeBase = Math.max(0, stack.nativeBase - capped.dropped);
    }
    if (cleanTitle) stack.entries[stack.index] = { url, title: cleanTitle };
    stack.updatedAt = this.now();
    this.changed();
  }

  /**
   * Called when a persisted tab is recreated: its engine starts with an empty history, so the
   * stored stack takes over and is aligned with the URL the tab reopens at.
   * `engineHasHistory` is set when the tab was still alive in the host (only the UI reloaded):
   * its engine history is intact and keeps driving back/forward.
   */
  restore(browserId: string, url: string, engineHasHistory = false): void {
    const stack = this.load().get(browserId);
    if (!stack) return;
    if (stack.entries[stack.index].url !== url && isSupportedHistoryUrl(url)) {
      const at = stack.entries.map((entry) => entry.url).lastIndexOf(url);
      if (at >= 0) {
        stack.index = at;
      } else {
        const capped = capEntries([...stack.entries.slice(0, stack.index + 1), { url, title: null }], stack.index + 1);
        stack.entries = capped.entries;
        stack.index = stack.entries.length - 1;
      }
    }
    stack.ferryxDriven = !engineHasHistory;
    stack.nativeBase = stack.index;
    stack.nativeTop = stack.index;
    stack.pending = null;
    this.changed();
  }

  forget(browserId: string): void {
    if (this.load().delete(browserId)) this.changed();
  }

  clear(): void {
    this.stacks = new Map();
    this.changed();
  }

  snapshot(browserId: string): { entries: BrowserNavigationEntry[]; index: number } | null {
    const stack = this.load().get(browserId);
    return stack ? { entries: stack.entries.map((entry) => ({ ...entry })), index: stack.index } : null;
  }

  availability(browserId: string, engineCanGoBack: boolean, engineCanGoForward: boolean): BrowserNavigationAvailability {
    const stack = this.load().get(browserId);
    if (!stack || !stack.ferryxDriven) {
      return { canGoBack: engineCanGoBack, canGoForward: engineCanGoForward, restoredBack: false, restoredForward: false };
    }
    const last = stack.entries.length - 1;
    return {
      canGoBack: stack.index > 0,
      canGoForward: stack.index < last,
      restoredBack: stack.index > 0 && stack.index <= stack.nativeBase,
      restoredForward: stack.index < last && stack.index >= stack.nativeTop,
    };
  }

  planBack(browserId: string): BrowserNavigationStep {
    const stack = this.load().get(browserId);
    if (!stack || !stack.ferryxDriven || stack.index > stack.nativeBase) return { kind: "native" };
    if (stack.index === 0) return { kind: "none" };
    stack.pending = stack.index - 1;
    return { kind: "navigate", url: stack.entries[stack.pending].url };
  }

  planForward(browserId: string): BrowserNavigationStep {
    const stack = this.load().get(browserId);
    if (!stack || !stack.ferryxDriven || stack.index < stack.nativeTop) return { kind: "native" };
    if (stack.index >= stack.entries.length - 1) return { kind: "none" };
    stack.pending = stack.index + 1;
    return { kind: "navigate", url: stack.entries[stack.pending].url };
  }

  cancelPending(browserId: string): void {
    const stack = this.load().get(browserId);
    if (stack) stack.pending = null;
  }
}

export const browserTabNavigation = new BrowserTabNavigationStore();

async function runStep(
  browserId: string,
  step: BrowserNavigationStep,
  native: (browserId: string) => Promise<void>,
  store: BrowserTabNavigationStore,
): Promise<void> {
  if (step.kind === "native") return native(browserId);
  if (step.kind === "none") return;
  try {
    await navigateBrowser(browserId, step.url);
  } catch (error) {
    store.cancelPending(browserId);
    throw error;
  }
}

export function goBackInBrowserTab(browserId: string, store: BrowserTabNavigationStore = browserTabNavigation): Promise<void> {
  return runStep(browserId, store.planBack(browserId), goBackBrowser, store);
}

export function goForwardInBrowserTab(browserId: string, store: BrowserTabNavigationStore = browserTabNavigation): Promise<void> {
  return runStep(browserId, store.planForward(browserId), goForwardBrowser, store);
}

export function useBrowserTabNavigation(
  browserId: string,
  engineCanGoBack: boolean,
  engineCanGoForward: boolean,
  store: BrowserTabNavigationStore = browserTabNavigation,
): BrowserNavigationAvailability {
  useSyncExternalStore(store.subscribe, store.getVersion, store.getVersion);
  return store.availability(browserId, engineCanGoBack, engineCanGoForward);
}
