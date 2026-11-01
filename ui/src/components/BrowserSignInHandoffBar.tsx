import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import {
  SIGN_IN_HANDOFF_EVENT,
  getSignInHandoffPlan,
  installBrowserEngine,
  openExternalUrl,
  parseBrowserEngineFailure,
  runSignInAuthSession,
  type HandoffAction,
  type HandoffPlan,
  type SignInHandoffDetail,
  type SignInProvider,
} from "../lib/browserTauri";
import { BROWSER_ENGINE_CHANGED_EVENT } from "../lib/browserExtensions";
import type { BrowserTab } from "../lib/types";

/** Emitted by the backend while the Chromium engine downloads. */
const INSTALL_PROGRESS_EVENT = "browser_engine_install_progress";

type InstallProgress = { receivedBytes: number; totalBytes: number | null };

const IDP_HOSTS = new Set([
  "accounts.google.com",
  "appleid.apple.com",
  "idmsa.apple.com",
  "login.microsoftonline.com",
  "login.live.com",
]);
const IDP_HOST_SUFFIXES = [".okta.com", ".oktapreview.com"];

const PROVIDER_NAME: Record<SignInProvider, string> = {
  google: "Google",
  apple: "Apple",
  microsoft: "Microsoft",
  okta: "Okta",
};

const ACTION_LABEL: Record<HandoffAction["kind"], string> = {
  chromiumTab: "Sign in with Chromium engine",
  installEngine: "Install Chromium engine and sign in",
  systemAuthSession: "Sign in with macOS",
  externalBrowser: "Open in default browser",
};

/** The identity provider host of `url`, or null when it is not a known sign-in host. */
function idpHostOf(url: string): string | null {
  let host: string;
  try {
    host = new URL(url).hostname.toLowerCase();
  } catch {
    return null;
  }
  if (IDP_HOSTS.has(host) || IDP_HOST_SUFFIXES.some((suffix) => host.endsWith(suffix))) return host;
  return null;
}

function errorText(err: unknown): string {
  if (err && typeof err === "object" && typeof (err as { message?: unknown }).message === "string") {
    const message = ((err as { message: string }).message).trim();
    if (message) return message;
  }
  return String(err);
}

function progressText(progress: InstallProgress | null): string {
  if (!progress?.totalBytes) return "Installing Chromium engine...";
  const percent = Math.min(100, Math.floor((progress.receivedBytes / progress.totalBytes) * 100));
  return `Installing Chromium engine... ${percent}%`;
}

/**
 * Shown in a system-webview tab whose identity provider refused sign-in ("This browser or app may
 * not be secure", "unsupported browser"). Offers the backend's in-app actions first: reopen the
 * sign-in in a Chromium tab of the same profile, install the Chromium engine and then do so, or
 * sign in through the macOS authentication session. Opening the default browser is only offered
 * after "Not now", unless no in-app action exists.
 */
