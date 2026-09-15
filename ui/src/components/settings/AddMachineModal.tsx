import { useEffect, useRef, useState, type FormEvent } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  FileCode,
  KeyRound,
  Terminal,
  X,
} from "lucide-react";
import type { MachineProjectTarget } from "../../lib/machineNavigation";
import {
  DEFAULT_MACHINE_LABEL,
  DEFAULT_RELAY_ORIGIN,
  pairedHostInventory,
} from "../../lib/pairedHostInventory";
import {
  createPairedDaemonProjectAdapter,
  type PairedHostContext,
} from "../../lib/pairedDaemonProject";
import { remoteHostStore, type RemoteHostStore } from "../../state/remoteHostStore";
import {
  extractIpcErrorMessage,
  getSshConfigPathOverride,
  importSshConfig,
  readSystemSshConfig,
  setSshConfigPathOverride,
  setSshPassword,
  testSshConnection,
  updateSshHost,
  type SshAuthMethod,
  type SshHost,
  type SshRemoteEnvironment,
  type SystemSshConfig,
} from "../../lib/sshHosts";
import { safeRandomUUID } from "../../lib/uuid";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { Input } from "../ui/input";
import { Label } from "../ui/label";

export interface HostFormData {
  label: string;
  hostname: string;
  username: string;
  port: string;
  identityFile: string;
  jumpHost: string;
  authMethod: SshAuthMethod;
}

export const DEFAULT_SSH_FORM: HostFormData = {
  label: "",
  hostname: "",
  username: "",
  port: "22",
  identityFile: "",
  jumpHost: "",
  authMethod: "agent",
};

const explanations: Record<string, string> = {
  NATIVE_CONTEXT_REQUIRED: "Native host inventory is unavailable. Use the desktop app with a compatible local daemon; browser mirror access cannot manage machines.",
  PAIR_FAILED: "Could not pair. Check connectivity and daemon compatibility, obtain a fresh machine-access PIN, and retry.",
  MACHINE_GRANT_REQUIRED: "Needs machine access. A mirror PIN cannot authorize projects. Re-pair with an owner-issued machine PIN; revoked credentials cannot reconnect.",
  UNSUPPORTED_CAPABILITY: "This relay, remote daemon, or local daemon does not advertise the required machine capabilities. Upgrade compatible components.",
  STALE_HOST_GENERATION: "Credentials changed during this request. Refresh the inventory and check capabilities again.",
  PAIRED_HOST_UNAVAILABLE: "The native host operation failed. Check the relay and daemon versions, connectivity, and PIN scope, then retry. Saved projects have not been removed.",
  OFFLINE: "Machine is offline. Saved projects remain available in the workspace; reconnect to the owning daemon.",
  UNCHECKED: "Check the remote machine capabilities before adding a project.",
  READY: "Machine project capabilities verified.",
};

async function defaultNegotiate(context: PairedHostContext) {
  const result = await createPairedDaemonProjectAdapter(context).capabilities();
  if (!["directoryBrowseV1", "machineWorkspaceV1"].every(capability => result.capabilities.includes(capability))) {
    throw new Error("UNSUPPORTED_CAPABILITY");
  }
}

export type VerifiedMachineResult =
  | {
      kind: "pairedDaemon";
      hostId: string;
      generation: string;
      name: string;
    }
  | {
      kind: "ssh";
      hostId: string;
      name: string;
      summary: { reachable: boolean; environment?: SshRemoteEnvironment | null };
    };

export interface AddMachineModalProps {
  isOpen: boolean;
  onClose: () => void;
  inventory?: typeof pairedHostInventory;
  store?: RemoteHostStore;
  negotiate?: (context: PairedHostContext) => Promise<unknown>;
  onSuccess: (result: VerifiedMachineResult) => void;
  onOfferProject?: (target: MachineProjectTarget) => void;
  onImportDone?: (selectedHostId: string) => void;
}

