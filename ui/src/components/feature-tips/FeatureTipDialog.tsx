import { useEffect, useRef } from "react";
import { Lightbulb, Loader2, X } from "lucide-react";

import type { FeatureTip, FeatureTipAction } from "../../lib/featureTips";
import { Button } from "../ui/button";
import { IconButton } from "../ui/IconButton";

const HEADING_ID = "feature-tip-heading";
const SHORTCUT_TOKEN = "{{shortcut}}";

/** Orca's busy labels: the CTA names the work it is doing instead of spinning silently. */
function busyLabel(action: FeatureTipAction): string {
  if (action === "setup-cli") return "Installing...";
  if (action === "enable-session-search") return "Turning on...";
  return "Working...";
}

/**
 * Renders a title that may carry Orca's `{{shortcut}}` token as an inline key chip.
 * The chip keeps the shortcut readable inside the sentence instead of naming a key in prose.
 */
function TipTitle({ title, shortcut }: { title: string; shortcut: string }): JSX.Element {
  const parts = title.split(SHORTCUT_TOKEN);
  return (
    <h2 id={HEADING_ID} className="text-[17px] font-semibold tracking-tight text-foreground">
      {parts.map((part, index) => (
        <span key={index}>
          {part}
          {index < parts.length - 1 ? (
            <kbd className="ml-0.5 inline-flex items-center whitespace-nowrap rounded-md border border-border bg-card px-2 py-0.5 align-middle font-mono text-[15px] font-medium text-foreground">
              {shortcut}
            </kbd>
          ) : null}
        </span>
      ))}
    </h2>
  );
}

/**
 * One-time feature tip dialog, ported from Orca's `FeatureTipDialogFrame` +
 * `FeatureTipActions` pair.
 *
 * Exposure is owned by the caller (at most one tip per app open, first pending tip wins,
 * marked seen before this mounts); this component only renders the tip and reports intent.
 * Escape and the close button both mean "Maybe Later" — Orca marks the tip seen on either.
 */
export function FeatureTipDialog(props: {
  tip: FeatureTip;
  shortcut: string;
  busy?: boolean;
  onClose: () => void;
  onAction: (action: FeatureTipAction) => void;
}): JSX.Element {
  const { tip, shortcut, busy = false, onClose, onAction } = props;
  const primaryRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    primaryRef.current?.focus({ preventScroll: true });
  }, []);

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      if (!busy) onClose();
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 p-4 backdrop-blur-sm"
      onKeyDown={handleKeyDown}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={HEADING_ID}
        data-native-terminal-yield="off"
        className="flex max-h-[min(36rem,90vh)] w-full max-w-md flex-col overflow-hidden rounded-lg border border-border bg-card shadow-lg"
      >
        <div className="flex items-start justify-between gap-4 px-6 pt-5">
          <div className="min-w-0">
            <div className="mb-2 flex h-5 items-center gap-1.5 text-[11px] font-medium text-muted-foreground">
              <Lightbulb className="size-4" aria-hidden="true" />
              Tip
            </div>
            <TipTitle title={tip.title} shortcut={shortcut} />
          </div>
          <IconButton label="Close" size="sm" onClick={onClose} disabled={busy}>
            <X className="size-3.5" aria-hidden="true" />
          </IconButton>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
          <p className="text-[13px] leading-relaxed text-muted-foreground">{tip.description}</p>
        </div>
        <div className="flex justify-end gap-2 border-t border-border px-6 py-3">
          <Button variant="ghost" size="sm" onClick={onClose} disabled={busy}>
            Maybe Later
          </Button>
          <Button
            ref={primaryRef}
            size="sm"
            className="min-w-24"
            onClick={() => onAction(tip.action)}
            disabled={busy}
          >
            {busy ? (
              <>
                <Loader2 className="size-4 animate-spin" aria-hidden="true" />
                {busyLabel(tip.action)}
              </>
            ) : (
              tip.ctaLabel
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
