import type { ReactNode } from "react";

import { cn } from "../../lib/cn";

type SectionHeaderProps = {
  title: string;
  count?: number;
  actions?: ReactNode;
  className?: string;
};

export function SectionHeader({ title, count, actions, className }: SectionHeaderProps) {
  return (
    <div className={cn("flex h-8 items-center justify-between gap-2 px-2", className)}>
      <div className="flex min-w-0 items-center gap-1.5 pl-2 text-xs font-semibold text-muted-foreground/80">
        <span className="truncate">{title}</span>
        {count !== undefined ? (
          <span className="rounded-full bg-sidebar-accent px-1.5 py-0.5 text-[10px] leading-none text-muted-foreground">
            {count}
          </span>
        ) : null}
      </div>
      {actions ? <div className="flex shrink-0 items-center gap-1">{actions}</div> : null}
    </div>
  );
}
