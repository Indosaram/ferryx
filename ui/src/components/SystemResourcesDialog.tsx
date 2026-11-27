import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import {
  Activity,
  AlertCircle,
  ArrowUpCircle,
  ChevronDown,
  ChevronRight,
  FolderGit2,
  Laptop,
  Layers,
  LoaderCircle,
  RefreshCw,
  Search,
  Server,
  Terminal,
  X,
} from "lucide-react";

import {
  RESOURCE_SAMPLE_INTERVAL_MS,
  SSH_RESOURCE_SAMPLE_INTERVAL_MS,
  appendHistory,
  fetchPairedResources,
  fetchSshResources,
  fetchSystemResources,
  formatBytes,
  formatDuration,
  formatPercent,
  groupSessionsByWorktree,
  resourceHosts,
  sshResourceHosts,
  unavailableLabel,
  usageRatio,
  type HostResourceSnapshot,
  type PairedResourceResult,
  type ResourceHistory,
  type ResourceSort,
  type SessionResourceUsage,
} from "../lib/systemResources";
import { closeTerminal } from "../lib/tauri";
import { listSshHosts, type SshHost } from "../lib/sshHosts";
import { remoteHostStore, type HostEndpoint } from "../state/remoteHostStore";
import { cn } from "../lib/cn";
import { floatingSurface } from "../lib/surface";
import { IconButton } from "./ui/IconButton";

const LOCAL_KEY = "local";
/** SSH samples share the pane with paired ones, so their keys are namespaced apart. */
const SSH_KEY_PREFIX = "ssh:";

type MachineOrigin = "local" | "paired" | "ssh";
type MachineStatus = "sampled" | "loading" | "unreachable" | "update-required" | "error";

type MachineEntry = {
  readonly machineKey: string;
  readonly title: string;
  readonly origin: MachineOrigin;
  readonly status: MachineStatus;
  readonly snapshot: HostResourceSnapshot | null;
  readonly history: ResourceHistory | undefined;
  readonly notice: string | null;
  readonly onKill: ((session: SessionResourceUsage) => void) | null;
};

const ORIGIN_LABEL: Record<MachineOrigin, string> = {
  local: "Local",
  paired: "Paired",
  ssh: "SSH",
};

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

function meterFillClass(ratio: number | null): string {
  if (ratio === null) return "bg-muted-foreground/40";
  if (ratio >= 0.9) return "bg-destructive";
  if (ratio >= 0.7) return "bg-status-warning";
  return "bg-status-success";
}

function Meter({ label, detail, ratio }: { label: string; detail: string; ratio: number | null }) {
  const clamped = ratio === null ? 0 : Math.min(100, Math.max(0, Math.round(ratio * 100)));
  return (
    <div className="px-4 py-2.5">
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-[10.5px] font-medium text-muted-foreground">{label}</span>
        <span className="text-[13px] font-semibold tabular-nums text-foreground">
          {ratio === null ? "—" : `${clamped}%`}
        </span>
      </div>
      <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-muted">
        <div
          className={cn(
            "h-full w-full origin-left rounded-full transition-transform duration-300 motion-reduce:transition-none",
            meterFillClass(ratio),
          )}
          style={{ transform: `scaleX(${clamped / 100})` }}
          data-testid={`meter-${label.toLowerCase().replace(/\s+/g, "-")}`}
        />
      </div>
      <div className="mt-1 truncate text-[10.5px] tabular-nums text-muted-foreground">{detail}</div>
    </div>
  );
}

function Sparkline({
  label,
  samples,
  valueText,
  tone,
}: {
  label: string;
  samples: readonly (number | null)[];
  valueText?: string;
  tone: "cpu" | "memory";
}) {
  const width = 52;
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
    const y = height - Math.min(1, Math.max(0, sample)) * (height - 2) - 1;
    if (samples.length === 1) {
      runs.push(`0.0,${y.toFixed(1)} ${width.toFixed(1)},${y.toFixed(1)}`);
      return;
    }
    current.push(`${(index * step).toFixed(1)},${y.toFixed(1)}`);
  });
  if (current.length > 0) runs.push(current.join(" "));
  return (
    <span className="inline-flex items-center gap-1.5" title={`${label} history`}>
      <span className="text-[10px] font-medium text-muted-foreground">{label}</span>
      <svg
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        aria-hidden="true"
        data-testid={`sparkline-${label.toLowerCase()}`}
      >
        {runs.map((points) => (
          <polyline
            key={points}
            points={points.includes(" ") ? points : `${points} ${points}`}
            fill="none"
            strokeWidth={1.25}
            strokeLinecap="round"
            strokeLinejoin="round"
            className={tone === "cpu" ? "stroke-status-working" : "stroke-status-success"}
          />
        ))}
      </svg>
      {valueText ? (
        <span className="w-8 text-right text-[11px] font-medium tabular-nums text-foreground">
          {valueText}
        </span>
      ) : null}
    </span>
  );
}

