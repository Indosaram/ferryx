// Maho's auto-archive policy for Today.
//
// Today is a scratch list: a tab nobody looked at for half a day is not something the
// user is still working on, so the scheduler archives it (an archived tab is
// restorable from the Library, never lost). The policy is a pure function of the
// store state plus an injected clock, which is why the tests never sleep: they move
// `now` forward and call `runAutoArchiveSweep` directly.
//
// What is never archived, in order of how much the user would mind losing it:
//   - Favorites and Pinned tabs (the user asked for them to stay),
//   - tabs an agent is driving (`agentControlled`), because control is mid-flight,
//   - the active tab of any Space and the Mini Pane tab (the page on screen),
//   - private tabs, which are never persisted and so could not be restored.
//
// The archive itself goes through `browserStore.closeTab`, the one action that both
// removes a live tab and keeps an archive entry for it.

import { useCallback, useEffect, useRef, useState } from "react";

import { BROWSER_AUTO_ARCHIVE_HOURS_STORAGE_KEY } from "../../../lib/storageKeys";
import {
  browserStore,
  type BrowserStore,
  type BrowserStoreTab,
} from "../../../state/browserStore";

/** Idle window before a Today tab is archived, when nothing else is configured. */
export const DEFAULT_AUTO_ARCHIVE_HOURS = 12;

/** Sweep cadence of `useBrowserAutoArchive`; 0 disables the timer. */
export const DEFAULT_AUTO_ARCHIVE_INTERVAL_MS = 5 * 60_000;

const MS_PER_HOUR = 3_600_000;

type HoursStorage = Pick<Storage, "getItem" | "setItem"> | null;

function defaultStorage(): HoursStorage {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** The configured idle window; anything unusable (missing, blank, NaN, <= 0) is the default. */
export function loadAutoArchiveHours(storage: HoursStorage = defaultStorage()): number {
  if (!storage) return DEFAULT_AUTO_ARCHIVE_HOURS;
  let raw: string | null = null;
  try {
    raw = storage.getItem(BROWSER_AUTO_ARCHIVE_HOURS_STORAGE_KEY);
  } catch {
    return DEFAULT_AUTO_ARCHIVE_HOURS;
  }
  if (raw === null || raw.trim() === "") return DEFAULT_AUTO_ARCHIVE_HOURS;
  const parsed = Number(raw);
  if (!Number.isFinite(parsed) || parsed <= 0) return DEFAULT_AUTO_ARCHIVE_HOURS;
  return parsed;
}

/** Persists the idle window; a storage failure keeps the default in effect for the session. */
export function saveAutoArchiveHours(hours: number, storage: HoursStorage = defaultStorage()): number {
  const value = Number.isFinite(hours) && hours > 0 ? hours : DEFAULT_AUTO_ARCHIVE_HOURS;
  try {
    storage?.setItem(BROWSER_AUTO_ARCHIVE_HOURS_STORAGE_KEY, String(value));
  } catch {
    // Quota or disabled storage: the in-memory default still applies.
  }
  return value;
}

export type AutoArchiveSweepOptions = {
  /** Injected clock, so a test moves time instead of waiting for it. */
  now?: () => number;
  /** Idle window in hours; defaults to the persisted setting. */
  maxIdleHours?: number;
};

export type AutoArchiveSweepResult = {
  archivedCount: number;
  /** browserIds in the order they were archived (oldest first). */
  archivedIds: readonly string[];
};

/** Whether the policy is allowed to archive this tab at all. */
export function isAutoArchivable(
  tab: BrowserStoreTab,
  state: { activeBrowserIdBySpace: Readonly<Record<string, string | null>>; miniBrowserId: string | null },
): boolean {
  if (tab.section !== "today" || tab.folderId) return false;
  if (tab.agentControlled === true) return false;
  if (tab.private === true) return false;
  if (state.miniBrowserId === tab.browserId) return false;
  return Object.values(state.activeBrowserIdBySpace).every((browserId) => browserId !== tab.browserId);
}

/**
 * Archives every Today tab idle for longer than `maxIdleHours`, across all Spaces.
 * Returns what it archived so a caller can offer an Undo or a count.
 */
export function runAutoArchiveSweep(
  store: BrowserStore = browserStore,
  options: AutoArchiveSweepOptions = {},
): AutoArchiveSweepResult {
  const now = options.now ?? (() => Date.now());
  const maxIdleHours = options.maxIdleHours ?? loadAutoArchiveHours();
  if (!Number.isFinite(maxIdleHours) || maxIdleHours <= 0) {
    return { archivedCount: 0, archivedIds: [] };
  }

  const state = store.getState();
  const cutoff = maxIdleHours * MS_PER_HOUR;
  const stamp = now();
  const doomed = Object.values(state.tabs)
    .filter((tab) => isAutoArchivable(tab, state) && stamp - tab.lastActiveAt > cutoff)
    .sort((left, right) => left.lastActiveAt - right.lastActiveAt
      || left.browserId.localeCompare(right.browserId));

  const archivedIds: string[] = [];
  for (const tab of doomed) {
    // A close during the loop can already have removed it; only a live tab archives.
    if (!store.getState().tabs[tab.browserId]) continue;
    if (store.closeTab(tab.browserId).ok) archivedIds.push(tab.browserId);
  }
  return { archivedCount: archivedIds.length, archivedIds };
}

export type BrowserAutoArchiveOptions = AutoArchiveSweepOptions & {
  /** Sweep cadence in milliseconds; 0 runs one sweep and installs no timer. */
  intervalMs?: number;
};

/**
 * Runs one sweep on mount and then on `intervalMs`, with the clock and the window
 * injectable. Returns how many tabs the most recent sweep archived.
 */
export function useBrowserAutoArchive(
  store: BrowserStore = browserStore,
  options: BrowserAutoArchiveOptions = {},
): number {
  const { intervalMs = DEFAULT_AUTO_ARCHIVE_INTERVAL_MS, maxIdleHours, now } = options;
  const nowRef = useRef(now);
  nowRef.current = now;
  const hoursRef = useRef(maxIdleHours);
  hoursRef.current = maxIdleHours;
  const [archivedCount, setArchivedCount] = useState(0);

  const sweep = useCallback(() => {
    const clock = nowRef.current ?? (() => Date.now());
    const hours = hoursRef.current ?? loadAutoArchiveHours();
    const result = runAutoArchiveSweep(store, { now: clock, maxIdleHours: hours });
    setArchivedCount(result.archivedCount);
  }, [store]);

  useEffect(() => {
    sweep();
    if (intervalMs <= 0) return;
    const timer = setInterval(sweep, intervalMs);
    return () => clearInterval(timer);
  }, [intervalMs, sweep]);

  return archivedCount;
}
