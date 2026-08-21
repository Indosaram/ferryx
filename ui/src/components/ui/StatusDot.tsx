import { LoaderCircle } from "lucide-react";

import { cn } from "../../lib/cn";
import type { TerminalActivityState } from "../../lib/activity";
import type { AgentState } from "../../lib/types";

export type StatusDotState = AgentState | TerminalActivityState | "unread";

type StatusDotProps = {
  state: StatusDotState;
  className?: string;
};

export function StatusDot({ state, className }: StatusDotProps) {
  if (state === "working") {
    return (
      <LoaderCircle
        aria-hidden="true"
        data-status-state="working"
        className={cn("size-3 shrink-0 animate-spin text-status-working motion-reduce:animate-none", className)}
      />
    );
  }

  if (state === "starting") {
    return (
      <span
        aria-hidden="true"
        data-status-state="starting"
        className={cn("size-2 shrink-0 animate-pulse rounded-full bg-status-warning motion-reduce:animate-none", className)}
      />
    );
  }

  if (state === "waiting") {
    return (
      <span
        aria-hidden="true"
        data-status-state="waiting"
        className={cn("size-2 shrink-0 rounded-full bg-status-warning ring-2 ring-status-warning/20", className)}
      />
    );
  }

  if (state === "unread") {
    return (
      <span
        aria-hidden="true"
        data-status-state="unread"
        className={cn("size-2 shrink-0 rounded-full bg-blue-500", className)}
      />
    );
  }

  if (state === "done" || state === "exited") {
    return (
      <span
        aria-hidden="true"
        data-status-state="done"
        className={cn("size-2 shrink-0 rounded-full bg-status-success", className)}
      />
    );
  }

  return (
    <span
      aria-hidden="true"
      data-status-state="failed"
      className={cn("size-2 shrink-0 rounded-full bg-destructive", className)}
    />
  );
}
