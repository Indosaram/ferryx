import { useCallback, useEffect, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";

import {
  hasSshTunnelCapability,
  hostRevisionFromList,
  listTunnels,
  resetHostTunnelStore,
  saveHostTunnels,
  SSH_TUNNEL_CAPABILITY,
  startTunnel,
  stopTunnel,
  toSshTunnelBridgeError,
  type SaveHostTunnelsOptions,
  type TunnelListResponse,
  type TunnelSpec,
} from "../../../lib/sshTunnels";
import {
  TUNNEL_REVISION_REQUIRED_REASON,
  TUNNEL_STORE_CONFLICT_REASON,
  type SshTunnelsEditorHandlers,
  type TunnelSaveConflict,
  type TunnelSectionError,
} from "./SshTunnelsEditor";

/** The `IpcErrorCode` Todo 8 answers with when the daemon never advertised `sshTunnelV1`. */
export const UNSUPPORTED_CAPABILITY_CODE = "UNSUPPORTED_CAPABILITY";

/**
 * A snapshot another owner already read, plus its refresh.
 *
 * The settings section reads the tunnel list once and hands the response down here, so the
 * drawer editor renders that snapshot instead of spending a second `cmd_ssh_tunnel_list`;
 * its own mutations re-read through `refresh` and stay on the same single reader.
 */
export interface HostTunnelsSource {
  /** The owner's newest response; `null` until it has one. */
  list: TunnelListResponse | null;
  /** What the owner's own read proved about `sshTunnelV1`. */
  capability: "present" | "absent" | "unknown";
  refresh: () => void;
}

/**
 * The tunnel definitions state of one settings session, shared by every SSH drawer.
 *
 * One snapshot per drawer rather than a poll: the tunnel list is small, the section is the
 * only reader, and a refresh runs after every mutation this hook performs so the rows never
 * show a definition the daemon refused.
 *
 * The capability arrives as a prop when the caller knows it (Todo 13 passes one so its
 * assertions are deterministic). When neither a capability nor a `source` is available,
 * nothing in the tree can answer it — the daemon's capability vector is not exposed as a
 * command — so this hook sends exactly one probe through the tunnel bridge and reads
 * `UNSUPPORTED_CAPABILITY` as absence. A known empty list still mounts the explanation and
 * sends nothing at all.
 *
 * With a `source` the owner is the single reader and also the answer: its verdict replaces the
 * probe, so a snapshot never costs a second `cmd_ssh_tunnel_list`. A verdict of `absent` — the
 * daemon refused the read it never advertised — closes the gate; `unknown` keeps the panel's
 * "unknown means allowed" default while the owner's first read is still in flight.
 */
export function useHostTunnels({ capabilities, active, source }: {
  capabilities?: readonly string[] | null;
  /** True while an SSH drawer is open; the capability probe never runs before that. */
  active: boolean;
  /**
   * A snapshot an owner already fetched. When present this hook reads nothing itself: the
   * editor renders `source.list` and re-reads through `source.refresh`. Without one it keeps
   * its own snapshot, its own refresh and its capability probe, exactly as before.
   */
  source?: HostTunnelsSource;
}) {
  const known = capabilities !== undefined;
  const sourced = source !== undefined;
  const [probed, setProbed] = useState<readonly string[] | null>(null);
  // A supplied snapshot is the answer too: the owner's read either served the command or was
  // refused because the daemon never advertised it. Probing here as well would spend the very
  // second `cmd_ssh_tunnel_list` this option exists to avoid, and only a refusal is evidence
  // against the feature — an unproven daemon keeps the default the panel itself uses.
  const effective: readonly string[] | null = known
    ? capabilities ?? []
    : sourced
      ? (source.capability === "absent" ? [] : [SSH_TUNNEL_CAPABILITY])
      : probed;
  const enabled = hasSshTunnelCapability(effective);

  const [ownList, setOwnList] = useState<TunnelListResponse | null>(null);
  // The owner's response wins when one was supplied; there is never a second copy to reconcile.
  const list = source ? source.list : ownList;
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<TunnelSectionError | null>(null);
  const [resetPath, setResetPath] = useState<string | null>(null);
  const [conflict, setConflict] = useState<TunnelSaveConflict | null>(null);

  // The newest response wins; a mutation that lands while a read is in flight discards it.
  const requestRef = useRef(0);
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const refreshOwn = useCallback(() => {
    // Nothing is requested while the capability is absent: an older daemon must see no tunnel
    // command at all, not just no write.
    if (!enabled || !isTauri()) {
      return;
    }
    const requestId = ++requestRef.current;
    setLoading(true);
    listTunnels()
      .then((response) => {
        if (!mountedRef.current || requestId !== requestRef.current) return;
        setOwnList(response);
        setError(null);
      })
      .catch((failure: unknown) => {
        if (!mountedRef.current || requestId !== requestRef.current) return;
        const bridge = toSshTunnelBridgeError(failure, "Failed to read SSH tunnels.");
        setError({ code: bridge.code, reason: bridge.reason, message: bridge.message });
      })
      .finally(() => {
        if (!mountedRef.current || requestId !== requestRef.current) return;
        setLoading(false);
      });
  }, [enabled]);

  // The capability probe: one list call, only when the caller could not answer.
  const probeRef = useRef(false);
  useEffect(() => {
    if (sourced || !active || known || probeRef.current || probed !== null) {
      return;
    }
    probeRef.current = true;
    if (!isTauri()) {
      // A browser build has no daemon socket at all, so there is nothing to probe.
      setProbed([]);
      return;
    }
    listTunnels()
      .then((response) => {
        if (!mountedRef.current) return;
        // A stub that answers nothing is not a tunnel-capable daemon: refuse to mount the body
        // rather than render rows over a missing response.
        if (!response || typeof response !== "object") {
          setProbed([]);
          return;
        }
        // The daemon served the command, so it advertises the capability; keep the snapshot.
        setProbed([SSH_TUNNEL_CAPABILITY]);
        setOwnList(response);
      })
      .catch((failure: unknown) => {
        if (!mountedRef.current) return;
        const bridge = toSshTunnelBridgeError(failure, "Failed to read SSH tunnels.");
        if (bridge.code === UNSUPPORTED_CAPABILITY_CODE) {
          setProbed([]);
          return;
        }
        // A daemon that advertises but is failing still owns the section: show the typed error
        // rather than pretending the feature is missing.
        setProbed([SSH_TUNNEL_CAPABILITY]);
        setError({ code: bridge.code, reason: bridge.reason, message: bridge.message });
      });
  }, [active, known, probed, sourced]);

  // A supplied snapshot is already the newest one, so a read of our own would only be a
  // discarded duplicate: mutations re-read through the owner instead.
  const refresh = sourced ? source.refresh : refreshOwn;

  const onSave = useCallback(
    (hostId: string, specs: readonly TunnelSpec[], options?: SaveHostTunnelsOptions) => {
      const expectedRevision = options?.force === true ? null : list?.definitionsRevision ?? null;
      setConflict(null);
      saveHostTunnels(hostId, specs, expectedRevision, options)
        .then(() => {
          if (!mountedRef.current) return;
          setError(null);
          refresh();
        })
        .catch((failure: unknown) => {
          if (!mountedRef.current) return;
          const bridge = toSshTunnelBridgeError(failure, "Failed to save the host's SSH tunnels.");
          // A refused compare-and-swap is the one error the operator can answer: keep the
          // typed reason and offer the overwrite, never a generic message.
          if (bridge.reason === TUNNEL_STORE_CONFLICT_REASON || bridge.reason === TUNNEL_REVISION_REQUIRED_REASON) {
            setConflict({ reason: bridge.reason, message: bridge.message, expectedRevision });
            return;
          }
          setError({ code: bridge.code, reason: bridge.reason, message: bridge.message });
        });
    },
    [list, refresh],
  );

  const onStart = useCallback(
    (hostId: string, specId: string) => {
      // The revision is the value this snapshot read; the daemon answers `host_revision_stale`
      // when the host changed underneath, which is a refresh, not a start.
      const hostRevision = list ? hostRevisionFromList(list, hostId) : 0;
      startTunnel({ hostId, specId, hostRevision })
        .then(() => {
          if (!mountedRef.current) return;
          setError(null);
          refresh();
        })
        .catch((failure: unknown) => {
          if (!mountedRef.current) return;
          const bridge = toSshTunnelBridgeError(failure, "Failed to start the SSH tunnel.");
          // The typed tunnel classification travels in `details.failureCode`; show it rather
          // than the generic IPC message when the daemon sent one.
          setError({
            code: bridge.code,
            reason: bridge.reason,
            message: bridge.failureCode ? `${bridge.failureCode}: ${bridge.message}` : bridge.message,
          });
          refresh();
        });
    },
    [list, refresh],
  );

  const onStop = useCallback(
    (runtimeId: string) => {
      stopTunnel(runtimeId)
        .then(() => {
          if (!mountedRef.current) return;
          setError(null);
          refresh();
        })
        .catch((failure: unknown) => {
          if (!mountedRef.current) return;
          const bridge = toSshTunnelBridgeError(failure, "Failed to stop the SSH tunnel.");
          setError({ code: bridge.code, reason: bridge.reason, message: bridge.message });
        });
    },
    [refresh],
  );

  const onReset = useCallback(() => {
    resetHostTunnelStore()
      .then((response) => {
        if (!mountedRef.current) return;
        setResetPath(response.quarantinedPath);
        setError(null);
        refresh();
      })
      .catch((failure: unknown) => {
        if (!mountedRef.current) return;
        const bridge = toSshTunnelBridgeError(failure, "Failed to reset the SSH tunnel store.");
        setError({ code: bridge.code, reason: bridge.reason, message: bridge.message });
      });
  }, [refresh]);

  const editor: SshTunnelsEditorHandlers = {
    capabilities: effective,
    list,
    loading,
    error,
    resetPath,
    conflict,
    onRefresh: refresh,
    onSave,
    onStart,
    onStop,
    onReset,
    // A sourced hook's snapshot belongs to the section, which reads it on mount; the editor
    // opening on that snapshot must not ask for it a second time.
    autoReadOnMount: !sourced,
  };
  return { editor, enabled };
}
