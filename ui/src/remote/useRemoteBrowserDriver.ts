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
import {
  reconcileMapRevision,
  type BrowserDriverChangedMessage,
  type BrowserErrorMessage,
  type BrowserSnapshotServerMessage,
  type ServerBrowserDriverRevoked,
} from "./browserProtocol";

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
  viewportRevision?: string | null;
  snapshotId?: string | null;
  mapRevision?: string | null;
}

export interface RemoteBrowserMutationGuards {
  documentGeneration?: string;
  viewportRevision?: string;
}

export interface RemoteBrowserClickParams extends RemoteBrowserMutationGuards {
  reference?: string;
  selector?: string;
  u?: number;
  v?: number;
  snapshotId?: string;
  mapRevision?: string;
  streamId?: number;
  sequenceNumber?: number;
  browserInstanceId?: string;
  [key: string]: unknown;
}

export interface RemoteBrowserFillOptions extends RemoteBrowserMutationGuards {
  snapshotId?: string;
  mapRevision?: string;
}

export interface UseRemoteBrowserDriverResult {
  driverState: RemoteDriverState;
  leaseEpoch: string | null;
  expiresAt: number | null;
  error: Error | null;
  snapshotId: string | null;
  mapRevision: string | null;
  claim: () => Promise<void>;
  release: () => Promise<void>;
  takeSnapshot: () => Promise<BrowserSnapshotServerMessage>;
  navigate: (url: string, guards?: RemoteBrowserMutationGuards) => Promise<unknown>;
  back: (guards?: RemoteBrowserMutationGuards) => Promise<unknown>;
  forward: (guards?: RemoteBrowserMutationGuards) => Promise<unknown>;
  reload: (guards?: RemoteBrowserMutationGuards) => Promise<unknown>;
  click: (
    selectorOrParams: string | RemoteBrowserClickParams,
    options?: RemoteBrowserClickParams
  ) => Promise<unknown>;
  fill: (
    selector: string,
    text: string,
    snapshotIdOrOptions?: string | RemoteBrowserFillOptions,
    guards?: RemoteBrowserMutationGuards
  ) => Promise<unknown>;
  keypress: (key: string, guards?: RemoteBrowserMutationGuards) => Promise<unknown>;
  wait: (params: { condition?: string; timeoutMs?: number }, guards?: RemoteBrowserMutationGuards) => Promise<unknown>;
  evalJs: (script: string, guards?: RemoteBrowserMutationGuards) => Promise<unknown>;
}

function needsSnapshotAcquisition(target: string | undefined): boolean {
  if (!target) return false;
  const trimmed = target.trim();
  if (trimmed === "" || trimmed === "active") return false;
  if (trimmed.startsWith("#") || trimmed.startsWith(".") || trimmed.startsWith("[")) return false;
  return true;
}

