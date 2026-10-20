import type { FormEvent } from "react";
import { AlertCircle, ChevronDown, ChevronRight, Trash2 } from "lucide-react";
import { MACHINE_STATUS_EXPLANATIONS } from "../../../lib/machineStatus";
import type { SshAuthMethod, SshHost, SshRemoteEnvironment, SshTargetSummary } from "../../../lib/sshHosts";
import type { HostFormData } from "../AddMachineModal";
import { Button } from "../../ui/button";
import { Input } from "../../ui/input";
import { Label } from "../../ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../../ui/select";
import { Sheet, SheetContent, SheetDescription, SheetTitle } from "../../ui/sheet";
import { useModalDialog } from "../../useModalDialog";
import { MachineStatusLabel, pairedDisplayName, sshRowStatus, usePairedRowStatus, type PairedStatusInput } from "./MachineRow";

export interface SshTestState {
  testing: boolean;
  reachable?: boolean;
  error?: string | null;
  environment?: SshRemoteEnvironment | null;
  stage?: string;
}

function Field({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="min-w-0">
      <dt className="text-[11px] font-semibold text-foreground/80">{label}</dt>
      <dd className={`break-all text-[12px] text-muted-foreground ${mono ? "font-mono" : ""}`}>{value}</dd>
    </div>
  );
}

function ConfirmGroup({ label, message, confirmLabel, confirmAria, cancelAria, busy, onConfirm, onCancel }: {
  label: string; message: string; confirmLabel: string; confirmAria: string; cancelAria: string; busy: boolean; onConfirm: () => void; onCancel: () => void;
}) {
  return (
    <div role="group" aria-label={label} className="space-y-2 rounded border border-border/80 bg-accent/20 p-3">
      <p className="text-[12px] text-status-warning">{message}</p>
      <div className="flex gap-2">
        <Button type="button" variant="destructive" size="sm" disabled={busy} aria-label={confirmAria} onClick={onConfirm}>{confirmLabel}</Button>
        <Button type="button" variant="outline" size="sm" disabled={busy} aria-label={cancelAria} onClick={onCancel}>Cancel</Button>
      </div>
    </div>
  );
}

export interface PairedDetailsProps extends PairedStatusInput {
  busy: boolean;
  /** Re-pairing goes through the Ferryx account, so it needs a signed-in session. */
  signedIn: boolean;
  repairRequested: boolean;
  confirmingForget: boolean;
  onCheck: () => void;
  onRepair: () => void;
  onForget: () => void;
  onConfirmForget: () => void;
  onCancelForget: () => void;
}

