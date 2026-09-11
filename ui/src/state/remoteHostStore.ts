import type { CandidateEndpoint } from "../lib/directPathUpgrade";

export type TransportType = "tailscale" | "mdns" | "sshTunnel" | "relay";
export type HostAuthStatus = "paired" | "unpaired" | "unknown";

export interface HostEndpoint {
  hostId: string;
  name: string;
  address: string;
  transport: TransportType;
  latencyMs?: number;
  authStatus: HostAuthStatus;
  online: boolean;
  machineId?: string;
  displayName?: string;
  relayOrigin?: string;
  deviceToken?: string | null;
  lastSeenAt?: number | null;
  directHints?: CandidateEndpoint[];
}

export interface PairedRemoteHost {
  machineId: string;
  displayName: string;
  relayOrigin: string;
  deviceToken: string | null;
  lastSeenAt: number | null;
  directHints: CandidateEndpoint[];
}

export const REMOTE_HOST_STORAGE_KEY = "ferryx_remote_hosts";

export function remoteHostKey(relayOrigin: string, machineId: string): string {
  return `${new URL(relayOrigin).origin}/host/${encodeURIComponent(machineId)}`;
}

function normalizeHost(input: HostEndpoint | PairedRemoteHost, storage?: Storage): HostEndpoint {
  const legacy = "hostId" in input ? input : null;
  const relayOrigin = input.relayOrigin ? new URL(input.relayOrigin).origin : undefined;
  const hostId = relayOrigin && input.machineId
    ? remoteHostKey(relayOrigin, input.machineId) : legacy!.hostId;
  // Only migrate explicitly host-scoped credentials. An origin-wide token cannot
  // identify which machine on a shared relay it belongs to.
  const tokenKey = `ferryx_remote_token_${legacy?.hostId ?? hostId}`;
  const deviceToken = input.deviceToken !== undefined ? input.deviceToken : storage?.getItem(tokenKey) ?? null;
  return {
    ...input,
    hostId,
    name: input.displayName ?? legacy!.name,
    address: relayOrigin ?? legacy!.address,
    transport: legacy?.transport ?? "relay",
    online: legacy?.online ?? false,
    authStatus: deviceToken ? "paired" : legacy?.authStatus ?? "unpaired",
    machineId: input.machineId ?? legacy!.hostId,
    displayName: input.displayName ?? legacy!.name,
    relayOrigin,
    deviceToken,
    lastSeenAt: input.lastSeenAt ?? null,
    directHints: input.directHints ?? [],
  };
}

function readInventory(storage?: Storage): RemoteHostState {
  if (!storage) return INITIAL_STATE;
  try {
    const raw = JSON.parse(storage.getItem(REMOTE_HOST_STORAGE_KEY) ?? "null");
    if (!raw) return INITIAL_STATE;
    const rows = raw.hosts ? Object.values(raw.hosts) : [raw];
    const hosts: Record<string, HostEndpoint> = {};
    let activeHostId: string | null = null;
    for (const row of rows) {
      if (!row || typeof row !== "object") continue;
      const host = row as HostEndpoint;
      if (!(typeof host.machineId === "string" && typeof host.relayOrigin === "string" && typeof host.displayName === "string")
        && !(typeof host.hostId === "string" && typeof host.name === "string" && typeof host.address === "string")) continue;
      const normalized = normalizeHost(host, storage);
      hosts[normalized.hostId] = normalized;
      if (raw.activeHostId === host.hostId || raw.activeHostId === normalized.hostId || !raw.hosts) activeHostId = normalized.hostId;
    }
    return { hosts, activeHostId, discovering: false };
  } catch (error) {
    console.warn("Unable to restore remote host inventory", error);
    return INITIAL_STATE;
  }
}

export interface RemoteHostState {
  readonly hosts: Readonly<Record<string, HostEndpoint>>;
  readonly activeHostId: string | null; // null represents local host
  readonly discovering: boolean;
}

export type RemoteHostActions = {
  setHosts: (hosts: (HostEndpoint | PairedRemoteHost)[]) => void;
  upsertHost: (host: HostEndpoint | PairedRemoteHost) => void;
  removeHost: (hostId: string) => void;
  setActiveHost: (hostId: string | null) => void;
  setDiscovering: (discovering: boolean) => void;
};

