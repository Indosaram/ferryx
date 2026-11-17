import { useEffect, useRef, useState } from "react";

import type { SshHost } from "../../../lib/sshHosts";
import {
  hasSshTunnelCapability,
  type SaveHostTunnelsOptions,
  type StoreStatus,
  type TunnelListResponse,
  type TunnelSpec,
  type TunnelView,
} from "../../../lib/sshTunnels";
import { Button } from "../../ui/button";
import { Input } from "../../ui/input";
import { Label } from "../../ui/label";
import { Switch } from "../../ui/switch";

/**
 * The saved-tunnel editor inside the SSH machine drawer.
 *
 * Definitions only: a saved row always carries a concrete local port, because
 * `validate_saved_spec` rejects `None` (the "let the daemon pick" affordance is the ad-hoc
 * form's, whose spec never reaches the store). Nothing here invents a second host store:
 * every mutation goes through the handlers `useSshActions` binds to the tunnel bridge, and
 * the list snapshot arrives as a prop.
 *
 * Two gates are load-bearing:
 *  - `hasSshTunnelCapability` is checked before the body mounts, so an older daemon gets the
 *    explanation and the surface issues **zero** invokes (MH-10), and
 *  - validation mirrors `validate_spec`/`validate_specs_for_host` on the backend: port
 *    `1..=65535`, a non-empty hostname/address, and no duplicate local port inside one host.
 */

/** `details.reason` a save answers when the presented revision is stale. */
export const TUNNEL_STORE_CONFLICT_REASON = "tunnel_store_conflict";
/** `details.reason` a save answers when it presented no revision at all. */
export const TUNNEL_REVISION_REQUIRED_REASON = "revision_required";

export const TUNNELS_CAPABILITY_MESSAGE = "SSH tunnels need a newer background service.";
export const TUNNELS_CORRUPT_MESSAGE =
  "The saved tunnel definitions file could not be read. Resetting moves it aside and starts empty.";

export interface TunnelSectionError {
  /** The `IpcErrorCode` the daemon answered with. */
  code: string;
  /** The typed `details.reason`, when the refusal carried one. */
  reason: string | null;
  message: string;
}

export interface TunnelSaveConflict {
  /** The typed reason: `tunnel_store_conflict` or `revision_required`. */
  reason: string;
  message: string;
  /** The revision the refused save presented; `null` when it presented none. */
  expectedRevision: number | null;
}

/** Everything the section needs; `useSshActions` produces this object once per section. */
export interface SshTunnelsEditorHandlers {
  /** The daemon's advertised capabilities; `undefined`/`[]` both read as "absent". */
  capabilities: readonly string[] | null;
  list: TunnelListResponse | null;
  loading: boolean;
  error: TunnelSectionError | null;
  /** Where a reset quarantined the unreadable file, once one succeeded. */
  resetPath: string | null;
  conflict: TunnelSaveConflict | null;
  onRefresh: () => void;
  onSave: (
    hostId: string,
    specs: readonly TunnelSpec[],
    options?: SaveHostTunnelsOptions,
  ) => void;
  onStart: (hostId: string, specId: string) => void;
  onStop: (runtimeId: string) => void;
  onReset: () => void;
}

export interface SshTunnelsEditorProps extends SshTunnelsEditorHandlers {
  host: SshHost;
  busy: boolean;
}

export interface TunnelDraft {
  /** The definition being edited; `null` while adding. */
  id: string | null;
  label: string;
  localPort: string;
  remoteHost: string;
  remotePort: string;
  enabled: boolean;
}

export interface TunnelDraftError {
  field: keyof Omit<TunnelDraft, "id">;
  message: string;
}

let draftCounter = 0;

/** A saved definition's id is opaque to the store; uniqueness inside one host is the rule. */
export function newTunnelSpecId(): string {
  draftCounter += 1;
  return `tunnel-${Date.now().toString(36)}-${draftCounter}`;
}

/** The backend's port range, applied to the raw field text so `""` and `1e3` both fail. */
export function parseTunnelPort(value: string): number | null {
  const trimmed = value.trim();
  if (!/^[0-9]+$/.test(trimmed)) {
    return null;
  }
  const port = Number(trimmed);
  return port >= 1 && port <= 65535 ? port : null;
}

/**
 * The client half of the backend's validation. These checks run before any invoke, so an
 * invalid row never reaches the daemon and never rewrites the definitions file.
 */
