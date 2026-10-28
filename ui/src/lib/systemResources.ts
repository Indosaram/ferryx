import { invoke } from "@tauri-apps/api/core";

import type { HostEndpoint } from "../state/remoteHostStore";

export type SessionResourceUsage = {
  sessionId: string;
  pid: number | null;
  worktreePath: string | null;
  cpuPercent: number | null;
  residentBytes: number | null;
  processCount: number | null;
};

export type HostResourceSnapshot = {
  sampledAtMs: number;
  platform: string;
  cpuCount: number;
  cpuUtilization: number | null;
  loadAverage1m: number | null;
  memoryTotalBytes: number | null;
  memoryUsedBytes: number | null;
  swapTotalBytes: number | null;
  swapUsedBytes: number | null;
  uptimeSeconds: number | null;
  diskTotalBytes: number | null;
  diskFreeBytes: number | null;
  processCount: number | null;
  sessions: SessionResourceUsage[];
  unavailable: string[];
};

export const RESOURCE_SAMPLE_INTERVAL_MS = 2000;

export async function fetchSystemResources(): Promise<HostResourceSnapshot> {
  return await invoke<HostResourceSnapshot>("cmd_system_resources");
}

/** Host capability that serves `GET /api/v1/resources`. */
export const RESOURCE_USAGE_CAPABILITY = "resourceUsageV1";

/** A paired machine's sample. A host that cannot answer is unknown, never idle. */
export type PairedResourceResult =
  | { status: "sampled"; snapshot: HostResourceSnapshot }
  | { status: "unreachable" }
  | { status: "update-required" };

type PairedInvoke = (command: "paired_host_operation", args: { request: unknown }) => Promise<unknown>;

/** Paired machines with a machine grant: the only hosts whose resources this desktop may read. */
export function resourceHosts(hosts: Readonly<Record<string, HostEndpoint>>): HostEndpoint[] {
  return Object.values(hosts)
    .filter((host) => host.authStatus === "paired" && host.grantScope === "machine" && host.generation)
    .sort((a, b) => (a.displayName ?? a.name).localeCompare(b.displayName ?? b.name));
}

function errorCode(error: unknown): string | null {
  if (!error || typeof error !== "object" || !("code" in error)) return null;
  const code = (error as { code?: unknown }).code;
  return typeof code === "string" ? code : null;
}

function isSnapshot(value: unknown): value is HostResourceSnapshot {
  if (!value || typeof value !== "object") return false;
  const row = value as Partial<HostResourceSnapshot>;
  return typeof row.platform === "string" && typeof row.cpuCount === "number"
    && Array.isArray(row.sessions) && Array.isArray(row.unavailable);
}

export async function fetchPairedResources(
  host: Pick<HostEndpoint, "hostId" | "generation">,
  call: PairedInvoke = (command, args) => invoke<unknown>(command, args),
): Promise<PairedResourceResult> {
  try {
    const raw = await call("paired_host_operation", {
      request: { hostId: host.hostId, generation: host.generation, operation: { kind: "resources" } },
    });
    const envelope = raw && typeof raw === "object" ? (raw as { hostId?: unknown; result?: unknown }) : {};
    const result = envelope.result && typeof envelope.result === "object"
      ? (envelope.result as { kind?: unknown; data?: unknown })
      : {};
    if (envelope.hostId !== host.hostId || result.kind !== "resources" || !isSnapshot(result.data)) {
      return { status: "unreachable" };
    }
    return { status: "sampled", snapshot: result.data };
  } catch (error) {
    return errorCode(error) === "PAIRED_HOST_CAPABILITY_UNAVAILABLE"
      ? { status: "update-required" }
      : { status: "unreachable" };
  }
}

export type ResourceSort = "name" | "cpu" | "memory";

export type WorktreeResourceGroup = {
  key: string;
  label: string;
  cpuPercent: number | null;
  residentBytes: number | null;
  sessions: SessionResourceUsage[];
};

function sumKnown(values: readonly (number | null)[]): number | null {
  const known = values.filter((value): value is number => value !== null && Number.isFinite(value));
  return known.length === 0 ? null : known.reduce((total, value) => total + value, 0);
}

