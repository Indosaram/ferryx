import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { remoteHostKey, remoteHostStore, REMOTE_HOST_STORAGE_KEY, type HostEndpoint, type RemoteHostStore } from "../state/remoteHostStore";

// The built-in relay is the product default: pairing must work with the PIN
// alone. Existing legacy identities remain on their original relay.
export const DEFAULT_RELAY_ORIGIN = "https://relay.checka.cc";
export const DEFAULT_MACHINE_LABEL = "Machine";

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

export interface PairedHostError {
  code: string;
  message: string;
  details?: unknown;
  retryable: boolean;
}

export type PairResult =
  | { ok: true; host: HostEndpoint }
  | { ok: false; error: PairedHostError };

export interface InventoryChangeEvent {
  type: "update" | "forget" | "reset" | "reconnect" | "pair" | "migrate" | "revoke";
  host?: HostView;
  hostId?: string;
  generation?: string;
}

export type InventoryUnlisten = () => void;

export function extractPairedHostError(err: unknown): PairedHostError {
  if (err && typeof err === "object") {
    const obj = err as Record<string, unknown>;
    if (typeof obj.code === "string") {
      const code = obj.code;
      const rawMessage = typeof obj.message === "string" ? obj.message : code;
      // Sanitize: ensure credentials (PINs, tokens) are never leaked in message
      const message = rawMessage
        .replace(/[0-9a-fA-F]{32,}/g, "[REDACTED]")
        .replace(/\b[0-9]{6}\b/g, "[REDACTED]");
      const details = obj.details;
      const retryable = typeof obj.retryable === "boolean"
        ? obj.retryable
        : isRetryableCode(code);
      return { code, message, details, retryable };
    }
  }
  if (typeof err === "string" && /^[A-Z0-9_]+$/.test(err)) {
    return {
      code: err,
      message: err,
      retryable: isRetryableCode(err),
    };
  }
  return {
    code: "PAIRED_HOST_UNAVAILABLE",
    message: "PAIRED_HOST_UNAVAILABLE",
    retryable: true,
  };
}

function isRetryableCode(code: string): boolean {
  switch (code) {
    case "PIN_EXPIRED":
    case "EXPIRED_PIN":
    case "INVALID_PIN":
    case "WRONG_RELAY":
    case "INVALID_RELAY_ORIGIN":
    case "MACHINE_GRANT_REQUIRED":
    case "UNAUTHORIZED":
      return false;
    case "TIMEOUT":
    case "HOST_UNAVAILABLE":
    case "PAIRED_HOST_UNAVAILABLE":
    case "DAEMON_UNAVAILABLE":
    case "PAIRED_HOST_STALE_GENERATION":
    default:
      return true;
  }
}

