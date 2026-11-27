import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import {
  clearSshPassword,
  deleteSshHost,
  extractIpcErrorMessage,
  prepareSshIntegration,
  removeSshIntegration,
  setSshPassword,
  testSshConnection,
  updateSshHost,
  type SshHost,
} from "../../../lib/sshHosts";
import type { HostFormData } from "../AddMachineModal";
import type { SshTestState } from "./MachineDrawer";
import { sameConnection } from "./remoteModel";
import { useHostTunnels, type HostTunnelsSource } from "./useHostTunnels";

/** Outcome of a credential action, shown beside the button that ran it. */
export interface SshPasswordFeedback {
  tone: "success" | "error";
  message: string;
}

/** SSH machine state and actions of the Remote section, moved out of RemoteSection unchanged. */
export function useSshActions({ sshHosts, busy, setBusy, setActionError, capabilities, tunnelSectionOpen, tunnelSource }: {
  sshHosts: SshHost[];
  busy: boolean;
  setBusy: (busy: boolean) => void;
  setActionError: (error: string | null, kind?: string | null) => void;
  /**
   * The capabilities the connected daemon advertises. `undefined` means "not known yet" and
   * is treated exactly like an absent `sshTunnelV1`: the tunnel section explains itself and
   * sends no request, which is the safe reading for an older background service.
   */
  capabilities?: readonly string[] | null;
  /** Whether an SSH drawer is open, i.e. whether the tunnel section can be read at all. */
  tunnelSectionOpen: boolean;
  /**
   * The snapshot the section already reads. Passing it makes the editor render that response
   * instead of fetching its own copy, so one drawer costs one `cmd_ssh_tunnel_list`.
   */
  tunnelSource?: HostTunnelsSource;
}) {
  const hostsRef = useRef(sshHosts);
  // SSH host states
  const [sshTestResults, setSshTestResults] = useState<Record<string, SshTestState>>({});
  const [sshPreparedHosts, setSshPreparedHosts] = useState<Record<string, boolean>>({});
  const [sshRemovalResults, setSshRemovalResults] = useState<Record<string, string>>({});
  const [sshPasswordFeedback, setSshPasswordFeedback] = useState<Record<string, SshPasswordFeedback>>({});
  const [editingSshHost, setEditingSshHost] = useState<SshHost | null>(null);
  const [editFormData, setEditFormData] = useState<HostFormData | null>(null);
  const [editPassword, setEditPassword] = useState("");
  const [showAdvancedEditSsh, setShowAdvancedEditSsh] = useState(false);

  const [confirmDeleteSshId, setConfirmDeleteSshId] = useState<string | null>(null);

  // Every request captures this counter when it starts. Opening another machine's drawer bumps
  // it, so a response that belongs to the previous drawer can no longer write the shared
  // `actionError` or the edit form — the user would otherwise see Machine A's failure in
  // Machine B's drawer. Per-host results stay keyed by host id and are guarded by
  // `isCurrentHost` instead, so they survive a drawer switch.
  const operationRef = useRef(0);
  const beginOperation = () => operationRef.current;
  const isCurrentOperation = (operation: number) => operationRef.current === operation;

  // Saved tunnel definitions: the editor is the only reader, and it sends nothing while the
  // capability is absent (MH-10). The hook owns its own snapshot, never a host-store copy —
  // unless the section already read one, in which case this renders that one.
  const { editor: tunnels } = useHostTunnels({ capabilities, active: tunnelSectionOpen, source: tunnelSource });

  // A connection change invalidates any in-flight test or prepare result for that host.
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

  /**
   * Drops everything the drawer was showing for the machine it just left. Called when the
   * drawer closes or switches hosts; bumping the counter also orphans any request still in
   * flight for the previous machine.
   */
  const resetTransientState = useCallback(() => {
    // Bumping the counter orphans the previous drawer's requests; clearing `busy` releases the
    // flag those requests would otherwise have cleared from their own `finally`. Without this,
    // abandoning an action by closing or switching drawers leaves the next drawer's first
    // action blocked behind a `busy` nobody will ever reset.
    operationRef.current += 1;
    setBusy(false);
    setEditingSshHost(null);
    setEditFormData(null);
    setEditPassword("");
    setShowAdvancedEditSsh(false);
    setConfirmDeleteSshId(null);
  }, [setBusy]);

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
    const operation = beginOperation();
    setBusy(true);
    setActionError(null);
    try {
      await prepareSshIntegration(host);
      if (!isCurrentHost(host)) return;
      setSshPreparedHosts(prev => ({ ...prev, [host.id]: true }));
    } catch (err) {
      if (!isCurrentHost(host) || !isCurrentOperation(operation)) return;
      setActionError(extractIpcErrorMessage(err, "Agent integration preparation failed."));
    } finally {
      if (isCurrentOperation(operation)) setBusy(false);
    }
  };

  const handleRemoveSshIntegration = async (host: SshHost) => {
    if (busy) return;
    const operation = beginOperation();
    setBusy(true);
    setActionError(null);
    try {
      const result = await removeSshIntegration(host.id);
      if (!isCurrentHost(host)) return;
      setSshPreparedHosts(prev => ({ ...prev, [host.id]: false }));
      setSshRemovalResults(prev => ({
        ...prev,
        [host.id]: `Removed ${result.removed.length}, kept ${result.keptModified.length} modified`,
      }));
    } catch (err) {
      if (!isCurrentHost(host) || !isCurrentOperation(operation)) return;
      setActionError(extractIpcErrorMessage(err, "Agent integration removal failed."));
    } finally {
      if (isCurrentOperation(operation)) setBusy(false);
    }
  };

  const handleToggleDisableSsh = async (host: SshHost) => {
    if (busy) return;
    const operation = beginOperation();
    setBusy(true);
    setActionError(null);
    try {
      await updateSshHost({ ...host, disabled: !host.disabled });
    } catch (err) {
      if (!isCurrentOperation(operation)) return;
      setActionError(extractIpcErrorMessage(err, "Failed to update SSH machine."));
    } finally {
      if (isCurrentOperation(operation)) setBusy(false);
    }
  };

  const handleDeleteSsh = async (hostId: string) => {
    if (busy) return;
    const operation = beginOperation();
    setBusy(true);
    setActionError(null);
    try {
      await deleteSshHost(hostId);
      if (!isCurrentOperation(operation)) return;
      setConfirmDeleteSshId(null);
    } catch (err) {
      if (!isCurrentOperation(operation)) return;
      setActionError(extractIpcErrorMessage(err, "Failed to delete SSH machine."));
    } finally {
      if (isCurrentOperation(operation)) setBusy(false);
    }
  };

  /** Removes the stored password without touching the rest of the host or its saved project. */
  const handleClearSshPassword = async (host: SshHost) => {
    if (busy) return;
    const operation = beginOperation();
    setBusy(true);
    setActionError(null);
    setSshPasswordFeedback(prev => {
      const next = { ...prev };
      delete next[host.id];
      return next;
    });
    try {
      await clearSshPassword(host);
      if (!isCurrentOperation(operation)) return;
      setSshPasswordFeedback(prev => ({ ...prev, [host.id]: { tone: "success", message: "Saved password cleared." } }));
      // The credential changed, so any earlier test result no longer describes this host.
      setSshTestResults(prev => {
        const next = { ...prev };
        delete next[host.id];
        return next;
      });
    } catch (err) {
      if (!isCurrentOperation(operation)) return;
      setSshPasswordFeedback(prev => ({
        ...prev,
        [host.id]: { tone: "error", message: extractIpcErrorMessage(err, "Failed to clear the saved password.") },
      }));
    } finally {
      if (isCurrentOperation(operation)) setBusy(false);
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
      shell: host.shell ?? "",
    });
    setShowAdvancedEditSsh(Boolean(host.identityFile || host.jumpHost || host.shell || host.authMethod !== "agent"));
  };

  const handleSaveEditSsh = async (e: FormEvent) => {
    e.preventDefault();
    if (!editingSshHost || !editFormData || busy) return;

    if (!editFormData.hostname.trim()) {
      setActionError("Hostname is required.");
      return;
    }
    const portNum = editFormData.port.trim() ? Number(editFormData.port) : 22;
    if (isNaN(portNum) || portNum < 1 || portNum > 65535 || !Number.isInteger(portNum)) {
      setActionError("Port must be an integer between 1 and 65535.");
      return;
    }
    const switchingToPassword = editFormData.authMethod === "password" && editingSshHost.authMethod !== "password";
    const newPassword = editFormData.authMethod === "password" ? editPassword : "";
    if (switchingToPassword && !newPassword) {
      // Saving here would store a password host with no credential and leave the machine unusable.
      setActionError("Enter the SSH password to switch this machine to password authentication.", "password-required");
      return;
    }

    const operation = beginOperation();
    setBusy(true);
    setActionError(null);

    const updated: SshHost = {
      ...editingSshHost,
      label: editFormData.label.trim() || editFormData.hostname.trim(),
      hostname: editFormData.hostname.trim(),
      username: editFormData.username.trim() || undefined,
      port: portNum,
      identityFile: editFormData.identityFile.trim() || undefined,
      jumpHost: editFormData.jumpHost.trim() || undefined,
      shell: editFormData.shell.trim() || undefined,
      authMethod: editFormData.authMethod,
    };

    try {
      if (newPassword) {
        // Verify the new credential against the machine before anything is stored: a failed
        // attempt must never replace the password that currently works.
        const summary = await testSshConnection(updated, newPassword);
        if (!isCurrentOperation(operation)) return;
        if (!summary.reachable) {
          setActionError(summary.lastError || "SSH authentication failed.");
          return;
        }
        await updateSshHost(updated);
        if (!isCurrentOperation(operation)) return;
        await setSshPassword(updated, newPassword);
        if (!isCurrentOperation(operation)) return;
      } else {
        await updateSshHost(updated);
        if (!isCurrentOperation(operation)) return;
      }
      setEditPassword("");
      setEditingSshHost(null);
      setEditFormData(null);
    } catch (err) {
      if (!isCurrentOperation(operation)) return;
      setActionError(extractIpcErrorMessage(err, "Failed to save SSH machine."));
    } finally {
      if (isCurrentOperation(operation)) setBusy(false);
    }
  };

  return {
    sshTestResults, setSshTestResults, sshPreparedHosts, sshRemovalResults, sshPasswordFeedback,
    editingSshHost, setEditingSshHost, editFormData, setEditFormData, editPassword, setEditPassword,
    showAdvancedEditSsh, setShowAdvancedEditSsh, confirmDeleteSshId, setConfirmDeleteSshId,
    handleTestSsh, handlePrepareSsh, handleRemoveSshIntegration, handleToggleDisableSsh, handleDeleteSsh,
    handleStartEditSsh, handleSaveEditSsh, handleClearSshPassword, resetTransientState, tunnels,
  };
}
