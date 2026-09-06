import { invoke, isTauri } from "@tauri-apps/api/core";
import { toast } from "../components/ui/sonner";

export const WINDOWS_STORE_MIGRATION_TOAST_ID = "windows-store-migration";
export const MICROSOFT_STORE_SEARCH_URL = "https://apps.microsoft.com/search?query=Ferryx";
const DISMISSED_FLAG_KEY = "ferryx.windowsStoreMigration.dismissed";

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function loadMigrationDismissed(storage: Storage | null = browserStorage()): boolean {
  return storage?.getItem(DISMISSED_FLAG_KEY) === "1";
}

export function saveMigrationDismissed(storage: Storage | null = browserStorage()): void {
  try {
    storage?.setItem(DISMISSED_FLAG_KEY, "1");
  } catch (error) {
    console.error("Persisting the Windows Store migration dismissal failed:", error);
  }
}

export async function maybeShowWindowsStoreMigrationNotice(storage: Storage | null = browserStorage()): Promise<void> {
  if (!isTauri()) return;
  if (loadMigrationDismissed(storage)) return;

  let channel = "";
  try {
    channel = await invoke<string>("distribution_channel");
  } catch {
    return;
  }
  if (channel !== "installer") return;

  toast.info("Ferryx for Windows is moving to the Microsoft Store", {
    id: WINDOWS_STORE_MIGRATION_TOAST_ID,
    duration: Infinity,
    description:
      "This installer will stop receiving updates. Once the Store version is published, reinstall Ferryx from the Microsoft Store to get automatic updates again.",
    action: {
      label: "Got it",
      onClick: () => saveMigrationDismissed(storage),
    },
    onDismiss: () => saveMigrationDismissed(storage),
  });
}
