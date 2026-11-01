import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, Check, Download, HardDrive, Loader2, RotateCcw, Trash2 } from "lucide-react";

import {
  extractBrowserErrorCode,
  getBrowserEngineStatus,
  installBrowserEngine,
  parseBrowserEngineFailure,
  removeBrowserEngine,
  rollbackBrowserEngine,
  type BrowserEngineStatus,
} from "../../lib/browserTauri";
import {
  BROWSER_ENGINE_CHANGED_EVENT,
  getBrowserEnginePaths,
  type BrowserEnginePaths,
} from "../../lib/browserExtensions";
import { getHostPlatform } from "../../lib/platform";
import { isStructuredIpcError } from "../../lib/tauri";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { SettingRow, SettingsGroup } from "./primitives";

export const BROWSER_ENGINE_INSTALL_PROGRESS_EVENT = "browser_engine_install_progress";

type InstallProgress = { receivedBytes: number; totalBytes: number | null };
type EngineAction = "install" | "remove" | "rollback";
type EngineFailure = { code: string; reason: string | null; message: string; retry: EngineAction | "status" };

/** What the user can do about each install failure reason the backend reports. */
const FAILURE_TEXT: Record<string, string> = {
  unsigned: "The engine download is not signed, so it was not installed.",
  badSignature: "The engine download's signature is not valid for this Ferryx, so it was not installed.",
  badManifest: "The engine download list is damaged. Try again later.",
  versionMismatch: "The published engine is for a different Ferryx version. Update Ferryx, then try again.",
  checksumMismatch: "The downloaded engine was damaged on the way. Try again.",
  partial: "The download stopped before it finished. Check the connection and try again.",
  oversized: "The download was larger than expected and was discarded. Try again.",
  diskFull: "Not enough disk space for the engine (about 500 MB). Free some space and try again.",
  badArchive: "The engine archive is damaged. Try again.",
  missingExecutable: "The engine archive is incomplete. Try again.",
  network: "The engine could not be downloaded. Check the connection and try again.",
  busy: "An engine install is already running.",
  noRollback: "No earlier engine is kept to go back to.",
  io: "The engine could not be written to disk.",
};

const INSTALL_ERROR_MESSAGES: Record<string, string> = {
  IO_ERROR: "Couldn't write the engine to disk",
  UNSUPPORTED_CAPABILITY: "No engine is published for this platform yet",
};

function fallbackMessage(retry: EngineFailure["retry"]): string {
  if (retry === "remove") return "The engine could not be removed";
  if (retry === "rollback") return "The earlier engine could not be restored";
  if (retry === "status") return "The engine status could not be read";
  return "The engine download failed";
}

function describeFailure(failure: unknown, retry: EngineFailure["retry"]): EngineFailure {
  // A typed install failure (details.reason) gets a sentence saying what to do about it.
  const { reason } = parseBrowserEngineFailure(failure);
  const known = reason ? FAILURE_TEXT[reason] : undefined;
  if (isStructuredIpcError(failure)) {
    const message = failure.message.trim();
    const fallback = INSTALL_ERROR_MESSAGES[failure.code] ?? fallbackMessage(retry);
    return { code: failure.code, reason, message: known ?? (message || fallback), retry };
  }
  return { code: extractBrowserErrorCode(failure), reason, message: known ?? fallbackMessage(retry), retry };
}

function engineSizes(): { download: string; disk: string } {
  return { download: "about 150 MB", disk: getHostPlatform() === "windows" ? "about 460 MB" : "about 330 MB" };
}

function percentOf(progress: InstallProgress | null): number | null {
  if (!progress || !progress.totalBytes || progress.totalBytes <= 0) return null;
  return Math.max(0, Math.min(100, Math.floor((progress.receivedBytes / progress.totalBytes) * 100)));
}

/** The Chromium engine is installed but this session cannot run it (a native Wayland window). */
function inactiveText(status: BrowserEngineStatus | null): string | null {
  if (status?.state !== "installed" || !status.inactive) return null;
  if (status.inactive === "wayland-session") {
    return "Not used in this Wayland session: Chromium tabs need an X11 window. Start Ferryx with GDK_BACKEND=x11 to use it; until then browser tabs use the system webview.";
  }
  return "Not used in this session; browser tabs use the system webview.";
}

