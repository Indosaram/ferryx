import type { SshHost, TerminalSessionSsh } from "./types";
import { deleteSshHost, importSshConfig, listSshHosts, updateSshHost } from "./tauri";

// Single source of truth is the Rust ssh_hosts.json store (cmd_ssh_* IPC);
// this module is only a subscription cache over it. Do NOT mirror hosts into
// localStorage — the plan's original ferryx.sshHosts key was superseded.

type SshHostsListener = (hosts: SshHost[]) => void;

let cache: SshHost[] | null = null;
let inflight: Promise<SshHost[]> | null = null;
const listeners = new Set<SshHostsListener>();

function emit(hosts: SshHost[]): void {
  cache = hosts;
  for (const listener of listeners) listener(hosts);
}

/** Dedupe/tombstone key mirroring the Rust `SshHost::key`. */
export function sshHostKey(hostname: string, username?: string | null, port?: number | null): string {
  const effectivePort = port ?? 22;
  return username ? `${username}@${hostname}:${effectivePort}` : `${hostname}:${effectivePort}`;
}

export function makeSshHostId(): string {
  return typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `host-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

export function sshPaneTitle(ssh: Pick<TerminalSessionSsh, "title" | "remotePath">): string {
  if (!ssh.remotePath) return ssh.title;
  const segments = ssh.remotePath.split("/").filter(Boolean);
  const slug = segments.length > 0 ? segments[segments.length - 1] : ssh.remotePath;
  return `${ssh.title}:${slug}`;
}

export async function loadSshHosts(force = false): Promise<SshHost[]> {
  if (!force && cache) return cache;
  if (!inflight) {
    inflight = listSshHosts()
      .then((hosts) => {
        emit(hosts);
        return hosts;
      })
      .finally(() => {
        inflight = null;
      });
  }
  return inflight;
}

export function getCachedSshHosts(): SshHost[] | null {
  return cache;
}

export function subscribeSshHosts(listener: SshHostsListener): () => void {
  listeners.add(listener);
  if (cache) listener(cache);
  return () => {
    listeners.delete(listener);
  };
}

export async function importSshHostsFromConfig(configText?: string | null): Promise<SshHost[]> {
  const hosts = await importSshConfig(configText);
  emit(hosts);
  return hosts;
}

export async function upsertSshHost(host: SshHost): Promise<SshHost[]> {
  const hosts = await updateSshHost(host);
  emit(hosts);
  return hosts;
}

export async function removeSshHost(id: string): Promise<SshHost[]> {
  const hosts = await deleteSshHost(id);
  emit(hosts);
  return hosts;
}

export function __resetSshHostsCacheForTest(): void {
  cache = null;
  inflight = null;
  listeners.clear();
}
