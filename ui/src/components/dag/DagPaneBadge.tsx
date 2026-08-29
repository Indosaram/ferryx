import { useMemo, useState, useSyncExternalStore } from "react";
import type { DagRunSnapshot } from "../../lib/dagTypes";
import { dagStore } from "../../state/dagStore";
import { DagGraphView } from "./DagGraphView";

const MAX_POPOVER_RUNS = 6;

export type DagPaneBadgeProps = {
  projectPath?: string;
};

function cleanPath(p: string): string {
  const norm = p.replace(/\\/g, "/").trim();
  if (norm.length > 1 && norm.endsWith("/")) {
    return norm.replace(/\/+$/, "");
  }
  return norm;
}

function matchesPathBoundary(panePath: string, projectKey: string): boolean {
  const pane = cleanPath(panePath);
  const proj = cleanPath(projectKey);
  if (!pane || !proj) return false;
  if (pane === proj) return true;
  if (proj === "/") return pane.startsWith("/");
  return pane.startsWith(proj + "/");
}

function resolveProjectRuns(
  state: ReturnType<typeof dagStore.getState>,
  projectPath: string | undefined,
): DagRunSnapshot[] {
  if (!projectPath) return [];
  const normalizedPane = cleanPath(projectPath);
  if (!normalizedPane) return [];

  // Check exact match first
  const exactKey = Object.keys(state.runsByProject).find(
    (k) => cleanPath(k) === normalizedPane,
  );
  if (exactKey && state.runsByProject[exactKey]) {
    return Object.values(state.runsByProject[exactKey]);
  }

  // Find longest prefix match respecting directory path boundaries (nested cwd)
  let bestKey: string | null = null;
  let bestKeyLen = -1;
  for (const key of Object.keys(state.runsByProject)) {
    const cleanedKey = cleanPath(key);
    if (matchesPathBoundary(normalizedPane, cleanedKey)) {
      if (cleanedKey.length > bestKeyLen) {
        bestKey = key;
        bestKeyLen = cleanedKey.length;
      }
    }
  }

  return bestKey ? Object.values(state.runsByProject[bestKey] ?? {}) : [];
}

function runUpdatedAt(run: DagRunSnapshot): number {
  const parsed = run.updatedAt ?? run.completedAt ?? run.startedAt;
  if (parsed === null || parsed === undefined) return Number.NaN;
  const time = Date.parse(parsed);
  return Number.isFinite(time) ? time : Number.NaN;
}

export function DagPaneBadge({ projectPath }: DagPaneBadgeProps): JSX.Element | null {
  const storeState = useSyncExternalStore(dagStore.subscribe, () => dagStore.getState());
  const [open, setOpen] = useState(false);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);

  const runs = useMemo(
    () => resolveProjectRuns(storeState, projectPath),
    [storeState, projectPath],
  );
  const runningRuns = useMemo(
    () =>
      runs
        .filter((run) => run.status === "running")
        .sort((a, b) => runUpdatedAt(b) - runUpdatedAt(a)),
    [runs],
  );

  if (runningRuns.length === 0) return null;

  const popoverRuns = runningRuns.slice(0, MAX_POPOVER_RUNS);
  const selected =
    popoverRuns.find((r) => r.runId === selectedRunId) ??
    popoverRuns[0] ??
    null;

  return (
    <div
      className="no-drag absolute bottom-3 right-5 z-30"
      data-testid="dag-pane-badge"
    >
      <button
        type="button"
        data-testid="dag-pane-badge-button"
        aria-label="dag run in progress"
        onClick={() => setOpen((previous) => !previous)}
        className="flex size-5 items-center justify-center rounded-md border border-border/60 bg-card/85 text-accent backdrop-blur-sm transition-colors hover:bg-accent/25 animate-pulse"
        style={{ filter: "drop-shadow(0 0 4px currentColor)" }}
      >
        <svg
          viewBox="0 0 16 16"
          className="size-3"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.6"
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
          <div
            data-testid="dag-pane-popover-backdrop"
            className="fixed inset-0 z-[69] cursor-default"
            onClick={() => setOpen(false)}
            aria-hidden="true"
          />
          <div
            data-testid="dag-pane-popover"
            className="absolute bottom-8 right-0 z-[70] flex max-h-[60vh] w-[400px] flex-col overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground shadow-2xl"
          >
            <div className="flex items-center justify-between border-b border-border px-3 py-2 text-[11px] font-medium text-muted-foreground">
              <span>
                DAG runs · {runningRuns.length} running
              </span>
            </div>
            <div className="flex min-h-0 flex-col overflow-y-auto">
              {popoverRuns.map((run) => (
                <button
                  key={run.runId}
                  type="button"
                  data-testid={`dag-pane-run-${run.runId}`}
                  onClick={() => setSelectedRunId(run.runId)}
                  className={`flex w-full items-center justify-between gap-2 px-3 py-1.5 text-left text-[11px] transition-colors hover:bg-accent/15 ${
                    selected?.runId === run.runId ? "bg-accent/10 font-medium" : ""
                  }`}
                >
                  <span className="flex min-w-0 items-center gap-1.5">
                    <span aria-hidden="true">▶</span>
                    <span className="truncate text-foreground">{run.name}</span>
                  </span>
                  <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
                    running ·{" "}
                    {run.counts.completed +
                      run.counts.failed +
                      run.counts.cancelled +
                      run.counts.skipped}
                    /{run.counts.total} done
                  </span>
                </button>
              ))}
            </div>
            {selected ? (
              <div className="h-[260px] overflow-hidden border-t border-border">
                <DagGraphView snapshot={selected} />
              </div>
            ) : null}
          </div>
        </>
      ) : null}
    </div>
  );
}