export function useRemoteBrowserDriver({
  client,
  browserId,
  subscriptionId,
  browserInstanceId,
  desktopEpoch,
  documentGeneration,
  viewportRevision,
  snapshotId,
  mapRevision,
}: UseRemoteBrowserDriverOptions): UseRemoteBrowserDriverResult {
  // Explicit claim ONLY: initial state is always 'viewing'
  const [driverState, setDriverState] = useState<RemoteDriverState>("viewing");
  const [leaseEpoch, setLeaseEpoch] = useState<string | null>(null);
  const [expiresAt, setExpiresAt] = useState<number | null>(null);
  const [error, setError] = useState<Error | null>(null);
  const [driverSnapshotId, setDriverSnapshotId] = useState<string | null>(snapshotId ?? null);
  const [driverMapRevision, setDriverMapRevision] = useState<string | null>(
    reconcileMapRevision(mapRevision) ?? null
  );

  useEffect(() => {
    if (snapshotId !== undefined) {
      setDriverSnapshotId(snapshotId ?? null);
    }
  }, [snapshotId]);

  useEffect(() => {
    if (mapRevision !== undefined) {
      setDriverMapRevision(reconcileMapRevision(mapRevision) ?? null);
    }
  }, [mapRevision]);

  // Keep latest guards in ref for stable callbacks
  const guardsRef = useRef({
    browserId,
    browserInstanceId,
    desktopEpoch,
    documentGeneration,
    viewportRevision,
    snapshotId: driverSnapshotId ?? snapshotId,
    mapRevision: driverMapRevision ?? reconcileMapRevision(mapRevision),
    leaseEpoch,
    driverState,
    client,
  });

  guardsRef.current = {
    browserId,
    browserInstanceId,
    desktopEpoch,
    documentGeneration,
    viewportRevision,
    snapshotId: driverSnapshotId ?? snapshotId,
    mapRevision: driverMapRevision ?? reconcileMapRevision(mapRevision),
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

    const unsubRevoked = client.onDriverRevoked
      ? client.onDriverRevoked((_msg: ServerBrowserDriverRevoked) => {
          setDriverState("revoked");
          setLeaseEpoch(null);
          setExpiresAt(null);
        })
      : typeof (client as any).on === "function"
      ? (client as any).on("driverRevoked", (_msg: any) => {
          setDriverState("revoked");
          setLeaseEpoch(null);
          setExpiresAt(null);
        })
      : () => {};

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
      unsubRevoked();
      unsubError();
      unsubClose();
    };
  }, [client]);

  const heartbeatTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Heartbeat loop: 15s TTL, emit heartbeat every 5s while in 'driving' state
  useEffect(() => {
    if (driverState !== "driving" || !client || !leaseEpoch) {
      if (heartbeatTimerRef.current) {
        clearInterval(heartbeatTimerRef.current);
        heartbeatTimerRef.current = null;
      }
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
    heartbeatTimerRef.current = intervalId;

    return () => {
      clearInterval(intervalId);
      heartbeatTimerRef.current = null;
    };
  }, [driverState, client, leaseEpoch]);

  // Unmount effect: clear heartbeat timers and release driving lease
  useEffect(() => {
    return () => {
      if (heartbeatTimerRef.current) {
        clearInterval(heartbeatTimerRef.current);
        heartbeatTimerRef.current = null;
      }
      const { client: curClient, leaseEpoch: curEpoch, driverState: curState } = guardsRef.current;
      if (curClient && curEpoch && curState === "driving") {
        void curClient.releaseDriver(curEpoch).catch(() => {});
      }
    };
  }, []);

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

  const takeSnapshot = useCallback(async (): Promise<BrowserSnapshotServerMessage> => {
    const { client: curClient, browserId: curBrowserId } = guardsRef.current;
    if (!curClient || !curBrowserId) {
      throw new Error("Cannot take snapshot: client or browserId missing");
    }
    const snap = await curClient.takeSnapshot(curBrowserId);
    const reconciled = reconcileMapRevision(snap.mapRevision) ?? snap.mapRevision;
    setDriverSnapshotId(snap.snapshotId);
    setDriverMapRevision(reconciled);
    guardsRef.current.snapshotId = snap.snapshotId;
    guardsRef.current.mapRevision = reconciled;
    return {
      ...snap,
      mapRevision: reconciled,
    };
  }, []);

  // Command dispatch with driving lease epoch and browser identity guards
  const dispatchCommand = useCallback(
    async (
      command: string,
      params?: Record<string, unknown>,
      explicitGuards?: RemoteBrowserMutationGuards
    ): Promise<unknown> => {
      const {
        client: curClient,
        browserId: curBrowserId,
        browserInstanceId: curInstanceId,
        desktopEpoch: curDesktopEpoch,
        documentGeneration: curDocGen,
        viewportRevision: curViewportRev,
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

      // Check documentGeneration guard
      const targetDocGen =
        explicitGuards?.documentGeneration ??
        (params?.documentGeneration as string | undefined) ??
        curDocGen;

      if (!targetDocGen) {
        throw new Error(`Cannot execute command "${command}": documentGeneration guard missing`);
      }
      if (curDocGen && targetDocGen !== curDocGen) {
        throw new Error(
          `Cannot execute command "${command}": stale documentGeneration (expected ${curDocGen}, got ${targetDocGen})`
        );
      }

      // Check viewportRevision guard
      const targetViewportRev =
        explicitGuards?.viewportRevision ??
        (params?.viewportRevision as string | undefined);

      if (curViewportRev && targetViewportRev && targetViewportRev !== curViewportRev) {
        throw new Error(
          `Cannot execute command "${command}": stale viewportRevision (expected ${curViewportRev}, got ${targetViewportRev})`
        );
      }

      // Single-shot command send: mutation replay prevention ensures dropped commands are not replayed
      return curClient.sendCommand({
        browserId: curBrowserId,
        leaseEpoch: curLeaseEpoch,
        browserInstanceId: curInstanceId,
        desktopEpoch: curDesktopEpoch,
        documentGeneration: targetDocGen,
        command,
        params,
      });
    },
    []
  );

  const navigate = useCallback(
    (url: string, guards?: RemoteBrowserMutationGuards) =>
      dispatchCommand("navigate", { url }, guards),
    [dispatchCommand]
  );

  const back = useCallback(
    (guards?: RemoteBrowserMutationGuards) =>
      dispatchCommand("back", undefined, guards),
    [dispatchCommand]
  );
  const forward = useCallback(
    (guards?: RemoteBrowserMutationGuards) =>
      dispatchCommand("forward", undefined, guards),
    [dispatchCommand]
  );
  const reload = useCallback(
    (guards?: RemoteBrowserMutationGuards) =>
      dispatchCommand("reload", undefined, guards),
    [dispatchCommand]
  );

  const click = useCallback(
    async (
      selectorOrParams: string | RemoteBrowserClickParams,
      options?: RemoteBrowserClickParams
    ) => {
      let params: Record<string, unknown>;
      let guards: RemoteBrowserMutationGuards | undefined;

      const isPointClick =
        typeof selectorOrParams === "object" &&
        typeof selectorOrParams.u === "number" &&
        typeof selectorOrParams.v === "number";

      if (typeof selectorOrParams === "string") {
        params = {
          selector: selectorOrParams,
          reference: selectorOrParams,
          ...options,
        };
        guards = options;
      } else {
        params = { ...selectorOrParams };
        if (selectorOrParams.reference && !params.selector) {
          params.selector = selectorOrParams.reference;
        }
        guards = {
          documentGeneration: selectorOrParams.documentGeneration,
          viewportRevision: selectorOrParams.viewportRevision,
        };
      }

      let effectiveSnapshotId =
        (params.snapshotId as string | undefined) ?? guardsRef.current.snapshotId ?? undefined;
      let effectiveMapRevision =
        reconcileMapRevision(params.mapRevision) ?? guardsRef.current.mapRevision ?? undefined;

      const target = (params.reference as string | undefined) ?? (params.selector as string | undefined);
      const needsSnapshot = Boolean(params.needsSnapshot || params.acquireSnapshot || needsSnapshotAcquisition(target));

      // Acquire valid remote snapshot before click when needed (for snapshot element references)
      if (!isPointClick && needsSnapshot && (!effectiveSnapshotId || !effectiveMapRevision)) {
        const { client: curClient, browserId: curBrowserId } = guardsRef.current;
        if (curClient && curBrowserId && typeof curClient.takeSnapshot === "function") {
          try {
            const snap = await curClient.takeSnapshot(curBrowserId);
            effectiveSnapshotId = snap.snapshotId;
            effectiveMapRevision = reconcileMapRevision(snap.mapRevision) ?? snap.mapRevision;
            setDriverSnapshotId(effectiveSnapshotId);
            setDriverMapRevision(effectiveMapRevision);
            guardsRef.current.snapshotId = effectiveSnapshotId;
            guardsRef.current.mapRevision = effectiveMapRevision;
          } catch {
            // Proceed to dispatchCommand and let error report downstream
          }
        }
      }

      if (effectiveSnapshotId !== undefined) {
        params.snapshotId = effectiveSnapshotId;
      }
      if (effectiveMapRevision !== undefined) {
        params.mapRevision = effectiveMapRevision;
      }

      return dispatchCommand("click", params, guards);
    },
    [dispatchCommand]
  );

  const fill = useCallback(
    async (
      selectorOrRef: string,
      textOrValue: string,
      snapshotIdOrOptions?: string | RemoteBrowserFillOptions,
      explicitGuards?: RemoteBrowserMutationGuards
    ) => {
      let snapshotId: string | undefined;
      let mapRevision: string | undefined;
      let guards: RemoteBrowserMutationGuards | undefined = explicitGuards;

      if (typeof snapshotIdOrOptions === "string") {
        snapshotId = snapshotIdOrOptions;
      } else if (snapshotIdOrOptions && typeof snapshotIdOrOptions === "object") {
        snapshotId = snapshotIdOrOptions.snapshotId;
        mapRevision = snapshotIdOrOptions.mapRevision;
        guards = snapshotIdOrOptions;
      }

      let effectiveSnapshotId =
        snapshotId ?? guardsRef.current.snapshotId ?? undefined;
      let effectiveMapRevision =
        reconcileMapRevision(mapRevision) ?? guardsRef.current.mapRevision ?? undefined;

      const target = selectorOrRef;
      const needsSnapshot = Boolean(
        (guards as Record<string, unknown> | undefined)?.needsSnapshot ||
        (guards as Record<string, unknown> | undefined)?.acquireSnapshot ||
        needsSnapshotAcquisition(target)
      );

      // Acquire valid remote snapshot before fill when needed
      if (needsSnapshot && (!effectiveSnapshotId || !effectiveMapRevision)) {
        const { client: curClient, browserId: curBrowserId } = guardsRef.current;
        if (curClient && curBrowserId && typeof curClient.takeSnapshot === "function") {
          try {
            const snap = await curClient.takeSnapshot(curBrowserId);
            effectiveSnapshotId = snap.snapshotId;
            effectiveMapRevision = reconcileMapRevision(snap.mapRevision) ?? snap.mapRevision;
            setDriverSnapshotId(effectiveSnapshotId);
            setDriverMapRevision(effectiveMapRevision);
            guardsRef.current.snapshotId = effectiveSnapshotId;
            guardsRef.current.mapRevision = effectiveMapRevision;
          } catch {
            // Proceed to dispatchCommand and let error report downstream
          }
        }
      }

      const params: Record<string, unknown> = {
        reference: selectorOrRef,
        selector: selectorOrRef,
        value: textOrValue,
        text: textOrValue,
      };

      if (effectiveSnapshotId !== undefined) {
        params.snapshotId = effectiveSnapshotId;
      }
      if (effectiveMapRevision !== undefined) {
        params.mapRevision = effectiveMapRevision;
      }

      return dispatchCommand("fill", params, guards);
    },
    [dispatchCommand]
  );

  const keypress = useCallback(
    (key: string, guards?: RemoteBrowserMutationGuards) =>
      dispatchCommand("keypress", { key }, guards),
    [dispatchCommand]
  );

  const wait = useCallback(
    (params: { condition?: string; timeoutMs?: number }, guards?: RemoteBrowserMutationGuards) =>
      dispatchCommand("wait", params as Record<string, unknown>, guards),
    [dispatchCommand]
  );

  const evalJs = useCallback(
    (script: string, guards?: RemoteBrowserMutationGuards) =>
      dispatchCommand("eval", { script }, guards),
    [dispatchCommand]
  );

  return {
    driverState,
    leaseEpoch,
    expiresAt,
    error,
    snapshotId: driverSnapshotId,
    mapRevision: driverMapRevision,
    claim,
    release,
    takeSnapshot,
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
