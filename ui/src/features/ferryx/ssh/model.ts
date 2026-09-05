import type { RunTarget } from "../../../lib/scopedContracts";
export interface HostConfig {
  id: string; name: string; hostname: string; user: string; port: number;
  identityFile: string | null; proxyJump: string | null; knownHostsFile: string;
}
export function validateHost(host: HostConfig): string | null {
  if (!Number.isInteger(host.port) || host.port < 1 || host.port > 65535) return "Port must be an integer from 1 to 65535.";
  for (const value of [host.hostname, host.user, ...(host.proxyJump ? [host.proxyJump] : [])]) {
    if (!value || value.startsWith("-") || !/^[a-zA-Z0-9._:@\[\]-]+$/.test(value)) return "Use a hostname, user or jump host, not SSH options.";
  }
  if (!host.id || !host.name.trim() || !host.knownHostsFile.trim()) return "Name and verified known-hosts file are required.";
  return null;
}
export function resolveRunTarget(target: RunTarget | undefined, hosts: readonly HostConfig[]): RunTarget {
  if (target?.kind === "ssh" && !hosts.some(host => host.id === target.hostId)) throw new Error("HOST_UNAVAILABLE");
  return target ?? { kind: "local" };
}
