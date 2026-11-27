/**
 * Remote adapter for the structured-chat transport (lane G7).
 *
 * The gateway serves `/api/v1/structured-chat` with the device bearer token every other remote
 * surface uses. This module is the binding point: `bindStructuredChatRemote` installs the
 * credentials the app already holds, and a caller with no binding gets a typed refusal rather
 * than a fetch to an empty base URL.
 */
import { remoteFetch } from "../remote/relayE2E";
import {
  structuredChatHttpWire,
  structuredChatTransportFrom,
  type StructuredChatOp,
  type StructuredChatRequest,
  type StructuredChatResponse,
  type StructuredChatTransport,
} from "./structuredChatTransport";

export interface StructuredChatRemoteDeps {
  readonly baseUrl: string;
  readonly token: string;
  /** Test seam. Production calls `remoteFetch`. */
  readonly fetchImpl?: (path: string, init: RequestInit) => Promise<Response>;
}

let boundDeps: StructuredChatRemoteDeps | null = null;

/** Install the credentials the remote surface uses. `null` clears them. */
export function bindStructuredChatRemote(deps: StructuredChatRemoteDeps | null): void {
  boundDeps = deps;
}

export function structuredChatRemoteDeps(): StructuredChatRemoteDeps | null {
  return boundDeps;
}

/** Build the transport for one binding; `null` deps yields a transport that refuses everything. */
export function remoteStructuredChatTransport(deps: StructuredChatRemoteDeps | null = boundDeps): StructuredChatTransport {
  if (deps === null) {
    return structuredChatTransportFrom(async () => ({
      status: 401,
      body: { error: { code: "UNAUTHORIZED", message: "This host is not connected yet." } },
    }));
  }
  return structuredChatTransportFrom(
    structuredChatHttpWire(deps, (path, init) => remoteFetch(deps.baseUrl, path, init)),
  );
}

/** The transport over the current binding, read at call time so a rebind takes effect at once. */
export function remoteStructuredChat<K extends StructuredChatOp>(
  request: StructuredChatRequest<K>,
  signal: AbortSignal,
): StructuredChatResponse<K> {
  return remoteStructuredChatTransport(boundDeps)(request, signal);
}
