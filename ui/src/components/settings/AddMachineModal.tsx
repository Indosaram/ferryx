import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
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
  type LucideIcon,
} from "lucide-react";
import { cn } from "../../lib/cn";
import type { MachineProjectTarget } from "../../lib/machineNavigation";
import {
  DEFAULT_RELAY_ORIGIN,
  pairedHostInventory,
  type PairedHostError,
} from "../../lib/pairedHostInventory";
import { isStructuredIpcError } from "../../lib/tauri";
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
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "../ui/dialog";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { EnrollmentStep } from "./remote/EnrollmentStep";

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
  PAIR_FAILED: "Could not pair. Check connectivity and daemon compatibility, issue a fresh enrollment code from a signed-in desktop, and retry.",
  MACHINE_GRANT_REQUIRED: "Needs machine access. This credential carries no machine grant. Pair again from a signed-in account; revoked credentials cannot reconnect.",
  UNSUPPORTED_CAPABILITY: "This relay, remote daemon, or local daemon does not advertise the required machine capabilities. Upgrade compatible components.",
  STALE_HOST_GENERATION: "Credentials changed during this request. Refresh the inventory and check capabilities again.",
  PAIRED_HOST_UNAVAILABLE: "The native host operation failed. Check the relay and daemon versions, connectivity, and machine grant, then retry. Saved projects have not been removed.",
  OFFLINE: "Machine is offline. Saved projects remain available in the workspace; reconnect to the owning daemon.",
  UNCHECKED: "Check the remote machine capabilities before adding a project.",
  READY: "Machine project capabilities verified.",
};

