import { useMemo, useSyncExternalStore } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { extractPairedHostError } from "./pairedHostInventory";
import { bootTrace } from "./tauri";

// TS mirror of src-tauri/src/paired_host/route_view.rs. The direct route's address is never sent.
export type HostRouteState = "relay" | "attempting" | "direct" | "disconnected";
export interface HostRoute { hostId: string; state: HostRouteState }
export interface PairedHostRouteList { supported: boolean; entries: HostRoute[] }
export interface PairedHostRouteView { supported: boolean; entry: HostRoute | null }

export const PAIRED_HOST_ROUTE_EVENT = "paired_host_route_changed";
export const PAIRED_HOST_ROUTE_LIST_COMMAND = "paired_host_route_list";

let supported = false;
let entries = new Map<string, HostRoute>();
// Hosts whose state arrived by event; a list response that raced the event must not overwrite it.
let eventSeen = new Set<string>();
let listed = false;
let bootstrapped: Promise<void> | null = null;
let unlisten: (() => void) | null = null;
let listenPromise: Promise<void> | null = null;
let lifecycle = 0;
let liveEventSeen = false;
const listeners = new Set<() => void>();

function emit(): void {
  for (const listener of listeners) listener();
}

/** Boundary validation: a malformed native payload is dropped, never stored. */
function validEntry(value: unknown): value is HostRoute {
  if (!value || typeof value !== "object") return false;
  const v = value as Record<string, unknown>;
  return typeof v.hostId === "string" && v.hostId.length > 0
    && (v.state === "relay" || v.state === "attempting" || v.state === "direct" || v.state === "disconnected");
}

function store(entry: HostRoute): void {
  entries = new Map(entries).set(entry.hostId, { hostId: entry.hostId, state: entry.state });
}

function applyEvent(payload: unknown): void {
  if (!validEntry(payload)) return;
  liveEventSeen = true;
  supported = true;
  if (!listed) eventSeen.add(payload.hostId);
  store(payload);
  emit();
}

function applyList(list: unknown): void {
  const value = list as Partial<PairedHostRouteList> | null;
  supported = liveEventSeen || value?.supported === true;
  for (const entry of Array.isArray(value?.entries) ? value.entries : []) {
    if (validEntry(entry) && !eventSeen.has(entry.hostId)) store(entry);
  }
  listed = true;
  eventSeen = new Set();
  emit();
}

/**
 * One list call plus the change event; never polls. Outside Tauri, or when the command is
 * missing, the store reports `supported: false`. Transient failures allow a later retry.
 */
export function bootstrapPairedHostRoute(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  if (bootstrapped) return bootstrapped;
  const epoch = lifecycle;
  listed = false;
  eventSeen = new Set();
  // Subscription ownership is independent of retryable list requests.
  listenPromise ??= listen<unknown>(PAIRED_HOST_ROUTE_EVENT, event => {
    if (epoch === lifecycle) applyEvent(event.payload);
  }).then(dispose => {
    if (epoch === lifecycle) unlisten = dispose;
    else dispose();
  }).catch((err: unknown) => {
    if (epoch !== lifecycle) return;
    listenPromise = null;
    recordUnavailable("listen", err);
  });
  bootstrapped = (async () => {
    await listenPromise;
    if (epoch !== lifecycle) return;
    try {
      const list = await invoke<PairedHostRouteList>(PAIRED_HOST_ROUTE_LIST_COMMAND);
      if (epoch === lifecycle) applyList(list);
    } catch (err) {
      if (epoch !== lifecycle) return;
      supported = liveEventSeen;
      listed = true;
      eventSeen = new Set();
      if (!isUnsupported(err)) bootstrapped = null;
      emit();
      recordUnavailable("list", err);
    }
    if (epoch === lifecycle && !listenPromise) bootstrapped = null;
  })();
  return bootstrapped;
}

function isUnsupported(err: unknown): boolean {
  const { code } = extractPairedHostError(err);
  return code === "UNKNOWN_COMMAND" || code === "UNSUPPORTED"
    || (typeof err === "string" && /unknown command|command .+ not found/i.test(err));
}

/** Only the structured code is recorded; the raw native error never leaves this boundary. */
function recordUnavailable(operation: "listen" | "list", err: unknown): void {
  try {
    void bootTrace("paired.route.unavailable", { operation, code: extractPairedHostError(err).code })
      ?.catch(() => undefined);
  } catch {
    // Diagnostics are best effort; the store has already failed closed to supported:false.
  }
}

export function subscribePairedHostRoute(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getPairedHostRoute(hostId: string): PairedHostRouteView {
  return { supported, entry: entries.get(hostId) ?? null };
}

export function usePairedHostRoute(hostId: string): PairedHostRouteView {
  const isSupported = useSyncExternalStore(subscribePairedHostRoute, () => supported, () => false);
  const entry = useSyncExternalStore(subscribePairedHostRoute, () => entries.get(hostId) ?? null, () => null);
  return useMemo(() => ({ supported: isSupported, entry }), [isSupported, entry]);
}

/** Chip text for a paired row: the live route when the daemon reports it, else the transport. */
export function pairedPathLabel(transport: string, route: PairedHostRouteView): string {
  if (route.supported && route.entry) {
    const labels = { direct: "Direct", relay: "Relay", attempting: "Connecting", disconnected: "Offline" } as const;
    return labels[route.entry.state];
  }
  return transport === "relay" ? "Relay" : transport;
}

export function resetPairedHostRouteForTests(): void {
  lifecycle += 1;
  unlisten?.();
  unlisten = null;
  listenPromise = null;
  bootstrapped = null;
  liveEventSeen = false;
  supported = false;
  entries = new Map();
  listed = false;
  eventSeen = new Set();
  listeners.clear();
}

import.meta.hot?.dispose(resetPairedHostRouteForTests);
