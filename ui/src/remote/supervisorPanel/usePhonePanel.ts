// The phone panel's data: GET /api/v1/supervisor/panel on the hub host, then `panel.*` owner
// frames from that host's `/api/v1/events` (through the supervisor frame bus). It subscribes
// before it fetches and never polls; a subscribe or lag boundary refetches once. Actions are
// direct hub routes (answer, pause, converse); each refetches on success so the card leaves.
import { useCallback, useEffect, useRef, useState } from "react";

import type { PanelSnapshot, SessionRef } from "../../lib/supervisorPanel/contract";
import type { SupervisorFrameBus } from "../home/machineSupervisorSources";
import type { SupervisorFetch } from "../remoteSupervisor";
import { applyPanelEvent, parsePanelFrame } from "./panelModel";

export type PanelState =
  | { readonly kind: "loading" }
  | { readonly kind: "ready"; readonly snapshot: PanelSnapshot }
  /** The host has no panel route (404): the caller keeps the previous home until it does. */
  | { readonly kind: "unsupported" }
  | { readonly kind: "error"; readonly message: string };

export type PanelActionResult = { readonly ok: true } | { readonly ok: false; readonly stale: boolean; readonly message: string };

function decode(body: Uint8Array | string): unknown {
  try {
    return JSON.parse(typeof body === "string" ? body : new TextDecoder().decode(body));
  } catch {
    return null;
  }
}

function isSnapshot(value: unknown): value is PanelSnapshot {
  if (!value || typeof value !== "object") return false;
  const record = value as Record<string, unknown>;
  return Array.isArray(record.decisions) && Array.isArray(record.handling) && Array.isArray(record.sessions) && Array.isArray(record.log);
}

export async function fetchPanel(fetchLike: SupervisorFetch): Promise<PanelState> {
  let res;
  try {
    res = await fetchLike("/api/v1/supervisor/panel");
  } catch {
    return { kind: "error", message: "This computer is not reachable right now." };
  }
  if (res.status === 404) return { kind: "unsupported" };
  if (res.status < 200 || res.status >= 300) return { kind: "error", message: "Couldn't read the supervisor panel." };
  const data = decode(res.body);
  if (!isSnapshot(data)) return { kind: "error", message: "Couldn't read the supervisor panel." };
  const record = data as unknown as Partial<PanelSnapshot>;
  return { kind: "ready", snapshot: { ...data, rules: record.rules ?? [], paused: record.paused === true, setupRequired: record.setupRequired === true } };
}

function isResyncFrame(frame: unknown): boolean {
  const value = typeof frame === "string" ? decode(frame) : frame;
  if (!value || typeof value !== "object") return false;
  const record = value as Record<string, unknown>;
  return record.type === "inventoryInvalidated" && (record.reason === "subscribe" || record.reason === "lag");
}

function jsonPost(body: unknown) {
  return { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) };
}

async function post(fetchLike: SupervisorFetch, path: string, body: unknown): Promise<PanelActionResult> {
  try {
    const res = await fetchLike(path, jsonPost(body));
    if (res.status >= 200 && res.status < 300) return { ok: true };
    return { ok: false, stale: res.status === 409, message: res.status === 403 ? "This phone can only view the panel." : "That didn't go through." };
  } catch {
    return { ok: false, stale: false, message: "This computer is not reachable right now." };
  }
}

export type PhonePanel = {
  readonly state: PanelState;
  /** Refetches; the promise settles when the new snapshot is in (pull-to-refresh holds on it). */
  readonly refresh: () => Promise<void>;
  readonly answer: (sessionRef: SessionRef, promptId: string, screenRevision: string | null, optionIndex: number) => Promise<PanelActionResult>;
  readonly setPaused: (paused: boolean) => Promise<PanelActionResult>;
  readonly converse: (text: string) => Promise<PanelActionResult>;
};

export function usePhonePanel(input: {
  readonly hostId: string | null;
  readonly fetchFor: (machineId: string) => SupervisorFetch | null;
  readonly bus: SupervisorFrameBus;
  readonly enabled: boolean;
}): PhonePanel {
  const { hostId, fetchFor, bus, enabled } = input;
  const [state, setState] = useState<PanelState>({ kind: "loading" });
  const fetchLike = enabled && hostId ? fetchFor(hostId) : null;
  const fetchRef = useRef(fetchLike);
  fetchRef.current = fetchLike;
  const seqRef = useRef(0);

  const refresh = useCallback(async () => {
    const current = fetchRef.current;
    if (!current) return;
    const seq = (seqRef.current += 1);
    const next = await fetchPanel(current);
    // A newer fetch or an event applied since: only the latest read wins; a failed refetch
    // keeps the last good snapshot on screen.
    if (seqRef.current !== seq) return;
    setState((previous) => (next.kind === "error" && previous.kind === "ready" ? previous : next));
  }, []);

  useEffect(() => {
    if (!enabled || !hostId || !fetchLike) {
      setState({ kind: "loading" });
      return;
    }
    const unsubscribe = bus.subscribe(hostId, (frame) => {
      if (isResyncFrame(frame)) {
        void refresh();
        return;
      }
      const event = parsePanelFrame(frame);
      if (!event) return;
      seqRef.current += 1;
      setState((previous) => (previous.kind === "ready" ? { kind: "ready", snapshot: applyPanelEvent(previous.snapshot, event) } : previous));
    });
    void refresh();
    return unsubscribe;
    // The fetch identity changes with the tunnel; a replaced tunnel resubscribes and refetches.
  }, [enabled, hostId, fetchLike, bus, refresh]);

  const act = useCallback(
    async (path: string, body: unknown): Promise<PanelActionResult> => {
      const current = fetchRef.current;
      if (!current) return { ok: false, stale: false, message: "This computer is not connected right now." };
      const result = await post(current, path, body);
      // A stale answer means the card moved on: read the truth again.
      if (result.ok || result.stale) await refresh();
      return result;
    },
    [refresh],
  );

  return {
    state,
    refresh,
    answer: (sessionRef, promptId, screenRevision, optionIndex) =>
      act(`/api/v1/supervisor/decisions/${encodeURIComponent(sessionRef)}/answer`, { promptId, screenRevision, optionIndex }),
    setPaused: (paused) => act("/api/v1/supervisor/pause", { paused }),
    converse: (text) => act("/api/v1/supervisor/converse", { text, mentions: [], requestId: globalThis.crypto.randomUUID() }),
  };
}
