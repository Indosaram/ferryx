import { useEffect, useRef, useState } from "react";
import { Star, X } from "lucide-react";

import type { StarNagMode } from "../../lib/starNag";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { IconButton } from "../ui/IconButton";

const HEADING_ID = "star-nag-heading";

function primaryLabel(mode: StarNagMode, busy: boolean): string {
  if (busy) return mode === "gh" ? "Starring..." : "Opening...";
  return mode === "gh" ? "Star on GitHub" : "Open GitHub";
}

/**
 * Bottom-right star card, ported from Orca's `StarNagCard`.
 *
 * Visibility, cooldown and the completion flag live in the caller so this stays a dumb surface.
 * Both the close button and Escape mean "dismiss", which is the doubling-defer path — not
 * completion, because the user never said they starred.
 */
export function StarNagCard(props: {
  /** Mode the prompt opened with; the card owns it afterwards so a failed star can fall back. */
  initialMode: StarNagMode;
  busy: boolean;
  /** Resolves false when the action failed, so the card can downgrade `gh` to the browser path. */
  onStar: (mode: StarNagMode) => Promise<boolean>;
  onLater: () => void;
  onDismiss: () => void;
}): JSX.Element {
  const { initialMode, busy, onStar, onLater, onDismiss } = props;
  const [mode, setMode] = useState<StarNagMode>(initialMode);
  const cardRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    cardRef.current?.focus({ preventScroll: true });
  }, []);

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape" && !busy) {
      event.preventDefault();
      onDismiss();
    }
  };

  // Orca's card keeps its own mode: a failed direct star switches the button to the browser
  // handoff instead of reporting an error, and a failed browser handoff keeps the prompt open.
  const handleStar = () => {
    if (busy) return;
    void onStar(mode).then(
      (ok) => {
        if (!ok && mode === "gh") setMode("web");
      },
      () => {
        if (mode === "gh") setMode("web");
      },
    );
  };

  return (
    <Card
      ref={cardRef}
      role="complementary"
      aria-labelledby={HEADING_ID}
      tabIndex={-1}
      data-native-terminal-yield="off"
      className="gap-0 py-0 shadow-lg"
      onKeyDown={handleKeyDown}
    >
      <div className="flex flex-col gap-2.5 p-3.5">
        <div className="flex items-start justify-between gap-2">
          <div className="flex min-w-0 items-center gap-2">
            <Star
              className="size-4 shrink-0 fill-status-warning/60 text-status-warning"
              aria-hidden="true"
            />
            <h3 id={HEADING_ID} className="text-sm font-semibold text-foreground">
              Enjoying Ferryx?
            </h3>
          </div>
          <IconButton label="Dismiss" size="sm" className="shrink-0" onClick={onDismiss} disabled={busy}>
            <X className="size-3.5" aria-hidden="true" />
          </IconButton>
        </div>
        <p className="text-sm leading-relaxed text-muted-foreground">
          Ferryx is open source. If it helped today, a GitHub star helps other developers find it.
        </p>
        <div className="mt-0.5 flex gap-2">
          <Button
            size="sm"
            className="min-w-0 flex-1 gap-1.5 border border-status-warning-border bg-status-warning-container text-status-warning-on hover:bg-status-warning-container/80"
            onClick={handleStar}
            disabled={busy}
          >
            <Star className="size-4" aria-hidden="true" />
            {primaryLabel(mode, busy)}
          </Button>
          <Button size="sm" variant="ghost" className="w-[84px]" onClick={onLater} disabled={busy}>
            Later
          </Button>
        </div>
      </div>
    </Card>
  );
}
