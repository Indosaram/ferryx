import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { isTauri } from "@tauri-apps/api/core";
import {
  AlertCircle,
  Check,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Copy,
  KeyRound,
  LogOut,
  Plus,
  Server,
  X,
} from "lucide-react";
import type { MachineProjectTarget, RemoteContext } from "../../lib/machineNavigation";
import {
  DEFAULT_MACHINE_LABEL,
  DEFAULT_RELAY_ORIGIN,
  pairedHostInventory,
} from "../../lib/pairedHostInventory";
import {
  createPairedDaemonProjectAdapter,
  type PairedHostContext,
} from "../../lib/pairedDaemonProject";
import {
  deleteSshHost,
  extractIpcErrorMessage,
  prepareSshIntegration,
  resetSshHostsCache,
  setSshPassword,
  testSshConnection,
  updateSshHost,
  useSshHosts,
  type SshAuthMethod,
  type SshHost,
  type SshRemoteEnvironment,
} from "../../lib/sshHosts";
import {
  remoteHostKey,
  remoteHostStore,
  selectHostList,
  type HostEndpoint,
  type RemoteHostStore,
} from "../../state/remoteHostStore";
import {
  AccountSessionError,
  clearStoredAccountSessionToken,
  getStoredAccountSessionToken,
  issueEnrollmentCode,
  listMachines,
  type AccountMachineView,
} from "../../remote/accountSession";
import { AccountSignIn } from "./AccountSignIn";
import { copyTextToClipboard } from "../../lib/clipboard";
import { AddMachineModal, type HostFormData } from "./AddMachineModal";
import { RemoteAccessSection } from "./RemoteAccessSection";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";

const explanations: Record<string, string> = {
  NATIVE_CONTEXT_REQUIRED: "Native host inventory is unavailable. Use the desktop app with a compatible local daemon; browser mirror access cannot manage machines.",
  PAIR_FAILED: "Could not pair. Check connectivity and daemon compatibility, obtain a fresh machine-access PIN, and retry.",
  MACHINE_GRANT_REQUIRED: "Needs machine access. A mirror PIN cannot authorize projects. Re-pair with an owner-issued machine PIN; revoked credentials cannot reconnect.",
  UNSUPPORTED_CAPABILITY: "This relay, remote daemon, or local daemon does not advertise the required machine capabilities. Upgrade compatible components.",
  STALE_HOST_GENERATION: "Credentials changed during this request. Refresh the inventory and check capabilities again.",
  PAIRED_HOST_UNAVAILABLE: "The native host operation failed. Check the relay and daemon versions, connectivity and PIN scope, then retry. Saved projects have not been removed.",
  OFFLINE: "Machine is offline. Saved projects remain available in the workspace; reconnect to the owning daemon.",
  UNCHECKED: "Check the remote machine capabilities before adding a project.",
  READY: "Machine project capabilities verified.",
};

const concisePairedStatus: Record<string, string> = {
  READY: "Ready",
  OFFLINE: "Offline",
  MACHINE_GRANT_REQUIRED: "Needs Grant",
  UNCHECKED: "Unchecked",
  NATIVE_CONTEXT_REQUIRED: "Desktop Required",
  UNSUPPORTED_CAPABILITY: "Incompatible",
  STALE_HOST_GENERATION: "Stale",
  PAIRED_HOST_UNAVAILABLE: "Unavailable",
  PAIR_FAILED: "Failed",
};

async function defaultNegotiate(context: PairedHostContext) {
  const result = await createPairedDaemonProjectAdapter(context).capabilities();
  if (!["directoryBrowseV1", "machineWorkspaceV1"].every(capability => result.capabilities.includes(capability))) {
    throw new Error("UNSUPPORTED_CAPABILITY");
  }
}

function sameConnection(left: SshHost, right: SshHost): boolean {
  return (
    left.id === right.id &&
    left.hostname === right.hostname &&
    left.username === right.username &&
    left.port === right.port &&
    left.identityFile === right.identityFile &&
    left.jumpHost === right.jumpHost &&
    left.authMethod === right.authMethod &&
    left.disabled === right.disabled
  );
}

interface SshTestState {
  testing: boolean;
  reachable?: boolean;
  error?: string | null;
  environment?: SshRemoteEnvironment | null;
  stage?: string;
}

export interface RemoteSectionProps {
  initialContext?: RemoteContext;
  legacySsh?: boolean;
  onOpenProject?: (target: MachineProjectTarget, context: RemoteContext) => void;
  onOpenSshProject?: (hostId: string) => void;
  store?: RemoteHostStore;
  inventory?: typeof pairedHostInventory;
  negotiate?: (context: PairedHostContext) => Promise<unknown>;
  accountSessionToken?: string | null;
  accountOrigin?: string;
}

