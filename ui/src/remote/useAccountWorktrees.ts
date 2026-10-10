import { useEffect, useRef, useState, useCallback } from "react";
import {
  allocateSession,
  listMachines,
  openTunnel,
  redeemInTunnel,
  requestGrant,
  AccountSessionError,
  type AccountMachineView,
} from "./accountSession";
import { getOrCreateAttachKey, type AttachKeyPair } from "./accountAttach";
import type { TunnelTransport } from "./attachTunnel";
import { suggestDeviceName } from "./deviceIdentity";
import { getOrCreateInstallationId } from "../lib/storageKeys";
import { type RemoteContextOption } from "./RemoteSessionList";
import {
  applySessionActivity,
  mergeSessionInventory,
  observationsFromContextTabs,
  parseInventoryEvent,
  parseSessionsPayload,
  workspaceLabelsFromCatalog,
  type RemoteSessionInventory,
  type SessionActivityObservation,
} from "./remoteSessionInventory";

export interface AccountWorktreeOption extends RemoteContextOption {
  machineId: string;
  machineDisplayName: string;
  machinePlatform: string;
  machineOnline: boolean;
  /** Session rows carry the states that were actually observed, or null when unknown. */
  running?: boolean;
  agentType?: string | null;
  daemonEpoch?: string | null;
  projectLabel?: string | null;
  workspaceLabel?: string | null;
  /** Null means no source has reported a state for this session yet. */
  inventoryActivityState?: "working" | "waiting" | "done" | null;
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
  errorCode?: string;
  options: AccountWorktreeOption[];
  /** Repository basenames keyed by workspace id; never absolute paths. */
  projectLabels?: Record<string, string>;
}

