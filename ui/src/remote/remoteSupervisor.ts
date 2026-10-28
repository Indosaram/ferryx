// The phone's view of each machine's hub supervisor over the remote API. Same contract as the
// desktop feed: subscribe to `supervisorChanged` first, then fetch; refetch only on change events
// or a socket resync, never poll. One timer re-renders when an acting/held glow expires.
import { useEffect, useMemo, useRef, useState } from "react";
import {
  nextSupervisionExpiry,
  normalizeSnapshot,
  type SupervisorLane,
  type SupervisorSnapshot,
} from "../lib/supervisorModel";
import type { SupervisorCallResult, SupervisorEnvelope, UnifiedSupervisorClient } from "../lib/unifiedSupervisor";

export const SUPERVISOR_CAPABILITY = "supervisorV1";
export const SUPERVISOR_CHANGED_FRAME = "supervisorChanged";

/** One HTTP response as the tunnel (`TunnelResponse`) or a direct-fetch adapter returns it. */
export type SupervisorHttpResponse = { readonly status: number; readonly body: Uint8Array | string };

/**
 * Issues one request against a machine's remote API. The adapter binds the machine's device
 * token, so callers pass only the path; `transport.fetchLike` with an Authorization header fits.
 */
export type SupervisorFetch = (
  path: string,
  init?: { method?: string; headers?: Record<string, string>; body?: string },
) => Promise<SupervisorHttpResponse>;

export type SupervisorSnapshotResult =
  | { kind: "ok"; snapshot: SupervisorSnapshot }
  | { kind: "unsupported" }
  | { kind: "error"; code: string; message: string };

export type LanePauseResult =
  | { kind: "ok"; laneKey: string; paused: boolean; changed: boolean }
  | { kind: "error"; code: string; message: string; status: number | null };

/** Error code for a transport that threw before any HTTP status arrived. */
export const SUPERVISOR_OFFLINE = "OFFLINE";

const STATUS_CODES: Record<number, string> = {
  400: "INVALID_REQUEST",
  403: "FORBIDDEN",
  404: "NOT_FOUND",
  409: "CONFLICT",
  503: "SUPERVISOR_UNAVAILABLE",
};

function decodeJson(body: Uint8Array | string): unknown {
  try {
    return JSON.parse(typeof body === "string" ? body : new TextDecoder().decode(body));
  } catch (err) {
    if (err instanceof SyntaxError) return null;
    throw err;
  }
}

function objectOf(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function errorOf(res: SupervisorHttpResponse): { code: string; message: string } {
  const data = objectOf(decodeJson(res.body));
  // The machine API answers `{error: {code, message}}`; older routes answered `{code, message}`.
  const envelope = objectOf(data?.error) ?? data;
  const code = typeof envelope?.code === "string" && envelope.code ? envelope.code : (STATUS_CODES[res.status] ?? "HTTP_ERROR");
  const message = typeof envelope?.message === "string" && envelope.message ? envelope.message : `Supervisor request failed (${res.status})`;
  return { code, message };
}

function offline(err: unknown): { code: string; message: string } {
  return { code: SUPERVISOR_OFFLINE, message: err instanceof Error ? err.message : String(err) };
}

/** Accepts the GET /api/v1/capabilities document or its `capabilities` string list. */
export function hasSupervisorCapability(capabilities: unknown): boolean {
  const list = Array.isArray(capabilities) ? capabilities : objectOf(capabilities)?.capabilities;
  return Array.isArray(list) && list.includes(SUPERVISOR_CAPABILITY);
}

export async function fetchSupervisorSnapshot(fetchLike: SupervisorFetch): Promise<SupervisorSnapshotResult> {
  let res: SupervisorHttpResponse;
  try {
    res = await fetchLike("/api/v1/supervisor");
  } catch (err) {
    return { kind: "error", ...offline(err) };
  }
  // An older daemon has no route at all.
  if (res.status === 404) return { kind: "unsupported" };
  if (res.status < 200 || res.status >= 300) return { kind: "error", ...errorOf(res) };
  const data = objectOf(decodeJson(res.body));
  if (!data) return { kind: "error", code: "INVALID_RESPONSE", message: "Supervisor response is not a JSON object" };
  return { kind: "ok", snapshot: normalizeSnapshot(data) };
}

export async function setRemoteLanePaused(
  fetchLike: SupervisorFetch,
  laneKey: string,
  paused: boolean,
): Promise<LanePauseResult> {
  let res: SupervisorHttpResponse;
  try {
    res = await fetchLike("/api/v1/supervisor/pause", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ laneKey, paused }),
    });
  } catch (err) {
    return { kind: "error", ...offline(err), status: null };
  }
  if (res.status < 200 || res.status >= 300) return { kind: "error", ...errorOf(res), status: res.status };
  const data = objectOf(decodeJson(res.body));
  return {
    kind: "ok",
    laneKey: typeof data?.laneKey === "string" ? data.laneKey : laneKey,
    paused: typeof data?.paused === "boolean" ? data.paused : paused,
    changed: data?.changed === true,
  };
}

