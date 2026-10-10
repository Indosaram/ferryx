import { useCallback, useEffect, useRef, useState } from "react";
import { Activity, RefreshCw } from "lucide-react";

import {
  RESOURCE_SAMPLE_INTERVAL_MS,
  fetchSystemResources,
  formatBytes,
  formatDuration,
  formatPercent,
  sessionsByCost,
  unavailableLabel,
  usageRatio,
  worktreeLabel,
  type HostResourceSnapshot,
} from "../lib/systemResources";
import { IconButton } from "./ui/IconButton";

function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string") return message;
  }
  return "The background service did not answer the resource query.";
}

function Meter({ label, detail, ratio }: { label: string; detail: string; ratio: number | null }) {
  const percent = ratio === null ? 0 : Math.round(ratio * 100);
  return (
    <div className="rounded-lg border border-border bg-background/40 px-3 py-2">
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-[11px] font-medium text-muted-foreground">{label}</span>
        <span className="text-[11px] tabular-nums text-foreground">{ratio === null ? "—" : `${percent}%`}</span>
      </div>
      <div className="mt-1.5 h-1 overflow-hidden rounded-full bg-muted">
        <div
          className="h-full rounded-full bg-status-success transition-[width] duration-300"
          style={{ width: `${Math.min(100, Math.max(0, percent))}%` }}
          data-testid={`meter-${label.toLowerCase().replace(/\s+/g, "-")}`}
        />
      </div>
      <div className="mt-1 text-[10.5px] tabular-nums text-muted-foreground">{detail}</div>
    </div>
  );
}

export function SystemResourcesDialog({ onClose }: { onClose: () => void }) {
  const [snapshot, setSnapshot] = useState<HostResourceSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const alive = useRef(true);

  const refresh = useCallback(async () => {
    try {
      const next = await fetchSystemResources();
      if (!alive.current) return;
      setSnapshot(next);
      setError(null);
    } catch (cause) {
      if (!alive.current) return;
      setError(errorMessage(cause));
    } finally {
      if (alive.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    void refresh();
    // The daemon samples only when asked, so this panel owns the cadence: it re-asks while it is
    // open and stops on unmount, which leaves an idle daemon with no collector to run.
    const timer = window.setInterval(() => {
      void refresh();
    }, RESOURCE_SAMPLE_INTERVAL_MS);
    return () => {
      alive.current = false;
      window.clearInterval(timer);
    };
  }, [refresh]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [onClose]);

  const sessions = snapshot ? sessionsByCost(snapshot.sessions) : [];
  const note = snapshot ? unavailableLabel(snapshot.unavailable) : null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onMouseDown={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="System resources"
        data-testid="system-resources-dialog"
        className="flex max-h-[80vh] w-full max-w-2xl animate-enter flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="flex shrink-0 items-center gap-2 border-b border-border px-4 py-3">
          <Activity aria-hidden="true" className="size-4 text-muted-foreground" />
          <span className="text-sm font-semibold text-foreground">System resources</span>
          {snapshot ? (
            <span className="text-[11px] text-muted-foreground">
              {snapshot.platform} · {snapshot.cpuCount} {snapshot.cpuCount === 1 ? "core" : "cores"} · up{" "}
              {formatDuration(snapshot.uptimeSeconds)}
            </span>
          ) : null}
          <span className="ml-auto flex items-center gap-1">
            <IconButton label="Refresh now" size="sm" onClick={() => void refresh()}>
              <RefreshCw className="size-3.5" />
            </IconButton>
          </span>
        </header>

        <div className="selectable min-h-0 flex-1 overflow-auto p-4">
          {loading && !snapshot ? (
            <p className="text-xs text-muted-foreground">Reading host resources from the background service…</p>
          ) : null}

          {error ? (
            <p role="alert" className="mb-3 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-foreground">
              {error}
            </p>
          ) : null}

          {snapshot ? (
            <>
              <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
                <Meter
                  label="CPU"
                  ratio={snapshot.cpuUtilization}
                  detail={
                    snapshot.loadAverage1m === null
                      ? formatPercent(snapshot.cpuUtilization)
                      : `load ${snapshot.loadAverage1m.toFixed(2)}`
                  }
                />
                <Meter
                  label="Memory"
                  ratio={usageRatio(snapshot.memoryUsedBytes, snapshot.memoryTotalBytes)}
                  detail={`${formatBytes(snapshot.memoryUsedBytes)} / ${formatBytes(snapshot.memoryTotalBytes)}`}
                />
                <Meter
                  label="Swap"
                  ratio={usageRatio(snapshot.swapUsedBytes, snapshot.swapTotalBytes)}
                  detail={`${formatBytes(snapshot.swapUsedBytes)} / ${formatBytes(snapshot.swapTotalBytes)}`}
                />
                <Meter
                  label="Disk"
                  ratio={usageRatio(
                    snapshot.diskTotalBytes === null || snapshot.diskFreeBytes === null
                      ? null
                      : snapshot.diskTotalBytes - snapshot.diskFreeBytes,
                    snapshot.diskTotalBytes,
                  )}
                  detail={
                    snapshot.diskFreeBytes === null
                      ? "—"
                      : `${formatBytes(snapshot.diskFreeBytes)} free of ${formatBytes(snapshot.diskTotalBytes)}`
                  }
                />
              </div>

              <div className="mt-4 flex items-baseline justify-between">
                <h3 className="text-xs font-semibold text-foreground">
                  Sessions ({snapshot.sessions.length})
                </h3>
                <span className="text-[11px] text-muted-foreground">
                  {snapshot.processCount === null ? "" : `${snapshot.processCount} processes on this host`}
                </span>
              </div>

              {sessions.length === 0 ? (
                <p className="mt-2 text-xs text-muted-foreground">No terminal sessions are running in the background service.</p>
              ) : (
                <table className="mt-2 w-full border-collapse text-[11px]">
                  <thead>
                    <tr className="text-left text-muted-foreground">
                      <th className="py-1 pr-2 font-medium">Worktree</th>
                      <th className="py-1 pr-2 font-medium">PID</th>
                      <th className="py-1 pr-2 font-medium">CPU</th>
                      <th className="py-1 pr-2 font-medium">Memory</th>
                      <th className="py-1 font-medium">Procs</th>
                    </tr>
                  </thead>
                  <tbody>
                    {sessions.map((session) => (
                      <tr key={session.sessionId} className="border-t border-border/60" data-testid="resource-session-row">
                        <td className="max-w-[18rem] truncate py-1 pr-2 text-foreground" title={session.worktreePath ?? session.sessionId}>
                          {worktreeLabel(session)}
                        </td>
                        <td className="py-1 pr-2 tabular-nums text-muted-foreground">{session.pid ?? "—"}</td>
                        <td className="py-1 pr-2 tabular-nums text-foreground">
                          {session.cpuPercent === null ? "—" : `${session.cpuPercent.toFixed(1)}%`}
                        </td>
                        <td className="py-1 pr-2 tabular-nums text-foreground">{formatBytes(session.residentBytes)}</td>
                        <td className="py-1 tabular-nums text-muted-foreground">{session.processCount ?? "—"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}

              {note ? <p className="mt-3 text-[10.5px] text-muted-foreground">{note}</p> : null}
            </>
          ) : null}
        </div>
      </div>
    </div>
  );
}
