/**
 * Snapshot ownership for the SSH tunnels panel.
 *
 * V1 has no tunnel event stream, so this hook is the only source of truth while the panel is
 * open: one `listTunnels()` in flight at a time, refreshed on open, on window focus, and every
 * `TUNNEL_REFRESH_MS` — and only while a caller has it mounted. A failed refresh keeps the last
 * good snapshot (a never-answered panel must not look green), which the stale threshold then
 * downgrades on its own.
 */
import { useCallback, useEffect, useRef, useState } from "react";

import {
  hostRevisionFromList,
  listTunnels,
  startTunnel,
  stopTunnel,
  type AdHocTunnelSpec,
  type TunnelListResponse,
  type TunnelView,
} from "../../../lib/sshTunnels";
import { TUNNEL_REFRESH_MS, viewKey, type TunnelRow } from "./tunnelPresentation";

/**
 * What a tunnel read proved about the daemon. `unknown` before any read, `present` once the
 * daemon answered, `absent` when it refused the command it never advertised.
 */
export type TunnelCapability = "present" | "absent" | "unknown";

export interface UseSshTunnelsOptions {
  /** Injected clock: the refresh schedule and the stale threshold both read it. */
  clock?: () => number;
  refreshMs?: number;
  /** Capability gate (MH-10): without it nothing is invoked at all. */
  enabled?: boolean;
}

export interface TunnelSnapshot {
  /** The newest successful `listTunnels()` response, or null before the first one. */
  list: TunnelListResponse | null;
  /** Views that were live once and are absent from the newest response: rendered as `stopped`. */
  retired: readonly TunnelView[];
  /** When the newest successful response arrived, per the injected clock. */
  receivedAt: number | null;
  error: string | null;
  refreshing: boolean;
  /**
   * What the read proved about `sshTunnelV1`: `present` once a response landed, `absent` when
   * the daemon refused the command because it never advertised it, `unknown` before any read.
   * A caller that hands this snapshot to the drawer editor answers its gate from here instead
   * of spending a probe of its own.
   */
  capability: TunnelCapability;
}

export interface UseSshTunnelsResult extends TunnelSnapshot {
  refresh: () => Promise<void>;
  /** Restore a failed row: by `specId` when it is saved, by its spec when it is ad-hoc. */
  startSaved: (row: TunnelRow) => Promise<void>;
  /** Forward a port now: an ad-hoc start the daemon mints an id for. */
  startAdHoc: (hostId: string, spec: AdHocTunnelSpec) => Promise<void>;
  stop: (row: TunnelRow) => Promise<void>;
  /**
   * Hide a stopped row for the rest of the session: the local retired history drops it and a
   * later `refresh()` never resurrects it. Nothing is deleted from the daemon or the host
   * store — the row simply stops being reported as a stopped tunnel.
   */
  dismissStopped: (row: TunnelRow) => void;
}

const EMPTY: TunnelSnapshot = {
  list: null,
  retired: [],
  receivedAt: null,
  error: null,
  refreshing: false,
  capability: "unknown",
};

export function useSshTunnels(options: UseSshTunnelsOptions = {}): UseSshTunnelsResult {
  const { enabled = true, refreshMs = TUNNEL_REFRESH_MS } = options;
  const clock = options.clock;
  const clockRef = useRef(clock);

  const [snapshot, setSnapshot] = useState<TunnelSnapshot>(EMPTY);
  const inFlightRef = useRef(false);
  const mountedRef = useRef(true);
  const snapshotRef = useRef(snapshot);
  snapshotRef.current = snapshot;

  // Keys whose stopped row was dismissed or superseded on purpose: the next `refresh()` must
  // not resurrect them. The ref is what the async read consults; the snapshot carries the
  // visible half so the row leaves the list on the same render.
  const dismissedKeysRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    clockRef.current = clock;
  });

  const refresh = useCallback(async () => {
    // One in-flight request only: a tick that lands mid-flight is dropped, never queued.
    if (!enabled || inFlightRef.current) return;
    inFlightRef.current = true;
    setSnapshot((prev) => (prev.refreshing ? prev : { ...prev, refreshing: true }));
    try {
      const list = normalizeList(await listTunnels());
      if (!mountedRef.current) return;
      const retired = retireMissingViews(list, snapshotRef.current, dismissedKeysRef.current);
      setSnapshot({
        list,
        retired,
        receivedAt: now(clockRef.current),
        error: null,
        refreshing: false,
        capability: "present",
      });
    } catch (error) {
      if (!mountedRef.current) return;
      // The last good snapshot stays; `receivedAt` is untouched so the stale marker appears.
      // A refusal because the daemon never advertised the command is not a transient failure:
      // it settles the capability, so the editor explains itself instead of showing an error.
      setSnapshot((prev) => ({
        ...prev,
        refreshing: false,
        error: errorMessage(error),
        capability: isUnsupported(error) ? "absent" : prev.capability,
      }));
    } finally {
      inFlightRef.current = false;
    }
  }, [enabled]);

  useEffect(() => {
    mountedRef.current = true;
    if (!enabled) return undefined;
    void refresh();
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    const timer = setInterval(() => void refresh(), refreshMs);
    return () => {
      mountedRef.current = false;
      window.removeEventListener("focus", onFocus);
      clearInterval(timer);
    };
  }, [enabled, refresh, refreshMs]);

  const hostRevisionFor = useCallback(
    (hostId: string): number => (snapshotRef.current.list ? hostRevisionFromList(snapshotRef.current.list, hostId) : 0),
    [],
  );

  const forgetRetired = useCallback((key: string) => {
    dismissedKeysRef.current.add(key);
    setSnapshot((prev) =>
      prev.retired.some((view) => viewKey(view) === key)
        ? { ...prev, retired: prev.retired.filter((view) => viewKey(view) !== key) }
        : prev,
    );
  }, []);

  const dismissStopped = useCallback((row: TunnelRow) => {
    if (row.state !== "stopped" || row.saved) return;
    // Only a row the local history invented can be dismissed. A stopped row the daemon itself
    // reported has no local entry to forget, and hiding it would contradict the list it came from.
    if (!snapshotRef.current.retired.some((view) => viewKey(view) === row.key)) return;
    forgetRetired(row.key);
  }, [forgetRetired]);

  // Every mutation the panel performs re-reads the list; it never patches a row locally.
  const startSaved = useCallback(async (row: TunnelRow) => {
    const hostRevision = hostRevisionFor(row.hostId);
    try {
      if (row.saved) {
        await startTunnel({ hostId: row.hostId, specId: row.specId, hostRevision });
      } else {
        await startTunnel({ hostId: row.hostId, spec: adHocSpecFromRow(row), hostRevision });
      }
      // Only an accepted restart supersedes the old view: the ad-hoc start mints a fresh spec
      // id, so the previous one can only return as a stopped row beside the new runtime. A
      // refused start keeps it, so the operator still has the row to act on again.
      if (!row.saved) forgetRetired(row.key);
    } finally {
      await refresh();
    }
  }, [forgetRetired, hostRevisionFor, refresh]);

  const startAdHoc = useCallback(async (hostId: string, spec: AdHocTunnelSpec) => {
    const hostRevision = hostRevisionFor(hostId);
    try {
      await startTunnel({ hostId, spec, hostRevision });
    } finally {
      await refresh();
    }
  }, [hostRevisionFor, refresh]);

  const stop = useCallback(async (row: TunnelRow) => {
    if (row.runtimeId === null) return;
    try {
      await stopTunnel(row.runtimeId);
    } finally {
      await refresh();
    }
  }, [refresh]);

  return { ...snapshot, refresh, startSaved, startAdHoc, stop, dismissStopped };
}

