import { useEffect, useState } from "react";
import { ShieldQuestion } from "lucide-react";

import { extractBrowserErrorCode } from "../lib/browserTauri";
import {
  controllerName,
  describeScopes,
  getBrowserAgentOverview,
  onBrowserAgentApprovals,
  resolveBrowserAgentApproval,
  type ApprovalResolution,
  type BrowserAgentApproval,
  type BrowserAgentController,
} from "../lib/browserAgent";
import { Button } from "./ui/button";

/**
 * Consent card for browser automation: shows the oldest pending pairing, origin grant or
 * one-time capability request and resolves it. It sits at the top of the app chrome, above
 * the tab bar, so a Chromium engine window over a browser pane never hides it.
 */
export function BrowserAgentApprovalCard() {
  const [pending, setPending] = useState<BrowserAgentApproval[]>([]);
  const [controllers, setControllers] = useState<BrowserAgentController[]>([]);
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    // The change event carries the authoritative pending list; the overview read only seeds
    // the list until the first event and keeps controller names current.
    let heardEvent = false;
    const refresh = () =>
      getBrowserAgentOverview().then(
        (overview) => {
          if (cancelled) return;
          if (!heardEvent) setPending(overview.pending);
          setControllers(overview.controllers);
        },
        (failure: unknown) => {
          if (!cancelled) setError(extractBrowserErrorCode(failure));
        },
      );
    // Subscribe before the first read so a request arriving in between is not missed.
    const unlisten = onBrowserAgentApprovals((next) => {
      if (cancelled) return;
      heardEvent = true;
      setPending(next);
      // A pairing just approved adds a controller name the card needs for later requests.
      void refresh();
    });
    void refresh();
    return () => {
      cancelled = true;
      void unlisten.then((stop) => stop());
    };
  }, []);

  const current = pending[0];
  useEffect(() => {
    setRemember(false);
    setError(null);
  }, [current?.id]);

  if (!current) return null;

  const resolve = (resolution: ApprovalResolution) => {
    setBusy(true);
    setError(null);
    void resolveBrowserAgentApproval(current.id, resolution)
      .then(
        // The backend's changed event normally replaces the list; drop the card at once anyway.
        () => setPending((list) => list.filter((item) => item.id !== current.id)),
        (failure: unknown) => setError(extractBrowserErrorCode(failure)),
      )
      .finally(() => setBusy(false));
  };

  const { title, detail } = describeApproval(current, controllers);

  return (
    <div
      role="alertdialog"
      aria-labelledby="browser-agent-approval-title"
      data-testid="browser-agent-approval"
      className="fixed left-1/2 top-2 z-[60] w-[min(440px,calc(100vw-24px))] -translate-x-1/2 rounded-lg border border-border bg-popover p-3 text-popover-foreground shadow-lg"
    >
      <div className="flex items-start gap-2">
        <ShieldQuestion className="mt-0.5 h-4 w-4 shrink-0 text-primary" aria-hidden />
        <div className="min-w-0 flex-1">
          <p id="browser-agent-approval-title" className="break-words text-[13px] font-medium">
            {title}
          </p>
          <p className="mt-1 text-[12px] text-muted-foreground">{detail}</p>
          {current.kind === "originGrant" ? (
            <label className="mt-2 flex items-center gap-2 text-[12px]">
              <input
                type="checkbox"
                checked={remember}
                onChange={(event) => setRemember(event.target.checked)}
                data-testid="browser-agent-approval-remember"
              />
              Keep this permission after Ferryx restarts
            </label>
          ) : null}
          {pending.length > 1 ? (
            <p className="mt-1 text-[11px] text-muted-foreground">{pending.length - 1} more waiting</p>
          ) : null}
          {error ? (
            <p role="alert" className="mt-1 text-[11px] text-destructive">
              {error}
            </p>
          ) : null}
          <div className="mt-3 flex justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={() => resolve("deny")}
              className="h-7 px-3 text-[12px]"
            >
              Deny
            </Button>
            <Button
              type="button"
              size="sm"
              disabled={busy}
              onClick={() => resolve(remember ? "approvePersistent" : "approveOnce")}
              className="h-7 px-3 text-[12px]"
            >
              {current.kind === "pairController" ? "Allow" : "Approve"}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

function describeApproval(
  approval: BrowserAgentApproval,
  controllers: BrowserAgentController[],
): { title: string; detail: string } {
  switch (approval.kind) {
    case "pairController":
      return {
        title: `Allow "${approval.label}" to control the browser?`,
        detail: "A local program asks to pair as a browser automation controller. It gets no page access until you grant sites.",
      };
    case "originGrant":
      return {
        title: `Let ${controllerName(controllers, approval.controller)} use ${approval.origin}?`,
        detail: `It could ${describeScopes(approval.scopes)} on this site only.`,
      };
    case "fileCapability":
    case "nativeCapability":
      return {
        title: `Let ${controllerName(controllers, approval.controller)} run ${approval.tool} once?`,
        detail: "This is a one-time permission for the next call only.",
      };
  }
}