function frameObject(frame: unknown): Record<string, unknown> | null {
  return objectOf(typeof frame === "string" ? decodeJson(frame) : frame);
}

/** One unified supervisor call (`/api/v1/supervisor/{request,conversation,memory}`). */
async function supervisorCall(
  fetchLike: SupervisorFetch,
  path: string,
  init?: { method?: string; headers?: Record<string, string>; body?: string },
): Promise<SupervisorCallResult> {
  let res: SupervisorHttpResponse;
  try {
    res = init ? await fetchLike(path, init) : await fetchLike(path);
  } catch (err) {
    return { kind: "error", ...offline(err), status: null };
  }
  if (res.status < 200 || res.status >= 300) return { kind: "error", ...errorOf(res), status: res.status };
  const data = objectOf(decodeJson(res.body));
  if (!data) return { kind: "error", code: "INVALID_RESPONSE", message: "Supervisor response is not a JSON object", status: res.status };
  return { kind: "ok", value: data };
}

function postJson(body: unknown) {
  return { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) };
}

/** `POST /api/v1/supervisor/request`: one envelope; the machine derives the actor. */
export function postSupervisorRequest(fetchLike: SupervisorFetch, envelope: SupervisorEnvelope): Promise<SupervisorCallResult> {
  return supervisorCall(fetchLike, "/api/v1/supervisor/request", postJson(envelope));
}

/** `GET /api/v1/supervisor/conversation?projectId=...&after=...`. */
export function fetchSupervisorConversation(fetchLike: SupervisorFetch, projectId: string, after: number): Promise<SupervisorCallResult> {
  const query = new URLSearchParams({ projectId, after: String(after) });
  return supervisorCall(fetchLike, `/api/v1/supervisor/conversation?${query.toString()}`);
}

/** `GET /api/v1/supervisor/memory`: the memory receipts. */
export function fetchSupervisorMemory(fetchLike: SupervisorFetch): Promise<SupervisorCallResult> {
  return supervisorCall(fetchLike, "/api/v1/supervisor/memory");
}

/** `POST /api/v1/supervisor/memory`: a `teach` or `forget` envelope. */
export function postSupervisorMemory(fetchLike: SupervisorFetch, envelope: SupervisorEnvelope): Promise<SupervisorCallResult> {
  return supervisorCall(fetchLike, "/api/v1/supervisor/memory", postJson(envelope));
}

/** The unified supervisor client over one machine's remote API. */
export function remoteUnifiedClient(fetchLike: SupervisorFetch, refresh: () => void): UnifiedSupervisorClient {
  return {
    request: (envelope) => postSupervisorRequest(fetchLike, envelope),
    conversation: (projectId, after) => fetchSupervisorConversation(fetchLike, projectId, after),
    memory: () => fetchSupervisorMemory(fetchLike),
    remember: (envelope) => postSupervisorMemory(fetchLike, envelope),
    refresh,
  };
}

/** True for an `/api/v1/events` frame (raw text or parsed) announcing a supervisor change. */
export function isSupervisorChangedFrame(frame: unknown): boolean {
  return frameObject(frame)?.type === SUPERVISOR_CHANGED_FRAME;
}

/** A fresh subscribe or a lagged socket may have missed supervisor changes: refetch once. */
function isResyncFrame(frame: unknown): boolean {
  const event = frameObject(frame);
  return event?.type === "inventoryInvalidated" && (event.reason === "subscribe" || event.reason === "lag");
}

/**
 * One machine as the hook needs it. `fetchLike` must already carry the device token;
 * `subscribe` delivers every `/api/v1/events` frame (string or parsed) and returns an
 * unsubscribe. `capabilities` is the capabilities document or list, or null while unknown
 * (then the route itself decides). Change `connectionId` when the transport is replaced so the
 * hook resubscribes on the new connection.
 */
