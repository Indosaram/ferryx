import type { HostEndpoint } from "../state/remoteHostStore";

// Host API calls go through these: a relay base with an encrypted session never sees plaintext.
export { remoteFetch, remoteOpenSocket, remoteSocketTarget } from "./relayE2E";

export function hostTransportUrl(host: HostEndpoint | null, transportUrl: string): string {
  return host?.relayOrigin && host.machineId && new URL(transportUrl).origin === host.relayOrigin
    ? `${host.relayOrigin}/host/${encodeURIComponent(host.machineId)}`
    : transportUrl;
}

export function remoteApiUrl(baseUrl: string, path: string): string {
  return baseUrl === window.location.origin ? path : `${baseUrl}${path}`;
}

export class SocketTicketError extends Error {
  readonly status: number;
  readonly isAuthDenial: boolean;
  constructor(status: number, message?: string) {
    super(message ?? `Socket ticket request failed (${status})`);
    this.name = "SocketTicketError";
    this.status = status;
    this.isAuthDenial = status === 401 || status === 403;
  }
}

/** Deadline for a plain host request (socket ticket, plaintext relay/host API call). */
export const REMOTE_REQUEST_TIMEOUT_MS = 15_000;

/** A plain host request that got no response within its deadline. */
export class RemoteRequestTimeoutError extends Error {
  readonly code = "REQUEST_TIMEOUT";
  constructor(timeoutMs: number) {
    super(`Remote request timed out after ${timeoutMs}ms`);
    this.name = "RemoteRequestTimeoutError";
  }
}

/**
 * `fetch` bounded by `timeoutMs` and the caller's signal (combined by hand: AbortSignal.any is
 * missing on older Safari and some test environments). A caller abort rejects as fetch does;
 * the deadline rejects with RemoteRequestTimeoutError.
 */
export async function fetchWithDeadline(
  input: string,
  init: RequestInit = {},
  timeoutMs: number = REMOTE_REQUEST_TIMEOUT_MS,
): Promise<Response> {
  const controller = new AbortController();
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    controller.abort();
  }, timeoutMs);
  const callerSignal = init.signal ?? null;
  const onCallerAbort = () => controller.abort(callerSignal?.reason);
  if (callerSignal?.aborted) controller.abort(callerSignal.reason);
  else callerSignal?.addEventListener("abort", onCallerAbort, { once: true });
  try {
    return await fetch(input, { ...init, signal: controller.signal });
  } catch (error) {
    if (timedOut) throw new RemoteRequestTimeoutError(timeoutMs);
    throw error;
  } finally {
    clearTimeout(timer);
    callerSignal?.removeEventListener("abort", onCallerAbort);
  }
}

/** Response header carrying the host's replacement bearer token after a transparent rotation. */
export const ROTATED_TOKEN_HEADER = "x-ferryx-rotated-token";

type TokenRotationListener = (previous: string, fresh: string) => void;
const tokenRotationListeners = new Set<TokenRotationListener>();

/** Subscribe to host-issued token rotations; returns the unsubscribe function. */
export function onDeviceTokenRotated(listener: TokenRotationListener): () => void {
  tokenRotationListeners.add(listener);
  return () => {
    tokenRotationListeners.delete(listener);
  };
}

/**
 * Announces a replacement bearer token the host answered for `previous`. Call it on the
 * reconnect reads (`/api/v1/socket-ticket`, `/api/v1/capabilities`); through a relay only the
 * proxied capabilities read reaches the host, since the relay mints its own socket tickets.
 */
export function noteTokenRotation(previous: string, response: Response): void {
  // Some callers' fetch doubles carry no headers object; treat that as "no rotation".
  const fresh = response.headers?.get(ROTATED_TOKEN_HEADER)?.trim();
  if (!fresh || fresh === previous) return;
  for (const listener of tokenRotationListeners) listener(previous, fresh);
}

/** Each dial (including retries) gets its own target-bound, single-use ticket. */
export async function remoteSocketUrl(
  baseUrl: string,
  target: string,
  deviceToken: string,
  signal?: AbortSignal,
): Promise<string> {
  const base = new URL(baseUrl);
  const socket = new URL(`${baseUrl}${target}`);
  socket.protocol = base.protocol === "https:" ? "wss:" : "ws:";
  const response = await fetchWithDeadline(`${baseUrl}/api/v1/socket-ticket`, {
    method: "POST",
    headers: { Authorization: `Bearer ${deviceToken}`, "Content-Type": "application/json" },
    body: JSON.stringify({ target }),
    signal,
  });
  if (!response.ok) {
    throw new SocketTicketError(response.status, `Socket ticket request failed (${response.status})`);
  }
  noteTokenRotation(deviceToken, response);
  const data: unknown = await response.json();
  if (!data || typeof data !== "object" || !("ticket" in data)
    || typeof data.ticket !== "string" || !data.ticket) throw new Error("Invalid socket ticket response");
  socket.searchParams.set("ticket", data.ticket);
  return socket.toString();
}
