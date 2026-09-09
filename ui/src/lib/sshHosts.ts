import { useCallback, useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

import { SSH_CONFIG_PATH_STORAGE_KEY } from "./storageKeys";

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

export interface SshRemoteEnvironment {
  platform: "posix" | "windows";
  executor: "sh" | "powershell" | "pwsh";
  version: string;
  home: string;
  temp: string;
  git: boolean;
}

export interface SshTargetSummary {
  host: SshHost;
  reachable: boolean;
  lastError?: string | null;
  checkedAt: number;
  environment?: SshRemoteEnvironment | null;
  diagnostic?: { code: string; message: string; details?: { stage?: string } } | null;
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

function normalizeHostInventory(hosts: SshHost[]): SshHost[] {
  const byId = new Map<string, SshHost>();
  for (const host of hosts) {
    // host.id is the routing identity used by project/worktree IPC. Keeping the
    // latest duplicate prevents a visible option from resolving to an older endpoint.
    byId.set(host.id, host);
  }
  return Array.from(byId.values());
}

function notifyListeners(hosts: SshHost[]): SshHost[] {
  const normalized = normalizeHostInventory(hosts);
  cachedHosts = normalized;
  inventoryEpoch += 1;
  for (const listener of listeners) {
    listener(normalized);
  }
  return normalized;
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
        return cachedHosts ?? normalizeHostInventory(hosts);
      }
      return notifyListeners(hosts);
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
  return notifyListeners(hosts);
}

export async function readSystemSshConfig(
  configPath: string | null = null,
): Promise<SystemSshConfig> {
  if (!isTauri()) {
    return { path: configPath ?? "~/.ssh/config", exists: false, rawText: "", hosts: [] };
  }
  return invoke<SystemSshConfig>("cmd_ssh_read_system_config", { configPath });
}

type ConfigPathStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function defaultConfigPathStorage(): ConfigPathStorage | null {
  return typeof window !== "undefined" && window.localStorage ? window.localStorage : null;
}

export function getSshConfigPathOverride(
  storage: ConfigPathStorage | null = defaultConfigPathStorage(),
): string | null {
  const value = storage?.getItem(SSH_CONFIG_PATH_STORAGE_KEY) ?? null;
  return value && value.trim() !== "" ? value : null;
}

export function setSshConfigPathOverride(
  configPath: string | null,
  storage: ConfigPathStorage | null = defaultConfigPathStorage(),
): void {
  if (!storage) return;
  if (configPath && configPath.trim() !== "") {
    storage.setItem(SSH_CONFIG_PATH_STORAGE_KEY, configPath.trim());
  } else {
    storage.removeItem(SSH_CONFIG_PATH_STORAGE_KEY);
  }
}

export async function updateSshHost(host: SshHost): Promise<SshHost[]> {
  if (!isTauri()) {
    throw new Error("SSH host changes are available only in the Ferryx desktop runtime");
  }
  const cleaned = cleanHostForIpc(host);
  const hosts = await invoke<SshHost[]>("cmd_ssh_update_host", { host: cleaned });
  return notifyListeners(hosts);
}

export async function deleteSshHost(id: string): Promise<SshHost[]> {
  if (!isTauri()) {
    throw new Error("SSH host changes are available only in the Ferryx desktop runtime");
  }
  const hosts = await invoke<SshHost[]>("cmd_ssh_delete_host", { id });
  return notifyListeners(hosts);
}

export async function testSshConnection(host: SshHost): Promise<SshTargetSummary> {
  const cleaned = cleanHostForIpc(host);
  return invoke<SshTargetSummary>("cmd_ssh_test_connection", { host: cleaned });
}

export async function prepareSshIntegration(host: SshHost): Promise<void> {
  return invoke<void>("cmd_ssh_prepare_integration", { host: cleanHostForIpc(host) });
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
