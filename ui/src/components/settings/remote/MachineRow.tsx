import type { ReactNode } from "react";
import { MoreHorizontal } from "lucide-react";
import { DEFAULT_MACHINE_LABEL } from "../../../lib/pairedHostInventory";
import {
  MACHINE_STATUS_EXPLANATIONS,
  pairedMachineStatus,
  sshMachineStatus,
  type MachineState,
  type MachineStatus,
} from "../../../lib/machineStatus";
import { usePairedHostReachability } from "../../../lib/pairedHostReachability";
import type { SshHost, SshTargetSummary } from "../../../lib/sshHosts";
import type { HostEndpoint } from "../../../state/remoteHostStore";
import { Badge } from "../../ui/badge";
import { Button } from "../../ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "../../ui/dropdown-menu";

export interface PairedStatusInput {
  host: HostEndpoint;
  nativeReady: boolean;
  accountEnrolled: boolean;
  check?: { generation: string; code: string };
  canOpenProject: boolean;
}

/**
 * Unified paired status. A host that is neither enrolled on the account nor capability-checked for
 * its current generation stays UNCHECKED, exactly as before the redesign: Add Project unlocks only
 * after "Check capabilities" succeeds.
 */
export function usePairedRowStatus({ host, nativeReady, accountEnrolled, check, canOpenProject }: PairedStatusInput): MachineStatus {
  const reachability = usePairedHostReachability(host.hostId);
  const status = pairedMachineStatus({ host, nativeReady, accountEnrolled, reachability, check, canOpenProject });
  const checked = Boolean(check && check.generation === host.generation);
  if (!accountEnrolled && !checked && status.code === "READY") {
    return { ...status, state: "settingUp", code: "UNCHECKED", label: "Unchecked", primaryEnabled: false, reason: MACHINE_STATUS_EXPLANATIONS.UNCHECKED };
  }
  return status;
}

export function pairedDisplayName(host: HostEndpoint): string {
  return host.name === DEFAULT_MACHINE_LABEL && host.machineId ? `${host.name} · ${host.machineId.slice(-8)}` : host.name;
}

const DOT_CLASS: Record<MachineState, string> = {
  connected: "bg-status-success",
  offline: "bg-muted-foreground/50",
  checking: "bg-status-working animate-pulse motion-reduce:animate-none",
  needsAccess: "bg-status-warning",
  settingUp: "bg-status-warning",
  error: "bg-destructive",
  disabled: "bg-muted-foreground/30",
};

const LABEL_CLASS: Record<MachineState, string> = {
  connected: "text-status-success",
  offline: "text-muted-foreground",
  checking: "text-muted-foreground",
  needsAccess: "text-status-warning",
  settingUp: "text-status-warning",
  error: "text-status-warning",
  disabled: "text-muted-foreground",
};

/** Status dot plus label; the element carrying role=status keeps the pre-redesign data-code contract. */
export function MachineStatusLabel({ status, title, testId = "machine-status" }: { status: MachineStatus; title?: string; testId?: string }) {
  return (
    <span className="flex shrink-0 items-center gap-1.5">
      <span aria-hidden="true" data-state={status.state} className={`size-2 shrink-0 rounded-full ${DOT_CLASS[status.state]}`} />
      <span
        role="status"
        data-testid={testId}
        data-code={status.code}
        title={title ?? MACHINE_STATUS_EXPLANATIONS[status.code]}
        className={`text-[12px] font-medium ${LABEL_CLASS[status.state]}`}
      >
        {status.label}
      </span>
    </span>
  );
}

export interface MenuItemSpec {
  label: string;
  onSelect: () => void;
  disabled?: boolean;
  destructive?: boolean;
}

