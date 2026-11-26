/**
 * The shared "SSH tunnels" panel: every tunnel the daemon reports, saved and ad-hoc, plus one
 * ad-hoc forward form. It renders what `listTunnels()` says and never invents a state — the
 * absence of a runtime id in a newer response is the only stop signal this client has, and a
 * snapshot older than the stale threshold loses its confident `listening` badge.
 */
import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { ArrowRight, RefreshCw } from "lucide-react";

import { extractIpcErrorMessage, type SshHost } from "../../../lib/sshHosts";
import { hasSshTunnelCapability, type AdHocTunnelSpec } from "../../../lib/sshTunnels";
import { Badge } from "../../ui/badge";
import { Button } from "../../ui/button";
import { Input } from "../../ui/input";
import { Label } from "../../ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../../ui/select";
import {
  buildTunnelRows,
  describeTunnelFailure,
  formatSince,
  isSnapshotStale,
  TUNNEL_STALE_BADGE_CLASSES,
  TUNNEL_STATE_BADGE_CLASSES,
  tunnelBadge,
  viewKey,
  type TunnelRow,
} from "./tunnelPresentation";
import { useSshTunnels, type UseSshTunnelsResult } from "./useSshTunnels";

export interface SshTunnelsPanelProps {
  /** The SSH inventory: row labels, and the machine an ad-hoc forward targets. */
  hosts: readonly SshHost[];
  /** Injected clock for tests; the stale threshold reads it instead of the wall clock. */
  clock?: () => number;
  /** The local daemon's capability list; when absent the panel assumes V1 support. */
  capabilities?: readonly string[] | null;
  /** Mount gate: an embedded panel that is not showing must not poll. */
  enabled?: boolean;
  /**
   * A snapshot the owner already reads. The settings section fetches the list once and hands
   * the result down, so the panel and the drawer editor render the same response instead of
   * each spending a `cmd_ssh_tunnel_list`; the owner owns the polling with it.
   */
  snapshot?: UseSshTunnelsResult;
}

const PORT_MIN = 1;
const PORT_MAX = 65_535;

type ParsedPort = { kind: "blank" } | { kind: "invalid" } | { kind: "ok"; value: number };

function parsePort(raw: string): ParsedPort {
  const trimmed = raw.trim();
  if (trimmed === "") return { kind: "blank" };
  if (!/^\d+$/.test(trimmed)) return { kind: "invalid" };
  const value = Number(trimmed);
  return value < PORT_MIN || value > PORT_MAX ? { kind: "invalid" } : { kind: "ok", value };
}

function StateBadge({ row, stale }: { row: TunnelRow; stale: boolean }) {
  const badge = tunnelBadge(row, stale);
  const classes = badge.state === "stale" ? TUNNEL_STALE_BADGE_CLASSES : TUNNEL_STATE_BADGE_CLASSES[row.state];
  return (
    <Badge
      variant="outline"
      data-testid="tunnel-state"
      data-state={badge.state}
      data-attempt={row.state === "reconnecting" ? String(row.attempt) : undefined}
      className={`h-4 shrink-0 px-1.5 py-0 text-[11px] ${classes}`}
    >
      {badge.label}
    </Badge>
  );
}

