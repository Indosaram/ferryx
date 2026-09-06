import { PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY } from "./storageKeys";
import type { SystemPermissionsStatus } from "./types";

export const OPEN_PERMISSIONS_ONBOARDING_EVENT = "ferryx:open-permissions-onboarding";

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function loadPermissionsOnboardingDismissed(
  storage: Storage | null = browserStorage()
): boolean {
  if (!storage) return false;
  try {
    const val = storage.getItem(PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY);
    return val === "true";
  } catch {
    return false;
  }
}

export function savePermissionsOnboardingDismissed(
  storage: Storage | null = browserStorage()
): void {
  if (!storage) return;
  try {
    storage.setItem(PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY, "true");
  } catch {}
}

export function resetPermissionsOnboardingDismissed(
  storage: Storage | null = browserStorage()
): void {
  if (!storage) return;
  try {
    storage.removeItem(PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY);
  } catch {}
}

export function shouldShowPermissionsOnboarding(
  status: SystemPermissionsStatus | null,
  dismissed: boolean
): boolean {
  return !dismissed && status !== null && status.platform === "macos" && !status.allGranted;
}
