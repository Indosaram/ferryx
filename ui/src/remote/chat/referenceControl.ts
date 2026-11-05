/**
 * Client for one reference-chat control apply (plan T09, section 4.3).
 *
 * The host maps control ids to adapter commands. This module never sends a command
 * template, never retries, and never treats a missing disposition as applied.
 * `agent-picker` does not POST: the prompt card answers the native picker.
 * `start-only` is not a mid-session apply.
 */
import type { TargetRef } from "../../lib/scopedContracts";
import {
  REFERENCE_OUTCOME_UNKNOWN_CODE,
  referenceChatRoute,
} from "./referenceTypes";

export const REFERENCE_CONTROL_ROUTE = "control";
export const REFERENCE_CONTROL_MODEL_ID = "model";
export const REFERENCE_CONTROL_EFFORT_ID = "effort";

export type ReferenceControlApplyKind = "command" | "agent-picker" | "start-only";

export interface ReferenceControlRequestBody {
  readonly requestId: string;
  readonly target: TargetRef;
  readonly registryId: string;
  readonly controlId: string;
  readonly valueId: string;
}

export type ReferenceControlStatus = "applied" | "rejected" | "unknown" | "picker";

export interface ReferenceControlResult {
  readonly status: ReferenceControlStatus;
  readonly requestId: string;
  readonly code?: string;
  readonly message?: string;
}

export interface ReferenceControlPostInit {
  readonly method: "POST";
  readonly body: string;
  readonly signal?: AbortSignal;
}

export type ReferenceControlPoster = (
  path: string,
  init: ReferenceControlPostInit,
) => Promise<{ readonly status: number; json: () => Promise<unknown> }>;

const REJECTED_STATUSES = new Set([400, 401, 403, 404, 405, 409, 422]);
const REJECTED_CODES = new Set([
  "NOT_FOUND",
  "UNSUPPORTED",
  "UNSUPPORTED_CAPABILITY",
  "REJECTED",
  "INVALID_REQUEST",
  "FORBIDDEN",
  "UNAUTHORIZED",
  "REQUEST_CONFLICT",
  "TARGET_EXPIRED",
]);

export function referenceControlPath(sessionId: string): string {
  return referenceChatRoute(sessionId, REFERENCE_CONTROL_ROUTE);
}

/**
 * One apply. `command` POSTs exactly once. `agent-picker` and `start-only` do not
 * touch the network. A thrown post, a timeout, or an ambiguous body is `unknown`.
 */
export async function applyReferenceControl(args: {
  readonly sessionId: string;
  readonly body: ReferenceControlRequestBody;
  readonly apply: ReferenceControlApplyKind;
  readonly signal?: AbortSignal;
  readonly post: ReferenceControlPoster;
}): Promise<ReferenceControlResult> {
  if (args.apply === "agent-picker") {
    return { status: "picker", requestId: args.body.requestId };
  }
  if (args.apply === "start-only") {
    return {
      status: "rejected",
      requestId: args.body.requestId,
      code: "START_ONLY",
      message: "This option applies only when the session starts.",
    };
  }
  const wire: ReferenceControlRequestBody = {
    requestId: args.body.requestId,
    target: args.body.target,
    registryId: args.body.registryId,
    controlId: args.body.controlId,
    valueId: args.body.valueId,
  };
  let response: { readonly status: number; json: () => Promise<unknown> };
  try {
    response = await args.post(referenceControlPath(args.sessionId), {
      method: "POST",
      body: JSON.stringify(wire),
      signal: args.signal,
    });
  } catch (error) {
    return {
      status: "unknown",
      requestId: args.body.requestId,
      code: REFERENCE_OUTCOME_UNKNOWN_CODE,
      message: error instanceof Error ? error.message : "control outcome unknown",
    };
  }
  let parsed: unknown = null;
  try {
    parsed = await response.json();
  } catch {
    parsed = null;
  }
  return classifyReferenceControlResponse(response.status, parsed, args.body.requestId);
}

/** Map one settled HTTP result. Does not POST and does not retry. */
export function classifyReferenceControlResponse(
  status: number,
  body: unknown,
  requestId: string,
): ReferenceControlResult {
  const record = body !== null && typeof body === "object" ? (body as Record<string, unknown>) : null;
  const failure = record === null ? null : failureOf(record);
  if (record?.ok === true) {
    const disposition = dispositionOf(record.data);
    if (disposition === "applied") return { status: "applied", requestId };
    if (disposition === "rejected") {
      return {
        status: "rejected",
        requestId,
        code: failure?.code ?? "REJECTED",
        message: failure?.message ?? "rejected",
      };
    }
    return {
      status: "unknown",
      requestId,
      code: failure?.code ?? REFERENCE_OUTCOME_UNKNOWN_CODE,
      message: failure?.message ?? "control outcome unknown",
    };
  }
  if (failure !== null && REJECTED_CODES.has(failure.code)) {
    return { status: "rejected", requestId, code: failure.code, message: failure.message };
  }
  if (failure !== null && (failure.code === REFERENCE_OUTCOME_UNKNOWN_CODE || failure.code === "TIMEOUT")) {
    return { status: "unknown", requestId, code: failure.code, message: failure.message };
  }
  if (REJECTED_STATUSES.has(status)) {
    return {
      status: "rejected",
      requestId,
      code: failure?.code ?? statusName(status),
      message: failure?.message ?? `control rejected (${status})`,
    };
  }
  return {
    status: "unknown",
    requestId,
    code: failure?.code ?? REFERENCE_OUTCOME_UNKNOWN_CODE,
    message: failure?.message ?? "control outcome unknown",
  };
}

function failureOf(record: Record<string, unknown>): { code: string; message: string } | null {
  const raw = record.error;
  if (typeof raw === "string" && raw.length > 0) return { code: raw, message: raw };
  if (raw === null || typeof raw !== "object") return null;
  const entry = raw as Record<string, unknown>;
  const code = typeof entry.code === "string" && entry.code.length > 0 ? entry.code : "REQUEST_FAILED";
  const message = typeof entry.message === "string" && entry.message.length > 0 ? entry.message : code;
  return { code, message };
}

function dispositionOf(data: unknown): "applied" | "rejected" | "unknown" | null {
  if (data === null || typeof data !== "object") return null;
  const record = data as Record<string, unknown>;
  const outcome = text(record.outcome) ?? text(record.status);
  if (outcome === "applied" || outcome === "accepted") return "applied";
  if (outcome === "rejected" || outcome === "refused") return "rejected";
  if (outcome === "unknown" || outcome === "outcome-unknown" || outcome === REFERENCE_OUTCOME_UNKNOWN_CODE) {
    return "unknown";
  }
  const receipt = record.receipt;
  if (receipt !== null && typeof receipt === "object") {
    const stage = text((receipt as Record<string, unknown>).stage);
    if (stage === "accepted") return "applied";
  }
  return null;
}

function text(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function statusName(status: number): string {
  if (status === 404) return "NOT_FOUND";
  if (status === 422 || status === 405) return "UNSUPPORTED";
  return "REJECTED";
}
