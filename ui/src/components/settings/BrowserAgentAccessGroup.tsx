import { useCallback, useEffect, useState } from "react";

import { extractBrowserErrorCode } from "../../lib/browserTauri";
import {
  describeScopes,
  getBrowserAgentOverview,
  onBrowserAgentApprovals,
  revokeBrowserAgent,
  setBrowserAgentSettings,
  type BrowserAgentOverview,
} from "../../lib/browserAgent";
import { Button } from "../ui/button";
import { Switch } from "../ui/switch";
import { SettingRow, SettingsGroup } from "./primitives";

/** Settings > Browser > Agent access: paired controllers, their site grants, and revocation. */
export function BrowserAgentAccessGroup() {
  const [overview, setOverview] = useState<BrowserAgentOverview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(
    () =>
      getBrowserAgentOverview().then(setOverview, (failure: unknown) => setError(extractBrowserErrorCode(failure))),
    [],
  );

  useEffect(() => {
    const unlisten = onBrowserAgentApprovals(() => void refresh());
    void refresh();
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [refresh]);

  const run = (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    void action()
      .then(refresh, (failure: unknown) => setError(extractBrowserErrorCode(failure)))
      .finally(() => setBusy(false));
  };

  const controllers = overview?.controllers.filter((controller) => !controller.revoked) ?? [];
  return (
    <SettingsGroup title="Agent access">
      <SettingRow
        label="Require paired agents"
        description="Browser commands from agents and scripts must come from a controller you allowed. Pair one with `ferryx browser controllers pair --name <label>` and approve the card that appears."
      >
        <Switch
          aria-label="Require paired agents"
          checked={overview?.settings.requirePairing ?? true}
          disabled={busy || overview === null}
          onCheckedChange={(checked) => run(() => setBrowserAgentSettings({ requirePairing: checked }))}
        />
      </SettingRow>
      {controllers.length === 0 ? (
        <p className="px-3 py-2 text-[12px] text-muted-foreground" data-testid="browser-agent-none">
          No agents are paired.
        </p>
      ) : (
        controllers.map((controller) => (
          <div key={controller.id} className="border-t border-border px-3 py-2" data-testid="browser-agent-controller">
            <div className="flex items-center justify-between gap-2">
              <span className="truncate text-[13px] font-medium">{controller.label}</span>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={busy}
                onClick={() => run(() => revokeBrowserAgent(controller.id))}
                className="h-7 px-2 text-[11px]"
                aria-label={`Remove ${controller.label}`}
              >
                Remove
              </Button>
            </div>
            {controller.grants.length === 0 ? (
              <p className="mt-1 text-[11px] text-muted-foreground">No sites allowed.</p>
            ) : (
              <ul className="mt-1 space-y-1">
                {controller.grants.map((grant) => (
                  <li key={grant.origin} className="flex items-center justify-between gap-2 text-[12px]">
                    <span className="min-w-0 truncate">
                      {grant.origin}
                      <span className="text-muted-foreground">
                        {" "}
                        ({describeScopes(grant.scopes)}
                        {grant.persistent ? ", kept" : ", this session"})
                      </span>
                    </span>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      disabled={busy}
                      onClick={() => run(() => revokeBrowserAgent(controller.id, grant.origin))}
                      className="h-6 px-2 text-[11px]"
                      aria-label={`Revoke ${grant.origin} for ${controller.label}`}
                    >
                      Revoke access
                    </Button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        ))
      )}
      {error ? (
        <p role="alert" className="px-3 pb-2 text-[11px] text-destructive">
          {error}
        </p>
      ) : null}
    </SettingsGroup>
  );
}