export function validateTunnelDraft(
  draft: TunnelDraft,
  siblings: readonly TunnelSpec[],
): TunnelDraftError | null {
  if (draft.label.trim() === "") {
    return { field: "label", message: "Label is required." };
  }
  const localPort = parseTunnelPort(draft.localPort);
  if (localPort === null) {
    return { field: "localPort", message: "Local port must be a whole number between 1 and 65535." };
  }
  const remotePort = parseTunnelPort(draft.remotePort);
  if (remotePort === null) {
    return { field: "remotePort", message: "Remote port must be a whole number between 1 and 65535." };
  }
  const remoteHost = draft.remoteHost.trim();
  if (remoteHost === "") {
    return { field: "remoteHost", message: "Remote host is required." };
  }
  if (/[\s/\0]/.test(remoteHost)) {
    return { field: "remoteHost", message: "Remote host must be a hostname or IP address." };
  }
  const duplicate = siblings.some((spec) => spec.id !== draft.id && spec.localPort === localPort);
  if (duplicate) {
    return {
      field: "localPort",
      message: `Local port ${localPort} is already used by another tunnel on this host.`,
    };
  }
  return null;
}

/** The wire spec for a validated draft; `null` when a port field still fails to parse. */
export function tunnelSpecFromDraft(draft: TunnelDraft, id: string): TunnelSpec | null {
  const localPort = parseTunnelPort(draft.localPort);
  const remotePort = parseTunnelPort(draft.remotePort);
  if (localPort === null || remotePort === null) {
    return null;
  }
  return {
    id,
    label: draft.label.trim(),
    localPort,
    remoteHost: draft.remoteHost.trim(),
    remotePort,
    enabled: draft.enabled,
  };
}

function draftFor(spec: TunnelSpec | null): TunnelDraft {
  if (!spec) {
    return { id: null, label: "", localPort: "", remoteHost: "", remotePort: "", enabled: true };
  }
  return {
    id: spec.id,
    label: spec.label,
    localPort: spec.localPort === null ? "" : String(spec.localPort),
    remoteHost: spec.remoteHost,
    remotePort: String(spec.remotePort),
    enabled: spec.enabled,
  };
}

function storeStatusOf(list: TunnelListResponse | null): StoreStatus | null {
  return list?.storeStatus ?? null;
}

/**
 * Whether the store refuses writes outright. A corrupt file and a newer-versioned file both
 * answer every save with an error, so the section offers only the explanation and, for the
 * corrupt case, the reset.
 */
function storeRefusesWrites(status: StoreStatus | null): boolean {
  return status?.kind === "corrupt" || status?.kind === "unsupported_version";
}

export function SshTunnelsEditor(props: SshTunnelsEditorProps) {
  // The gate comes first: an older daemon never mounts the body, so nothing it owns can
  // issue an invoke. `undefined` reads as absent, exactly like `hasSshTunnelCapability`.
  if (!hasSshTunnelCapability(props.capabilities)) {
    return (
      <section
        aria-label="Tunnels"
        data-testid="ssh-tunnels-section"
        data-capability="absent"
        className="space-y-2 rounded border border-border bg-muted/30 p-3"
      >
        <h3 className="text-[13px] font-semibold text-foreground">Tunnels</h3>
        <p data-testid="ssh-tunnels-capability" className="text-[12px] text-muted-foreground">
          {TUNNELS_CAPABILITY_MESSAGE}
        </p>
      </section>
    );
  }
  return <SshTunnelsBody {...props} />;
}

