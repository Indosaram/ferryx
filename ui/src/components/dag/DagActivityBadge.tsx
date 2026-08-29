import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import type { DagRunSnapshot } from "../../lib/dagTypes";
import { dagStore } from "../../state/dagStore";
import { DagGraphView } from "./DagGraphView";

const COMPLETED_GRACE_MS = 90_000;
const LIVE_TICK_MS = 15_000;
const MAX_POPOVER_RUNS = 8;

type DagActivityBadgeProps = {
  className?: string;
};

function collectRuns(state: ReturnType<typeof dagStore.getState>): DagRunSnapshot[] {
  return Object.values(state.runsByProject).flatMap((runs) => Object.values(runs));
}

function runUpdatedAt(run: DagRunSnapshot): number {
  const parsed = run.updatedAt ?? run.completedAt ?? run.startedAt;
  if (parsed === null || parsed === undefined) return Number.NaN;
  return Date.parse(parsed);
}

function isLive(run: DagRunSnapshot, now: number): boolean {
  if (run.status === "running" || run.status === "paused") return true;
  const updated = runUpdatedAt(run);
  return Number.isFinite(updated) && now - updated < COMPLETED_GRACE_MS;
}

function runProgressLabel(run: DagRunSnapshot): string {
  const counts = run.counts;
  return `${counts.completed + counts.failed + counts.cancelled + counts.skipped}/${counts.total} done`;
}

export function DagActivityBadge({ className = "" }: DagActivityBadgeProps): JSX.Element | null {
  const storeState = useSyncExternalStore(dagStore.subscribe, () => dagStore.getState());
  const [now, setNow] = useState(() => Date.now());
  const [open, setOpen] = useState(false);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);

  const runs = useMemo(() => collectRuns(storeState), [storeState]);
  const liveRuns = useMemo(
    () =>
      runs
        .filter((run) => isLive(run, now))
        .sort((a, b) => runUpdatedAt(b) - runUpdatedAt(a)),
    [runs, now],
  );
  const runningCount = liveRuns.filter((run) => run.status === "running").length;
  const visible = liveRuns.length > 0 || open;

  useEffect(() => {
    if (!visible) return;
    const timer = window.setInterval(() => setNow(Date.now()), LIVE_TICK_MS);
    return () => window.clearInterval(timer);
  }, [visible]);

  if (!visible) return null;

  const hasRunning = runningCount > 0;
  const popoverRuns = liveRuns.slice(0, MAX_POPOVER_RUNS);
  const selected =
    popoverRuns.find((run) => run.runId === selectedRunId) ?? popoverRuns[0] ?? null;

  return (
    <div className={`no-drag fixed right-3 top-1.5 z-[70] ${className}`} data-testid="dag-activity-badge">
      <button
        type="button"
        aria-label={hasRunning ? `${runningCount} dag run(s) running` : "Recent dag runs"}
        onClick={() => setOpen((previous) => !previous)}
        className="flex size-6 items-center justify-center rounded-md border border-border/60 bg-card/80 text-accent transition-colors hover:bg-accent/20"
        style={hasRunning ? { filter: "drop-shadow(0 0 5px currentColor)" } : undefined}
      >
        <svg
          viewBox="0 0 16 16"
          className={`size-3.5 ${hasRunning ? "animate-pulse" : "opacity-70"}`}
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          aria-hidden="true"
        >
          <circle cx="8" cy="3.5" r="2" />
          <circle cx="3" cy="12.5" r="2" />
          <circle cx="13" cy="12.5" r="2" />
          <path d="M8 5.5v2.8M8 8.3L4.3 10.9M8 8.3l3.7 2.6" />
        </svg>
      </button>

      {open ? (
        <>
          <div className="fixed inset-0 z-[71] cursor-default" onClick={() => setOpen(false)} aria-hidden="true" />
          <div
            data-testid="dag-activity-popover"
            className="absolute right-0 top-8 z-[72] flex max-h-[70vh] w-[440px] flex-col overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground shadow-2xl"
          >
            <div className="border-b border-border px-3 py-2 text-[11px] font-medium text-muted-foreground">
              DAG runs {hasRunning ? `· ${runningCount} running` : ""}
            </div>
            <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
              {popoverRuns.map((run) => (
                <button
                  key={run.runId}
                  type="button"
                  data-testid={`dag-activity-run-${run.runId}`}
                  onClick={() => setSelectedRunId(run.runId)}
                  className={`flex items-center justify-between gap-2 px-3 py-1.5 text-left text-[11px] transition-colors ${
                    selected?.runId === run.runId ? "bg-accent/30" : "hover:bg-accent/15"
                  }`}
                >
                  <span className="flex min-w-0 items-center gap-1.5">
                    <span aria-hidden="true">
                      {run.status === "running"
                        ? "▶"
                        : run.status === "failed"
                          ? "✗"
                          : run.status === "completed"
                            ? "✓"
                            : "⊘"}
                    </span>
                    <span className="truncate font-medium text-foreground">{run.name}</span>
                  </span>
                  <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
                    {run.status === "running" ? "running · " : ""}
                    {runProgressLabel(run)}
                  </span>
                </button>
              ))}
            </div>
            {selected ? (
              <div className="h-[300px] overflow-hidden border-t border-border">
                <DagGraphView snapshot={selected} />
              </div>
            ) : null}
          </div>
        </>
      ) : null}
    </div>
  );
}
