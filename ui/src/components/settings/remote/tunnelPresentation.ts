/**
 * Display helpers for the shared SSH tunnels panel.
 *
 * Everything here is pure so the panel's badge, `since` and failure copy can be tested from
 * machine values: the component never invents a state token, and a failure's text is looked up
 * in a closed table instead of echoing a raw stack or stderr string.
 */
import {
  TUNNEL_FAILURE_CODES,
  type HostTunnelDefinitions,
  type TunnelFailureCode,
  type TunnelListResponse,
  type TunnelState,
  type TunnelView,
} from "../../../lib/sshTunnels";

/** A snapshot older than this is rendered as `stale`, never as a confident `listening`. */
export const TUNNEL_STALE_AFTER_MS = 15_000;

/** The panel's only refresh cadence: one in-flight list call, never faster than this. */
export const TUNNEL_REFRESH_MS = 5_000;

export const TUNNEL_STATE_LABELS: Record<TunnelState, string> = {
  stopped: "Stopped",
  listening: "Listening",
  reconnecting: "Reconnecting",
  failed: "Failed",
};

export const TUNNEL_STATE_BADGE_CLASSES: Record<TunnelState, string> = {
  listening: "border-status-success/50 text-status-success",
  reconnecting: "border-status-warning/50 text-status-warning",
  failed: "border-destructive/50 text-destructive",
  stopped: "border-border/70 text-muted-foreground",
};

/** The stale marker replaces a `listening` badge; it is not a fifth runtime state. */
export const TUNNEL_STALE_BADGE_CLASSES = "border-status-warning/50 text-status-warning";

/** One short sentence per `TunnelFailureCode` (MH-4's ten codes). */
export const TUNNEL_FAILURE_REASONS: Record<TunnelFailureCode, string> = {
  port_in_use: "The local port is already in use.",
  host_unreachable: "The SSH host could not be reached.",
  auth_failed: "The host rejected the SSH credentials.",
  host_key_rejected: "The host key changed or is not trusted.",
  channel_failed: "The forwarding channel failed.",
  unsupported_platform: "This platform cannot forward ports.",
  unsupported_config: "The host's SSH configuration is not supported.",
  daemon_unavailable: "The background service is not available.",
  credential_unavailable: "The stored credential could not be read.",
  host_changed: "The machine's connection details changed.",
};

/**
 * The 14-token `unsupported_config` vocabulary owned by Todo 1's `UNSUPPORTED_CONFIG_PIECES`.
 * A piece outside this table renders the code's own sentence — never the raw token.
 */
export const TUNNEL_FAILURE_PIECE_REASONS: Record<string, string> = {
  config_alias: "the host is an SSH config alias.",
  config_path: "a config Include or file path is used.",
  config_only_endpoint: "the endpoint exists only in the SSH config.",
  config_only_jump: "the jump host exists only in the SSH config.",
  jump_identity: "the jump host uses its own identity.",
  jump_config: "the jump host relies on SSH config options.",
  jump_password: "the jump host needs a password prompt.",
  identity_agent: "the identity comes from a running agent.",
  identity_file_token: "the identity file path uses an unsupported token.",
  certificate_file: "a certificate file is configured.",
  match_block: "a Match block applies to this host.",
  bad_option: "an SSH option is not understood.",
  algorithm_mismatch: "the configured algorithms cannot be negotiated.",
  unsupported_directive: "the SSH configuration uses an unsupported directive.",
};

/** Canonical order of the closed vocabulary, so a table drift is visible in one place. */
export const TUNNEL_FAILURE_PIECE_TOKENS = Object.keys(TUNNEL_FAILURE_PIECE_REASONS);

export interface TunnelRow {
  /** Stable row identity: host plus the (minted or saved) spec id. */
  key: string;
  hostId: string;
  specId: string;
  runtimeId: string | null;
  label: string;
  localPort: number;
  remoteHost: string;
  remotePort: number;
  state: TunnelState;
  since: number | null;
  attempt: number;
  failure: TunnelFailureCode | null;
  failurePiece: string | null;
  /** `definitions` membership is the only proof a spec is saved; never `specId` alone. */
  saved: boolean;
}

