import { getVersion } from "@tauri-apps/api/app";
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";

import { UPDATE_BACKGROUND_DOWNLOAD_STORAGE_KEY, UPDATE_CHANNEL_STORAGE_KEY } from "./storageKeys";

export type UpdateFailure = "cancelled" | "stalled";

/** Release channel the in-app updater follows. Stable is the default. */
export type UpdateChannel = "stable" | "beta";

/**
 * Update manifests per channel. They mirror `STABLE_UPDATE_ENDPOINT` and `BETA_UPDATE_ENDPOINT` in
 * src-tauri/src/ipc/updater.rs, which picks the endpoint from the channel this module sends; the UI
 * never sends a URL. Beta falls back to the stable manifest when no beta manifest is published.
 */
export const UPDATE_ENDPOINTS: Record<UpdateChannel, readonly string[]> = {
  stable: ["https://github.com/Indosaram/ferryx/releases/latest/download/latest.json"],
  beta: [
    "https://github.com/Indosaram/ferryx/releases/latest/download/latest-beta.json",
    "https://github.com/Indosaram/ferryx/releases/latest/download/latest.json",
  ],
};

function channelStorage(): Storage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

export function getUpdateChannel(): UpdateChannel {
  try {
    return channelStorage()?.getItem(UPDATE_CHANNEL_STORAGE_KEY) === "beta" ? "beta" : "stable";
  } catch {
    return "stable";
  }
}

/**
 * Whether a found update is downloaded in the background instead of only on request. Defaults to
 * off, so nothing is fetched until the user asks for it.
 */
