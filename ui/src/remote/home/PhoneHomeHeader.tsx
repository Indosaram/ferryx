// The one header every phone home destination uses: the destination title and an optional
// trailing action, pinned above the destination's own scroller.
import type { ReactNode } from "react";

export function PhoneHomeHeader({ title, trailing }: { readonly title: string; readonly trailing?: ReactNode }) {
  return (
    <div data-testid="phone-home-header" className="flex h-14 shrink-0 items-center gap-2 px-4">
      <h1 className="min-w-0 flex-1 truncate text-lg font-semibold text-chat-foreground">{title}</h1>
      {trailing}
    </div>
  );
}
