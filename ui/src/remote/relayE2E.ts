import type { HostEndpoint } from "../state/remoteHostStore";
import type { AttachKeyPair } from "./accountAttach";
import type { OpenTunnelParams } from "./accountSession";
import type { FetchLikeInit, TunnelResponse, TunnelTransport, TunnelWebSocket } from "./attachTunnel";
import { remoteApiUrl, remoteSocketUrl, SocketTicketError } from "./remoteClient";

/** Typed code for a relay host this client cannot encrypt to (no attach key, e.g. paired by PIN). */
export const REPAIR_REQUIRED = "REPAIR_REQUIRED";
/** The relay's refusal of a plaintext proxy request (HTTP 426). */
export const E2E_REQUIRED = "E2E_REQUIRED";

export const RELAY_REPAIR_MESSAGE =
  "This device was paired with a PIN, so relay traffic to this machine is not end-to-end encrypted. Re-pair it through your Ferryx account to encrypt it.";

export class RelayRepairRequiredError extends Error {
  readonly code = REPAIR_REQUIRED;
  constructor(message: string = RELAY_REPAIR_MESSAGE) {
    super(message);
    this.name = "RelayRepairRequiredError";
  }
}

/** A typed failure to allocate or open an end-to-end encrypted relay session. */
export class RelayE2EError extends Error {
  readonly code: string;
  readonly status?: number;
  constructor(code: string, message: string, status?: number) {
    super(message);
    this.name = "RelayE2EError";
    this.code = code;
    this.status = status;
  }
}

export const CLIENT_BUILD_HEADER = "X-Ferryx-Client-Build";
/**
 * Response header a relay that understands `?e2e=1` sends on every `/attach/session` answer.
 * With it present, a 4xx is a real refusal (typed RelayE2EError), never "unsupported".
 */
export const RELAY_E2E_AWARE_HEADER = "x-ferryx-relay-e2e";

/**
 * The relay does not know the device-token `?e2e=1` allocation yet (400, 404, 405, or any 401 -
 * an old account relay answers a device token with 401 UNAUTHORIZED): clients ship before the
 * relay, so a paired relay host falls back to the plaintext path for that base. Account mode
 * never allocates here (it rides its own account tunnel), so it is unaffected.
 */
export class RelayE2EUnsupportedError extends Error {
  readonly code = "E2E_UNSUPPORTED";
  readonly status: number;
  constructor(status: number) {
    super(`Relay does not support encrypted sessions yet (${status})`);
    this.name = "RelayE2EUnsupportedError";
    this.status = status;
  }
}

/** The build identity sent to the relay so it can tell which clients still use plaintext. */
export function clientBuild(): string {
  return typeof __FERRYX_BUILD__ === "string" && __FERRYX_BUILD__ ? __FERRYX_BUILD__ : "dev";
}

/** Moves bytes for one machine through an encrypted attach session instead of the relay proxy. */
export interface RelayE2ETransport {
  fetchLike(pathAndQuery: string, init?: FetchLikeInit): Promise<TunnelResponse>;
  openWebSocket(pathAndQuery: string, headers?: Record<string, string>): Promise<TunnelWebSocket>;
}

const registry = new Map<string, RelayE2ETransport>();
/** Bases whose relay answered the `?e2e=1` allocation as unsupported; plaintext for this page session. */
const plaintextFallback = new Set<string>();

function registryKey(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, "");
}

/** Routes every remoteFetch/remoteOpenSocket for `baseUrl` through `transport`; returns the unregister function. */
export function registerRelayE2E(baseUrl: string, transport: RelayE2ETransport): () => void {
  const key = registryKey(baseUrl);
  registry.set(key, transport);
  // A fresh registration (new token, new connection) gets a fresh chance at encryption.
  plaintextFallback.delete(key);
  return () => {
    if (registry.get(key) === transport) registry.delete(key);
  };
}

export function relayE2EFor(baseUrl: string): RelayE2ETransport | null {
  const key = registryKey(baseUrl);
  if (plaintextFallback.has(key)) return null;
  return registry.get(key) ?? null;
}

function rememberPlaintextFallback(baseUrl: string): void {
  const key = registryKey(baseUrl);
  if (plaintextFallback.has(key)) return;
  plaintextFallback.add(key);
  console.warn(`Relay does not support end-to-end encrypted sessions yet; using the plaintext relay path for ${key}`);
}

/** Test hook: forget every remembered plaintext fallback. */
export function resetRelayE2EFallbacks(): void {
  plaintextFallback.clear();
}