export type RemoteSupervisorMachine = {
  readonly machineId: string;
  readonly label: string;
  readonly fetchLike: SupervisorFetch;
  readonly subscribe: (handler: (frame: unknown) => void) => () => void;
  readonly capabilities: unknown;
  readonly connectionId?: string;
};

export type RemoteSupervisorState = "loading" | "ok" | "unsupported" | "offline" | "error";

export type RemoteSupervisorEntry = {
  readonly machineId: string;
  readonly label: string;
  readonly state: RemoteSupervisorState;
  /** Present only while `state` is "ok". */
  readonly snapshot: SupervisorSnapshot | null;
  readonly error: { code: string; message: string } | null;
};

export type RemoteSupervisorLane = SupervisorLane & { readonly machineId: string };

type Settled = Pick<RemoteSupervisorEntry, "state" | "snapshot" | "error">;

function settle(result: SupervisorSnapshotResult): Settled {
  switch (result.kind) {
    case "ok":
      return { state: "ok", snapshot: result.snapshot, error: null };
    case "unsupported":
      return { state: "unsupported", snapshot: null, error: null };
    case "error":
      return { state: result.code === SUPERVISOR_OFFLINE ? "offline" : "error", snapshot: null, error: result };
    default: {
      const unreachable: never = result;
      return unreachable;
    }
  }
}

export function useRemoteSupervisor({ machines }: { readonly machines: readonly RemoteSupervisorMachine[] }): {
  machines: RemoteSupervisorEntry[];
  lanes: RemoteSupervisorLane[];
  now: number;
} {
  const machinesRef = useRef(machines);
  machinesRef.current = machines;
  const [settled, setSettled] = useState<Record<string, Settled>>({});
  const [now, setNow] = useState(() => Date.now());
  const key = machines
    .map((m) => `${m.machineId}\u0000${m.connectionId ?? ""}\u0000${m.capabilities == null ? "?" : hasSupervisorCapability(m.capabilities)}`)
    .join("\u0001");

  useEffect(() => {
    const cleanups: Array<() => void> = [];
    for (const machine of machinesRef.current) {
      const { machineId } = machine;
      const put = (next: Settled) => setSettled((prev) => ({ ...prev, [machineId]: next }));
      if (machine.capabilities != null && !hasSupervisorCapability(machine.capabilities)) {
        put({ state: "unsupported", snapshot: null, error: null });
        continue;
      }
      put({ state: "loading", snapshot: null, error: null });
      let alive = true;
      let inFlight = false;
      let again = false;
      // A change during a fetch queues exactly one follow-up fetch instead of racing a second.
      const refetch = async () => {
        if (inFlight) {
          again = true;
          return;
        }
        inFlight = true;
        do {
          again = false;
          const current = machinesRef.current.find((m) => m.machineId === machineId) ?? machine;
          const result = await fetchSupervisorSnapshot(current.fetchLike);
          if (!alive) return;
          put(settle(result));
        } while (again);
        inFlight = false;
      };
      const unsubscribe = machine.subscribe((frame) => {
        if (isSupervisorChangedFrame(frame) || isResyncFrame(frame)) void refetch();
      });
      void refetch();
      cleanups.push(() => {
        alive = false;
        unsubscribe();
      });
    }
    return () => {
      for (const cleanup of cleanups) cleanup();
    };
  }, [key]);

  const entries = useMemo(
    () =>
      machines.map((m): RemoteSupervisorEntry => ({
        machineId: m.machineId,
        label: m.label,
        ...(settled[m.machineId] ?? { state: "loading", snapshot: null, error: null }),
      })),
    [machines, settled],
  );

  useEffect(() => setNow(Date.now()), [settled]);
  useEffect(() => {
    let expiry: number | null = null;
    for (const entry of entries) {
      const next = entry.snapshot ? nextSupervisionExpiry(entry.snapshot, now) : null;
      if (next !== null && (expiry === null || next < expiry)) expiry = next;
    }
    if (expiry === null) return;
    const timer = setTimeout(() => setNow(Date.now()), expiry - now + 16);
    return () => clearTimeout(timer);
  }, [entries, now]);

  const lanes = useMemo(
    () => entries.flatMap((e) => (e.snapshot ? e.snapshot.lanes.map((lane) => ({ ...lane, machineId: e.machineId })) : [])),
    [entries],
  );
  return { machines: entries, lanes, now };
}
