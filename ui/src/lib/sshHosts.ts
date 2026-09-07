import { useCallback, useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

export type SshHostSource = "config" | "manual";
export type SshAuthMethod = "agent" | "key";

export interface SshHost {
  id: string;
  label: string;
  hostname: string;
  username?: string | null;
  port?: number | null;
  identityFile?: string | null;
  jumpHost?: string | null;
  source: SshHostSource;
  authMethod: SshAuthMethod;
  disabled?: boolean | null;
}

export interface SshTargetSummary {
  host: SshHost;
  reachable: boolean;
  lastError?: string | null;
  checkedAt: number;
}

export interface SystemSshConfig {
  path: string;
  exists: boolean;
  rawText: string;
  hosts: SshHost[];
}

export function formatSshTarget(host: { username?: string | null; hostname: string }): string {
  if (host.username && host.username.trim() !== "") {
    return `${host.username.trim()}@${host.hostname.trim()}`;
  }
  return host.hostname.trim();
}

export function formatSshKey(host: { username?: string | null; hostname: string; port?: number | null }): string {
  const port = host.port ?? 22;
  return `${formatSshTarget(host)}:${port}`;
}

// In-memory shared state for active consumers (authoritative source of truth remains the Tauri backend)
let cachedHosts: SshHost[] | null = null;
let inflightFetch: Promise<SshHost[]> | null = null;
// Bumped on every inventory write (cache update or cache reset). In-flight reads capture
// the epoch at request time and discard their result if a write landed in the meantime,
// so an obsolete read can never resurrect stale inventory over a newer mutation.
let inventoryEpoch = 0;
const listeners = new Set<(hosts: SshHost[]) => void>();

function notifyListeners(hosts: SshHost[]): void {
  cachedHosts = hosts;
  inventoryEpoch += 1;
  for (const listener of listeners) {
    listener(hosts);
  }
}

export function subscribeSshHosts(listener: (hosts: SshHost[]) => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getCachedSshHosts(): SshHost[] | null {
  return cachedHosts;
}

export function resetSshHostsCache(): void {
  cachedHosts = null;
  inflightFetch = null;
  inventoryEpoch += 1;
  listeners.clear();
}

export function extractIpcErrorMessage(err: unknown, fallback: string): string {
  if (err instanceof Error && err.message.trim() !== "") {
    return err.message;
  }
  if (typeof err === "string" && err.trim() !== "") {
    return err;
  }
  // Tauri rejects with serialized backend error objects ({ code, message, details });
  // extract the human-readable message instead of String(err) => "[object Object]".
  if (err && typeof err === "object") {
    const message = (err as { message?: unknown }).message;
    if (typeof message === "string" && message.trim() !== "") {
      return message;
    }
  }
  return fallback;
}

function cleanHostForIpc(host: SshHost): SshHost {
  const cleaned: SshHost = {
    id: host.id,
    label: host.label.trim(),
    hostname: host.hostname.trim(),
    source: host.source,
    authMethod: host.authMethod,
  };
  if (host.username && host.username.trim() !== "") {
    cleaned.username = host.username.trim();
  }
  if (host.port !== undefined && host.port !== null) {
    if (!Number.isInteger(host.port) || host.port < 1 || host.port > 65535) {
      throw new Error("Port must be an integer between 1 and 65535.");
    }
    cleaned.port = host.port;
  }
  if (host.identityFile && host.identityFile.trim() !== "") {
    cleaned.identityFile = host.identityFile.trim();
  }
  if (host.jumpHost && host.jumpHost.trim() !== "") {
    cleaned.jumpHost = host.jumpHost.trim();
  }
  if (typeof host.disabled === "boolean") {
    cleaned.disabled = host.disabled;
  }
  return cleaned;
}

export async function listSshHosts(): Promise<SshHost[]> {
  if (inflightFetch) {
    return inflightFetch;
  }
  if (!isTauri()) {
    const empty: SshHost[] = [];
    notifyListeners(empty);
    return empty;
  }

  const requestEpoch = inventoryEpoch;
  inflightFetch = (async () => {
    try {
      const hosts = await invoke<SshHost[]>("cmd_ssh_list_hosts");
      if (requestEpoch !== inventoryEpoch) {
        // A mutation completed while this read was in flight; the read is obsolete.
        // Discard it so stale inventory cannot overwrite the newer mutation result.
        return cachedHosts ?? hosts;
      }
      notifyListeners(hosts);
      return hosts;
    } finally {
      inflightFetch = null;
    }
  })();

  return inflightFetch;
}

export async function importSshConfig(configText: string): Promise<SshHost[]> {
  if (!isTauri()) {
    throw new Error("SSH host changes are available only in the Ferryx desktop runtime");
  }
  const hosts = await invoke<SshHost[]>("cmd_ssh_import_config", { configText });
  notifyListeners(hosts);
  return hosts;
}

export async function readSystemSshConfig(): Promise<SystemSshConfig> {
  if (!isTauri()) {
    return { path: "~/.ssh/config", exists: false, rawText: "", hosts: [] };
  }
  return invoke<SystemSshConfig>("cmd_ssh_read_system_config");
}

export async function updateSshHost(host: SshHost): Promise<SshHost[]> {
  if (!isTauri()) {
    throw new Error("SSH host changes are available only in the Ferryx desktop runtime");
  }
  const cleaned = cleanHostForIpc(host);
  const hosts = await invoke<SshHost[]>("cmd_ssh_update_host", { host: cleaned });
  notifyListeners(hosts);
  return hosts;
}

export async function deleteSshHost(id: string): Promise<SshHost[]> {
  if (!isTauri()) {
    throw new Error("SSH host changes are available only in the Ferryx desktop runtime");
  }
  const hosts = await invoke<SshHost[]>("cmd_ssh_delete_host", { id });
  notifyListeners(hosts);
  return hosts;
}

export async function testSshConnection(host: SshHost): Promise<SshTargetSummary> {
  const cleaned = cleanHostForIpc(host);
  return invoke<SshTargetSummary>("cmd_ssh_test_connection", { host: cleaned });
}

export interface UseSshHostsResult {
  hosts: SshHost[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<SshHost[]>;
}

export function useSshHosts(): UseSshHostsResult {
  const [hosts, setHosts] = useState<SshHost[]>(() => cachedHosts ?? []);
  const [loading, setLoading] = useState<boolean>(() => cachedHosts === null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async (): Promise<SshHost[]> => {
    setLoading(true);
    setError(null);
    try {
      const result = await listSshHosts();
      return result;
    } catch (err) {
      setError(extractIpcErrorMessage(err, "Failed to load SSH machines."));
      throw err;
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    const unsubscribe = subscribeSshHosts((newHosts) => {
      setHosts(newHosts);
      setLoading(false);
      setError(null);
    });

    if (cachedHosts === null) {
      void refresh().catch(() => {
        // error already recorded in setError
      });
    } else {
      setHosts(cachedHosts);
    }

    return unsubscribe;
  }, [refresh]);

  return { hosts, loading, error, refresh };
}