export function BrowserSignInHandoffBar({ tab }: { tab: BrowserTab }) {
  const [plan, setPlan] = useState<HandoffPlan | null>(null);
  // The primary action first, then the alternatives still worth trying.
  const [offered, setOffered] = useState<HandoffAction[]>([]);
  const [declined, setDeclined] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [busy, setBusy] = useState<"install" | "authSession" | null>(null);
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dismissedUrl, setDismissedUrl] = useState<string | null>(null);
  // Bumped on every plan request so a late plan of an earlier page is ignored.
  const generation = useRef(0);
  // The page an action finishes on; results for an earlier page are ignored.
  const currentUrl = useRef(tab.url);
  currentUrl.current = tab.url;
  const systemEngine = tab.engine !== "chromium";
  const idpHost = systemEngine ? idpHostOf(tab.url) : null;

  useEffect(() => {
    generation.current += 1;
    const current = generation.current;
    setPlan(null);
    setOffered([]);
    setDeclined(false);
    setMenuOpen(false);
    setError(null);
    if (!idpHost) return;
    void getSignInHandoffPlan(tab.browserId, tab.url, tab.title).then(
      (next) => {
        if (generation.current !== current) return;
        setPlan(next);
        setOffered(next ? [next.primary, ...next.alternatives] : []);
      },
      () => {
        // Detection is advisory: without an answer the tab simply shows no bar.
        if (generation.current === current) setPlan(null);
      },
    );
  }, [idpHost, tab.browserId, tab.url, tab.title]);

  useEffect(() => {
    if (busy !== "install") {
      setProgress(null);
      return;
    }
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    // Progress is optional: without it the text stays indeterminate.
    void listen<InstallProgress>(INSTALL_PROGRESS_EVENT, (event) => setProgress(event.payload)).then(
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
  }, [busy]);

  if (!idpHost || !plan || dismissedUrl === tab.url) return null;

  const host: string = idpHost;
  const shownPlan: HandoffPlan = plan;
  const pageUrl = tab.url;
  const hide = () => setDismissedUrl(pageUrl);
  const stale = () => currentUrl.current !== pageUrl;

  /** Shows why `failed` did not work and offers the next action, or the decline menu. */
  const fail = (failed: HandoffAction, message: string) => {
    setError(message);
    // Actions are not offered while one runs, so `offered` is current here.
    const rest = offered.filter((action) => action !== failed);
    setOffered(rest);
    if (rest.length === 0) setDeclined(true);
  };

  const openChromiumTab = (url: string) => {
    const detail: SignInHandoffDetail = {
      url,
      profileId: tab.profileId,
      browserId: `signin:${crypto.randomUUID()}`,
      idpHost: host,
    };
    window.dispatchEvent(new CustomEvent<SignInHandoffDetail>(SIGN_IN_HANDOFF_EVENT, { detail }));
    hide();
  };

  const perform = (action: HandoffAction) => {
    setError(null);
    setMenuOpen(false);
    switch (action.kind) {
      case "chromiumTab":
        openChromiumTab(action.url);
        return;
      case "installEngine":
        setBusy("install");
        void installBrowserEngine()
          .then(
            (status) => {
              window.dispatchEvent(new CustomEvent(BROWSER_ENGINE_CHANGED_EVENT));
              if (stale()) return;
              if (status.state !== "installed") {
                fail(action, "The Chromium engine is not available on this system.");
              } else if (status.inactive) {
                fail(action, "The Chromium engine is installed but cannot run in this session.");
              } else {
                openChromiumTab(action.url);
              }
            },
            (err: unknown) => {
              if (stale()) return;
              const failure = parseBrowserEngineFailure(err);
              fail(action, failure.message ?? `The Chromium engine could not be installed (${failure.reason ?? failure.code}).`);
            },
          )
          .finally(() => setBusy(null));
        return;
      case "systemAuthSession":
        setBusy("authSession");
        void runSignInAuthSession(tab.browserId, action.authorizeUrl, action.callback)
          .then(
            (result) => {
              if (stale()) return;
              if (result.outcome === "completed") hide();
              else if (result.outcome === "unavailable") fail(action, result.reason);
              // Cancelled: the tab stays as it is and the bar stays offered.
            },
            (err: unknown) => {
              if (!stale()) fail(action, errorText(err));
            },
          )
          .finally(() => setBusy(null));
        return;
      case "externalBrowser":
        void openExternalUrl(action.url).then(
          () => {
            if (!stale()) hide();
          },
          (err: unknown) => {
            if (!stale()) setError(errorText(err));
          },
        );
        return;
    }
  };

  const notNow = () => {
    setError(null);
    // Without an in-app path the external browser is already offered, so "Not now" dismisses.
    if (shownPlan.primary.kind === "externalBrowser") hide();
    else setDeclined(true);
  };

  const disabled = busy !== null;
  const [primary, ...alternatives] = offered;

  return (
    <div
      role="status"
      className="relative flex items-center gap-2 border-b border-border bg-popover px-3 py-1.5 text-[11px]"
      data-testid="browser-sign-in-handoff"
    >
      <span className="flex-1">
        {declined
          ? "Sign-in stays blocked in this tab."
          : `${PROVIDER_NAME[shownPlan.provider]} does not allow sign-in in this browser engine.`}
        {busy === "install" ? <span className="ml-2 text-muted-foreground">{progressText(progress)}</span> : null}
      </span>
      {declined ? (
        <div className="relative">
          <button
            type="button"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((open) => !open)}
            className="rounded border border-border px-2 py-0.5 hover:bg-accent"
          >
            More options
          </button>
          {menuOpen ? (
            <div
              role="menu"
              aria-label="Sign-in options"
              className="absolute right-0 top-full z-10 mt-1 flex min-w-44 flex-col rounded-md border border-border bg-popover py-1 shadow-lg"
            >
              {shownPlan.onDecline.map((action) => (
                <button
                  key={action.kind}
                  type="button"
                  role="menuitem"
                  onClick={() => perform(action)}
                  className="px-3 py-1 text-left hover:bg-accent"
                >
                  {ACTION_LABEL[action.kind]}
                </button>
              ))}
              <button type="button" role="menuitem" onClick={hide} className="px-3 py-1 text-left hover:bg-accent">
                Dismiss
              </button>
            </div>
          ) : null}
        </div>
      ) : (
        <>
          {primary ? (
            <button
              type="button"
              disabled={disabled}
              onClick={() => perform(primary)}
              className="rounded border border-border bg-accent px-2 py-0.5 font-medium hover:bg-accent/80 disabled:opacity-50"
            >
              {ACTION_LABEL[primary.kind]}
            </button>
          ) : null}
          {alternatives.map((action) => (
            <button
              key={action.kind}
              type="button"
              disabled={disabled}
              onClick={() => perform(action)}
              className="rounded border border-border px-2 py-0.5 hover:bg-accent disabled:opacity-50"
            >
              {ACTION_LABEL[action.kind]}
            </button>
          ))}
          <button
            type="button"
            disabled={disabled}
            onClick={notNow}
            className="rounded px-2 py-0.5 hover:bg-accent disabled:opacity-50"
          >
            Not now
          </button>
        </>
      )}
      {error ? <span className="text-destructive">{error}</span> : null}
    </div>
  );
}
