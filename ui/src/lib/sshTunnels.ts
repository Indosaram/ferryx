/**
 * Typed bridge to the sshTunnelV1 command family.
 *
 * Nothing here renders or caches: the settings surfaces own their snapshots, `sshHosts.ts`
 * stays the only host inventory, and every caller gates on `hasSshTunnelCapability` before
 * it sends a request (MH-10). The seven command names below are Todo 8's, verbatim; the
 * payload keys are Todo 3's camelCase wire names.
 *
 * Two wire rules are easy to get wrong and are enforced here rather than at the call sites:
 *  - an ad-hoc spec omits `id` instead of inventing one (`TunnelSpec.id` is `#[serde(default)]`
 *    server-side so the daemon can mint it at accept time), and
 *  - `identity` and `expectedRevision` are OMITTED when absent, never sent as `null`
 *    (`TunnelInvalidateHostRequest.identity` and `expectedRevision` are `Option` precisely so
 *    an older caller can leave the key out).
 */
import { invoke } from "@tauri-apps/api/core";

import { extractIpcErrorMessage, type SshHost } from "./sshHosts";

export const SSH_TUNNEL_CAPABILITY = "sshTunnelV1";

/** Wire names of Todo 8's seven commands. Never build these by interpolation. */
export const SSH_TUNNEL_COMMANDS = {
  list: "cmd_ssh_tunnel_list",
  start: "cmd_ssh_tunnel_start",
  stop: "cmd_ssh_tunnel_stop",
  invalidateHost: "cmd_ssh_tunnel_invalidate_host",
  specsSave: "cmd_ssh_tunnel_specs_save",
  specsReset: "cmd_ssh_tunnel_specs_reset",
  forgetHost: "cmd_ssh_tunnel_forget_host",
} as const;

/** A rejection that carried no usable code still surfaces a real IpcErrorCode token. */
export const SSH_TUNNEL_FALLBACK_ERROR_CODE = "INTERNAL_ERROR";

export function hasSshTunnelCapability(capabilities?: readonly string[] | null): boolean {
  if (!capabilities || !Array.isArray(capabilities)) {
    return false;
  }
  return capabilities.includes(SSH_TUNNEL_CAPABILITY);
}

/** The four runtime states, as machine tokens (Todo 1's snake_case serde values). */
export const TUNNEL_STATES = ["stopped", "listening", "reconnecting", "failed"] as const;

export type TunnelState = (typeof TUNNEL_STATES)[number];

/**
 * The ten runtime failure codes (MH-4). These are `TunnelFailureCode` values and have no
 * relationship to `IpcErrorCode`: a refusal carries the tunnel code in `details.failureCode`.
 */
export const TUNNEL_FAILURE_CODES = [
  "port_in_use",
  "host_unreachable",
  "auth_failed",
  "host_key_rejected",
  "channel_failed",
  "unsupported_platform",
  "unsupported_config",
  "daemon_unavailable",
  "credential_unavailable",
  "host_changed",
] as const;

export type TunnelFailureCode = (typeof TUNNEL_FAILURE_CODES)[number];

export function isTunnelFailureCode(value: unknown): value is TunnelFailureCode {
  return typeof value === "string" && (TUNNEL_FAILURE_CODES as readonly string[]).includes(value);
}

export interface TunnelSpec {
  /** Saved definitions always carry their id; the daemon mints one for an ad-hoc start. */
  id: string;
  label: string;
  /** `null` means "the daemon allocates"; only an ad-hoc start may leave it out. */
  localPort: number | null;
  remoteHost: string;
  remotePort: number;
  enabled: boolean;
}

/** An ad-hoc start payload: `id` is minted by the daemon when the key is absent. */
export interface AdHocTunnelSpec extends Omit<TunnelSpec, "id" | "localPort"> {
  id?: string;
  localPort?: number | null;
}

/** The post-mutation host tuple the daemon fingerprints against a start's revision. */
export interface TunnelHostIdentity {
  hostname: string;
  username: string | null;
  port: number | null;
  identityFile: string | null;
  jumpHost: string | null;
  disabled: boolean | null;
}

export interface TunnelView {
  /** Non-optional on the wire: an ad-hoc start gets a minted, unsaved spec id. */
  specId: string;
  /** Absent exactly when `state` is `stopped`. */
  runtimeId: string | null;
  label: string;
  hostId: string;
  localPort: number;
  remoteHost: string;
  remotePort: number;
  state: TunnelState;
  /** Unix epoch milliseconds of the current state; absent when never started. */
  since: number | null;
  attempt: number;
  failure: TunnelFailureCode | null;
  /** Qualifier for `unsupported_config`; the sole wire carrier (never `details.piece`). */
  failurePiece: string | null;
}