function isRelayHostUrl(url: string): boolean {
  try {
    return new URL(url, window.location.origin).pathname.startsWith("/host/");
  } catch {
    return false;
  }
}

function headerRecord(headers: HeadersInit | undefined): Record<string, string> {
  if (!headers) return {};
  if (headers instanceof Headers || Array.isArray(headers)) {
    const out: Record<string, string> = {};
    new Headers(headers).forEach((value, key) => {
      out[key] = value;
    });
    return out;
  }
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(headers)) out[key] = String(value);
  return out;
}

function withClientBuild(headers: HeadersInit | undefined): HeadersInit {
  if (headers instanceof Headers || Array.isArray(headers)) {
    const copy = new Headers(headers);
    copy.set(CLIENT_BUILD_HEADER, clientBuild());
    return copy;
  }
  return { ...(headers ?? {}), [CLIENT_BUILD_HEADER]: clientBuild() };
}

async function bodyBytes(body: BodyInit | null | undefined): Promise<Uint8Array | string | undefined> {
  if (body === null || body === undefined) return undefined;
  if (typeof body === "string") return body;
  if (body instanceof Uint8Array) return body;
  if (body instanceof ArrayBuffer) return new Uint8Array(body);
  if (ArrayBuffer.isView(body)) return new Uint8Array(body.buffer, body.byteOffset, body.byteLength);
  if (body instanceof URLSearchParams) return body.toString();
  return new Uint8Array(await new Response(body).arrayBuffer());
}

function tunnelResponse(raw: TunnelResponse): Response {
  const nullBody = raw.status === 204 || raw.status === 205 || raw.status === 304;
  return new Response(nullBody ? null : raw.body.slice(), { status: raw.status, headers: raw.headers });
}

function errorCode(data: unknown): string | null {
  if (!data || typeof data !== "object") return null;
  const record = data as { code?: unknown; error?: { code?: unknown } };
  if (typeof record.code === "string") return record.code;
  if (record.error && typeof record.error === "object" && typeof record.error.code === "string") return record.error.code;
  return null;
}

async function throwIfE2ERequired(response: Response): Promise<void> {
  if (response.status !== 426) return;
  let code: string | null = null;
  try {
    code = errorCode(await response.clone().json());
  } catch {
    code = null;
  }
  if (code === E2E_REQUIRED) throw new RelayRepairRequiredError();
}

/**
 * One host API request. A base registered with registerRelayE2E is reached only inside its
 * encrypted tunnel; any other base uses plain fetch, tagged with the client build on relay
 * `/host/` URLs. A relay refusing plaintext (426 E2E_REQUIRED) rejects with RelayRepairRequiredError.
 */
export async function remoteFetch(baseUrl: string, path: string, init: RequestInit = {}): Promise<Response> {
  const e2e = relayE2EFor(baseUrl);
  if (e2e) {
    init.signal?.throwIfAborted();
    try {
      const raw = await e2e.fetchLike(path, {
        method: init.method,
        headers: headerRecord(init.headers),
        body: await bodyBytes(init.body),
        signal: init.signal ?? undefined,
      });
      init.signal?.throwIfAborted();
      return tunnelResponse(raw);
    } catch (error) {
      if (!(error instanceof RelayE2EUnsupportedError)) throw error;
      rememberPlaintextFallback(baseUrl);
    }
  }
  const url = remoteApiUrl(baseUrl, path);
  const response = await fetch(url, isRelayHostUrl(url) ? { ...init, headers: withClientBuild(init.headers) } : init);
  await throwIfE2ERequired(response);
  return response;
}

/**
 * Where one host WebSocket comes from: an open socket inside the registered tunnel (bearer in the
 * tunnel), or a single-use ticketed URL the caller dials itself. Returning the URL lets the caller
 * construct the native socket and attach its handlers in the same tick, so no early frame is lost.
 */
export async function remoteSocketTarget(
  baseUrl: string,
  target: string,
  token: string,
  signal?: AbortSignal,
): Promise<string | TunnelWebSocket> {
  const e2e = relayE2EFor(baseUrl);
  if (e2e) {
    try {
      return await e2e.openWebSocket(target, { Authorization: `Bearer ${token}` });
    } catch (error) {
      if (!(error instanceof RelayE2EUnsupportedError)) throw error;
      rememberPlaintextFallback(baseUrl);
    }
  }
  let url: string;
  // Tickets bind the path; a query (terminal geometry) rides on the socket URL itself.
  const queryAt = target.indexOf("?");
  const path = queryAt === -1 ? target : target.slice(0, queryAt);
  try {
    url = await remoteSocketUrl(baseUrl, path, token, signal);
  } catch (error) {
    if (error instanceof SocketTicketError && error.status === 426) throw new RelayRepairRequiredError();
    throw error;
  }
  if (queryAt !== -1) {
    const socketUrl = new URL(url);
    for (const [key, value] of new URLSearchParams(target.slice(queryAt + 1))) socketUrl.searchParams.set(key, value);
    url = socketUrl.toString();
  }
  signal?.throwIfAborted();
  return url;
}

