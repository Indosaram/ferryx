import { useEffect, useRef, useState, useCallback } from "react";
import {
  allocateSession,
  listMachines,
  openTunnel,
  planLimitStateFromError,
  redeemInTunnel,
  requestGrant,
  type AccountMachineView,
  type PlanLimitState,
} from "./accountSession";
import { getOrCreateAttachKey, type AttachKeyPair } from "./accountAttach";
import type { TunnelTransport } from "./attachTunnel";
import { suggestDeviceName } from "./deviceIdentity";
import { getOrCreateInstallationId } from "../lib/storageKeys";
import { normalizeRemoteWorkspaceState, type RemoteContextOption } from "./RemoteSessionList";

export interface AccountWorktreeOption extends RemoteContextOption {
  machineId: string;
  machineDisplayName: string;
  machinePlatform: string;
  machineOnline: boolean;
}

export interface ActiveMachineConnection {
  transport: TunnelTransport;
  close: () => void;
  machine: AccountMachineView;
  deviceToken: string;
}

export interface MachineDiscoveryStatus {
  machine: AccountMachineView;
  status: "idle" | "tunneling" | "ready" | "offline" | "error";
  error?: string;
  options: AccountWorktreeOption[];
}

export interface UseAccountWorktreesResult {
  loading: boolean;
  initialized: boolean;
  error: string | null;
  machines: AccountMachineView[];
  machineStatuses: Record<string, MachineDiscoveryStatus>;
  accountOptions: AccountWorktreeOption[];
  retryMachine: (machineId: string) => Promise<void>;
  /**
   * Transfers ownership of a tunnel to the caller: the returned connection is removed
   * from the discovery pool, so discovery cleanup (closeAllExcept, regeneration, unmount)
   * never closes it. The caller must close it when it is replaced.
   */
  acquireConnection: (machineId: string) => Promise<ActiveMachineConnection | null>;
  closeAllExcept: (machineId: string | null) => void;
}

interface StoredTunnelState {
  transport: TunnelTransport;
  close: () => void;
  deviceToken: string;
  machine: AccountMachineView;
}

