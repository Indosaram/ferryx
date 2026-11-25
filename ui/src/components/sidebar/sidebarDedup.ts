import type { RegisteredProject } from "../../lib/tauri";
import type { SshHost } from "../../lib/sshHosts";
import type { HostEndpoint, RemoteHostState } from "../../state/remoteHostStore";

export function normalizeMachineToken(raw: string): string {
  const trimmed = raw.trim().replace(/\.+$/, "").toLowerCase();
  if (trimmed.endsWith(".local")) return trimmed.slice(0, -".local".length);
  const dot = trimmed.indexOf(".");
  if (dot > 0) {
    const tail = trimmed.slice(dot + 1);
    if (tail === "ts.net" || tail.endsWith(".ts.net")) return trimmed.slice(0, dot);
  }
  return trimmed;
}

function normalizeRoot(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

function publishedOriginHost(origin: string): string | null {
  try {
    return new URL(origin).hostname.replace(/^\[|\]$/g, "").toLowerCase();
  } catch {
    return null;
  }
}

export function resolveSshHostToPairedHost(
  hosts: readonly SshHost[],
  remoteState: RemoteHostState,
): Map<string, string> {
  const map = new Map<string, string>();
  const pairedHosts: HostEndpoint[] = Object.values(remoteState.hosts ?? {});
  if (pairedHosts.length === 0 || hosts.length === 0) return map;

  for (const paired of pairedHosts) {
    if (paired.sshHostId) {
      map.set(paired.sshHostId, paired.hostId);
    }
    const originIps = new Set(
      (paired.publishedOrigins ?? [])
        .map((o) => publishedOriginHost(o.origin))
        .filter((h): h is string => Boolean(h)),
    );
    const wantedName = normalizeMachineToken(paired.displayName ?? paired.name);
    const matchedHostnames = new Set<string>(originIps);

    for (const ssh of hosts) {
      if (ssh.disabled) continue;
      const hostIp = (ssh.hostname ?? "").trim().toLowerCase();
      if (!hostIp) continue;
      const labelNorm = normalizeMachineToken(ssh.label ?? "");
      if (
        originIps.has(hostIp)
        || (wantedName && (labelNorm === wantedName || normalizeMachineToken(hostIp) === wantedName))
      ) {
        matchedHostnames.add(hostIp);
      }
    }

    for (const ssh of hosts) {
      if (ssh.disabled) continue;
      const hostIp = (ssh.hostname ?? "").trim().toLowerCase();
      if (hostIp && matchedHostnames.has(hostIp) && !map.has(ssh.id)) {
        map.set(ssh.id, paired.hostId);
      }
    }
  }
  return map;
}

export function deduplicateSidebarProjects(
  projects: RegisteredProject[],
  hosts: readonly SshHost[],
  remoteState: RemoteHostState,
): RegisteredProject[] {
  const sshToPaired = resolveSshHostToPairedHost(hosts, remoteState);
  if (sshToPaired.size === 0) return projects;

  const pairedRoots = new Set<string>();
  for (const project of projects) {
    if (project.target?.kind === "pairedDaemon") {
      pairedRoots.add(`${project.target.hostId}\0${normalizeRoot(project.repoRoot)}`);
    }
  }
  if (pairedRoots.size === 0) return projects;

  return projects.filter((project) => {
    if (project.target?.kind !== "ssh") return true;
    const pairedHostId = sshToPaired.get(project.target.hostId);
    if (!pairedHostId) return true;
    return !pairedRoots.has(`${pairedHostId}\0${normalizeRoot(project.repoRoot)}`);
  });
}