function MachineStatusDot({ status }: { status: MachineStatus }) {
  if (status === "sampled") {
    return <span aria-hidden="true" className="size-2 shrink-0 rounded-full bg-status-success" />;
  }
  if (status === "update-required") {
    return <span aria-hidden="true" className="size-2 shrink-0 rounded-full bg-status-warning" />;
  }
  if (status === "loading") {
    return <span aria-hidden="true" className="size-2 shrink-0 rounded-full bg-status-idle" />;
  }
  return <span aria-hidden="true" className="size-2 shrink-0 rounded-full border border-muted-foreground/70 bg-transparent" />;
}

function MachineStatusPill({ status }: { status: MachineStatus }) {
  if (status === "sampled") return null;
  if (status === "update-required") {
    return (
      <span className="shrink-0 rounded-xs bg-status-warning-container px-1.5 py-0.5 text-[10px] font-medium text-status-warning-on">
        Update needed
      </span>
    );
  }
  if (status === "loading") {
    return (
      <span className="shrink-0 rounded-xs bg-status-idle-container px-1.5 py-0.5 text-[10px] font-medium text-status-idle-on">
        Reading
      </span>
    );
  }
  return (
    <span className="shrink-0 rounded-xs bg-status-idle-container px-1.5 py-0.5 text-[10px] font-medium text-status-idle-on">
      Offline
    </span>
  );
}

