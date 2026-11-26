import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { isTauri } from "@tauri-apps/api/core";
import type { MachineFilter, MachineProjectTarget, RemoteContext } from "../../lib/machineNavigation";
import { DEFAULT_MACHINE_LABEL, pairedHostInventory, type ReconcileReport } from "../../lib/pairedHostInventory";
import type { PairedHostContext } from "../../lib/pairedDaemonProject";
import { MACHINE_STATUS_EXPLANATIONS } from "../../lib/machineStatus";
import { bootstrapPairedHostReachability } from "../../lib/pairedHostReachability";
import { bootstrapPairedHostRoute } from "../../lib/pairedHostRoute";
import { extractIpcErrorMessage, useSshHosts, type SshHost } from "../../lib/sshHosts";
import { hasSshTunnelCapability } from "../../lib/sshTunnels";
import { isTailscaleProvisioned, syncTailscaleMachines, useTailscaleDiscovery, useTailscaleTargetsByHost } from "../../lib/tailscaleDiscovery";
import { remoteHostStore, type HostEndpoint, type RemoteHostStore } from "../../state/remoteHostStore";
import {
  AccountSessionError,
  clearStoredAccountSessionToken,
  getStoredAccountTokenOrigin,
  logoutAccountSession,
  getConfiguredAccountOrigin,
  getStoredAccountSessionToken,
  storeAccountOrigin,
  approveMachineRequest,
  denyMachineRequest,
  issueInstallKey,
  listMachines,
  listPendingMachineRequests,
  lookupMachineRequest,
  type AccountMachineView,
  type PendingMachineRequest,
} from "../../remote/accountSession";
import { adoptAccountSession, adoptAccountSessionWithReport } from "../../lib/accountPairing";
import { getAgentIntegration, toIpcError } from "../../lib/tauri";
import { useRemoteDesktopSupported } from "../../lib/remoteDesktop";
import { AccountSignIn } from "./AccountSignIn";
import { AddMachineModal, type AddMachineMethod } from "./AddMachineModal";
import { AccountBar } from "./remote/AccountBar";
import { AdvancedSection } from "./remote/AdvancedSection";
import { CloudProvidersSection } from "./remote/CloudProvidersSection";
import { DiscoveredList } from "./remote/DiscoveredList";
import { deleteCloudMachine } from "../../lib/cloudHost";
import { MachineDrawer, RemoveSshIntegrationDialog, type MachineDrawerContent, type RepairFeedback } from "./remote/MachineDrawer";
import { MachineList } from "./remote/MachineList";
import { MachineApprovalCard } from "./remote/MachineApprovalCard";
import { PairedMachineRow, pairedDisplayName, SshMachineRow, sshRowStatus } from "./remote/MachineRow";
import { SshTunnelsPanel } from "./remote/SshTunnelsPanel";
import { ThisMachineSection } from "./remote/ThisMachineSection";
import {
  accountErrorMessage,
  CHECK_RESULT_CODES,
  defaultNegotiate,
  describeAccountError,
  matchesQuery,
  mergePairedHosts,
  type MixedItem,
} from "./remote/remoteModel";
import { useSshActions } from "./remote/useSshActions";
import { useSshTunnels } from "./remote/useSshTunnels";

/** Stop-only setting: "off" disables automatic account pairing. Absent means on. */
const AUTO_PAIR_KEY = "ferryx.remote.autoPairAccountMachines";

/** Account-wide failures a reconciliation pass can report, in the user's terms. */
const REPAIR_BLOCKED_COPY: Record<string, string> = {
  NOT_SIGNED_IN: "The daemon has no account session, so it could not re-pair anything. Sign in again.",
  UNAUTHORIZED: "Your account session has expired. Sign in again to re-pair this machine.",
  SESSION_EXPIRED: "Your account session has expired. Sign in again to re-pair this machine.",
  SESSION_NOT_SAVED: "The daemon could not store your account session. Try signing in again.",
  INVENTORY_UNAVAILABLE: "The local machine inventory is unavailable, so pairing could not be checked. Try again in a moment.",
};

/**
 * Turns one reconciliation pass into the drawer's outcome line. Every branch names a code the
 * daemon produced, so the message is specific without inventing prose about a failure the UI
 * cannot see.
 */
function describeReconcileReport(report: ReconcileReport | null, host: HostEndpoint): RepairFeedback {
  if (!report) {
    return {
      tone: "error",
      code: "PAIRED_HOST_UNAVAILABLE",
      message: "Re-pairing did not complete. Check the connection to your account and try again.",
    };
  }
  if (report.blocked) {
    return {
      tone: "error",
      code: report.blocked,
      message: REPAIR_BLOCKED_COPY[report.blocked] ?? `Re-pairing stopped: ${report.blocked}.`,
    };
  }
  const skip = report.skipped.find((entry) => entry.machineId === host.machineId);
  if (skip) {
    return {
      tone: "error",
      code: skip.code,
      message: MACHINE_STATUS_EXPLANATIONS[skip.code] ?? `This machine could not be re-paired: ${skip.code}.`,
    };
  }
  if (report.paired.includes(host.hostId)) {
    return { tone: "success", code: "PAIRED", message: "This machine was re-paired. Its credentials are up to date." };
  }
  if (host.machineId && report.alreadyUsable.includes(host.machineId)) {
    return { tone: "success", code: "ALREADY_USABLE", message: "This machine is already paired with your account, so nothing needed to change." };
  }
  return { tone: "neutral", code: "NO_CHANGE", message: "The account pass finished and left this machine as it was." };
}

