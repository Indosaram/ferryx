export type TransportType = "tailscale" | "mdns" | "sshTunnel";
export type HostAuthStatus = "paired" | "unpaired" | "unknown";

export interface HostEndpoint {
  hostId: string;
  name: string;
  address: string;
  transport: TransportType;
  latencyMs?: number;
  authStatus: HostAuthStatus;
  online: boolean;
}

export interface RemoteHostState {
  readonly hosts: Readonly<Record<string, HostEndpoint>>;
  readonly activeHostId: string | null; // null represents local host
  readonly discovering: boolean;
}

export type RemoteHostActions = {
  setHosts: (hosts: HostEndpoint[]) => void;
  upsertHost: (host: HostEndpoint) => void;
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
export function createRemoteHostStore(initialState: RemoteHostState = INITIAL_STATE): RemoteHostStore {
  let state = initialState;
  const listeners = new Set<(s: RemoteHostState) => void>();

  function getState(): RemoteHostState {
    return state;
  }

  function setState(updater: (prev: RemoteHostState) => RemoteHostState): void {
    const nextState = updater(state);
    if (nextState === state) return;
    state = nextState;
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

  function setHosts(hosts: HostEndpoint[]): void {
    setState((prev) => ({
      ...prev,
      hosts: Object.fromEntries(hosts.map((host) => [host.hostId, host])),
    }));
  }

  function upsertHost(host: HostEndpoint): void {
    setState((prev) => ({
      ...prev,
      hosts: {
        ...prev.hosts,
        [host.hostId]: host,
      },
    }));
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