function SortButton({
  value,
  sort,
  onSort,
  children,
}: {
  value: ResourceSort;
  sort: ResourceSort;
  onSort: (value: ResourceSort) => void;
  children: string;
}) {
  const active = sort === value;
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={() => onSort(value)}
      className={cn(
        "min-h-6 rounded-sm px-2 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        active
          ? "bg-popover text-foreground shadow-elevation-1"
          : "text-muted-foreground hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

const CPU_COL = "w-14 shrink-0 text-right";
const MEM_COL = "w-16 shrink-0 text-right sm:w-20";
const ACTION_COL = "flex w-6 shrink-0 justify-end";

function MachineRailItem({
  entry,
  selected,
  onSelect,
}: {
  entry: MachineEntry;
  selected: boolean;
  onSelect: () => void;
}) {
  const HardwareIcon = entry.origin === "local" ? Laptop : Server;
  const memRatio = entry.snapshot
    ? usageRatio(entry.snapshot.memoryUsedBytes, entry.snapshot.memoryTotalBytes)
    : null;
  const sessionCount = entry.snapshot?.sessions.length ?? 0;

  let summary: string;
  if (entry.snapshot) {
    summary = `CPU ${formatPercent(entry.snapshot.cpuUtilization)} · Mem ${formatPercent(memRatio)}`;
  } else if (entry.status === "loading") {
    summary = "Reading…";
  } else if (entry.status === "update-required") {
    summary = "Update needed";
  } else {
    summary = "Offline";
  }

  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={selected ? "true" : undefined}
      data-testid="resource-machine-nav-item"
      data-machine-key={entry.machineKey}
      className={cn(
        "flex w-full flex-col gap-0.5 rounded-lg px-2.5 py-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        selected
          ? "bg-accent text-foreground"
          : "text-muted-foreground hover:bg-surface-hover/50 hover:text-foreground",
      )}
    >
      <span className="flex w-full items-center gap-1.5">
        <MachineStatusDot status={entry.status} />
        <HardwareIcon aria-hidden="true" className="size-3.5 shrink-0 text-muted-foreground" />
        <span className={cn("min-w-0 flex-1 truncate text-xs font-medium", selected ? "text-foreground" : "text-foreground/90")}>
          {entry.title}
        </span>
        {entry.snapshot ? (
          <span
            className="shrink-0 rounded-full bg-muted px-1.5 py-0.5 text-[10px] leading-none tabular-nums text-muted-foreground"
            title={`${sessionCount} ${sessionCount === 1 ? "session" : "sessions"}`}
          >
            {sessionCount}
          </span>
        ) : null}
      </span>
      <span className="block truncate pl-3.5 text-[10.5px] tabular-nums text-muted-foreground">
        {summary}
      </span>
    </button>
  );
}

function MachineSection({
  entry,
  sort,
  collapsed,
  onToggle,
}: {
  entry: MachineEntry;
  sort: ResourceSort;
  collapsed: ReadonlySet<string>;
  onToggle: (key: string) => void;
}) {
  const { machineKey, title, origin, status, snapshot, history, notice, onKill } = entry;
  const isCollapsed = collapsed.has(machineKey);
  const groups = snapshot ? groupSessionsByWorktree(snapshot.sessions, sort) : [];
  const note = snapshot ? unavailableLabel(snapshot.unavailable) : null;
  const HardwareIcon = origin === "local" ? Laptop : Server;
  const memRatio = snapshot ? usageRatio(snapshot.memoryUsedBytes, snapshot.memoryTotalBytes) : null;
  const swapRatio = snapshot ? usageRatio(snapshot.swapUsedBytes, snapshot.swapTotalBytes) : null;
  const diskRatio = snapshot
    ? usageRatio(
        snapshot.diskTotalBytes === null || snapshot.diskFreeBytes === null
          ? null
          : snapshot.diskTotalBytes - snapshot.diskFreeBytes,
        snapshot.diskTotalBytes,
      )
    : null;

  return (
    <section
      className="overflow-hidden rounded-xl border border-border bg-panel shadow-elevation-1"
      data-testid="resource-machine"
      data-machine-level="host"
      aria-label={title}
    >
      <div
        className={cn(
          "flex flex-wrap items-center gap-x-3 gap-y-2 bg-muted/35 px-4 py-3",
          !isCollapsed && "border-b border-border",
        )}
      >
        <button
          type="button"
          onClick={() => onToggle(machineKey)}
          aria-expanded={!isCollapsed}
          aria-label={`${isCollapsed ? "Expand" : "Collapse"} ${title}`}
          className="-ml-1 flex min-h-7 min-w-0 max-w-full items-center gap-2 rounded-md px-1 text-left transition-colors hover:bg-surface-hover/40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          {isCollapsed ? (
            <ChevronRight aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
          ) : (
            <ChevronDown aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
          )}
          <span className="flex size-7 shrink-0 items-center justify-center rounded-md bg-background text-foreground">
            <HardwareIcon aria-hidden="true" className="size-3.5" />
          </span>
          <span className="min-w-0">
            <span className="flex items-center gap-1.5">
              <span className="truncate text-sm font-semibold leading-tight text-foreground">{title}</span>
              <span className="shrink-0 rounded-xs bg-muted px-1.5 py-0.5 text-[10px] font-medium leading-none text-muted-foreground">
                {ORIGIN_LABEL[origin]}
              </span>
              <MachineStatusPill status={status} />
            </span>
            {snapshot ? (
              <span className="mt-0.5 block truncate text-[11px] tabular-nums text-muted-foreground">
                {snapshot.platform} · {snapshot.cpuCount} {snapshot.cpuCount === 1 ? "core" : "cores"} · up{" "}
                {formatDuration(snapshot.uptimeSeconds)}
              </span>
            ) : null}
          </span>
        </button>

        {history ? (
          <span className="ml-auto flex shrink-0 items-center gap-3.5">
            <Sparkline
              label="CPU"
              samples={history.cpu}
              tone="cpu"
              valueText={snapshot ? formatPercent(snapshot.cpuUtilization) : undefined}
            />
            <Sparkline
              label="Mem"
              samples={history.memory}
              tone="memory"
              valueText={snapshot ? formatPercent(memRatio) : undefined}
            />
          </span>
        ) : null}
      </div>

      {isCollapsed ? null : (
        <div>
          {notice ? (
            <p
              className="mx-4 my-3 flex items-center gap-2 rounded-lg bg-muted/40 px-3 py-2 text-xs text-muted-foreground"
              data-testid="resource-machine-notice"
            >
              {status === "loading" ? (
                <LoaderCircle aria-hidden="true" className="size-3.5 shrink-0 animate-spin motion-reduce:animate-none" />
              ) : status === "update-required" ? (
                <ArrowUpCircle aria-hidden="true" className="size-3.5 shrink-0 text-status-warning" />
              ) : (
                <AlertCircle aria-hidden="true" className="size-3.5 shrink-0 text-muted-foreground" />
              )}
              <span>{notice}</span>
            </p>
          ) : null}

          {snapshot ? (
            <>
              <div className="grid grid-cols-2 divide-y divide-border/60 border-b border-border bg-background/40 sm:grid-cols-4 sm:divide-x sm:divide-y-0">
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
                  ratio={memRatio}
                  detail={`${formatBytes(snapshot.memoryUsedBytes)} / ${formatBytes(snapshot.memoryTotalBytes)}`}
                />
                <Meter
                  label="Swap"
                  ratio={swapRatio}
                  detail={`${formatBytes(snapshot.swapUsedBytes)} / ${formatBytes(snapshot.swapTotalBytes)}`}
                />
                <Meter
                  label="Disk"
                  ratio={diskRatio}
                  detail={
                    snapshot.diskFreeBytes === null
                      ? "—"
                      : `${formatBytes(snapshot.diskFreeBytes)} free of ${formatBytes(snapshot.diskTotalBytes)}`
                  }
                />
              </div>

              <div className="bg-background/65" data-testid="resource-sessions-region">
                <div className="flex items-center gap-2 border-b border-border/60 bg-muted/20 px-4 py-1.5">
                  <Terminal aria-hidden="true" className="size-3.5 shrink-0 text-muted-foreground" />
                  <h3 className="shrink-0 whitespace-nowrap text-[11px] font-semibold text-foreground">
                    Sessions ({snapshot.sessions.length})
                  </h3>
                  {snapshot.processCount !== null ? (
                    <span className="min-w-0 truncate text-[11px] tabular-nums text-muted-foreground">
                      · {snapshot.processCount} processes on this host
                    </span>
                  ) : null}
                  {groups.length > 0 ? (
                    <>
                      <span className={cn(CPU_COL, "ml-auto text-[10px] font-medium uppercase tracking-wider text-muted-foreground")}>
                        CPU
                      </span>
                      <span className={cn(MEM_COL, "text-[10px] font-medium uppercase tracking-wider text-muted-foreground")}>
                        Memory
                      </span>
                      <span className={ACTION_COL} aria-hidden="true" />
                    </>
                  ) : null}
                </div>

                {groups.length === 0 ? (
                  <p className="px-4 py-3 text-xs text-muted-foreground">
                    No terminal sessions are running on this machine.
                  </p>
                ) : (
                  <div className="py-1">
                    {groups.map((group) => {
                      const groupKey = `${machineKey}\u0000${group.key}`;
                      const groupCollapsed = collapsed.has(groupKey);
                      return (
                        <div
                          key={group.key}
                          className="border-t border-border/50 first:border-t-0"
                          data-testid="resource-worktree-group"
                          data-resource-level="worktree"
                        >
                          <div className="flex items-center gap-2 px-4 py-1.5 transition-colors hover:bg-surface-hover/35">
                            <button
                              type="button"
                              onClick={() => onToggle(groupKey)}
                              aria-expanded={!groupCollapsed}
                              aria-label={`${groupCollapsed ? "Expand" : "Collapse"} ${group.label}`}
                              className="flex min-h-6 min-w-0 flex-1 items-center gap-1.5 rounded-sm text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                            >
                              {groupCollapsed ? (
                                <ChevronRight aria-hidden="true" className="size-3 shrink-0 text-muted-foreground" />
                              ) : (
                                <ChevronDown aria-hidden="true" className="size-3 shrink-0 text-muted-foreground" />
                              )}
                              <FolderGit2 aria-hidden="true" className="size-3.5 shrink-0 text-muted-foreground" />
                              <span
                                className="truncate text-xs font-medium text-foreground"
                                title={group.key || undefined}
                              >
                                {group.label}
                              </span>
                              <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">
                                · {group.sessions.length}
                              </span>
                            </button>
                            <span className={cn(CPU_COL, "text-xs font-medium tabular-nums text-foreground")}>
                              {formatCpu(group.cpuPercent)}
                            </span>
                            <span className={cn(MEM_COL, "text-xs font-medium tabular-nums text-foreground")}>
                              {formatBytes(group.residentBytes)}
                            </span>
                            <span className={ACTION_COL} aria-hidden="true" />
                          </div>

                          {groupCollapsed ? null : (
                            <div className="mb-1.5 ml-[23px] space-y-0.5 border-l border-border/75 pl-2.5 pr-4">
                              {group.sessions.map((session) => (
                                <div
                                  key={session.sessionId}
                                  className="group/session flex min-h-7 items-center gap-2 rounded-md pl-2 transition-colors hover:bg-surface-hover/45"
                                  data-testid="resource-session-row"
                                  data-resource-level="session"
                                >
                                  <Terminal aria-hidden="true" className="size-3 shrink-0 text-muted-foreground/70" />
                                  <span
                                    className="flex min-w-0 flex-1 flex-col overflow-hidden sm:flex-row sm:items-baseline sm:gap-2"
                                    title={session.sessionId}
                                  >
                                    <span className="truncate font-mono text-[11px] leading-tight text-foreground/85">
                                      {session.sessionId}
                                    </span>
                                    <span className="truncate text-[10.5px] leading-tight tabular-nums text-muted-foreground sm:shrink-0">
                                      pid {session.pid ?? "—"} · {session.processCount ?? "—"} procs
                                    </span>
                                  </span>
                                  <span
                                    className={cn(
                                      CPU_COL,
                                      "text-[11px] tabular-nums",
                                      session.cpuPercent !== null && session.cpuPercent >= 10
                                        ? "font-medium text-foreground"
                                        : "text-muted-foreground",
                                    )}
                                  >
                                    {formatCpu(session.cpuPercent)}
                                  </span>
                                  <span className={cn(MEM_COL, "text-[11px] tabular-nums text-muted-foreground")}>
                                    {formatBytes(session.residentBytes)}
                                  </span>
                                  <span className={ACTION_COL}>
                                    {onKill ? (
                                      <IconButton
                                        label={`Kill session ${session.sessionId}`}
                                        size="sm"
                                        className="size-6 opacity-0 hover:text-destructive focus-visible:opacity-100 group-hover/session:opacity-100"
                                        onClick={() => onKill(session)}
                                      >
                                        <X className="size-3" />
                                      </IconButton>
                                    ) : null}
                                  </span>
                                </div>
                              ))}
                            </div>
                          )}
                        </div>
                      );
                    })}
                  </div>
                )}

                {note ? (
                  <p className="border-t border-border/50 px-4 py-2 text-[10.5px] text-muted-foreground">
                    {note}
                  </p>
                ) : null}
              </div>
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
  const [sshHosts, setSshHosts] = useState<readonly SshHost[]>([]);
  const [sshResults, setSshResults] = useState<Readonly<Record<string, PairedResourceResult>>>({});
  const [history, setHistory] = useState<Readonly<Record<string, ResourceHistory>>>({});
  const [sort, setSort] = useState<ResourceSort>("memory");
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const [selectedMachine, setSelectedMachine] = useState<string | null>(null);
  const [machineQuery, setMachineQuery] = useState("");
  const [killTarget, setKillTarget] = useState<SessionResourceUsage | null>(null);
  const [killing, setKilling] = useState(false);
  const [killError, setKillError] = useState<string | null>(null);
  const alive = useRef(true);

  const hostMap = useSyncExternalStore(remoteHostStore.subscribe, () => remoteHostStore.getState().hosts);
  const hosts = resourceHosts(hostMap);
  // Generation is part of the key: a re-paired host must not keep its predecessor's samples.
  const hostKey = hosts.map((host) => `${host.hostId}\u0000${host.generation}`).join("\n");
  // `resourceHosts` returns a fresh array every render, so the names are keyed by value: a memo on
  // the array identity would rebuild every render and re-arm the SSH sampling effect forever.
  const pairedNamesKey = hosts.map((host) => hostLabel(host).trim().toLowerCase()).join("\u0000");
  const sshTargets = useMemo(
    // One machine can be both paired and reachable over SSH; a paired row owns the name.
    () => sshResourceHosts(sshHosts, pairedNamesKey.split("\u0000")),
    [pairedNamesKey, sshHosts],
  );

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

  useEffect(() => {
    let cancelled = false;
    void listSshHosts().then((next) => {
      if (!cancelled) setSshHosts(next);
    }).catch(() => {
      // A host store that cannot be read leaves the pane with the machines it does know.
    });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (sshTargets.length === 0) return;
    let cancelled = false;
    const timers = new Map<string, number>();
    const sample = async (host: SshHost) => {
      const result = await fetchSshResources(host);
      if (cancelled) return;
      setSshResults((prev) => ({ ...prev, [host.id]: result }));
      if (result.status === "sampled") {
        setHistory((prev) => ({ ...prev, [`${SSH_KEY_PREFIX}${host.id}`]: appendHistory(prev[`${SSH_KEY_PREFIX}${host.id}`], result.snapshot) }));
      }
      timers.set(host.id, window.setTimeout(() => void sample(host), SSH_RESOURCE_SAMPLE_INTERVAL_MS));
    };
    sshTargets.forEach((host) => void sample(host));
    return () => {
      cancelled = true;
      timers.forEach((timer) => window.clearTimeout(timer));
      setSshResults({});
    };
  }, [sshTargets]);

  const refreshAll = useCallback(() => {
    void refresh();
    for (const host of hosts) {
      void fetchPairedResources(host).then((result) => {
        if (alive.current) setPaired((prev) => ({ ...prev, [host.hostId]: result }));
      });
    }
    for (const host of sshTargets) {
      void fetchSshResources(host).then((result) => {
        if (alive.current) setSshResults((prev) => ({ ...prev, [host.id]: result }));
      });
    }
  }, [refresh, hosts, sshTargets]);

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
      if (event.target instanceof HTMLInputElement && event.target.value !== "") {
        event.preventDefault();
        setMachineQuery("");
        return;
      }
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

  const machines: MachineEntry[] = [
    {
      machineKey: LOCAL_KEY,
      title: "This machine",
      origin: "local",
      status: snapshot ? "sampled" : loading ? "loading" : "error",
      snapshot,
      history: history[LOCAL_KEY],
      notice: loading && !snapshot ? "Reading host resources from the background service…" : null,
      onKill: (session) => {
        setKillError(null);
        setKillTarget(session);
      },
    },
    ...hosts.map((host): MachineEntry => {
      const result = paired[host.hostId];
      const label = hostLabel(host);
      const notice = !result
        ? `Reading ${label}…`
        : result.status === "unreachable"
          ? `Couldn't reach ${label}; its resources are unknown.`
          : result.status === "update-required"
            ? `Update Ferryx on ${label} to see its resources.`
            : null;
      return {
        machineKey: host.hostId,
        title: label,
        origin: "paired",
        status: result ? result.status : "loading",
        snapshot: result?.status === "sampled" ? result.snapshot : null,
        history: history[host.hostId],
        notice,
        onKill: null,
      };
    }),
    ...sshTargets.map((host): MachineEntry => {
      const machineKey = `${SSH_KEY_PREFIX}${host.id}`;
      const result = sshResults[host.id];
      const notice = !result
        ? `Reading ${host.label}…`
        : result.status === "unreachable"
          ? `Couldn't sample ${host.label} over SSH; its resources are unknown.`
          : result.status === "update-required"
            ? `Update Ferryx on ${host.label} to see its resources.`
            : null;
      return {
        machineKey,
        title: host.label,
        origin: "ssh",
        status: result ? result.status : "loading",
        snapshot: result?.status === "sampled" ? result.snapshot : null,
        history: history[machineKey],
        notice,
        onKill: null,
      };
    }),
  ];

  const totalMachines = machines.length;
  const totalSessions = machines.reduce((sum, entry) => sum + (entry.snapshot?.sessions.length ?? 0), 0);
  const effectiveSelected =
    selectedMachine !== null && machines.some((entry) => entry.machineKey === selectedMachine)
      ? selectedMachine
      : null;

  const trimmedQuery = machineQuery.trim().toLowerCase();
  const railMachines = trimmedQuery
    ? machines.filter((entry) => entry.title.toLowerCase().includes(trimmedQuery))
    : machines;

  const visibleMachines = effectiveSelected
    ? machines.filter((entry) => entry.machineKey === effectiveSelected)
    : machines;

  const allCollapsed =
    visibleMachines.length > 0 && visibleMachines.every((entry) => collapsed.has(entry.machineKey));

  const toggleAllMachines = () => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (allCollapsed) {
        visibleMachines.forEach((entry) => next.delete(entry.machineKey));
      } else {
        visibleMachines.forEach((entry) => next.add(entry.machineKey));
      }
      return next;
    });
  };

  const selectMachine = (machineKey: string | null) => {
    setSelectedMachine(machineKey);
    if (machineKey !== null) {
      setCollapsed((prev) => {
        if (!prev.has(machineKey)) return prev;
        const next = new Set(prev);
        next.delete(machineKey);
        return next;
      });
    }
  };

  const focusedEntry = effectiveSelected
    ? machines.find((entry) => entry.machineKey === effectiveSelected) ?? null
    : null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onMouseDown={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="System resources"
        data-testid="system-resources-dialog"
        className={floatingSurface(
          "relative flex h-[82vh] max-h-[760px] w-full max-w-5xl animate-enter flex-col overflow-hidden rounded-xl motion-reduce:animate-none",
        )}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="flex shrink-0 items-center gap-2.5 border-b border-border bg-card px-4 py-3">
          <Activity aria-hidden="true" className="size-4 shrink-0 text-status-success" />
          <h1 className="text-sm font-semibold text-foreground">System resources</h1>
          <span className="rounded-full bg-muted px-2 py-0.5 text-[11px] font-medium tabular-nums text-muted-foreground">
            {totalMachines} {totalMachines === 1 ? "machine" : "machines"}
          </span>
          <span className="ml-auto flex items-center gap-1">
            <IconButton label="Refresh now" size="sm" onClick={refreshAll}>
              <RefreshCw className="size-3.5" />
            </IconButton>
            <IconButton label="Close system resources" size="sm" onClick={onClose}>
              <X className="size-3.5" />
            </IconButton>
          </span>
        </header>

        <div className="flex min-h-0 flex-1 flex-col md:flex-row">
          {totalMachines > 1 ? (
            <div className="flex shrink-0 items-center gap-2 border-b border-border bg-card px-3 py-2 md:hidden">
              <label htmlFor="resource-machine-select" className="shrink-0 text-[11px] font-medium text-muted-foreground">
                Machine
              </label>
              <select
                id="resource-machine-select"
                aria-label="Select machine"
                value={effectiveSelected ?? ""}
                onChange={(event) => selectMachine(event.target.value === "" ? null : event.target.value)}
                className="h-8 min-w-0 flex-1 rounded-md border border-input bg-background px-2 text-xs text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                <option value="">All machines ({totalMachines})</option>
                {machines.map((entry) => {
                  const statusText = entry.snapshot
                    ? `CPU ${formatPercent(entry.snapshot.cpuUtilization)}`
                    : entry.status === "loading"
                      ? "Reading…"
                      : entry.status === "update-required"
                        ? "Update needed"
                        : "Offline";
                  return (
                    <option key={entry.machineKey} value={entry.machineKey}>
                      {entry.title} — {statusText}
                    </option>
                  );
                })}
              </select>
            </div>
          ) : null}

          <aside
            data-testid="resource-machine-rail"
            className="hidden min-h-0 w-60 shrink-0 flex-col border-r border-border bg-card md:flex"
          >
            {totalMachines > 4 ? (
              <div className="shrink-0 border-b border-border/60 p-2">
                <div className="relative flex items-center">
                  <Search
                    aria-hidden="true"
                    className="pointer-events-none absolute left-2.5 size-3.5 text-muted-foreground"
                  />
                  <input
                    type="search"
                    role="searchbox"
                    aria-label="Filter machines"
                    value={machineQuery}
                    onChange={(event) => setMachineQuery(event.target.value)}
                    placeholder="Filter machines"
                    className="h-7 w-full rounded-md border border-input bg-background pl-8 pr-2 text-xs text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  />
                </div>
              </div>
            ) : null}

            <nav aria-label="Machines" className="min-h-0 flex-1 space-y-1 overflow-y-auto p-2 scrollbar-sleek">
              <button
                type="button"
                onClick={() => selectMachine(null)}
                aria-current={effectiveSelected === null ? "true" : undefined}
                data-testid="resource-machine-nav-all"
                className={cn(
                  "flex min-h-9 w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                  effectiveSelected === null
                    ? "bg-accent text-foreground"
                    : "text-muted-foreground hover:bg-surface-hover/50 hover:text-foreground",
                )}
              >
                <Layers aria-hidden="true" className="size-3.5 shrink-0" />
                <span className="min-w-0 flex-1 truncate text-xs font-medium">All machines</span>
                <span className="shrink-0 text-[10.5px] tabular-nums text-muted-foreground">
                  {totalSessions} {totalSessions === 1 ? "session" : "sessions"}
                </span>
              </button>

              <div className="my-1.5 border-t border-border/60" role="separator" />

              {railMachines.length === 0 ? (
                <p className="px-2.5 py-3 text-xs text-muted-foreground">No matching machines.</p>
              ) : (
                railMachines.map((entry) => (
                  <MachineRailItem
                    key={entry.machineKey}
                    entry={entry}
                    selected={effectiveSelected === entry.machineKey}
                    onSelect={() => selectMachine(entry.machineKey)}
                  />
                ))
              )}
            </nav>
          </aside>

          <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background">
            <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-border bg-card/50 px-4 py-2">
              <div className="flex min-w-0 items-center gap-2">
                <span className="truncate text-xs font-medium text-foreground">
                  {focusedEntry ? focusedEntry.title : "All machines"}
                </span>
                {focusedEntry ? (
                  <button
                    type="button"
                    onClick={() => selectMachine(null)}
                    className="min-h-6 rounded-sm px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-surface-hover/50 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    Show all
                  </button>
                ) : totalMachines > 1 ? (
                  <button
                    type="button"
                    onClick={toggleAllMachines}
                    className="min-h-6 rounded-sm px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-surface-hover/50 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    {allCollapsed ? "Expand all" : "Collapse all"}
                  </button>
                ) : null}
              </div>

              <div className="flex items-center gap-2" role="group" aria-label="Sort sessions">
                <span className="text-[11px] text-muted-foreground">Sort sessions</span>
                <div className="inline-flex items-center gap-0.5 rounded-md bg-muted/60 p-0.5">
                  <SortButton value="memory" sort={sort} onSort={setSort}>Memory</SortButton>
                  <SortButton value="cpu" sort={sort} onSort={setSort}>CPU</SortButton>
                  <SortButton value="name" sort={sort} onSort={setSort}>Name</SortButton>
                </div>
              </div>
            </div>

            <div
              data-testid="resource-inspector-body"
              className="selectable min-h-0 flex-1 space-y-4 overflow-y-auto p-4 scrollbar-sleek"
            >
              {error ? (
                <p
                  role="alert"
                  className="rounded-lg bg-destructive/10 px-3.5 py-2.5 text-xs text-destructive"
                >
                  {error}
                </p>
              ) : null}

              {visibleMachines.map((entry) => (
                <MachineSection
                  key={entry.machineKey}
                  entry={entry}
                  sort={sort}
                  collapsed={collapsed}
                  onToggle={toggle}
                />
              ))}
            </div>
          </div>
        </div>

        {killTarget ? (
          <div className="absolute inset-0 z-10 flex items-center justify-center bg-black/50 p-6">
            <div
              role="alertdialog"
              aria-modal="true"
              aria-label="Kill session"
              className={floatingSurface("w-full max-w-sm rounded-xl p-4")}
            >
              <h2 className="truncate text-sm font-semibold text-foreground">Kill {killTarget.sessionId}?</h2>
              <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
                Force-quits this terminal and every process in it. Unsaved work in the pane is lost. This can't be undone.
              </p>
              {killError ? <p role="alert" className="mt-2 text-xs text-destructive">{killError}</p> : null}
              <div className="mt-4 flex justify-end gap-2">
                <button
                  type="button"
                  disabled={killing}
                  onClick={() => setKillTarget(null)}
                  className="min-h-8 rounded-md border border-border px-3 py-1.5 text-xs font-medium text-foreground transition-colors hover:bg-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  disabled={killing}
                  onClick={() => void confirmKill()}
                  className="min-h-8 min-w-[96px] rounded-md bg-destructive px-3 py-1.5 text-xs font-medium text-destructive-foreground transition-opacity hover:opacity-90 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
                >
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
