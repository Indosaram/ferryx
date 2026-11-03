// The one header every phone home destination uses: the destination title, an optional context
// title (the open session's workspace / worktree) under it, and an optional trailing action,
// pinned above the destination's own scroller.
import type { ReactNode } from "react";

export function PhoneHomeHeader({
  title,
  contextTitle,
  trailing,
}: {
  readonly title: string;
  /** Where the open session lives. Absent or blank: the title stands alone. */
  readonly contextTitle?: string | null;
  readonly trailing?: ReactNode;
}) {
  const context = contextTitle?.trim();
  return (
    <div data-testid="phone-home-header" className="flex h-14 shrink-0 items-center gap-2 px-4">
      <div className="flex min-w-0 flex-1 flex-col justify-center">
        <h1 className="min-w-0 truncate text-lg font-semibold leading-tight text-chat-foreground">{title}</h1>
        {context ? (
          <p data-testid="phone-home-context" className="min-w-0 truncate text-xs leading-tight text-chat-foreground-secondary">
            {context}
          </p>
        ) : null}
      </div>
      {trailing}
    </div>
  );
}
