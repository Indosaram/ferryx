import { useEffect, useRef, useState, type FormEvent } from "react";
import {
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

/** SSH machine state and actions of the Remote section, moved out of RemoteSection unchanged. */
export function useSshActions({ sshHosts, busy, setBusy, setActionError }: {
  sshHosts: SshHost[];
  busy: boolean;
  setBusy: (busy: boolean) => void;
  setActionError: (error: string | null) => void;
}) {
  const hostsRef = useRef(sshHosts);
  // SSH host states
  const [sshTestResults, setSshTestResults] = useState<Record<string, SshTestState>>({});
  const [sshPreparedHosts, setSshPreparedHosts] = useState<Record<string, boolean>>({});
  const [sshRemovalResults, setSshRemovalResults] = useState<Record<string, string>>({});
  const [editingSshHost, setEditingSshHost] = useState<SshHost | null>(null);
  const [editFormData, setEditFormData] = useState<HostFormData | null>(null);
  const [editPassword, setEditPassword] = useState("");
  const [showAdvancedEditSsh, setShowAdvancedEditSsh] = useState(false);

  const [confirmDeleteSshId, setConfirmDeleteSshId] = useState<string | null>(null);

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

  const handleRemoveSshIntegration = async (host: SshHost) => {
    if (busy) return;
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
      if (!isCurrentHost(host)) return;
      setActionError(extractIpcErrorMessage(err, "Agent integration removal failed."));
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
      setConfirmDeleteSshId(null);
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

  const handleSaveEditSsh = async (e: FormEvent) => {
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

  return {
    sshTestResults, setSshTestResults, sshPreparedHosts, sshRemovalResults,
    editingSshHost, setEditingSshHost, editFormData, setEditFormData, editPassword, setEditPassword,
    showAdvancedEditSsh, setShowAdvancedEditSsh, confirmDeleteSshId, setConfirmDeleteSshId,
    handleTestSsh, handlePrepareSsh, handleRemoveSshIntegration, handleToggleDisableSsh, handleDeleteSsh,
    handleStartEditSsh, handleSaveEditSsh,
  };
}