export function useAccountWorktrees(
  relayUrl: string,
  accountSessionToken: string | null,
  enabled: boolean,
  onUnauthorized?: () => void,
  onPlanLimit?: (state: PlanLimitState) => void,
): UseAccountWorktreesResult {
  const [machines, setMachines] = useState<AccountMachineView[]>([]);
  const [loading, setLoading] = useState(() => enabled && Boolean(accountSessionToken));
  const [initialized, setInitialized] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [machineStatuses, setMachineStatuses] = useState<Record<string, MachineDiscoveryStatus>>({});

  const tunnelsRef = useRef<Map<string, StoredTunnelState>>(new Map());
  const activeGenerationRef = useRef(0);
  const machinesRef = useRef<AccountMachineView[]>([]);
  machinesRef.current = machines;
  // The caller's callback identity changes with its connection state; keeping it out of
  // the discovery effect deps stops every connection change from restarting discovery.
  const onUnauthorizedRef = useRef(onUnauthorized);
  onUnauthorizedRef.current = onUnauthorized;
  const onPlanLimitRef = useRef(onPlanLimit);
  onPlanLimitRef.current = onPlanLimit;

  const closeAllExcept = useCallback((keepMachineId: string | null) => {
    tunnelsRef.current.forEach((t, mid) => {
      if (mid !== keepMachineId) {
        try {
          t.close();
        } catch {}
      }
    });
    if (keepMachineId) {
      const kept = tunnelsRef.current.get(keepMachineId);
      tunnelsRef.current.clear();
      if (kept) tunnelsRef.current.set(keepMachineId, kept);
    } else {
      tunnelsRef.current.clear();
    }
  }, []);

  const probeMachine = useCallback(
    async (
      machine: AccountMachineView,
      attachKey: AttachKeyPair,
      isGenerationAlive: () => boolean,
    ) => {
      const isOnline = machine.online !== false;
      if (!isOnline) {
        if (isGenerationAlive()) {
          setMachineStatuses((prev) => ({
            ...prev,
            [machine.machineId]: { machine, status: "offline", options: [] },
          }));
        }
        return;
      }

      if (isGenerationAlive()) {
        setMachineStatuses((prev) => ({
          ...prev,
          [machine.machineId]: { machine, status: "tunneling", options: [] },
        }));
      }

      let localTunnel: { transport: TunnelTransport; close: () => void } | null = null;
      let tunnelRetained = false;

      try {
        const grant = await requestGrant(
          relayUrl,
          accountSessionToken!,
          machine,
          attachKey.publicKey,
          { grantScope: "machine" },
        );
        if (!isGenerationAlive()) return;

        const session = await allocateSession(
          relayUrl,
          accountSessionToken!,
          machine.machineId,
        );
        if (!isGenerationAlive()) return;

        localTunnel = await openTunnel({
          relayOrigin: grant.relayOrigin || relayUrl,
          machineId: machine.machineId,
          enrollmentEpoch: machine.enrollmentEpoch,
          machineAttachPublicKey: grant.machineAttachPublicKey,
          localKeyPair: attachKey,
          sessionId: session.sessionId,
        });
        if (!isGenerationAlive()) return;

        const pair = await redeemInTunnel(
          localTunnel.transport,
          grant.pairingToken,
          suggestDeviceName(),
          getOrCreateInstallationId(),
        );
        if (!isGenerationAlive()) return;

        const state: StoredTunnelState = {
          transport: localTunnel.transport,
          close: localTunnel.close,
          deviceToken: pair.token,
          machine,
        };
        tunnelsRef.current.set(machine.machineId, state);
        tunnelRetained = true;

        const res = await localTunnel.transport.fetchLike("/api/v1/workspace/state", {
          headers: { Authorization: `Bearer ${pair.token}` },
        });

        if (res.status >= 200 && res.status < 300) {
          const text = new TextDecoder().decode(res.body);
          const raw = JSON.parse(text);

          const projectRows: any[] = Array.isArray(raw?.projects)
            ? raw.projects
            : Array.isArray(raw?.workspaces)
            ? raw.workspaces
            : [];

          const isolated = { projects: projectRows };
          const normalized = normalizeRemoteWorkspaceState(isolated);

          const machineName = machine.displayName || machine.machineId;
          const machineOpts: AccountWorktreeOption[] = [];
          const seen = new Set<string>();

          for (const opt of normalized.options) {
            const rawWs = opt.workspaceId;
            if (!rawWs) continue;
            const slug = opt.worktreeSlug ?? null;
            const label = opt.worktreeLabel ?? null;
            const dedupeKey = `${rawWs}\u0000${slug ?? ""}`;
            if (!seen.has(dedupeKey)) {
              seen.add(dedupeKey);
              // workspaceId stays the raw desktop id; machine identity travels in machineId.
              machineOpts.push({
                ...opt,
                workspaceId: rawWs,
                worktreeSlug: slug,
                worktreeLabel: label,
                machineId: machine.machineId,
                machineDisplayName: machineName,
                machinePlatform: machine.platform || "remote",
                machineOnline: machine.online !== false,
              });
            }
          }

          if (isGenerationAlive()) {
            setMachineStatuses((prev) => ({
              ...prev,
              [machine.machineId]: { machine, status: "ready", options: machineOpts },
            }));
          }
        } else {
          throw new Error(`Workspace query failed (${res.status})`);
        }
      } catch (err) {
        if (tunnelRetained) {
          if (tunnelsRef.current.get(machine.machineId)?.transport === localTunnel?.transport) {
            tunnelsRef.current.delete(machine.machineId);
          }
          tunnelRetained = false;
        }
        if (isGenerationAlive()) {
          const limitState = planLimitStateFromError(err);
          if (limitState) onPlanLimitRef.current?.(limitState);
          setMachineStatuses((prev) => ({
            ...prev,
            [machine.machineId]: {
              machine,
              status: "error",
              error: err instanceof Error ? err.message : "Tunnel discovery failed",
              options: [],
            },
          }));
        }
      } finally {
        if (localTunnel && !tunnelRetained) {
          try {
            localTunnel.close();
          } catch {}
        }
      }
    },
    [accountSessionToken, relayUrl],
  );

  useEffect(() => {
    if (!enabled || !accountSessionToken) {
      // retryMachine checks only this counter, so disabling discovery must bump it too:
      // otherwise a retry probe started before the suspension can still land its tunnel.
      activeGenerationRef.current += 1;
      closeAllExcept(null);
      setMachines([]);
      setMachineStatuses({});
      setLoading(false);
      setInitialized(false);
      setError(null);
      return;
    }

    activeGenerationRef.current += 1;
    const currentGeneration = activeGenerationRef.current;
    let cancelled = false;

    closeAllExcept(null);
    setMachineStatuses({});
    setLoading(true);
    setInitialized(false);
    setError(null);

    const isGenerationAlive = () =>
      !cancelled && activeGenerationRef.current === currentGeneration;

    listMachines(relayUrl, accountSessionToken)
      .then(async (data) => {
        if (!isGenerationAlive()) return;
        setMachines(data);
        setLoading(false);
        setInitialized(true);

        const attachKey = await getOrCreateAttachKey();
        if (!attachKey) throw new Error("Failed to prepare initiator attach key");

        const onlineMachines = data.filter((m) => m.online !== false);
        const offlineMachines = data.filter((m) => m.online === false);

        if (offlineMachines.length > 0 && isGenerationAlive()) {
          setMachineStatuses((prev) => {
            const next = { ...prev };
            for (const off of offlineMachines) {
              next[off.machineId] = { machine: off, status: "offline", options: [] };
            }
            return next;
          });
        }

        await Promise.allSettled(
          onlineMachines.map((m) => probeMachine(m, attachKey, isGenerationAlive)),
        );
      })
      .catch((err) => {
        if (!isGenerationAlive()) return;
        const limitState = planLimitStateFromError(err);
        if (limitState) onPlanLimitRef.current?.(limitState);
        if (err && typeof err === "object" && "code" in err && (err as { code: string }).code === "UNAUTHORIZED") {
          onUnauthorizedRef.current?.();
          return;
        }
        setError(err instanceof Error ? err.message : "Failed to load account machines");
        setLoading(false);
        setInitialized(true);
      });

    return () => {
      cancelled = true;
      closeAllExcept(null);
    };
  }, [accountSessionToken, closeAllExcept, enabled, probeMachine, relayUrl]);

  const retryMachine = useCallback(
    async (machineId: string) => {
      const machine = machinesRef.current.find((m) => m.machineId === machineId);
      if (!machine || !accountSessionToken) return;

      const currentGen = activeGenerationRef.current;
      const isRetryAlive = () => activeGenerationRef.current === currentGen;

      try {
        const attachKey = await getOrCreateAttachKey();
        if (!attachKey || !isRetryAlive()) return;
        await probeMachine(machine, attachKey, isRetryAlive);
      } catch (err) {
        if (isRetryAlive()) {
          setMachineStatuses((prev) => ({
            ...prev,
            [machine.machineId]: {
              machine,
              status: "error",
              error: err instanceof Error ? err.message : "Retry failed",
              options: [],
            },
          }));
        }
      }
    },
    [accountSessionToken, probeMachine],
  );

  const acquireConnection = useCallback(
    async (machineId: string): Promise<ActiveMachineConnection | null> => {
      const existing = tunnelsRef.current.get(machineId);
      if (existing) {
        tunnelsRef.current.delete(machineId);
        return {
          transport: existing.transport,
          close: existing.close,
          machine: existing.machine,
          deviceToken: existing.deviceToken,
        };
      }

      const machine = machinesRef.current.find((m) => m.machineId === machineId);
      if (!machine || !accountSessionToken) return null;

      try {
        const attachKey = await getOrCreateAttachKey();
        if (!attachKey) return null;

        const grant = await requestGrant(
          relayUrl,
          accountSessionToken,
          machine,
          attachKey.publicKey,
          { grantScope: "machine" },
        );

        const session = await allocateSession(
          relayUrl,
          accountSessionToken,
          machine.machineId,
        );

        const tunnel = await openTunnel({
          relayOrigin: grant.relayOrigin || relayUrl,
          machineId: machine.machineId,
          enrollmentEpoch: machine.enrollmentEpoch,
          machineAttachPublicKey: grant.machineAttachPublicKey,
          localKeyPair: attachKey,
          sessionId: session.sessionId,
        });

        const pair = await redeemInTunnel(
          tunnel.transport,
          grant.pairingToken,
          suggestDeviceName(),
          getOrCreateInstallationId(),
        ).catch((err) => {
          try {
            tunnel.close();
          } catch {}
          throw err;
        });

        return {
          transport: tunnel.transport,
          close: tunnel.close,
          machine,
          deviceToken: pair.token,
        };
      } catch (err) {
        const limitState = planLimitStateFromError(err);
        if (limitState) onPlanLimitRef.current?.(limitState);
        console.warn("Account tunnel acquisition failed", err);
        return null;
      }
    },
    [accountSessionToken, relayUrl],
  );

  const accountOptions: AccountWorktreeOption[] = [];
  for (const machine of machines) {
    const status = machineStatuses[machine.machineId];
    if (status?.status === "ready" && status.options.length > 0) {
      for (const opt of status.options) {
        accountOptions.push(opt);
      }
    }
  }

  return {
    loading,
    initialized,
    error,
    machines,
    machineStatuses,
    accountOptions,
    retryMachine,
    acquireConnection,
    closeAllExcept,
  };
}