/** One host WebSocket: inside the registered tunnel (bearer in the tunnel) or by a single-use ticket. */
export async function remoteOpenSocket(
  baseUrl: string,
  target: string,
  token: string,
  signal?: AbortSignal,
): Promise<WebSocket | TunnelWebSocket> {
  const opened = await remoteSocketTarget(baseUrl, target, token, signal);
  return typeof opened === "string" ? new WebSocket(opened) : opened;
}

type OpenedTunnel = { transport: TunnelTransport; close: () => void };

export interface RelayE2EDeps {
  fetch?: typeof fetch;
  openTunnel?: (params: OpenTunnelParams) => Promise<OpenedTunnel>;
}

export interface OpenRelayE2ETransportParams {
  relayOrigin: string;
  machineId: string;
  enrollmentEpoch: string | number;
  machineAttachPublicKey: string;
  /** The device attach key; a promise is awaited on first use, so registration never waits for it. */
  attachKey: AttachKeyPair | Promise<AttachKeyPair>;
  /** Device token (or account session token) authorizing the `?e2e=1` allocation. */
  bearer: string;
  deps?: RelayE2EDeps;
}

export interface RelayE2EConnection extends RelayE2ETransport {
  close(): void;
  readonly isClosed: boolean;
}

const E2E_ALLOCATION_TIMEOUT_MS = 15_000;

