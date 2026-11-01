import { useEffect, useState } from "react";

import {
  SIGN_IN_HANDOFF_EVENT,
  detectSignInBlock,
  getBrowserEngineStatus,
  openExternalUrl,
  type SignInHandoffDetail,
} from "../lib/browserTauri";
import type { BrowserTab } from "../lib/types";

const GOOGLE_ACCOUNTS_HOST = "accounts.google.com";

function mightBeGoogleSignIn(url: string): boolean {
  try {
    return new URL(url).hostname.toLowerCase() === GOOGLE_ACCOUNTS_HOST;
  } catch {
    return false;
  }
}

/**
 * Shown in a system-webview tab that Google refused to sign in ("This browser or app may not be
 * secure"). "Sign in with Chromium engine" reopens the sign-in in a Chromium tab of the same
 * profile; Dismiss leaves the tab as it is.
 */
export function BrowserSignInHandoffBar({ tab }: { tab: BrowserTab }) {
  const [retryUrl, setRetryUrl] = useState<string | null>(null);
  const [engineInstalled, setEngineInstalled] = useState(false);
  const [dismissedUrl, setDismissedUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const systemEngine = tab.engine !== "chromium";

  useEffect(() => {
    setRetryUrl(null);
    if (!systemEngine || !mightBeGoogleSignIn(tab.url)) return;
    let cancelled = false;
    void Promise.all([detectSignInBlock(tab.url, tab.title), getBrowserEngineStatus()]).then(
      ([block, engine]) => {
        if (cancelled) return;
        setEngineInstalled(engine.state === "installed");
        setRetryUrl(block.kind === "googleBlocked" ? block.retryUrl : null);
      },
      () => {
        // Detection is advisory: without an answer the tab simply shows no bar.
        if (!cancelled) setRetryUrl(null);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [systemEngine, tab.url, tab.title]);

  if (!retryUrl || dismissedUrl === tab.url) return null;

  const handOff = () => {
    const detail: SignInHandoffDetail = { url: retryUrl, profileId: tab.profileId };
    window.dispatchEvent(new CustomEvent<SignInHandoffDetail>(SIGN_IN_HANDOFF_EVENT, { detail }));
    setDismissedUrl(tab.url);
  };

  const openInDefaultBrowser = () => {
    void openExternalUrl(retryUrl).then(
      () => setDismissedUrl(tab.url),
      (failure: unknown) => setError(String(failure)),
    );
  };

  return (
    <div
      role="status"
      className="flex items-center gap-2 border-b border-border bg-popover px-3 py-1.5 text-[11px]"
      data-testid="browser-sign-in-handoff"
    >
      <span className="flex-1">
        Google does not allow sign-in in this browser engine.
        {engineInstalled ? null : " Install the Chromium engine in Settings > Browser to sign in here."}
      </span>
      {engineInstalled ? (
        <button type="button" onClick={handOff} className="rounded border border-border px-2 py-0.5 hover:bg-accent">
          Sign in with Chromium engine
        </button>
      ) : (
        <button type="button" onClick={openInDefaultBrowser} className="rounded border border-border px-2 py-0.5 hover:bg-accent">
          Open in default browser
        </button>
      )}
      <button type="button" onClick={() => setDismissedUrl(tab.url)} className="rounded px-2 py-0.5 hover:bg-accent">
        Dismiss
      </button>
      {error ? <span className="text-destructive">{error}</span> : null}
    </div>
  );
}