/** The injected clock when a test supplies one, the wall clock otherwise. */
function now(clock?: () => number): number {
  return clock ? clock() : Date.now();
}

/**
 * The retired set for the next snapshot: a view the newest response no longer reports was live
 * once, so it renders as `stopped` with its last known ports instead of vanishing. A row that
 * comes back in a later response leaves the retired set, because the response is canonical;
 * a key the operator dismissed never enters it at all.
 */
function retireMissingViews(
  next: TunnelListResponse,
  previous: TunnelSnapshot,
  dismissed: ReadonlySet<string>,
): TunnelView[] {
  const liveKeys = new Set(next.views.map(viewKey));
  const liveRuntimeIds = new Set(
    next.views.map((view) => view.runtimeId).filter((id): id is string => id !== null),
  );
  const candidates: TunnelView[] = [...previous.retired, ...(previous.list?.views ?? [])];
  const retired = new Map<string, TunnelView>();
  for (const view of candidates) {
    const key = viewKey(view);
    if (dismissed.has(key)) continue;
    if (liveKeys.has(key)) continue;
    if (view.runtimeId !== null && liveRuntimeIds.has(view.runtimeId)) continue;
    retired.set(key, view);
  }
  return [...retired.values()];
}

/**
 * A response that is not the documented shape reads as an empty list: an older daemon, or a
 * bridge mock in another suite, must not be able to crash the panel or paint a green row.
 */
function normalizeList(raw: unknown): TunnelListResponse {
  const record = (typeof raw === "object" && raw !== null ? raw : {}) as Partial<TunnelListResponse>;
  return {
    views: Array.isArray(record.views) ? record.views : [],
    definitions: Array.isArray(record.definitions) ? record.definitions : [],
    hostRevisions: Array.isArray(record.hostRevisions) ? record.hostRevisions : [],
    definitionsRevision: typeof record.definitionsRevision === "number" ? record.definitionsRevision : 0,
    storeStatus: record.storeStatus ?? { kind: "ok", path: "", invalidHosts: [], quarantinedPath: null },
  };
}

/**
 * The ad-hoc payload for restoring a runtime the daemon never saved. `id` is omitted on purpose:
 * the daemon mints one per runtime, so sending the minted id back would look like a saved spec.
 *
 * The port is the one the runtime reported: an explicit port stays stable across a restart.
 * `0` is not a legal wire value (`validate_spec` rejects it), so an absent port — never `0` —
 * is how a restart asks the daemon to allocate one.
 */
function adHocSpecFromRow(row: TunnelRow): AdHocTunnelSpec {
  return {
    label: row.label,
    localPort: row.localPort > 0 ? row.localPort : undefined,
    remoteHost: row.remoteHost,
    remotePort: row.remotePort,
    enabled: true,
  };
}

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim() !== "") return error.message;
  if (typeof error === "string" && error.trim() !== "") return error;
  const message = (error as { message?: unknown } | null)?.message;
  return typeof message === "string" && message.trim() !== "" ? message : "Could not read SSH tunnels.";
}

/**
 * Whether the read was refused because the daemon never advertised `cmd_ssh_tunnel_list`.
 * `toSshTunnelBridgeError` carries the `IpcErrorCode`, so an older daemon's refusal is
 * distinguishable from a transient failure of a capable one.
 */
function isUnsupported(error: unknown): boolean {
  const code = (error as { code?: unknown } | null)?.code;
  return code === "UNSUPPORTED_CAPABILITY";
}