function PairedDetails(props: PairedDetailsProps) {
  const { host, busy, nativeReady } = props;
  const status = usePairedRowStatus(props);
  const name = host.name;
  const displayName = pairedDisplayName(host);
  return (
    <div className="space-y-4">
      <MachineStatusLabel status={status} testId="machine-drawer-status" />
      {status.code !== "READY" && MACHINE_STATUS_EXPLANATIONS[status.code] ? (
        <p className="text-[12px] text-muted-foreground">{MACHINE_STATUS_EXPLANATIONS[status.code]}</p>
      ) : null}
      <dl className="grid grid-cols-1 gap-2 sm:grid-cols-2">
        <Field label="Machine ID" value={host.machineId ?? ""} mono />
        <Field label="Relay" value={host.relayOrigin ?? ""} mono />
        <Field label="Transport" value={host.transport} />
        <Field label="Generation" value={host.generation ?? "unknown"} />
        <Field label="Grant" value={host.grantScope ?? "unknown"} />
        <Field label="Auth Status" value={host.authStatus} />
      </dl>
      <div className="flex flex-wrap gap-2">
        <Button type="button" variant="outline" size="sm" aria-label={`Check capabilities for ${name}`} disabled={busy || !host.generation || !nativeReady} onClick={props.onCheck}>
          Check capabilities
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-label={`Re-pair ${name}`}
          disabled={busy || !props.signedIn}
          aria-describedby={props.signedIn ? undefined : "machine-drawer-repair-reason"}
          onClick={props.onRepair}
        >
          Re-pair
        </Button>
        <Button type="button" variant="outline" size="sm" aria-label={`Forget ${name}`} disabled={busy || !host.generation} onClick={props.onForget}>
          Forget credentials
        </Button>
      </div>
      {!props.signedIn ? (
        <p id="machine-drawer-repair-reason" data-testid="machine-drawer-repair-reason" className="text-[12px] text-muted-foreground">
          Sign in to re-pair.
        </p>
      ) : null}
      {props.repairRequested ? (
        <p role="status" data-code="REPAIR_REQUIRES_ENROLLMENT" className="text-[12px] text-muted-foreground">
          Re-pairing goes through your Ferryx account. Sign in above, issue an enrollment code, and run{" "}
          <code className="font-mono">ferryx-cli account enroll --code &lt;code&gt;</code> on that machine. It then
          appears in this list and can be connected. No credentials have been replaced.
        </p>
      ) : null}
      {props.confirmingForget ? (
        <ConfirmGroup
          label="Confirm credential removal"
          message={`Forget credentials for ${displayName}? Saved project/layout references and files remain. This does not unregister projects, close remote sessions, or revoke other devices. Re-pair to reconnect.`}
          confirmLabel="Confirm forget"
          confirmAria={`Confirm forget ${name}`}
          cancelAria="Cancel forget"
          busy={busy}
          onConfirm={props.onConfirmForget}
          onCancel={props.onCancelForget}
        />
      ) : null}
    </div>
  );
}

export interface SshEditState {
  form: HostFormData;
  password: string;
  showAdvanced: boolean;
  onChange: (update: (form: HostFormData) => HostFormData) => void;
  onPasswordChange: (value: string) => void;
  onToggleAdvanced: () => void;
  onSubmit: (event: FormEvent) => void;
  onCancel: () => void;
}