export function RemoteSection({
  initialContext,
  legacySsh = false,
  onOpenProject,
  onOpenSshProject,
  store = remoteHostStore,
  inventory = pairedHostInventory,
  negotiate = defaultNegotiate,
  accountSessionToken: accountSessionTokenProp,
  accountOrigin = DEFAULT_RELAY_ORIGIN,
}: RemoteSectionProps) {
  const [accountToken, setAccountToken] = useState<string | null>(() => {
    if (accountSessionTokenProp !== undefined) return accountSessionTokenProp;
    return getStoredAccountSessionToken();
  });

  useEffect(() => {
    if (accountSessionTokenProp !== undefined) {
      setAccountToken(accountSessionTokenProp);
    }
  }, [accountSessionTokenProp]);

  const [accountMachines, setAccountMachines] = useState<AccountMachineView[]>([]);
  const [accountMachinesLoading, setAccountMachinesLoading] = useState(false);
  const [issuedCode, setIssuedCode] = useState<string | null>(null);
  const [isIssuingCode, setIsIssuingCode] = useState(false);
  const [copiedEnrollCommand, setCopiedEnrollCommand] = useState(false);

  useEffect(() => {
    if (!accountToken) {
      setAccountMachines([]);
      return;
    }
    let active = true;
    setAccountMachinesLoading(true);
    listMachines(accountOrigin, accountToken)
      .then((machines) => {
        if (!active) return;
        setAccountMachines(machines);
      })
      .catch((err: unknown) => {
        if (!active) return;
        if (
          err instanceof AccountSessionError &&
          (err.code === "UNAUTHORIZED" || err.status === 401)
        ) {
          clearStoredAccountSessionToken();
          setAccountToken(null);
          return;
        }
        if (err instanceof AccountSessionError) {
          setActionError(err.message);
        } else if (
          err &&
          typeof err === "object" &&
          "code" in err &&
          typeof (err as { code: string }).code === "string"
        ) {
          const typed = err as { code: string; message?: string };
          setActionError(typed.message || typed.code);
        } else if (err instanceof Error) {
          setActionError(err.message);
        }
      })
      .finally(() => {
        if (active) setAccountMachinesLoading(false);
      });
    return () => {
      active = false;
    };
  }, [accountToken, accountOrigin]);

  const handleIssueEnrollmentCode = async () => {
    if (!accountToken || isIssuingCode) return;
    setIsIssuingCode(true);
    setActionError(null);
    try {
      const res = await issueEnrollmentCode(accountOrigin, accountToken);
      setIssuedCode(res.code);
    } catch (err: unknown) {
      if (err instanceof AccountSessionError) {
        setActionError(err.message);
      } else if (
        err &&
        typeof err === "object" &&
        "code" in err &&
        typeof (err as { code: string }).code === "string"
      ) {
        const typed = err as { code: string; message?: string };
        setActionError(typed.message || typed.code);
      } else if (err instanceof Error) {
        setActionError(err.message);
      } else {
        setActionError("Failed to issue enrollment code");
      }
    } finally {
      setIsIssuingCode(false);
    }
  };

  const handleSignOut = () => {
    clearStoredAccountSessionToken();
    setAccountToken(null);
    setAccountMachines([]);
    setIssuedCode(null);
  };
  const [context] = useState<RemoteContext>(
    initialContext ?? { page: "machines", filter: legacySsh ? "ssh" : "all" },
  );
  const [searchQuery, setSearchQuery] = useState("");
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);

  // Namespaced selected machine key (e.g. "paired:xyz" or "ssh:xyz")
  const [selectedKey, setSelectedKey] = useState<string | undefined>(() => {
    if (!initialContext?.machine) return undefined;
    return `${initialContext.machine.kind === "pairedDaemon" ? "paired" : "ssh"}:${initialContext.machine.hostId}`;
  });

  const [expandedDetails, setExpandedDetails] = useState<Set<string>>(new Set());

  // Store & Host inventory
  const state = useSyncExternalStore(store.subscribe, store.getState);
  const { hosts: sshHosts } = useSshHosts();
  const hostsRef = useRef(sshHosts);

  // Paired host states
  const [checks, setChecks] = useState<Record<string, { generation: string; code: string }>>({});
  const [confirmForget, setConfirmForget] = useState<PairedHostContext | null>(null);
  const [repairHostId, setRepairHostId] = useState<string | null>(null);

  // SSH host states
  const [sshTestResults, setSshTestResults] = useState<Record<string, SshTestState>>({});
  const [sshPreparedHosts, setSshPreparedHosts] = useState<Record<string, boolean>>({});
  const [editingSshHost, setEditingSshHost] = useState<SshHost | null>(null);
  const [editFormData, setEditFormData] = useState<HostFormData | null>(null);
  const [editPassword, setEditPassword] = useState("");
  const [showAdvancedEditSsh, setShowAdvancedEditSsh] = useState(false);

  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  // Namespaced row refs to avoid ID collisions
  const rowRefs = useRef(new Map<string, HTMLDivElement>());

  // Restore sameConnection invalidation from SshSection
  useEffect(() => {
    const stale = hostsRef.current.filter(prev => !sshHosts.some(cur => sameConnection(prev, cur)));
    hostsRef.current = sshHosts;
    if (stale.length === 0) return;
    setSshTestResults(prev => {
      const next = { ...prev };
      for (const h of stale) delete next[h.id];
      return next;
    });
    setSshPreparedHosts(prev => {
      const next = { ...prev };
      for (const h of stale) delete next[h.id];
      return next;
    });
  }, [sshHosts]);

  const isCurrentHost = (host: SshHost) =>
    hostsRef.current.some(cur => sameConnection(cur, host));

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
      void listMachines(accountOrigin, accountToken)
        .then(setAccountMachines)
        .catch(() => {});
    }
  };

  const toggleDetails = (key: string) => {
    setExpandedDetails(prev => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  // Paired actions
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
      const message = failure instanceof Error ? failure.message : "";
      code = Object.prototype.hasOwnProperty.call(explanations, message) ? message : "PAIRED_HOST_UNAVAILABLE";
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

  // SSH actions with stale async guard
  const handleTestSsh = async (host: SshHost) => {
    if (sshTestResults[host.id]?.testing || busy) return;
    setSshTestResults(prev => ({ ...prev, [host.id]: { testing: true } }));
    setActionError(null);

    try {
      const summary = await testSshConnection(host);
      if (!isCurrentHost(host)) return;
      setSshTestResults(prev => ({
        ...prev,
        [host.id]: {
          testing: false,
          reachable: summary.reachable,
          error: summary.lastError,
          environment: summary.environment,
          stage: summary.diagnostic?.details?.stage,
        },
      }));
    } catch (err) {
      if (!isCurrentHost(host)) return;
      setSshTestResults(prev => ({
        ...prev,
        [host.id]: {
          testing: false,
          reachable: false,
          error: extractIpcErrorMessage(err, "Connection test failed."),
        },
      }));
    }
  };

  const handlePrepareSsh = async (host: SshHost) => {
    if (busy) return;
    setBusy(true);
    setActionError(null);
    try {
      await prepareSshIntegration(host);
      if (!isCurrentHost(host)) return;
      setSshPreparedHosts(prev => ({ ...prev, [host.id]: true }));
    } catch (err) {
      if (!isCurrentHost(host)) return;
      setActionError(extractIpcErrorMessage(err, "Agent integration preparation failed."));
    } finally {
      setBusy(false);
    }
  };

  const handleToggleDisableSsh = async (host: SshHost) => {
    if (busy) return;
    setBusy(true);
    setActionError(null);
    try {
      await updateSshHost({ ...host, disabled: !host.disabled });
    } catch (err) {
      setActionError(extractIpcErrorMessage(err, "Failed to update SSH machine."));
    } finally {
      setBusy(false);
    }
  };

  const handleDeleteSsh = async (hostId: string) => {
    if (busy) return;
    setBusy(true);
    setActionError(null);
    try {
      await deleteSshHost(hostId);
    } catch (err) {
      setActionError(extractIpcErrorMessage(err, "Failed to delete SSH machine."));
    } finally {
      setBusy(false);
    }
  };

  const handleStartEditSsh = (host: SshHost) => {
    setEditPassword("");
    setEditingSshHost(host);
    setEditFormData({
      label: host.label,
      hostname: host.hostname,
      username: host.username ?? "",
      port: host.port ? String(host.port) : "22",
      identityFile: host.identityFile ?? "",
      jumpHost: host.jumpHost ?? "",
      authMethod: host.authMethod,
    });
    setShowAdvancedEditSsh(Boolean(host.identityFile || host.jumpHost || host.authMethod !== "agent"));
  };

  const handleSaveEditSsh = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!editingSshHost || !editFormData || busy) return;

    if (!editFormData.label.trim()) {
      setActionError("Label is required.");
      return;
    }
    if (!editFormData.hostname.trim()) {
      setActionError("Hostname is required.");
      return;
    }
    const portNum = editFormData.port.trim() ? Number(editFormData.port) : 22;
    if (isNaN(portNum) || portNum < 1 || portNum > 65535 || !Number.isInteger(portNum)) {
      setActionError("Port must be an integer between 1 and 65535.");
      return;
    }

    setBusy(true);
    setActionError(null);

    const updated: SshHost = {
      ...editingSshHost,
      label: editFormData.label.trim(),
      hostname: editFormData.hostname.trim(),
      username: editFormData.username.trim() || undefined,
      port: portNum,
      identityFile: editFormData.identityFile.trim() || undefined,
      jumpHost: editFormData.jumpHost.trim() || undefined,
      authMethod: editFormData.authMethod,
    };

    try {
      if (updated.authMethod === "password" && editPassword) {
        await setSshPassword(updated, editPassword);
        const summary = await testSshConnection(updated);
        if (!summary.reachable) {
          setActionError(summary.lastError || "SSH authentication failed.");
          return;
        }
      }
      await updateSshHost(updated);
      setEditPassword("");
      setEditingSshHost(null);
      setEditFormData(null);
    } catch (err) {
      setActionError(extractIpcErrorMessage(err, "Failed to save SSH machine."));
    } finally {
      setBusy(false);
    }
  };

  const pairedHosts = selectHostList(state);

  const accountHostEndpoints: HostEndpoint[] = accountMachines.map((m) => {
    const hostId = m.machineId
      ? remoteHostKey(m.relayOrigin || accountOrigin, m.machineId)
      : m.machineRecordId;
    const local =
      state.hosts[hostId] ??
      Object.values(state.hosts).find((h) => h.machineId === m.machineId);
    return {
      hostId: local?.hostId ?? hostId,
      name: m.displayName || local?.name || DEFAULT_MACHINE_LABEL,
      address: m.relayOrigin || accountOrigin,
      transport: "relay",
      authStatus: "paired",
      online: m.online,
      machineId: m.machineId,
      displayName: m.displayName,
      relayOrigin: m.relayOrigin || accountOrigin,
      generation: local?.generation ?? String(m.enrollmentEpoch ?? "1"),
      grantScope: m.grantScope ?? local?.grantScope ?? "machine",
      lastSeenAt: m.lastSeenAt,
    };
  });

  const mergedPairedHosts: HostEndpoint[] = [
    ...accountHostEndpoints,
    ...pairedHosts.filter(
      (ph) =>
        !accountHostEndpoints.some(
          (ah) => ah.machineId === ph.machineId || ah.hostId === ph.hostId,
        ),
    ),
  ];

  type MixedItem =
    | { kind: "paired"; id: string; name: string; host: HostEndpoint }
    | { kind: "ssh"; id: string; name: string; host: SshHost };

  const mixedItems: MixedItem[] = [
    ...mergedPairedHosts.map((h) => ({
      kind: "paired" as const,
      id: h.hostId,
      name: h.name || DEFAULT_MACHINE_LABEL,
      host: h,
    })),
    ...sshHosts.map((h) => ({
      kind: "ssh" as const,
      id: h.id,
      name: h.label || h.hostname,
      host: h,
    })),
  ].sort((a, b) => a.name.localeCompare(b.name));

  const filteredItems = mixedItems.filter(item => {
    if (!searchQuery.trim()) return true;
    const q = searchQuery.toLowerCase();
    if (item.kind === "paired") {
      return (
        item.host.name.toLowerCase().includes(q) ||
        (item.host.machineId && item.host.machineId.toLowerCase().includes(q)) ||
        item.host.hostId.toLowerCase().includes(q)
      );
    } else {
      return (
        item.host.label.toLowerCase().includes(q) ||
        item.host.hostname.toLowerCase().includes(q) ||
        (item.host.username && item.host.username.toLowerCase().includes(q))
      );
    }
  });

  return (
    <section aria-label="Remote" className="space-y-5">
      <h1 className="text-xl font-semibold">Remote</h1>

      {!accountToken ? (
        <AccountSignIn
          origin={accountOrigin}
          onSignIn={(tok) => setAccountToken(tok)}
        />
      ) : (
        <div
          data-testid="account-status-bar"
          className="rounded-lg border border-border bg-card p-4 space-y-3"
        >
          <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <div className="text-xs font-semibold text-foreground flex items-center gap-1.5">
                <Badge variant="outline" className="text-[10px] px-1.5 py-0 h-4 bg-primary/10 text-primary border-primary/30">
                  Account Active
                </Badge>
                <span>Ferryx Account</span>
              </div>
              <p className="text-[11px] text-muted-foreground mt-0.5">
                Enrolled machines on this account are automatically listed below.
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={isIssuingCode}
                onClick={handleIssueEnrollmentCode}
              >
                <KeyRound className="mr-1 size-3.5" />
                {isIssuingCode ? "Issuing…" : "Issue Enrollment Code"}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={handleSignOut}
                aria-label="Sign Out"
              >
                <LogOut className="mr-1 size-3.5" />
                Sign Out
              </Button>
            </div>
          </div>

          {issuedCode ? (
            <div
              data-testid="issued-enrollment-card"
              className="rounded-md border border-primary/40 bg-accent/20 p-3 space-y-2 text-xs"
            >
              <div className="flex items-center justify-between">
                <span className="font-semibold text-foreground">
                  Enrollment Code Issued: <span className="font-mono text-primary font-bold">{issuedCode}</span>
                </span>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="size-6 p-0"
                  onClick={() => setIssuedCode(null)}
                >
                  <X className="size-3.5" />
                </Button>
              </div>
              <p className="text-[11px] text-muted-foreground">
                Run this command on the headless machine to enroll it:
              </p>
              <div className="flex items-center gap-2">
                <code
                  data-testid="enrollment-command"
                  className="block flex-1 rounded bg-muted/80 px-2.5 py-1.5 font-mono text-[11px] text-foreground select-all break-all border border-border"
                >
                  {`ferryx-cli account enroll --code ${issuedCode} --origin ${accountOrigin}`}
                </code>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-7 text-xs shrink-0"
                  onClick={() => {
                    void copyTextToClipboard(
                      `ferryx-cli account enroll --code ${issuedCode} --origin ${accountOrigin}`,
                    );
                    setCopiedEnrollCommand(true);
                    setTimeout(() => setCopiedEnrollCommand(false), 2000);
                  }}
                >
                  {copiedEnrollCommand ? (
                    <>
                      <Check className="mr-1 size-3" /> Copied
                    </>
                  ) : (
                    <>
                      <Copy className="mr-1 size-3" /> Copy
                    </>
                  )}
                </Button>
              </div>
            </div>
          ) : null}

          {accountMachines.length === 0 && !accountMachinesLoading ? (
            <p className="text-[11px] text-muted-foreground">
              No account machines enrolled yet. The other machine has to enroll first.
            </p>
          ) : null}
        </div>
      )}

      {(
        <div className="space-y-4">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <p className="text-sm text-muted-foreground">Connect to another machine.</p>
            </div>
            {/* EXACTLY ONE "Add Machine" button in the toolbar */}
            <div className="flex items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                aria-label="Add Machine"
                onClick={() => setIsAddModalOpen(true)}
              >
                <Plus className="mr-1 size-3.5" />
                Add Machine
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                aria-label="Refresh machines"
                onClick={handleRefreshAll}
              >
                Refresh
              </Button>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <Input
              aria-label="Search machines"
              placeholder="Search machines…"
              value={searchQuery}
              onChange={e => setSearchQuery(e.target.value)}
              className="h-8 w-64 text-xs"
            />
          </div>

          {actionError ? (
            <div role="alert" className="flex items-center gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-2.5 text-xs text-destructive">
              <AlertCircle className="size-4 shrink-0" />
              <span>{actionError}</span>
            </div>
          ) : null}

          {/* ONE mixed machine list */}
          {filteredItems.length === 0 ? (
            <div className="flex flex-col items-center justify-center rounded-lg border border-dashed border-border p-8 text-center">
              <Server className="size-8 text-muted-foreground/50 mb-2" />
              <p className="text-sm font-medium text-foreground">
                {mixedItems.length === 0 ? "No remote machines configured" : "No matching machines found"}
              </p>
              <p className="text-xs text-muted-foreground mt-1">
                {mixedItems.length === 0
                  ? "The other machine has to enroll first. Issue an enrollment code above to enroll a machine, or use Add Machine to connect via SSH."
                  : "Try clearing your search query to see all machines."}
              </p>
            </div>
          ) : (
            <div role="list" aria-label="Remote machines" className="space-y-3">
              {filteredItems.map(item => {
                const namespacedKey = `${item.kind}:${item.id}`;
                const isSelected = selectedKey === namespacedKey;
                const isExpanded = expandedDetails.has(namespacedKey);

                if (item.kind === "paired") {
                  const host = item.host;
                  const result = checks[host.hostId];
                  const isEnrolledAccountMachine = accountMachines.some(
                    (m) => m.machineId === host.machineId,
                  );
                  const code =
                    state.nativeStatus !== "ready" && !isEnrolledAccountMachine
                      ? "NATIVE_CONTEXT_REQUIRED"
                      : host.authStatus !== "paired" || host.grantScope !== "machine"
                      ? "MACHINE_GRANT_REQUIRED"
                      : !host.online
                      ? "OFFLINE"
                      : result?.generation === host.generation
                      ? result.code
                      : isEnrolledAccountMachine
                      ? "READY"
                      : "UNCHECKED";

                  return (
                    <div
                      key={namespacedKey}
                      tabIndex={-1}
                      ref={node => {
                        if (node) rowRefs.current.set(namespacedKey, node);
                        else rowRefs.current.delete(namespacedKey);
                      }}
                      data-machine-id={namespacedKey}
                      data-selected={isSelected}
                      className={`rounded-lg border transition-colors p-3.5 space-y-2 ${
                        isSelected
                          ? "border-primary/60 bg-accent/30"
                          : "border-border bg-card/50 hover:bg-card/80"
                      }`}
                    >
                      {/* Common row: Name, Status, Add Project, Details */}
                      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                        <div className="flex items-center gap-2 min-w-0">
                          <span className="text-sm font-medium text-foreground truncate">
                            {host.name}
                            {host.name === DEFAULT_MACHINE_LABEL && host.machineId
                              ? ` · ${host.machineId.slice(-8)}`
                              : ""}
                          </span>
                          <Badge variant="outline" className="text-[10px] px-1.5 py-0 h-4 shrink-0">
                            Paired
                          </Badge>
                          {/* Concise status on row */}
                          <span
                            role="status"
                            data-testid="machine-status"
                            data-code={code}
                            title={explanations[code]}
                            className={`text-xs font-medium shrink-0 ${
                              code === "READY"
                                ? "text-status-success"
                                : code === "OFFLINE"
                                ? "text-muted-foreground"
                                : "text-status-warning"
                            }`}
                          >
                            {concisePairedStatus[code] ?? "Unchecked"}
                          </span>
                        </div>

                        <div className="flex items-center gap-2 shrink-0">
                          {/* Disable paired Add Project without onOpenProject even if only onOpenSshProject set */}
                          <Button
                            type="button"
                            size="sm"
                            aria-label={`Add Project on ${host.name}`}
                            disabled={busy || code !== "READY" || !onOpenProject}
                            onClick={() =>
                              handleOpenProject({
                                kind: "pairedDaemon",
                                hostId: host.hostId,
                                generation: host.generation!,
                              })
                            }
                          >
                            Add Project
                          </Button>
                          <Button
                            type="button"
                            variant="outline"
                            size="sm"
                            aria-label={`Details for ${host.name}`}
                            aria-expanded={isExpanded}
                            onClick={() => toggleDetails(namespacedKey)}
                          >
                            Details
                            {isExpanded ? (
                              <ChevronUp className="ml-1 size-3.5" />
                            ) : (
                              <ChevronDown className="ml-1 size-3.5" />
                            )}
                          </Button>
                        </div>
                      </div>

                      {/* Technical metadata hidden in details, wrapping long items */}
                      {isExpanded ? (
                        <div className="pt-2 border-t border-border/60 text-xs space-y-3">
                          {code !== "READY" && explanations[code] ? (
                            <p className="text-xs text-muted-foreground">{explanations[code]}</p>
                          ) : null}

                          <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-2 text-muted-foreground break-all">
                            <div>
                              <span className="font-semibold text-foreground/80">Machine ID:</span>{" "}
                              <span className="font-mono">{host.machineId}</span>
                            </div>
                            <div>
                              <span className="font-semibold text-foreground/80">Relay:</span>{" "}
                              <span className="font-mono">{host.relayOrigin}</span>
                            </div>
                            <div>
                              <span className="font-semibold text-foreground/80">Transport:</span>{" "}
                              <span>{host.transport}</span>
                            </div>
                            <div>
                              <span className="font-semibold text-foreground/80">Generation:</span>{" "}
                              <span>{host.generation ?? "unknown"}</span>
                            </div>
                            <div>
                              <span className="font-semibold text-foreground/80">Grant:</span>{" "}
                              <span>{host.grantScope ?? "unknown"}</span>
                            </div>
                            <div>
                              <span className="font-semibold text-foreground/80">Auth Status:</span>{" "}
                              <span>{host.authStatus}</span>
                            </div>
                          </div>

                          <div className="flex flex-wrap gap-2 pt-1">
                            <Button
                              type="button"
                              variant="outline"
                              size="sm"
                              aria-label={`Check capabilities for ${host.name}`}
                              disabled={busy || !host.generation || state.nativeStatus !== "ready"}
                              onClick={() => void handleCheckPaired(host)}
                            >
                              Check capabilities
                            </Button>
                            <Button
                              type="button"
                              variant="outline"
                              size="sm"
                              aria-label={`Re-pair ${host.name}`}
                              disabled={busy}
                              onClick={() => setRepairHostId(host.hostId)}
                            >
                              Re-pair
                            </Button>
                            <Button
                              type="button"
                              variant="outline"
                              size="sm"
                              aria-label={`Forget ${host.name}`}
                              disabled={busy || !host.generation}
                              onClick={() =>
                                setConfirmForget({
                                  hostId: host.hostId,
                                  generation: host.generation!,
                                })
                              }
                            >
                              Forget credentials
                            </Button>
                          </div>

                          {repairHostId === host.hostId ? (
                            <p role="status" data-code="REPAIR_REQUIRES_IDENTITY_SUPPORT" className="text-xs text-muted-foreground">
                              Obtain a fresh machine-access PIN from the owner. Identity-checked re-pair is not supported by this desktop API yet. No credentials have been replaced. You can explicitly forget credentials and pair a new connection; verify the returned machine identity.
                            </p>
                          ) : null}

                          {confirmForget?.hostId === host.hostId ? (
                            <div role="group" aria-label="Confirm credential removal" className="space-y-2 rounded border border-border/80 bg-accent/20 p-3">
                              <p className="text-xs text-status-warning">
                                Forget credentials for {host.name}? Saved project/layout references and files remain. This does not unregister projects, close remote sessions, or revoke other devices. Re-pair to reconnect.
                              </p>
                              <div className="flex gap-2">
                                <Button
                                  type="button"
                                  variant="destructive"
                                  size="sm"
                                  disabled={busy}
                                  aria-label={`Confirm forget ${host.name}`}
                                  onClick={() => void handleConfirmForgetPaired()}
                                >
                                  Confirm forget
                                </Button>
                                <Button
                                  type="button"
                                  variant="outline"
                                  size="sm"
                                  disabled={busy}
                                  aria-label="Cancel forget"
                                  onClick={() => setConfirmForget(null)}
                                >
                                  Cancel
                                </Button>
                              </div>
                            </div>
                          ) : null}
                        </div>
                      ) : null}
                    </div>
                  );
                } else {
                  // SSH machine
                  const host = item.host;
                  const testState = sshTestResults[host.id];
                  const isPrepared = sshPreparedHosts[host.id];
                  const code = host.disabled
                    ? "DISABLED"
                    : testState?.testing
                    ? "TESTING"
                    : testState?.reachable === true
                    ? isPrepared
                      ? "READY"
                      : "REACHABLE"
                    : testState?.reachable === false
                    ? "ERROR"
                    : "UNCHECKED";

                  const statusText = host.disabled
                    ? "Disabled"
                    : testState?.testing
                    ? "Testing…"
                    : testState?.reachable === true
                    ? isPrepared
                      ? "Ready"
                      : "Reachable"
                    : testState?.reachable === false
                    ? "Failed"
                    : "Not checked";

                  const isEditingThis = editingSshHost?.id === host.id;

                  return (
                    <div
                      key={namespacedKey}
                      tabIndex={-1}
                      ref={node => {
                        if (node) rowRefs.current.set(namespacedKey, node);
                        else rowRefs.current.delete(namespacedKey);
                      }}
                      data-machine-id={namespacedKey}
                      data-selected={isSelected}
                      className={`rounded-lg border transition-colors p-3.5 space-y-2 ${
                        isSelected
                          ? "border-primary/60 bg-accent/30"
                          : "border-border bg-card/50 hover:bg-card/80"
                      }`}
                    >
                      {/* Common row: Name, Status, Add Project, Details */}
                      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                        <div className="flex items-center gap-2 min-w-0">
                          <span className="text-sm font-medium text-foreground truncate">
                            {host.label}
                          </span>
                          <Badge variant="outline" className="text-[10px] px-1.5 py-0 h-4 shrink-0">
                            SSH
                          </Badge>
                          {/* Concise status on row */}
                          <span
                            role="status"
                            data-testid="machine-status"
                            data-code={code}
                            title={testState?.error || statusText}
                            className={`text-xs font-medium shrink-0 ${
                              code === "READY" || code === "REACHABLE"
                                ? "text-status-success"
                                : code === "DISABLED" || code === "UNCHECKED"
                                ? "text-muted-foreground"
                                : "text-status-warning"
                            }`}
                          >
                            {statusText}
                          </span>
                        </div>

                        <div className="flex items-center gap-2 shrink-0">
                          <Button
                            type="button"
                            size="sm"
                            aria-label={`Add Project on ${host.label}`}
                            disabled={busy || Boolean(host.disabled) || (!onOpenProject && !onOpenSshProject)}
                            onClick={() => handleOpenProject({ kind: "ssh", hostId: host.id })}
                          >
                            Add Project
                          </Button>
                          <Button
                            type="button"
                            variant="outline"
                            size="sm"
                            aria-label={`Details for ${host.label}`}
                            aria-expanded={isExpanded}
                            onClick={() => toggleDetails(namespacedKey)}
                          >
                            Details
                            {isExpanded ? (
                              <ChevronUp className="ml-1 size-3.5" />
                            ) : (
                              <ChevronDown className="ml-1 size-3.5" />
                            )}
                          </Button>
                        </div>
                      </div>

                      {/* Technical metadata hidden in details, wrapping long items */}
                      {isExpanded ? (
                        <div className="pt-2 border-t border-border/60 text-xs space-y-3">
                          {isEditingThis && editFormData ? (
                            <form onSubmit={handleSaveEditSsh} className="space-y-3 p-3 rounded bg-muted/30 border border-border">
                              <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                                <div className="space-y-1">
                                  <Label className="text-xs">Label</Label>
                                  <Input
                                    aria-label="Label"
                                    value={editFormData.label}
                                    onChange={e => setEditFormData(f => f && { ...f, label: e.target.value })}
                                    className="h-7 text-xs"
                                    required
                                  />
                                </div>
                                <div className="space-y-1">
                                  <Label className="text-xs">Hostname</Label>
                                  <Input
                                    aria-label="Hostname"
                                    value={editFormData.hostname}
                                    onChange={e => setEditFormData(f => f && { ...f, hostname: e.target.value })}
                                    className="h-7 text-xs"
                                    required
                                  />
                                </div>
                              </div>

                              <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                                <div className="space-y-1">
                                  <Label className="text-xs">Username</Label>
                                  <Input
                                    aria-label="Username"
                                    value={editFormData.username}
                                    onChange={e => setEditFormData(f => f && { ...f, username: e.target.value })}
                                    className="h-7 text-xs"
                                  />
                                </div>
                                <div className="space-y-1">
                                  <Label className="text-xs">Port</Label>
                                  <Input
                                    aria-label="Port"
                                    value={editFormData.port}
                                    onChange={e => setEditFormData(f => f && { ...f, port: e.target.value })}
                                    className="h-7 text-xs"
                                  />
                                </div>
                              </div>

                              {/* Collapsible advanced options in edit mode */}
                              <div>
                                <button
                                  type="button"
                                  className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground font-medium"
                                  onClick={() => setShowAdvancedEditSsh(v => !v)}
                                >
                                  {showAdvancedEditSsh ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
                                  {showAdvancedEditSsh ? "Hide Advanced Options" : "Show Advanced Options (Key, Agent, Jump Host)"}
                                </button>
                              </div>

                              {showAdvancedEditSsh ? (
                                <div className="space-y-2 pt-1 border-t border-border/50">
                                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                                    <div className="space-y-1">
                                      <Label className="text-xs">Auth Method</Label>
                                      <Select
                                        value={editFormData.authMethod}
                                        onValueChange={(val: SshAuthMethod) =>
                                          setEditFormData(f => f && { ...f, authMethod: val })
                                        }
                                      >
                                        <SelectTrigger className="h-7 text-xs" aria-label="Auth Method">
                                          <SelectValue />
                                        </SelectTrigger>
                                        <SelectContent>
                                          <SelectItem value="agent">SSH Agent</SelectItem>
                                          <SelectItem value="key">Key File</SelectItem>
                                          <SelectItem value="password">Password</SelectItem>
                                        </SelectContent>
                                      </Select>
                                      {editFormData.authMethod === "password" ? <div className="space-y-1 pt-2">
                                        <Label htmlFor="edit-ssh-password" className="text-xs">SSH password</Label>
                                        <Input id="edit-ssh-password" type="password" autoComplete="off"
                                          value={editPassword} onChange={e => setEditPassword(e.target.value)}
                                          placeholder="Enter password to authenticate again" />
                                      </div> : null}
                                    </div>
                                    <div className="space-y-1">
                                      <Label className="text-xs">Identity File</Label>
                                      <Input
                                        aria-label="Identity File"
                                        value={editFormData.identityFile}
                                        onChange={e => setEditFormData(f => f && { ...f, identityFile: e.target.value })}
                                        className="h-7 text-xs"
                                        placeholder="~/.ssh/id_ed25519"
                                      />
                                    </div>
                                  </div>
                                  <div className="space-y-1">
                                    <Label className="text-xs">Jump Host</Label>
                                    <Input
                                      aria-label="Jump Host"
                                      value={editFormData.jumpHost}
                                      onChange={e => setEditFormData(f => f && { ...f, jumpHost: e.target.value })}
                                      className="h-7 text-xs"
                                      placeholder="bastion.corp.net"
                                    />
                                  </div>
                                </div>
                              ) : null}

                              <div className="flex justify-end gap-2 pt-1">
                                <Button
                                  type="button"
                                  variant="outline"
                                  size="sm"
                                  onClick={() => { setEditingSshHost(null); setEditFormData(null); }}
                                >
                                  Cancel
                                </Button>
                                <Button type="submit" size="sm" disabled={busy}>
                                  Save Changes
                                </Button>
                              </div>
                            </form>
                          ) : (
                            <>
                              <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-2 text-muted-foreground break-all">
                                <div>
                                  <span className="font-semibold text-foreground/80">Hostname:</span>{" "}
                                  <span className="font-mono">{host.hostname}</span>
                                </div>
                                <div>
                                  <span className="font-semibold text-foreground/80">Port:</span>{" "}
                                  <span>{host.port ?? 22}</span>
                                </div>
                                <div>
                                  <span className="font-semibold text-foreground/80">Username:</span>{" "}
                                  <span>{host.username ?? "(default)"}</span>
                                </div>
                                <div>
                                  <span className="font-semibold text-foreground/80">Auth method:</span>{" "}
                                  <span>{host.authMethod}</span>
                                </div>
                                {host.identityFile ? (
                                  <div>
                                    <span className="font-semibold text-foreground/80">Identity file:</span>{" "}
                                    <span className="font-mono">{host.identityFile}</span>
                                  </div>
                                ) : null}
                                {host.jumpHost ? (
                                  <div>
                                    <span className="font-semibold text-foreground/80">Jump host:</span>{" "}
                                    <span>{host.jumpHost}</span>
                                  </div>
                                ) : null}
                              </div>

                              {testState?.error ? (
                                <p className="text-xs text-destructive break-words">{testState.error}</p>
                              ) : null}

                              {testState?.environment ? (
                                <div
                                  data-testid={`ssh-runtime-${host.id}`}
                                  data-platform={testState.environment.platform}
                                  data-git={String(testState.environment.git)}
                                  className="space-y-0.5 rounded border border-border/60 bg-muted/20 p-2 text-[11px] break-all"
                                >
                                  <div>
                                    <span className="font-semibold">Platform:</span>{" "}
                                    {testState.environment.platform} ({testState.environment.executor})
                                  </div>
                                  <div>
                                    <span className="font-semibold">Git:</span>{" "}
                                    {testState.environment.git ? "Installed" : "Not installed"}
                                  </div>
                                  <div>
                                    <span className="font-semibold">Home:</span> {testState.environment.home}
                                  </div>
                                </div>
                              ) : null}

                              <div className="flex flex-wrap gap-2 pt-1">
                                <Button
                                  type="button"
                                  variant="outline"
                                  size="sm"
                                  aria-label={`Test connection to ${host.label}`}
                                  disabled={busy || testState?.testing}
                                  onClick={() => void handleTestSsh(host)}
                                >
                                  Test connection
                                </Button>
                                <Button
                                  type="button"
                                  variant="outline"
                                  size="sm"
                                  aria-label={`Prepare agent integration on ${host.label}`}
                                  disabled={busy || Boolean(host.disabled)}
                                  onClick={() => void handlePrepareSsh(host)}
                                >
                                  Prepare agent integration
                                </Button>
                                <Button
                                  type="button"
                                  variant="outline"
                                  size="sm"
                                  aria-label={`Edit ${host.label}`}
                                  disabled={busy}
                                  onClick={() => handleStartEditSsh(host)}
                                >
                                  Edit
                                </Button>
                                <Button
                                  type="button"
                                  variant="outline"
                                  size="sm"
                                  aria-label={`Toggle disable ${host.label}`}
                                  disabled={busy}
                                  onClick={() => void handleToggleDisableSsh(host)}
                                >
                                  {host.disabled ? "Enable" : "Disable"}
                                </Button>
                                <Button
                                  type="button"
                                  variant="outline"
                                  size="sm"
                                  aria-label={`Delete ${host.label}`}
                                  disabled={busy}
                                  onClick={() => void handleDeleteSsh(host.id)}
                                >
                                  Delete
                                </Button>
                              </div>
                            </>
                          )}
                        </div>
                      ) : null}
                    </div>
                  );
                }
              })}
            </div>
          )}
        </div>
      )}
      <details open={context.page === "access"} className="rounded-lg border border-border p-4">
        <summary className="cursor-pointer text-sm font-medium">Access to This Machine</summary>
        <RemoteAccessSection />
      </details>
      <details open={context.page === "details"} className="rounded-lg border border-border p-4">
        <summary className="cursor-pointer text-sm font-medium">Connection diagnostics</summary>
        <div className="space-y-4">
          <dl className="text-sm">
            <dt className="font-semibold">Native inventory</dt>
            <dd className="text-muted-foreground">{state.nativeStatus}</dd>
            <dt className="font-semibold mt-2">Credential migration</dt>
            <dd className="text-muted-foreground">{state.migrationStatus}</dd>
          </dl>
          <RemoteAccessSection detailsOnly />
        </div>
      </details>

      {/* ONE Add Machine opens real unified modal with SSH and Config Import */}
      <AddMachineModal
        isOpen={isAddModalOpen}
        onClose={() => setIsAddModalOpen(false)}
        inventory={inventory}
        store={store}
        negotiate={negotiate}
        initialRelayOrigin={accountOrigin}
        onSuccess={result => {
          if (result.kind === "pairedDaemon") {
            // Propagate verified generation-bound state
            setChecks(prev => ({
              ...prev,
              [result.hostId]: { generation: result.generation, code: "READY" },
            }));
            setSelectedKey(`paired:${result.hostId}`);
          } else if (result.kind === "ssh") {
            // Propagate verified SSH connection test result
            setSshTestResults(prev => ({
              ...prev,
              [result.hostId]: {
                testing: false,
                reachable: result.summary.reachable,
                environment: result.summary.environment,
              },
            }));
            setSelectedKey(`ssh:${result.hostId}`);
          }
        }}
        onOfferProject={target => {
          handleOpenProject(target);
        }}
        onImportDone={importedHostId => {
          // Select newly imported host; status remains UNCHECKED until user tests it
          setSelectedKey(`ssh:${importedHostId}`);
        }}
      />
    </section>
  );
}
