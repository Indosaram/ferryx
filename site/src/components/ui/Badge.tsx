import * as React from "react";
import { cn } from "@/lib/utils";

export interface BadgeProps extends React.HTMLAttributes<HTMLDivElement> {
  variant?: "default" | "secondary" | "outline";
}

function Badge({ className, variant = "default", ...props }: BadgeProps) {
  return (
    <div
      className={cn(
        "inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-ink/30",
        {
          "border-transparent bg-ink text-page":
            variant === "default",
          "border-line bg-surface text-ink-soft":
            variant === "secondary",
          "border-line text-ink-soft": variant === "outline",
        },
        className
      )}
      {...props}
    />
  );
}

export { Badge };