export interface UseAccountWorktreesResult {
  loading: boolean;
  initialized: boolean;
  error: string | null;
  machines: AccountMachineView[];
  machineStatuses: Record<string, MachineDiscoveryStatus>;
  accountOptions: AccountWorktreeOption[];
  /**
   * Every session the machine has reported, keyed by machine id. This is the complete
   * inventory (all workspaces, worktrees, daemon epochs and session ids) and is not the
   * picker's selected session.
   */
  sessionInventories: Record<string, RemoteSessionInventory>;
  /**
   * Applies an `inventoryInvalidated` boundary pushed on the machine events socket: a
   * complete boundary replaces the machine's inventory, a partial one only adds.
   */
  applyInventoryEvent: (machineId: string, raw: unknown) => void;
  /** Merges a fetched `/api/v1/sessions` payload into the machine's inventory. */
  noteSessionsPayload: (machineId: string, raw: unknown) => void;
  /** Records observed agent activity for one session without inventing rows. */
  noteSessionActivity: (machineId: string, observation: SessionActivityObservation) => void;
  /** Records published desktop context tabs as activity observations. */
  noteContextTabs: (machineId: string, tabs: unknown) => void;
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

interface MinimalMachineSessionTarget {
  machineId: string;
  sessionId: string;
  daemonEpoch?: string | number | null;
}

interface MinimalMachineSession {
  workspaceId: string;
  worktree: { wsId: string; slug: string } | null;
  target?: MinimalMachineSessionTarget | null;
  sessionId?: string | null;
  daemonEpoch?: string | number | null;
  running?: boolean;
  title?: string | null;
}

export function useAccountWorktrees(
  relayUrl: string,
  accountSessionToken: string | null,
  enabled: boolean,
  onUnauthorized?: () => void,
): UseAccountWorktreesResult {
  const [machines, setMachines] = useState<AccountMachineView[]>([]);
  const [loading, setLoading] = useState(() => enabled && Boolean(accountSessionToken));
  const [initialized, setInitialized] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [machineStatuses, setMachineStatuses] = useState<Record<string, MachineDiscoveryStatus>>({});
  const [sessionInventories, setSessionInventories] = useState<Record<string, RemoteSessionInventory>>({});

  const tunnelsRef = useRef<Map<string, StoredTunnelState>>(new Map());
  const activeGenerationRef = useRef(0);
  const machinesRef = useRef<AccountMachineView[]>([]);
  machinesRef.current = machines;
  // The caller's callback identity changes with its connection state; keeping it out of
  // the discovery effect deps stops every connection change from restarting discovery.
  const onUnauthorizedRef = useRef(onUnauthorized);
  onUnauthorizedRef.current = onUnauthorized;

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

  const noteSessionsPayload = useCallback((machineId: string, raw: unknown) => {
    const parsed = parseSessionsPayload(machineId, raw);
    if (!parsed.ok) return;
    setSessionInventories((prev) => ({
      ...prev,
      [machineId]: mergeSessionInventory(prev[machineId] ?? null, parsed.inventory),
    }));
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

        const projRes = await localTunnel.transport.fetchLike("/api/v1/workspace/projects", {
          headers: { Authorization: `Bearer ${pair.token}` },
        });

        if (projRes.status < 200 || projRes.status >= 300) {
          throw new Error(`Machine projects query failed (${projRes.status})`);
        }

        const projsText = new TextDecoder().decode(projRes.body);
        let projsData: { projects?: Array<{ workspaceId: string; availability: string }> };
        try {
          projsData = JSON.parse(projsText);
        } catch {
          throw new Error("Machine projects query returned invalid JSON");
        }
        const validProjects = (projsData.projects ?? []).filter(
          (p) => p.availability === "ready" && p.workspaceId && p.workspaceId.trim().length > 0,
        );

        const machineName = machine.displayName || machine.machineId;
        const projectLabels = Object.fromEntries(workspaceLabelsFromCatalog(projsData));
        const machineOpts: AccountWorktreeOption[] = [];
        const seen = new Set<string>();

        for (const proj of validProjects) {
          const wsId = proj.workspaceId;
          const wtRes = await localTunnel.transport.fetchLike(
            `/api/v1/workspace/worktrees?workspaceId=${encodeURIComponent(wsId)}`,
            { headers: { Authorization: `Bearer ${pair.token}` } },
          );

          if (wtRes.status < 200 || wtRes.status >= 300) {
            throw new Error(`Machine worktrees query failed for ${wsId} (${wtRes.status})`);
          }

          const wtText = new TextDecoder().decode(wtRes.body);
          let wtData: { worktrees?: Array<{ workspaceId: string; identity: { wsId: string; slug: string } | null; branch: string | null }> };
          try {
            wtData = JSON.parse(wtText);
          } catch {
            throw new Error(`Machine worktrees query returned invalid JSON for ${wsId}`);
          }
          const worktrees = wtData.worktrees ?? [];

          for (const wt of worktrees) {
            if (wt.workspaceId !== wsId) continue;
            const slug = wt.identity?.slug ?? null;
            const branch = wt.branch ? wt.branch.replace(/^refs\/heads\//, "") : null;
            const label = branch ?? slug ?? "main";
            const dedupeKey = `${wsId}\u0000${slug ?? ""}`;
            if (!seen.has(dedupeKey)) {
              seen.add(dedupeKey);
              machineOpts.push({
                workspaceId: wsId,
                worktreeSlug: slug,
                worktreeLabel: label,
                machineId: machine.machineId,
                machineDisplayName: machineName,
                machinePlatform: machine.platform || "remote",
                machineOnline: machine.online !== false,
              });
            }
          }
        }

        const sessRes = await localTunnel.transport.fetchLike("/api/v1/sessions", {
          headers: { Authorization: `Bearer ${pair.token}` },
        });

        if (sessRes.status < 200 || sessRes.status >= 300) {
          throw new Error(`Machine sessions query failed (${sessRes.status})`);
        }

        const sessText = new TextDecoder().decode(sessRes.body);
        let sessData: { revision?: string; completeness?: string; sessions?: MinimalMachineSession[] };
        try {
          sessData = JSON.parse(sessText);
        } catch {
          throw new Error("Machine sessions query returned invalid JSON");
        }
        if (!sessData || typeof sessData !== "object" || !Array.isArray(sessData.sessions)) {
          throw new Error("Machine sessions query returned invalid payload");
        }
        noteSessionsPayload(machine.machineId, sessData);

        if (isGenerationAlive()) {
          setMachineStatuses((prev) => ({
            ...prev,
            [machine.machineId]: { machine, status: "ready", options: machineOpts, projectLabels },
          }));
        }
      } catch (err) {
        if (tunnelRetained) {
          if (tunnelsRef.current.get(machine.machineId)?.transport === localTunnel?.transport) {
            tunnelsRef.current.delete(machine.machineId);
          }
          tunnelRetained = false;
        }
        if (isGenerationAlive()) {
          const errorCode = err instanceof AccountSessionError
            ? err.code
            : typeof err === "object" && err !== null && "code" in err && typeof (err as { code: unknown }).code === "string"
            ? (err as { code: string }).code
            : undefined;
          setMachineStatuses((prev) => ({
            ...prev,
            [machine.machineId]: {
              machine,
              status: "error",
              error: err instanceof Error ? err.message : "Tunnel discovery failed",
              errorCode,
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
    [accountSessionToken, noteSessionsPayload, relayUrl],
  );

  useEffect(() => {
    if (!enabled || !accountSessionToken) {
      closeAllExcept(null);
      setMachines([]);
      setMachineStatuses({});
      setSessionInventories({});
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
      activeGenerationRef.current += 1;
      closeAllExcept(null);
    };
  }, [accountSessionToken, closeAllExcept, enabled, probeMachine, relayUrl]);

  const applyInventoryEvent = useCallback((machineId: string, raw: unknown) => {
    const event = parseInventoryEvent(machineId, raw);
    if (event.kind !== "inventory") return;
    setSessionInventories((prev) => ({
      ...prev,
      [machineId]: mergeSessionInventory(prev[machineId] ?? null, event.inventory),
    }));
  }, []);

  const noteSessionActivity = useCallback((machineId: string, observation: SessionActivityObservation) => {
    setSessionInventories((prev) => {
      const current = prev[machineId];
      if (!current) return prev;
      const next = applySessionActivity(current, [observation]);
      return next && next !== current ? { ...prev, [machineId]: next } : prev;
    });
  }, []);

  const noteContextTabs = useCallback((machineId: string, tabs: unknown) => {
    const observations = observationsFromContextTabs(tabs);
    if (observations.length === 0) return;
    setSessionInventories((prev) => {
      const current = prev[machineId];
      if (!current) return prev;
      const next = applySessionActivity(current, observations);
      return next && next !== current ? { ...prev, [machineId]: next } : prev;
    });
  }, []);

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
        console.warn("Account tunnel acquisition failed", err);
        return null;
      }
    },
    [accountSessionToken, relayUrl],
  );

  const accountOptions: AccountWorktreeOption[] = [];
  for (const machine of machines) {
    const status = machineStatuses[machine.machineId];
    if (status?.status !== "ready") continue;
    const machineName = status.machine.displayName || machine.displayName || machine.machineId;
    const machinePlatform = status.machine.platform || machine.platform || "remote";
    const machineOnline = status.machine.online !== false;
    /* Snapshot options describe worktrees; session rows are rebuilt from the live
       inventory on every render so activity, additions and authoritative removals reach
       the picker instead of the discovery-time snapshot. */
    for (const opt of status.options) {
      if (opt.sessionId) continue;
      accountOptions.push({
        ...opt,
        machineId: machine.machineId,
        machineDisplayName: machineName,
        machinePlatform,
        machineOnline,
      });
    }
    const inventory = sessionInventories[machine.machineId];
    const projectLabels = status.projectLabels ?? {};
    const worktreeLabels = new Map<string, string>();
    for (const opt of status.options) {
      const label = opt.worktreeLabel ?? opt.worktreeSlug;
      if (label) worktreeLabels.set(`${opt.workspaceId}\u0000${opt.worktreeSlug ?? ""}`, label);
    }
    for (const entry of inventory?.entries ?? []) {
      /* Stopped sessions stay in `sessionInventories` for honest display, but they are not
         pickable attach targets. */
      if (!entry.running) continue;
      const label = entry.title
        ? `${entry.title} (${entry.sessionId.slice(0, 8)})`
        : `Session ${entry.sessionId.slice(0, 8)}`;
      const workspaceLabel = projectLabels[entry.workspaceId] ?? null;
      const worktreeLabel = worktreeLabels.get(`${entry.workspaceId}\u0000${entry.worktreeSlug ?? ""}`)
        ?? entry.worktreeLabel
        ?? (entry.worktreeSlug === null ? "main" : entry.worktreeSlug);
      const option: RemoteContextOption = {
        workspaceId: entry.workspaceId,
        worktreeSlug: entry.worktreeSlug,
        worktreeLabel,
        sessionId: entry.sessionId,
        sessionLabel: label,
        ...(entry.activityState ? { attention: entry.activityState } : {}),
        machineId: machine.machineId,
      };
      accountOptions.push({
        ...option,
        machineId: machine.machineId,
        machineDisplayName: machineName,
        machinePlatform,
        machineOnline,
        ...(entry.activityState ? { activityState: entry.activityState } : {}),
        inventoryActivityState: entry.activityState,
        running: entry.running,
        agentType: entry.agentType,
        daemonEpoch: entry.daemonEpoch,
        projectLabel: workspaceLabel,
        workspaceLabel,
      });
    }
  }

  return {
    loading,
    initialized,
    error,
    machines,
    machineStatuses,
    accountOptions,
    sessionInventories,
    applyInventoryEvent,
    noteSessionsPayload,
    noteSessionActivity,
    noteContextTabs,
    retryMachine,
    acquireConnection,
    closeAllExcept,
  };
}
