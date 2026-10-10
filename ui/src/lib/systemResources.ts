import { invoke } from "@tauri-apps/api/core";

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
