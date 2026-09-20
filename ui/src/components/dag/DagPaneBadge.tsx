import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import type { DagRunSnapshot } from "../../lib/dagTypes";
import type { TerminalSession } from "../../lib/types";
import { dagStore } from "../../state/dagStore";
import { DagGraphView } from "./DagGraphView";

export type DagPaneBadgeProps = {
  readonly projectPath?: string;
  /** Identifies this pane as a run owner; sibling panes of one project must not share a run. */
  readonly paneId?: string;
  readonly providerSessionId?: string | null;
  readonly sessions?: Readonly<Record<string, TerminalSession>> | readonly TerminalSession[];
  readonly agentPresent?: boolean;
  readonly agentWorking?: boolean;
  readonly retainSettled?: boolean;
};

function collectSessionOwnedRuns(
  state: ReturnType<typeof dagStore.getState>,
  providerSessionId: string | null | undefined,
): DagRunSnapshot[] {
  if (!providerSessionId) return [];
  const owned: DagRunSnapshot[] = [];
  for (const projectRuns of Object.values(state.runsByProject)) {
    for (const run of Object.values(projectRuns)) {
      if (run.rootSessionId === providerSessionId) owned.push(run);
    }
  }
  return owned;
}

function runUpdatedAt(run: DagRunSnapshot): number {
  const parsed = run.updatedAt ?? run.completedAt ?? run.startedAt;
  if (!parsed) return 0;
  const time = Date.parse(parsed);
  return Number.isFinite(time) ? time : 0;
}