export type RemoteHostStore = RemoteHostActions & {
  getState: () => RemoteHostState;
  setState: (updater: (prev: RemoteHostState) => RemoteHostState) => void;
  subscribe: (listener: (state: RemoteHostState) => void) => () => void;
  reset: () => void;
};

const INITIAL_STATE: RemoteHostState = {
  hosts: {},
  activeHostId: null,
  discovering: false,
};

export function selectActiveHost(state: RemoteHostState): HostEndpoint | null {
  if (state.activeHostId === null) return null;
  return state.hosts[state.activeHostId] ?? null;
}

export function selectHostList(state: RemoteHostState): readonly HostEndpoint[] {
  return Object.values(state.hosts).sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * Creates an isolated remote-host registry store. Switching `activeHostId` only ever changes
 * that single field — the `hosts` map (and therefore any local workspace/terminal session state
 * that lives outside this store) is left untouched, so local sessions are never dropped when the
 * user switches between hosts or back to local (`null`).
 */
export function createRemoteHostStore(
  initialState?: RemoteHostState,
  storage: Storage | undefined = typeof localStorage === "undefined" ? undefined : localStorage,
): RemoteHostStore {
  let state = initialState ?? readInventory(storage);
  const listeners = new Set<(s: RemoteHostState) => void>();

  function getState(): RemoteHostState {
    return state;
  }

  function setState(updater: (prev: RemoteHostState) => RemoteHostState): void {
    const nextState = updater(state);
    if (nextState === state) return;
    state = nextState;
    try {
      storage?.setItem(REMOTE_HOST_STORAGE_KEY, JSON.stringify({ ...state, discovering: false }));
    } catch (error) {
      console.warn("Unable to persist remote host inventory", error);
    }
    for (const listener of listeners) {
      listener(state);
    }
  }

  function subscribe(listener: (s: RemoteHostState) => void): () => void {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }

  function mergeHost(input: HostEndpoint | PairedRemoteHost, prev: RemoteHostState): HostEndpoint {
    const host = normalizeHost(input, storage);
    const existing = prev.hosts[host.hostId];
    return normalizeHost({ ...existing, ...host,
      deviceToken: input.deviceToken === undefined ? existing?.deviceToken ?? host.deviceToken : input.deviceToken,
      directHints: input.directHints ?? existing?.directHints ?? host.directHints,
    });
  }

  function setHosts(hosts: (HostEndpoint | PairedRemoteHost)[]): void {
    setState((prev) => ({
      ...prev,
      // Discovery refreshes must not erase paired machines that are offline.
      hosts: {
        ...Object.fromEntries(Object.entries(prev.hosts).filter(([, host]) => host.deviceToken)),
        ...Object.fromEntries(hosts.map((input) => {
          const host = mergeHost(input, prev);
          return [host.hostId, host];
        })),
      },
    }));
  }

  function upsertHost(input: HostEndpoint | PairedRemoteHost): void {
    setState((prev) => {
      const host = mergeHost(input, prev);
      return { ...prev, hosts: { ...prev.hosts, [host.hostId]: host } };
    });
  }

  function removeHost(hostId: string): void {
    setState((prev) => {
      if (!(hostId in prev.hosts)) return prev;
      const { [hostId]: _removed, ...nextHosts } = prev.hosts;
      return {
        ...prev,
        hosts: nextHosts,
        // Dropping the currently-active remote host falls back to local rather than pointing at
        // a host that no longer exists.
        activeHostId: prev.activeHostId === hostId ? null : prev.activeHostId,
      };
    });
  }

  function setActiveHost(hostId: string | null): void {
    setState((prev) => {
      if (prev.activeHostId === hostId) return prev;
      return { ...prev, activeHostId: hostId };
    });
  }

  function setDiscovering(discovering: boolean): void {
    setState((prev) => {
      if (prev.discovering === discovering) return prev;
      return { ...prev, discovering };
    });
  }

  function reset(): void {
    setState(() => INITIAL_STATE);
  }

  return {
    getState,
    setState,
    subscribe,
    setHosts,
    upsertHost,
    removeHost,
    setActiveHost,
    setDiscovering,
    reset,
  };
}

export const remoteHostStore = createRemoteHostStore();
