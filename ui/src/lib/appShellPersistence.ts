import {
  ACTIVE_PROJECT_STORAGE_KEY,
  PROJECTS_STORAGE_KEY,
  SIDEBAR_OPEN_STORAGE_KEY,
} from "./storageKeys";
import type { RegisteredProject } from "./tauri";
import { windowScopedStorageKey } from "./windowIdentity";

export function persistProjects(projects: RegisteredProject[]) {
  try {
    window.localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
  } catch {
    // A local persistence failure must not block native project registration.
  }
}

/** Per window: a secondary window keeps its own selection so it never switches the primary's. */
export function persistActiveProjectId(workspaceId: string) {
  try {
    window.localStorage.setItem(windowScopedStorageKey(ACTIVE_PROJECT_STORAGE_KEY), workspaceId);
  } catch {
    // The selected project still remains active for this session.
  }
}

export function persistSidebarOpen(open: boolean) {
  try {
    window.localStorage.setItem(SIDEBAR_OPEN_STORAGE_KEY, String(open));
  } catch {
    // Persistence failure should not break in-memory state.
  }
}
