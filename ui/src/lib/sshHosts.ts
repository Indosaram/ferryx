import { useCallback, useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { SSH_CONFIG_PATH_STORAGE_KEY } from "./storageKeys";

export type SshHostSource = "config" | "manual" | "tailscale";
export type SshAuthMethod = "agent" | "key" | "password";

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

export type SshHelperProbeState = "installed" | "missing" | "unknown";

export interface SshTargetSummary {
  host: SshHost;
  reachable: boolean;
  lastError?: string | null;
  checkedAt: number;
  environment?: SshRemoteEnvironment | null;
  helper?: SshHelperProbeState | null;
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

export function mergeTailscaleSshHosts(hosts: SshHost[]): SshHost[] {
  return notifyListeners(hosts);
}

export function resetSshHostsCache(options?: { clearListeners?: boolean }): void {
  cachedHosts = null;
  inflightFetch = null;
  inventoryEpoch += 1;
  if (options?.clearListeners) {
    listeners.clear();
  }
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

/**
 * `configPath` names the file `configText` was read from. The backend lets hosts connect by their
 * alias only when that is the ssh client's default config; pasted text omits it.
 */
export async function importSshConfig(configText: string, configPath?: string): Promise<SshHost[]> {
  if (!isTauri()) {
    throw new Error("SSH host changes are available only in the Ferryx desktop runtime");
  }
  const hosts = await invoke<SshHost[]>(
    "cmd_ssh_import_config",
    configPath ? { configText, configPath } : { configText },
  );
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

export async function setSshPassword(host: SshHost, password: string): Promise<void> {
  return invoke<void>("cmd_ssh_set_password", { host: cleanHostForIpc(host), password });
}

export async function clearSshPassword(host: SshHost): Promise<void> {
  return invoke<void>("cmd_ssh_clear_password", { host: cleanHostForIpc(host) });
}

export type SshAuthPromptKind = "password" | "passphrase" | "code" | "other";

/** An interactive question OpenSSH asked during a connection (password, key passphrase, 2FA code, ...). */
export interface SshAuthPrompt {
  id: string;
  /** The user@host (or connection target) the prompt is for. */
  host: string;
  /** OpenSSH's question text, verbatim. */
  prompt: string;
  kind: SshAuthPromptKind;
  expiresInMs: number;
}

export const SSH_AUTH_PROMPT_EVENT = "ssh_auth_prompt";

/** Answers a pending SSH auth prompt; `null` cancels it. Rejects when the prompt already expired. */
export async function answerSshAuthPrompt(id: string, answer: string | null): Promise<void> {
  return invoke<void>("cmd_ssh_answer_auth_prompt", { id, answer });
}

/**
 * Subscribes to SSH auth prompts and tells the backend a prompt UI is listening
 * (it fails prompts immediately when nobody listens). Returns the unsubscribe function.
 */
export async function listenSshAuthPrompts(onPrompt: (prompt: SshAuthPrompt) => void): Promise<() => void> {
  if (!isTauri()) {
    return () => {};
  }
  const unlisten = await listen<SshAuthPrompt>(SSH_AUTH_PROMPT_EVENT, event => onPrompt(event.payload));
  try {
    await invoke<void>("cmd_ssh_auth_prompt_listeners", { active: true });
  } catch (err) {
    unlisten();
    throw err;
  }
  return () => {
    unlisten();
    void (async () => {
      try {
        await invoke<void>("cmd_ssh_auth_prompt_listeners", { active: false });
      } catch (err) {
        console.warn("Failed to unregister the SSH auth prompt listener", err);
      }
    })();
  };
}

export async function testSshConnection(host: SshHost): Promise<SshTargetSummary> {
  const cleaned = cleanHostForIpc(host);
  return invoke<SshTargetSummary>("cmd_ssh_test_connection", { host: cleaned });
}

export interface SshScannedHostKey {
  keyType: string;
  fingerprintSha256: string;
  line: string;
}

/** Reads the host keys the server presents, without trusting any of them. */
export async function scanSshHostKey(host: SshHost): Promise<{ keys: SshScannedHostKey[] }> {
  return invoke<{ keys: SshScannedHostKey[] }>("cmd_ssh_scan_host_key", { host: cleanHostForIpc(host) });
}

/** Pins the host key; the backend refuses when the server no longer presents this fingerprint. */
export async function trustSshHostKey(host: SshHost, fingerprintSha256: string): Promise<void> {
  return invoke<void>("cmd_ssh_trust_host_key", { host: cleanHostForIpc(host), fingerprintSha256 });
}

export async function prepareSshIntegration(host: SshHost): Promise<void> {
  return invoke<void>("cmd_ssh_prepare_integration", { host: cleanHostForIpc(host) });
}

export interface RemoteRemovalReport {
  removed: string[];
  keptModified: string[];
  absent: string[];
}

export async function removeSshIntegration(hostId: string): Promise<RemoteRemovalReport> {
  return invoke<RemoteRemovalReport>("cmd_ssh_remove_integration", { hostId });
}

export interface SshNodeInstallReport {
  cliPath: string;
  version: string;
  enrolled?: { accountId: string; machineRecordId: string };
  service: { supervised: boolean; backend: string; reason?: string };
}

/** Installs the Ferryx CLI on a stored SSH host; the key travels over ssh stdin, never argv. */
export async function installFerryxNode(
  hostId: string,
  accountOrigin: string,
  installKey: string,
): Promise<SshNodeInstallReport> {
  return invoke<SshNodeInstallReport>("cmd_ssh_install_ferryx_node", {
    hostId,
    accountOrigin,
    installKey,
  });
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