/** `POST {relay}/api/v1/attach/session?e2e=1`; failures are typed and bounded by a deadline. */
export async function allocateE2ESession(
  relayOrigin: string,
  bearer: string,
  machineId: string,
  fetchImpl: typeof fetch = fetch,
): Promise<string> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), E2E_ALLOCATION_TIMEOUT_MS);
  let response: Response;
  try {
    response = await fetchImpl(`${relayOrigin.replace(/\/+$/, "")}/api/v1/attach/session?e2e=1`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${bearer}`,
        "Content-Type": "application/json",
        [CLIENT_BUILD_HEADER]: clientBuild(),
      },
      body: JSON.stringify({ machineId }),
      signal: controller.signal,
    });
  } catch (error) {
    if (controller.signal.aborted) {
      throw new RelayE2EError("REQUEST_TIMEOUT", `Encrypted session allocation timed out after ${E2E_ALLOCATION_TIMEOUT_MS}ms`);
    }
    throw error;
  } finally {
    clearTimeout(timer);
  }
  let data: unknown = null;
  try {
    data = await response.json();
  } catch {
    data = null;
  }
  if (!response.ok) {
    const code = errorCode(data);
    if (code === E2E_REQUIRED || response.status === 426) throw new RelayRepairRequiredError();
    // A relay that understands `?e2e=1` says so in a header; its 401 is an auth denial
    // (sign out / repair), never a reason to downgrade.
    const e2eAware = response.headers?.get(RELAY_E2E_AWARE_HEADER) === "1";
    if (e2eAware && response.status === 401) {
      throw new RelayE2EError("UNAUTHORIZED", "Encrypted session allocation was refused (401)", 401);
    }
    // Only an old relay without that header falls back to plaintext: 400, 404, 405, or any 401
    // (an old account relay answers a device token with 401 {"code":"UNAUTHORIZED"}). 426/E2E_REQUIRED,
    // 409, 402 and other statuses stay typed errors.
    if (
      !e2eAware
      && (response.status === 400 || response.status === 401 || response.status === 404 || response.status === 405)
    ) {
      throw new RelayE2EUnsupportedError(response.status);
    }
    throw new RelayE2EError(
      code ?? "ALLOCATE_SESSION_FAILED",
      `Encrypted session allocation failed (${response.status})`,
      response.status,
    );
  }
  const sessionId = data && typeof data === "object" ? (data as { sessionId?: unknown }).sessionId : undefined;
  if (typeof sessionId !== "string" || !sessionId) {
    throw new RelayE2EError("INVALID_RESPONSE", "Encrypted session allocation returned no session id", response.status);
  }
  return sessionId;
}

/**
 * Encrypted transport to one relay-paired machine. HTTP shares one attach session, opened lazily
 * and reopened once it closes; every WebSocket gets its own fresh attach session.
 */
export function openRelayE2ETransport(params: OpenRelayE2ETransportParams): RelayE2EConnection {
  const fetchImpl = params.deps?.fetch ?? ((input: RequestInfo | URL, init?: RequestInit) => fetch(input, init));
  // Loaded on first use: remoteClient re-exports this module, and a static import would pull the
  // account/inventory modules into every importer of remoteClient.
  const openTunnelImpl = params.deps?.openTunnel
    ?? (async (tunnelParams: OpenTunnelParams) => (await import("./accountSession")).openTunnel(tunnelParams));
  const attachKey = Promise.resolve(params.attachKey);
  // A key that cannot be created surfaces on the first request, never as an unhandled rejection.
  attachKey.catch(() => {});
  let closed = false;
  let http: OpenedTunnel | null = null;
  let opening: Promise<OpenedTunnel> | null = null;
  const sockets = new Set<{ socket: TunnelWebSocket; tunnel: OpenedTunnel }>();

  const assertOpen = () => {
    if (closed) throw new RelayE2EError("CONNECTION_CLOSED", "Encrypted relay connection has been closed");
  };

  const openSession = async (): Promise<OpenedTunnel> => {
    const localKeyPair = await attachKey;
    const sessionId = await allocateE2ESession(params.relayOrigin, params.bearer, params.machineId, fetchImpl);
    assertOpen();
    return openTunnelImpl({
      relayOrigin: params.relayOrigin,
      machineId: params.machineId,
      enrollmentEpoch: params.enrollmentEpoch,
      machineAttachPublicKey: params.machineAttachPublicKey,
      localKeyPair,
      sessionId,
    });
  };

  const httpTunnel = async (): Promise<OpenedTunnel> => {
    assertOpen();
    if (http && http.transport.isClosed !== true) return http;
    http = null;
    if (!opening) {
      opening = openSession()
        .then((tunnel) => {
          if (closed) {
            tunnel.close();
            throw new RelayE2EError("CONNECTION_CLOSED", "Encrypted relay connection has been closed");
          }
          http = tunnel;
          return tunnel;
        })
        .finally(() => {
          opening = null;
        });
    }
    return opening;
  };

  return {
    async fetchLike(pathAndQuery: string, init?: FetchLikeInit): Promise<TunnelResponse> {
      const tunnel = await httpTunnel();
      return tunnel.transport.fetchLike(pathAndQuery, init);
    },
    async openWebSocket(pathAndQuery: string, headers?: Record<string, string>): Promise<TunnelWebSocket> {
      assertOpen();
      const tunnel = await openSession();
      if (closed) {
        tunnel.close();
        assertOpen();
      }
      let socket: TunnelWebSocket;
      try {
        socket = await tunnel.transport.openWebSocket(pathAndQuery, headers);
      } catch (error) {
        tunnel.close();
        throw error;
      }
      for (const entry of sockets) if (entry.socket.readyState === 3) sockets.delete(entry);
      sockets.add({ socket, tunnel });
      return socket;
    },
    close(): void {
      if (closed) return;
      closed = true;
      http?.close();
      http = null;
      for (const entry of sockets) {
        try {
          entry.socket.close();
        } catch {
          /* the tunnel close below still releases it */
        }
        entry.tunnel.close();
      }
      sockets.clear();
    },
    get isClosed(): boolean {
      return closed;
    },
  };
}

/**
 * How traffic to `host` travels: "direct" when not through its relay, "e2e" when the relay only
 * carries ciphertext, "plaintext-fallback" when an old relay forced this page session back to
 * the plaintext path, REPAIR_REQUIRED for a relay host this client holds no attach info for.
 */
export function relayE2EState(
  host: HostEndpoint | null,
  transportUrl: string,
): "direct" | "e2e" | "plaintext-fallback" | typeof REPAIR_REQUIRED {
  if (!host?.relayOrigin || !host.machineId) return "direct";
  let origin: string;
  try {
    origin = new URL(transportUrl).origin;
  } catch {
    return "direct";
  }
  if (origin !== host.relayOrigin) return "direct";
  if (!host.attachPublicKey || !host.enrollmentEpoch) return REPAIR_REQUIRED;
  const base = registryKey(`${host.relayOrigin}/host/${encodeURIComponent(host.machineId)}`);
  return plaintextFallback.has(base) ? "plaintext-fallback" : "e2e";
}