export interface PairedHostCommands {
  list(): Promise<HostView[]>;
  capabilities(): Promise<{ pairedHostInventoryV1: boolean; pairedDaemonProxyV1: boolean }>;
  pair(request: PairHostRequest): Promise<HostView>;
  forget(request: MigrationReceipt): Promise<void>;
  migrate(request: LegacyCredentialRequest): Promise<MigrationReceipt>;
  read(request: MigrationReceipt): Promise<HostView>;
}
// No raw native exception crosses this boundary: it may contain request credentials.
// Structured error fields {code, message, details, retryable} are sanitized and preserved.
async function command<T>(name: string, request?: object): Promise<T> {
  try { return await invoke<T>(name, request ? { request } : undefined); }
  catch (err) {
    const structured = extractPairedHostError(err);
    const error = new Error(structured.message) as Error & PairedHostError;
    error.code = structured.code;
    error.details = structured.details;
    error.retryable = structured.retryable;
    throw error;
  }
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

export function normalizeRelayOrigin(value: string): string {
  if (typeof value !== "string") throw new Error("INVALID_RELAY_ORIGIN");
  const trimmed = value.trim();
  if (!trimmed) throw new Error("INVALID_RELAY_ORIGIN");
  try {
    const url = new URL(trimmed);
    const isLoopback = url.hostname === "localhost" || url.hostname === "127.0.0.1" || url.hostname === "::1" || url.hostname === "[::1]";
    const validProtocol = url.protocol === "https:" || (url.protocol === "http:" && isLoopback);
    if (!validProtocol || url.username || url.password || (url.pathname !== "/" && url.pathname !== "") || url.search || url.hash) {
      throw new Error("INVALID_RELAY_ORIGIN");
    }
    return url.origin;
  } catch (err) {
    if (err instanceof Error && err.message === "INVALID_RELAY_ORIGIN") throw err;
    throw new Error("INVALID_RELAY_ORIGIN");
  }
}

export interface ParsedPairingInvite {
  pin?: string;
  relayOrigin?: string;
}

export function parsePairingInvite(input: string): ParsedPairingInvite | null {
  if (!input || typeof input !== "string") return null;
  const trimmed = input.trim();
  if (!trimmed.includes("://") && !trimmed.startsWith("#pair=")) return null;

  try {
    if (trimmed.startsWith("#pair=")) {
      const hashParams = new URLSearchParams(trimmed.slice(1));
      const pin = hashParams.get("pair") || undefined;
      const relayParam = hashParams.get("relay");
      let relayOrigin: string | undefined;
      if (relayParam) {
        try { relayOrigin = normalizeRelayOrigin(relayParam); } catch {}
      }
      return { pin, relayOrigin };
    }

    const url = new URL(trimmed);
    const isLoopback = url.hostname === "localhost" || url.hostname === "127.0.0.1" || url.hostname === "::1" || url.hostname === "[::1]";
    if (url.protocol !== "https:" && !(url.protocol === "http:" && isLoopback)) {
      return null;
    }

    let pin: string | undefined;
    let relayOrigin: string | undefined;

    if (url.hash) {
      const hash = url.hash.replace(/^#/, "");
      const hashParams = new URLSearchParams(hash);
      const p = hashParams.get("pair");
      if (p) pin = p;
      const r = hashParams.get("relay");
      if (r) {
        try { relayOrigin = normalizeRelayOrigin(r); } catch {}
      }
    }

    if (!pin && url.search) {
      const p = url.searchParams.get("pair");
      if (p) pin = p;
      const r = url.searchParams.get("relay");
      if (r && !relayOrigin) {
        try { relayOrigin = normalizeRelayOrigin(r); } catch {}
      }
    }

    if (!relayOrigin) {
      try {
        relayOrigin = normalizeRelayOrigin(url.origin);
      } catch {}
    }

    if (!pin && !relayOrigin) return null;
    return { pin, relayOrigin };
  } catch {
    return null;
  }
}

function origin(value: string): string {
  return normalizeRelayOrigin(value);
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
  let appWideUnlisten: InventoryUnlisten | null = null;
  const fencedGenerations = new Map<string, bigint>();
  const tombstones = new Map<string, bigint>();

  // Machine features follow native inventory readiness alone; there is no
  // user-facing rollout gate. A local inventory failure fails closed below.
  let projectsEnabled = true;
  let proxyAvailable = true;
  function setProjectsEnabled(enabled: boolean) {
    projectsEnabled = enabled;
    store.setState(s => ({ ...s, machineFeaturesEnabled: enabled && s.nativeStatus === "ready" }));
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

  function updateHostWithFence(view: HostView): boolean {
    const incomingGen = BigInt(view.generation);
    const hostId = view.hostId;
    const currentFenced = fencedGenerations.get(hostId);
    if (currentFenced !== undefined && incomingGen < currentFenced) {
      return false; // older generation rejected
    }
    const tombstoneGen = tombstones.get(hostId);
    if (tombstoneGen !== undefined && incomingGen <= tombstoneGen) {
      return false; // forgotten at equal or newer generation
    }
    fencedGenerations.set(hostId, incomingGen);
    tombstones.delete(hostId);
    const host = endpoint(view);
    store.upsertHost(host);
    return true;
  }

  function forgetHostWithFence(hostId: string, generationStr: string): boolean {
    const gen = BigInt(generationStr);
    const currentFenced = fencedGenerations.get(hostId);
    if (currentFenced !== undefined && gen < currentFenced) {
      return false;
    }
    fencedGenerations.set(hostId, gen);
    tombstones.set(hostId, gen);
    store.removeHost(hostId);
    return true;
  }

  function handleNativeEvent(event: InventoryChangeEvent) {
    if (!event || typeof event !== "object") return;
    ++revision; // Invalidate any in-flight refresh!
    if (event.type === "update" || event.type === "pair") {
      if (event.host) {
        updateHostWithFence(event.host);
      }
    } else if (event.type === "forget") {
      if (event.hostId && event.generation) {
        forgetHostWithFence(event.hostId, event.generation);
      }
    } else if (event.type === "reconnect" || event.type === "reset" || event.type === "migrate") {
      if (event.host) {
        updateHostWithFence(event.host);
      } else {
        void refresh();
      }
    } else if (event.type === "revoke") {
      if (event.hostId) {
        const existing = store.getState().hosts[event.hostId];
        if (existing) {
          if (event.generation && existing.generation) {
            try {
              if (BigInt(event.generation) < BigInt(existing.generation)) {
                // R3-N4: stale revoke event for an older generation; ignore
                return;
              }
            } catch {}
          }
          store.upsertHost({ ...existing, authStatus: "revoked", online: false });
        }
      }
    }
  }

  async function subscribeAppWide(): Promise<InventoryUnlisten> {
    if (appWideUnlisten) return appWideUnlisten;
    if (isTauri()) {
      try {
        const unlisten = await listen<InventoryChangeEvent>("paired_host_inventory_changed", (e) => {
          handleNativeEvent(e.payload);
        });
        appWideUnlisten = () => {
          unlisten();
          appWideUnlisten = null;
        };
        return appWideUnlisten;
      } catch (err) {
        console.warn("Failed to subscribe app-wide to paired host inventory events", err);
      }
    }
    return () => {};
  }

  async function refresh() {
    const request = ++refreshRequest;
    const started = revision;
    try {
      const [views, capability] = await Promise.all([commands.list(), commands.capabilities()]);
      if (capability.pairedHostInventoryV1 !== true) throw new Error("INVENTORY_UNAVAILABLE");
      const validViews = views.filter(v => {
        const gen = BigInt(v.generation);
        const fenced = fencedGenerations.get(v.hostId);
        if (fenced !== undefined && gen < fenced) return false;
        const tomb = tombstones.get(v.hostId);
        if (tomb !== undefined && gen <= tomb) return false;
        return true;
      });
      const hosts = validViews.map(endpoint);
      if (request !== refreshRequest || started !== revision) return;
      for (const v of validViews) {
        fencedGenerations.set(v.hostId, BigInt(v.generation));
      }
      proxyAvailable = capability.pairedDaemonProxyV1 === true;
      store.setHosts(hosts);
      store.setState(s => ({ ...s, nativeStatus: "ready", machineFeaturesEnabled: true }));
    } catch { if (request === refreshRequest && started === revision) unavailable(); }
  }

  async function pair(request: PairHostRequest, onPaired?: (host: HostEndpoint) => void): Promise<PairResult> {
    const started = ++revision;
    try {
      const relayOrigin = origin(request.relayOrigin);
      const host = endpoint(await commands.pair({ ...request, relayOrigin }));
      if (host.relayOrigin !== relayOrigin) {
        return {
          ok: false,
          error: {
            code: "STALE_HOST_GENERATION",
            message: "Credentials changed during this request. Refresh the inventory and retry.",
            retryable: true,
          },
        };
      }
      if (started !== revision) {
        // R3-N3: native event arrival during the IPC round-trip increments revision;
        // if the store already reflects this exact host, active paired auth, and generation, accept it.
        const current = store.getState().hosts[host.hostId];
        const isSelfUpdate = current &&
          current.generation === host.generation &&
          current.authStatus === "paired" &&
          current.machineId === host.machineId;
        if (!isSelfUpdate) {
          return {
            ok: false,
            error: {
              code: "STALE_HOST_GENERATION",
              message: "Credentials changed during this request. Refresh the inventory and retry.",
              retryable: true,
            },
          };
        }
      }
      fencedGenerations.set(host.hostId, BigInt(host.generation!));
      tombstones.delete(host.hostId);
      store.upsertHost(host);
      onPaired?.(host);
      return { ok: true, host };
    } catch (err) {
      const structured = extractPairedHostError(err);
      return { ok: false, error: structured };
    }
  }

  /** Deprecated boolean shim for backwards compatibility with external callers */
  async function pairBoolean(request: PairHostRequest, onPaired?: (host: HostEndpoint) => void): Promise<boolean> {
    const result = await pair(request, onPaired);
    return result.ok;
  }

  async function forget(hostId: string): Promise<boolean> {
    const host = store.getState().hosts[hostId];
    if (!host?.generation) return false;
    const started = ++revision;
    try {
      await commands.forget({ hostId, generation: host.generation });
      if (started !== revision) {
        const current = store.getState().hosts[hostId];
        if (current && current.generation !== host.generation) return false;
      }
      const gen = BigInt(host.generation);
      fencedGenerations.set(hostId, gen);
      tombstones.set(hostId, gen);
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
        if (receipt.hostId !== hostId || !validGeneration(receipt.generation)) throw new Error("MIGRATION_PENDING");
        // R4-N1: native migrate emits paired_host_inventory_changed before returning receipt,
        // incrementing global revision. If store reflects this exact host and generation, accept it.
        const hostAtReceipt = store.getState().hosts[hostId];
        const selfMigrate = hostAtReceipt && hostAtReceipt.generation === receipt.generation && hostAtReceipt.hostId === hostId;
        if (started !== revision && !selfMigrate) throw new Error("MIGRATION_PENDING");
        const verified = endpoint(await commands.read(receipt));
        if (verified.hostId !== receipt.hostId || verified.generation !== receipt.generation) throw new Error("MIGRATION_PENDING");
        const hostAtVerified = store.getState().hosts[hostId];
        const selfVerified = hostAtVerified && hostAtVerified.generation === receipt.generation && hostAtVerified.hostId === receipt.hostId;
        if (started !== revision && !selfVerified) throw new Error("MIGRATION_PENDING");
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
      if (started === revision || (!pending && Object.keys(store.getState().hosts).length > 0)) store.setState(s => ({ ...s, migrationStatus: pending ? "pending" : "complete" }));
    } catch { store.setState(s => ({ ...s, migrationStatus: "pending" })); }
  }
  return {
    refresh,
    pair,
    pairBoolean,
    forget,
    capture,
    migrateLegacy,
    setProjectsEnabled,
    getProjectsEnabled,
    hasProxyCapability,
    subscribeAppWide,
    handleNativeEvent,
  };
}
export const pairedHostInventory = createPairedHostInventory(remoteHostStore, nativePairedHostCommands,
  typeof localStorage === "undefined" ? undefined : localStorage);
export async function bootstrapPairedHostInventory() {
  if (!isTauri()) return;
  await pairedHostInventory.subscribeAppWide();
  await pairedHostInventory.migrateLegacy();
  await pairedHostInventory.refresh();
}
