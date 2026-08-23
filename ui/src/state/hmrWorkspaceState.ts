import type { WorkspaceState } from "./workspaceStore";

export type HmrWorkspaceStoreData = {
  workspaceStates?: Record<string, WorkspaceState>;
};

const HMR_REGISTRY_SYMBOL = "__FERRYX_HMR_WORKSPACE_STATES__";

interface GlobalWithHmrRegistry {
  [HMR_REGISTRY_SYMBOL]?: Record<string, WorkspaceState>;
}

function getHmrRegistry(): Record<string, WorkspaceState> | null {
  if (typeof import.meta !== "undefined" && import.meta.env?.DEV) {
    if (typeof window !== "undefined") {
      const globalWindow = window as unknown as GlobalWithHmrRegistry;
      if (!globalWindow[HMR_REGISTRY_SYMBOL]) {
        globalWindow[HMR_REGISTRY_SYMBOL] = {};
      }
      return globalWindow[HMR_REGISTRY_SYMBOL];
    }
    if (typeof globalThis !== "undefined") {
      const globalObject = globalThis as unknown as GlobalWithHmrRegistry;
      if (!globalObject[HMR_REGISTRY_SYMBOL]) {
        globalObject[HMR_REGISTRY_SYMBOL] = {};
      }
      return globalObject[HMR_REGISTRY_SYMBOL];
    }
  }
  return null;
}

export function getHmrWorkspaceState(workspaceId: string): WorkspaceState | null {
  const registry = getHmrRegistry();
  if (registry && registry[workspaceId]) {
    return registry[workspaceId];
  }
  if (typeof import.meta !== "undefined" && import.meta.hot?.data) {
    const hotData = import.meta.hot.data as HmrWorkspaceStoreData | undefined;
    return hotData?.workspaceStates?.[workspaceId] ?? null;
  }
  return null;
}

export function setHmrWorkspaceState(workspaceId: string, state: WorkspaceState): void {
  const registry = getHmrRegistry();
  if (registry) {
    registry[workspaceId] = state;
  }
  if (typeof import.meta !== "undefined" && import.meta.hot?.data) {
    const hotData = import.meta.hot.data as HmrWorkspaceStoreData;
    if (!hotData.workspaceStates) {
      hotData.workspaceStates = {};
    }
    hotData.workspaceStates[workspaceId] = state;
  }
}

export function clearHmrWorkspaceState(workspaceId?: string): void {
  const registry = getHmrRegistry();
  if (registry) {
    if (workspaceId) {
      delete registry[workspaceId];
    } else {
      for (const key of Object.keys(registry)) {
        delete registry[key];
      }
    }
  }
  if (typeof import.meta !== "undefined" && import.meta.hot?.data) {
    const hotData = import.meta.hot.data as HmrWorkspaceStoreData | undefined;
    if (hotData?.workspaceStates) {
      if (workspaceId) {
        delete hotData.workspaceStates[workspaceId];
      } else {
        hotData.workspaceStates = {};
      }
    }
  }
}

if (typeof import.meta !== "undefined" && import.meta.hot) {
  const hotData = import.meta.hot.data as HmrWorkspaceStoreData | undefined;
  if (hotData?.workspaceStates) {
    const registry = getHmrRegistry();
    if (registry) {
      Object.assign(registry, hotData.workspaceStates);
    }
  }
  import.meta.hot.dispose((data: Record<string, unknown>) => {
    const registry = getHmrRegistry();
    if (registry) {
      data.workspaceStates = { ...registry };
    }
  });
}