export function getBackgroundUpdateDownload(storage: Storage | null = channelStorage()): boolean {
  try {
    return storage?.getItem(UPDATE_BACKGROUND_DOWNLOAD_STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

/** Persists the preference and, when it is turned on, stages an offer that is already waiting. */
export function setBackgroundUpdateDownload(
  enabled: boolean,
  storage: Storage | null = channelStorage(),
): void {
  try {
    storage?.setItem(UPDATE_BACKGROUND_DOWNLOAD_STORAGE_KEY, enabled ? "true" : "false");
  } catch (error) {
    console.warn("Failed to persist the background update download preference", error);
  }
  if (enabled && status.state === "available") void stageAvailableUpdate();
}

/**
 * Persists the channel and re-checks so the offer reflects it, unless a download is underway. A
 * staged download belongs to the previous channel, so it is discarded first: the bytes cannot be
 * installed under a channel that no longer matches them.
 */
export async function setUpdateChannel(channel: UpdateChannel): Promise<void> {
  try {
    channelStorage()?.setItem(UPDATE_CHANNEL_STORAGE_KEY, channel);
  } catch (error) {
    console.warn("Failed to persist the update channel", error);
  }
  if (status.state === "staged") {
    await sendCancel();
    pendingUpdate = null;
    setStatus({ state: "idle" });
  }
  await checkForUpdate();
}

/** Offer returned by `cmd_updater_check` (`UpdaterOffer` in ipc/updater.rs). */
type UpdateOffer = {
  version: string;
  currentVersion: string;
  date?: string;
  body?: string;
};

export type UpdateStatus = {
  state: "idle" | "checking" | "available" | "downloading" | "staged" | "downloaded" | "error";
  version?: string;
  releaseNotes?: string;
  downloadProgress?: number;
  error?: string;
  /** Why the last download of this offer ended without installing; the offer stays retryable. */
  lastFailure?: UpdateFailure;
  /** A cancel was requested; the download ends when the backend acknowledges it. */
  cancelRequested?: boolean;
  /** The downloaded update is being installed, which cannot be cancelled. */
  installing?: boolean;
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
let pendingUpdate: UpdateOffer | null = null;

function setStatus(next: UpdateStatus): void {
  status = next;
  for (const listener of statusListeners) listener(status);
}

function describe(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  // Backend IPC failures arrive as structured `{ code, message, details }` objects.
  if (typeof error === "object" && error !== null) {
    const { message } = error as { message?: unknown };
    if (typeof message === "string") return message;
  }
  return "Unknown updater failure";
}

/** `cmd_updater_install` reports this when no verified bytes are waiting (see ipc/updater.rs). */
const UPDATER_NO_PENDING_INSTALL = "UPDATER_NO_PENDING_INSTALL";

function isNoPendingInstall(error: unknown): boolean {
  if (typeof error === "object" && error !== null) {
    const { code } = error as { code?: unknown };
    if (code === UPDATER_NO_PENDING_INSTALL) return true;
  }
  return describe(error).includes(UPDATER_NO_PENDING_INSTALL);
}

export function getUpdateStatus(): UpdateStatus {
  return status;
}

let managedExternallyCache: boolean | null = null;

export async function updatesManagedExternally(): Promise<boolean> {
  if (managedExternallyCache === null) {
    try {
      managedExternallyCache = await invoke<boolean>("cmd_updater_managed_externally");
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
  // Re-checking only refreshes the pending handle, so it stays allowed while an update is already
  // offered: a release published mid-session then replaces the stale offer. It must not run while a
  // check or download is in flight, nor once an update is staged, or it would discard a completed
  // download and re-enable the install button on top of a running installer.
  if (
    status.state === "checking" ||
    status.state === "downloading" ||
    status.state === "staged" ||
    status.state === "downloaded"
  ) {
    return;
  }

  setStatus({ state: "checking" });
  try {
    // The Rust command builds the same updater as the download, so a debug endpoint override
    // applies to the offer too.
    const update = await invoke<UpdateOffer | null>("cmd_updater_check", {
      updateChannel: getUpdateChannel(),
    });
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
    if (getBackgroundUpdateDownload()) void stageAvailableUpdate();
  } catch (error) {
    pendingUpdate = null;
    setStatus({ state: "error", error: describe(error) });
  }
}

/** Progress stream of `cmd_updater_download` (`UpdaterDownloadEvent` in ipc/updater.rs). */
type UpdaterDownloadEvent =
  | { event: "started"; data: { contentLength: number | null } }
  | { event: "progress"; data: { chunkLength: number } }
  | { event: "finished" }
  | { event: "cancelled" }
  | { event: "stalled" }
  | { event: "error"; data: { message: string } };

type DownloadOutcome = "finished" | UpdateFailure | { error: string };

/** How a supervised download ended, with the cancel race already resolved. */
type DownloadResult =
  | { kind: "downloaded" }
  | { kind: "failed"; failure: UpdateFailure }
  | { kind: "error"; error: string };

/** How often a cancel the backend has not yet acted on is sent again. */
export const CANCEL_RETRY_MS = 1000;

type ActiveDownload = {
  cancelRequested: boolean;
  /** The backend reported a terminal event; nothing is left to cancel. */
  settled: boolean;
  cancelRetry: ReturnType<typeof setInterval> | null;
};

let activeDownload: ActiveDownload | null = null;

function stopCancelRetry(download: ActiveDownload): void {
  if (download.cancelRetry !== null) {
    clearInterval(download.cancelRetry);
    download.cancelRetry = null;
  }
}

async function sendCancel(): Promise<void> {
  try {
    await invoke("cmd_updater_cancel");
  } catch (error) {
    console.error("Cancelling the update download failed:", error);
  }
}

/** Runs `cmd_updater_download` under the shared supervision and reports how it ended. */
async function runDownload(update: UpdateOffer): Promise<DownloadResult> {
  const version = update.version;
  const releaseNotes = update.body ?? undefined;
  const download: ActiveDownload = {
    cancelRequested: false,
    settled: false,
    cancelRetry: null,
  };
  activeDownload = download;
  let contentLength = 0;
  let downloaded = 0;

  let resolveOutcome: (outcome: DownloadOutcome) => void = () => {};
  const outcome = new Promise<DownloadOutcome>((resolve) => {
    resolveOutcome = resolve;
  });
  const settle = (result: DownloadOutcome): void => {
    download.settled = true;
    stopCancelRetry(download);
    resolveOutcome(result);
  };
  const onEvent = new Channel<UpdaterDownloadEvent>((message) => {
    switch (message.event) {
      case "started":
        contentLength = message.data.contentLength ?? 0;
        break;
      case "progress":
        downloaded += message.data.chunkLength;
        break;
      case "finished":
        settle("finished");
        return;
      case "cancelled":
        settle("cancelled");
        return;
      case "stalled":
        settle("stalled");
        return;
      case "error":
        settle({ error: message.data.message });
        return;
    }
    // Without a Content-Length the total is unknown, so the bar stays at 0 rather than
    // reporting a fraction of an unknown whole.
    const progress = contentLength > 0 ? Math.min(downloaded / contentLength, 1) : 0;
    setStatus({
      state: "downloading",
      version,
      releaseNotes,
      downloadProgress: progress,
      ...(download.cancelRequested ? { cancelRequested: true } : {}),
    });
  });

  setStatus({ state: "downloading", version, releaseNotes, downloadProgress: 0 });
  let result: DownloadOutcome;
  try {
    await invoke("cmd_updater_download", {
      expectedVersion: version,
      onEvent,
      updateChannel: getUpdateChannel(),
    });
    result = await outcome;
  } catch (error) {
    download.settled = true;
    stopCancelRetry(download);
    activeDownload = null;
    return { kind: "error", error: describe(error) };
  }

  activeDownload = null;
  if (result === "finished" && !download.cancelRequested) {
    return { kind: "downloaded" };
  }

  if (typeof result === "object") {
    return { kind: "error", error: result.error };
  }
  // A download that completed after the user cancelled is discarded rather than installed.
  if (result === "finished") await sendCancel();
  return {
    kind: "failed",
    failure: result === "stalled" && !download.cancelRequested ? "stalled" : "cancelled",
  };
}

/**
 * Downloads the offered update without installing it, leaving the verified bytes in the backend's
 * slot. The user's restart then installs them, so a background download never restarts the app.
 */
export async function stageAvailableUpdate(): Promise<void> {
  if (!isTauri()) return;

  const update = pendingUpdate;
  if (!update || status.state !== "available" || activeDownload) return;

  const version = update.version;
  const releaseNotes = update.body ?? undefined;
  const result = await runDownload(update);
  if (result.kind === "downloaded") {
    setStatus({ state: "staged", version, releaseNotes, downloadProgress: 1 });
    return;
  }
  if (result.kind === "error") {
    setStatus({ state: "error", version, releaseNotes, error: result.error });
    return;
  }
  // The offer stays available, and is never marked dismissed, so the user can retry it and the
  // periodic check is not blocked.
  setStatus({ state: "available", version, releaseNotes, lastFailure: result.failure });
}

/** Installs the bytes a finished download left in the backend's slot. Never relaunches. */
async function installStagedUpdate(): Promise<boolean> {
  const current = status;
  if (current.state !== "staged") return false;
  const { version, releaseNotes } = current;

  setStatus({ state: "downloading", version, releaseNotes, downloadProgress: 1, installing: true });
  try {
    await invoke("cmd_updater_install");
  } catch (error) {
    if (isNoPendingInstall(error)) {
      // Another window already consumed or discarded the bytes, so the offer is downloaded again
      // rather than leaving the card dead on an install that can never succeed.
      setStatus({ state: "available", version, releaseNotes });
      return false;
    }
    setStatus({ state: "error", version, releaseNotes, error: describe(error) });
    return false;
  }
  setStatus({ state: "downloaded", version, releaseNotes, downloadProgress: 1 });
  return true;
}

/** Installs a staged update and restarts into it. */
export async function applyStagedUpdate(): Promise<void> {
  if (!isTauri()) return;
  if (await installStagedUpdate()) await relaunchApp();
}

/**
 * Downloads the offered update and installs it, without relaunching: the callers that promise a
 * restart (the settings card, the toast) call `relaunchApp` once the state reaches "downloaded".
 */
export async function downloadAndInstallUpdate(): Promise<void> {
  if (!isTauri()) return;
  await stageAvailableUpdate();
  if (status.state !== "staged") return;
  await installStagedUpdate();
}

/**
 * Cancels the running update download. Installation is not cancellable, so this does nothing once
 * the downloaded update is being installed.
 */
export async function cancelUpdateDownload(): Promise<void> {
  if (!isTauri()) return;
  const download = activeDownload;
  if (!download || download.cancelRequested) return;
  download.cancelRequested = true;
  setStatus({ ...status, cancelRequested: true });
  await sendCancel();
  // The backend ignores a cancel that arrives while it is still checking for the update, before
  // the download exists, so the cancel is repeated until the download reports how it ended.
  if (!download.settled && download.cancelRetry === null) {
    download.cancelRetry = setInterval(() => void sendCancel(), CANCEL_RETRY_MS);
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
  void updatesManagedExternally().then((managed) => {
    if (managed || pollGeneration !== generation) return;
    void checkForUpdate();
    const tick = (): void => {
      void checkForUpdate().finally(() => {
        if (pollGeneration === generation) {
          pollTimer = setTimeout(tick, intervalMs);
        }
      });
    };
    pollTimer = setTimeout(tick, intervalMs);
  });
  return stopUpdatePolling;
}
