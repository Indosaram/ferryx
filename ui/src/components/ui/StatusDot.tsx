import { CheckCircle2, MessageCircleQuestion } from "lucide-react";

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

  if (state === "waiting") {
    return <MessageCircleQuestion className={cn("size-3 shrink-0 text-status-warning", className)} />;
  }

  if (state === "done") {
    return <CheckCircle2 className={cn("size-3 shrink-0 text-status-success", className)} />;
  }

  return <span className={cn("size-2 shrink-0 rounded-full bg-status-idle", className)} />;
}