/** Account polling cadence while the Add Machine wizard is open or a machine awaits approval. */
export const ACCOUNT_ONBOARDING_POLL_MS = 5_000;

export interface RemoteSectionProps {
  initialContext?: RemoteContext;
  legacySsh?: boolean;
  onOpenProject?: (target: MachineProjectTarget, context: RemoteContext) => void;
  onOpenSshProject?: (hostId: string) => void;
  /** Opens a remote desktop pane for a paired host; the action shows only when RD is supported. */
  onOpenRemoteDesktop?: (hostId: string, label: string) => void;
  store?: RemoteHostStore;
  inventory?: typeof pairedHostInventory;
  negotiate?: (context: PairedHostContext) => Promise<unknown>;
  accountSessionToken?: string | null;
  accountOrigin?: string;
  accountOriginResolver?: () => string;
  onAccountSessionChange?: (token: string | null, origin: string) => void;
  /**
   * The capabilities the connected background service advertises. Absent/empty means the
   * tunnel section explains itself and sends nothing; it is a caller input, never discovered
   * by a failed invoke (MH-10).
   */
  daemonCapabilities?: readonly string[] | null;
}

export function RemoteSection({
  initialContext,
  legacySsh = false,
  onOpenProject,
  onOpenSshProject,
  onOpenRemoteDesktop,
  store = remoteHostStore,
  inventory = pairedHostInventory,
  negotiate = defaultNegotiate,
  accountSessionToken: accountSessionTokenProp,
  accountOrigin: accountOriginProp,
  accountOriginResolver = getConfiguredAccountOrigin,
  onAccountSessionChange,
  daemonCapabilities,
}: RemoteSectionProps) {
  const [configuredOrigin, setConfiguredOrigin] = useState(() => accountOriginResolver());
  const accountOrigin = accountOriginProp ?? configuredOrigin;
  const [accountOriginDraft, setAccountOriginDraft] = useState(accountOrigin);
  const [accountOriginError, setAccountOriginError] = useState<string | null>(null);
  const remoteDesktopSupported = useRemoteDesktopSupported();
  // Declared first: the account-list effect below reports failures through this setter, so it
  // must exist before that effect is created (a later binding would be a TDZ error on render).
  const [actionError, setActionErrorState] = useState<string | null>(null);
  /* The structured reason behind `actionError`: the drawer renders it as a token so a caller
     asserts the cause instead of the sentence. */
  const [actionErrorKind, setActionErrorKind] = useState<string | null>(null);
  const setActionError = useCallback((error: string | null, kind: string | null = null) => {
    setActionErrorState(error);
    setActionErrorKind(kind);
  }, []);
  const [accountToken, setAccountToken] = useState<string | null>(() => {
    if (accountSessionTokenProp !== undefined) return accountSessionTokenProp;
    return getStoredAccountSessionToken(accountOrigin);
  });

  useEffect(() => {
    if (accountSessionTokenProp !== undefined) {
      setAccountToken(accountSessionTokenProp);
    }
  }, [accountSessionTokenProp]);

  const onAccountSessionChangeRef = useRef(onAccountSessionChange);
  onAccountSessionChangeRef.current = onAccountSessionChange;

  useEffect(() => {
    onAccountSessionChangeRef.current?.(accountToken, accountOrigin);
  }, [accountToken, accountOrigin]);

  const [accountMachines, setAccountMachines] = useState<AccountMachineView[]>([]);
  const [accountMachinesLoading, setAccountMachinesLoading] = useState(false);
  const [cloudRefreshSignal, setCloudRefreshSignal] = useState(0);
  const [deletingCloudName, setDeletingCloudName] = useState<string | null>(null);
  // Monotonic id of the newest account-machine list request. A response is applied only while
  // it is still the newest, so a sign-out or origin switch mid-request cannot repopulate the list.
  const machinesRequestRef = useRef(0);

  useEffect(() => {
    const requestId = ++machinesRequestRef.current;
    if (!accountToken) {
      setAccountMachines([]);
      return;
    }
    let active = true;
    setAccountMachinesLoading(true);
    listMachines(accountOrigin, accountToken)
      .then((machines) => {
        if (!active || requestId !== machinesRequestRef.current) return;
        setAccountMachines(machines);
      })
      .catch((err: unknown) => {
        if (!active || requestId !== machinesRequestRef.current) return;
        if (
          err instanceof AccountSessionError &&
          err.code === "UNAUTHORIZED"
        ) {
          endAccountSession();
          return;
        }
        const message = accountErrorMessage(err);
        if (message !== null) setActionError(message);
      })
      .finally(() => {
        if (active) setAccountMachinesLoading(false);
      });
    return () => {
      active = false;
    };
    // `endAccountSession` is declared below and only called from inside the callback, so it is
    // deliberately not a dependency: listing it here would evaluate it during render (TDZ).
  }, [accountToken, accountOrigin, setActionError]);

  const refreshAccountMachines = async (
    token: string,
    origin: string,
    describe: (message: string) => string = (message) => message,
  ) => {
    const requestId = ++machinesRequestRef.current;
    try {
      const machines = await listMachines(origin, token);
      if (requestId === machinesRequestRef.current) setAccountMachines(machines);
    } catch (err: unknown) {
      if (requestId !== machinesRequestRef.current) return;
      if (err instanceof AccountSessionError && err.code === "UNAUTHORIZED") {
        // The account no longer accepts this session, so the daemon must stop using it too;
        // otherwise it keeps retrying pairing with a token the server already rejected.
        void adoptAccountSession(origin, null);
        clearStoredAccountSessionToken();
        setAccountToken(null);
        return;
      }
      setActionError(describe(describeAccountError(err)));
    }
  };

  // Pending approval requests addressed to this account (by email hint). An older account
  // server without the endpoint, or any transient failure, leaves the list as it was: the
  // cards are an addition to the machine list, never a reason to show an error.
  const [pendingRequests, setPendingRequests] = useState<PendingMachineRequest[]>([]);
  const pendingRequestRef = useRef(0);
  const refreshPendingRequests = async (token: string, origin: string) => {
    const requestId = ++pendingRequestRef.current;
    try {
      const pending = await listPendingMachineRequests(origin, token);
      if (requestId === pendingRequestRef.current) setPendingRequests(pending);
    } catch {
      // Optional surface; see above.
    }
  };

  useEffect(() => {
    pendingRequestRef.current += 1;
    setPendingRequests([]);
    if (!accountToken) return;
    void refreshPendingRequests(accountToken, accountOrigin);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accountToken, accountOrigin]);

  const forgetAccountSession = () => {
    void adoptAccountSession(accountOrigin, null);
  };
  // Sign-out, an origin switch and a rejected token all end here, so the daemon is always told
  // to stop using a session that is going away.
  const endAccountSession = () => {
    clearStoredAccountSessionToken();
    setAccountToken(null);
    setAccountMachines([]);
    setPendingRequests([]);
    forgetAccountSession();
  };
  const handleSignOut = () => {
    if (accountToken) {
      const issuerOrigin = getStoredAccountTokenOrigin() || accountOrigin;
      void logoutAccountSession(issuerOrigin, accountToken);
    }
    endAccountSession();
  };
  // Stop-only setting: absent means on, and only an explicit off stops pairing.
  const autoPair = useSyncExternalStore(
    (onChange) => {
      const onStorage = (event: StorageEvent) => { if (event.key === AUTO_PAIR_KEY) onChange(); };
      window.addEventListener("storage", onStorage);
      return () => window.removeEventListener("storage", onStorage);
    },
    () => window.localStorage.getItem(AUTO_PAIR_KEY) !== "off",
  );
  useEffect(() => {
    if (!accountToken || !autoPair) return;
    void adoptAccountSession(accountOrigin, accountToken);
  }, [accountToken, accountOrigin, autoPair]);

  const handleUseAccountOrigin = () => {
    let next: URL;
    try {
      next = new URL(accountOriginDraft.trim());
    } catch {
      setAccountOriginError("Enter an HTTPS account server URL.");
      return;
    }
    const loopback = ["localhost", "127.0.0.1", "[::1]"].includes(next.hostname);
    if ((next.protocol !== "https:" && !(next.protocol === "http:" && loopback)) || next.username || next.password || next.pathname !== "/" || next.search || next.hash) {
      setAccountOriginError("Use an HTTPS server origin without a path or credentials (HTTP is allowed only on this machine).");
      return;
    }
    const origin = next.origin;
    setAccountOriginError(null);
    if (origin === accountOrigin) return;
    storeAccountOrigin(origin);
    endAccountSession();
    setActionError(null);
    setConfiguredOrigin(origin);
  };
  const [context] = useState<RemoteContext>(
    initialContext ?? { page: "machines", filter: legacySsh ? "ssh" : "all" },
  );
  // A deep link may scope the list to SSH machines. The scope is a dismissible view, never a
  // trap: the pill below the header clears it back to every machine.
  const [machineFilter, setMachineFilter] = useState<MachineFilter>(context.filter);
  const [searchQuery, setSearchQuery] = useState("");
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);
  // undefined opens the wizard on "Choose a method"; a method opens it on that method's step.
  const [addMachineMethod, setAddMachineMethod] = useState<AddMachineMethod | undefined>(undefined);
  const openAddMachine = (method?: AddMachineMethod) => {
    setAddMachineMethod(method);
    setIsAddModalOpen(true);
  };

  // Bounded polling: only while the wizard is open (an install or SSH enrollment is landing) or a
  // request is waiting for approval, and only while signed in. Closing the wizard, deciding the
  // last request, signing out or unmounting clears the interval.
  const pollingActive = Boolean(accountToken) && (isAddModalOpen || pendingRequests.length > 0);
  useEffect(() => {
    if (!pollingActive || !accountToken) return;
    const token = accountToken;
    const origin = accountOrigin;
    const interval = setInterval(() => {
      const requestId = ++machinesRequestRef.current;
      listMachines(origin, token)
        .then((machines) => {
          if (requestId === machinesRequestRef.current) setAccountMachines(machines);
        })
        .catch(() => {
          // A failed background poll keeps the last list; Refresh reports errors explicitly.
        });
      void refreshPendingRequests(token, origin);
    }, ACCOUNT_ONBOARDING_POLL_MS);
    return () => clearInterval(interval);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pollingActive, accountToken, accountOrigin]);

  const handleIssueInstallKey = async () => {
    if (!accountToken) throw new Error("Sign in to issue an install key");
    return issueInstallKey(accountOrigin, accountToken);
  };
  const handleLookupMachineRequest = async (userCode: string) => {
    if (!accountToken) throw new Error("Sign in to approve a machine");
    return lookupMachineRequest(accountOrigin, accountToken, userCode);
  };
  const handleApproveMachineRequest = async (requestId: string, userCode: string) => {
    if (!accountToken) throw new Error("Sign in to approve a machine");
    const result = await approveMachineRequest(accountOrigin, accountToken, requestId, userCode);
    void refreshAccountMachines(accountToken, accountOrigin);
    return result;
  };
  const handleDenyMachineRequest = async (requestId: string, userCode: string) => {
    if (!accountToken) throw new Error("Sign in to deny a machine");
    await denyMachineRequest(accountOrigin, accountToken, requestId, userCode);
  };

  // Namespaced selected machine key (e.g. "paired:xyz" or "ssh:xyz")
  const [selectedKey, setSelectedKey] = useState<string | undefined>(() => {
    if (!initialContext?.machine) return undefined;
    return `${initialContext.machine.kind === "pairedDaemon" ? "paired" : "ssh"}:${initialContext.machine.hostId}`;
  });


  // Store & Host inventory
  const state = useSyncExternalStore(store.subscribe, store.getState);
  const { hosts: sshHosts, refresh: refreshHosts } = useSshHosts();
  const tailscaleState = useTailscaleDiscovery();
  const tailscaleTargets = useTailscaleTargetsByHost();

  // Paired host states
  const [checks, setChecks] = useState<Record<string, { generation: string; code: string }>>({});
  const [confirmForget, setConfirmForget] = useState<PairedHostContext | null>(null);
  const [repairHostId, setRepairHostId] = useState<string | null>(null);
  const [repairBusyHostId, setRepairBusyHostId] = useState<string | null>(null);
  const [repairFeedback, setRepairFeedback] = useState<(RepairFeedback & { hostId: string }) | null>(null);

  const [agentIntegrationEnabled, setAgentIntegrationEnabled] = useState(true);
  const [pendingRemoveSshHost, setPendingRemoveSshHost] = useState<SshHost | null>(null);
  const [busy, setBusy] = useState(false);

  // Details drawer: the open row key, and the control that receives focus back on close: the row's
  // Details button, or its "More actions" trigger when the drawer was opened from that menu.
  const [drawerKey, setDrawerKey] = useState<string | null>(null);

  // One tunnel read for the whole section. The panel renders this snapshot and the drawer
  // editor saves into it, so opening a drawer costs no second `cmd_ssh_tunnel_list`.
  // A known-absent capability leaves the hook disabled, so an older daemon sees no tunnel
  // command at all; an unknown one reads once, which is what the panel always did on mount.
  const tunnelSource = useSshTunnels({
    enabled: isTauri() && (daemonCapabilities == null || hasSshTunnelCapability(daemonCapabilities)),
  });

  const {
    sshTestResults, setSshTestResults, sshPreparedHosts, sshRemovalResults, sshPasswordFeedback,
    editingSshHost, setEditingSshHost, editFormData, setEditFormData, editPassword, setEditPassword,
    showAdvancedEditSsh, setShowAdvancedEditSsh, confirmDeleteSshId, setConfirmDeleteSshId,
    handleTestSsh, handlePrepareSsh, handleRemoveSshIntegration, handleToggleDisableSsh, handleDeleteSsh,
    handleStartEditSsh, handleSaveEditSsh, handleClearSshPassword, resetTransientState, tunnels,
  } = useSshActions({
    sshHosts,
    busy,
    setBusy,
    setActionError,
    capabilities: daemonCapabilities,
    // The tunnel capability probe waits for a drawer: with no SSH drawer open nothing reads the
    // section, and a settings surface must not ask the daemon anything it does not render.
    tunnelSectionOpen: drawerKey !== null,
    tunnelSource,
  });

  const lastDrawerKeyRef = useRef<string | null>(null);
  // Bumped whenever the drawer closes or moves to another machine, so a re-pair pass that was
  // started for the previous machine cannot write its outcome into the new one.
  const drawerGenerationRef = useRef(0);
  const drawerOpenedFromMenuRef = useRef(false);
  const detailsRefs = useRef(new Map<string, HTMLButtonElement>());
  const menuTriggerRefs = useRef(new Map<string, HTMLButtonElement>());
  // Namespaced row refs to avoid ID collisions; used to reveal the row named by initialContext.
  const rowRefs = useRef(new Map<string, HTMLDivElement>());
  const pendingRevealRef = useRef(Boolean(initialContext?.machine));
  /**
   * Forgets everything the drawer was showing. `returnFocusTo` reads its own ref, so closing a
   * drawer still returns focus to the control that opened it.
   */
  const resetDrawerState = () => {
    drawerGenerationRef.current += 1;
    resetTransientState();
    setConfirmForget(null);
    setRepairHostId(null);
    setRepairBusyHostId(null);
    setRepairFeedback(null);
    setActionError(null);
  };
  const openDrawer = (key: string, fromMenu = false) => {
    // Moving between machines starts the new drawer clean: no leftover edit form, confirmation
    // prompt or error from the machine the user just left.
    if (lastDrawerKeyRef.current !== null && lastDrawerKeyRef.current !== key) resetDrawerState();
    lastDrawerKeyRef.current = key;
    drawerOpenedFromMenuRef.current = fromMenu;
    setDrawerKey(key);
  };
  const closeDrawer = () => {
    setDrawerKey(null);
    resetDrawerState();
  };


  useEffect(() => {
    let active = true;
    getAgentIntegration()
      .then(state => {
        if (active) setAgentIntegrationEnabled(state.enabled);
      })
      .catch((err: unknown) => {
        if (active) setActionError(extractIpcErrorMessage(err, "Failed to read the agent integration setting."));
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    void inventory.refresh();
    const refresh = () => { void inventory.refresh(); };
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [inventory]);

  // Reachability and route both keep their own retry state: a transient daemon failure leaves
  // them un-bootstrapped, so coming back to the window is a cheap second chance. Both are
  // idempotent once bootstrapped, and an unsupported backend resolves without a request.
  useEffect(() => {
    if (!isTauri()) return;
    const retry = () => {
      void bootstrapPairedHostReachability();
      void bootstrapPairedHostRoute();
    };
    window.addEventListener("focus", retry);
    return () => window.removeEventListener("focus", retry);
  }, []);

  const handleOpenProject = (target: MachineProjectTarget) => {
    if (onOpenProject) {
      onOpenProject(target, { ...context, machine: target });
    } else if (target.kind === "ssh") {
      onOpenSshProject?.(target.hostId);
    }
  };

  const handleRefreshAll = () => {
    // Refresh is the one control that must re-read everything the section shows: the paired
    // inventory, the saved SSH machines (its store, not just the cache), Tailscale discovery,
    // cloud machines, and the two paired-host observers. Only the account list is conditional on
    // being signed in.
    if (isTauri()) {
      void inventory.refresh();
      void refreshHosts().catch(() => undefined);
      void syncTailscaleMachines().catch(() => undefined);
      void bootstrapPairedHostReachability();
      void bootstrapPairedHostRoute();
    }
    setCloudRefreshSignal((n) => n + 1);
    if (accountToken) {
      void refreshAccountMachines(accountToken, accountOrigin);
    }
  };

  const handleDeleteCloudMachine = async (name: string) => {
    if (deletingCloudName) return;
    setDeletingCloudName(name);
    setActionError(null);
    try {
      await deleteCloudMachine(name);
      setCloudRefreshSignal((n) => n + 1);
    } catch (failure) {
      setActionError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setDeletingCloudName(null);
    }
  };


  const handleCheckPaired = async (host: HostEndpoint) => {
    if (!host.generation || state.nativeStatus !== "ready" || busy) return;
    setBusy(true);
    setActionError(null);
    let code = "READY";
    const ctx: PairedHostContext = { hostId: host.hostId, generation: host.generation };
    setChecks(prev => ({ ...prev, [host.hostId]: { generation: ctx.generation, code: "UNCHECKED" } }));

    try {
      const current = store.getState().hosts[host.hostId];
      if (current?.generation !== ctx.generation) throw new Error("STALE_HOST_GENERATION");
      if (current.authStatus !== "paired" || current.grantScope !== "machine") throw new Error("MACHINE_GRANT_REQUIRED");
      await negotiate(ctx);
    } catch (failure) {
      // Structured code first (PairedOperationError carries it); plain Errors thrown locally
      // (stale generation, missing grant, unsupported capability) carry the code as message.
      const ipc = toIpcError(failure);
      code = [ipc.code, ipc.message].find((key) => CHECK_RESULT_CODES.has(key))
        ?? "PAIRED_HOST_UNAVAILABLE";
    } finally {
      setBusy(false);
    }

    if (store.getState().hosts[host.hostId]?.generation !== ctx.generation) return;
    setChecks(prev => ({ ...prev, [host.hostId]: { generation: ctx.generation, code } }));
  };

  const handleConfirmForgetPaired = async () => {
    if (!confirmForget) return;
    const current = state.hosts[confirmForget.hostId];
    if (current?.generation !== confirmForget.generation) {
      setActionError("Credentials changed during this request. Refresh and retry.");
      setConfirmForget(null);
      return;
    }
    setBusy(true);
    setActionError(null);
    try {
      if (await inventory.forget(confirmForget.hostId)) {
        setConfirmForget(null);
      } else {
        setActionError("The native host operation failed.");
      }
    } finally {
      setBusy(false);
    }
  };

  /**
   * Re-pair hands the account session to the daemon again and reports what that pass actually did.
   * A report can be a success (`paired`), a no-op (`alreadyUsable`) or a specific refusal
   * (`blocked` / `skipped`), so the drawer never shows a generic "done" it cannot back up.
   */
  const handleRepairPaired = async (host: HostEndpoint) => {
    if (!accountToken || repairBusyHostId) return;
    const generation = drawerGenerationRef.current;
    setRepairBusyHostId(host.hostId);
    setRepairFeedback(null);
    setRepairHostId(null);
    setActionError(null);
    try {
      const report = await adoptAccountSessionWithReport(accountOrigin, accountToken);
      if (drawerGenerationRef.current !== generation) return;
      await inventory.refresh();
      if (drawerGenerationRef.current !== generation) return;
      setRepairFeedback({ hostId: host.hostId, ...describeReconcileReport(report, host) });
    } finally {
      setRepairBusyHostId((current) => (current === host.hostId ? null : current));
    }
  };

  const nativeReady = state.nativeStatus === "ready";
  const isEnrolled = (host: HostEndpoint) => accountMachines.some((m) => m.machineId === host.machineId);
  const tailscaleTargetFor = (host: SshHost) => tailscaleTargets.get(host.id);
  // A Tailscale peer whose helper is not installed is not usable yet; it waits under Discovered.
  // A disabled host stays under My machines: sync skips disabled hosts, so Discovered could never
  // recover it, and only its row menu offers Enable, Edit and Delete.
  const isDiscovered = (host: SshHost) => {
    if (host.disabled) return false;
    const target = tailscaleTargetFor(host);
    return (host.source === "tailscale" || Boolean(target)) && !isTailscaleProvisioned(target);
  };
  const mixedItems: MixedItem[] = [
    ...mergePairedHosts(accountMachines, state, accountOrigin).map((h) => ({ kind: "paired" as const, id: h.hostId, name: h.name || DEFAULT_MACHINE_LABEL, host: h })),
    ...sshHosts.filter((h) => !isDiscovered(h)).map((h) => ({ kind: "ssh" as const, id: h.id, name: h.label || h.hostname, host: h })),
  ].sort((a, b) => a.name.localeCompare(b.name));
  // The filter narrows what is shown, never what exists: `total` follows the scoped list so the
  // header count and the empty state describe the same thing the user is looking at.
  const scopedItems = machineFilter === "ssh" ? mixedItems.filter((item) => item.kind === "ssh") : mixedItems;
  const filteredItems = scopedItems.filter((item) => matchesQuery(item, searchQuery));
  const sshStatusInput = (host: SshHost) => ({
    host,
    target: tailscaleTargetFor(host),
    discoveryRefreshing: tailscaleState.refreshing,
    test: sshTestResults[host.id],
    prepared: Boolean(sshPreparedHosts[host.id]),
    canOpenProject: Boolean(onOpenProject || onOpenSshProject),
  });
  const discovered = sshHosts.filter(isDiscovered).map((host) => ({ host, target: tailscaleTargetFor(host), status: sshRowStatus(sshStatusInput(host)) }));
  const discoveryBanner = tailscaleState.status !== "idle" && (tailscaleState.refreshing || tailscaleState.status === "error" || tailscaleState.hosts.length === 0)
    ? {
        tone: tailscaleState.status === "error" ? ("alert" as const) : ("status" as const),
        message: tailscaleState.refreshing
          ? "Discovering Tailscale machines…"
          : tailscaleState.status === "error"
          ? tailscaleState.discoveryError?.message ?? tailscaleState.lastError ?? "Tailscale discovery failed."
          : "No Tailscale machines discovered.",
      }
    : null;
  const retryTailscale = () => void syncTailscaleMachines().catch(() => undefined);
  const bindRow = (key: string) => (node: HTMLDivElement | null) => {
    if (node) rowRefs.current.set(key, node);
    else rowRefs.current.delete(key);
  };
  const bindDetails = (key: string) => (node: HTMLButtonElement | null) => {
    if (node) detailsRefs.current.set(key, node);
    else detailsRefs.current.delete(key);
  };
  const bindMenuTrigger = (key: string) => (node: HTMLButtonElement | null) => {
    if (node) menuTriggerRefs.current.set(key, node);
    else menuTriggerRefs.current.delete(key);
  };

  // Reveal the machine named by initialContext once its row exists.
  useEffect(() => {
    if (!pendingRevealRef.current || !selectedKey) return;
    const row = rowRefs.current.get(selectedKey);
    if (!row) return;
    pendingRevealRef.current = false;
    row.scrollIntoView?.({ block: "nearest" });
    row.focus({ preventScroll: true });
  });

  // Discovered rows are saved SSH hosts too; their Details opens the same SSH drawer as My machines.
  const drawerCandidates: MixedItem[] = [
    ...mixedItems,
    ...discovered.map(({ host }) => ({ kind: "ssh" as const, id: host.id, name: host.label || host.hostname, host })),
  ];
  const drawerItem = drawerKey ? drawerCandidates.find((item) => `${item.kind}:${item.id}` === drawerKey) : undefined;
  let drawerContent: MachineDrawerContent | null = null;
  if (drawerItem?.kind === "paired") {
    const host = drawerItem.host;
    const enrolled = isEnrolled(host);
    drawerContent = {
      kind: "paired", host, nativeReady, accountEnrolled: enrolled, check: checks[host.hostId], canOpenProject: Boolean(onOpenProject), busy,
      signedIn: Boolean(accountToken),
      repairRequested: repairHostId === host.hostId,
      repairBusy: repairBusyHostId === host.hostId,
      repairFeedback: repairFeedback?.hostId === host.hostId ? repairFeedback : null,
      confirmingForget: confirmForget?.hostId === host.hostId,
      onCheck: () => void handleCheckPaired(host),
      onRepair: () => {
        // An enrolled account machine re-pairs by handing the session to the daemon again, which
        // redeems a fresh machine grant through the same identity-checked path as signing in.
        // A machine the account does not know cannot be re-paired from here at all.
        if (enrolled && accountToken) {
          void handleRepairPaired(host);
          return;
        }
        setRepairHostId(host.hostId);
      },
      onForget: () => setConfirmForget({ hostId: host.hostId, generation: host.generation! }),
      onConfirmForget: () => void handleConfirmForgetPaired(),
      onCancelForget: () => setConfirmForget(null),
    };
  } else if (drawerItem?.kind === "ssh") {
    const host = drawerItem.host;
    drawerContent = {
      kind: "ssh", ...sshStatusInput(host), test: sshTestResults[host.id], busy, agentIntegrationEnabled,
      removalResult: sshRemovalResults[host.id],
      passwordFeedback: sshPasswordFeedback[host.id] ?? null,
      tunnels,
      confirmingDelete: confirmDeleteSshId === host.id,
      edit: editingSshHost?.id === host.id && editFormData
        ? {
            form: editFormData,
            password: editPassword,
            showAdvanced: showAdvancedEditSsh,
            onChange: (update) => setEditFormData((f) => f && update(f)),
            onPasswordChange: setEditPassword,
            onToggleAdvanced: () => setShowAdvancedEditSsh((v) => !v),
            onSubmit: handleSaveEditSsh,
            onCancel: () => { setEditingSshHost(null); setEditFormData(null); },
          }
        : null,
      onTest: () => void handleTestSsh(host),
      onPrepare: () => void handlePrepareSsh(host),
      // The confirm dialog is a separate modal; close the drawer first so it is not inert under the Sheet.
      onRemoveIntegration: () => { closeDrawer(); setPendingRemoveSshHost(host); },
      onClearPassword: () => void handleClearSshPassword(host),
      onEdit: () => handleStartEditSsh(host),
      onToggleDisable: () => void handleToggleDisableSsh(host),
      onDelete: () => setConfirmDeleteSshId(host.id),
      onConfirmDelete: () => void handleDeleteSsh(host.id),
      onCancelDelete: () => setConfirmDeleteSshId(null),
    };
  }

  return (
    <section aria-label="Remote" className="space-y-6">
      <h1 className="text-xl font-semibold">Remote</h1>

      {!accountToken ? (
        <AccountSignIn
          key={accountOrigin}
          origin={accountOrigin}
          onSignIn={(tok) => { if (getConfiguredAccountOrigin() === accountOrigin) setAccountToken(tok); }}
        />
      ) : (
        <AccountBar
          accountOrigin={accountOrigin}
          showNoMachinesHint={accountMachines.length === 0 && !accountMachinesLoading}
          onSignOut={handleSignOut}
        />
      )}

      {accountToken && pendingRequests.length > 0 ? (
        <section aria-label="Machines waiting for approval" className="space-y-2">
          <h2 className="text-[13px] font-medium text-foreground">Waiting for approval</h2>
          {pendingRequests.map((request) => (
            <MachineApprovalCard
              key={request.requestId}
              request={request}
              autoFocus={false}
              onApprove={() => handleApproveMachineRequest(request.requestId, request.userCode)}
              onDeny={() => handleDenyMachineRequest(request.requestId, request.userCode)}
              onDecided={() => {
                if (accountToken) void refreshPendingRequests(accountToken, accountOrigin);
              }}
            />
          ))}
        </section>
      ) : null}

      {machineFilter === "ssh" ? (
        <div className="flex flex-wrap items-center gap-2">
          <span
            data-testid="remote-machine-filter-pill"
            className="inline-flex items-center gap-1 rounded-full border border-border bg-muted/40 py-0.5 pl-2.5 pr-1 text-[11px] text-muted-foreground"
          >
            Showing SSH machines
            <button
              type="button"
              aria-label="Show all machines"
              className="rounded-full px-1.5 py-0.5 font-medium text-foreground hover:bg-accent"
              onClick={() => setMachineFilter("all")}
            >
              Show all
            </button>
          </span>
        </div>
      ) : null}

      <MachineList
        total={scopedItems.length}
        shown={filteredItems.length}
        searchQuery={searchQuery}
        actionError={drawerContent ? null : actionError}
        signedIn={Boolean(accountToken)}
        onSearchChange={setSearchQuery}
        onAddMachine={() => openAddMachine()}
        onRefresh={handleRefreshAll}
        cloudRefreshSignal={cloudRefreshSignal}
        onDeleteCloudMachine={handleDeleteCloudMachine}
        deletingCloudName={deletingCloudName}
      >
        {filteredItems.map((item) => {
          const key = `${item.kind}:${item.id}`;
          const common = {
            selected: selectedKey === key,
            busy,
            onDetails: () => openDrawer(key),
            onMenuDetails: () => openDrawer(key, true),
            rowRef: bindRow(key),
            detailsRef: bindDetails(key),
            menuTriggerRef: bindMenuTrigger(key),
          };
          if (item.kind === "paired") {
            const host = item.host;
            return (
              <PairedMachineRow
                key={key}
                {...common}
                host={host}
                nativeReady={nativeReady}
                accountEnrolled={isEnrolled(host)}
                check={checks[host.hostId]}
                canOpenProject={Boolean(onOpenProject)}
                signedIn={Boolean(accountToken)}
                onAddProject={() => handleOpenProject({ kind: "pairedDaemon", hostId: host.hostId, generation: host.generation! })}
                onCheck={() => void handleCheckPaired(host)}
                onForget={() => { openDrawer(key, true); setConfirmForget({ hostId: host.hostId, generation: host.generation! }); }}
                onOpenRemoteDesktop={remoteDesktopSupported && onOpenRemoteDesktop
                  ? () => onOpenRemoteDesktop(host.hostId, pairedDisplayName(host))
                  : undefined}
              />
            );
          }
          const host = item.host;
          return (
            <SshMachineRow
              key={key}
              {...common}
              {...sshStatusInput(host)}
              onAddProject={() => handleOpenProject({ kind: "ssh", hostId: host.id })}
              onRetry={() => void handleTestSsh(host)}
              onSetUp={() => openDrawer(key)}
              onEdit={() => { openDrawer(key, true); handleStartEditSsh(host); }}
              onToggleDisable={() => void handleToggleDisableSsh(host)}
              onDelete={() => { openDrawer(key, true); setConfirmDeleteSshId(host.id); }}
            />
          );
        })}
      </MachineList>

      <CloudProvidersSection />

      <DiscoveredList
        hosts={discovered}
        banner={discoveryBanner}
        refreshing={tailscaleState.refreshing}
        // Setup is per machine: it opens that host's drawer, where the failed step and its own
        // Retry live. Re-running discovery for the whole tailnet would not help this peer.
        onSetUp={(host) => openDrawer(`ssh:${host.id}`)}
        onRetryDiscovery={retryTailscale}
        onDetails={(hostId) => openDrawer(`ssh:${hostId}`)}
        detailsRef={(hostId) => bindDetails(`ssh:${hostId}`)}
      />

      {/* Every tunnel the daemon reports, saved and ad-hoc, in one list. The capability is the
          section's own input, so an absent one renders nothing and asks the daemon nothing. */}
      <SshTunnelsPanel
        hosts={sshHosts}
        capabilities={daemonCapabilities}
        enabled={isTauri()}
        snapshot={tunnelSource}
      />

      <ThisMachineSection />

      <AdvancedSection
        open={context.page === "details"}
        showAccountServer={!accountOriginProp}
        accountOrigin={accountOrigin}
        accountOriginDraft={accountOriginDraft}
        accountOriginError={accountOriginError}
        onAccountOriginDraftChange={setAccountOriginDraft}
        onUseAccountOrigin={handleUseAccountOrigin}
        nativeStatus={state.nativeStatus}
        migrationStatus={state.migrationStatus}
      />

      <MachineDrawer
        content={drawerContent}
        error={actionError}
        errorKind={actionErrorKind}
        onClose={closeDrawer}
        returnFocusTo={() => {
          const key = lastDrawerKeyRef.current;
          if (!key) return null;
          return (drawerOpenedFromMenuRef.current ? menuTriggerRefs.current : detailsRefs.current).get(key);
        }}
      />

      {/* ONE Add Machine opens the method-first wizard: install command, approval, SSH, or config import */}
      <AddMachineModal
        isOpen={isAddModalOpen}
        onClose={() => setIsAddModalOpen(false)}
        inventory={inventory}
        initialMethod={addMachineMethod}
        accountOrigin={accountOrigin}
        onIssueInstallKey={accountToken ? handleIssueInstallKey : undefined}
        accountMachines={accountMachines}
        onLookupMachineRequest={accountToken ? handleLookupMachineRequest : undefined}
        onApproveMachineRequest={accountToken ? handleApproveMachineRequest : undefined}
        onDenyMachineRequest={accountToken ? handleDenyMachineRequest : undefined}
        onSuccess={result => {
          if (result.kind === "pairedDaemon") {
            // Propagate verified generation-bound state
            setChecks(prev => ({ ...prev, [result.hostId]: { generation: result.generation, code: "READY" } }));
            setSelectedKey(`paired:${result.hostId}`);
          } else if (result.kind === "ssh") {
            // Propagate verified SSH connection test result
            setSshTestResults(prev => ({
              ...prev,
              [result.hostId]: { testing: false, reachable: result.summary.reachable, environment: result.summary.environment },
            }));
            setSelectedKey(`ssh:${result.hostId}`);
          }
        }}
        onOfferProject={target => handleOpenProject(target)}
        onImportDone={importedHostId => {
          // Select newly imported host; status remains UNCHECKED until user tests it
          setSelectedKey(`ssh:${importedHostId}`);
        }}
      />
      {pendingRemoveSshHost ? (
        <RemoveSshIntegrationDialog
          hostLabel={pendingRemoveSshHost.label}
          onCancel={() => setPendingRemoveSshHost(null)}
          onConfirm={() => {
            const host = pendingRemoveSshHost;
            setPendingRemoveSshHost(null);
            void handleRemoveSshIntegration(host);
          }}
        />
      ) : null}
    </section>
  );
}