function GraphGlyph(): JSX.Element {
  return (
    <svg
      viewBox="0 0 16 16"
      className="size-3.5"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      aria-hidden="true"
    >
      <path d="M8 5.2v2.6M7.2 8.6 4.6 10.6M8.8 8.6l2.6 2" />
      <circle cx="8" cy="3.4" r="1.9" fill="currentColor" stroke="none" />
      <circle cx="3.4" cy="12.2" r="1.9" fill="currentColor" stroke="none" />
      <circle cx="12.6" cy="12.2" r="1.9" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function DagPaneBadge({
  projectPath,
  providerSessionId,
  retainSettled = false,
}: DagPaneBadgeProps): JSX.Element | null {
  const storeState = useSyncExternalStore(dagStore.subscribe, () => dagStore.getState());
  const [open, setOpen] = useState(false);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);

  const runs = useMemo(
    () => collectSessionOwnedRuns(storeState, providerSessionId),
    [storeState, providerSessionId],
  );
  const eligibleRuns = useMemo(
    () =>
      runs
        .filter((run) => (retainSettled ? true : run.status === "running"))
        .sort((a, b) => runUpdatedAt(b) - runUpdatedAt(a)),
    [runs, retainSettled],
  );

  const paneRuns = useMemo(
    () => eligibleRuns.filter((candidate) =>
      typeof candidate.rootSessionId === "string" &&
      candidate.rootSessionId.trim() !== "" &&
      candidate.rootSessionId === providerSessionId,
    ),
    [eligibleRuns, providerSessionId],
  );

  const run = paneRuns.length > 0 ? paneRuns[0] : null;
  const watchFailure = projectPath ? (storeState.watchFailures?.[projectPath] ?? null) : null;
  const activeRun = (selectedRunId ? paneRuns.find((r) => r.runId === selectedRunId) : null) ?? run;

  const visible = run !== null;
  const modalRef = useRef<HTMLDivElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const previousActiveElementRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!open || !visible) return;

    previousActiveElementRef.current = document.activeElement as HTMLElement | null;
    const focusTimer = setTimeout(() => {
      closeButtonRef.current?.focus();
    }, 0);

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        setOpen(false);
        return;
      }
      if (event.key === "Tab" && modalRef.current) {
        const focusable = modalRef.current.querySelectorAll<HTMLElement>(
          'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
        );
        if (focusable.length > 0) {
          const first = focusable[0];
          const last = focusable[focusable.length - 1];
          if (event.shiftKey && document.activeElement === first) {
            event.preventDefault();
            last.focus();
          } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first.focus();
          }
        }
      }
    };

    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      clearTimeout(focusTimer);
      document.removeEventListener("keydown", onKeyDown, true);
      previousActiveElementRef.current?.focus();
    };
  }, [open, visible]);

  if (run === null) {
    if (!watchFailure) return null;
    // The stream stopped for good (helper/capability/auth/unavailable) - say so instead of
    // showing nothing, which is indistinguishable from "no runs yet".
    return (
      <div className="no-drag absolute bottom-0 right-5 z-30" data-testid="dag-pane-badge">
        <span
          role="status"
          data-testid="dag-watch-failure"
          data-code={watchFailure.code}
          aria-label={`DAG stream unavailable: ${watchFailure.code}`}
          title={watchFailure.message}
          className="flex size-5 items-center justify-center rounded-md border border-amber-400/50 bg-zinc-900/85 text-[11px] font-semibold text-amber-300 shadow-[0_0_10px_rgb(251_191_36_/_0.30)] backdrop-blur-sm"
        >
          !
        </span>
      </div>
    );
  }

  return (
    <div
      className="no-drag absolute bottom-0 right-5 z-30"
      data-testid="dag-pane-badge"
    >
      {watchFailure ? (
        <span
          role="status"
          data-testid="dag-watch-failure"
          data-code={watchFailure.code}
          aria-label={`DAG stream unavailable: ${watchFailure.code}`}
          title={watchFailure.message}
          className="mr-1 inline-flex size-5 items-center justify-center rounded-md border border-amber-400/50 bg-zinc-900/85 text-[11px] font-semibold text-amber-300 shadow-[0_0_10px_rgb(251_191_36_/_0.30)] backdrop-blur-sm"
        >
          !
        </span>
      ) : null}
      <button
        type="button"
        data-testid="dag-pane-badge-button"
        aria-label="dag run in progress"
        onClick={() => setOpen((previous) => !previous)}
        className="flex size-5 items-center justify-center rounded-md border border-indigo-400/40 bg-zinc-900/85 text-indigo-300 shadow-[0_0_10px_rgb(99_102_241_/_0.35)] backdrop-blur-sm transition-colors hover:bg-zinc-800 animate-pulse"
        style={{ filter: "drop-shadow(0 0 4px currentColor)" }}
      >
        <GraphGlyph />
      </button>

      {open
        ? createPortal(
            <>
              <div
                data-testid="dag-pane-modal-backdrop"
                className="fixed inset-0 z-[100] bg-black/60"
                onClick={() => setOpen(false)}
                onKeyDown={(e) => e.stopPropagation()}
                aria-hidden="true"
              />
              <div
                className="pointer-events-none fixed inset-0 z-[101] flex items-center justify-center p-6"
                onKeyDown={(e) => e.stopPropagation()}
              >
                <div
                  ref={modalRef}
                  data-testid="dag-pane-modal"
                  role="dialog"
                  aria-modal="true"
                  aria-label="DAG runs"
                  tabIndex={-1}
                  className="pointer-events-auto flex h-[min(820px,86vh)] w-[min(1280px,92vw)] flex-col overflow-hidden rounded-2xl border border-border bg-popover text-popover-foreground shadow-2xl outline-none"
                >
                  <div className="flex items-center justify-between border-b border-border px-4 py-2.5">
                    <div className="flex min-w-0 items-center gap-2 text-xs text-foreground">
                      <span className="shrink-0 text-indigo-500">
                        <GraphGlyph />
                      </span>
                      {paneRuns.length > 1 ? (
                        <div
                          className="flex items-center gap-1.5 overflow-x-auto py-0.5 no-scrollbar"
                          role="tablist"
                          data-testid="dag-pane-modal-tabs"
                        >
                          {paneRuns.map((r) => {
                            const isSelected = r.runId === activeRun?.runId;
                            return (
                              <button
                                key={r.runId}
                                type="button"
                                role="tab"
                                aria-selected={isSelected}
                                data-testid={`dag-pane-modal-tab-${r.runId}`}
                                onClick={() => setSelectedRunId(r.runId)}
                                className={`flex items-center gap-1.5 rounded-lg px-2.5 py-1 text-xs transition-colors ${
                                  isSelected
                                    ? "bg-accent font-medium text-foreground shadow-sm"
                                    : "text-muted-foreground hover:bg-accent/50 hover:text-foreground"
                                }`}
                              >
                                <span
                                  className="truncate max-w-[240px]"
                                  data-testid={isSelected ? "dag-pane-modal-title" : undefined}
                                >
                                  {r.name}
                                </span>
                              </button>
                            );
                          })}
                        </div>
                      ) : (
                        <span className="truncate font-medium" data-testid="dag-pane-modal-title">
                          {activeRun?.name}
                        </span>
                      )}
                    </div>
                    <button
                      ref={closeButtonRef}
                      type="button"
                      data-testid="dag-pane-modal-close"
                      aria-label="Close DAG runs"
                      onClick={() => setOpen(false)}
                      className="rounded-md px-2 py-0.5 text-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                    >
                      ✕
                    </button>
                  </div>
                  <div
                    className="min-h-0 flex-1 overflow-hidden"
                    data-testid={`dag-pane-run-${activeRun?.runId}`}
                  >
                    {activeRun && (
                      <DagGraphView
                        snapshot={activeRun}
                        showRunName={false}
                        projectPath={projectPath}
                      />
                    )}
                  </div>
                </div>
              </div>
            </>,
            document.body,
          )
        : null}
    </div>
  );
}
