/**
 * Remote Browser Driver Lifecycle Hook (§4.4, §6.2, Phase 6)
 *
 * Distinguishes driver states: viewing, claiming, driving, occupied, revoked.
 * Enforces explicit claim ONLY (NO auto-claim on mount or connect).
 * Maintains 15s TTL heartbeat loop (5s interval) while driving.
 * Dispatches commands with lease epoch and browser identity guards.
 * Mutation replay prevention: NO auto-replay on reconnect or ticket renewal.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import type { BrowserClient } from "./browserClient";
import type { BrowserDriverChangedMessage, BrowserErrorMessage } from "./browserProtocol";

export type RemoteDriverState =
  | "viewing"
  | "claiming"
  | "driving"
  | "occupied"
  | "revoked";

export interface UseRemoteBrowserDriverOptions {
  client: BrowserClient | null;
  browserId: string | null;
  subscriptionId?: string | null;
  browserInstanceId?: string | null;
  desktopEpoch?: string | null;
  documentGeneration?: string | null;
}

export interface UseRemoteBrowserDriverResult {
  driverState: RemoteDriverState;
  leaseEpoch: string | null;
  expiresAt: number | null;
  error: Error | null;
  claim: () => Promise<void>;
  release: () => Promise<void>;
  navigate: (url: string) => Promise<unknown>;
  back: () => Promise<unknown>;
  forward: () => Promise<unknown>;
  reload: () => Promise<unknown>;
  click: (params: { reference?: string; u?: number; v?: number; snapshotId?: string }) => Promise<unknown>;
  fill: (reference: string, value: string, snapshotId?: string) => Promise<unknown>;
  keypress: (key: string) => Promise<unknown>;
  wait: (params: { condition?: string; timeoutMs?: number }) => Promise<unknown>;
  evalJs: (script: string) => Promise<unknown>;
}

export function useRemoteBrowserDriver({
  client,
  browserId,
  subscriptionId,
  browserInstanceId,
  desktopEpoch,
  documentGeneration,
}: UseRemoteBrowserDriverOptions): UseRemoteBrowserDriverResult {
  // Explicit claim ONLY: initial state is always 'viewing'
  const [driverState, setDriverState] = useState<RemoteDriverState>("viewing");
  const [leaseEpoch, setLeaseEpoch] = useState<string | null>(null);
  const [expiresAt, setExpiresAt] = useState<number | null>(null);
  const [error, setError] = useState<Error | null>(null);

  // Keep latest guards in ref for stable callbacks
  const guardsRef = useRef({
    browserId,
    browserInstanceId,
    desktopEpoch,
    documentGeneration,
    leaseEpoch,
    driverState,
    client,
  });

  guardsRef.current = {
    browserId,
    browserInstanceId,
    desktopEpoch,
    documentGeneration,
    leaseEpoch,
    driverState,
    client,
  };

  // Listen to client driver state changes & errors
  useEffect(() => {
    if (!client) {
      setDriverState("viewing");
      setLeaseEpoch(null);
      setExpiresAt(null);
      return;
    }

    const unsubDriver = client.onDriverChanged((msg: BrowserDriverChangedMessage) => {
      if (msg.isDriver && msg.leaseEpoch) {
        setDriverState("driving");
        setLeaseEpoch(msg.leaseEpoch);
        setExpiresAt(Date.now() + 15000);
      } else if (!msg.isDriver) {
        setDriverState((prev) => {
          if (prev === "driving") {
            return "revoked";
          }
          return prev === "claiming" ? "viewing" : prev;
        });
        setLeaseEpoch(null);
        setExpiresAt(null);
      }
    });

    const unsubError = client.onError((err: Error | BrowserErrorMessage) => {
      const code = "code" in err ? err.code : undefined;
      if (code === "BROWSER_DRIVER_BUSY") {
        setDriverState("occupied");
      } else if (code === "DESKTOP_RECLAIMED" || code === "BROWSER_DRIVER_REVOKED") {
        setDriverState("revoked");
        setLeaseEpoch(null);
        setExpiresAt(null);
      }
    });

    const unsubClose = client.onClose(() => {
      // On reconnect/close, reset driver lease immediately. Never auto-replay pending commands.
      setDriverState("viewing");
      setLeaseEpoch(null);
      setExpiresAt(null);
    });

    return () => {
      unsubDriver();
      unsubError();
      unsubClose();
    };
  }, [client]);

  // Heartbeat loop: 15s TTL, emit heartbeat every 5s while in 'driving' state
  useEffect(() => {
    if (driverState !== "driving" || !client || !leaseEpoch) {
      return;
    }

    const intervalId = setInterval(async () => {
      try {
        await client.heartbeat(leaseEpoch);
        setExpiresAt(Date.now() + 15000);
      } catch (err) {
        // Lease heartbeat failed or expired: gracefully return to viewing
        setDriverState("viewing");
        setLeaseEpoch(null);
        setExpiresAt(null);
        setError(err instanceof Error ? err : new Error(String(err)));
      }
    }, 5000);

    return () => {
      clearInterval(intervalId);
    };
  }, [driverState, client, leaseEpoch]);

  const claim = useCallback(async () => {
    if (!client || !browserId) {
      throw new Error("Cannot claim driver lease: client or browserId missing");
    }

    setDriverState("claiming");
    setError(null);

    try {
      const effectiveSubId =
        subscriptionId ||
        (client as unknown as { currentSubscription?: { subscriptionId: string } })
          .currentSubscription?.subscriptionId ||
        "sub-default";

      const claimed = await client.claimDriver(browserId, effectiveSubId);
      setLeaseEpoch(claimed.leaseEpoch);
      setExpiresAt(Date.now() + 15000);
      setDriverState("driving");
    } catch (err: unknown) {
      const code = (err as { code?: string })?.code;
      if (code === "BROWSER_DRIVER_BUSY") {
        setDriverState("occupied");
      } else if (code === "DESKTOP_RECLAIMED") {
        setDriverState("revoked");
      } else {
        setDriverState("viewing");
      }
      const errorObj = err instanceof Error ? err : new Error(String(err));
      setError(errorObj);
      throw errorObj;
    }
  }, [client, browserId, subscriptionId]);

  const release = useCallback(async () => {
    if (!client || !leaseEpoch) {
      setDriverState("viewing");
      setLeaseEpoch(null);
      setExpiresAt(null);
      return;
    }

    const epochToRelease = leaseEpoch;
    try {
      await client.releaseDriver(epochToRelease);
    } catch {
      // Ignored during release teardown
    } finally {
      setDriverState("viewing");
      setLeaseEpoch(null);
      setExpiresAt(null);
    }
  }, [client, leaseEpoch]);

  // Command dispatch with driving lease epoch and browser identity guards
  const dispatchCommand = useCallback(
    async (command: string, params?: Record<string, unknown>): Promise<unknown> => {
      const {
        client: curClient,
        browserId: curBrowserId,
        browserInstanceId: curInstanceId,
        desktopEpoch: curDesktopEpoch,
        documentGeneration: curDocGen,
        leaseEpoch: curLeaseEpoch,
        driverState: curDriverState,
      } = guardsRef.current;

      if (curDriverState !== "driving" || !curLeaseEpoch) {
        throw new Error(
          `Cannot execute command "${command}": not in driving state (current: ${curDriverState})`
        );
      }
      if (!curClient) {
        throw new Error(`Cannot execute command "${command}": WebSocket client is not connected`);
      }
      if (!curBrowserId) {
        throw new Error(`Cannot execute command "${command}": browserId guard missing`);
      }
      if (!curInstanceId) {
        throw new Error(`Cannot execute command "${command}": browserInstanceId guard missing`);
      }
      if (!curDesktopEpoch) {
        throw new Error(`Cannot execute command "${command}": desktopEpoch guard missing`);
      }
      if (!curDocGen) {
        throw new Error(`Cannot execute command "${command}": documentGeneration guard missing`);
      }

      // Single-shot command send: mutation replay prevention ensures dropped commands are not replayed
      return curClient.sendCommand({
        browserId: curBrowserId,
        leaseEpoch: curLeaseEpoch,
        browserInstanceId: curInstanceId,
        desktopEpoch: curDesktopEpoch,
        documentGeneration: curDocGen,
        command,
        params,
      });
    },
    []
  );

  const navigate = useCallback(
    (url: string) => dispatchCommand("navigate", { url }),
    [dispatchCommand]
  );

  const back = useCallback(() => dispatchCommand("back"), [dispatchCommand]);
  const forward = useCallback(() => dispatchCommand("forward"), [dispatchCommand]);
  const reload = useCallback(() => dispatchCommand("reload"), [dispatchCommand]);

  const click = useCallback(
    (params: { reference?: string; u?: number; v?: number; snapshotId?: string }) =>
      dispatchCommand("click", params as Record<string, unknown>),
    [dispatchCommand]
  );

  const fill = useCallback(
    (reference: string, value: string, snapshotId?: string) =>
      dispatchCommand("fill", {
        reference,
        value,
        ...(snapshotId ? { snapshotId } : {}),
      }),
    [dispatchCommand]
  );

  const keypress = useCallback(
    (key: string) => dispatchCommand("keypress", { key }),
    [dispatchCommand]
  );

  const wait = useCallback(
    (params: { condition?: string; timeoutMs?: number }) =>
      dispatchCommand("wait", params as Record<string, unknown>),
    [dispatchCommand]
  );

  const evalJs = useCallback(
    (script: string) => dispatchCommand("eval", { script }),
    [dispatchCommand]
  );

  return {
    driverState,
    leaseEpoch,
    expiresAt,
    error,
    claim,
    release,
    navigate,
    back,
    forward,
    reload,
    click,
    fill,
    keypress,
    wait,
    evalJs,
  };
}