interface MachineRowProps {
  rowKey: string;
  name: string;
  /** Name used in the accessible action labels; matches the pre-redesign templates. */
  actionName: string;
  kindLabel: string;
  pathChip: string;
  status: MachineStatus;
  statusTitle?: string;
  selected: boolean;
  primary: { kind: "add" | "connect"; disabled: boolean; onClick: () => void };
  disabledReason: string | null;
  note?: ReactNode;
  menuItems: MenuItemSpec[];
  onDetails: () => void;
  rowRef: (node: HTMLDivElement | null) => void;
  detailsRef: (node: HTMLButtonElement | null) => void;
}

export function MachineRow({ rowKey, name, actionName, kindLabel, pathChip, status, statusTitle, selected, primary, disabledReason, note, menuItems, onDetails, rowRef, detailsRef }: MachineRowProps) {
  const reasonId = `machine-reason-${rowKey.replace(/[^a-zA-Z0-9_-]/g, "-")}`;
  const destructive = menuItems.filter(item => item.destructive);
  const regular = menuItems.filter(item => !item.destructive);
  return (
    <div
      role="listitem"
      tabIndex={-1}
      ref={rowRef}
      data-machine-id={rowKey}
      data-selected={selected}
      className={`space-y-1.5 rounded-lg border p-3 transition-colors ${selected ? "border-primary/60 bg-accent/30" : "border-border bg-card/50 hover:bg-card/80"}`}
    >
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <span className="truncate text-[13px] font-medium text-foreground">{name}</span>
          <Badge variant="outline" className="h-4 shrink-0 px-1.5 py-0 text-[11px]">{kindLabel}</Badge>
          <MachineStatusLabel status={status} title={statusTitle} />
          <span className="shrink-0 rounded border border-border/70 bg-muted/40 px-1.5 text-[11px] text-muted-foreground">{pathChip}</span>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          {primary.kind === "connect" ? (
            <Button type="button" size="sm" aria-label={`Connect ${actionName}`} disabled={primary.disabled} aria-describedby={primary.disabled && disabledReason ? reasonId : undefined} onClick={primary.onClick}>
              Connect
            </Button>
          ) : (
            <Button type="button" size="sm" variant={primary.disabled ? "outline" : "default"} aria-label={`Add Project on ${actionName}`} disabled={primary.disabled} aria-describedby={primary.disabled && disabledReason ? reasonId : undefined} onClick={primary.onClick}>
              Add Project
            </Button>
          )}
          <Button type="button" variant="outline" size="sm" ref={detailsRef} aria-label={`Details for ${actionName}`} aria-haspopup="dialog" onClick={onDetails}>
            Details
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button type="button" variant="ghost" size="sm" className="size-8 p-0" aria-label={`More actions for ${actionName}`}>
                <MoreHorizontal className="size-4" aria-hidden="true" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {regular.map(item => (
                <DropdownMenuItem key={item.label} disabled={item.disabled} onSelect={item.onSelect}>{item.label}</DropdownMenuItem>
              ))}
              {destructive.length > 0 ? <DropdownMenuSeparator /> : null}
              {destructive.map(item => (
                <DropdownMenuItem key={item.label} variant="destructive" disabled={item.disabled} onSelect={item.onSelect}>{item.label}</DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
      {primary.disabled && disabledReason ? (
        <p id={reasonId} data-testid="machine-disabled-reason" className="break-words text-[12px] text-muted-foreground">{disabledReason}</p>
      ) : null}
      {note}
    </div>
  );
}

export interface PairedMachineRowProps extends PairedStatusInput {
  selected: boolean;
  busy: boolean;
  signedIn: boolean;
  onAddProject: () => void;
  onConnect: () => void;
  onDetails: () => void;
  onCheck: () => void;
  onForget: () => void;
  rowRef: (node: HTMLDivElement | null) => void;
  detailsRef: (node: HTMLButtonElement | null) => void;
}

export function PairedMachineRow(props: PairedMachineRowProps) {
  const { host, busy, signedIn, nativeReady, canOpenProject } = props;
  const status = usePairedRowStatus(props);
  const name = pairedDisplayName(host);
  const connect = status.primary === "connect";
  const disabled = connect ? busy || !signedIn : busy || status.code !== "READY" || !canOpenProject;
  const reason = busy ? "Another action is in progress." : connect && !signedIn ? "Sign in to your Ferryx account to connect." : status.reason ?? MACHINE_STATUS_EXPLANATIONS[status.code] ?? null;
  return (
    <MachineRow
      rowKey={`paired:${host.hostId}`}
      name={name}
      actionName={host.name}
      kindLabel="Paired"
      pathChip={host.transport === "relay" ? "Relay" : host.transport}
      status={status}
      selected={props.selected}
      primary={connect
        ? { kind: "connect", disabled, onClick: props.onConnect }
        : { kind: "add", disabled, onClick: props.onAddProject }}
      disabledReason={reason}
      menuItems={[
        { label: "Details", onSelect: props.onDetails },
        { label: "Check capabilities", onSelect: props.onCheck, disabled: busy || !host.generation || !nativeReady },
        { label: "Forget credentials…", onSelect: props.onForget, disabled: busy || !host.generation, destructive: true },
      ]}
      onDetails={props.onDetails}
      rowRef={props.rowRef}
      detailsRef={props.detailsRef}
    />
  );
}

export interface SshMachineRowProps {
  host: SshHost;
  target?: SshTargetSummary;
  discoveryRefreshing: boolean;
  test?: { testing?: boolean; reachable?: boolean; error?: string | null };
  prepared: boolean;
  canOpenProject: boolean;
  selected: boolean;
  busy: boolean;
  onAddProject: () => void;
  onDetails: () => void;
  onEdit: () => void;
  onToggleDisable: () => void;
  onDelete: () => void;
  rowRef: (node: HTMLDivElement | null) => void;
  detailsRef: (node: HTMLButtonElement | null) => void;
}

export function sshRowStatus(props: Pick<SshMachineRowProps, "host" | "target" | "discoveryRefreshing" | "test" | "prepared" | "canOpenProject">): MachineStatus {
  return sshMachineStatus({
    host: props.host,
    tailscaleTarget: props.target,
    discoveryRefreshing: props.discoveryRefreshing,
    test: props.test,
    prepared: props.prepared,
    canOpenProject: props.canOpenProject,
  });
}

export function SshMachineRow(props: SshMachineRowProps) {
  const { host, target, busy } = props;
  const status = sshRowStatus(props);
  const disabled = busy || Boolean(host.disabled) || !props.canOpenProject;
  const reason = busy ? "Another action is in progress." : host.disabled ? "Machine is disabled." : !props.canOpenProject ? "Opening projects is not available here." : null;
  return (
    <MachineRow
      rowKey={`ssh:${host.id}`}
      name={host.label}
      actionName={host.label}
      kindLabel={host.source === "tailscale" ? "Tailscale · SSH" : "SSH"}
      pathChip={host.jumpHost ? "Jump host" : "Direct"}
      status={status}
      statusTitle={props.test?.error || target?.lastError || status.label}
      selected={props.selected}
      primary={{ kind: "add", disabled, onClick: props.onAddProject }}
      disabledReason={reason}
      note={target?.diagnostic ? (
        <p role="note" className="break-words text-[12px] text-muted-foreground">{target.diagnostic.details?.stage ?? target.diagnostic.code}: {target.diagnostic.message}</p>
      ) : null}
      menuItems={[
        { label: "Details", onSelect: props.onDetails },
        { label: "Edit…", onSelect: props.onEdit, disabled: busy },
        { label: host.disabled ? "Enable" : "Disable", onSelect: props.onToggleDisable, disabled: busy },
        { label: "Delete machine…", onSelect: props.onDelete, disabled: busy, destructive: true },
      ]}
      onDetails={props.onDetails}
      rowRef={props.rowRef}
      detailsRef={props.detailsRef}
    />
  );
}
