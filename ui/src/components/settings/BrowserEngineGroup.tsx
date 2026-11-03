import { useEffect, useState } from "react";

import {
  extractBrowserErrorCode,
  getBrowserEngineStatus,
  installBrowserEngine,
  removeBrowserEngine,
  type BrowserEngineStatus,
} from "../../lib/browserTauri";
import { Button } from "../ui/button";
import { SettingRow, SettingsGroup } from "./primitives";

/** Installs or removes the on-demand Chromium engine that new browser tabs use when present. */
export function BrowserEngineGroup() {
  const [status, setStatus] = useState<BrowserEngineStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void getBrowserEngineStatus().then(
      (next) => {
        if (!cancelled) setStatus(next);
      },
      (failure: unknown) => {
        if (!cancelled) setError(extractBrowserErrorCode(failure));
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  if (status?.state === "unsupported") return null;

  const run = (action: () => Promise<BrowserEngineStatus>) => {
    setBusy(true);
    setError(null);
    void action()
      .then(setStatus, (failure: unknown) => setError(extractBrowserErrorCode(failure)))
      .finally(() => setBusy(false));
  };

  const installed = status?.state === "installed";
  return (
    <SettingsGroup title="Chromium Engine">
      <SettingRow
        label="Full browser engine"
        description={
          status?.state === "installed"
            ? `Installed (${status.version}). New browser tabs use Chromium, with Chrome Web Store extensions, saved passwords and Chrome's own menus.`
            : "Download Chromium so browser tabs run Chrome Web Store extensions, save passwords and play protected video. Without it, tabs use the system webview."
        }
      >
        <div className="flex flex-col items-end gap-1">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={busy || status === null}
            onClick={() => run(installed ? removeBrowserEngine : installBrowserEngine)}
            className="h-7 px-2 text-[11px]"
            data-testid="browser-engine-action"
          >
            {busy ? (installed ? "Removing..." : "Downloading...") : installed ? "Remove" : "Download"}
          </Button>
          {error ? (
            <span role="alert" className="text-[11px] text-destructive">
              {error}
            </span>
          ) : null}
        </div>
      </SettingRow>
    </SettingsGroup>
  );
}
