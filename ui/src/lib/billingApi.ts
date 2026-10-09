/**
 * Account billing client for the desktop settings Plan surface
 * (plan `relay-monetization`, todo 11; wire contract fixed by todo 7).
 *
 * Routes (all account-session authenticated):
 *   GET  /api/account/v1/billing/entitlement          -> BillingEntitlement
 *   POST /api/account/v1/billing/checkout             -> { url }
 *   POST /api/account/v1/billing/quantity             -> BillingEntitlement
 *   GET  /api/account/v1/org/members                  -> OrgMember[]
 *   POST /api/account/v1/org/invite                   -> 2xx
 *   POST /api/account/v1/org/members/{userId}/remove  -> 2xx
 *
 * Auth is the canonical account session bearer, resolved through the same
 * helpers the rest of the account surface uses (`getConfiguredAccountOrigin`,
 * `getStoredAccountSessionToken`); no other credential shape is invented here.
 *
 * A self-hosted relay never mounts these routes and answers 404, which callers
 * read as "this deployment has no billing" — not as an error worth surfacing.
 *
 * Errors cross the boundary once, as `BillingApiError` carrying the server's
 * structured `{code, message, details}` envelope; callers branch on `code`,
 * never on message text.
 */
import {
  getConfiguredAccountOrigin,
  getStoredAccountSessionToken,
} from "../remote/accountSession";

export const BILLING_ENTITLEMENT_PATH = "/api/account/v1/billing/entitlement";
export const BILLING_CHECKOUT_PATH = "/api/account/v1/billing/checkout";
export const BILLING_QUANTITY_PATH = "/api/account/v1/billing/quantity";
export const ORG_MEMBERS_PATH = "/api/account/v1/org/members";
export const ORG_INVITE_PATH = "/api/account/v1/org/invite";

export const BILLING_PLANS = [
  "free",
  "pro_monthly",
  "pro_annual",
  "team_monthly",
  "team_annual",
] as const;
export type BillingPlan = (typeof BILLING_PLANS)[number];

export const CHECKOUT_PLANS = [
  "pro_monthly",
  "pro_annual",
  "team_monthly",
  "team_annual",
  "team_hostpack_annual",
] as const;
export type CheckoutPlan = (typeof CHECKOUT_PLANS)[number];

export const ENTITLEMENT_STATUSES = ["ok", "over_limit", "past_due", "stopped"] as const;
export type EntitlementStatus = (typeof ENTITLEMENT_STATUSES)[number];

export const ORG_ROLES = ["owner", "admin", "member"] as const;
export type OrgRole = (typeof ORG_ROLES)[number];

export interface BillingEntitlement {
  readonly plan: BillingPlan;
  readonly status: EntitlementStatus;
  readonly machineLimit: number;
  readonly machinesUsed: number;
  readonly seats: number | null;
  readonly hostPacks: number;
  readonly graceEndsAt: number | null;
  readonly orgId: string | null;
  readonly role: OrgRole | null;
  readonly manageUrl: string | null;
}

export interface OrgMember {
  readonly userId: string;
  readonly email: string;
  readonly role: OrgRole;
}

export interface PlanLimitDetails {
  readonly plan: string | null;
  readonly limit: number | null;
  readonly used: number | null;
}

export interface SuspensionDetails {
  readonly plan: string | null;
  readonly status: string | null;
  readonly graceEndsAt: number | null;
  readonly stoppedAt: number | null;
}

export class BillingApiError extends Error {
  readonly code: string;
  readonly status: number | null;
  readonly details: unknown;

  constructor(code: string, message: string, status: number | null = null, details: unknown = null) {
    super(message);
    this.name = "BillingApiError";
    this.code = code;
    this.status = status;
    this.details = details;
  }
}

export const BILLING_REQUEST_TIMEOUT_MS = 15_000;

export interface BillingCallOptions {
  readonly accountOrigin?: string;
  readonly accountSessionToken?: string | null;
  readonly fetchImpl?: typeof fetch;
  readonly signal?: AbortSignal;
  readonly timeoutMs?: number;
}

interface ResolvedCall {
  readonly base: string;
  readonly sessionToken: string | null;
  readonly fetchImpl: typeof fetch;
  readonly signal: AbortSignal | undefined;
  readonly timeoutMs: number;
}

interface JsonRequest {
  readonly method: "GET" | "POST";
  readonly body?: unknown;
}

function resolveCall(options: BillingCallOptions): ResolvedCall {
  const origin = (options.accountOrigin ?? getConfiguredAccountOrigin()).trim().replace(/\/+$/, "");
  const sessionToken =
    options.accountSessionToken === undefined
      ? getStoredAccountSessionToken(origin)
      : options.accountSessionToken;
  return {
    base: origin,
    sessionToken,
    fetchImpl: options.fetchImpl ?? fetch,
    signal: options.signal,
    timeoutMs: options.timeoutMs ?? BILLING_REQUEST_TIMEOUT_MS,
  };
}