export function getModalErrorMessage(error: PairedHostError | string): string {
  const code = typeof error === "string" ? error : error.code;
  switch (code) {
    case "PIN_EXPIRED":
    case "EXPIRED_PIN":
      return "Code expired. Sign in to your Ferryx account, issue a fresh enrollment code, and retry.";
    case "INVALID_PIN":
      return "Invalid code. Check the code entered and issue a fresh enrollment code if needed.";
    case "WRONG_RELAY":
    case "INVALID_RELAY_ORIGIN":
      return `Relay mismatch. Verify both machines use the same relay origin (default: ${DEFAULT_RELAY_ORIGIN}).`;
    case "DAEMON_UNAVAILABLE":
    case "HOST_UNAVAILABLE":
      return "Remote daemon unavailable. Start the daemon on the remote machine with 'ferryx-cli --daemon' and retry.";
    case "STALE_HOST_GENERATION":
    case "PAIRED_HOST_STALE_GENERATION":
      return "Credentials changed during this request. Refresh the inventory and retry.";
    case "MACHINE_GRANT_REQUIRED":
      return explanations.MACHINE_GRANT_REQUIRED;
    case "PAIRED_HOST_UNAUTHORIZED":
    case "UNAUTHORIZED":
      return "Authorization rejected. Check machine permissions and retry.";
    case "NATIVE_CONTEXT_REQUIRED":
      return "Native host inventory is unavailable. Use the desktop app with a compatible local daemon.";
    case "PAIR_FAILED":
    default:
      if (typeof error === "object" && error.message && error.message !== code && error.message !== "PAIRED_HOST_UNAVAILABLE") {
        return error.message;
      }
      return explanations[code] ?? "Could not pair. Check connectivity and daemon compatibility, issue a fresh enrollment code from a signed-in desktop, and retry.";
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

export type AddMachineMethod = "enrollment" | "ssh" | "import";

const METHODS: ReadonlyArray<{ value: AddMachineMethod; title: string; description: string; Icon: LucideIcon }> = [
  {
    value: "enrollment",
    title: "Enrollment code",
    description: "For a headless machine signed in to your Ferryx account. Works through the relay, no open ports.",
    Icon: KeyRound,
  },
  {
    value: "ssh",
    title: "Connect with SSH",
    description: "Host, user and key. Ferryx tests the connection before saving it.",
    Icon: Terminal,
  },
  {
    value: "import",
    title: "Import SSH Config",
    description: "Pick hosts from ~/.ssh/config or paste a config block.",
    Icon: FileCode,
  },
];

const METHOD_STEP_DESCRIPTION: Record<AddMachineMethod, string> = {
  enrollment: "Run the command on the machine you want to add.",
  ssh: "Enter the connection details for the machine.",
  import: "Import hosts from an SSH configuration.",
};

function isAddMachineMethod(value: unknown): value is AddMachineMethod {
  return value === "enrollment" || value === "ssh" || value === "import";
}

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export interface AddMachineModalProps {
  isOpen: boolean;
  onClose: () => void;
  inventory?: typeof pairedHostInventory;
  onSuccess: (result: VerifiedMachineResult) => void;
  onOfferProject?: (target: MachineProjectTarget) => void;
  onImportDone?: (selectedHostId: string) => void;
  /** Opens directly on step 2 of this method; otherwise the wizard starts at "Choose a method". */
  initialMethod?: AddMachineMethod;
  /** Account server origin the enrollment command points at. */
  accountOrigin?: string;
  /** Issues an enrollment code for the signed-in account; absent while signed out. */
  onIssueEnrollmentCode?: () => Promise<string>;
}

/** Add Machine wizard: step 1 chooses a method, step 2 runs it. Built on the Dialog primitive. */
export function AddMachineModal(props: AddMachineModalProps) {
  const { isOpen, onClose } = props;
  const [busy, setBusy] = useState(false);
  const contentRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLElement | null>(null);
  const openRef = useRef(isOpen);
  openRef.current = isOpen;
  // A new wizard session (fresh state) per open. Captured during render, before the dialog's
  // focus scope moves focus inside, so the trigger that opened it gets focus back on close.
  const renderOpenRef = useRef(false);
  const sessionRef = useRef(0);
  if (isOpen && !renderOpenRef.current) {
    sessionRef.current += 1;
    triggerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  }
  renderOpenRef.current = isOpen;

  const prevOpenRef = useRef(isOpen);
  useEffect(() => {
    if (prevOpenRef.current && !isOpen) {
      setBusy(false);
      triggerRef.current?.focus();
    }
    prevOpenRef.current = isOpen;
  }, [isOpen]);

  // Disabling the focused control while busy would strand focus on <body>, outside the
  // focus scope; park it on the dialog container instead.
  useEffect(() => {
    if (isOpen && busy && !contentRef.current?.contains(document.activeElement)) {
      contentRef.current?.focus();
    }
  }, [isOpen, busy]);

  const handleClose = () => {
    if (busy) return;
    onClose();
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      // Keep the Escape inside the wizard so the surrounding SettingsDialog stays open.
      e.stopPropagation();
      e.preventDefault();
      handleClose();
      return;
    }
    if (e.key === "Tab" && contentRef.current) {
      if (contentRef.current.querySelectorAll(FOCUSABLE).length === 0) {
        // Nothing is tabbable (every control disabled): keep focus on the dialog container.
        e.preventDefault();
        contentRef.current.focus();
      }
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={(next) => { if (!next) handleClose(); }}>
      <DialogContent
        ref={contentRef}
        closeDisabled={busy}
        onKeyDown={handleKeyDown}
        // Escape is handled in onKeyDown so it can stop propagation; outside clicks never
        // discard a half-filled form; focus return is handled above, synchronously.
        onEscapeKeyDown={(e) => e.preventDefault()}
        onInteractOutside={(e) => e.preventDefault()}
        onCloseAutoFocus={(e) => e.preventDefault()}
        className="max-h-[90vh] max-w-[520px] gap-4 overflow-y-auto p-5 max-sm:bottom-0 max-sm:left-0 max-sm:top-auto max-sm:max-w-none max-sm:translate-x-0 max-sm:translate-y-0 max-sm:rounded-b-none"
      >
        <AddMachineWizard
          key={sessionRef.current}
          {...props}
          busy={busy}
          setBusy={setBusy}
          onClose={handleClose}
          isActive={() => openRef.current}
        />
      </DialogContent>
    </Dialog>
  );
}

interface AddMachineWizardProps extends AddMachineModalProps {
  busy: boolean;
  setBusy: (busy: boolean) => void;
  isActive: () => boolean;
}

function AddMachineWizard({
  onClose,
  inventory = pairedHostInventory,
  onSuccess,
  onOfferProject,
  onImportDone,
  initialMethod,
  accountOrigin = DEFAULT_RELAY_ORIGIN,
  onIssueEnrollmentCode,
  busy,
  setBusy,
  isActive,
}: AddMachineWizardProps) {
  const idPrefix = useId();
  const [step, setStep] = useState<"choose" | "details">(isAddMachineMethod(initialMethod) ? "details" : "choose");
  const [method, setMethod] = useState<AddMachineMethod>(isAddMachineMethod(initialMethod) ? initialMethod : "enrollment");

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
  const [error, setError] = useState<string | null>(null);
  const [structuredError, setStructuredError] = useState<PairedHostError | null>(null);
  const [systemConfigError, setSystemConfigError] = useState<string | null>(null);
  const [successTarget, setSuccessTarget] = useState<{ target: MachineProjectTarget; name: string } | null>(null);
  const [importResult, setImportResult] = useState<{ count: number; hosts: SshHost[] } | null>(null);

  const bodyRef = useRef<HTMLDivElement>(null);
  const radioRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const isMountedRef = useRef(true);
  const stepMountedRef = useRef(false);

  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);
  // A result that lands after the wizard was dismissed is discarded.
  const alive = () => isMountedRef.current && isActive();

  // Move focus into the new step; the dialog's own open autofocus covers the first step.
  useEffect(() => {
    if (!stepMountedRef.current) {
      stepMountedRef.current = true;
      return;
    }
    bodyRef.current
      ?.querySelector<HTMLElement>('input:not([disabled]), textarea:not([disabled]), [role="radio"][aria-checked="true"], button:not([disabled])')
      ?.focus();
  }, [step, method]);

  useEffect(() => {
    if (successTarget || importResult) {
      bodyRef.current?.querySelector<HTMLElement>("[data-result-action]")?.focus();
    }
  }, [successTarget, importResult]);

  useEffect(() => {
    // A newer request (path override change, close/reopen) supersedes this one, so a
    // slow earlier read can never overwrite the config the user is now looking at.
    let stale = false;
    setSystemConfigError(null);
    void readSystemSshConfig(configPathOverride)
      .then(cfg => {
        if (!stale && alive()) {
          setSystemConfig(cfg);
          if (cfg && cfg.hosts.length > 0) {
            setImportSubMode("system");
          }
        }
      })
      .catch(err => {
        if (!stale && alive()) {
          setSystemConfig(null);
          setSystemConfigError(extractIpcErrorMessage(err, "Could not read the system SSH configuration."));
        }
      });
    return () => {
      stale = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [configPathOverride]);

  const showError = (message: string, structured: PairedHostError | null = null) => {
    setError(message);
    setStructuredError(structured);
  };

  const clearError = () => {
    setError(null);
    setStructuredError(null);
  };

  // Structured IPC errors carry a stable `code`; keep it so the code-specific
  // guidance below renders, and fall back to the plain message otherwise.
  const reportError = (err: unknown, fallback: string) => {
    if (isStructuredIpcError(err) && err.message.trim() !== "") {
      const structured: PairedHostError = { code: err.code, message: err.message, retryable: false };
      showError(getModalErrorMessage(structured), structured);
      return;
    }
    showError(extractIpcErrorMessage(err, fallback));
  };

  const goBack = () => {
    if (busy) return;
    clearError();
    setStep("choose");
  };

  const handleMethodKeyDown = (e: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let next: number | null = null;
    if (e.key === "ArrowDown" || e.key === "ArrowRight") next = (index + 1) % METHODS.length;
    else if (e.key === "ArrowUp" || e.key === "ArrowLeft") next = (index - 1 + METHODS.length) % METHODS.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = METHODS.length - 1;
    if (next === null) return;
    e.preventDefault();
    setMethod(METHODS[next].value);
    radioRefs.current[next]?.focus();
  };

  const handleSshSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;

    if (!sshForm.label.trim()) {
      showError("Label is required.");
      return;
    }
    if (!sshForm.hostname.trim()) {
      showError("Hostname or IP is required.");
      return;
    }
    const portNumber = sshForm.port.trim() ? Number(sshForm.port) : 22;
    if (isNaN(portNumber) || portNumber < 1 || portNumber > 65535 || !Number.isInteger(portNumber)) {
      showError("Port must be an integer between 1 and 65535.");
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
      if (!alive()) return;

      if (!summary.reachable) {
        showError(summary.lastError || "SSH connection test failed. Check host, credentials, and network reachability.");
        return;
      }

      await updateSshHost(candidateHost);
      if (!alive()) return;

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
      if (!alive()) return;
      reportError(err, "SSH connection test failed.");
    } finally {
      setBusy(false);
    }
  };

  const handleImportTextSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy || !configText.trim()) return;

    setBusy(true);
    setError(null);

    try {
      const importedHosts = await importSshConfig(configText);
      if (!alive()) return;

      if (Array.isArray(importedHosts) && importedHosts.length > 0) {
        // Imported entries remain UNCHECKED until actual check.
        // Clearly separate imported-not-verified result with NO ready/project offer.
        setImportResult({ count: importedHosts.length, hosts: importedHosts });
        onImportDone?.(importedHosts[0].id);
      } else {
        showError("No hosts were found in that SSH configuration. Check the text and try again.");
      }
    } catch (err) {
      if (!alive()) return;
      reportError(err, "Failed to import SSH configuration.");
      // Retains configText on failure!
    } finally {
      setBusy(false);
    }
  };

  const handleImportSystemAll = async () => {
    if (busy || !systemConfig?.rawText) return;
    setBusy(true);
    setError(null);

    try {
      const importedHosts = await importSshConfig(systemConfig.rawText);
      if (!alive()) return;

      if (Array.isArray(importedHosts) && importedHosts.length > 0) {
        setImportResult({ count: importedHosts.length, hosts: importedHosts });
        onImportDone?.(importedHosts[0].id);
      } else {
        showError("No hosts were found in the system SSH configuration.");
      }
    } catch (err) {
      if (!alive()) return;
      reportError(err, "Failed to import system SSH configuration.");
    } finally {
      setBusy(false);
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
      reportError(err, "Failed to select configuration file.");
    }
  };

  const methodTitle = METHODS.find((m) => m.value === method)?.title ?? "";
  const showingResult = Boolean(successTarget || importResult);

  return (
    <>
      <div className="space-y-1 pr-8">
        <p className="text-[11px] font-medium text-muted-foreground">
          {step === "choose" ? "Step 1 of 2" : `Step 2 of 2 · ${methodTitle}`}
        </p>
        <DialogTitle className="text-[14px]">Add Machine</DialogTitle>
        <DialogDescription>
          {step === "choose" ? "Choose a method. How do you want to connect?" : METHOD_STEP_DESCRIPTION[method]}
        </DialogDescription>
      </div>

      <div ref={bodyRef} className="space-y-4">
        {error ? (
          <div role="alert" className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-[12px] text-destructive">
            <AlertCircle className="mt-0.5 size-4 shrink-0" />
            <div className="flex w-full flex-col gap-1">
              <span className="break-words">{error}</span>
              {structuredError?.code === "PIN_EXPIRED" || structuredError?.code === "EXPIRED_PIN" ? (
                <span className="text-[11px] font-medium text-destructive/90">
                  Action: Issue a fresh enrollment code from a signed-in desktop and enter it above.
                </span>
              ) : structuredError?.code === "WRONG_RELAY" || structuredError?.code === "INVALID_RELAY_ORIGIN" ? (
                <span className="text-[11px] font-medium text-destructive/90">
                  Action: Verify both machines use the same relay URL (default: {DEFAULT_RELAY_ORIGIN}).
                </span>
              ) : structuredError?.code === "DAEMON_UNAVAILABLE" || structuredError?.code === "HOST_UNAVAILABLE" ? (
                <span className="text-[11px] font-medium text-destructive/90">
                  Action: Ensure &lsquo;ferryx-cli --daemon&rsquo; is running on the remote machine.
                </span>
              ) : structuredError?.code === "STALE_HOST_GENERATION" || structuredError?.code === "PAIRED_HOST_STALE_GENERATION" ? (
                <div className="mt-1 flex items-center gap-2">
                  <span className="text-[11px] font-medium text-destructive/90">
                    Action: Inventory changed.
                  </span>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="h-6 px-2 text-[11px]"
                    onClick={() => {
                      void inventory.refresh();
                      clearError();
                    }}
                  >
                    Refresh inventory
                  </Button>
                </div>
              ) : null}
            </div>
          </div>
        ) : null}

        {/* Success View for verified SSH machines */}
        {successTarget ? (
          <div className="space-y-4 py-4 text-center">
            <div className="flex justify-center">
              <CheckCircle2 className="size-10 text-status-success" />
            </div>
            <p className="text-[13px] font-medium text-foreground">
              Machine &ldquo;{successTarget.name}&rdquo; connected and verified successfully!
            </p>
            <p className="text-[12px] text-muted-foreground">
              The machine has been added to your inventory and capabilities have been verified.
            </p>
            <div className="flex justify-center gap-3 pt-2">
              <Button
                type="button"
                data-result-action
                onClick={() => {
                  onOfferProject?.(successTarget.target);
                  onClose();
                }}
              >
                Add Project
              </Button>
              <Button type="button" variant="outline" onClick={onClose}>
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
            <p className="text-[13px] font-medium text-foreground">
              Imported {importResult.count} machine{importResult.count === 1 ? "" : "s"} into inventory
            </p>
            <p className="mx-auto max-w-sm text-[12px] text-muted-foreground">
              Imported machines are recorded as unchecked. Test the connection from machine details before adding remote projects.
            </p>
            <div className="flex justify-center gap-3 pt-2">
              <Button type="button" data-result-action onClick={onClose}>
                Done
              </Button>
            </div>
          </div>
        ) : null}

        {!showingResult && step === "choose" ? (
          <>
            <div role="radiogroup" aria-label="Connection methods" className="flex flex-col gap-1.5">
              {METHODS.map(({ value, title, description, Icon }, index) => {
                const selected = value === method;
                const titleId = `${idPrefix}-${value}-title`;
                const descriptionId = `${idPrefix}-${value}-description`;
                return (
                  <button
                    key={value}
                    ref={(element) => { radioRefs.current[index] = element; }}
                    type="button"
                    role="radio"
                    aria-checked={selected}
                    aria-labelledby={titleId}
                    aria-describedby={descriptionId}
                    tabIndex={selected ? 0 : -1}
                    onClick={() => setMethod(value)}
                    onKeyDown={(e) => handleMethodKeyDown(e, index)}
                    className={cn(
                      "flex w-full items-center gap-3 rounded-lg border border-border px-3 py-2.5 text-left transition-colors hover:bg-accent/40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                      selected && "bg-muted",
                    )}
                  >
                    <span
                      className={cn(
                        "grid size-7 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground",
                        selected && "bg-accent text-foreground",
                      )}
                    >
                      <Icon className="size-3.5" aria-hidden="true" />
                    </span>
                    <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <span id={titleId} className="text-[12px] font-medium text-foreground">{title}</span>
                      <span id={descriptionId} className="text-[11px] text-muted-foreground">{description}</span>
                    </span>
                    <span
                      aria-hidden="true"
                      className={cn(
                        "size-3.5 shrink-0 rounded-full",
                        selected ? "border-4 border-primary" : "border border-input",
                      )}
                    />
                  </button>
                );
              })}
            </div>
            <div className="flex justify-end gap-2 border-t border-border pt-3">
              <Button type="button" variant="ghost" size="sm" className="text-[12px]" onClick={onClose}>
                Cancel
              </Button>
              <Button
                type="button"
                size="sm"
                className="text-[12px]"
                onClick={() => {
                  clearError();
                  setStep("details");
                }}
              >
                Continue
              </Button>
            </div>
          </>
        ) : null}

        {!showingResult && step === "details" && method === "enrollment" ? (
          <EnrollmentStep
            accountOrigin={accountOrigin}
            onIssueEnrollmentCode={onIssueEnrollmentCode}
            onBack={goBack}
            onDone={onClose}
          />
        ) : null}

        {!showingResult && step === "details" && method === "ssh" ? (
          <form className="space-y-3" onSubmit={handleSshSubmit}>
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <div className="space-y-1">
                <Label htmlFor="ssh-label" className="text-[11px] font-medium">Label</Label>
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
                <Label htmlFor="ssh-hostname" className="text-[11px] font-medium">Hostname</Label>
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

            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <div className="space-y-1">
                <Label htmlFor="ssh-username" className="text-[11px] font-medium">Username</Label>
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
                <Label htmlFor="ssh-port" className="text-[11px] font-medium">Port</Label>
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
              <Label htmlFor="ssh-auth-method" className="text-[11px] font-medium">Authentication method</Label>
              <select id="ssh-auth-method" aria-label="Authentication method" disabled={busy}
                className="h-9 w-full rounded-md border border-input bg-background px-3 text-[12px]"
                value={sshForm.authMethod}
                onChange={e => { setSshForm(f => ({ ...f, authMethod: e.target.value as SshAuthMethod })); setSshPasswordInput(""); }}>
                <option value="agent">SSH Agent</option>
                <option value="key">Key File</option>
                <option value="password">Password</option>
              </select>
            </div>
            {sshForm.authMethod === "password" ? <div className="space-y-1">
              <Label htmlFor="ssh-password" className="text-[11px] font-medium">SSH password</Label>
              <Input id="ssh-password" type="password" autoComplete="off" required disabled={busy}
                value={sshPassword} onChange={e => setSshPasswordInput(e.target.value)} />
              <p className="text-[11px] text-muted-foreground">Kept in memory for connections, not saved in machine settings.</p>
            </div> : null}

            {/* Advanced SSH fields collapsed per approved proposal */}
            <div className="pt-1">
              <button
                type="button"
                disabled={busy}
                className="flex items-center gap-1 text-[11px] font-medium text-muted-foreground hover:text-foreground"
                onClick={() => setShowAdvancedSsh(v => !v)}
              >
                {showAdvancedSsh ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
                {showAdvancedSsh ? "Hide Advanced Options" : "Show Advanced Options (Key, Agent, Jump Host)"}
              </button>
            </div>

            {showAdvancedSsh ? (
              <div className="space-y-3 border-t border-border/50 pt-2">
                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                  <div className="space-y-1">
                    <Label htmlFor="ssh-identity" className="text-[11px] font-medium">Identity File</Label>
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
                  <Label htmlFor="ssh-jump-host" className="text-[11px] font-medium">Jump Host</Label>
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

            <div className="flex items-center gap-2 border-t border-border pt-3">
              <Button type="button" variant="ghost" size="sm" className="text-[12px]" disabled={busy} onClick={goBack}>
                Back
              </Button>
              <span className="flex-1" />
              <Button type="button" variant="outline" size="sm" className="text-[12px]" disabled={busy} onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" size="sm" className="text-[12px]" disabled={busy}>
                {busy ? "Testing & Connecting…" : "Connect SSH Machine"}
              </Button>
            </div>
          </form>
        ) : null}

        {!showingResult && step === "details" && method === "import" ? (
          <div className="space-y-4">
            {systemConfigError ? (
              <p role="alert" className="text-[11px] text-destructive">
                Could not read the system SSH configuration: {systemConfigError}
              </p>
            ) : null}
            <div className="flex gap-2 border-b border-border/60 pb-2">
              {systemConfig?.exists ? (
                <Button
                  type="button"
                  variant={importSubMode === "system" ? "default" : "outline"}
                  size="sm"
                  className="text-[12px]"
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
                className="text-[12px]"
                disabled={busy}
                onClick={() => setImportSubMode("paste")}
              >
                Paste Configuration
              </Button>
            </div>

            {importSubMode === "system" && systemConfig ? (
              <>
                <Card className="space-y-3 border-border bg-muted/20 p-3.5">
                  <div className="space-y-1 text-[12px]">
                    <div className="flex items-center justify-between">
                      <span className="font-medium text-foreground">
                        {configPathOverride ? "Custom SSH Config" : "System SSH Config"}
                      </span>
                      <span className="font-mono text-[11px] text-muted-foreground">
                        {systemConfig.hosts.length} hosts found
                      </span>
                    </div>
                    <p className="break-all text-[11px] text-muted-foreground">
                      {systemConfig.path || "~/.ssh/config"}
                    </p>
                  </div>

                  <div className="flex flex-wrap gap-2 pt-1">
                    <Button
                      type="button"
                      size="sm"
                      className="text-[12px]"
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
                        className="text-[12px]"
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
                        className="text-[12px]"
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
                <div className="flex items-center gap-2 border-t border-border pt-3">
                  <Button type="button" variant="ghost" size="sm" className="text-[12px]" disabled={busy} onClick={goBack}>
                    Back
                  </Button>
                  <span className="flex-1" />
                  <Button type="button" variant="outline" size="sm" className="text-[12px]" disabled={busy} onClick={onClose}>
                    Cancel
                  </Button>
                </div>
              </>
            ) : (
              <form className="space-y-3" onSubmit={handleImportTextSubmit}>
                <div className="space-y-1">
                  <Label htmlFor="ssh-config-text" className="text-[11px] font-medium">
                    SSH Configuration
                  </Label>
                  <textarea
                    id="ssh-config-text"
                    aria-label="SSH Configuration"
                    disabled={busy}
                    rows={6}
                    className="w-full rounded-md border border-input bg-transparent p-2 font-mono text-[11px] shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
                    placeholder={"Host dev-box\n  HostName 10.0.0.1\n  User ubuntu\n  Port 22"}
                    value={configText}
                    onChange={e => setConfigText(e.target.value)}
                  />
                </div>

                <div className="flex items-center gap-2 border-t border-border pt-3">
                  <Button type="button" variant="ghost" size="sm" className="text-[12px]" disabled={busy} onClick={goBack}>
                    Back
                  </Button>
                  <span className="flex-1" />
                  <Button type="button" variant="outline" size="sm" className="text-[12px]" disabled={busy} onClick={onClose}>
                    Cancel
                  </Button>
                  <Button type="submit" size="sm" className="text-[12px]" disabled={busy || !configText.trim()}>
                    {busy ? "Importing…" : "Import Configuration"}
                  </Button>
                </div>
              </form>
            )}
          </div>
        ) : null}
      </div>
    </>
  );
}