function TunnelRowView({ row, hostLabel, stale, busy, dismissible, onRestore, onStart, onStop, onDismiss }: {
  row: TunnelRow;
  hostLabel: string;
  stale: boolean;
  busy: boolean;
  /** Whether this stopped row exists only in the panel's local history. */
  dismissible: boolean;
  onRestore: () => void;
  onStart: () => void;
  onStop: () => void;
  onDismiss: () => void;
}) {
  const reason = row.state === "failed" ? describeTunnelFailure(row.failure, row.failurePiece) : "";
  return (
    <li
      role="listitem"
      data-testid="tunnel-row"
      data-host-id={row.hostId}
      data-spec-id={row.specId}
      data-state={row.state}
      data-since={row.since ?? undefined}
      data-saved={String(row.saved)}
      className="space-y-1.5 rounded-lg border border-border bg-card/50 p-3"
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className="truncate text-[13px] font-medium text-foreground">{row.label}</span>
        <Badge variant="outline" className="h-4 shrink-0 px-1.5 py-0 text-[11px]">{hostLabel}</Badge>
        <StateBadge row={row} stale={stale} />
        {row.state === "reconnecting" ? (
          <span className="text-[11px] text-muted-foreground" data-testid="tunnel-attempt">
            attempt {row.attempt}
          </span>
        ) : null}
        <span className="ml-auto text-[11px] text-muted-foreground" data-testid="tunnel-since">{formatSince(row.since)}</span>
      </div>
      <div className="flex flex-wrap items-center gap-1.5 font-mono text-[11px] text-muted-foreground">
        <span data-testid="tunnel-local-port">localhost:{row.localPort}</span>
        <ArrowRight aria-hidden="true" className="size-3" />
        <span data-testid="tunnel-remote-target">{`${row.remoteHost}:${row.remotePort}`}</span>
      </div>
      {reason ? (
        <p role="status" data-testid="tunnel-failure-reason" data-failure={row.failure} className="break-words text-[12px] text-destructive">
          {reason}
        </p>
      ) : null}
      <div className="flex flex-wrap gap-2">
        {row.state === "failed" ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label={`Restore ${row.label}`}
            disabled={busy}
            onClick={onRestore}
          >
            Restore
          </Button>
        ) : null}
        {row.state === "stopped" ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label={`Start ${row.label}`}
            disabled={busy}
            onClick={onStart}
          >
            Start
          </Button>
        ) : null}
        {dismissible ? (
          // Only a row the local history invented can be dismissed: it exists nowhere else,
          // while a saved definition's stopped row belongs to the store the operator still
          // edits, and a stopped view the daemon reported would come straight back.
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-label={`Dismiss ${row.label}`}
            disabled={busy}
            onClick={onDismiss}
          >
            Dismiss
          </Button>
        ) : null}
        {row.state === "listening" || row.state === "reconnecting" ? (
          <Button type="button" variant="outline" size="sm" aria-label={`Stop ${row.label}`} disabled={busy} onClick={onStop}>
            Stop
          </Button>
        ) : null}
      </div>
    </li>
  );
}

