import { useEffect, useState } from "react";

import {
  getBrowserEngineStatus,
  installBrowserEngine,
  parseBrowserEngineFailure,
  removeBrowserEngine,
  rollbackBrowserEngine,
  type BrowserEngineFailure,
  type BrowserEngineStatus,
} from "../../lib/browserTauri";
import { Button } from "../ui/button";
import { SettingRow, SettingsGroup } from "./primitives";

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

function failureText(failure: BrowserEngineFailure): string {
  const known = failure.reason ? FAILURE_TEXT[failure.reason] : undefined;
  if (known) return known;
  return failure.message ? `${failure.code}: ${failure.message}` : failure.code;
}

type Action = "install" | "remove" | "rollback";

/** Installs, updates, rolls back or removes the on-demand Chromium engine that new browser tabs use. */
export function BrowserEngineGroup() {
  const [status, setStatus] = useState<BrowserEngineStatus | null>(null);
  const [busy, setBusy] = useState<Action | null>(null);
  const [failure, setFailure] = useState<{ action: Action; failure: BrowserEngineFailure } | null>(null);

  useEffect(() => {
    let cancelled = false;
    void getBrowserEngineStatus().then(
      (next) => {
        if (!cancelled) setStatus(next);
      },
      (error: unknown) => {
        if (!cancelled) setFailure({ action: "install", failure: parseBrowserEngineFailure(error) });
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  if (status?.state === "unsupported") return null;

  const run = (action: Action) => {
    const call = action === "install" ? installBrowserEngine : action === "remove" ? removeBrowserEngine : rollbackBrowserEngine;
    setBusy(action);
    setFailure(null);
    void call()
      .then(setStatus, (error: unknown) => setFailure({ action, failure: parseBrowserEngineFailure(error) }))
      .finally(() => setBusy(null));
  };

  const installed = status?.state === "installed" ? status : null;
  const label = busy === "install" ? "Downloading..." : busy === "remove" ? "Removing..." : installed ? "Remove" : "Download";
  return (
    <SettingsGroup title="Chromium Engine">
      <SettingRow
        label="Full browser engine"
        description={
          installed
            ? `Installed (${installed.version}). New browser tabs use Chromium, with Chrome Web Store extensions, saved passwords and Chrome's own menus.`
            : "Download Chromium so browser tabs run Chrome Web Store extensions, save passwords and play protected video. Without it, tabs use the system webview."
        }
      >
        <div className="flex flex-col items-end gap-1">
          <div className="flex gap-1">
            {installed?.previous ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={busy !== null}
                onClick={() => run("rollback")}
                className="h-7 px-2 text-[11px]"
                data-testid="browser-engine-rollback"
              >
                {busy === "rollback" ? "Switching..." : `Use ${installed.previous}`}
              </Button>
            ) : null}
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy !== null || status === null}
              onClick={() => run(installed ? "remove" : "install")}
              className="h-7 px-2 text-[11px]"
              data-testid="browser-engine-action"
            >
              {label}
            </Button>
          </div>
          {failure ? (
            <div className="flex max-w-xs items-center gap-2" data-testid="browser-engine-failure" data-reason={failure.failure.reason ?? undefined}>
              <span role="alert" className="text-[11px] text-destructive">
                {failureText(failure.failure)}
              </span>
              {failure.action === "install" && status !== null ? (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={busy !== null}
                  onClick={() => run("install")}
                  className="h-6 px-2 text-[11px]"
                  data-testid="browser-engine-retry"
                >
                  Retry
                </Button>
              ) : null}
            </div>
          ) : null}
        </div>
      </SettingRow>
    </SettingsGroup>
  );
}
