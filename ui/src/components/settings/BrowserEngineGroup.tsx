import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, Check, Download, HardDrive, Loader2, RotateCcw, Trash2 } from "lucide-react";

import {
  extractBrowserErrorCode,
  getBrowserEngineStatus,
  installBrowserEngine,
  removeBrowserEngine,
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
type EngineAction = "install" | "remove";
type EngineFailure = { code: string; message: string; retry: EngineAction | "status" };

const INSTALL_ERROR_MESSAGES: Record<string, string> = {
  IO_ERROR: "Couldn't write the engine to disk",
  UNSUPPORTED_CAPABILITY: "No engine is published for this platform yet",
};

function describeFailure(failure: unknown, retry: EngineFailure["retry"]): EngineFailure {
  if (isStructuredIpcError(failure)) {
    const message = failure.message.trim();
    const fallback =
      INSTALL_ERROR_MESSAGES[failure.code] ??
      (retry === "remove" ? "The engine could not be removed" : retry === "status" ? "The engine status could not be read" : "The engine download failed");
    return { code: failure.code, message: message || fallback, retry };
  }
  const fallback =
    retry === "remove" ? "The engine could not be removed" : retry === "status" ? "The engine status could not be read" : "The engine download failed";
  return { code: extractBrowserErrorCode(failure), message: fallback, retry };
}

function engineSizes(): { download: string; disk: string } {
  return { download: "about 150 MB", disk: getHostPlatform() === "windows" ? "about 460 MB" : "about 330 MB" };
}

function percentOf(progress: InstallProgress | null): number | null {
  if (!progress || !progress.totalBytes || progress.totalBytes <= 0) return null;
  return Math.max(0, Math.min(100, Math.floor((progress.receivedBytes / progress.totalBytes) * 100)));
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
    const operation = action === "install" ? installBrowserEngine : removeBrowserEngine;
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
      : status == null
        ? "checking"
        : status.state;
  const stateLabel =
    stateKey === "downloading"
      ? "Downloading..."
      : stateKey === "removing"
        ? "Removing..."
        : stateKey === "installed" && status?.state === "installed"
          ? `Installed ${status.version}`
          : stateKey === "checking"
            ? "Checking..."
            : "Not installed";
  const busy = busyAction !== null;
  const installed = status?.state === "installed";

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
