// Every supervisor setup stage as a compact list, derived only from the SetupStatus: done,
// pending or blocked, and a spinner only on the stage the supervisor is working on right now.
// Transport-free, so the onboarding step, the desktop banner and the phone banner share it.
import { AlertCircle, Check, Circle, Loader2 } from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "../../lib/cn";
import { setupChecklist, type SetupChecklistState, type SetupStatus } from "../../lib/supervisorSetupModel";

export type SetupChecklistProps = {
  readonly status: SetupStatus;
  /** Shown at the end of the "Earlier jobs" row while jobs are waiting (the Adopt button). */
  readonly earlierJobsAction?: ReactNode;
  readonly touch?: boolean;
  readonly className?: string;
};

const STATE_LABELS: Record<SetupChecklistState, string> = {
  done: "Done",
  pending: "Not yet",
  blocked: "Needs attention",
  active: "In progress",
};

function StateIcon({ state, touch }: { state: SetupChecklistState; touch: boolean }) {
  const size = touch ? "size-4" : "size-3.5";
  switch (state) {
    case "done":
      return <Check aria-hidden="true" className={cn(size, "shrink-0 text-status-success")} />;
    case "blocked":
      return <AlertCircle aria-hidden="true" className={cn(size, "shrink-0 text-destructive")} />;
    case "active":
      return <Loader2 aria-hidden="true" className={cn(size, "shrink-0 animate-spin text-muted-foreground motion-reduce:animate-none")} />;
    case "pending":
      return <Circle aria-hidden="true" className={cn(size, "shrink-0 text-muted-foreground/60")} />;
  }
}

export function SetupChecklist({ status, earlierJobsAction, touch = false, className }: SetupChecklistProps) {
  const rows = setupChecklist(status);
  return (
    <ul role="list" aria-label="Supervisor setup steps" data-testid="setup-checklist" className={cn("flex min-w-0 flex-col", className)}>
      {rows.map((row) => (
        <li
          key={row.id}
          data-testid={`setup-checklist-${row.id}`}
          data-state={row.state}
          className={cn("flex min-w-0 items-center gap-2 leading-snug", touch ? "min-h-9 text-[14px]" : "min-h-7 text-[12px]")}
        >
          <StateIcon state={row.state} touch={touch} />
          <span className="sr-only">{STATE_LABELS[row.state]}:</span>
          <span className={cn("shrink-0", row.state === "done" ? "text-muted-foreground" : "font-medium text-foreground")}>{row.label}</span>
          <span
            data-testid={`setup-checklist-${row.id}-detail`}
            title={row.detail}
            className={cn(
              "min-w-0 flex-1 truncate",
              row.state === "blocked" ? "font-mono text-destructive" : "text-muted-foreground",
              row.id === "owner" && row.state === "done" ? "font-mono" : null,
            )}
          >
            {row.detail}
          </span>
          {row.id === "earlier_jobs" && row.state !== "done" && earlierJobsAction ? <span className="shrink-0">{earlierJobsAction}</span> : null}
        </li>
      ))}
    </ul>
  );
}
