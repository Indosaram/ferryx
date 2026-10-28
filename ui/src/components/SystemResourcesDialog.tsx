import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { Activity, ChevronDown, ChevronRight, RefreshCw, X } from "lucide-react";

import {
  RESOURCE_SAMPLE_INTERVAL_MS,
  appendHistory,
  fetchPairedResources,
  fetchSystemResources,
  formatBytes,
  formatDuration,
  formatPercent,
  groupSessionsByWorktree,
  resourceHosts,
  unavailableLabel,
  usageRatio,
  type HostResourceSnapshot,
  type PairedResourceResult,
  type ResourceHistory,
  type ResourceSort,
  type SessionResourceUsage,
} from "../lib/systemResources";
import { closeTerminal } from "../lib/tauri";
import { remoteHostStore, type HostEndpoint } from "../state/remoteHostStore";
import { cn } from "../lib/cn";
import { IconButton } from "./ui/IconButton";

const LOCAL_KEY = "local";

function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string") return message;
  }
  return "The background service did not answer the resource query.";
}

function formatCpu(percent: number | null): string {
  return percent === null ? "—" : `${percent.toFixed(1)}%`;
}

function hostLabel(host: HostEndpoint): string {
  return host.displayName?.trim() || host.name;
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

function Sparkline({ label, samples }: { label: string; samples: readonly (number | null)[] }) {
  const width = 56;
  const height = 16;
  const step = samples.length > 1 ? width / (samples.length - 1) : 0;
  const runs: string[] = [];
  let current: string[] = [];
  samples.forEach((sample, index) => {
    if (sample === null) {
      if (current.length > 0) runs.push(current.join(" "));
      current = [];
      return;
    }
    const y = height - Math.min(1, Math.max(0, sample)) * height;
    current.push(`${(index * step).toFixed(1)},${y.toFixed(1)}`);
  });
  if (current.length > 0) runs.push(current.join(" "));
  return (
    <span className="flex items-center gap-1" title={`${label} history`}>
      <span className="text-[10px] text-muted-foreground">{label}</span>
      <svg width={width} height={height} viewBox={`0 0 ${width} ${height}`} aria-hidden="true" data-testid={`sparkline-${label.toLowerCase()}`}>
        {runs.map((points) => (
          <polyline key={points} points={points.includes(" ") ? points : `${points} ${points}`} fill="none"
            strokeWidth={1} strokeLinecap="round" strokeLinejoin="round" className="stroke-muted-foreground" />
        ))}
      </svg>
    </span>
  );
}

function SortButton({ value, sort, onSort, children, className }: {
  value: ResourceSort; sort: ResourceSort; onSort: (value: ResourceSort) => void; children: string; className?: string;
}) {
  return (
    <button
      type="button"
      aria-pressed={sort === value}
      onClick={() => onSort(value)}
      className={cn("uppercase tracking-wide hover:text-foreground", sort === value ? "font-semibold text-foreground" : "text-muted-foreground", className)}
    >
      {children}
    </button>
  );
}

const CPU_COL = "w-14 text-right";
const MEM_COL = "w-20 text-right";
const ACTION_COL = "flex w-6 justify-end";

function MachineSection({
  machineKey, title, snapshot, history, notice, sort, collapsed, onToggle, onKill,
}: {
  machineKey: string;
  title: string;
  snapshot: HostResourceSnapshot | null;
  history: ResourceHistory | undefined;
  notice: string | null;
  sort: ResourceSort;
  collapsed: ReadonlySet<string>;
  onToggle: (key: string) => void;
  onKill: ((session: SessionResourceUsage) => void) | null;
}) {
  const isCollapsed = collapsed.has(machineKey);
  const groups = snapshot ? groupSessionsByWorktree(snapshot.sessions, sort) : [];
  const note = snapshot ? unavailableLabel(snapshot.unavailable) : null;
  return (
    <section className="border-b border-border/60 last:border-b-0" data-testid="resource-machine" aria-label={title}>
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 px-4 py-2">
        <button
          type="button"
          onClick={() => onToggle(machineKey)}
          aria-expanded={!isCollapsed}
          aria-label={`${isCollapsed ? "Expand" : "Collapse"} ${title}`}
          className="flex max-w-full shrink-0 items-center gap-1.5 text-left"
        >
          {isCollapsed ? <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" /> : <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />}
          <span className="truncate text-xs font-semibold text-foreground">{title}</span>
        </button>
        {snapshot ? (
          <span className="truncate text-[11px] text-muted-foreground">
            {snapshot.platform} · {snapshot.cpuCount} {snapshot.cpuCount === 1 ? "core" : "cores"} · up{" "}
            {formatDuration(snapshot.uptimeSeconds)}
          </span>
        ) : null}
        {history ? (
          <span className="ml-auto flex shrink-0 items-center gap-3">
            <Sparkline label="CPU" samples={history.cpu} />
            <Sparkline label="Mem" samples={history.memory} />
          </span>
        ) : null}
      </div>

      {isCollapsed ? null : (
        <div className="px-4 pb-3">
          {notice ? (
            <p className="mb-2 rounded-md border border-border bg-muted/40 px-3 py-1.5 text-[11px] text-muted-foreground" data-testid="resource-machine-notice">
              {notice}
            </p>
          ) : null}
          {snapshot ? (
            <>
              <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
                <Meter label="CPU" ratio={snapshot.cpuUtilization}
                  detail={snapshot.loadAverage1m === null ? formatPercent(snapshot.cpuUtilization) : `load ${snapshot.loadAverage1m.toFixed(2)}`} />
                <Meter label="Memory" ratio={usageRatio(snapshot.memoryUsedBytes, snapshot.memoryTotalBytes)}
                  detail={`${formatBytes(snapshot.memoryUsedBytes)} / ${formatBytes(snapshot.memoryTotalBytes)}`} />
                <Meter label="Swap" ratio={usageRatio(snapshot.swapUsedBytes, snapshot.swapTotalBytes)}
                  detail={`${formatBytes(snapshot.swapUsedBytes)} / ${formatBytes(snapshot.swapTotalBytes)}`} />
                <Meter label="Disk"
                  ratio={usageRatio(snapshot.diskTotalBytes === null || snapshot.diskFreeBytes === null ? null : snapshot.diskTotalBytes - snapshot.diskFreeBytes, snapshot.diskTotalBytes)}
                  detail={snapshot.diskFreeBytes === null ? "—" : `${formatBytes(snapshot.diskFreeBytes)} free of ${formatBytes(snapshot.diskTotalBytes)}`} />
              </div>

              <div className="mt-3 flex items-baseline justify-between">
                <h3 className="text-[11px] font-semibold text-foreground">Sessions ({snapshot.sessions.length})</h3>
                <span className="text-[11px] text-muted-foreground">
                  {snapshot.processCount === null ? "" : `${snapshot.processCount} processes on this host`}
                </span>
              </div>

              {groups.length === 0 ? (
                <p className="mt-1 text-xs text-muted-foreground">No terminal sessions are running on this machine.</p>
              ) : (
                <div className="mt-1 text-[11px]">
                  {groups.map((group) => {
                    const groupKey = `${machineKey}\u0000${group.key}`;
                    const groupCollapsed = collapsed.has(groupKey);
                    return (
                      <div key={group.key} className="border-t border-border/40" data-testid="resource-worktree-group">
                        <div className="flex items-center gap-2 py-1">
                          <button
                            type="button"
                            onClick={() => onToggle(groupKey)}
                            aria-expanded={!groupCollapsed}
                            aria-label={`${groupCollapsed ? "Expand" : "Collapse"} ${group.label}`}
                            className="flex min-w-0 flex-1 items-center gap-1 text-left"
                          >
                            {groupCollapsed ? <ChevronRight className="size-3 shrink-0 text-muted-foreground" /> : <ChevronDown className="size-3 shrink-0 text-muted-foreground" />}
                            <span className="truncate font-medium text-foreground" title={group.key || undefined}>{group.label}</span>
                            <span className="shrink-0 text-muted-foreground">· {group.sessions.length}</span>
                          </button>
                          <span className={cn(CPU_COL, "tabular-nums text-foreground")}>{formatCpu(group.cpuPercent)}</span>
                          <span className={cn(MEM_COL, "tabular-nums text-foreground")}>{formatBytes(group.residentBytes)}</span>
                          <span className={ACTION_COL} aria-hidden="true" />
                        </div>
                        {groupCollapsed ? null : group.sessions.map((session) => (
                          <div key={session.sessionId} className="group/session flex items-center gap-2 py-0.5 pl-5" data-testid="resource-session-row">
                            <span className="min-w-0 flex-1 truncate text-muted-foreground" title={session.sessionId}>
                              {session.sessionId}
                              <span className="ml-2 tabular-nums">pid {session.pid ?? "—"} · {session.processCount ?? "—"} procs</span>
                            </span>
                            <span className={cn(CPU_COL, "tabular-nums text-foreground")}>{formatCpu(session.cpuPercent)}</span>
                            <span className={cn(MEM_COL, "tabular-nums text-foreground")}>{formatBytes(session.residentBytes)}</span>
                            <span className={ACTION_COL}>
                              {onKill ? (
                                <IconButton
                                  label={`Kill session ${session.sessionId}`}
                                  size="sm"
                                  className="size-5 opacity-0 hover:text-destructive focus-visible:opacity-100 group-hover/session:opacity-100"
                                  onClick={() => onKill(session)}
                                >
                                  <X className="size-3" />
                                </IconButton>
                              ) : null}
                            </span>
                          </div>
                        ))}
                      </div>
                    );
                  })}
                </div>
              )}
              {note ? <p className="mt-2 text-[10.5px] text-muted-foreground">{note}</p> : null}
            </>
          ) : null}
        </div>
      )}
    </section>
  );
}

export function SystemResourcesDialog({
  onClose,
  killSession = closeTerminal,
}: {
  onClose: () => void;
  killSession?: (backendSessionId: string) => Promise<void>;
}) {
  const [snapshot, setSnapshot] = useState<HostResourceSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [paired, setPaired] = useState<Readonly<Record<string, PairedResourceResult>>>({});
  const [history, setHistory] = useState<Readonly<Record<string, ResourceHistory>>>({});
  const [sort, setSort] = useState<ResourceSort>("memory");
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const [killTarget, setKillTarget] = useState<SessionResourceUsage | null>(null);
  const [killing, setKilling] = useState(false);
  const [killError, setKillError] = useState<string | null>(null);
  const alive = useRef(true);

  const hostMap = useSyncExternalStore(remoteHostStore.subscribe, () => remoteHostStore.getState().hosts);
  const hosts = resourceHosts(hostMap);
  // Generation is part of the key: a re-paired host must not keep its predecessor's samples.
  const hostKey = hosts.map((host) => `${host.hostId}\u0000${host.generation}`).join("\n");

  const refresh = useCallback(async () => {
    try {
      const next = await fetchSystemResources();
      if (!alive.current) return;
      setSnapshot(next);
      setHistory((prev) => ({ ...prev, [LOCAL_KEY]: appendHistory(prev[LOCAL_KEY], next) }));
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
    let cancelled = false;
    let timer: number | undefined;
    // The daemon samples only when asked, so this panel owns the cadence: it re-asks while it is
    // open and stops on unmount, which leaves an idle daemon with no collector to run. The next
    // ask is scheduled only after the previous one settles, so slow samples never pile up.
    const sample = async () => {
      await refresh();
      if (cancelled) return;
      timer = window.setTimeout(() => {
        void sample();
      }, RESOURCE_SAMPLE_INTERVAL_MS);
    };
    void sample();
    return () => {
      cancelled = true;
      alive.current = false;
      window.clearTimeout(timer);
    };
  }, [refresh]);

  useEffect(() => {
    if (hostKey === "") return;
    let cancelled = false;
    const timers = new Map<string, number>();
    const targets = hostKey.split("\n").map((entry) => {
      const [hostId, generation] = entry.split("\u0000");
      return { hostId: hostId!, generation: generation! };
    });
    const sample = async (target: { hostId: string; generation: string }) => {
      const result = await fetchPairedResources(target);
      if (cancelled) return;
      setPaired((prev) => ({ ...prev, [target.hostId]: result }));
      if (result.status === "sampled") {
        setHistory((prev) => ({ ...prev, [target.hostId]: appendHistory(prev[target.hostId], result.snapshot) }));
      }
      timers.set(target.hostId, window.setTimeout(() => void sample(target), RESOURCE_SAMPLE_INTERVAL_MS));
    };
    targets.forEach((target) => void sample(target));
    return () => {
      cancelled = true;
      timers.forEach((timer) => window.clearTimeout(timer));
      setPaired({});
      setHistory((prev) => {
        const local = prev[LOCAL_KEY];
        const next: Record<string, ResourceHistory> = {};
        if (local) next[LOCAL_KEY] = local;
        return next;
      });
    };
  }, [hostKey]);

  const refreshAll = useCallback(() => {
    void refresh();
    for (const host of hosts) {
      void fetchPairedResources(host).then((result) => {
        if (alive.current) setPaired((prev) => ({ ...prev, [host.hostId]: result }));
      });
    }
  }, [refresh, hosts]);

  const toggle = useCallback((key: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const confirmKill = useCallback(async () => {
    if (!killTarget) return;
    setKilling(true);
    setKillError(null);
    try {
      await killSession(killTarget.sessionId);
      if (!alive.current) return;
      setKillTarget(null);
      void refresh();
    } catch (cause) {
      if (alive.current) setKillError(errorMessage(cause));
    } finally {
      if (alive.current) setKilling(false);
    }
  }, [killSession, killTarget, refresh]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      if (killTarget) {
        if (!killing) setKillTarget(null);
        return;
      }
      onClose();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [onClose, killTarget, killing]);

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
        className="relative flex max-h-[80vh] w-full max-w-2xl animate-enter flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="flex shrink-0 items-center gap-2 border-b border-border px-4 py-3">
          <Activity aria-hidden="true" className="size-4 text-muted-foreground" />
          <span className="text-sm font-semibold text-foreground">System resources</span>
          <span className="text-[11px] text-muted-foreground">
            {1 + hosts.length} {hosts.length === 0 ? "machine" : "machines"}
          </span>
          <span className="ml-auto flex items-center gap-1">
            <IconButton label="Refresh now" size="sm" onClick={refreshAll}>
              <RefreshCw className="size-3.5" />
            </IconButton>
          </span>
        </header>

        <div className="flex shrink-0 items-center gap-2 border-b border-border/60 bg-muted/30 px-4 py-1 text-[10px]">
          <SortButton value="name" sort={sort} onSort={setSort} className="flex-1 text-left">Name</SortButton>
          <SortButton value="cpu" sort={sort} onSort={setSort} className={CPU_COL}>CPU</SortButton>
          <SortButton value="memory" sort={sort} onSort={setSort} className={MEM_COL}>Memory</SortButton>
          <span className={ACTION_COL} aria-hidden="true" />
        </div>

        <div className="selectable min-h-0 flex-1 overflow-auto">
          {error ? (
            <p role="alert" className="mx-4 mt-3 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-foreground">
              {error}
            </p>
          ) : null}

          <MachineSection
            machineKey={LOCAL_KEY}
            title="This machine"
            snapshot={snapshot}
            history={history[LOCAL_KEY]}
            notice={loading && !snapshot ? "Reading host resources from the background service…" : null}
            sort={sort}
            collapsed={collapsed}
            onToggle={toggle}
            onKill={(session) => {
              setKillError(null);
              setKillTarget(session);
            }}
          />
          {hosts.map((host) => {
            const result = paired[host.hostId];
            const label = hostLabel(host);
            const notice = !result
              ? `Reading ${label}…`
              : result.status === "unreachable"
                ? `Couldn't reach ${label}; its resources are unknown.`
                : result.status === "update-required"
                  ? `Update Ferryx on ${label} to see its resources.`
                  : null;
            return (
              <MachineSection
                key={host.hostId}
                machineKey={host.hostId}
                title={label}
                snapshot={result?.status === "sampled" ? result.snapshot : null}
                history={history[host.hostId]}
                notice={notice}
                sort={sort}
                collapsed={collapsed}
                onToggle={toggle}
                onKill={null}
              />
            );
          })}
        </div>

        {killTarget ? (
          <div className="absolute inset-0 z-10 flex items-center justify-center bg-black/40 p-6">
            <div role="alertdialog" aria-modal="true" aria-label="Kill session" className="w-full max-w-sm rounded-lg border border-border bg-card p-4 shadow-xl">
              <h2 className="text-sm font-semibold text-foreground">Kill {killTarget.sessionId}?</h2>
              <p className="mt-1 text-xs text-muted-foreground">
                Force-quits this terminal and every process in it. Unsaved work in the pane is lost. This can't be undone.
              </p>
              {killError ? <p role="alert" className="mt-2 text-xs text-destructive">{killError}</p> : null}
              <div className="mt-4 flex justify-end gap-2">
                <button type="button" disabled={killing} onClick={() => setKillTarget(null)}
                  className="rounded-md border border-border px-3 py-1.5 text-xs text-foreground hover:bg-accent disabled:opacity-50">
                  Cancel
                </button>
                <button type="button" disabled={killing} onClick={() => void confirmKill()}
                  className="rounded-md bg-destructive px-3 py-1.5 text-xs text-destructive-foreground hover:opacity-90 disabled:opacity-50">
                  {killing ? "Killing…" : "Kill session"}
                </button>
              </div>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
