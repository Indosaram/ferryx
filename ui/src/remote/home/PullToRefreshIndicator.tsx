// The spinner a phone home list shows above its content while it is pulled or refreshing: it
// turns with the pull, spins while the refresh runs, and announces "Refreshing" politely.
// It mounts with the pull (before the text appears) so the live region exists when it announces,
// and is absent at rest so the list carries no empty status region.
import { cn } from "../../lib/cn";
import { PULL_THRESHOLD, type PullToRefreshState } from "./usePullToRefresh";

export function PullToRefreshIndicator({ pullDistance, refreshing, pulling }: PullToRefreshState) {
  if (pullDistance <= 0 && !refreshing) return null;
  const progress = Math.min(1, pullDistance / PULL_THRESHOLD);
  return (
    <div
      data-testid="pull-refresh-indicator"
      data-refreshing={refreshing ? "true" : "false"}
      role="status"
      aria-live="polite"
      className={cn(
        "pointer-events-none absolute inset-x-0 top-0 z-10 flex items-center justify-center overflow-hidden",
        pulling ? null : "transition-[height,opacity] duration-200 ease-out motion-reduce:transition-none",
      )}
      style={{ height: pullDistance, opacity: refreshing ? 1 : progress }}
    >
      <span
        aria-hidden="true"
        className={cn(
          "block size-6 rounded-full border-2 border-chat-border border-t-chat-foreground-secondary",
          refreshing ? "animate-spin motion-reduce:animate-none" : null,
        )}
        style={refreshing ? undefined : { transform: `rotate(${Math.round(progress * 270)}deg)` }}
      />
      {refreshing ? <span className="sr-only">Refreshing</span> : null}
    </div>
  );
}
