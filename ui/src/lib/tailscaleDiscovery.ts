import { useSyncExternalStore } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

import { getCachedSshHosts, mergeTailscaleSshHosts, subscribeSshHosts, type SshHost, type SshTargetSummary } from "./sshHosts";

export interface TailscaleDiscoveryError {
  code: string;
  message: string;
  details?: { stage?: string } | null;
}

export interface TailscaleSyncResult {
  hosts: SshHost[];
  targets: SshTargetSummary[];
  discoveryError?: TailscaleDiscoveryError | null;
}

export interface TailscaleDiscoveryState {
  status: "idle" | "discovering" | "ready" | "error";
  hosts: SshHost[];
  targets: SshTargetSummary[];
  targetsByHost: ReadonlyMap<string, SshTargetSummary>;
  discoveryError: TailscaleDiscoveryError | null;
  lastError: string | null;
  refreshing: boolean;
}

const initialState: TailscaleDiscoveryState = {
  status: "idle",
  hosts: [],
  targets: [],
  targetsByHost: new Map(),
  discoveryError: null,
  lastError: null,
  refreshing: false,
};

let state = initialState;
let inflight: Promise<TailscaleSyncResult> | null = null;
let rerunRequested = false;
let lifecycleUsers = 0;
let inventoryEpoch = 0;
let stopLifecycle: (() => void) | null = null;
let observedInventory = "[]";
let publishingInventory = false;
const listeners = new Set<() => void>();

function publish(next: TailscaleDiscoveryState): void {
  state = next;
  for (const listener of listeners) listener();
}

export function getTailscaleDiscoverySnapshot(): TailscaleDiscoveryState {
  return state;
}

export function subscribeTailscaleDiscovery(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error && typeof error.message === "string") return error.message;
  return "Tailscale machine discovery failed.";
}

function syncOnce(): Promise<TailscaleSyncResult> {
  if (!isTauri()) return Promise.resolve({ hosts: [], targets: [], discoveryError: null });
  const requestEpoch = inventoryEpoch;
  publish({ ...state, status: state.status === "idle" ? "discovering" : state.status, refreshing: true, lastError: null });
  return invoke<TailscaleSyncResult>("cmd_ssh_sync_tailscale").then((result) => {
    if (requestEpoch === inventoryEpoch) {
      const hosts = [...result.hosts];
      publishingInventory = true;
      const merged = mergeTailscaleSshHosts(hosts);
      publishingInventory = false;
      const updatedInventory = getCachedSshHosts() ?? merged;
      const ids = new Set(updatedInventory.map((host) => host.id));
      publish({
        status: result.discoveryError ? "error" : "ready",
        hosts: updatedInventory.filter((host) => host.source === "tailscale"),
        targets: result.targets.filter((target) => ids.has(target.host.id)),
        targetsByHost: new Map(result.targets.filter((target) => ids.has(target.host.id)).map((target) => [target.host.id, target])),
        discoveryError: result.discoveryError ?? null,
        lastError: null,
        refreshing: false,
      });
    } else {
      publish({ ...state, refreshing: false });
      return { hosts: state.hosts, targets: state.targets, discoveryError: state.discoveryError };
    }
    return result;
  }).catch((error: unknown) => {
    if (requestEpoch === inventoryEpoch) {
      publish({ ...state, status: "error", refreshing: false, lastError: errorMessage(error) });
    }
    throw error;
  });
}

export async function syncTailscaleMachines(): Promise<TailscaleSyncResult> {
  return runSync();
}

async function runSync(): Promise<TailscaleSyncResult> {
  if (inflight) {
    rerunRequested = true;
    return inflight;
  }
  const current = syncOnce();
  inflight = current;
  try {
    return await current;
  } finally {
    if (inflight === current) inflight = null;
    if (rerunRequested) {
      rerunRequested = false;
      void runSync().catch(() => undefined);
    }
  }
}

function onInventoryChanged(hosts: SshHost[]): void {
  const fingerprint = JSON.stringify(hosts);
  if (fingerprint !== observedInventory && !publishingInventory) {
    inventoryEpoch += 1;
    if (inflight) rerunRequested = true;
  }
  observedInventory = fingerprint;
  const inventory = getCachedSshHosts() ?? hosts;
  const currentTailnet = inventory.filter((host) => host.source === "tailscale");
  const targets = state.targets.filter((target) => inventory.some((host) => JSON.stringify(host) === JSON.stringify(target.host)));
  publish({ ...state, hosts: currentTailnet, targets, targetsByHost: new Map(targets.map((target) => [target.host.id, target])) });
}

export function startTailscaleDiscovery(): () => void {
  if (!isTauri() || typeof window === "undefined") return () => undefined;
  lifecycleUsers += 1;
  if (lifecycleUsers === 1) {
    const refresh = () => {
      if (inflight) rerunRequested = true;
      else void runSync().catch(() => undefined);
    };
    window.addEventListener("focus", refresh);
    window.addEventListener("online", refresh);
    stopLifecycle = () => {
      window.removeEventListener("focus", refresh);
      window.removeEventListener("online", refresh);
    };
    void runSync().catch(() => undefined);
  }
  let stopped = false;
  return () => {
    if (stopped) return;
    stopped = true;
    lifecycleUsers -= 1;
    if (lifecycleUsers === 0) {
      stopLifecycle?.();
      stopLifecycle = null;
    }
  };
}

const stopInventorySubscription = subscribeSshHosts(onInventoryChanged);
import.meta.hot?.dispose(() => {
  stopInventorySubscription();
  stopLifecycle?.();
});

export function useTailscaleDiscovery(): TailscaleDiscoveryState {
  return useSyncExternalStore(subscribeTailscaleDiscovery, getTailscaleDiscoverySnapshot, getTailscaleDiscoverySnapshot);
}

export function useTailscaleTargetsByHost(): ReadonlyMap<string, SshTargetSummary> {
  return useTailscaleDiscovery().targetsByHost;
}

export function resetTailscaleDiscoveryForTests(): void {
  stopLifecycle?.();
  stopLifecycle = null;
  lifecycleUsers = 0;
  inflight = null;
  rerunRequested = false;
  inventoryEpoch = 0;
  observedInventory = "[]";
  publishingInventory = false;
  publish(initialState);
}

export function isTailscaleProvisioned(target: SshTargetSummary | undefined): boolean {
  return Boolean(target?.reachable && target.helper === "installed" && !target.diagnostic);
}
