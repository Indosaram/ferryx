import { toast } from "../components/ui/sonner";
import { DISMISSED_UPDATE_VERSION_STORAGE_KEY } from "./storageKeys";
import {
  downloadAndInstallUpdate,
  subscribeUpdateStatus,
  type UpdateStatus,
} from "./updater";

export const UPDATE_TOAST_ID = "update-available";

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function loadDismissedUpdateVersion(storage: Storage | null = browserStorage()): string | null {
  if (!storage) return null;
  try {
    const val = storage.getItem(DISMISSED_UPDATE_VERSION_STORAGE_KEY);
    return val && val.trim() ? val.trim() : null;
  } catch {
    return null;
  }
}

export function saveDismissedUpdateVersion(
  version: string,
  storage: Storage | null = browserStorage(),
): void {
  try {
    storage?.setItem(DISMISSED_UPDATE_VERSION_STORAGE_KEY, version);
  } catch {
    // ignore quota/disabled storage
  }
}

export function handleUpdateStatusChange(
  status: UpdateStatus,
  storage: Storage | null = browserStorage(),
): void {
  const version = status.version ?? "";
  switch (status.state) {
    case "available": {
      if (!version) return;
      const dismissed = loadDismissedUpdateVersion(storage);
      if (dismissed === version) return;

      toast.info("Update available", {
        id: UPDATE_TOAST_ID,
        description: `Ferryx v${version} is ready. Sessions won't be interrupted.`,
        duration: Infinity,
        action: {
          label: "Update",
          onClick: () => {
            void downloadAndInstallUpdate();
          },
        },
        onDismiss: () => {
          saveDismissedUpdateVersion(version, storage);
        },
      });
      break;
    }
    case "downloading": {
      const percent = Math.round((status.downloadProgress ?? 0) * 100);
      const description = version
        ? `Downloading version ${version}… ${percent}%`
        : `Downloading update… ${percent}%`;
      toast.loading("Downloading update", {
        id: UPDATE_TOAST_ID,
        description,
        duration: Infinity,
      });
      break;
    }
    case "downloaded": {
      toast.success(
        version ? `Version ${version} installed. Relaunching…` : "Update installed. Relaunching…",
        {
          id: UPDATE_TOAST_ID,
          duration: 8000,
        },
      );
      break;
    }
    case "error": {
      if (status.error) {
        toast.error(status.error, {
          id: UPDATE_TOAST_ID,
          duration: Infinity,
        });
      }
      break;
    }
    default:
      break;
  }
}

let initialized = false;
let unsubscribe: (() => void) | null = null;

export function initUpdateToasts(): () => void {
  if (initialized) {
    return unsubscribe ?? (() => {});
  }
  initialized = true;
  unsubscribe = subscribeUpdateStatus((status) => {
    handleUpdateStatusChange(status);
  });
  return () => {
    initialized = false;
    unsubscribe?.();
    unsubscribe = null;
  };
}
