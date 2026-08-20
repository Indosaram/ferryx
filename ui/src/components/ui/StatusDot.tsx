import { CheckCircle2, CircleX, MessageCircleQuestion } from "lucide-react";

import { cn } from "../../lib/cn";
import type { AgentState } from "../../lib/types";

type StatusDotProps = {
  state: AgentState;
  className?: string;
};

export function StatusDot({ state, className }: StatusDotProps) {
  if (state === "working") {
    return (
      <span className={cn("relative inline-flex size-3 shrink-0 items-center justify-center", className)}>
        <span className="absolute size-3 animate-ping rounded-full bg-status-working/25 motion-reduce:animate-none" />
        <span className="size-2 rounded-full bg-status-working" />
      </span>
    );
  }

  if (state === "starting") {
    return <span className={cn("size-2 shrink-0 animate-pulse rounded-full bg-status-warning motion-reduce:animate-none", className)} />;
  }

  if (state === "waiting") {
    return <MessageCircleQuestion className={cn("size-3 shrink-0 text-status-warning", className)} />;
  }

  if (state === "exited") {
    return <CheckCircle2 className={cn("size-3 shrink-0 text-status-success", className)} />;
  }

  return <CircleX className={cn("size-3 shrink-0 text-destructive", className)} />;
}
