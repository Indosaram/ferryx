import { useCallback, useEffect, useState } from "react";

import {
  extractBrowserErrorCode,
  getDefaultBrowserStatus,
  makeDefaultBrowser,
  type DefaultBrowserStatus,
} from "../../lib/browserTauri";
import { Button } from "../ui/button";
import { SettingRow, SettingsGroup } from "./primitives";

function describe(status: DefaultBrowserStatus | null, note: string | null): string {
  if (note) return note;
  if (!status) return "Checking...";
  if (status.state === "default") return "Ferryx is your default browser. Links from other apps open in a Ferryx tab.";
  if (status.state === "notDefault") {
    // The host names only browsers it recognizes; anything else is "another app", never a raw id.
    const current = status.current?.trim() || "another app";
    return `Links from other apps open in ${current}. Make Ferryx the default to open them in a Ferryx tab.`;
  }
  return "Make Ferryx the default browser so links from other apps open in a Ferryx tab.";
}

/** "Default browser" row: whether Ferryx handles http(s) links, and the button that asks the OS. */
export function DefaultBrowserGroup() {
  const [status, setStatus] = useState<DefaultBrowserStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const refresh = useCallback(() => {
    return getDefaultBrowserStatus().then(setStatus, (failure: unknown) => setError(extractBrowserErrorCode(failure)));
  }, []);

  useEffect(() => {
    let cancelled = false;
    void getDefaultBrowserStatus().then(
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

  const makeDefault = () => {
    setBusy(true);
    setError(null);
    setNote(null);
    void makeDefaultBrowser()
      .then((outcome) => {
        if (outcome.outcome === "openedSystemSettings") {
          setNote("Choose Ferryx as the web browser in the system settings that just opened.");
        }
        return refresh();
      }, (failure: unknown) => setError(extractBrowserErrorCode(failure)))
      .finally(() => setBusy(false));
  };

  const isDefault = status?.state === "default";
  return (
    <SettingsGroup title="Default Browser">
      <SettingRow label="Default browser" description={describe(status, note)}>
        <div className="flex flex-col items-end gap-1">
          <span className="text-[11px] text-muted-foreground" data-testid="default-browser-status">
            {!status ? "..." : isDefault ? "Default" : "Not default"}
          </span>
          {isDefault ? null : (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy || !status}
              onClick={makeDefault}
              className="h-7 px-2 text-[11px]"
              data-testid="default-browser-action"
            >
              {busy ? "Asking..." : "Make Ferryx the default browser"}
            </Button>
          )}
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
