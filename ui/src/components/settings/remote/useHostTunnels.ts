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
 * The tunnel definitions state of one settings session, shared by every SSH drawer.
 *
 * One snapshot per drawer rather than a poll: the tunnel list is small, the section is the
 * only reader, and a refresh runs after every mutation this hook performs so the rows never
 * show a definition the daemon refused.
 *
 * The capability arrives as a prop when the caller knows it (Todo 13 passes one so its
 * assertions are deterministic). When it is `undefined` nothing in the tree can answer it —
 * the daemon's capability vector is not exposed as a command — so this hook sends exactly
 * one probe through the tunnel bridge and reads `UNSUPPORTED_CAPABILITY` as absence. A known
 * empty list still mounts the explanation and sends nothing at all.
 */
export function useHostTunnels({ capabilities, active }: {
  capabilities?: readonly string[] | null;
  /** True while an SSH drawer is open; the capability probe never runs before that. */
  active: boolean;
}) {
  const known = capabilities !== undefined;
  const [probed, setProbed] = useState<readonly string[] | null>(null);
  const effective: readonly string[] | null = known ? capabilities ?? [] : probed;
  const enabled = hasSshTunnelCapability(effective);

  const [list, setList] = useState<TunnelListResponse | null>(null);
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

  const refresh = useCallback(() => {
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
        setList(response);
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
    if (!active || known || probeRef.current || probed !== null) {
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
        setList(response);
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
  }, [active, known, probed]);

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
  };
  return { editor, enabled };
}
