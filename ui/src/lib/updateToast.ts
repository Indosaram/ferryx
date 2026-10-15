import { toast } from "../components/ui/sonner";
import { DISMISSED_UPDATE_VERSION_STORAGE_KEY } from "./storageKeys";
import {
  cancelUpdateDownload,
  downloadAndInstallUpdate,
  getUpdateStatus,
  relaunchApp,
  subscribeUpdateStatus,
  type UpdateStatus,
  updatesManagedExternally,
} from "./updater";

export const UPDATE_TOAST_ID = "update-available";

async function installAndRelaunch(): Promise<void> {
  await downloadAndInstallUpdate();
  if (getUpdateStatus().state === "downloaded") await relaunchApp();
}

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
      if (status.lastFailure === "stalled") {
        toast.error("Update download stalled", {
          id: UPDATE_TOAST_ID,
          description: `Ferryx v${version} stopped downloading. Nothing was installed.`,
          duration: Infinity,
          action: {
            label: "Retry",
            onClick: () => installAndRelaunch(),
          },
        });
        return;
      }
      const dismissed = loadDismissedUpdateVersion(storage);
      if (dismissed === version) return;

      toast.info("Update available", {
        id: UPDATE_TOAST_ID,
        description:
          status.lastFailure === "cancelled"
            ? `Download of Ferryx v${version} was cancelled. Nothing was installed.`
            : `Ferryx v${version} is ready. Sessions won't be interrupted.`,
        duration: Infinity,
        action: {
          label: "Update",
          onClick: () => installAndRelaunch(),
        },
        onDismiss: () => {
          saveDismissedUpdateVersion(version, storage);
        },
      });
      break;
    }
    case "downloading": {
      if (status.installing) {
        // Installation cannot be cancelled, so the toast offers no action here.
        toast.loading("Installing update", {
          id: UPDATE_TOAST_ID,
          description: version ? `Installing version ${version}…` : "Installing update…",
          duration: Infinity,
          action: undefined,
        });
        break;
      }
      const percent = Math.round((status.downloadProgress ?? 0) * 100);
      const description = status.cancelRequested
        ? "Cancelling download…"
        : version
          ? `Downloading version ${version}… ${percent}%`
          : `Downloading update… ${percent}%`;
      toast.loading("Downloading update", {
        id: UPDATE_TOAST_ID,
        description,
        duration: Infinity,
        action: status.cancelRequested
          ? undefined
          : {
              label: "Cancel",
              onClick: () => {
                void cancelUpdateDownload();
              },
            },
      });
      break;
    }
    case "downloaded": {
      toast.success(
        version ? `Version ${version} installed. Relaunching…` : "Update installed. Relaunching…",
        {
          id: UPDATE_TOAST_ID,
          duration: 8000,
          // Toast updates merge into the existing toast, so drop the downloading Cancel action.
          action: undefined,
        },
      );
      break;
    }
    case "error": {
      if (status.error) {
        toast.error(status.error, {
          id: UPDATE_TOAST_ID,
          duration: Infinity,
          action: undefined,
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
  void updatesManagedExternally().then((managed) => {
    if (managed || !initialized) return;
    unsubscribe = subscribeUpdateStatus((status) => {
      handleUpdateStatusChange(status);
    });
  });
  return () => {
    initialized = false;
    unsubscribe?.();
    unsubscribe = null;
  };
}
