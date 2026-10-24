import { useSyncExternalStore } from "react";

import {
  ACTIVE_PROJECT_STORAGE_KEY,
  ADVANCED_FEATURES_STORAGE_KEY,
  LEGACY_STORAGE_KEY_MAP,
  PROJECTS_STORAGE_KEY,
} from "./storageKeys";

/**
 * Progressive disclosure for newcomers. The embedded browser, the DAG viewer, the supervisor
 * (hub) panel and the Browser/Notifications settings sections are entry points a first-time
 * user does not need. A new install (no stored workspace state) starts with them hidden; an
 * existing install starts with them shown so nobody loses a surface they already use. The
 * resolved default is persisted on first read, so adding a project later does not flip it.
 * Hiding is entry-point only: live panes stay mounted and keyboard shortcuts stay bound.
 */
export const ADVANCED_FEATURES_EVENT = "ferryx:advanced-features";

const WORKSPACE_STATE_KEYS = [PROJECTS_STORAGE_KEY, ACTIVE_PROJECT_STORAGE_KEY];

let fallbackForTests: boolean | null = null;

/**
 * Test seam: the shared vitest setup pins the unset value to `true` so suites that clear
 * localStorage keep exercising every surface; advancedFeatures tests reset it to `null`.
 */
export function setAdvancedFeaturesFallbackForTests(value: boolean | null) {
  fallbackForTests = value;
}

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function hasStoredWorkspaceState(storage: Storage | null = browserStorage()): boolean {
  if (!storage) return false;
  try {
    return WORKSPACE_STATE_KEYS.some((key) =>
      [key, ...(LEGACY_STORAGE_KEY_MAP[key] ?? [])].some((candidate) => storage.getItem(candidate) !== null),
    );
  } catch {
    return false;
  }
}

export function loadAdvancedFeatures(storage: Storage | null = browserStorage()): boolean {
  let raw: string | null = null;
  try {
    raw = storage?.getItem(ADVANCED_FEATURES_STORAGE_KEY) ?? null;
  } catch {
    raw = null;
  }
  if (raw === "true") return true;
  if (raw === "false") return false;
  if (fallbackForTests !== null) return fallbackForTests;
  const resolved = hasStoredWorkspaceState(storage);
  try {
    storage?.setItem(ADVANCED_FEATURES_STORAGE_KEY, String(resolved));
  } catch {
    // Unpersisted, the same default is recomputed on the next read.
  }
  return resolved;
}

export function saveAdvancedFeatures(enabled: boolean, storage: Storage | null = browserStorage()): boolean {
  try {
    storage?.setItem(ADVANCED_FEATURES_STORAGE_KEY, String(enabled));
  } catch (error) {
    console.warn("Failed to persist advanced features setting", error);
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent<boolean>(ADVANCED_FEATURES_EVENT, { detail: enabled }));
  }
  return enabled;
}

function subscribe(listener: () => void) {
  const onStorage = (event: StorageEvent) => {
    if (event.key === ADVANCED_FEATURES_STORAGE_KEY || event.key === null) listener();
  };
  window.addEventListener(ADVANCED_FEATURES_EVENT, listener);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(ADVANCED_FEATURES_EVENT, listener);
    window.removeEventListener("storage", onStorage);
  };
}

/** Live value: toggling in Settings shows or hides the entry points without a reload. */
export function useAdvancedFeatures(): boolean {
  return useSyncExternalStore(subscribe, () => loadAdvancedFeatures(), () => false);
}