function SshTunnelsBody(props: SshTunnelsEditorProps) {
  const { host, busy, list, loading, error, conflict, resetPath } = props;
  const [draft, setDraft] = useState<TunnelDraft | null>(null);
  const [formError, setFormError] = useState<TunnelDraftError | null>(null);
  // A refused save keeps the specs the operator typed, so the force path saves them and
  // not the stale list; it clears as soon as the definitions revision moves.
  const [pendingSave, setPendingSave] = useState<readonly TunnelSpec[] | null>(null);

  const refreshRef = useRef(props.onRefresh);
  refreshRef.current = props.onRefresh;
  // Reading the snapshot once per open drawer is the section's only automatic request; a probe
  // that already answered with a snapshot must not cause a second one.
  const initialListRef = useRef(list);
  useEffect(() => {
    if (initialListRef.current === null) refreshRef.current();
  }, []);

  const revision = list?.definitionsRevision ?? null;
  useEffect(() => {
    setPendingSave(null);
  }, [revision]);

  const status = storeStatusOf(list);
  const definitions = list?.definitions.find((entry) => entry.hostId === host.id)?.specs ?? [];
  const views: TunnelView[] = (list?.views ?? []).filter((view) => view.hostId === host.id);
  const corrupt = status?.kind === "corrupt";
  const unsupportedVersion = status?.kind === "unsupported_version";
  const partial = status?.kind === "partial" && status.invalidHosts.includes(host.id);
  const recoveryOnly = storeRefusesWrites(status);

  const beginEdit = (spec: TunnelSpec | null) => {
    setFormError(null);
    setDraft(draftFor(spec));
  };

  const submitDraft = () => {
    if (!draft) {
      return;
    }
    const failure = validateTunnelDraft(draft, definitions);
    if (failure) {
      setFormError(failure);
      return;
    }
    const spec = tunnelSpecFromDraft(draft, draft.id ?? newTunnelSpecId());
    if (!spec) {
      setFormError({ field: "localPort", message: "Local port is not valid." });
      return;
    }
    const next = draft.id === null
      ? [...definitions, spec]
      : definitions.map((existing) => (existing.id === spec.id ? spec : existing));
    setFormError(null);
    setPendingSave(next);
    setDraft(null);
    // A label-only edit is still one save; nothing here stops or restarts a running tunnel.
    props.onSave(host.id, next);
  };

  const removeSpec = (spec: TunnelSpec) => {
    const next = definitions.filter((existing) => existing.id !== spec.id);
    setPendingSave(next);
    setDraft(null);
    props.onSave(host.id, next);
  };

  const forceSave = () => {
    const specs = pendingSave ?? definitions;
    props.onSave(host.id, specs, { force: true });
  };

  const form = draft ? (
    <form
      className="space-y-2 rounded border border-border bg-muted/30 p-2"
      onSubmit={(event) => {
        event.preventDefault();
        submitDraft();
      }}
    >
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
        <div className="space-y-1">
          <Label className="text-[12px]">Label</Label>
          <Input
            aria-label="Label"
            className="h-7 text-[12px]"
            value={draft.label}
            autoComplete="off"
            spellCheck={false}
            onChange={(event) => setDraft({ ...draft, label: event.target.value })}
          />
        </div>
        <div className="space-y-1">
          <Label className="text-[12px]">Local port</Label>
          <Input
            aria-label="Local port"
            className="h-7 text-[12px] font-mono"
            value={draft.localPort}
            inputMode="numeric"
            autoComplete="off"
            spellCheck={false}
            onChange={(event) => setDraft({ ...draft, localPort: event.target.value })}
          />
        </div>
        <div className="space-y-1">
          <Label className="text-[12px]">Remote host</Label>
          <Input
            aria-label="Remote host"
            className="h-7 text-[12px] font-mono"
            value={draft.remoteHost}
            autoComplete="off"
            spellCheck={false}
            onChange={(event) => setDraft({ ...draft, remoteHost: event.target.value })}
          />
        </div>
        <div className="space-y-1">
          <Label className="text-[12px]">Remote port</Label>
          <Input
            aria-label="Remote port"
            className="h-7 text-[12px] font-mono"
            value={draft.remotePort}
            inputMode="numeric"
            autoComplete="off"
            spellCheck={false}
            onChange={(event) => setDraft({ ...draft, remotePort: event.target.value })}
          />
        </div>
      </div>
      <div className="flex items-center gap-2">
        <Switch
          aria-label="Enabled"
          checked={draft.enabled}
          onCheckedChange={(checked) => setDraft({ ...draft, enabled: checked === true })}
        />
        <span className="text-[12px] text-muted-foreground">Start with this tunnel enabled</span>
      </div>
      {formError ? (
        <p role="alert" data-testid="ssh-tunnels-form-error" data-field={formError.field} className="text-[12px] text-destructive">
          {formError.message}
        </p>
      ) : null}
      <div className="flex justify-end gap-2">
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => { setDraft(null); setFormError(null); }}>
          Cancel
        </Button>
        <Button type="submit" size="sm" disabled={busy}>
          Save tunnel
        </Button>
      </div>
    </form>
  ) : null;

  return (
    <section
      aria-label="Tunnels"
      data-testid="ssh-tunnels-section"
      data-capability="present"
      data-store-kind={status?.kind ?? "unknown"}
      className="space-y-3 rounded border border-border bg-muted/30 p-3"
    >
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-[13px] font-semibold text-foreground">Tunnels</h3>
        {!recoveryOnly && draft === null ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label={`Add tunnel to ${host.label}`}
            disabled={busy || loading}
            onClick={() => beginEdit(null)}
          >
            Add tunnel
          </Button>
        ) : null}
      </div>

      {corrupt ? (
        <div data-testid="ssh-tunnels-corrupt" className="space-y-2 rounded border border-destructive/40 bg-destructive/10 p-2 text-[12px]">
          <p className="text-destructive">{TUNNELS_CORRUPT_MESSAGE}</p>
          <Button type="button" variant="destructive" size="sm" disabled={busy} onClick={props.onReset}>
            Reset tunnel store
          </Button>
        </div>
      ) : null}

      {resetPath ? (
        <p data-testid="ssh-tunnels-quarantined-path" className="break-all font-mono text-[11px] text-muted-foreground">
          {resetPath}
        </p>
      ) : null}

      {unsupportedVersion ? (
        <p data-testid="ssh-tunnels-unsupported-version" className="text-[12px] text-status-warning">
          This tunnel definitions file was written by a newer Ferryx. Update Ferryx before editing these tunnels.
        </p>
      ) : null}

      {partial ? (
        <div data-testid="ssh-tunnels-partial" className="space-y-2 rounded border border-status-warning/40 bg-status-warning/10 p-2 text-[12px]">
          <p>
            Some saved definitions could not be read:{" "}
            <span data-testid="ssh-tunnels-invalid-hosts" className="font-mono text-[11px]">
              {status?.invalidHosts.join(", ")}
            </span>
          </p>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={busy}
            onClick={() => props.onSave(host.id, definitions, { discardInvalid: true })}
          >
            Discard unreadable definitions
          </Button>
        </div>
      ) : null}

      {!recoveryOnly && definitions.length === 0 && draft === null ? (
        <p className="text-[12px] text-muted-foreground">No saved tunnels for this machine.</p>
      ) : null}

      {!recoveryOnly ? (
        <ul aria-label={`Saved tunnels on ${host.label}`} className="space-y-1">
          {definitions.map((spec) => {
            const view = views.find((candidate) => candidate.specId === spec.id);
            const state = view?.state ?? "stopped";
            const running = state === "listening" || state === "reconnecting";
            return (
              <li
                key={spec.id}
                data-testid={`ssh-tunnel-row-${spec.id}`}
                data-state={state}
                data-enabled={String(spec.enabled)}
                className="space-y-1 rounded border border-border/60 bg-background/60 p-2 text-[12px]"
              >
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-medium text-foreground">{spec.label}</span>
                  <span className="font-mono text-[11px] text-muted-foreground">:{spec.localPort ?? "auto"}</span>
                  <span className="font-mono text-[11px] text-muted-foreground">
                    {spec.remoteHost}:{spec.remotePort}
                  </span>
                  <span data-testid={`ssh-tunnel-enabled-${spec.id}`} className="text-[11px] text-muted-foreground">
                    {spec.enabled ? "Enabled" : "Disabled"}
                  </span>
                  <span data-testid={`ssh-tunnel-state-${spec.id}`} data-state={state} className="text-[11px] text-muted-foreground">
                    {state}
                  </span>
                  {view?.failure ? (
                    <span data-testid={`ssh-tunnel-failure-${spec.id}`} data-failure={view.failure} className="text-[11px] text-destructive">
                      {view.failure}
                    </span>
                  ) : null}
                </div>
                <div className="flex flex-wrap gap-2">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    aria-label={`Start tunnel ${spec.label}`}
                    disabled={busy || !spec.enabled || running}
                    onClick={() => props.onStart(host.id, spec.id)}
                  >
                    Start
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    aria-label={`Stop tunnel ${spec.label}`}
                    disabled={busy || !view?.runtimeId}
                    onClick={() => {
                      if (view?.runtimeId) props.onStop(view.runtimeId);
                    }}
                  >
                    Stop
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    aria-label={`Edit tunnel ${spec.label}`}
                    disabled={busy}
                    onClick={() => beginEdit(spec)}
                  >
                    Edit
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    aria-label={`Remove tunnel ${spec.label}`}
                    disabled={busy}
                    onClick={() => removeSpec(spec)}
                  >
                    Remove
                  </Button>
                </div>
                {draft?.id === spec.id ? form : null}
              </li>
            );
          })}
        </ul>
      ) : null}

      {draft?.id === null ? form : null}

      {error ? (
        <p
          role="alert"
          data-testid="ssh-tunnels-error"
          data-code={error.code}
          data-reason={error.reason ?? ""}
          className="break-words text-[12px] text-destructive"
        >
          {error.message}
        </p>
      ) : null}

      {conflict ? (
        <div data-testid="ssh-tunnels-conflict" data-reason={conflict.reason} className="space-y-2 rounded border border-status-warning/40 bg-status-warning/10 p-2 text-[12px]">
          <p>{conflict.message}</p>
          <Button type="button" variant="outline" size="sm" disabled={busy} onClick={forceSave}>
            Overwrite with these definitions
          </Button>
        </div>
      ) : null}
    </section>
  );
}