export interface HostTunnelDefinitions {
  hostId: string;
  specs: TunnelSpec[];
}

export interface HostRevision {
  hostId: string;
  revision: number;
}

export type StoreStatusKind = "ok" | "corrupt" | "partial" | "unsupported_version";

export interface StoreStatus {
  kind: StoreStatusKind;
  path: string;
  /** Wire form of the store's `host_has_invalid_definitions`; non-empty exactly when `partial`. */
  invalidHosts: string[];
  quarantinedPath: string | null;
}

export interface TunnelListResponse {
  views: TunnelView[];
  definitions: HostTunnelDefinitions[];
  /** The authoritative revision per host; an absent entry reads as 0 and is startable at 0. */
  hostRevisions: HostRevision[];
  /** The compare-and-swap input a save must present. */
  definitionsRevision: number;
  storeStatus: StoreStatus;
}

export interface TunnelStartResponse {
  /** A first-attempt terminal failure answers `Ok` with `state: "failed"`, never a rejection. */
  view: TunnelView;
  acceptedRevision: number;
}

export interface TunnelStopResponse {
  /** Idempotent: an unknown or already removed runtime answers `false`, not an error. */
  stopped: boolean;
}

export interface TunnelInvalidateHostResponse {
  /** The revision the barrier moved the table to; compare it to decide the retry. */
  revision: number;
}

export interface TunnelSpecsSaveResponse {
  definitionsRevision: number;
}

export interface TunnelSpecsResetResponse {
  quarantinedPath: string | null;
}

export interface TunnelForgetHostResponse {
  removed: number;
}

export interface StartTunnelOptions {
  hostId: string;
  /** Exactly one of `specId` / `spec`. */
  specId?: string;
  spec?: AdHocTunnelSpec;
  /** The value last read from `listTunnels().hostRevisions`, never invented. */
  hostRevision: number;
}

export interface SaveHostTunnelsOptions {
  /** Todo 2's escape hatch for a partially corrupt row; omitted unless true. */
  discardInvalid?: boolean;
  /** The only path for a `null`-revision save; omitted unless true. */
  force?: boolean;
}

/** A rejection from a tunnel command with the IpcErrorCode kept as-is. */
export class SshTunnelBridgeError extends Error {
  readonly code: string;
  readonly reason: string | null;
  readonly failureCode: TunnelFailureCode | null;
  readonly details: Record<string, unknown> | null;

  constructor(code: string, message: string, details: Record<string, unknown> | null) {
    super(message);
    this.name = "SshTunnelBridgeError";
    this.code = code;
    this.details = details;
    const reason = details?.reason;
    const failureCode = details?.failureCode;
    this.reason = typeof reason === "string" ? reason : null;
    this.failureCode = isTunnelFailureCode(failureCode) ? failureCode : null;
  }
}