export function SshTunnelsPanel({ hosts, clock, capabilities, enabled = true, snapshot }: SshTunnelsPanelProps) {
  // The capability is decided from inputs the caller already has, never from a failed invoke.
  // A caller that knows the vector decides; one that only has a snapshot reads its verdict,
  // so a daemon that refused the read hides the panel instead of showing an error about a
  // feature it does not have. Without either, the v1 default stands for a plain embedder.
  const capable = capabilities != null
    ? hasSshTunnelCapability(capabilities)
    : snapshot
      ? snapshot.capability !== "absent"
      : true;
  const gated = enabled && capable;
  // An injected snapshot is the single reader: the panel's own hook stays inert, so the
  // section never asks the daemon for the same list twice.
  const own = useSshTunnels({ clock, enabled: gated && snapshot === undefined });
  const tunnels = snapshot ?? own;
  const now = clock ?? Date.now;
  const [form, setForm] = useState({ hostId: "", label: "", localPort: "", remoteHost: "", remotePort: "" });
  const [formError, setFormErrorState] = useState<string | null>(null);
  /* Why the form refused: a token the panel renders so a caller asserts the cause, not the copy. */
  const [formErrorKind, setFormErrorKind] = useState<string | null>(null);
  const setFormError = (message: string | null, kind: string | null = null) => {
    setFormErrorState(message);
    setFormErrorKind(kind);
  };
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // The state disables the buttons on the next render; the ref refuses a second click that
  // arrives before React has committed it, so one click is one request.
  const busyRef = useRef(false);

  const hostLabels = useMemo(() => {
    const map = new Map<string, string>();
    for (const host of hosts) map.set(host.id, host.label || host.hostname);
    return map;
  }, [hosts]);

  // `SshHost` carries `disabled`, not `enabled`: only a machine that is not disabled can take
  // a forward. The list is a memo so the selection effect below depends on identity, not on
  // the parent's array allocation.
  const eligibleHosts = useMemo(() => hosts.filter((host) => host.disabled !== true), [hosts]);

  // The inventory arrives asynchronously and rows can be disabled or removed while the form is
  // open, so the selection follows the eligible set: first eligible machine when nothing valid
  // is chosen, cleared when none is. It never keeps a host the operator can no longer use.
  useEffect(() => {
    setForm((prev) => {
      if (eligibleHosts.some((host) => host.id === prev.hostId)) return prev;
      const nextHostId = eligibleHosts[0]?.id ?? "";
      return prev.hostId === nextHostId ? prev : { ...prev, hostId: nextHostId };
    });
  }, [eligibleHosts]);

  const rows = useMemo(() => buildTunnelRows(tunnels.list, tunnels.retired), [tunnels.list, tunnels.retired]);
  // The keys only the local history knows about: a stopped row for one of these is the panel's
  // own invention and can be dismissed, unlike a stopped view the daemon reported.
  const retiredKeys = useMemo(() => new Set(tunnels.retired.map(viewKey)), [tunnels.retired]);
  const stale = isSnapshotStale(tunnels.receivedAt, now());

  if (!gated) return null;

  const runMutation = async (action: () => Promise<void>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setActionError(null);
    try {
      await action();
    } catch (error) {
      setActionError(extractIpcErrorMessage(error, "The tunnel action failed."));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const submitAdHoc = (event: FormEvent) => {
    event.preventDefault();
    setFormError(null);
    const remoteHost = form.remoteHost.trim();
    if (remoteHost === "") {
      setFormError("Remote host is required.", "remote-host-required");
      return;
    }
    const remotePort = parsePort(form.remotePort);
    if (remotePort.kind === "blank") {
      setFormError("Remote port is required.", "remote-port-required");
      return;
    }
    if (remotePort.kind === "invalid") {
      setFormError("Remote port must be an integer between 1 and 65535.", "remote-port-invalid");
      return;
    }
    const localPort = parsePort(form.localPort);
    if (localPort.kind === "invalid") {
      setFormError("Local port must be an integer between 1 and 65535.", "local-port-invalid");
      return;
    }
    const hostId = form.hostId;
    // No silent `hosts[0]` fallback: an empty or disabled selection is a validation failure the
    // operator sees, never a forward through a machine they did not pick.
    if (!eligibleHosts.some((host) => host.id === hostId)) {
      setFormError(
        eligibleHosts.length === 0
          ? "No enabled SSH machine can take a forward."
          : "Select an SSH machine that is not disabled.",
        eligibleHosts.length === 0 ? "no-enabled-host" : "host-not-eligible",
      );
      return;
    }
    const localPortValue = localPort.kind === "ok" ? localPort.value : null;
    // An ad-hoc spec omits `id`: the daemon mints one per runtime and never saves it.
    const spec: AdHocTunnelSpec = {
      label: form.label.trim() || `${localPortValue ?? "auto"}:${remoteHost}:${remotePort.value}`,
      localPort: localPortValue,
      remoteHost,
      remotePort: remotePort.value,
      enabled: true,
    };
    void runMutation(async () => {
      await tunnels.startAdHoc(hostId, spec);
      setForm({ hostId, label: "", localPort: "", remoteHost: "", remotePort: "" });
    });
  };

  return (
    <section aria-label="SSH tunnels" data-testid="ssh-tunnels-panel" className="space-y-3">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-[13px] font-medium text-foreground">SSH tunnels</h2>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label="Refresh SSH tunnels"
          disabled={tunnels.refreshing}
          onClick={() => void tunnels.refresh()}
        >
          <RefreshCw aria-hidden="true" className="size-3.5" />
        </Button>
      </div>

      {tunnels.error ? (
        <p role="alert" data-testid="ssh-tunnels-error" className="break-words text-[12px] text-status-warning">
          {tunnels.error}
        </p>
      ) : null}

      {actionError ? (
        <p role="alert" data-testid="ssh-tunnel-action-error" className="break-words text-[12px] text-destructive">
          {actionError}
        </p>
      ) : null}

      {rows.length === 0 ? (
        <p data-testid="ssh-tunnels-empty" className="text-[12px] text-muted-foreground">
          {tunnels.receivedAt === null ? "Reading tunnels\u2026" : "No tunnels are running."}
        </p>
      ) : (
        <ul aria-label="Tunnels" className="space-y-2">
          {rows.map((row) => (
            <TunnelRowView
              key={row.key}
              row={row}
              hostLabel={hostLabels.get(row.hostId) ?? row.hostId}
              stale={stale}
              busy={busy}
              dismissible={!row.saved && retiredKeys.has(row.key)}
              onRestore={() => void runMutation(() => tunnels.startSaved(row))}
              onStart={() => void runMutation(() => tunnels.startSaved(row))}
              onStop={() => void runMutation(() => tunnels.stop(row))}
              onDismiss={() => tunnels.dismissStopped(row)}
            />
          ))}
        </ul>
      )}

      <form
        aria-label="Forward a port now"
        data-testid="ssh-tunnel-adhoc-form"
        className="space-y-2 rounded border border-border bg-muted/30 p-3"
        onSubmit={submitAdHoc}
      >
        <p className="text-[12px] font-medium text-foreground">Forward a port now</p>
        <div className="space-y-1">
          <Label className="text-[12px]" htmlFor="adhoc-tunnel-host">Machine</Label>
          <Select
            value={form.hostId}
            onValueChange={(value: string) => setForm((prev) => ({ ...prev, hostId: value }))}
          >
            <SelectTrigger id="adhoc-tunnel-host" className="h-7 text-[12px]">
              <SelectValue placeholder="Select a machine" />
            </SelectTrigger>
            <SelectContent>
              {hosts.map((host) => (
                <SelectItem key={host.id} value={host.id} disabled={host.disabled === true}>
                  {host.label || host.hostname}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
          <div className="space-y-1">
            <Label className="text-[12px]" htmlFor="adhoc-tunnel-label">Tunnel label</Label>
            <Input
              id="adhoc-tunnel-label"
              className="h-7 text-[12px]"
              value={form.label}
              autoComplete="off"
              spellCheck={false}
              onChange={(event) => setForm((prev) => ({ ...prev, label: event.target.value }))}
            />
          </div>
          <div className="space-y-1">
            <Label className="text-[12px]" htmlFor="adhoc-tunnel-local-port">Local port</Label>
            <Input
              id="adhoc-tunnel-local-port"
              className="h-7 text-[12px]"
              inputMode="numeric"
              placeholder="auto"
              value={form.localPort}
              autoComplete="off"
              onChange={(event) => setForm((prev) => ({ ...prev, localPort: event.target.value }))}
            />
          </div>
          <div className="space-y-1">
            <Label className="text-[12px]" htmlFor="adhoc-tunnel-remote-host">Remote host</Label>
            <Input
              id="adhoc-tunnel-remote-host"
              className="h-7 text-[12px]"
              placeholder="127.0.0.1"
              value={form.remoteHost}
              autoComplete="off"
              spellCheck={false}
              onChange={(event) => setForm((prev) => ({ ...prev, remoteHost: event.target.value }))}
            />
          </div>
          <div className="space-y-1">
            <Label className="text-[12px]" htmlFor="adhoc-tunnel-remote-port">Remote port</Label>
            <Input
              id="adhoc-tunnel-remote-port"
              className="h-7 text-[12px]"
              inputMode="numeric"
              value={form.remotePort}
              autoComplete="off"
              onChange={(event) => setForm((prev) => ({ ...prev, remotePort: event.target.value }))}
            />
          </div>
        </div>
        {formError ? (
          <p role="alert" data-testid="ssh-tunnel-form-error" data-error-kind={formErrorKind ?? undefined} className="text-[12px] text-destructive">{formError}</p>
        ) : null}
        <div className="flex justify-end">
          <Button type="submit" size="sm" disabled={busy}>Forward port</Button>
        </div>
      </form>
    </section>
  );
}
