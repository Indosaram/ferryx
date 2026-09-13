import { invoke, isTauri } from "@tauri-apps/api/core";
import { remoteHostKey, remoteHostStore, REMOTE_HOST_STORAGE_KEY, type HostEndpoint, type RemoteHostStore } from "../state/remoteHostStore";

export interface HostView {
  hostId: string;
  relayOrigin: string;
  machineId: string;
  displayLabel: string;
  grantScope: "mirror" | "machine";
  generation: string;
  authStatus: "paired" | "needsMachineGrant" | "revoked" | "unknown";
  online: boolean;
}
export interface MigrationReceipt { hostId: string; generation: string }
export interface PairHostRequest { relayOrigin: string; pin: string; displayLabel: string }
export interface LegacyCredentialRequest { relayOrigin: string; machineId: string; displayLabel: string; deviceToken: string }
export interface PairedHostCommands {
  list(): Promise<HostView[]>;
  capabilities(): Promise<{ pairedHostInventoryV1: boolean; pairedDaemonProxyV1: boolean }>;
  pair(request: PairHostRequest): Promise<HostView>;
  forget(request: MigrationReceipt): Promise<void>;
  migrate(request: LegacyCredentialRequest): Promise<MigrationReceipt>;
  read(request: MigrationReceipt): Promise<HostView>;
}
// No raw native exception crosses this boundary: it may contain request credentials.
async function command<T>(name: string, request?: object): Promise<T> {
  try { return await invoke<T>(name, request ? { request } : undefined); }
  catch { throw new Error("PAIRED_HOST_UNAVAILABLE"); }
}
export const nativePairedHostCommands: PairedHostCommands = {
  list: () => command("paired_host_list"),
  capabilities: () => command("paired_host_capabilities"),
  pair: request => command("paired_host_pair", request),
  forget: request => command("paired_host_forget", request),
  migrate: request => command("paired_host_migrate_legacy", request),
  read: request => command("paired_host_read", request),
};
function validGeneration(value: unknown): value is string {
  return typeof value === "string" && /^(0|[1-9][0-9]*)$/.test(value) && BigInt(value) <= 18446744073709551615n;
}
function origin(value: string): string {
  const url = new URL(value);
  if (url.protocol !== "https:" || url.username || url.password || url.pathname !== "/" || url.search || url.hash) throw new Error("INVALID_RELAY_ORIGIN");
  return url.origin;
}
function endpoint(view: HostView): HostEndpoint {
  if (!view || typeof view.machineId !== "string" || !view.machineId || typeof view.displayLabel !== "string"
    || !validGeneration(view.generation) || view.hostId !== remoteHostKey(origin(view.relayOrigin), view.machineId)
    || !["mirror", "machine"].includes(view.grantScope)
    || !["paired", "needsMachineGrant", "revoked", "unknown"].includes(view.authStatus) || typeof view.online !== "boolean") throw new Error("INVALID_HOST_VIEW");
  return { hostId: view.hostId, relayOrigin: view.relayOrigin, machineId: view.machineId,
    name: view.displayLabel, displayName: view.displayLabel, address: view.relayOrigin, transport: "relay",
    generation: view.generation, grantScope: view.grantScope, authStatus: view.authStatus, online: view.online };
}