/**
 * Bounded fetch: never follows redirects (a 3xx must not re-send the account bearer
 * to another host) and always carries an abort signal, so a hung account server
 * cannot leave the settings spinner running forever. The caller's signal and the
 * timeout are combined through one controller, which every supported WebView has.
 */
async function boundedFetch(resolved: ResolvedCall, input: string, init: RequestInit): Promise<Response> {
  const controller = new AbortController();
  const callerSignal = resolved.signal;
  const abortFromCaller = () => controller.abort();
  if (callerSignal) {
    if (callerSignal.aborted) controller.abort();
    else callerSignal.addEventListener("abort", abortFromCaller, { once: true });
  }
  let timedOut = false;
  if (controller.signal.aborted) {
    throw new BillingApiError("REQUEST_ABORTED", "The request was cancelled.", null);
  }
  const timeoutId = setTimeout(() => {
    timedOut = true;
    controller.abort();
  }, resolved.timeoutMs);
  try {
    return await resolved.fetchImpl(input, { ...init, redirect: "error", signal: controller.signal });
  } catch (error) {
    const aborted = controller.signal.aborted;
    if (timedOut) {
      throw new BillingApiError(
        "REQUEST_TIMEOUT",
        `The account server did not answer within ${resolved.timeoutMs}ms.`,
        null,
      );
    }
    if (aborted) {
      throw new BillingApiError("REQUEST_ABORTED", "The request was cancelled.", null);
    }
    throw toBillingApiError(error);
  } finally {
    clearTimeout(timeoutId);
    callerSignal?.removeEventListener("abort", abortFromCaller);
  }
}