/** Installs or removes the on-demand Chromium engine that new browser tabs use when present. */
export function BrowserEngineGroup() {
  const [status, setStatus] = useState<BrowserEngineStatus | null>(null);
  const [busyAction, setBusyAction] = useState<EngineAction | null>(null);
  const [failure, setFailure] = useState<EngineFailure | null>(null);
  const [confirmingRemove, setConfirmingRemove] = useState(false);
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [paths, setPaths] = useState<BrowserEnginePaths | null>(null);

  const loadStatus = useCallback(() => {
    setFailure(null);
    return getBrowserEngineStatus().then(setStatus, (error: unknown) => setFailure(describeFailure(error, "status")));
  }, []);

  useEffect(() => {
    let cancelled = false;
    void getBrowserEngineStatus().then(
      (next) => {
        if (!cancelled) setStatus(next);
      },
      (error: unknown) => {
        if (!cancelled) setFailure(describeFailure(error, "status"));
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  const installedState = status?.state === "installed";
  useEffect(() => {
    if (!installedState) {
      setPaths(null);
      return;
    }
    let cancelled = false;
    // The location line is informational: when the command fails it is simply not shown.
    void getBrowserEnginePaths().then(
      (next) => {
        if (!cancelled) setPaths(next);
      },
      () => {
        if (!cancelled) setPaths(null);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [installedState]);

  const installing = busyAction === "install";
  useEffect(() => {
    if (!installing) {
      setProgress(null);
      return;
    }
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    // Progress events are optional: when the subscription fails the bar stays indeterminate,
    // which is still an accurate picture of an install in flight.
    void listen<InstallProgress>(BROWSER_ENGINE_INSTALL_PROGRESS_EVENT, (event) => setProgress(event.payload)).then(
      (stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      },
      () => undefined,
    );
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [installing]);

  if (status?.state === "unsupported") return null;

  const run = (action: EngineAction) => {
    setBusyAction(action);
    setFailure(null);
    setConfirmingRemove(false);
    const operation =
      action === "install" ? installBrowserEngine : action === "remove" ? removeBrowserEngine : rollbackBrowserEngine;
    void operation()
      .then(
        (next) => {
          setStatus(next);
          window.dispatchEvent(new CustomEvent(BROWSER_ENGINE_CHANGED_EVENT));
        },
        (error: unknown) => setFailure(describeFailure(error, action)),
      )
      .finally(() => setBusyAction(null));
  };

  const retry = () => {
    if (!failure) return;
    if (failure.retry === "status") void loadStatus();
    else run(failure.retry);
  };

  const percent = percentOf(progress);
  const sizes = engineSizes();
  const stateKey = busyAction === "install"
    ? "downloading"
    : busyAction === "remove"
      ? "removing"
      : busyAction === "rollback"
        ? "switching"
        : status == null
        ? "checking"
        : status.state;
  const stateLabel =
    stateKey === "downloading"
      ? "Downloading..."
      : stateKey === "removing"
        ? "Removing..."
        : stateKey === "switching"
          ? "Switching..."
          : stateKey === "installed" && status?.state === "installed"
          ? `Installed ${status.version}`
          : stateKey === "checking"
            ? "Checking..."
            : "Not installed";
  const busy = busyAction !== null;
  const installed = status?.state === "installed";
  const previous = status?.state === "installed" ? status.previous ?? null : null;
  const inactive = inactiveText(status);

  return (
    <div id="browser-engine" className="scroll-mt-4">
      <SettingsGroup title="Chromium Engine">
        <SettingRow
          label="Full browser engine"
          description="Adds Chrome Web Store extensions, Chrome's own menus and password manager, and shared logins across a profile's tabs. New tabs use it; tabs that are already open keep their engine. Without it, tabs use the system webview."
        >
          <div className="flex items-center gap-2">
            <Badge
              variant={installed ? "secondary" : "outline"}
              className="max-w-[200px] gap-1 text-[11px] font-medium"
              data-testid="browser-engine-state"
              data-state={stateKey}
              title={stateLabel}
            >
              {busy || stateKey === "checking" ? (
                <Loader2 className="size-3 shrink-0 animate-spin" aria-hidden="true" />
              ) : installed ? (
                <Check className="size-3 shrink-0" aria-hidden="true" />
              ) : null}
              <span className="truncate">{stateLabel}</span>
            </Badge>
            {previous ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={busy}
                onClick={() => run("rollback")}
                className="h-7 px-2 text-[11px]"
                title="Go back to the engine that was installed before this one"
                data-testid="browser-engine-rollback"
              >
                {busyAction === "rollback" ? "Switching..." : `Use ${previous}`}
              </Button>
            ) : null}
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy || status == null}
              onClick={() => (installed ? setConfirmingRemove(true) : run("install"))}
              className="h-7 gap-1.5 px-2 text-[11px]"
              data-testid="browser-engine-action"
            >
              {installed ? (
                <Trash2 className="size-3.5" aria-hidden="true" />
              ) : (
                <Download className="size-3.5" aria-hidden="true" />
              )}
              {installed ? "Remove" : "Download"}
            </Button>
          </div>
        </SettingRow>

        {inactive ? (
          <p className="border-b border-border/40 py-2 text-[11px] text-muted-foreground" data-testid="browser-engine-inactive">
            {inactive}
          </p>
        ) : null}

        <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 border-b border-border/40 py-3 text-[11px]">
          <dt className="text-muted-foreground">Download size</dt>
          <dd className="text-foreground" data-testid="browser-engine-download-size">{sizes.download}</dd>
          <dt className="text-muted-foreground">Disk use</dt>
          <dd className="text-foreground" data-testid="browser-engine-disk-size">{sizes.disk}</dd>
          {installed && paths ? (
            <>
              <dt className="text-muted-foreground">Location</dt>
              <dd
                className="flex min-w-0 items-center gap-1.5 text-foreground"
                data-testid="browser-engine-location"
              >
                <HardDrive className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
                <span className="truncate" title={paths.installDir}>{paths.installDir}</span>
              </dd>
            </>
          ) : null}
        </dl>

        {installing ? (
          <div className="space-y-1 py-3" data-testid="browser-engine-progress">
            <div
              role="progressbar"
              aria-label="Engine download progress"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={percent ?? undefined}
              className="h-1.5 w-full overflow-hidden rounded-full bg-muted"
            >
              <div
                className={percent === null ? "h-full w-1/3 animate-pulse rounded-full bg-primary" : "h-full rounded-full bg-primary transition-[width]"}
                style={percent === null ? undefined : { width: `${percent}%` }}
              />
            </div>
            {percent !== null ? (
              <p className="text-[11px] text-muted-foreground" data-testid="browser-engine-percent">{percent}%</p>
            ) : null}
          </div>
        ) : null}

        {confirmingRemove ? (
          <div
            className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-border bg-muted/40 px-3 py-2"
            data-testid="browser-engine-remove-confirm"
          >
            <p className="min-w-0 text-[12px] text-foreground">
              Remove engine? Open Chromium tabs keep running until closed.
            </p>
            <div className="flex items-center gap-2">
              <Button
                type="button"
                variant="destructive"
                size="sm"
                className="h-7 px-2 text-[11px]"
                onClick={() => run("remove")}
                data-testid="browser-engine-remove-confirm-yes"
              >
                Remove
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-7 px-2 text-[11px]"
                onClick={() => setConfirmingRemove(false)}
                data-testid="browser-engine-remove-confirm-cancel"
              >
                Cancel
              </Button>
            </div>
          </div>
        ) : null}

        {failure ? (
          <div
            role="alert"
            data-testid="browser-engine-failure"
            data-reason={failure.reason ?? undefined}
            className="mt-3 flex items-start justify-between gap-3 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2"
          >
            <div className="flex min-w-0 items-start gap-2">
              <AlertCircle className="mt-0.5 size-3.5 shrink-0 text-destructive" aria-hidden="true" />
              <div className="min-w-0">
                <p className="text-[12px] text-foreground" data-testid="browser-engine-error-message">{failure.message}</p>
                <p className="truncate text-[11px] text-muted-foreground" title={failure.code} data-testid="browser-engine-error-code">
                  {failure.code}
                </p>
              </div>
            </div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={retry}
              className="h-7 shrink-0 gap-1.5 px-2 text-[11px]"
              data-testid="browser-engine-retry"
            >
              <RotateCcw className="size-3.5" aria-hidden="true" />
              Retry
            </Button>
          </div>
        ) : null}
      </SettingsGroup>
    </div>
  );
}
