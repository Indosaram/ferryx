import { toast } from "../components/ui/sonner";
import { PERMISSIONS_TOAST_DISMISSED_STORAGE_KEY } from "./storageKeys";
import { getSystemPermissionsStatus } from "./tauri";

export const PERMISSIONS_TOAST_ID = "permissions-guidance";

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function loadPermissionsToastDismissed(
  storage: Storage | null = browserStorage()
): boolean {
  if (!storage) return false;
  try {
    const val = storage.getItem(PERMISSIONS_TOAST_DISMISSED_STORAGE_KEY);
    return val === "true";
  } catch {
    return false;
  }
}

export function savePermissionsToastDismissed(
  storage: Storage | null = browserStorage()
): void {
  if (!storage) return;
  try {
    storage.setItem(PERMISSIONS_TOAST_DISMISSED_STORAGE_KEY, "true");
  } catch {}
}

export function resetPermissionsToastDismissed(
  storage: Storage | null = browserStorage()
): void {
  if (!storage) return;
  try {
    storage.removeItem(PERMISSIONS_TOAST_DISMISSED_STORAGE_KEY);
  } catch {}
}

export function showPermissionsGuidanceToast(options: {
  onOpenSettings: () => void;
  storage?: Storage | null;
}): string | number | undefined {
  return toast.warning("System Permissions Recommended", {
    id: PERMISSIONS_TOAST_ID,
    description:
      "Grant Full Disk Access, Accessibility, and Notifications to prevent macOS Photo Library prompts and ensure smooth terminal execution.",
    duration: 15000,
    action: {
      label: "Open Settings",
      onClick: (event?: { preventDefault?: () => void }) => {
        event?.preventDefault?.();
        toast.dismiss(PERMISSIONS_TOAST_ID);
        options.onOpenSettings();
      },
    },
    cancel: {
      label: "Later",
      onClick: () => {
        savePermissionsToastDismissed(options.storage);
      },
    },
  });
}

export async function checkAndPromptPermissions(options: {
  onOpenSettings: () => void;
  storage?: Storage | null;
  force?: boolean;
}): Promise<boolean> {
  const dismissed = loadPermissionsToastDismissed(options.storage);
  if (dismissed && !options.force) {
    return false;
  }

  try {
    const status = await getSystemPermissionsStatus();
    if (status.allGranted) {
      toast.dismiss(PERMISSIONS_TOAST_ID);
      return false;
    }

    showPermissionsGuidanceToast({
      onOpenSettings: options.onOpenSettings,
      storage: options.storage,
    });
    return true;
  } catch {
    return false;
  }
}

export function initPermissionsToast(options: {
  onOpenSettings: (section: "permissions") => void;
  delayMs?: number;
}): () => void {
  const timer = setTimeout(() => {
    void checkAndPromptPermissions({
      onOpenSettings: () => options.onOpenSettings("permissions"),
    });
  }, options.delayMs ?? 1800);

  return () => {
    clearTimeout(timer);
  };
}