/** Native owns credentials and transport cancellation; this adapter fences renderer callbacks. */
export function createPairedHostInventory(store: RemoteHostStore, commands = nativePairedHostCommands, storage?: Storage) {
  let revision = 0;
  let refreshRequest = 0;
  // Rollout is opt-in and separate from advertised support; the effective flag
  // remains the store's existing machineFeaturesEnabled, never a second gate.
  let projectsEnabled = false;
  try { projectsEnabled = storage?.getItem("pairedDaemonProjectsV1") === "true"; }
  catch { projectsEnabled = false; }
  let proxyAvailable = false;
  function setProjectsEnabled(enabled: boolean) {
    storage?.setItem("pairedDaemonProjectsV1", String(enabled));
    projectsEnabled = enabled;
    store.setState(s => ({ ...s, machineFeaturesEnabled: enabled && proxyAvailable && s.nativeStatus === "ready" }));
  }
  function getProjectsEnabled() { return projectsEnabled; }
  function hasProxyCapability() { return proxyAvailable; }
  function unavailable() {
    proxyAvailable = false;
    store.setState(s => ({ ...s, nativeStatus: "unavailable", machineFeaturesEnabled: false,
      hosts: Object.fromEntries(Object.entries(s.hosts).map(([id, host]) => [id, { ...host, online: false }])) }));
  }
  function capture(hostId: string) {
    const generation = store.getState().hosts[hostId]?.generation;
    const requestRevision = revision;
    return () => generation !== undefined && requestRevision === revision && store.getState().hosts[hostId]?.generation === generation;
  }
  async function refresh() {
    const request = ++refreshRequest;
    const started = revision;
    try {
      const [views, capability] = await Promise.all([commands.list(), commands.capabilities()]);
      if (capability.pairedHostInventoryV1 !== true) throw new Error("INVENTORY_UNAVAILABLE");
      const hosts = views.map(endpoint);
      if (request !== refreshRequest || started !== revision) return;
      proxyAvailable = capability.pairedDaemonProxyV1 === true;
      store.setHosts(hosts);
      store.setState(s => ({ ...s, nativeStatus: "ready", machineFeaturesEnabled: projectsEnabled && proxyAvailable }));
    } catch { if (request === refreshRequest && started === revision) unavailable(); }
  }
  async function pair(request: PairHostRequest): Promise<boolean> {
    const started = ++revision;
    try {
      const relayOrigin = origin(request.relayOrigin);
      const host = endpoint(await commands.pair({ ...request, relayOrigin }));
      if (started !== revision || host.relayOrigin !== relayOrigin) return false;
      store.upsertHost(host);
      return true;
    } catch { if (started === revision) unavailable(); return false; }
  }
  async function forget(hostId: string): Promise<boolean> {
    const host = store.getState().hosts[hostId];
    if (!host?.generation) return false;
    const started = ++revision;
    try {
      await commands.forget({ hostId, generation: host.generation });
      if (started !== revision || store.getState().hosts[hostId]?.generation !== host.generation) return false;
      store.removeHost(hostId);
      return true;
    } catch { if (started === revision) unavailable(); return false; }
  }
  async function migrateLegacy(): Promise<void> {
    if (!isTauri() || !storage) return;
    const started = ++revision;
    store.setState(s => ({ ...s, migrationStatus: "pending" }));
    try {
      const original = storage.getItem(REMOTE_HOST_STORAGE_KEY);
      if (!original) { store.setState(s => ({ ...s, migrationStatus: "complete" })); return; }
      const parsed = JSON.parse(original);
      const entries: [string | null, unknown][] = parsed.hosts ? Object.entries(parsed.hosts) : [[null, parsed]];
      let expected = original;
      let pending = false;
      for (const [key, value] of entries) {
        if (!value || typeof value !== "object") { pending = true; continue; }
        const row = value as Record<string, unknown>;
        if (typeof row.machineId !== "string" || !row.machineId || typeof row.relayOrigin !== "string") { pending = true; continue; }
        const relayOrigin = origin(row.relayOrigin);
        const hostId = remoteHostKey(relayOrigin, row.machineId);
        // Only canonical per-host keys, never an origin-wide or guessed alias key.
        const tokenKey = `ferryx_remote_token_${hostId}`;
        const keyedToken = storage.getItem(tokenKey);
        const token = typeof row.deviceToken === "string" && row.deviceToken ? row.deviceToken : keyedToken;
        if (!token) continue;
        // Conflicting copies cannot prove which credential is current. Leave both intact.
        if (keyedToken && keyedToken !== token) { pending = true; continue; }
        const receipt = await commands.migrate({ relayOrigin, machineId: row.machineId,
          displayLabel: typeof row.displayName === "string" ? row.displayName : typeof row.name === "string" ? row.name : row.machineId,
          deviceToken: token });
        if (started !== revision || receipt.hostId !== hostId || !validGeneration(receipt.generation)) throw new Error("MIGRATION_PENDING");
        const verified = endpoint(await commands.read(receipt));
        if (started !== revision || verified.hostId !== receipt.hostId || verified.generation !== receipt.generation) throw new Error("MIGRATION_PENDING");
        // No await between comparison and cleanup. Concurrent edits are left intact for retry.
        if (storage.getItem(REMOTE_HOST_STORAGE_KEY) !== expected || storage.getItem(tokenKey) !== keyedToken) throw new Error("MIGRATION_PENDING");
        const current = JSON.parse(expected);
        const target = key === null ? current : current.hosts[key];
        delete target.deviceToken;
        const cleaned = JSON.stringify(current);
        storage.setItem(REMOTE_HOST_STORAGE_KEY, cleaned);
        expected = cleaned;
        if (keyedToken === token) storage.removeItem(tokenKey);
        store.upsertHost(verified);
      }
      if (started === revision) store.setState(s => ({ ...s, migrationStatus: pending ? "pending" : "complete" }));
    } catch { store.setState(s => ({ ...s, migrationStatus: "pending" })); }
  }
  return { refresh, pair, forget, capture, migrateLegacy, setProjectsEnabled, getProjectsEnabled, hasProxyCapability };
}
export const pairedHostInventory = createPairedHostInventory(remoteHostStore, nativePairedHostCommands,
  typeof localStorage === "undefined" ? undefined : localStorage);
export async function bootstrapPairedHostInventory() {
  if (!isTauri()) return;
  await pairedHostInventory.migrateLegacy();
  await pairedHostInventory.refresh();
}
