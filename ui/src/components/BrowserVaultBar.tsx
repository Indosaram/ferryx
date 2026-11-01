import { useEffect, useRef, useState } from "react";
import { KeyRound, X } from "lucide-react";

import { describeBrowserError, isVaultUnavailable } from "../lib/browserErrorText";
import {
  dismissVaultPrompt,
  fillVaultItem,
  onVaultSavePrompt,
  saveVaultPrompt,
  vaultMatchesForBrowser,
  type VaultItemSummary,
  type VaultSavePrompt,
} from "../lib/browserVault";

const MAX_FILL_OFFERS = 3;

function isWebUrl(url: string): boolean {
  return /^https?:\/\//i.test(url);
}

/** Offers to save a submitted password and to fill a saved login for the current page. */
export function BrowserVaultBar({ browserId, url }: { browserId: string; url: string }) {
  const [prompt, setPrompt] = useState<VaultSavePrompt | null>(null);
  const [promptError, setPromptError] = useState<string | null>(null);
  const [matches, setMatches] = useState<VaultItemSummary[]>([]);
  const [dismissedUrl, setDismissedUrl] = useState<string | null>(null);
  const [fillError, setFillError] = useState<string | null>(null);
  const matchSeq = useRef(0);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    setPrompt(null);
    setPromptError(null);
    void onVaultSavePrompt((next) => {
      if (next.browserId !== browserId) return;
      setPrompt(next);
      setPromptError(null);
    }).then(
      (stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      },
      (failure: unknown) => {
        if (!cancelled) setPromptError(describeBrowserError(failure));
      },
    );
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [browserId]);

  useEffect(() => {
    const seq = ++matchSeq.current;
    setMatches([]);
    setFillError(null);
    if (!isWebUrl(url)) return;
    void vaultMatchesForBrowser(browserId).then(
      (next) => {
        if (seq === matchSeq.current) setMatches(next ?? []);
      },
      (failure: unknown) => {
        // The lookup runs on every page load without the user asking: no keyring simply means
        // no fill offer (Settings > Passwords explains why), never an error in the page.
        if (seq !== matchSeq.current || isVaultUnavailable(failure)) return;
        setFillError(describeBrowserError(failure));
      },
    );
    return () => {
      matchSeq.current += 1;
    };
  }, [browserId, url]);

  const acceptPrompt = (current: VaultSavePrompt) => {
    setPromptError(null);
    void saveVaultPrompt(current.promptId).then(
      () => setPrompt((shown) => (shown?.promptId === current.promptId ? null : shown)),
      (failure: unknown) => setPromptError(describeBrowserError(failure)),
    );
  };

  const dismissPrompt = (current: VaultSavePrompt) => {
    setPromptError(null);
    void dismissVaultPrompt(current.promptId).then(
      () => setPrompt((shown) => (shown?.promptId === current.promptId ? null : shown)),
      (failure: unknown) => setPromptError(describeBrowserError(failure)),
    );
  };

  const fill = (id: string) => {
    setFillError(null);
    void fillVaultItem(browserId, id).catch((failure: unknown) => setFillError(describeBrowserError(failure)));
  };

  const showOffer = matches.length > 0 && dismissedUrl !== url;
  const barClass = "flex items-center gap-2 border-b border-border bg-popover px-3 py-1.5 text-[11px]";

  return (
    <>
      {prompt ? (
        <div className={barClass} data-testid="vault-save-prompt">
          <KeyRound className="size-3.5 shrink-0" />
          <span className="min-w-0 flex-1 truncate">
            {prompt.update ? "Update" : "Save"} password for {prompt.username} on {prompt.origin}?
          </span>
          {promptError ? (
            <span role="alert" className="text-destructive">
              {promptError}
            </span>
          ) : null}
          <button
            type="button"
            onClick={() => acceptPrompt(prompt)}
            className="rounded border border-border px-2 py-1 hover:bg-accent"
            data-testid="vault-save-accept"
          >
            {prompt.update ? "Update" : "Save"}
          </button>
          <button
            type="button"
            onClick={() => dismissPrompt(prompt)}
            className="rounded border border-border px-2 py-1 hover:bg-accent"
            data-testid="vault-save-dismiss"
          >
            Not now
          </button>
        </div>
      ) : promptError ? (
        <div className={barClass}>
          <span role="alert" className="text-destructive">
            {promptError}
          </span>
        </div>
      ) : null}
      {showOffer ? (
        <div className={barClass} data-testid="vault-fill-offer">
          <KeyRound className="size-3.5 shrink-0" />
          <span className="min-w-0 flex-1 truncate">Fill saved login</span>
          {fillError ? (
            <span role="alert" className="text-destructive">
              {fillError}
            </span>
          ) : null}
          {matches.slice(0, MAX_FILL_OFFERS).map((match) => (
            <button
              key={match.id}
              type="button"
              onClick={() => fill(match.id)}
              className="max-w-40 truncate rounded border border-border px-2 py-1 hover:bg-accent"
              data-testid="vault-fill"
            >
              {match.username}
            </button>
          ))}
          <button
            type="button"
            aria-label="Dismiss saved logins"
            onClick={() => setDismissedUrl(url)}
            className="rounded p-1 hover:bg-accent"
            data-testid="vault-fill-dismiss"
          >
            <X className="size-3.5" />
          </button>
        </div>
      ) : fillError ? (
        <div className={barClass}>
          <span role="alert" className="text-destructive">
            {fillError}
          </span>
        </div>
      ) : null}
    </>
  );
}

