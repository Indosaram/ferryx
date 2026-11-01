import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { isTauri } from "@tauri-apps/api/core";
import type { MachineProjectTarget, RemoteContext } from "../../lib/machineNavigation";
import { DEFAULT_MACHINE_LABEL, nativePairedHostCommands, pairedHostInventory } from "../../lib/pairedHostInventory";
import type { PairedHostContext } from "../../lib/pairedDaemonProject";
import { extractIpcErrorMessage, resetSshHostsCache, useSshHosts, type SshHost } from "../../lib/sshHosts";
import { isTailscaleProvisioned, syncTailscaleMachines, useTailscaleDiscovery, useTailscaleTargetsByHost } from "../../lib/tailscaleDiscovery";
import { remoteHostStore, type HostEndpoint, type RemoteHostStore } from "../../state/remoteHostStore";
import {
  AccountSessionError,
  allocateSession,
  clearStoredAccountSessionToken,
  getStoredAccountTokenOrigin,
  logoutAccountSession,
  getConfiguredAccountOrigin,
  getStoredAccountSessionToken,
  storeAccountOrigin,
  approveMachineRequest,
  denyMachineRequest,
  issueEnrollmentCode,
  issueInstallKey,
  listMachines,
  listPendingMachineRequests,
  lookupMachineRequest,
  openTunnel,
  redeemInTunnel,
  requestGrant,
  type AccountMachineView,
  type PendingMachineRequest,
} from "../../remote/accountSession";
import { getOrCreateAttachKey } from "../../remote/accountAttach";
import { getAgentIntegration, toIpcError } from "../../lib/tauri";
import { useRemoteDesktopSupported } from "../../lib/remoteDesktop";
import { AccountSignIn } from "./AccountSignIn";
import { AddMachineModal, type AddMachineMethod } from "./AddMachineModal";
import { AccountBar } from "./remote/AccountBar";
import { AdvancedSection } from "./remote/AdvancedSection";
import { DiscoveredList } from "./remote/DiscoveredList";
import { MachineDrawer, RemoveSshIntegrationDialog, type MachineDrawerContent } from "./remote/MachineDrawer";
import { MachineList } from "./remote/MachineList";
import { MachineApprovalCard } from "./remote/MachineApprovalCard";
import { PairedMachineRow, pairedDisplayName, SshMachineRow, sshRowStatus } from "./remote/MachineRow";
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
}: RemoteSectionProps) {
  const [configuredOrigin, setConfiguredOrigin] = useState(() => accountOriginResolver());
  const accountOrigin = accountOriginProp ?? configuredOrigin;
  const [accountOriginDraft, setAccountOriginDraft] = useState(accountOrigin);
  const [accountOriginError, setAccountOriginError] = useState<string | null>(null);
  const remoteDesktopSupported = useRemoteDesktopSupported();
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
          clearStoredAccountSessionToken();
          setAccountToken(null);
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
  }, [accountToken, accountOrigin]);

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
        clearStoredAccountSessionToken();
        setAccountToken(null);
        return;
      }
      setActionError(describe(describeAccountError(err)));
    }
  };

  // The Add Machine wizard owns the issued code, its command and the copy state; it shows the
  // failure itself, so this only translates the error into a readable message.
  const handleIssueEnrollmentCode = async (): Promise<string> => {
    if (!accountToken) throw new Error("Sign in to issue an enrollment code");
    try {
      const res = await issueEnrollmentCode(accountOrigin, accountToken);
      return res.code;
    } catch (err: unknown) {
      throw new Error(accountErrorMessage(err) ?? "Failed to issue enrollment code");
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

  const handleSignOut = () => {
    if (accountToken) {
      const issuerOrigin = getStoredAccountTokenOrigin() || accountOrigin;
      void logoutAccountSession(issuerOrigin, accountToken);
    }
    clearStoredAccountSessionToken();
    setAccountToken(null);
    setAccountMachines([]);
    setPendingRequests([]);
  };
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
    clearStoredAccountSessionToken();
    setAccountToken(null);
    setAccountMachines([]);
    setActionError(null);
    setConfiguredOrigin(origin);
  };
  const [context] = useState<RemoteContext>(
    initialContext ?? { page: "machines", filter: legacySsh ? "ssh" : "all" },
  );
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
  const { hosts: sshHosts } = useSshHosts();
  const tailscaleState = useTailscaleDiscovery();
  const tailscaleTargets = useTailscaleTargetsByHost();

  // Paired host states
  const [checks, setChecks] = useState<Record<string, { generation: string; code: string }>>({});
  const [confirmForget, setConfirmForget] = useState<PairedHostContext | null>(null);
  const [repairHostId, setRepairHostId] = useState<string | null>(null);

  const [agentIntegrationEnabled, setAgentIntegrationEnabled] = useState(true);
  const [pendingRemoveSshHost, setPendingRemoveSshHost] = useState<SshHost | null>(null);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const {
    sshTestResults, setSshTestResults, sshPreparedHosts, sshRemovalResults,
    editingSshHost, setEditingSshHost, editFormData, setEditFormData, editPassword, setEditPassword,
    showAdvancedEditSsh, setShowAdvancedEditSsh, confirmDeleteSshId, setConfirmDeleteSshId,
    handleTestSsh, handlePrepareSsh, handleRemoveSshIntegration, handleToggleDisableSsh, handleDeleteSsh,
    handleStartEditSsh, handleSaveEditSsh,
  } = useSshActions({ sshHosts, busy, setBusy, setActionError });

  // Details drawer: the open row key, and the control that receives focus back on close: the row's
  // Details button, or its "More actions" trigger when the drawer was opened from that menu.
  const [drawerKey, setDrawerKey] = useState<string | null>(null);
  const lastDrawerKeyRef = useRef<string | null>(null);
  const drawerOpenedFromMenuRef = useRef(false);
  const detailsRefs = useRef(new Map<string, HTMLButtonElement>());
  const menuTriggerRefs = useRef(new Map<string, HTMLButtonElement>());
  // Namespaced row refs to avoid ID collisions; used to reveal the row named by initialContext.
  const rowRefs = useRef(new Map<string, HTMLDivElement>());
  const pendingRevealRef = useRef(Boolean(initialContext?.machine));
  const openDrawer = (key: string, fromMenu = false) => {
    lastDrawerKeyRef.current = key;
    drawerOpenedFromMenuRef.current = fromMenu;
    setDrawerKey(key);
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

  const handleOpenProject = (target: MachineProjectTarget) => {
    if (onOpenProject) {
      onOpenProject(target, { ...context, machine: target });
    } else if (target.kind === "ssh") {
      onOpenSshProject?.(target.hostId);
    }
  };

  const handleRefreshAll = () => {
    if (isTauri()) void inventory.refresh();
    resetSshHostsCache();
    if (accountToken) {
      void refreshAccountMachines(accountToken, accountOrigin);
    }
  };


  // Paired actions
  // Redeems an account-issued machine grant on THIS desktop. The web client has had this flow
  // since account enrollment shipped; the desktop never did, so an enrolled machine sat at
  // "Needs Grant" forever and its projects could not be added. Every piece already existed --
  // the grant API, the encrypted tunnel, and `paired_host_migrate_legacy` which persists the
  // redeemed device token -- so this wires them together rather than inventing a new path.
  const handleConnectAccountMachine = async (host: HostEndpoint) => {
    if (busy) return;
    const machine = accountMachines.find((m) => m.machineId === host.machineId);
    if (!machine || !accountToken) return;

    setBusy(true);
    setActionError(null);
    try {
      // A daemon that redeems grants itself keeps the device key in Rust and stores the host with
      // its machine attach key, so its relay traffic is end-to-end encrypted from the start.
      const capabilities = await nativePairedHostCommands.capabilities().catch(() => null);
      if (
        capabilities?.pairedHostE2EV1 === true
        && nativePairedHostCommands.redeemGrant
        && nativePairedHostCommands.clientAttachPublicKey
      ) {
        const clientPublicKey = await nativePairedHostCommands.clientAttachPublicKey();
        const grant = await requestGrant(
          accountOrigin,
          accountToken,
          machine,
          clientPublicKey,
          { grantScope: "machine" },
        );
        await nativePairedHostCommands.redeemGrant({
          relayOrigin: grant.relayOrigin || accountOrigin,
          machineId: machine.machineId,
          displayLabel: machine.displayName,
          pairingToken: grant.pairingToken,
          machineAttachPublicKey: grant.machineAttachPublicKey,
          enrollmentEpoch: String(machine.enrollmentEpoch),
          accountSessionToken: accountToken,
        });
        await refreshAccountMachines(
          accountToken,
          accountOrigin,
          (message) => `Connected, but the machine list could not be refreshed: ${message}`,
        );
        return;
      }

      const attachKey = await getOrCreateAttachKey();
      if (!attachKey) throw new Error("ATTACH_KEY_UNSUPPORTED");

      // A mirror grant connects but is refused by every route that lists or opens projects,
      // which is exactly the state this button exists to leave.
      const grant = await requestGrant(
        accountOrigin,
        accountToken,
        machine,
        attachKey.publicKey,
        { grantScope: "machine" },
      );
      const session = await allocateSession(accountOrigin, accountToken, machine.machineId);
      const tunnel = await openTunnel({
        relayOrigin: grant.relayOrigin || accountOrigin,
        machineId: machine.machineId,
        enrollmentEpoch: machine.enrollmentEpoch,
        machineAttachPublicKey: grant.machineAttachPublicKey,
        localKeyPair: attachKey,
        sessionId: session.sessionId,
      });
      try {
        const pair = await redeemInTunnel(tunnel.transport, grant.pairingToken, machine.displayName);
        // Persist through the daemon so the grant survives a restart and the local inventory
        // reports `paired` with machine scope.
        await nativePairedHostCommands.migrate({
          relayOrigin: grant.relayOrigin || accountOrigin,
          machineId: machine.machineId,
          displayLabel: machine.displayName,
          deviceToken: pair.token,
        });
      } finally {
        tunnel.close();
      }
      // The grant itself succeeded; only the refresh failed, so say that rather than
      // leaving a stale "Needs Grant" row with no explanation.
      await refreshAccountMachines(
        accountToken,
        accountOrigin,
        (message) => `Connected, but the machine list could not be refreshed: ${message}`,
      );
    } catch (failure) {
      setActionError(failure instanceof Error ? failure.message : "ACCOUNT_GRANT_FAILED");
    } finally {
      setBusy(false);
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
  const filteredItems = mixedItems.filter((item) => matchesQuery(item, searchQuery));
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
      confirmingForget: confirmForget?.hostId === host.hostId,
      onCheck: () => void handleCheckPaired(host),
      onRepair: () => {
        // An enrolled account machine re-pairs by redeeming a fresh machine grant,
        // the same identity-checked path as Connect. Anything else enrolls first.
        if (enrolled && accountToken) {
          setRepairHostId(null);
          void handleConnectAccountMachine(host);
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
      onRemoveIntegration: () => { setDrawerKey(null); setPendingRemoveSshHost(host); },
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
          onIssueCode={() => openAddMachine("enrollment")}
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

      <MachineList
        total={mixedItems.length}
        shown={filteredItems.length}
        searchQuery={searchQuery}
        actionError={drawerContent ? null : actionError}
        signedIn={Boolean(accountToken)}
        onSearchChange={setSearchQuery}
        onAddMachine={() => openAddMachine()}
        onRefresh={handleRefreshAll}
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
                onConnect={() => void handleConnectAccountMachine(host)}
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
              onEdit={() => { openDrawer(key, true); handleStartEditSsh(host); }}
              onToggleDisable={() => void handleToggleDisableSsh(host)}
              onDelete={() => { openDrawer(key, true); setConfirmDeleteSshId(host.id); }}
            />
          );
        })}
      </MachineList>

      <DiscoveredList
        hosts={discovered}
        banner={discoveryBanner}
        refreshing={tailscaleState.refreshing}
        onSetUp={retryTailscale}
        onRetryDiscovery={retryTailscale}
        onDetails={(hostId) => openDrawer(`ssh:${hostId}`)}
        detailsRef={(hostId) => bindDetails(`ssh:${hostId}`)}
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
        onClose={() => setDrawerKey(null)}
        returnFocusTo={() => {
          const key = lastDrawerKeyRef.current;
          if (!key) return null;
          return (drawerOpenedFromMenuRef.current ? menuTriggerRefs.current : detailsRefs.current).get(key);
        }}
      />

      {/* ONE Add Machine opens the method-first wizard: enrollment code, SSH, or config import */}
      <AddMachineModal
        isOpen={isAddModalOpen}
        onClose={() => setIsAddModalOpen(false)}
        inventory={inventory}
        initialMethod={addMachineMethod}
        accountOrigin={accountOrigin}
        onIssueEnrollmentCode={accountToken ? handleIssueEnrollmentCode : undefined}
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
