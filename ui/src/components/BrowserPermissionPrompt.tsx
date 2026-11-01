import { useEffect, useState } from "react";
import { Camera, CircleAlert, Mic, MonitorUp, ShieldQuestion } from "lucide-react";

import {
  decideBrowserPermission,
  describeSitePermissionKinds,
  getBrowserMediaCapture,
  getPendingBrowserPermissions,
  onBrowserMediaCapture,
  onBrowserPermissionPrompt,
  onBrowserPermissionPromptClosed,
  sitePermissionErrorCode as extractBrowserErrorCode,
  type BrowserMediaCapture,
  type BrowserPermissionPrompt,
} from "../lib/browserSitePermissions";

/**
 * The browser pane's site permission prompt: which site, what it wants, Allow or Block, and
 * whether to remember the answer. Requests the user never answers stay denied by the host.
 */
export function BrowserPermissionPrompts({ browserId }: { browserId: string }) {
  const [prompts, setPrompts] = useState<BrowserPermissionPrompt[]>([]);
  const [remember, setRemember] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setPrompts([]);
    let cancelled = false;
    void getPendingBrowserPermissions(browserId).then(
      (pending) => {
        if (!cancelled) {
          setPrompts((current) => {
            const known = new Set(current.map((prompt) => prompt.requestId));
            return [...pending.filter((prompt) => !known.has(prompt.requestId)), ...current];
          });
        }
      },
      (failure: unknown) => {
        if (!cancelled) setError(extractBrowserErrorCode(failure));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [browserId]);

  useEffect(() => {
    let disposed = false;
    const cleanups: (() => void)[] = [];
    const keep = (unlisten: () => void) => {
      if (disposed) unlisten();
      else cleanups.push(unlisten);
    };
    const fail = (failure: unknown) => {
      if (!disposed) setError(extractBrowserErrorCode(failure));
    };
    void onBrowserPermissionPrompt((prompt) => {
      if (prompt.browserId !== browserId) return;
      setPrompts((current) =>
        current.some((existing) => existing.requestId === prompt.requestId) ? current : [...current, prompt],
      );
    }).then(keep, fail);
    void onBrowserPermissionPromptClosed((closed) => {
      if (closed.browserId !== browserId) return;
      setPrompts((current) => current.filter((prompt) => prompt.requestId !== closed.requestId));
    }).then(keep, fail);
    return () => {
      disposed = true;
      cleanups.forEach((unlisten) => unlisten());
    };
  }, [browserId]);

  const prompt = prompts[0];
  if (!prompt) {
    return error ? (
      <div role="alert" className="border-b border-destructive/20 bg-destructive/10 px-3 py-1.5 text-xs text-destructive">
        Site permission prompts unavailable: {error}
      </div>
    ) : null;
  }

  const answer = (allow: boolean) => {
    const keep = prompt.canRemember && remember;
    setPrompts((current) => current.filter((pending) => pending.requestId !== prompt.requestId));
    void decideBrowserPermission(prompt.requestId, allow, keep).then(
      () => setError(null),
      (failure: unknown) => setError(extractBrowserErrorCode(failure)),
    );
  };

  return (
    <div
      role="alertdialog"
      aria-label="Site permission request"
      data-testid="browser-permission-prompt"
      className="flex flex-wrap items-center gap-2 border-b border-border bg-popover px-3 py-1.5 text-[11px]"
    >
      <ShieldQuestion className="size-3.5 shrink-0" />
      <span className="min-w-0 flex-1">
        <span className="font-mono">{prompt.origin}</span> wants to use{" "}
        <strong>{describeSitePermissionKinds(prompt.kinds).toLowerCase()}</strong>
        {prompts.length > 1 ? <span className="text-muted-foreground"> (+{prompts.length - 1} more)</span> : null}
      </span>
      {prompt.canRemember ? (
        <label className="flex items-center gap-1 text-muted-foreground">
          <input
            type="checkbox"
            aria-label="Remember this decision"
            checked={remember}
            onChange={(event) => setRemember(event.target.checked)}
            className="size-3.5 accent-foreground"
          />
          Remember
        </label>
      ) : null}
      <button type="button" onClick={() => answer(false)} className="rounded border border-border px-2 py-1 hover:bg-accent">
        Block
      </button>
      <button
        type="button"
        onClick={() => answer(true)}
        className="rounded border border-border bg-foreground px-2 py-1 text-background hover:opacity-90"
      >
        Allow
      </button>
      {error ? <span className="text-destructive">{error}</span> : null}
    </div>
  );
}

const EMPTY_CAPTURE = { camera: false, microphone: false, screen: false };

/** Camera, microphone and screen-sharing icons while the tab's page uses them. */
export function BrowserMediaCaptureIndicator({ browserId }: { browserId: string }) {
  const [capture, setCapture] = useState<Pick<BrowserMediaCapture, "camera" | "microphone" | "screen"> & { origin?: string | null }>(
    EMPTY_CAPTURE,
  );
  // Without capture state the indicator cannot vouch that nothing is recording, so it says so.
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    setCapture(EMPTY_CAPTURE);
    setFailure(null);
    let cancelled = false;
    void getBrowserMediaCapture(browserId).then(
      (current) => {
        // An IPC result without capture state (older host, unmocked invoke) means nothing known in use.
        if (!cancelled) setCapture(current ?? EMPTY_CAPTURE);
      },
      (error: unknown) => {
        if (!cancelled) setFailure(extractBrowserErrorCode(error));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [browserId]);

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void onBrowserMediaCapture((next) => {
      if (next.browserId === browserId) setCapture(next);
    }).then(
      (unlisten) => {
        if (disposed) unlisten();
        else cleanup = unlisten;
      },
      (error: unknown) => {
        if (!disposed) setFailure(extractBrowserErrorCode(error));
      },
    );
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [browserId]);

  if (failure) {
    return (
      <CircleAlert
        className="size-3.5 text-amber-500"
        data-testid="browser-media-capture-unknown"
        aria-label={`Camera and microphone use unknown: ${failure}`}
      />
    );
  }
  if (!capture.camera && !capture.microphone && !capture.screen) return null;
  const site = capture.origin ? ` by ${capture.origin}` : "";
  return (
    <div className="flex items-center gap-1 text-red-500" data-testid="browser-media-capture-indicator">
      {capture.camera ? <Camera className="size-3.5" aria-label={`Camera in use${site}`} /> : null}
      {capture.microphone ? <Mic className="size-3.5" aria-label={`Microphone in use${site}`} /> : null}
      {capture.screen ? <MonitorUp className="size-3.5" aria-label={`Screen shared${site}`} /> : null}
    </div>
  );
}