export function AddMachineModal({
  isOpen,
  onClose,
  inventory = pairedHostInventory,
  store = remoteHostStore,
  negotiate = defaultNegotiate,
  onSuccess,
  onOfferProject,
  onImportDone,
}: AddMachineModalProps) {
  const [activeTab, setActiveTab] = useState<"pin" | "ssh" | "import">("pin");

  // PIN state
  const [pin, setPin] = useState("");

  // SSH state
  const [sshForm, setSshForm] = useState<HostFormData>(DEFAULT_SSH_FORM);
  const [sshPassword, setSshPasswordInput] = useState("");
  const [showAdvancedSsh, setShowAdvancedSsh] = useState(false);

  // Config Import state
  const [configText, setConfigText] = useState("");
  const [importSubMode, setImportSubMode] = useState<"system" | "paste">("paste");
  const [systemConfig, setSystemConfig] = useState<SystemSshConfig | null>(null);
  const [configPathOverride, setConfigPathOverrideState] = useState<string | null>(() =>
    getSshConfigPathOverride(),
  );

  // Status state
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [successTarget, setSuccessTarget] = useState<{ target: MachineProjectTarget; name: string } | null>(null);
  const [importResult, setImportResult] = useState<{ count: number; hosts: SshHost[] } | null>(null);

  // Focus trap & lifecycle refs
  const modalRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLElement | null>(null);
  const wasOpenRef = useRef(false);
  const isMountedRef = useRef(true);
  const isDismissedRef = useRef(false);

  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
      isDismissedRef.current = true;
    };
  }, []);

  // Capture trigger once per open, reset dismissed on open, focus via effect without sleep, and restore trigger focus on close
  useEffect(() => {
    if (isOpen) {
      isDismissedRef.current = false;
      if (!wasOpenRef.current) {
        wasOpenRef.current = true;
        triggerRef.current = document.activeElement as HTMLElement | null;
      }
      const input = modalRef.current?.querySelector<HTMLElement>(
        'input:not([disabled]), textarea:not([disabled]), [role="tab"][aria-selected="true"]',
      );
      input?.focus();
    } else {
      if (wasOpenRef.current) {
        wasOpenRef.current = false;
        triggerRef.current?.focus();
      }
    }
  }, [isOpen]);

  useEffect(() => {
    if (isOpen && (successTarget || importResult)) {
      modalRef.current?.querySelector<HTMLElement>("[data-result-action]")?.focus();
    }
  }, [isOpen, successTarget, importResult]);

  // Read system SSH config when modal opens or path changes
  useEffect(() => {
    if (!isOpen) return;
    void readSystemSshConfig(configPathOverride)
      .then(cfg => {
        if (isMountedRef.current && !isDismissedRef.current) {
          setSystemConfig(cfg);
          if (cfg && cfg.hosts.length > 0) {
            setImportSubMode("system");
          }
        }
      })
      .catch(() => {
        if (isMountedRef.current && !isDismissedRef.current) {
          setSystemConfig(null);
        }
      });
  }, [isOpen, configPathOverride]);

  if (!isOpen) return null;

  const handleClose = () => {
    if (busy) return;
    isDismissedRef.current = true;
    setError(null);
    setSuccessTarget(null);
    setImportResult(null);
    setPin("");
    setSshForm(DEFAULT_SSH_FORM);
    setSshPasswordInput("");
    setConfigText("");
    onClose();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      e.preventDefault();
      if (!busy) handleClose();
      return;
    }

    if (e.key === "Tab") {
      const focusable = modalRef.current?.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      );
      if (!focusable || focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];

      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  };

  const handlePairSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy || !pin.trim()) return;

    setBusy(true);
    setError(null);

    let pairedContext: PairedHostContext | undefined;
    try {
      const ok = await inventory.pair(
        { relayOrigin: DEFAULT_RELAY_ORIGIN, displayLabel: DEFAULT_MACHINE_LABEL, pin },
        host => { pairedContext = { hostId: host.hostId, generation: host.generation! }; },
      );
      if (isDismissedRef.current || !isMountedRef.current) return;

      if (!ok || !pairedContext) {
        setError(explanations.PAIR_FAILED);
        setBusy(false);
        return;
      }

      // Initial live auth/scope check
      let host = store.getState().hosts[pairedContext.hostId];
      if (!host || host.generation !== pairedContext.generation || host.authStatus !== "paired" || host.grantScope !== "machine") {
        setError(
          !host || host.generation !== pairedContext.generation
            ? explanations.STALE_HOST_GENERATION
            : explanations.MACHINE_GRANT_REQUIRED,
        );
        setBusy(false);
        return;
      }

      // Negotiate capabilities within modal
      await negotiate(pairedContext);
      if (isDismissedRef.current || !isMountedRef.current) return;

      // RECHECK live paired generation/auth after await negotiate and before project offer!
      host = store.getState().hosts[pairedContext.hostId];
      if (!host || host.generation !== pairedContext.generation || host.authStatus !== "paired" || host.grantScope !== "machine") {
        setError(
          !host || host.generation !== pairedContext.generation
            ? explanations.STALE_HOST_GENERATION
            : explanations.MACHINE_GRANT_REQUIRED,
        );
        setBusy(false);
        return;
      }

      // Verified! Clear PIN on success
      setPin("");
      const machineName = host.name || DEFAULT_MACHINE_LABEL;
      const target: MachineProjectTarget = {
        kind: "pairedDaemon",
        hostId: pairedContext.hostId,
        generation: pairedContext.generation,
      };

      setSuccessTarget({ target, name: machineName });
      onSuccess({
        kind: "pairedDaemon",
        hostId: pairedContext.hostId,
        generation: pairedContext.generation,
        name: machineName,
      });
    } catch (err) {
      if (isDismissedRef.current || !isMountedRef.current) return;
      const msg = err instanceof Error ? err.message : "";
      setError(explanations[msg] ?? (msg || explanations.PAIR_FAILED));
    } finally {
      if (isMountedRef.current) {
        setBusy(false);
      }
    }
  };

  const handleSshSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;

    if (!sshForm.label.trim()) {
      setError("Label is required.");
      return;
    }
    if (!sshForm.hostname.trim()) {
      setError("Hostname or IP is required.");
      return;
    }
    const portNumber = sshForm.port.trim() ? Number(sshForm.port) : 22;
    if (isNaN(portNumber) || portNumber < 1 || portNumber > 65535 || !Number.isInteger(portNumber)) {
      setError("Port must be an integer between 1 and 65535.");
      return;
    }

    setBusy(true);
    setError(null);

    const candidateHost: SshHost = {
      id: safeRandomUUID(),
      label: sshForm.label.trim(),
      hostname: sshForm.hostname.trim(),
      username: sshForm.username.trim() || undefined,
      port: portNumber,
      identityFile: sshForm.identityFile.trim() || undefined,
      jumpHost: sshForm.jumpHost.trim() || undefined,
      source: "manual",
      authMethod: sshForm.authMethod,
      disabled: false,
    };

    try {
      if (candidateHost.authMethod === "password") {
        await setSshPassword(candidateHost, sshPassword);
      }
      const summary = await testSshConnection(candidateHost);
      if (isDismissedRef.current || !isMountedRef.current) return;

      if (!summary.reachable) {
        setError(summary.lastError || "SSH connection test failed. Check host, credentials, and network reachability.");
        setBusy(false);
        return;
      }

      await updateSshHost(candidateHost);
      if (isDismissedRef.current || !isMountedRef.current) return;

      const target: MachineProjectTarget = { kind: "ssh", hostId: candidateHost.id };
      setSshPasswordInput("");
      setSuccessTarget({ target, name: candidateHost.label });
      onSuccess({
        kind: "ssh",
        hostId: candidateHost.id,
        name: candidateHost.label,
        summary: { reachable: true, environment: summary.environment },
      });
    } catch (err) {
      if (isDismissedRef.current || !isMountedRef.current) return;
      setError(extractIpcErrorMessage(err, "SSH connection test failed."));
    } finally {
      if (isMountedRef.current) {
        setBusy(false);
      }
    }
  };

  const handleImportTextSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy || !configText.trim()) return;

    setBusy(true);
    setError(null);

    try {
      const importedHosts = await importSshConfig(configText);
      if (isDismissedRef.current || !isMountedRef.current) return;

      if (Array.isArray(importedHosts) && importedHosts.length > 0) {
        // Imported entries remain UNCHECKED until actual check.
        // Clearly separate imported-not-verified result with NO ready/project offer.
        setImportResult({ count: importedHosts.length, hosts: importedHosts });
        onImportDone?.(importedHosts[0].id);
      } else {
        handleClose();
      }
    } catch (err) {
      if (isDismissedRef.current || !isMountedRef.current) return;
      setError(extractIpcErrorMessage(err, "Failed to import SSH configuration."));
      // Retains configText on failure!
    } finally {
      if (isMountedRef.current) {
        setBusy(false);
      }
    }
  };

  const handleImportSystemAll = async () => {
    if (busy || !systemConfig?.rawText) return;
    setBusy(true);
    setError(null);

    try {
      const importedHosts = await importSshConfig(systemConfig.rawText);
      if (isDismissedRef.current || !isMountedRef.current) return;

      if (Array.isArray(importedHosts) && importedHosts.length > 0) {
        setImportResult({ count: importedHosts.length, hosts: importedHosts });
        onImportDone?.(importedHosts[0].id);
      } else {
        handleClose();
      }
    } catch (err) {
      if (isDismissedRef.current || !isMountedRef.current) return;
      setError(extractIpcErrorMessage(err, "Failed to import system SSH configuration."));
    } finally {
      if (isMountedRef.current) {
        setBusy(false);
      }
    }
  };

  const handleChooseConfigFile = async () => {
    if (!isTauri() || busy) return;
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        title: "Select an SSH config file",
      });
      const chosen = Array.isArray(selected) ? selected[0] : selected;
      if (!chosen) return;
      setConfigPathOverrideState(chosen);
      setSshConfigPathOverride(chosen);
    } catch (err) {
      setError(extractIpcErrorMessage(err, "Failed to select configuration file."));
    }
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Add Machine"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onKeyDown={handleKeyDown}
    >
      <div
        ref={modalRef}
        className="relative w-full max-w-lg rounded-lg border border-border bg-card p-6 shadow-xl space-y-4 max-h-[90vh] overflow-y-auto"
      >
        <div className="flex items-center justify-between border-b border-border pb-3">
          <h2 className="text-base font-semibold">Add Machine</h2>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-label="Close"
            disabled={busy}
            className="size-7 p-0"
            onClick={handleClose}
          >
            <X className="size-4" />
          </Button>
        </div>

        {error ? (
          <div role="alert" className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-xs text-destructive">
            <AlertCircle className="size-4 shrink-0 mt-0.5" />
            <span className="break-words">{error}</span>
          </div>
        ) : null}

        {/* Success View for verified machines (PIN or SSH) */}
        {successTarget ? (
          <div className="space-y-4 py-4 text-center">
            <div className="flex justify-center">
              <CheckCircle2 className="size-10 text-emerald-500" />
            </div>
            <p className="text-sm font-medium text-foreground">
              Machine &ldquo;{successTarget.name}&rdquo; connected and verified successfully!
            </p>
            <p className="text-xs text-muted-foreground">
              The machine has been added to your inventory and capabilities have been verified.
            </p>
            <div className="flex justify-center gap-3 pt-2">
              <Button
                type="button"
                data-result-action
                onClick={() => {
                  onOfferProject?.(successTarget.target);
                  handleClose();
                }}
              >
                Add Project
              </Button>
              <Button type="button" variant="outline" onClick={handleClose}>
                Done
              </Button>
            </div>
          </div>
        ) : importResult ? (
          /* Separate Imported Result: clearly separated with NO ready/project offer */
          <div className="space-y-4 py-4 text-center">
            <div className="flex justify-center">
              <FileCode className="size-10 text-primary" />
            </div>
            <p className="text-sm font-medium text-foreground">
              Imported {importResult.count} machine{importResult.count === 1 ? "" : "s"} into inventory
            </p>
            <p className="text-xs text-muted-foreground max-w-sm mx-auto">
              Imported machines are recorded as unchecked. Test the connection from machine details before adding remote projects.
            </p>
            <div className="flex justify-center gap-3 pt-2">
              <Button type="button" data-result-action onClick={handleClose}>
                Done
              </Button>
            </div>
          </div>
        ) : (
          <>
            {/* Method tabs wrap cleanly on narrow screens */}
            <div
              role="tablist"
              aria-label="Connection methods"
              className="flex flex-wrap gap-1 border-b border-border pb-1 text-xs"
            >
              <button
                type="button"
                role="tab"
                aria-selected={activeTab === "pin"}
                disabled={busy}
                className={`flex items-center gap-1.5 px-3 py-2 font-medium border-b-2 transition-colors disabled:opacity-50 ${
                  activeTab === "pin"
                    ? "border-primary text-foreground"
                    : "border-transparent text-muted-foreground hover:text-foreground"
                }`}
                onClick={() => { setActiveTab("pin"); setError(null); }}
              >
                <KeyRound className="size-3.5" />
                Pair with PIN
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={activeTab === "ssh"}
                disabled={busy}
                className={`flex items-center gap-1.5 px-3 py-2 font-medium border-b-2 transition-colors disabled:opacity-50 ${
                  activeTab === "ssh"
                    ? "border-primary text-foreground"
                    : "border-transparent text-muted-foreground hover:text-foreground"
                }`}
                onClick={() => { setActiveTab("ssh"); setError(null); }}
              >
                <Terminal className="size-3.5" />
                Connect with SSH
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={activeTab === "import"}
                disabled={busy}
                className={`flex items-center gap-1.5 px-3 py-2 font-medium border-b-2 transition-colors disabled:opacity-50 ${
                  activeTab === "import"
                    ? "border-primary text-foreground"
                    : "border-transparent text-muted-foreground hover:text-foreground"
                }`}
                onClick={() => { setActiveTab("import"); setError(null); }}
              >
                <FileCode className="size-3.5" />
                Import SSH Config
              </button>
            </div>

            {/* PIN Method */}
            {activeTab === "pin" ? (
              <form className="space-y-4 pt-2" onSubmit={handlePairSubmit}>
                <div className="text-xs text-muted-foreground space-y-1">
                  <p>
                    On the remote machine, ensure the daemon is running and generate a machine-access PIN:
                  </p>
                  <code className="block rounded bg-muted/60 px-2 py-1 font-mono text-[11px] text-foreground">
                    ferryx-cli pair generate --access machine
                  </code>
                  <p className="pt-1">
                    Relay: <span className="font-mono text-foreground">{DEFAULT_RELAY_ORIGIN}</span>
                  </p>
                </div>

                <div className="space-y-1.5">
                  <Label htmlFor="add-machine-pin" className="text-xs font-medium">
                    Machine PIN
                  </Label>
                  <Input
                    id="add-machine-pin"
                    aria-label="Machine PIN"
                    type="password"
                    autoComplete="off"
                    disabled={busy}
                    required
                    value={pin}
                    placeholder="Enter 6-digit PIN"
                    onChange={e => setPin(e.target.value)}
                  />
                </div>

                <div className="flex justify-end gap-2 pt-2">
                  <Button type="button" variant="outline" disabled={busy} onClick={handleClose}>
                    Cancel
                  </Button>
                  <Button type="submit" disabled={busy || !pin.trim()}>
                    {busy ? "Verifying…" : "Pair Machine"}
                  </Button>
                </div>
              </form>
            ) : activeTab === "ssh" ? (
              /* SSH Method */
              <form className="space-y-3 pt-2" onSubmit={handleSshSubmit}>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <div className="space-y-1">
                    <Label htmlFor="ssh-label" className="text-xs font-medium">Label</Label>
                    <Input
                      id="ssh-label"
                      aria-label="Label"
                      disabled={busy}
                      required
                      value={sshForm.label}
                      placeholder="e.g., dev-server"
                      onChange={e => setSshForm(f => ({ ...f, label: e.target.value }))}
                    />
                  </div>
                  <div className="space-y-1">
                    <Label htmlFor="ssh-hostname" className="text-xs font-medium">Hostname</Label>
                    <Input
                      id="ssh-hostname"
                      aria-label="Hostname"
                      disabled={busy}
                      required
                      value={sshForm.hostname}
                      placeholder="IP or hostname"
                      onChange={e => setSshForm(f => ({ ...f, hostname: e.target.value }))}
                    />
                  </div>
                </div>

                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <div className="space-y-1">
                    <Label htmlFor="ssh-username" className="text-xs font-medium">Username</Label>
                    <Input
                      id="ssh-username"
                      aria-label="Username"
                      disabled={busy}
                      value={sshForm.username}
                      placeholder="ubuntu"
                      onChange={e => setSshForm(f => ({ ...f, username: e.target.value }))}
                    />
                  </div>
                  <div className="space-y-1">
                    <Label htmlFor="ssh-port" className="text-xs font-medium">Port</Label>
                    <Input
                      id="ssh-port"
                      aria-label="Port"
                      disabled={busy}
                      value={sshForm.port}
                      placeholder="22"
                      onChange={e => setSshForm(f => ({ ...f, port: e.target.value }))}
                    />
                  </div>
                </div>

                <div className="space-y-1">
                  <Label htmlFor="ssh-auth-method" className="text-xs font-medium">Authentication method</Label>
                  <select id="ssh-auth-method" aria-label="Authentication method" disabled={busy}
                    className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
                    value={sshForm.authMethod}
                    onChange={e => { setSshForm(f => ({ ...f, authMethod: e.target.value as SshAuthMethod })); setSshPasswordInput(""); }}>
                    <option value="agent">SSH Agent</option>
                    <option value="key">Key File</option>
                    <option value="password">Password</option>
                  </select>
                </div>
                {sshForm.authMethod === "password" ? <div className="space-y-1">
                  <Label htmlFor="ssh-password" className="text-xs font-medium">SSH password</Label>
                  <Input id="ssh-password" type="password" autoComplete="off" required disabled={busy}
                    value={sshPassword} onChange={e => setSshPasswordInput(e.target.value)} />
                  <p className="text-xs text-muted-foreground">Kept in memory for connections, not saved in machine settings.</p>
                </div> : null}

                {/* Advanced SSH fields collapsed per approved proposal */}
                <div className="pt-1">
                  <button
                    type="button"
                    disabled={busy}
                    className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground font-medium"
                    onClick={() => setShowAdvancedSsh(v => !v)}
                  >
                    {showAdvancedSsh ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
                    {showAdvancedSsh ? "Hide Advanced Options" : "Show Advanced Options (Key, Agent, Jump Host)"}
                  </button>
                </div>

                {showAdvancedSsh ? (
                  <div className="space-y-3 pt-2 border-t border-border/50">
                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                      <div className="space-y-1">
                        <Label htmlFor="ssh-identity" className="text-xs font-medium">Identity File</Label>
                        <Input
                          id="ssh-identity"
                          aria-label="Identity file"
                          disabled={busy}
                          value={sshForm.identityFile}
                          placeholder="~/.ssh/id_ed25519"
                          onChange={e => setSshForm(f => ({ ...f, identityFile: e.target.value }))}
                        />
                      </div>
                    </div>

                    <div className="space-y-1">
                      <Label htmlFor="ssh-jump-host" className="text-xs font-medium">Jump Host</Label>
                      <Input
                        id="ssh-jump-host"
                        aria-label="Jump host"
                        disabled={busy}
                        value={sshForm.jumpHost}
                        placeholder="bastion.corp.net"
                        onChange={e => setSshForm(f => ({ ...f, jumpHost: e.target.value }))}
                      />
                    </div>
                  </div>
                ) : null}

                <div className="flex justify-end gap-2 pt-2">
                  <Button type="button" variant="outline" disabled={busy} onClick={handleClose}>
                    Cancel
                  </Button>
                  <Button type="submit" disabled={busy}>
                    {busy ? "Testing & Connecting…" : "Connect SSH Machine"}
                  </Button>
                </div>
              </form>
            ) : (
              /* Config Import Method */
              <div className="space-y-4 pt-2">
                <div className="flex gap-2 border-b border-border/60 pb-2">
                  {systemConfig?.exists ? (
                    <Button
                      type="button"
                      variant={importSubMode === "system" ? "default" : "outline"}
                      size="sm"
                      disabled={busy}
                      onClick={() => setImportSubMode("system")}
                    >
                      System Config ({systemConfig.hosts.length})
                    </Button>
                  ) : null}
                  <Button
                    type="button"
                    variant={importSubMode === "paste" ? "default" : "outline"}
                    size="sm"
                    disabled={busy}
                    onClick={() => setImportSubMode("paste")}
                  >
                    Paste Configuration
                  </Button>
                </div>

                {importSubMode === "system" && systemConfig ? (
                  <Card className="p-3.5 space-y-3 bg-muted/20 border-border">
                    <div className="text-xs space-y-1">
                      <div className="flex items-center justify-between">
                        <span className="font-medium text-foreground">
                          {configPathOverride ? "Custom SSH Config" : "System SSH Config"}
                        </span>
                        <span className="text-muted-foreground font-mono text-[11px]">
                          {systemConfig.hosts.length} hosts found
                        </span>
                      </div>
                      <p className="text-[11px] text-muted-foreground break-all">
                        {systemConfig.path || "~/.ssh/config"}
                      </p>
                    </div>

                    <div className="flex flex-wrap gap-2 pt-1">
                      <Button
                        type="button"
                        size="sm"
                        disabled={busy || systemConfig.hosts.length === 0}
                        aria-label="Import all system hosts"
                        onClick={handleImportSystemAll}
                      >
                        {busy ? "Importing…" : `Import All (${systemConfig.hosts.length} hosts)`}
                      </Button>
                      {isTauri() ? (
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          disabled={busy}
                          aria-label="Choose custom SSH config file"
                          onClick={handleChooseConfigFile}
                        >
                          Choose file…
                        </Button>
                      ) : null}
                      {configPathOverride ? (
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          disabled={busy}
                          onClick={() => {
                            setConfigPathOverrideState(null);
                            setSshConfigPathOverride(null);
                          }}
                        >
                          Use default
                        </Button>
                      ) : null}
                    </div>
                  </Card>
                ) : (
                  <form className="space-y-3" onSubmit={handleImportTextSubmit}>
                    <div className="space-y-1">
                      <Label htmlFor="ssh-config-text" className="text-xs font-medium">
                        SSH Configuration
                      </Label>
                      <textarea
                        id="ssh-config-text"
                        aria-label="SSH Configuration"
                        disabled={busy}
                        rows={6}
                        className="w-full rounded-md border border-input bg-transparent p-2 font-mono text-xs shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
                        placeholder={"Host dev-box\n  HostName 10.0.0.1\n  User ubuntu\n  Port 22"}
                        value={configText}
                        onChange={e => setConfigText(e.target.value)}
                      />
                    </div>

                    <div className="flex justify-end gap-2 pt-2">
                      <Button type="button" variant="outline" disabled={busy} onClick={handleClose}>
                        Cancel
                      </Button>
                      <Button type="submit" disabled={busy || !configText.trim()}>
                        {busy ? "Importing…" : "Import Configuration"}
                      </Button>
                    </div>
                  </form>
                )}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