async function sendRequest(
  resolved: ResolvedCall,
  path: string,
  request: JsonRequest,
): Promise<Response> {
  if (resolved.sessionToken === null) {
    throw new BillingApiError(
      "ACCOUNT_SESSION_REQUIRED",
      "Sign in to the account server to manage the plan.",
      401,
    );
  }
  const headers: Record<string, string> = {
    Authorization: `Bearer ${resolved.sessionToken}`,
  };
  if (request.body !== undefined) {
    headers["Content-Type"] = "application/json";
  }
  return boundedFetch(resolved, `${resolved.base}${path}`, {
    method: request.method,
    headers,
    ...(request.body === undefined ? {} : { body: JSON.stringify(request.body) }),
  });
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function asString(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}

function asNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function asNullableNumber(value: unknown): number | null {
  return value === null || value === undefined ? null : asNumber(value);
}

function parseEnum<T extends string>(values: readonly T[], value: unknown): T | null {
  if (typeof value !== "string") return null;
  for (const candidate of values) {
    if (candidate === value) return candidate;
  }
  return null;
}

async function readJsonBody(response: Response): Promise<unknown> {
  try {
    return await response.json();
  } catch (error) {
    // A missing or non-JSON body is a null payload, not a transport failure.
    if (error instanceof SyntaxError) return null;
    throw error;
  }
}

function fallbackCodeForStatus(status: number): string {
  if (status === 401) return "UNAUTHORIZED";
  if (status === 402) return "PAYMENT_REQUIRED";
  if (status === 403) return "FORBIDDEN";
  if (status === 404) return "NOT_FOUND";
  if (status === 503) return "BILLING_UNCONFIGURED";
  return "BILLING_REQUEST_FAILED";
}

async function requireOk(response: Response): Promise<void> {
  if (response.ok) return;
  const payload = await readJsonBody(response);
  const envelope = isRecord(payload) ? payload : null;
  const code = (envelope ? asString(envelope.code) : null) ?? fallbackCodeForStatus(response.status);
  const message =
    (envelope ? asString(envelope.message) : null) ?? `Request failed (${response.status})`;
  throw new BillingApiError(code, message, response.status, envelope?.details ?? null);
}

function invalidResponse(what: string): BillingApiError {
  return new BillingApiError("INVALID_RESPONSE", `Malformed ${what} response from the account server.`, null);
}

function parseEntitlement(payload: unknown): BillingEntitlement {
  if (!isRecord(payload)) throw invalidResponse("entitlement");
  const plan = parseEnum(BILLING_PLANS, payload.plan);
  const status = parseEnum(ENTITLEMENT_STATUSES, payload.status);
  const machineLimit = asNumber(payload.machineLimit);
  const machinesUsed = asNumber(payload.machinesUsed);
  const hostPacks = asNumber(payload.hostPacks);
  if (
    plan === null ||
    status === null ||
    machineLimit === null ||
    machinesUsed === null ||
    hostPacks === null
  ) {
    throw invalidResponse("entitlement");
  }
  return {
    plan,
    status,
    machineLimit,
    machinesUsed,
    hostPacks,
    seats: asNullableNumber(payload.seats),
    graceEndsAt: asNullableNumber(payload.graceEndsAt),
    orgId: asString(payload.orgId),
    role: parseEnum(ORG_ROLES, payload.role),
    manageUrl: asString(payload.manageUrl),
  };
}

function parseOrgMembers(payload: unknown): readonly OrgMember[] {
  const list = Array.isArray(payload)
    ? payload
    : isRecord(payload) && Array.isArray(payload.members)
      ? payload.members
      : null;
  if (list === null) throw invalidResponse("org members");
  const members: OrgMember[] = [];
  for (const entry of list) {
    if (!isRecord(entry)) continue;
    const userId = asString(entry.userId);
    const role = parseEnum(ORG_ROLES, entry.role);
    if (userId === null || role === null) continue;
    members.push({ userId, email: asString(entry.email) ?? userId, role });
  }
  return members;
}

export function toBillingApiError(error: unknown): BillingApiError {
  if (error instanceof BillingApiError) return error;
  if (error instanceof Error) {
    return new BillingApiError("NETWORK_FAILED", error.message, null);
  }
  return new BillingApiError("NETWORK_FAILED", "Failed to reach the account server.", null);
}

export function planLimitDetails(details: unknown): PlanLimitDetails | null {
  if (!isRecord(details)) return null;
  const plan = asString(details.plan);
  const limit = asNumber(details.limit);
  const used = asNumber(details.used);
  if (plan === null && limit === null && used === null) return null;
  return { plan, limit, used };
}

export function suspensionDetails(details: unknown): SuspensionDetails | null {
  if (!isRecord(details)) return null;
  const plan = asString(details.plan);
  const status = asString(details.status);
  const graceEndsAt = asNumber(details.graceEndsAt);
  const stoppedAt = asNumber(details.stoppedAt);
  if (plan === null && status === null && graceEndsAt === null && stoppedAt === null) return null;
  return { plan, status, graceEndsAt, stoppedAt };
}

export async function fetchBillingEntitlement(
  options: BillingCallOptions = {},
): Promise<BillingEntitlement> {
  const response = await sendRequest(resolveCall(options), BILLING_ENTITLEMENT_PATH, {
    method: "GET",
  });
  await requireOk(response);
  return parseEntitlement(await readJsonBody(response));
}

export interface BillingCheckoutOptions extends BillingCallOptions {
  readonly seats?: number;
  readonly hostPacks?: number;
}

export async function startBillingCheckout(
  plan: CheckoutPlan,
  options: BillingCheckoutOptions = {},
): Promise<string> {
  const body: Record<string, unknown> = { plan };
  if (options.seats !== undefined) body.seats = options.seats;
  if (options.hostPacks !== undefined) body.hostPacks = options.hostPacks;
  const response = await sendRequest(resolveCall(options), BILLING_CHECKOUT_PATH, {
    method: "POST",
    body,
  });
  await requireOk(response);
  const payload = await readJsonBody(response);
  const url = isRecord(payload) ? asString(payload.url) : null;
  if (url === null) throw invalidResponse("checkout");
  return url;
}

export interface BillingQuantityChange {
  readonly seats?: number;
  readonly hostPacks?: number;
}

export async function updateBillingQuantity(
  change: BillingQuantityChange,
  options: BillingCallOptions = {},
): Promise<BillingEntitlement> {
  const body: Record<string, unknown> = {};
  if (change.seats !== undefined) body.seats = change.seats;
  if (change.hostPacks !== undefined) body.hostPacks = change.hostPacks;
  const response = await sendRequest(resolveCall(options), BILLING_QUANTITY_PATH, {
    method: "POST",
    body,
  });
  await requireOk(response);
  return parseEntitlement(await readJsonBody(response));
}

export async function listOrgMembers(
  options: BillingCallOptions = {},
): Promise<readonly OrgMember[]> {
  const response = await sendRequest(resolveCall(options), ORG_MEMBERS_PATH, { method: "GET" });
  await requireOk(response);
  return parseOrgMembers(await readJsonBody(response));
}

export interface OrgInvite {
  readonly email: string;
  readonly expiresAt: number;
}

export async function inviteOrgMember(
  email: string,
  options: BillingCallOptions = {},
): Promise<OrgInvite> {
  const response = await sendRequest(resolveCall(options), ORG_INVITE_PATH, {
    method: "POST",
    body: { email: email.trim() },
  });
  await requireOk(response);
  const payload = await readJsonBody(response);
  const invitedEmail = isRecord(payload) ? asString(payload.email) : null;
  const expiresAt = isRecord(payload) ? asNumber(payload.expiresAt) : null;
  if (invitedEmail === null || expiresAt === null) throw invalidResponse("org invite");
  return { email: invitedEmail, expiresAt };
}

export async function removeOrgMember(
  userId: string,
  options: BillingCallOptions = {},
): Promise<void> {
  const response = await sendRequest(
    resolveCall(options),
    `${ORG_MEMBERS_PATH}/${encodeURIComponent(userId)}/remove`,
    { method: "POST" },
  );
  await requireOk(response);
}
