import { Check, ChevronDown, Circle, X } from "lucide-react";
import { useState } from "react";

import { summarizeSetupProgress, type SetupProgressItem } from "../../lib/setupProgress";

export type SetupProgressPanelProps = {
  items: SetupProgressItem[];
  onDismiss: () => void;
  onOpenSetup: () => void;
};

export function SetupProgressPanel({ items, onDismiss, onOpenSetup }: SetupProgressPanelProps) {
  const [expanded, setExpanded] = useState(false);
  const { done, total, nextItem } = summarizeSetupProgress(items);
  if (!nextItem) return null;

  return (
    <div
      data-testid="setup-progress-panel"
      className="mx-3 mt-2 rounded-md border border-border bg-accent/40 px-3 py-2.5"
    >
      <div className="flex items-center gap-2">
        <span className="text-[13px] font-medium text-foreground">Setup</span>
        <span className="text-[11px] tabular-nums text-muted-foreground">
          {done}/{total}
        </span>
        <button
          type="button"
          aria-label="Dismiss setup progress"
          onClick={onDismiss}
          className="ml-auto rounded p-0.5 text-muted-foreground hover:bg-accent hover:text-foreground"
        >
          <X className="size-3.5" />
        </button>
      </div>

      <div className="mt-1.5 h-1 w-full overflow-hidden rounded-full bg-border">
        <div
          className="h-full rounded-full bg-foreground transition-[width] duration-300"
          style={{ width: `${(done / total) * 100}%` }}
        />
      </div>

      <div className="mt-2 text-[12px] font-medium text-foreground">{nextItem.title}</div>
      <div className="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">{nextItem.hint}</div>

      <div className="mt-2 flex items-center gap-2">
        <button
          type="button"
          data-testid="setup-progress-continue"
          onClick={onOpenSetup}
          className="rounded-md bg-foreground px-2.5 py-1 text-[11px] font-medium text-background hover:opacity-90"
        >
          Continue setup
        </button>
        <button
          type="button"
          data-testid="setup-progress-toggle"
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
          className="inline-flex items-center gap-0.5 rounded px-1 py-1 text-[11px] text-muted-foreground hover:text-foreground"
        >
          {expanded ? "Hide all" : "Show all"}
          <ChevronDown className={`size-3 transition-transform ${expanded ? "rotate-180" : ""}`} />
        </button>
      </div>

      {expanded ? (
        <ul className="mt-2 grid gap-1 border-t border-border pt-2">
          {items.map((item) => (
            <li
              key={item.id}
              data-testid="setup-progress-item"
              className="flex items-center gap-1.5 text-[11px] text-muted-foreground"
            >
              {item.done ? (
                <Check className="size-3 shrink-0 text-foreground" />
              ) : (
                <Circle className="size-3 shrink-0" />
              )}
              <span className={item.done ? "line-through" : undefined}>{item.title}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
