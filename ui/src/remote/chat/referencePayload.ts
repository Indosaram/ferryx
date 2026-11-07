/**
 * Authenticated rev3 payload read. The ref is opaque; this client never sends a host path.
 *
 * One request. An unknown result is not retried. A target switch aborts through `AbortSignal`
 * and that rejection is not turned into a payload result.
 */
import { remoteFetch } from "../relayE2E";
import { referenceChatRoute, type ReferencePayloadRef, type ReferenceTargetRef } from "./referenceTypes";

export type ReferencePayloadKind = "output" | "diff";

export type ReferencePayloadResult =
  | {
      readonly status: "loaded";
      readonly kind: ReferencePayloadKind;
      readonly body: string;
      readonly byteLength: number;
      readonly digest: string;
      readonly cap: number;
      readonly truncated: false;
    }
  | {
      readonly status: "truncated";
      readonly kind: ReferencePayloadKind;
      readonly body: string;
      readonly byteLength: number;
      readonly digest: string;
      readonly cap: number;
      readonly truncated: true;
    }
  | { readonly status: "expired" }
  | { readonly status: "notFound" }
  | { readonly status: "unavailable"; readonly code: string };

export interface ReferencePayloadDeps {
  readonly baseUrl: string;
  readonly token: string;
  readonly registryId: string;
  readonly signal?: AbortSignal;
  /** Test seam. Production calls `remoteFetch`. */
  readonly fetchImpl?: (path: string, init: RequestInit) => Promise<Response>;
}

let boundRoute: ReferencePayloadDeps | null = null;

/** Credentials for the signal-only call RemoteApp already makes. A null binding clears it. */
export function bindReferencePayloadRoute(next: ReferencePayloadDeps | null): void {
  boundRoute = next;
}

function isAbortSignal(value: AbortSignal | ReferencePayloadDeps): value is AbortSignal {
  return typeof (value as AbortSignal).aborted === "boolean" && typeof (value as AbortSignal).addEventListener === "function";
}

function refText(ref: string | ReferencePayloadRef): string {
  return typeof ref === "string" ? ref : ref.ref;
}

function abortError(): Error {
  const error = new Error("The operation was aborted");
  error.name = "AbortError";
  return error;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function payloadQuery(sessionId: string, target: ReferenceTargetRef, registryId: string, ref: string): string {
  const { ownerId, epoch, backendSessionId } = target.target;
  const params = new URLSearchParams();
  params.set("ownerId", ownerId);
  params.set("epoch", epoch);
  params.set("backendSessionId", backendSessionId);
  params.set("registryId", registryId);
  const providerSessionId = target.providerSessionId;
  if (typeof providerSessionId === "string" && providerSessionId.trim().length > 0) {
    params.set("providerSessionId", providerSessionId);
  }
  params.set("ref", ref);
  return `${referenceChatRoute(sessionId, "payload")}?${params.toString()}`;
}

function errorCode(body: unknown, status: number): string {
  if (isRecord(body) && isRecord(body.error) && typeof body.error.code === "string") {
    return body.error.code;
  }
  if (status === 410) return "TARGET_EXPIRED";
  if (status === 404) return "NOT_FOUND";
  return "REQUEST_FAILED";
}

function loadedResult(body: unknown): ReferencePayloadResult | null {
  if (!isRecord(body)) return null;
  const status = body.status;
  const kind = body.kind;
  if ((status !== "loaded" && status !== "truncated") || (kind !== "output" && kind !== "diff")) {
    return null;
  }
  if (typeof body.body !== "string" || typeof body.byteLength !== "number" || typeof body.digest !== "string") {
    return null;
  }
  if (typeof body.cap !== "number" || typeof body.truncated !== "boolean") return null;
  if (status === "truncated" || body.truncated) {
    return {
      status: "truncated",
      kind,
      body: body.body,
      byteLength: body.byteLength,
      digest: body.digest,
      cap: body.cap,
      truncated: true,
    };
  }
  return {
    status: "loaded",
    kind,
    body: body.body,
    byteLength: body.byteLength,
    digest: body.digest,
    cap: body.cap,
    truncated: false,
  };
}

/**
 * Fetch the full body for one payload ref.
 *
 * `signal` aborts the read when the caller switches targets. Unknown and transport failures
 * become `unavailable` and are not retried.
 */
export async function loadReferencePayload(
  sessionId: string,
  target: ReferenceTargetRef,
  ref: string | ReferencePayloadRef,
  signalOrDeps?: AbortSignal | ReferencePayloadDeps,
): Promise<ReferencePayloadResult> {
  const text = refText(ref);
  const deps = signalOrDeps === undefined
    ? boundRoute
    : isAbortSignal(signalOrDeps)
      ? boundRoute === null
        ? null
        : { ...boundRoute, signal: signalOrDeps }
      : signalOrDeps;
  if (signalOrDeps !== undefined && isAbortSignal(signalOrDeps) && signalOrDeps.aborted) throw abortError();
  if (deps === null || deps.token.length === 0 || deps.registryId.length === 0) {
    return { status: "unavailable", code: "UNAVAILABLE" };
  }
  if (deps.signal?.aborted) throw abortError();
  if (text.includes("/") || text.includes("\\") || text.includes("..") || text.trim().length === 0) {
    return { status: "unavailable", code: "INVALID_REQUEST" };
  }
  const path = payloadQuery(sessionId, target, deps.registryId, text);
  const init: RequestInit = {
    headers: { Authorization: `Bearer ${deps.token}` },
    signal: deps.signal,
  };
  let response: Response;
  try {
    response = deps.fetchImpl
      ? await deps.fetchImpl(path, init)
      : await remoteFetch(deps.baseUrl, path, init);
  } catch (error) {
    if (deps.signal?.aborted) throw error;
    return { status: "unavailable", code: "NETWORK_ERROR" };
  }
  if (deps.signal?.aborted) throw abortError();
  if (response.ok) {
    let parsed: unknown;
    try {
      parsed = await response.json();
    } catch {
      return { status: "unavailable", code: "MALFORMED_RESPONSE" };
    }
    return loadedResult(parsed) ?? { status: "unavailable", code: "MALFORMED_RESPONSE" };
  }
  let parsed: unknown = null;
  try {
    parsed = await response.json();
  } catch {
    parsed = null;
  }
  const code = errorCode(parsed, response.status);
  if (code === "TARGET_EXPIRED" || response.status === 410) return { status: "expired" };
  if (code === "NOT_FOUND" || response.status === 404) return { status: "notFound" };
  return { status: "unavailable", code };
}
