import React from "react";
import type { DagNodeSnapshot } from "../../lib/dagTypes";
import { formatRouteText, getNodeStateGlyph } from "./dagViewUtils";

export type DagNodeCardProps = {
  readonly node: DagNodeSnapshot;
  readonly isCriticalPath: boolean;
  readonly blockedCount: number;
  readonly style?: React.CSSProperties;
};

export function DagNodeCard({
  node,
  isCriticalPath,
  blockedCount,
  style,
}: DagNodeCardProps): JSX.Element {
  const glyph = getNodeStateGlyph(node.state);
  const routeText = formatRouteText(node.route);
  const displayLabel = node.label || node.id;

  let stateStyle = "border-border/50 bg-card/80 text-muted-foreground";
  if (node.state === "running") {
    stateStyle = "border-status-working/60 bg-status-working/10 text-foreground animate-pulse";
  } else if (node.state === "completed") {
    stateStyle = "border-emerald-500/40 bg-emerald-950/25 text-emerald-400";
  } else if (node.state === "failed") {
    stateStyle = "border-rose-500/40 bg-rose-950/25 text-rose-400";
  } else if (node.state === "paused") {
    stateStyle = "border-amber-500/40 bg-amber-950/25 text-amber-400";
  }

  const criticalPathClass = isCriticalPath ? "ring-1 ring-primary/60 border-primary/70 shadow-sm" : "";

  return (
    <div
      data-testid={`dag-node-${node.id}`}
      data-node-id={node.id}
      data-node-state={node.state}
      data-critical-path={isCriticalPath ? "true" : "false"}
      style={style}
      className={`group relative flex flex-col justify-between rounded-lg border p-2.5 text-xs select-none transition-all duration-150 ${stateStyle} ${criticalPathClass}`}
    >
      <div className="flex items-center justify-between gap-1.5 min-w-0">
        <div className="flex items-center gap-1.5 min-w-0 flex-1">
          <span className="font-mono text-sm shrink-0 leading-none" data-testid="dag-node-glyph">
            {glyph}
          </span>
          <span className="truncate font-medium text-foreground text-[12px]" title={displayLabel}>
            {displayLabel}
          </span>
        </div>
        {node.attempt > 1 && (
          <span className="shrink-0 font-mono text-[10px] text-muted-foreground px-1 py-0.5 rounded bg-muted/60">
            x{node.attempt}
          </span>
        )}
      </div>

      <div className="mt-2 flex items-center justify-between gap-1 text-[11px] text-muted-foreground">
        <span className="truncate font-mono text-[10px] text-muted-foreground/80" title={routeText}>
          {routeText}
        </span>
        {blockedCount > 0 && (
          <span
            className="shrink-0 inline-flex items-center rounded-full bg-amber-500/20 px-1.5 py-0.5 text-[10px] font-medium text-amber-300 border border-amber-500/30"
            data-testid="dag-bottleneck-badge"
          >
            blocks {blockedCount}
          </span>
        )}
      </div>
    </div>
  );
}
