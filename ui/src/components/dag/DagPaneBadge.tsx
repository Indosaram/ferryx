import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import type { DagRunSnapshot } from "../../lib/dagTypes";
import type { TerminalSession } from "../../lib/types";
import { dagStore } from "../../state/dagStore";
import { dagRunOwnership } from "../../state/dagRunOwnership";
import { DagGraphView } from "./DagGraphView";

export type DagPaneBadgeProps = {
  readonly projectPath?: string;
  /** Identifies this pane as a run owner; sibling panes of one project must not share a run. */
  readonly paneId?: string;
  readonly providerSessionId?: string | null;
  readonly sessions?: Readonly<Record<string, TerminalSession>> | readonly TerminalSession[];
  readonly agentPresent?: boolean;
  readonly agentWorking?: boolean;
};

function cleanPath(p: string): string {
  let norm = p.replace(/\\/g, "/").trim();
  if (norm.startsWith("/private/")) norm = norm.slice("/private".length);
  if (norm.length > 1 && norm.endsWith("/")) norm = norm.replace(/\/+$/, "");
  return norm;
}

function matchesPathBoundary(panePath: string, projectKey: string): boolean {
  if (!panePath || !projectKey) return false;
  if (panePath === projectKey) return true;
  if (projectKey === "/") return panePath.startsWith("/");
  return panePath.startsWith(`${projectKey}/`);
}

function resolveProjectRuns(
  state: ReturnType<typeof dagStore.getState>,
  projectPath: string | undefined,
): DagRunSnapshot[] {
  if (!projectPath) return [];
  const normalizedPane = cleanPath(projectPath);
  if (!normalizedPane) return [];

  let bestKey: string | null = null;
  let bestKeyLength = -1;
  for (const key of Object.keys(state.runsByProject)) {
    const cleanedKey = cleanPath(key);
    if (
      matchesPathBoundary(normalizedPane, cleanedKey) &&
      cleanedKey.length > bestKeyLength
    ) {
      bestKey = key;
      bestKeyLength = cleanedKey.length;
    }
  }

  return bestKey === null ? [] : Object.values(state.runsByProject[bestKey]);
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
  paneId,
  providerSessionId,
  sessions,
  agentPresent = false,
  agentWorking = false,
}: DagPaneBadgeProps): JSX.Element | null {
  const storeState = useSyncExternalStore(dagStore.subscribe, () => dagStore.getState());
  const ownersByRunId = useSyncExternalStore(
    dagRunOwnership.subscribe,
    () => dagRunOwnership.getState(),
  );
  const [open, setOpen] = useState(false);

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

  useEffect(() => {
    dagRunOwnership.retain(
      runningRuns.map((candidate) => candidate.runId),
      runs.map((candidate) => candidate.runId),
    );
  }, [runningRuns, runs]);

  useEffect(() => {
    if (!agentWorking || paneId === undefined || paneId === "") return;
    for (const candidate of runningRuns) dagRunOwnership.claim(candidate.runId, paneId);
  }, [agentWorking, paneId, runningRuns]);

  const exactlyMatchedByAnotherPane = useMemo(() => {
    const matchedRunIds = new Set<string>();
    if (!sessions) return matchedRunIds;
    const sessionList = Array.isArray(sessions) ? sessions : Object.values(sessions);
    for (const candidate of runningRuns) {
      if (typeof candidate.rootSessionId !== "string" || candidate.rootSessionId.trim() === "") {
        continue;
      }
      if (
        sessionList.some(
          (session) =>
            session.id !== paneId &&
            session.providerSession?.id === candidate.rootSessionId,
        )
      ) {
        matchedRunIds.add(candidate.runId);
      }
    }
    return matchedRunIds;
  }, [runningRuns, sessions, paneId]);

  const run = useMemo(() => {
    const exact = runningRuns.find(
      (candidate) =>
        typeof candidate.rootSessionId === "string" &&
        candidate.rootSessionId.trim() !== "" &&
        candidate.rootSessionId === providerSessionId,
    );
    if (exact) return exact;
    const owned = runningRuns.find((candidate) => ownersByRunId[candidate.runId] === paneId);
    if (owned) return owned;
    if (!agentPresent) return null;
    return runningRuns.find(
      (candidate) =>
        ownersByRunId[candidate.runId] === undefined &&
        !exactlyMatchedByAnotherPane.has(candidate.runId),
    ) ?? null;
  }, [runningRuns, ownersByRunId, paneId, providerSessionId, agentPresent, exactlyMatchedByAnotherPane]);

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

  if (run === null) return null;

  return (
    <div
      className="no-drag absolute bottom-0 right-5 z-30"
      data-testid="dag-pane-badge"
    >
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
                    <span className="flex min-w-0 items-center gap-2 text-xs text-foreground">
                      <span className="shrink-0 text-indigo-500">
                        <GraphGlyph />
                      </span>
                      <span className="truncate font-medium" data-testid="dag-pane-modal-title">
                        {run.name}
                      </span>
                    </span>
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
                    data-testid={`dag-pane-run-${run.runId}`}
                  >
                    <DagGraphView snapshot={run} showRunName={false} />
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
