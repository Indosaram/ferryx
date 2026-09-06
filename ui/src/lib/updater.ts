import { getVersion } from "@tauri-apps/api/app";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

export type UpdateStatus = {
  state: "idle" | "checking" | "available" | "downloading" | "downloaded" | "error";
  version?: string;
  releaseNotes?: string;
  downloadProgress?: number;
  error?: string;
};

type CloseGuardListener = () => Promise<void> | void;
const closeGuardListeners = new Set<CloseGuardListener>();

export function registerWindowCloseGuard(listener: CloseGuardListener): () => void {
  closeGuardListeners.add(listener);
  return () => {
    closeGuardListeners.delete(listener);
  };
}

export async function flushCloseGuards(): Promise<void> {
  for (const listener of closeGuardListeners) {
    try {
      await listener();
    } catch (e) {
      console.error("Window close guard execution failed:", e);
    }
  }
}

type StatusListener = (status: UpdateStatus) => void;

const statusListeners = new Set<StatusListener>();
let status: UpdateStatus = { state: "idle" };
let pendingUpdate: Update | null = null;

function setStatus(next: UpdateStatus): void {
  status = next;
  for (const listener of statusListeners) listener(status);
}

function describe(error: unknown): string {
  if (error instanceof Error) return error.message;
  return typeof error === "string" ? error : "Unknown updater failure";
}

export function getUpdateStatus(): UpdateStatus {
  return status;
}

let managedExternallyCache: boolean | null = null;

async function updatesManagedExternally(): Promise<boolean> {
  if (managedExternallyCache === null) {
    try {
      managedExternallyCache = await invoke<boolean>("updater_managed_externally");
    } catch {
      managedExternallyCache = false;
    }
  }
  return managedExternallyCache;
}

export function subscribeUpdateStatus(listener: StatusListener): () => void {
  statusListeners.add(listener);
  return () => {
    statusListeners.delete(listener);
  };
}

export async function getCurrentVersion(): Promise<string | null> {
  if (!isTauri()) return null;
  try {
    return await getVersion();
  } catch (error) {
    console.error("Reading the app version failed:", error);
    return null;
  }
}

export async function checkForUpdate(): Promise<void> {
  if (!isTauri()) return;
  if (await updatesManagedExternally()) return;
  if (status.state !== "idle" && status.state !== "error") return;

  setStatus({ state: "checking" });
  try {
    const update = await check();
    if (!update) {
      pendingUpdate = null;
      setStatus({ state: "idle" });
      return;
    }
    pendingUpdate = update;
    setStatus({
      state: "available",
      version: update.version,
      releaseNotes: update.body ?? undefined,
    });
  } catch (error) {
    pendingUpdate = null;
    setStatus({ state: "error", error: describe(error) });
  }
}

export async function downloadAndInstallUpdate(): Promise<void> {
  if (!isTauri()) return;

  const update = pendingUpdate;
  if (!update) return;

  const version = update.version;
  const releaseNotes = update.body ?? undefined;
  let contentLength = 0;
  let downloaded = 0;

  setStatus({ state: "downloading", version, releaseNotes, downloadProgress: 0 });
  try {
    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          contentLength = event.data.contentLength ?? 0;
          break;
        case "Progress":
          downloaded += event.data.chunkLength;
          break;
        case "Finished":
          downloaded = contentLength;
          break;
      }
      // Without a Content-Length the total is unknown, so the bar stays at 0 rather than
      // reporting a fraction of an unknown whole.
      const progress = contentLength > 0 ? Math.min(downloaded / contentLength, 1) : 0;
      setStatus({ state: "downloading", version, releaseNotes, downloadProgress: progress });
    });
    setStatus({ state: "downloaded", version, releaseNotes, downloadProgress: 1 });
    await relaunchApp();
  } catch (error) {
    setStatus({ state: "error", version, releaseNotes, error: describe(error) });
  }
}

export async function relaunchApp(): Promise<void> {
  if (!isTauri()) return;
  await flushCloseGuards();
  await relaunch();
}

export const UPDATE_CHECK_INTERVAL_MS = 60 * 60 * 1000;

let pollGeneration = 0;
let pollTimer: ReturnType<typeof setTimeout> | null = null;

export function stopUpdatePolling(): void {
  pollGeneration += 1;
  if (pollTimer !== null) {
    clearTimeout(pollTimer);
    pollTimer = null;
  }
}

export function startUpdatePolling(intervalMs: number = UPDATE_CHECK_INTERVAL_MS): () => void {
  stopUpdatePolling();
  const generation = pollGeneration;
  void checkForUpdate();
  const tick = (): void => {
    void checkForUpdate().finally(() => {
      if (pollGeneration === generation) {
        pollTimer = setTimeout(tick, intervalMs);
      }
    });
  };
  pollTimer = setTimeout(tick, intervalMs);
  return stopUpdatePolling;
}
