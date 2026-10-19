import { useMemo, useSyncExternalStore } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { extractPairedHostError } from "./pairedHostInventory";
import { bootTrace } from "./tauri";

// TS mirror of src-tauri/src/paired_host/reachability.rs (plan "Wire contract").
export type ReachabilityState = "checking" | "online" | "offline";
export interface HostReachability {
  hostId: string;
  /** Canonical decimal u64 (Rust `Epoch`). */
  generation: string;
  state: ReachabilityState;
  checkedAtMs: number;
  lastError: string | null;
}
export interface PairedHostReachabilityList { supported: boolean; entries: HostReachability[] }
export interface PairedHostReachabilityView { supported: boolean; entry: HostReachability | null }

export const PAIRED_HOST_REACHABILITY_EVENT = "paired_host_reachability_changed";
export const PAIRED_HOST_REACHABILITY_LIST_COMMAND = "paired_host_reachability_list";

let supported = false;
let entries = new Map<string, HostReachability>();
let bootstrapped: Promise<void> | null = null;
let unlisten: (() => void) | null = null;
const listeners = new Set<() => void>();

function emit(): void {
  for (const listener of listeners) listener();
}

function canonicalGeneration(value: unknown): value is string {
  return typeof value === "string" && /^(0|[1-9][0-9]*)$/.test(value) && BigInt(value) <= 18446744073709551615n;
}

/** Boundary validation: a malformed native payload is dropped, never stored. */
function validEntry(value: unknown): value is HostReachability {
  if (!value || typeof value !== "object") return false;
  const v = value as Record<string, unknown>;
  return typeof v.hostId === "string" && v.hostId.length > 0
    && canonicalGeneration(v.generation)
    && (v.state === "checking" || v.state === "online" || v.state === "offline")
    && typeof v.checkedAtMs === "number" && Number.isFinite(v.checkedAtMs)
    && (v.lastError === null || typeof v.lastError === "string");
}

/** Upserts by hostId. An older generation, or an older probe of the same generation, is dropped. */
function upsert(entry: HostReachability): boolean {
  const current = entries.get(entry.hostId);
  if (current) {
    const incoming = BigInt(entry.generation);
    const stored = BigInt(current.generation);
    if (incoming < stored) return false;
    if (incoming === stored && entry.checkedAtMs < current.checkedAtMs) return false;
  }
  entries = new Map(entries).set(entry.hostId, entry);
  return true;
}

function applyEvent(payload: unknown): void {
  if (validEntry(payload) && upsert(payload)) emit();
}

function applyList(list: unknown): void {
  const value = list as Partial<PairedHostReachabilityList> | null;
  supported = value?.supported === true;
  for (const entry of Array.isArray(value?.entries) ? value.entries : []) {
    if (validEntry(entry)) upsert(entry);
  }
  emit();
}

/**
 * One list call plus the change event; never polls. Outside Tauri, or when the command is
 * missing or fails (older backend without `paired_host_reachability_list`), the store reports
 * `supported: false` and callers fall back to the legacy `online` flag.
 */
export function bootstrapPairedHostReachability(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  if (bootstrapped) return bootstrapped;
  bootstrapped = (async () => {
    // Subscribe before listing so a change racing the list is not lost; upsert() orders them.
    try {
      unlisten = await listen<unknown>(PAIRED_HOST_REACHABILITY_EVENT, event => applyEvent(event.payload));
    } catch (err) {
      recordUnavailable("listen", err);
    }
    try {
      applyList(await invoke<PairedHostReachabilityList>(PAIRED_HOST_REACHABILITY_LIST_COMMAND));
    } catch (err) {
      supported = false;
      emit();
      recordUnavailable("list", err);
    }
  })();
  return bootstrapped;
}

/** Only the structured code is recorded; the raw native error never leaves this boundary. */
function recordUnavailable(operation: "listen" | "list", err: unknown): void {
  try {
    void bootTrace("paired.reachability.unavailable", { operation, code: extractPairedHostError(err).code })
      ?.catch(() => undefined);
  } catch {
    // Diagnostics are best effort; the store has already failed closed to supported:false.
  }
}

export function subscribePairedHostReachability(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getPairedHostReachability(hostId: string): PairedHostReachabilityView {
  return { supported, entry: entries.get(hostId) ?? null };
}

export function usePairedHostReachability(hostId: string): PairedHostReachabilityView {
  const isSupported = useSyncExternalStore(subscribePairedHostReachability, () => supported, () => false);
  const entry = useSyncExternalStore(subscribePairedHostReachability, () => entries.get(hostId) ?? null, () => null);
  return useMemo(() => ({ supported: isSupported, entry }), [isSupported, entry]);
}

import.meta.hot?.dispose(() => {
  unlisten?.();
  unlisten = null;
  bootstrapped = null;
});
