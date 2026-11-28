import { useState } from "react";
import { Check, ExternalLink, Loader2, Star, X } from "lucide-react";

import type { StarNagMode } from "../../lib/starNag";
import { Button } from "../ui/button";
import { toast } from "../ui/sonner";

type StarNagToastStatus = "idle" | "busy" | "starred" | "opened";

/** What the toast did, so the caller can retire the prompt or only start a cooldown. */
export type StarNagToastOutcome = "starred" | "opened" | "later" | "dismissed";

function actionLabel(mode: StarNagMode, status: StarNagToastStatus): string {
  if (status === "starred") return "Starred - thank you!";
  if (status === "opened") return "GitHub opened";
  if (status === "busy") return mode === "web" ? "Opening..." : "Starring...";
  return mode === "web" ? "Open GitHub" : "Star on GitHub";
}

/**
 * Star prompt toast, ported from Orca's `StarNagToastHost`.
 *
 * It never auto-closes (Orca passes `duration: Infinity`), so the only exits are the primary
 * button, "Later", or the close button. "Later" and close both mean defer, never completion —
 * only a successful star does. A failed `gh` call downgrades the toast to the browser path
 * instead of reporting an error, matching Orca.
 */
function StarNagToast(props: {
  id: string | number;
  mode: StarNagMode;
  markResolved: () => void;
  setDismissSuppressed: (suppressed: boolean) => void;
  onStar: () => Promise<boolean>;
  onOpenWeb: () => Promise<boolean>;
}): JSX.Element {
  const { id, mode: initialMode, markResolved, setDismissSuppressed, onStar, onOpenWeb } = props;
  const [mode, setMode] = useState<StarNagMode>(initialMode);
  const [status, setStatus] = useState<StarNagToastStatus>("idle");
  const busy = status === "busy";

  const close = () => {
    if (busy) return;
    toast.dismiss(id);
  };

  const later = () => {
    if (busy || status === "starred" || status === "opened") return;
    // "Later" is a defer, not a completion, so it must reach the caller as an unresolved dismiss.
    toast.dismiss(id);
  };

  const act = async () => {
    if (busy || status === "starred") return;
    setStatus("busy");
    setDismissSuppressed(true);
    if (mode === "web") {
      let ok = false;
      try {
        ok = await onOpenWeb();
      } catch {
        ok = false;
      }
      if (!ok) {
        setDismissSuppressed(false);
        setStatus("idle");
        return;
      }
      markResolved();
      setStatus("opened");
      return;
    }
    let ok = false;
    try {
      ok = await onStar();
    } catch {
      ok = false;
    }
    if (!ok) {
      // Orca downgrades to the browser path rather than showing an error.
      setMode("web");
      setDismissSuppressed(false);
      setStatus("idle");
      return;
    }
    markResolved();
    setStatus("starred");
  };

  const completedStar = status === "starred";
  const primaryActionClass = completedStar
    ? "min-w-0 flex-1 gap-1.5 border border-status-warning-border/40 bg-status-warning-container/60 text-status-warning-on hover:bg-status-warning-container/60"
    : "min-w-0 flex-1 gap-1.5 border border-status-warning-border bg-status-warning-container text-status-warning-on hover:bg-status-warning-container/80";

  return (
    <div
      role="dialog"
      aria-label="Star Ferryx on GitHub"
      data-native-terminal-yield="off"
      className="relative w-[min(340px,calc(100vw-32px))] overflow-hidden rounded-lg border border-border bg-popover p-3.5 text-popover-foreground shadow-2xl"
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 space-y-1.5">
          <div className="flex items-center gap-2">
            <span
              className={
                completedStar
                  ? "flex size-6 shrink-0 items-center justify-center rounded-full border border-status-warning-border/40 bg-status-warning-container/60 text-status-warning"
                  : "flex size-6 shrink-0 items-center justify-center rounded-full border border-status-success-border bg-status-success-container text-status-success"
              }
              aria-hidden="true"
            >
              {completedStar ? <Star className="size-3.5 fill-current" /> : <Check className="size-3.5" />}
            </span>
            <div className="text-sm font-semibold text-foreground">Onboarding completed!</div>
          </div>
          <p className="text-sm leading-5 text-muted-foreground">
            If you&apos;re enjoying Ferryx so far, a GitHub star helps other developers discover it.
          </p>
        </div>
        <Button
          variant="ghost"
          size="icon"
          className="size-7 shrink-0"
          onClick={close}
          disabled={busy}
          aria-label="Dismiss"
        >
          <X className="size-3.5" aria-hidden="true" />
        </Button>
      </div>
      <div className="mt-3 flex gap-2">
        <Button
          variant="default"
          size="sm"
          className={primaryActionClass}
          onClick={() => void act()}
          disabled={busy || status === "starred" || status === "opened"}
        >
          {busy ? (
            <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
          ) : mode === "web" ? (
            <ExternalLink className="size-3.5" aria-hidden="true" />
          ) : (
            <Star className="size-3.5" aria-hidden="true" />
          )}
          {actionLabel(mode, status)}
        </Button>
        <Button
          variant="secondary"
          size="sm"
          className="w-[84px]"
          onClick={later}
          disabled={busy || status === "starred" || status === "opened"}
        >
          Later
        </Button>
      </div>
    </div>
  );
}

export type StarNagToastArgs = {
  mode: StarNagMode;
  onStar: () => Promise<boolean>;
  onOpenWeb: () => Promise<boolean>;
  /** Fired once when the toast closes; `resolved` is false for an unresolved dismiss. */
  onDismissed: (resolved: boolean) => void;
};

/**
 * Opens the star toast. Returns a function that clears it, for callers that unmount.
 *
 * Mirrors Orca's `onDismiss` handler: an unresolved, unsuppressed dismissal is reported so the
 * caller can run the defer path (cooldown + doubled threshold) rather than marking completion.
 */
export function showStarNagToast(args: StarNagToastArgs): () => void {
  let resolved = false;
  let dismissSuppressed = false;
  const id = toast.custom(
    (toastId) => (
      <StarNagToast
        id={toastId}
        mode={args.mode}
        markResolved={() => {
          resolved = true;
        }}
        setDismissSuppressed={(suppressed) => {
          dismissSuppressed = suppressed;
        }}
        onStar={args.onStar}
        onOpenWeb={args.onOpenWeb}
      />
    ),
    {
      duration: Infinity,
      closeButton: false,
      dismissible: false,
      unstyled: true,
      onDismiss: () => {
        if (!resolved && !dismissSuppressed) args.onDismissed(false);
        else if (resolved) args.onDismissed(true);
      },
    },
  );
  return () => toast.dismiss(id);
}