function plainObject(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/**
 * Reads a rejection into `SshTunnelBridgeError`. The code and the tunnel classification are
 * read from the object's fields, never recovered by parsing the message.
 */
export function toSshTunnelBridgeError(error: unknown, fallbackMessage: string): SshTunnelBridgeError {
  if (error instanceof SshTunnelBridgeError) {
    return error;
  }
  const record = plainObject(error);
  const message = extractIpcErrorMessage(error, fallbackMessage);
  const details = plainObject(record?.details);
  const rawCode = record?.code;
  const code =
    typeof rawCode === "string" && rawCode.trim() !== "" ? rawCode : SSH_TUNNEL_FALLBACK_ERROR_CODE;
  return new SshTunnelBridgeError(code, message, details);
}

async function callTunnelCommand<T>(
  command: string,
  args: Record<string, unknown> | null,
  fallbackMessage: string,
): Promise<T> {
  try {
    return args === null ? await invoke<T>(command) : await invoke<T>(command, args);
  } catch (error) {
    throw toSshTunnelBridgeError(error, fallbackMessage);
  }
}

/** An ad-hoc spec on the wire: absent keys stay absent, so no `id` is fabricated. */
function adHocSpecPayload(spec: AdHocTunnelSpec): Record<string, unknown> {
  const payload: Record<string, unknown> = {
    label: spec.label,
    remoteHost: spec.remoteHost,
    remotePort: spec.remotePort,
    enabled: spec.enabled,
  };
  if (spec.id !== undefined) {
    payload.id = spec.id;
  }
  if (spec.localPort !== undefined) {
    payload.localPort = spec.localPort;
  }
  return payload;
}

export function listTunnels(): Promise<TunnelListResponse> {
  return callTunnelCommand<TunnelListResponse>(
    SSH_TUNNEL_COMMANDS.list,
    null,
    "Failed to read SSH tunnels.",
  );
}

export function startTunnel(options: StartTunnelOptions): Promise<TunnelStartResponse> {
  const args: Record<string, unknown> = {
    hostId: options.hostId,
    hostRevision: options.hostRevision,
  };
  if (options.specId !== undefined) {
    args.specId = options.specId;
  }
  if (options.spec !== undefined) {
    args.spec = adHocSpecPayload(options.spec);
  }
  return callTunnelCommand<TunnelStartResponse>(
    SSH_TUNNEL_COMMANDS.start,
    args,
    "Failed to start the SSH tunnel.",
  );
}

export function stopTunnel(runtimeId: string): Promise<TunnelStopResponse> {
  return callTunnelCommand<TunnelStopResponse>(
    SSH_TUNNEL_COMMANDS.stop,
    { runtimeId },
    "Failed to stop the SSH tunnel.",
  );
}

/**
 * The host-mutation barrier. `identity` is the POST-mutation tuple; forward it verbatim when
 * the caller has one and omit the key when it does not (never `null`).
 */
export function invalidateHost(
  hostId: string,
  hostRevision: number,
  identity?: TunnelHostIdentity,
): Promise<TunnelInvalidateHostResponse> {
  const args: Record<string, unknown> = { hostId, hostRevision };
  if (identity !== undefined) {
    args.identity = identity;
  }
  return callTunnelCommand<TunnelInvalidateHostResponse>(
    SSH_TUNNEL_COMMANDS.invalidateHost,
    args,
    "Failed to invalidate the host's SSH tunnels.",
  );
}

/**
 * Replaces the host's saved definitions. `expectedRevision` is the `definitionsRevision` from
 * the list call that produced `specs`; `null` means a deliberate overwrite and requires
 * `force`, which is the only path Todo 2 documents for it.
 */
export function saveHostTunnels(
  hostId: string,
  specs: readonly TunnelSpec[],
  expectedRevision: number | null,
  options?: SaveHostTunnelsOptions,
): Promise<TunnelSpecsSaveResponse> {
  const args: Record<string, unknown> = { hostId, specs };
  if (expectedRevision !== null) {
    args.expectedRevision = expectedRevision;
  }
  if (options?.discardInvalid === true) {
    args.discardInvalid = true;
  }
  if (options?.force === true) {
    args.force = true;
  }
  return callTunnelCommand<TunnelSpecsSaveResponse>(
    SSH_TUNNEL_COMMANDS.specsSave,
    args,
    "Failed to save the host's SSH tunnels.",
  );
}

/** Quarantines an unparseable definitions file and returns where it went. */
export function resetHostTunnelStore(): Promise<TunnelSpecsResetResponse> {
  return callTunnelCommand<TunnelSpecsResetResponse>(
    SSH_TUNNEL_COMMANDS.specsReset,
    null,
    "Failed to reset the SSH tunnel store.",
  );
}

/** Drops a deleted host's definitions; the delete path sends this beside the host write. */
export function forgetHostTunnels(hostId: string): Promise<TunnelForgetHostResponse> {
  return callTunnelCommand<TunnelForgetHostResponse>(
    SSH_TUNNEL_COMMANDS.forgetHost,
    { hostId },
    "Failed to forget the host's SSH tunnels.",
  );
}

/**
 * The revision a start must present for `hostId`. An absent entry reads as 0 and is startable
 * at 0, which is what the daemon compares against; nothing here guesses a higher number.
 */
export function hostRevisionFromList(list: TunnelListResponse, hostId: string): number {
  return list.hostRevisions.find((row) => row.hostId === hostId)?.revision ?? 0;
}

/**
 * The client-side join that gives `SshHost` its `tunnels` field. Derived from the definitions
 * response only: never persisted and never sent back in a host update payload. With the
 * capability absent no host gains the field at all.
 */
export function withHostTunnels(
  hosts: readonly SshHost[],
  definitions: readonly HostTunnelDefinitions[],
  capabilities?: readonly string[] | null,
): SshHost[] {
  if (!hasSshTunnelCapability(capabilities)) {
    return [...hosts];
  }
  const byHost = new Map<string, TunnelSpec[]>();
  for (const definition of definitions) {
    byHost.set(definition.hostId, definition.specs);
  }
  return hosts.map((host) => ({ ...host, tunnels: byHost.get(host.id) ?? [] }));
}