function rowKey(hostId: string, specId: string): string {
  return `${hostId}\u0000${specId}`;
}

/** Stable identity of a view: the host plus the (saved or minted) spec id. */
export function viewKey(view: TunnelView): string {
  return rowKey(view.hostId, view.specId);
}

function savedSpecKeys(definitions: readonly HostTunnelDefinitions[]): Set<string> {
  const keys = new Set<string>();
  for (const host of definitions) {
    for (const spec of host.specs) {
      keys.add(rowKey(host.hostId, spec.id));
    }
  }
  return keys;
}

function rowFromView(view: TunnelView, saved: boolean): TunnelRow {
  return {
    key: rowKey(view.hostId, view.specId),
    hostId: view.hostId,
    specId: view.specId,
    runtimeId: view.runtimeId,
    label: view.label,
    localPort: view.localPort,
    remoteHost: view.remoteHost,
    remotePort: view.remotePort,
    state: view.state,
    since: view.since,
    attempt: view.attempt,
    failure: view.failure,
    failurePiece: view.failurePiece,
    saved,
  };
}

/**
 * Joins the newest response with the views the panel already saw: a row whose runtime id is
 * missing from the newest response drops to `stopped` instead of keeping a green badge. The
 * daemon only reports running runtimes, so absence is the stop signal this client has.
 */
export function buildTunnelRows(
  list: TunnelListResponse | null,
  retired: readonly TunnelView[] = [],
): TunnelRow[] {
  if (!list) {
    return [];
  }
  const saved = savedSpecKeys(list.definitions);
  const rows = list.views.map((view) => rowFromView(view, saved.has(rowKey(view.hostId, view.specId))));
  const liveRuntimeIds = new Set(rows.map((row) => row.runtimeId).filter((id): id is string => id !== null));
  const presentKeys = new Set(rows.map((row) => row.key));
  const stopped: TunnelRow[] = [];
  for (const view of retired) {
    if (presentKeys.has(viewKey(view))) continue;
    if (view.runtimeId !== null && liveRuntimeIds.has(view.runtimeId)) continue;
    stopped.push({
      ...rowFromView(view, saved.has(rowKey(view.hostId, view.specId))),
      runtimeId: null,
      state: "stopped",
      attempt: 0,
      failure: null,
      failurePiece: null,
    });
  }
  return [...rows, ...stopped];
}

/** A snapshot the injected clock says is older than the threshold is stale, not green. */
export function isSnapshotStale(receivedAt: number | null, now: number, staleAfterMs = TUNNEL_STALE_AFTER_MS): boolean {
  return receivedAt !== null && now - receivedAt > staleAfterMs;
}

/**
 * The badge a row shows. A stale snapshot can only downgrade `listening`: a failed tunnel stays
 * failed (its reason is still the operator's to act on) and a stopped row stays stopped.
 */
export function tunnelBadge(row: TunnelRow, stale: boolean): { state: TunnelState | "stale"; label: string } {
  if (stale && row.state === "listening") {
    return { state: "stale", label: "Stale" };
  }
  return { state: row.state, label: TUNNEL_STATE_LABELS[row.state] };
}

/** The typed reason text for a failed row; the piece narrows `unsupported_config` only. */
export function describeTunnelFailure(failure: TunnelFailureCode | null, piece: string | null): string {
  if (failure === null) return "";
  const base = TUNNEL_FAILURE_REASONS[failure] ?? "The tunnel failed.";
  if (failure !== "unsupported_config" || piece === null) return base;
  const detail = TUNNEL_FAILURE_PIECE_REASONS[piece];
  return detail ? `${base} Reason: ${detail}` : base;
}

/** Local wall-clock label for a runtime's `since`; the machine value stays in `data-since`. */
export function formatSince(since: number | null): string {
  if (since === null) return "never started";
  const date = new Date(since);
  if (Number.isNaN(date.getTime())) return "unknown";
  return `since ${date.toLocaleTimeString()}`;
}

/** Keeps a drifted copy table honest: every MH-4 code has a sentence. */
export function missingFailureReasonCodes(): string[] {
  return TUNNEL_FAILURE_CODES.filter((code) => !(code in TUNNEL_FAILURE_REASONS));
}