function compareBy(sort: ResourceSort) {
  return (left: { label: string; cpuPercent: number | null; residentBytes: number | null },
    right: { label: string; cpuPercent: number | null; residentBytes: number | null }) => {
    if (sort === "name") return left.label.localeCompare(right.label);
    const primary = sort === "cpu"
      ? (right.cpuPercent ?? -1) - (left.cpuPercent ?? -1)
      : (right.residentBytes ?? -1) - (left.residentBytes ?? -1);
    return primary !== 0 ? primary : left.label.localeCompare(right.label);
  };
}

/** Sessions grouped by worktree with summed known costs; an unknown session adds nothing. */
export function groupSessionsByWorktree(
  sessions: readonly SessionResourceUsage[],
  sort: ResourceSort,
): WorktreeResourceGroup[] {
  const groups = new Map<string, SessionResourceUsage[]>();
  for (const session of sessions) {
    const key = session.worktreePath ?? "";
    groups.set(key, [...(groups.get(key) ?? []), session]);
  }
  const order = compareBy(sort);
  return [...groups.entries()]
    .map(([key, members]) => ({
      key,
      label: key === "" ? "Other sessions" : worktreeLabel(members[0]!),
      cpuPercent: sumKnown(members.map((member) => member.cpuPercent)),
      residentBytes: sumKnown(members.map((member) => member.residentBytes)),
      sessions: members
        .map((member) => ({ member, label: member.sessionId, cpuPercent: member.cpuPercent, residentBytes: member.residentBytes }))
        .sort(order)
        .map(({ member }) => member),
    }))
    .sort(order);
}

/** Oldest-first bounded history; a missing reading is kept as a gap rather than a zero. */
export const RESOURCE_HISTORY_LIMIT = 60;

export type ResourceHistory = { cpu: (number | null)[]; memory: (number | null)[] };

export function appendHistory(history: ResourceHistory | undefined, snapshot: HostResourceSnapshot): ResourceHistory {
  const push = (values: (number | null)[] | undefined, next: number | null) =>
    [...(values ?? []), next].slice(-RESOURCE_HISTORY_LIMIT);
  return {
    cpu: push(history?.cpu, snapshot.cpuUtilization),
    memory: push(history?.memory, usageRatio(snapshot.memoryUsedBytes, snapshot.memoryTotalBytes)),
  };
}

export function formatBytes(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  if (value < 1024) return `${Math.round(value)} B`;
  const units = ["KB", "MB", "GB", "TB", "PB"] as const;
  let next = value;
  let unit = -1;
  do {
    next /= 1024;
    unit += 1;
  } while (next >= 1024 && unit < units.length - 1);
  const digits = next >= 100 ? 0 : next >= 10 ? 1 : 2;
  return `${next.toFixed(digits)} ${units[unit]}`;
}

export function formatPercent(ratio: number | null | undefined): string {
  if (ratio === null || ratio === undefined || !Number.isFinite(ratio)) return "—";
  return `${Math.round(ratio * 100)}%`;
}

export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) return "—";
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}

export function usageRatio(used: number | null | undefined, total: number | null | undefined): number | null {
  if (used === null || used === undefined || total === null || total === undefined) return null;
  if (!Number.isFinite(used) || !Number.isFinite(total) || total <= 0) return null;
  return Math.min(1, Math.max(0, used / total));
}

export function worktreeLabel(session: SessionResourceUsage): string {
  const path = session.worktreePath;
  if (!path) return session.sessionId;
  const parts = path.split(/[\\/]/).filter((part) => part.length > 0);
  return parts[parts.length - 1] ?? session.sessionId;
}

export function sessionsByCost(sessions: readonly SessionResourceUsage[]): SessionResourceUsage[] {
  return [...sessions].sort((left, right) => {
    const memory = (right.residentBytes ?? 0) - (left.residentBytes ?? 0);
    if (memory !== 0) return memory;
    return (right.cpuPercent ?? 0) - (left.cpuPercent ?? 0);
  });
}

export function unavailableLabel(names: readonly string[]): string | null {
  if (names.length === 0) return null;
  return `This host could not report: ${names.join(", ")}`;
}
