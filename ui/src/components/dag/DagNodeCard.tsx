import React from "react";
import type { DagNodeSnapshot, DagNodeState } from "../../lib/dagTypes";
import { formatRouteText, getNodeStateGlyph } from "./dagViewUtils";

export type DagNodeCardProps = {
  readonly node: DagNodeSnapshot;
  readonly isCriticalPath: boolean;
  readonly blockedCount: number;
  readonly style?: React.CSSProperties;
};

type StateAppearance = {
  readonly label: string;
  readonly card: string;
  readonly accent: string;
};

const STATE_APPEARANCE: Record<DagNodeState, StateAppearance> = {
  running: {
    label: "running",
    card: "border-indigo-500/70 bg-indigo-500/10 shadow-[0_0_0_1px_rgb(99_102_241_/_0.15)]",
    accent: "text-indigo-500",
  },
  completed: {
    label: "done",
    card: "border-border bg-foreground/[0.04]",
    accent: "text-foreground/60",
  },
  failed: {
    label: "failed",
    card: "border-rose-500/60 bg-rose-500/10",
    accent: "text-rose-500",
  },
  paused: {
    label: "paused",
    card: "border-amber-500/50 bg-amber-500/10",
    accent: "text-amber-500",
  },
  blocked: {
    label: "blocked",
    card: "border-border bg-muted/40",
    accent: "text-muted-foreground",
  },
  scheduled: {
    label: "queued",
    card: "border-border bg-muted/40",
    accent: "text-muted-foreground",
  },
  pending: {
    label: "waiting",
    card: "border-border bg-muted/30",
    accent: "text-muted-foreground",
  },
  skipped: {
    label: "skipped",
    card: "border-border bg-muted/30",
    accent: "text-muted-foreground",
  },
  cancelled: {
    label: "cancelled",
    card: "border-border bg-muted/30",
    accent: "text-muted-foreground",
  },
};

export function DagNodeCard({
  node,
  isCriticalPath,
  blockedCount,
  style,
}: DagNodeCardProps): JSX.Element {
  const appearance = STATE_APPEARANCE[node.state];
  const glyph = getNodeStateGlyph(node.state);
  const routeText = formatRouteText(node.route);
  const displayLabel = node.label || node.id;
  const criticalPathClass = isCriticalPath ? "ring-1 ring-primary/50" : "";

  return (
    <div
      data-testid={`dag-node-${node.id}`}
      data-node-id={node.id}
      data-node-state={node.state}
      data-critical-path={isCriticalPath ? "true" : "false"}
      style={style}
      className={`group relative flex flex-col justify-between rounded-lg border p-2.5 text-xs select-none ${appearance.card} ${criticalPathClass}`}
    >
      <div className="flex items-center justify-between gap-1.5 min-w-0">
        <div className="flex items-center gap-1.5 min-w-0 flex-1">
          <span
            className={`font-mono text-sm shrink-0 leading-none ${appearance.accent}`}
            data-testid="dag-node-glyph"
          >
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

      <div className="mt-2 flex items-center justify-between gap-1 text-[11px]">
        <span className={`shrink-0 font-medium ${appearance.accent}`} data-testid="dag-node-state-label">
          {appearance.label}
        </span>
        <span
          className="truncate font-mono text-[10px] text-muted-foreground"
          title={routeText}
        >
          {routeText}
        </span>
        {blockedCount > 1 && (
          <span
            className="shrink-0 inline-flex items-center rounded-full border border-amber-500/40 bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-medium text-amber-600"
            data-testid="dag-bottleneck-badge"
          >
            blocks {blockedCount}
          </span>
        )}
      </div>
    </div>
  );
}