function SshEditForm({ edit, busy }: { edit: SshEditState; busy: boolean }) {
  const { form } = edit;
  const input = (label: string, key: keyof HostFormData, placeholder?: string, required?: boolean) => (
    <div className="space-y-1">
      <Label className="text-[12px]">{label}</Label>
      <Input aria-label={label} value={form[key]} placeholder={placeholder} required={required} className="h-7 text-[12px]"
        onChange={e => { const value = e.target.value; edit.onChange(f => ({ ...f, [key]: value })); }} />
    </div>
  );
  return (
    <form onSubmit={edit.onSubmit} className="space-y-3 rounded border border-border bg-muted/30 p-3">
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
        {input("Label", "label", undefined, true)}
        {input("Hostname", "hostname", undefined, true)}
      </div>
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
        {input("Username", "username")}
        {input("Port", "port")}
      </div>
      <button type="button" className="flex items-center gap-1 text-[12px] font-medium text-muted-foreground hover:text-foreground" onClick={edit.onToggleAdvanced}>
        {edit.showAdvanced ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
        {edit.showAdvanced ? "Hide Advanced Options" : "Show Advanced Options (Key, Agent, Jump Host)"}
      </button>
      {edit.showAdvanced ? (
        <div className="space-y-2 border-t border-border/50 pt-1">
          <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
            <div className="space-y-1">
              <Label className="text-[12px]">Auth Method</Label>
              <Select value={form.authMethod} onValueChange={(val: SshAuthMethod) => edit.onChange(f => ({ ...f, authMethod: val }))}>
                <SelectTrigger className="h-7 text-[12px]" aria-label="Auth Method">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="agent">SSH Agent</SelectItem>
                  <SelectItem value="key">Key File</SelectItem>
                  <SelectItem value="password">Password</SelectItem>
                </SelectContent>
              </Select>
              {form.authMethod === "password" ? (
                <div className="space-y-1 pt-2">
                  <Label htmlFor="edit-ssh-password" className="text-[12px]">SSH password</Label>
                  <Input id="edit-ssh-password" type="password" autoComplete="off" value={edit.password}
                    onChange={e => edit.onPasswordChange(e.target.value)} placeholder="Enter password to authenticate again" />
                </div>
              ) : null}
            </div>
            {input("Identity File", "identityFile", "~/.ssh/id_ed25519")}
          </div>
          {input("Jump Host", "jumpHost", "bastion.corp.net")}
        </div>
      ) : null}
      <div className="flex justify-end gap-2 pt-1">
        <Button type="button" variant="outline" size="sm" onClick={edit.onCancel}>Cancel</Button>
        <Button type="submit" size="sm" disabled={busy}>Save Changes</Button>
      </div>
    </form>
  );
}

export interface SshDetailsProps {
  host: SshHost;
  target?: SshTargetSummary;
  discoveryRefreshing: boolean;
  test?: SshTestState;
  prepared: boolean;
  canOpenProject: boolean;
  busy: boolean;
  agentIntegrationEnabled: boolean;
  removalResult?: string;
  edit: SshEditState | null;
  confirmingDelete: boolean;
  onTest: () => void;
  onPrepare: () => void;
  onRemoveIntegration: () => void;
  onEdit: () => void;
  onToggleDisable: () => void;
  onDelete: () => void;
  onConfirmDelete: () => void;
  onCancelDelete: () => void;
}

function SshDetails(props: SshDetailsProps) {
  const { host, test, busy } = props;
  const status = sshRowStatus(props);
  if (props.edit) return <SshEditForm edit={props.edit} busy={busy} />;
  return (
    <div className="space-y-4">
      <MachineStatusLabel status={status} title={test?.error || status.label} testId="machine-drawer-status" />
      <dl className="grid grid-cols-1 gap-2 sm:grid-cols-2">
        <Field label="Hostname" value={host.hostname} mono />
        <Field label="Port" value={String(host.port ?? 22)} />
        <Field label="Username" value={host.username ?? "(default)"} />
        <Field label="Auth method" value={host.authMethod} />
        {host.identityFile ? <Field label="Identity file" value={host.identityFile} mono /> : null}
        {host.jumpHost ? <Field label="Jump host" value={host.jumpHost} /> : null}
      </dl>
      {test?.error ? <p className="break-words text-[12px] text-destructive">{test.error}</p> : null}
      {test?.environment ? (
        <div
          data-testid={`ssh-runtime-${host.id}`}
          data-platform={test.environment.platform}
          data-git={String(test.environment.git)}
          className="space-y-0.5 break-all rounded border border-border/60 bg-muted/20 p-2 text-[11px]"
        >
          <div><span className="font-semibold">Platform:</span> {test.environment.platform} ({test.environment.executor})</div>
          <div><span className="font-semibold">Git:</span> {test.environment.git ? "Installed" : "Not installed"}</div>
          <div><span className="font-semibold">Home:</span> {test.environment.home}</div>
        </div>
      ) : null}
      <div className="flex flex-wrap gap-2">
        <Button type="button" variant="outline" size="sm" aria-label={`Test connection to ${host.label}`} disabled={busy || test?.testing} onClick={props.onTest}>
          Test connection
        </Button>
        {props.agentIntegrationEnabled ? (
          <Button type="button" variant="outline" size="sm" aria-label={`Prepare agent integration on ${host.label}`} disabled={busy || Boolean(host.disabled)} onClick={props.onPrepare}>
            Prepare agent integration
          </Button>
        ) : null}
        <Button type="button" variant="outline" size="sm" aria-label={`Remove agent integration on ${host.label}`} disabled={busy || Boolean(host.disabled)} onClick={props.onRemoveIntegration}>
          Remove integration
        </Button>
        {props.removalResult ? <span className="self-center text-[11px] text-muted-foreground">{props.removalResult}</span> : null}
        <Button type="button" variant="outline" size="sm" aria-label={`Edit ${host.label}`} disabled={busy} onClick={props.onEdit}>Edit</Button>
        <Button type="button" variant="outline" size="sm" aria-label={`Toggle disable ${host.label}`} disabled={busy} onClick={props.onToggleDisable}>
          {host.disabled ? "Enable" : "Disable"}
        </Button>
        <Button type="button" variant="outline" size="sm" aria-label={`Delete ${host.label}`} disabled={busy} onClick={props.onDelete}>Delete</Button>
      </div>
      {props.confirmingDelete ? (
        <ConfirmGroup
          label="Confirm machine deletion"
          message={`Delete ${host.label}? This removes the saved SSH machine from Ferryx.`}
          confirmLabel="Confirm delete"
          confirmAria={`Confirm delete ${host.label}`}
          cancelAria="Cancel delete"
          busy={busy}
          onConfirm={props.onConfirmDelete}
          onCancel={props.onCancelDelete}
        />
      ) : null}
    </div>
  );
}

export type MachineDrawerContent =
  | ({ kind: "paired" } & PairedDetailsProps)
  | ({ kind: "ssh" } & SshDetailsProps);

/**
 * Details live in a right-anchored Sheet that overlays the list, so opening it never reflows the
 * rows. Radix returns focus to the trigger captured by the parent (the row's Details button).
 */
export function MachineDrawer({ content, error, onClose, returnFocusTo }: {
  content: MachineDrawerContent | null;
  /** The section's action error; shown here while the drawer covers the list. */
  error: string | null;
  onClose: () => void;
  returnFocusTo: () => HTMLElement | null | undefined;
}) {
  const title = content ? (content.kind === "paired" ? pairedDisplayName(content.host) : content.host.label) : "";
  return (
    <Sheet open={content !== null} onOpenChange={open => { if (!open) onClose(); }}>
      <SheetContent
        onEscapeKeyDown={event => {
          // Escape closes only the drawer, never the Settings dialog underneath it.
          event.stopPropagation();
        }}
        onCloseAutoFocus={event => {
          const target = returnFocusTo();
          if (target && target.isConnected) {
            event.preventDefault();
            target.focus({ preventScroll: true });
          }
        }}
      >
        <div className="space-y-1 pr-6">
          <SheetTitle>{title}</SheetTitle>
          <SheetDescription>{content?.kind === "paired" ? "Paired machine" : "SSH machine"}</SheetDescription>
        </div>
        {error ? (
          <div role="alert" className="flex items-center gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-2.5 text-[12px] text-destructive">
            <AlertCircle className="size-4 shrink-0" />
            <span>{error}</span>
          </div>
        ) : null}
        {content?.kind === "paired" ? <PairedDetails {...content} /> : content?.kind === "ssh" ? <SshDetails {...content} /> : null}
      </SheetContent>
    </Sheet>
  );
}

export function RemoveSshIntegrationDialog({ hostLabel, onCancel, onConfirm }: {
  hostLabel: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { dialogRef } = useModalDialog({ onEscape: onCancel });
  const title = `Remove agent integration from ${hostLabel}?`;
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onMouseDown={event => {
        event.stopPropagation();
        onCancel();
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-label={title}
        aria-modal="true"
        className="w-full max-w-md animate-enter overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
        onMouseDown={event => event.stopPropagation()}
      >
        <header className="flex items-center gap-2 border-b border-border px-4 py-3 text-sm font-semibold text-foreground">
          <Trash2 className="size-4 text-destructive" />
          {title}
        </header>
        <div className="space-y-4 p-4 text-[12px]">
          <p className="text-muted-foreground">
            Ferryx will remove the agent state hooks it installed on this host. Files you
            modified after installation are kept.
          </p>
          <div className="flex justify-end gap-2">
            <button type="button" onClick={onCancel} className="rounded-md px-3 py-2 text-muted-foreground hover:bg-accent hover:text-foreground">
              Cancel
            </button>
            <button type="button" onClick={onConfirm} className="rounded-md bg-destructive px-3 py-2 font-semibold text-destructive-foreground">
              Remove
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
